//! Quoted-string scanners: single-quote, dollar-single-quote.
//! scan_double_quote arrives in Task B3 (it's currently &mut self in mod.rs).
//!
//! Each scanner is a free function. Takes the input slice, current pos,
//! the variant payload (`start` of the opening quote), shared state,
//! and the span accumulator.

use super::super::command_checker::CheckerEnv;
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

// -----------------------------------------------------------------------
// scan_double_quote
// -----------------------------------------------------------------------

pub(super) fn scan_double_quote(
    chars: &[char],
    pos: usize,
    start: usize,
    state: &mut ScannerState,
    spans: &mut Vec<ColorSpan>,
    _checker_env: &CheckerEnv,
) -> usize {
    let mut p = pos;
    let mut text_start = start; // includes the opening "

    while p < chars.len() {
        match chars[p] {
            '"' => {
                // Closing double quote
                spans.push(ColorSpan {
                    start: text_start,
                    end: p + 1,
                    style: HighlightStyle::DoubleString,
                });
                state.pop_mode();
                return p + 1;
            }
            '\\' => {
                // Escape: skip next char
                p += 1;
                if p < chars.len() {
                    p += 1;
                }
            }
            '$' => {
                // Emit DoubleString for text accumulated so far
                if p > text_start {
                    spans.push(ColorSpan {
                        start: text_start,
                        end: p,
                        style: HighlightStyle::DoubleString,
                    });
                }
                // Handle $ expansion inside double quotes
                let next = if p + 1 < chars.len() {
                    Some(chars[p + 1])
                } else {
                    None
                };
                match next {
                    Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                        let var_start = p;
                        let mut end = p + 1;
                        while end < chars.len()
                            && (chars[end].is_ascii_alphanumeric() || chars[end] == '_')
                        {
                            end += 1;
                        }
                        spans.push(ColorSpan {
                            start: var_start,
                            end,
                            style: HighlightStyle::Variable,
                        });
                        p = end;
                        text_start = p;
                    }
                    Some(c)
                        if c.is_ascii_digit()
                            || matches!(c, '@' | '*' | '#' | '?' | '-' | '$' | '!') =>
                    {
                        spans.push(ColorSpan {
                            start: p,
                            end: p + 2,
                            style: HighlightStyle::Variable,
                        });
                        p += 2;
                        text_start = p;
                    }
                    Some('{') => {
                        // ${...} inside double quote — scan to closing }
                        let brace_start = p;
                        p += 2; // skip ${
                        while p < chars.len() && chars[p] != '}' {
                            p += 1;
                        }
                        if p < chars.len() {
                            p += 1; // skip }
                        }
                        spans.push(ColorSpan {
                            start: brace_start,
                            end: p,
                            style: HighlightStyle::Variable,
                        });
                        text_start = p;
                    }
                    Some('(') => {
                        // $( or $(( inside double quotes
                        if p + 2 < chars.len() && chars[p + 2] == '(' {
                            // $(( — arithmetic
                            let arith_start = p;
                            p += 3;
                            while p + 1 < chars.len()
                                && !(chars[p] == ')' && chars[p + 1] == ')')
                            {
                                p += 1;
                            }
                            if p + 1 < chars.len() {
                                p += 2;
                            }
                            spans.push(ColorSpan {
                                start: arith_start,
                                end: p,
                                style: HighlightStyle::ArithSub,
                            });
                            text_start = p;
                        } else {
                            // $( — command sub inside double quotes
                            let cmd_start = p;
                            p += 2;
                            let mut depth = 1;
                            while p < chars.len() && depth > 0 {
                                if chars[p] == '(' {
                                    depth += 1;
                                } else if chars[p] == ')' {
                                    depth -= 1;
                                }
                                if depth > 0 {
                                    p += 1;
                                }
                            }
                            if p < chars.len() {
                                p += 1;
                            }
                            spans.push(ColorSpan {
                                start: cmd_start,
                                end: p,
                                style: HighlightStyle::CommandSub,
                            });
                            text_start = p;
                        }
                    }
                    _ => {
                        // Bare $
                        p += 1;
                        text_start = p - 1; // include $ in next string span
                    }
                }
            }
            '`' => {
                // Backtick inside double quotes
                if p > text_start {
                    spans.push(ColorSpan {
                        start: text_start,
                        end: p,
                        style: HighlightStyle::DoubleString,
                    });
                }
                let bt_start = p;
                p += 1;
                while p < chars.len() && chars[p] != '`' {
                    if chars[p] == '\\' {
                        p += 1;
                    }
                    p += 1;
                }
                if p < chars.len() {
                    p += 1; // skip closing `
                }
                spans.push(ColorSpan {
                    start: bt_start,
                    end: p,
                    style: HighlightStyle::CommandSub,
                });
                text_start = p;
            }
            _ => {
                p += 1;
            }
        }
    }
    // Unclosed — mark_unclosed_errors will handle it
    p
}
