//! Top-level scanner for normal (unquoted, unstacked) shell-syntax mode.
//!
//! Handles whitespace, operators (`|`, `&&`, `||`, `;`, `&`), redirects
//! (`<`, `>`, `>>`, etc.), opening of quotes/expansions/comments
//! (delegates by pushing onto state.mode_stack), and falls through to
//! scan_word for unquoted words and scan_dollar for `$` expansions.

use super::super::command_checker::{CheckerEnv, CommandChecker};
use super::expansion;
use super::helpers::is_operator_char;
use super::helpers::is_redirect_start;
use super::super::highlight::{ColorSpan, HighlightStyle};
use super::state::{ScanMode, ScannerState};
use super::word;

pub(super) fn scan_normal(
    chars: &[char],
    pos: usize,
    state: &mut ScannerState,
    spans: &mut Vec<ColorSpan>,
    checker: &mut CommandChecker,
    checker_env: &CheckerEnv,
) -> usize {
    if pos >= chars.len() {
        return pos;
    }

    let ch = chars[pos];

    // --- Whitespace ---
    if ch.is_ascii_whitespace() {
        state.word_start = true;
        return pos + 1;
    }

    // --- Comment ---
    if ch == '#' && state.word_start {
        state.push_mode(ScanMode::Comment { start: pos });
        return pos;
    }

    // --- Operators: | & ; ---
    if is_operator_char(ch) {
        let start = pos;
        let mut end = pos + 1;

        if ch == '|' && end < chars.len() && chars[end] == '|' {
            end += 1; // ||
        } else if ch == '&' && end < chars.len() && chars[end] == '&' {
            end += 1; // &&
        } else if ch == ';' && end < chars.len() && chars[end] == ';' {
            end += 1; // ;;
        }

        spans.push(ColorSpan {
            start,
            end,
            style: HighlightStyle::Operator,
        });
        state.command_position = true;
        state.word_start = true;
        return end;
    }

    // --- Redirects: < > ---
    if is_redirect_start(ch) {
        let start = pos;
        let mut end = pos + 1;

        if ch == '>' && end < chars.len() {
            match chars[end] {
                '>' | '|' | '&' => end += 1,
                _ => {}
            }
        } else if ch == '<' && end < chars.len() {
            match chars[end] {
                '<' | '&' | '>' => end += 1,
                _ => {}
            }
            // <<- (here-doc strip)
            if end == start + 2
                && chars[start + 1] == '<'
                && end < chars.len()
                && chars[end] == '-'
            {
                end += 1;
            }
        }

        spans.push(ColorSpan {
            start,
            end,
            style: HighlightStyle::Redirect,
        });
        // After a redirect the next token is a filename, not a command
        state.command_position = false;
        state.word_start = true;
        return end;
    }

    // --- Parentheses ---
    if ch == '(' {
        spans.push(ColorSpan {
            start: pos,
            end: pos + 1,
            style: HighlightStyle::Operator,
        });
        state.command_position = true;
        state.word_start = true;
        return pos + 1;
    }

    if ch == ')' {
        // Check if we are closing a CommandSub: the stack would be
        // [..., CommandSub, Normal] and current mode is Normal.
        let stack_len = state.mode_stack.len();
        if stack_len >= 2
            && let ScanMode::CommandSub { start } = state.mode_stack[stack_len - 2]
        {
            // Pop Normal, then pop CommandSub
            state.pop_mode(); // pops Normal
            spans.push(ColorSpan {
                start,
                end: pos + 1,
                style: HighlightStyle::CommandSub,
            });
            state.pop_mode(); // pops CommandSub
            state.word_start = false;
            state.command_position = false;
            return pos + 1;
        }

        // Otherwise, plain operator (subshell close, etc.)
        spans.push(ColorSpan {
            start: pos,
            end: pos + 1,
            style: HighlightStyle::Operator,
        });
        state.command_position = false;
        state.word_start = true;
        return pos + 1;
    }

    // --- Quotes ---
    if ch == '\'' {
        state.push_mode(ScanMode::SingleQuote { start: pos });
        state.word_start = false;
        state.command_position = false;
        return pos + 1; // skip opening quote, scan_single_quote takes over
    }

    if ch == '"' {
        state.push_mode(ScanMode::DoubleQuote { start: pos });
        state.word_start = false;
        state.command_position = false;
        return pos + 1;
    }

    // --- Backtick ---
    if ch == '`' {
        let stack_len = state.mode_stack.len();
        if stack_len >= 2
            && let ScanMode::Backtick { start } = state.mode_stack[stack_len - 2]
        {
            // Closing backtick
            state.pop_mode(); // pops Normal
            spans.push(ColorSpan {
                start,
                end: pos + 1,
                style: HighlightStyle::CommandSub,
            });
            state.pop_mode(); // pops Backtick
            state.word_start = false;
            state.command_position = false;
            return pos + 1;
        }
        // Opening backtick — push Backtick then Normal
        state.push_mode(ScanMode::Backtick { start: pos });
        state.push_mode(ScanMode::Normal);
        state.word_start = true;
        state.command_position = true;
        return pos + 1;
    }

    // --- Dollar expansions ---
    if ch == '$' {
        return expansion::scan_dollar(chars, pos, state, spans, checker_env);
    }

    // --- Tilde ---
    if ch == '~' && state.word_start {
        spans.push(ColorSpan {
            start: pos,
            end: pos + 1,
            style: HighlightStyle::Tilde,
        });
        state.word_start = false;
        // Tilde doesn't change command_position by itself — it's part of a word.
        return pos + 1;
    }

    // --- Regular word ---
    word::scan_word(chars, pos, state, spans, checker, checker_env)
}
