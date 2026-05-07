//! Unquoted-word scanner — collects a word from `pos` until a word break,
//! classifies it (keyword / command / argument / variable / etc.), and
//! emits a span. The only scanner that needs `CommandChecker` access
//! to determine command existence.

use super::super::command_checker::CheckerEnv;
use super::super::highlight::{ColorSpan, HighlightStyle};
use super::ctx::ScanCtx;
use super::helpers::{
    COMMAND_POSITION_KEYWORDS, is_keyword, is_redirect_start, is_valid_name, is_word_break,
};
use crate::interactive::command_checker::CommandExistence;

pub(super) fn scan_word(ctx: &mut ScanCtx<'_>, env: &CheckerEnv<'_>, pos: usize) -> usize {
    let start = pos;
    let mut end = pos;
    while end < ctx.input.len() && !is_word_break(ctx.input[end]) {
        end += 1;
    }

    if end == start {
        // Safety: if nothing consumed, advance by one to avoid infinite loop.
        ctx.state.word_start = false;
        return pos + 1;
    }

    let word: String = ctx.input[start..end].iter().collect();

    // --- Check for assignment (VAR=value) in command position ---
    if ctx.state.command_position
        && let Some(eq_idx) = word.find('=')
    {
        let name_part = &word[..eq_idx];
        if !name_part.is_empty() && is_valid_name(name_part) {
            // It's an assignment prefix. The part before = (inclusive) is
            // Assignment; the part after is Default.
            let eq_char_pos = start + eq_idx;
            ctx.spans.push(ColorSpan {
                start,
                end: eq_char_pos + 1,
                style: HighlightStyle::Assignment,
            });
            if eq_char_pos + 1 < end {
                ctx.spans.push(ColorSpan {
                    start: eq_char_pos + 1,
                    end,
                    style: HighlightStyle::Default,
                });
            }
            // command_position stays true after an assignment prefix
            ctx.state.word_start = true;
            return end;
        }
    }

    // --- IO number: all digits followed by redirect ---
    if word.chars().all(|c| c.is_ascii_digit())
        && end < ctx.input.len()
        && is_redirect_start(ctx.input[end])
    {
        ctx.spans.push(ColorSpan {
            start,
            end,
            style: HighlightStyle::IoNumber,
        });
        ctx.state.word_start = false;
        // command_position unchanged
        return end;
    }

    // --- Command position: keyword or command check ---
    if ctx.state.command_position {
        if is_keyword(&word) {
            ctx.spans.push(ColorSpan {
                start,
                end,
                style: HighlightStyle::Keyword,
            });
            // After a keyword, next word is generally in command position
            // (e.g., `if cmd`, `while cmd`). Some keywords end command
            // position (`fi`, `done`, `esac`, `}`), but those are followed
            // by operators anyway. We set command_position based on whether
            // this is a COMMAND_POSITION_KEYWORDS keyword.
            ctx.state.command_position = COMMAND_POSITION_KEYWORDS.contains(&word.as_str());
            // Keywords like "fi", "done" etc. act like statement terminators —
            // what follows is likely an operator, so command_position stays false
            // until an operator resets it. But for safety, keywords like "if",
            // "while", "for", "case", "until", "{" do put us in command position.
            if matches!(
                word.as_str(),
                "if" | "while" | "until" | "for" | "case" | "{" | "in"
            ) {
                ctx.state.command_position = true;
            }
            ctx.state.word_start = true;
            return end;
        }

        // Check command existence
        let existence = ctx.checker.check(&word, env);
        let style = match existence {
            CommandExistence::Valid => HighlightStyle::CommandValid,
            CommandExistence::Invalid => HighlightStyle::CommandInvalid,
        };
        ctx.spans.push(ColorSpan { start, end, style });
        ctx.state.command_position = false;
        ctx.state.word_start = true;
        return end;
    }

    // --- Default (argument) ---
    ctx.spans.push(ColorSpan {
        start,
        end,
        style: HighlightStyle::Default,
    });
    ctx.state.word_start = true;
    ctx.state.command_position = false;
    end
}
