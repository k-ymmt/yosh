//! Dollar-expansion scanners: $variable, ${braced}, $((arith)).
//!
//! Each scanner is a free function. scan_dollar (top-level $-detector
//! that branches into the others) lands here in Task B3.

use super::super::command_checker::CheckerEnv;
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

// -----------------------------------------------------------------------
// scan_dollar – handle $... in Normal mode
// -----------------------------------------------------------------------

pub(super) fn scan_dollar(
    chars: &[char],
    pos: usize,
    state: &mut ScannerState,
    spans: &mut Vec<ColorSpan>,
    _checker_env: &CheckerEnv,
) -> usize {
    let next = if pos + 1 < chars.len() {
        Some(chars[pos + 1])
    } else {
        None
    };

    match next {
        Some('\'') => {
            // $'...' — ANSI-C quoting
            state.push_mode(ScanMode::DollarSingleQuote { start: pos });
            state.word_start = false;
            state.command_position = false;
            pos + 2 // skip $'
        }
        Some('(') => {
            // Check for $(( — arithmetic
            if pos + 2 < chars.len() && chars[pos + 2] == '(' {
                state.push_mode(ScanMode::ArithSub { start: pos });
                state.word_start = false;
                state.command_position = false;
                pos + 3 // skip $((
            } else {
                // $( — command substitution
                state.push_mode(ScanMode::CommandSub { start: pos });
                state.push_mode(ScanMode::Normal);
                state.word_start = true;
                state.command_position = true;
                pos + 2 // skip $(
            }
        }
        Some('{') => {
            state.push_mode(ScanMode::Parameter {
                start: pos,
                braced: true,
            });
            state.word_start = false;
            state.command_position = false;
            pos + 2 // skip ${
        }
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {
            // $NAME
            let var_start = pos;
            let mut end = pos + 1;
            while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_')
            {
                end += 1;
            }
            spans.push(ColorSpan {
                start: var_start,
                end,
                style: HighlightStyle::Variable,
            });
            state.word_start = false;
            state.command_position = false;
            end
        }
        Some(c)
            if c.is_ascii_digit() || matches!(c, '@' | '*' | '#' | '?' | '-' | '$' | '!') =>
        {
            // $0 .. $9, $@, $*, $#, $?, $-, $$, $!
            spans.push(ColorSpan {
                start: pos,
                end: pos + 2,
                style: HighlightStyle::Variable,
            });
            state.word_start = false;
            state.command_position = false;
            pos + 2
        }
        _ => {
            // Bare $ at end of input or before something unexpected – treat as
            // default text.
            state.word_start = false;
            pos + 1
        }
    }
}

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
