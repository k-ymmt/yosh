//! Top-level scanner for normal (unquoted, unstacked) shell-syntax mode.
//!
//! Handles whitespace, operators (`|`, `&&`, `||`, `;`, `&`), redirects
//! (`<`, `>`, `>>`, etc.), opening of quotes/expansions/comments
//! (delegates by pushing onto state.mode_stack), and falls through to
//! scan_word for unquoted words and scan_dollar for `$` expansions.

use super::super::command_checker::CheckerEnv;
use super::super::highlight::{ColorSpan, HighlightStyle};
use super::ctx::ScanCtx;
use super::expansion;
use super::helpers::is_operator_char;
use super::helpers::is_redirect_start;
use super::state::ScanMode;
use super::word;

pub(super) fn scan_normal(ctx: &mut ScanCtx<'_>, env: &CheckerEnv<'_>, pos: usize) -> usize {
    if pos >= ctx.input.len() {
        return pos;
    }

    let ch = ctx.input[pos];

    // --- Whitespace ---
    if ch.is_ascii_whitespace() {
        // A newline separates commands like `;`: the first word of the next
        // line (in-editor multiline buffer or accumulated PS2 text) is at
        // command position again — unless a here-doc redirection is pending,
        // in which case the following lines are its body, not commands.
        if ch == '\n' {
            if !ctx.state.pending_heredocs.is_empty() {
                ctx.state.push_mode(ScanMode::HereDoc);
                ctx.state.word_start = true;
                return pos + 1;
            }
            ctx.state.command_position = true;
        }
        ctx.state.word_start = true;
        return pos + 1;
    }

    // --- Comment ---
    if ch == '#' && ctx.state.word_start {
        ctx.state.push_mode(ScanMode::Comment { start: pos });
        return pos;
    }

    // --- Operators: | & ; ---
    if is_operator_char(ch) {
        let start = pos;
        let mut end = pos + 1;

        if ch == '|' && end < ctx.input.len() && ctx.input[end] == '|' {
            end += 1; // ||
        } else if ch == '&' && end < ctx.input.len() && ctx.input[end] == '&' {
            end += 1; // &&
        } else if ch == ';' && end < ctx.input.len() && ctx.input[end] == ';' {
            end += 1; // ;;
        }

        ctx.spans.push(ColorSpan {
            start,
            end,
            style: HighlightStyle::Operator,
        });
        ctx.state.command_position = true;
        ctx.state.word_start = true;
        return end;
    }

    // --- Redirects: < > ---
    if is_redirect_start(ch) {
        let start = pos;
        let mut end = pos + 1;

        if ch == '>' && end < ctx.input.len() {
            match ctx.input[end] {
                '>' | '|' | '&' => end += 1,
                _ => {}
            }
        } else if ch == '<' && end < ctx.input.len() {
            match ctx.input[end] {
                '<' | '&' | '>' => end += 1,
                _ => {}
            }
            // <<- (here-doc strip)
            if end == start + 2
                && ctx.input[start + 1] == '<'
                && end < ctx.input.len()
                && ctx.input[end] == '-'
            {
                end += 1;
            }
        }

        ctx.spans.push(ColorSpan {
            start,
            end,
            style: HighlightStyle::Redirect,
        });

        // `<<WORD` / `<<-WORD`: here-document. Capture the delimiter here
        // (in one scan step, so incremental checkpoints never land inside
        // it) and remember it; the body lines that follow the next newline
        // are scanned by ScanMode::HereDoc instead of as commands.
        let is_heredoc = ch == '<'
            && end >= start + 2
            && ctx.input[start + 1] == '<'
            && (end == start + 2 || ctx.input[start + 2] == '-');
        if is_heredoc {
            let strip_tabs = end == start + 3;
            let mut p = end;
            while p < ctx.input.len() && matches!(ctx.input[p], ' ' | '\t') {
                p += 1;
            }
            let delim_start = p;
            let mut delim = String::new();
            if p < ctx.input.len() && matches!(ctx.input[p], '\'' | '"') {
                let quote = ctx.input[p];
                p += 1;
                while p < ctx.input.len() && ctx.input[p] != quote && ctx.input[p] != '\n' {
                    delim.push(ctx.input[p]);
                    p += 1;
                }
                if p < ctx.input.len() && ctx.input[p] == quote {
                    p += 1;
                }
            } else {
                while p < ctx.input.len() && !is_heredoc_delim_end(ctx.input[p]) {
                    delim.push(ctx.input[p]);
                    p += 1;
                }
            }
            if !delim.is_empty() {
                ctx.spans.push(ColorSpan {
                    start: delim_start,
                    end: p,
                    style: HighlightStyle::String,
                });
                ctx.state.pending_heredocs.push((delim, strip_tabs));
            }
            ctx.state.command_position = false;
            ctx.state.word_start = true;
            return p;
        }

        // After a redirect the next token is a filename, not a command
        ctx.state.command_position = false;
        ctx.state.word_start = true;
        return end;
    }

    // --- Parentheses ---
    if ch == '(' {
        ctx.spans.push(ColorSpan {
            start: pos,
            end: pos + 1,
            style: HighlightStyle::Operator,
        });
        ctx.state.command_position = true;
        ctx.state.word_start = true;
        return pos + 1;
    }

    if ch == ')' {
        // Check if we are closing a CommandSub: the stack would be
        // [..., CommandSub, Normal] and current mode is Normal.
        let stack_len = ctx.state.mode_stack.len();
        if stack_len >= 2
            && let ScanMode::CommandSub { start } = ctx.state.mode_stack[stack_len - 2]
        {
            // Pop Normal, then pop CommandSub
            ctx.state.pop_mode(); // pops Normal
            ctx.spans.push(ColorSpan {
                start,
                end: pos + 1,
                style: HighlightStyle::CommandSub,
            });
            ctx.state.pop_mode(); // pops CommandSub
            ctx.state.word_start = false;
            ctx.state.command_position = false;
            return pos + 1;
        }

        // Otherwise, plain operator (subshell close, etc.)
        ctx.spans.push(ColorSpan {
            start: pos,
            end: pos + 1,
            style: HighlightStyle::Operator,
        });
        ctx.state.command_position = false;
        ctx.state.word_start = true;
        return pos + 1;
    }

    // --- Quotes ---
    if ch == '\'' {
        ctx.state.push_mode(ScanMode::SingleQuote { start: pos });
        ctx.state.word_start = false;
        ctx.state.command_position = false;
        return pos + 1; // skip opening quote, scan_single_quote takes over
    }

    if ch == '"' {
        ctx.state.push_mode(ScanMode::DoubleQuote { start: pos });
        ctx.state.word_start = false;
        ctx.state.command_position = false;
        return pos + 1;
    }

    // --- Backtick ---
    if ch == '`' {
        let stack_len = ctx.state.mode_stack.len();
        if stack_len >= 2
            && let ScanMode::Backtick { start } = ctx.state.mode_stack[stack_len - 2]
        {
            // Closing backtick
            ctx.state.pop_mode(); // pops Normal
            ctx.spans.push(ColorSpan {
                start,
                end: pos + 1,
                style: HighlightStyle::CommandSub,
            });
            ctx.state.pop_mode(); // pops Backtick
            ctx.state.word_start = false;
            ctx.state.command_position = false;
            return pos + 1;
        }
        // Opening backtick — push Backtick then Normal
        ctx.state.push_mode(ScanMode::Backtick { start: pos });
        ctx.state.push_mode(ScanMode::Normal);
        ctx.state.word_start = true;
        ctx.state.command_position = true;
        return pos + 1;
    }

    // --- Dollar expansions ---
    if ch == '$' {
        return expansion::scan_dollar(ctx, env, pos);
    }

    // --- Tilde ---
    if ch == '~' && ctx.state.word_start {
        ctx.spans.push(ColorSpan {
            start: pos,
            end: pos + 1,
            style: HighlightStyle::Tilde,
        });
        ctx.state.word_start = false;
        // Tilde doesn't change command_position by itself — it's part of a word.
        return pos + 1;
    }

    // --- Regular word ---
    word::scan_word(ctx, env, pos)
}

/// Characters that end an unquoted here-doc delimiter word.
fn is_heredoc_delim_end(ch: char) -> bool {
    ch.is_ascii_whitespace() || matches!(ch, '|' | '&' | ';' | '<' | '>' | '(' | ')')
}
