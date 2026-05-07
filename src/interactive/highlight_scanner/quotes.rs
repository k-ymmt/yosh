//! Quoted-string scanners: single-quote, dollar-single-quote.
//! scan_double_quote arrives in Task B3 (it's currently &mut self in mod.rs).
//!
//! Each scanner is a free function. Takes the input slice, current pos,
//! the variant payload (`start` of the opening quote), shared state,
//! and the span accumulator.

use super::super::highlight::ColorSpan;
use super::super::highlight::HighlightStyle;
use super::state::ScannerState;

pub(super) fn scan_single_quote(
    chars: &[char],
    pos: usize,
    start: usize,
    state: &mut ScannerState,
    spans: &mut Vec<ColorSpan>,
) -> usize {
    let mut p = pos;
    while p < chars.len() {
        if chars[p] == '\'' {
            spans.push(ColorSpan {
                start,
                end: p + 1,
                style: HighlightStyle::String,
            });
            state.pop_mode();
            return p + 1;
        }
        p += 1;
    }
    // Unclosed — mark_unclosed_errors will handle it
    p
}

pub(super) fn scan_dollar_single_quote(
    chars: &[char],
    pos: usize,
    start: usize,
    state: &mut ScannerState,
    spans: &mut Vec<ColorSpan>,
) -> usize {
    let mut p = pos;
    while p < chars.len() {
        if chars[p] == '\\' {
            // escape: skip next
            p += 1;
            if p < chars.len() {
                p += 1;
            }
            continue;
        }
        if chars[p] == '\'' {
            spans.push(ColorSpan {
                start,
                end: p + 1,
                style: HighlightStyle::String,
            });
            state.pop_mode();
            return p + 1;
        }
        p += 1;
    }
    // Unclosed
    p
}
