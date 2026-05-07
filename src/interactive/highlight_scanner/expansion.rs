//! Dollar-expansion scanners: $variable, ${braced}, $((arith)).
//!
//! Each scanner is a free function. scan_dollar (top-level $-detector
//! that branches into the others) lands here in Task B3.

use super::super::highlight::{ColorSpan, HighlightStyle};
use super::state::{ScanMode, ScannerState};

// -----------------------------------------------------------------------
// scan_parameter (braced)
// -----------------------------------------------------------------------

pub(super) fn scan_parameter(
    chars: &[char],
    pos: usize,
    start: usize,
    _braced: bool,
    state: &mut ScannerState,
    spans: &mut Vec<ColorSpan>,
) -> usize {
    let mut p = pos;
    while p < chars.len() {
        if chars[p] == '}' {
            spans.push(ColorSpan {
                start,
                end: p + 1,
                style: HighlightStyle::Variable,
            });
            state.pop_mode();
            return p + 1;
        }
        p += 1;
    }
    // Unclosed
    p
}

// scan_dollar will land here in Task B3.

// -----------------------------------------------------------------------
// scan_arith_sub
// -----------------------------------------------------------------------

pub(super) fn scan_arith_sub(
    chars: &[char],
    pos: usize,
    start: usize,
    state: &mut ScannerState,
    spans: &mut Vec<ColorSpan>,
) -> usize {
    let mut p = pos;
    while p + 1 < chars.len() {
        if chars[p] == ')' && chars[p + 1] == ')' {
            spans.push(ColorSpan {
                start,
                end: p + 2,
                style: HighlightStyle::ArithSub,
            });
            state.pop_mode();
            return p + 2;
        }
        p += 1;
    }
    // Advance to end if unclosed
    chars.len()
}
