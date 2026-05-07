//! Comment scanner — handles a `#`-comment that starts at the beginning
//! of a word and runs to the end of the input.

use super::super::highlight::ColorSpan;
use super::super::highlight::HighlightStyle;
use super::state::ScannerState;

pub(super) fn scan_comment(
    chars: &[char],
    _pos: usize,
    start: usize,
    state: &mut ScannerState,
    spans: &mut Vec<ColorSpan>,
) -> usize {
    // Comment spans to the end of the input.
    spans.push(ColorSpan {
        start,
        end: chars.len(),
        style: HighlightStyle::Comment,
    });
    state.pop_mode();
    chars.len()
}
