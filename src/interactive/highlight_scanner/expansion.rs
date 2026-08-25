//! Dollar-expansion scanners: $variable, ${braced}, $((arith)).
//!
//! Each scanner is a free function taking (ctx, env, pos, [payload]).

use super::super::command_checker::CheckerEnv;
use super::super::highlight::{ColorSpan, HighlightStyle};
use super::ctx::ScanCtx;
use super::state::ScanMode;

// -----------------------------------------------------------------------
// scan_parameter (braced)
// -----------------------------------------------------------------------

pub(super) fn scan_parameter(
    ctx: &mut ScanCtx<'_>,
    _env: &CheckerEnv<'_>,
    pos: usize,
    start: usize,
    _braced: bool,
) -> usize {
    let mut p = pos;
    while p < ctx.input.len() {
        if ctx.input[p] == '}' {
            ctx.spans.push(ColorSpan {
                start,
                end: p + 1,
                style: HighlightStyle::Variable,
            });
            ctx.state.pop_mode();
            return p + 1;
        }
        p += 1;
    }
    // Unclosed
    p
}

// -----------------------------------------------------------------------
// scan_dollar_name – shared $NAME / special-parameter scan
// -----------------------------------------------------------------------

/// When the character after the `$` at `pos` starts a plain name
/// (`$NAME`) or is a positional/special parameter (`$0`..`$9`, `$@`,
/// `$*`, `$#`, `$?`, `$-`, `$$`, `$!`), push a Variable span and return
/// the position after it. Returns `None` for everything else (`$'`,
/// `$(`, `${`, bare `$`) — those cases are dispatched by the caller,
/// whose handling differs between normal mode (mode stack) and the
/// inline double-quote scanner.
pub(super) fn scan_dollar_name(ctx: &mut ScanCtx<'_>, pos: usize) -> Option<usize> {
    let next = *ctx.input.get(pos + 1)?;
    if next.is_ascii_alphabetic() || next == '_' {
        // $NAME
        let mut end = pos + 1;
        while end < ctx.input.len()
            && (ctx.input[end].is_ascii_alphanumeric() || ctx.input[end] == '_')
        {
            end += 1;
        }
        ctx.spans.push(ColorSpan {
            start: pos,
            end,
            style: HighlightStyle::Variable,
        });
        Some(end)
    } else if next.is_ascii_digit() || matches!(next, '@' | '*' | '#' | '?' | '-' | '$' | '!') {
        ctx.spans.push(ColorSpan {
            start: pos,
            end: pos + 2,
            style: HighlightStyle::Variable,
        });
        Some(pos + 2)
    } else {
        None
    }
}

// -----------------------------------------------------------------------
// scan_dollar – handle $... in Normal mode
// -----------------------------------------------------------------------

pub(super) fn scan_dollar(ctx: &mut ScanCtx<'_>, _env: &CheckerEnv<'_>, pos: usize) -> usize {
    let next = if pos + 1 < ctx.input.len() {
        Some(ctx.input[pos + 1])
    } else {
        None
    };

    match next {
        Some('\'') => {
            // $'...' — ANSI-C quoting
            ctx.state
                .push_mode(ScanMode::DollarSingleQuote { start: pos });
            ctx.state.word_start = false;
            ctx.state.command_position = false;
            pos + 2 // skip $'
        }
        Some('(') => {
            // Check for $(( — arithmetic
            if pos + 2 < ctx.input.len() && ctx.input[pos + 2] == '(' {
                ctx.state.push_mode(ScanMode::ArithSub { start: pos });
                ctx.state.word_start = false;
                ctx.state.command_position = false;
                pos + 3 // skip $((
            } else {
                // $( — command substitution
                ctx.state.push_mode(ScanMode::CommandSub { start: pos });
                ctx.state.push_mode(ScanMode::Normal);
                ctx.state.word_start = true;
                ctx.state.command_position = true;
                pos + 2 // skip $(
            }
        }
        Some('{') => {
            ctx.state.push_mode(ScanMode::Parameter {
                start: pos,
                braced: true,
            });
            ctx.state.word_start = false;
            ctx.state.command_position = false;
            pos + 2 // skip ${
        }
        _ => match scan_dollar_name(ctx, pos) {
            Some(end) => {
                // $NAME or a positional/special parameter.
                ctx.state.word_start = false;
                ctx.state.command_position = false;
                end
            }
            None => {
                // Bare $ at end of input or before something unexpected –
                // treat as default text.
                ctx.state.word_start = false;
                pos + 1
            }
        },
    }
}

// -----------------------------------------------------------------------
// scan_arith_sub
// -----------------------------------------------------------------------

pub(super) fn scan_arith_sub(
    ctx: &mut ScanCtx<'_>,
    _env: &CheckerEnv<'_>,
    pos: usize,
    start: usize,
) -> usize {
    let mut p = pos;
    while p + 1 < ctx.input.len() {
        if ctx.input[p] == ')' && ctx.input[p + 1] == ')' {
            ctx.spans.push(ColorSpan {
                start,
                end: p + 2,
                style: HighlightStyle::ArithSub,
            });
            ctx.state.pop_mode();
            return p + 2;
        }
        p += 1;
    }
    // Advance to end if unclosed
    ctx.input.len()
}
