//! Comment scanner — handles a `#`-comment that starts at the beginning
//! of a word and runs to the end of the input.

use super::super::command_checker::CheckerEnv;
use super::super::highlight::{ColorSpan, HighlightStyle};
use super::ctx::ScanCtx;

pub(super) fn scan_comment(
    ctx: &mut ScanCtx<'_>,
    _env: &CheckerEnv<'_>,
    _pos: usize,
    start: usize,
) -> usize {
    // Comment spans to the end of the input.
    ctx.spans.push(ColorSpan {
        start,
        end: ctx.input.len(),
        style: HighlightStyle::Comment,
    });
    ctx.state.pop_mode();
    ctx.input.len()
}
