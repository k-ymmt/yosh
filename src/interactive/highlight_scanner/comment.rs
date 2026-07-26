//! Comment scanner — handles a `#`-comment that starts at the beginning
//! of a word and runs to the end of the line.

use super::super::command_checker::CheckerEnv;
use super::super::highlight::{ColorSpan, HighlightStyle};
use super::ctx::ScanCtx;

pub(super) fn scan_comment(
    ctx: &mut ScanCtx<'_>,
    _env: &CheckerEnv<'_>,
    _pos: usize,
    start: usize,
) -> usize {
    // Comment spans to the end of the line: a multiline buffer's following
    // lines are commands again. The newline itself is handed back to
    // scan_normal so command position is restored there.
    let end = ctx.input[start..]
        .iter()
        .position(|&c| c == '\n')
        .map(|i| start + i)
        .unwrap_or(ctx.input.len());
    ctx.spans.push(ColorSpan {
        start,
        end,
        style: HighlightStyle::Comment,
    });
    ctx.state.pop_mode();
    end
}
