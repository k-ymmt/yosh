//! Quoted-string scanners: single-quote, double-quote, dollar-single-quote.
//!
//! Each scanner is a free function taking (ctx, env, pos, [payload]).

use super::super::command_checker::CheckerEnv;
use super::super::highlight::{ColorSpan, HighlightStyle};
use super::ctx::ScanCtx;

pub(super) fn scan_single_quote(
    ctx: &mut ScanCtx<'_>,
    _env: &CheckerEnv<'_>,
    pos: usize,
    start: usize,
) -> usize {
    let mut p = pos;
    while p < ctx.input.len() {
        if ctx.input[p] == '\'' {
            ctx.spans.push(ColorSpan {
                start,
                end: p + 1,
                style: HighlightStyle::String,
            });
            ctx.state.pop_mode();
            return p + 1;
        }
        p += 1;
    }
    // Unclosed — mark_unclosed_errors will handle it
    p
}

pub(super) fn scan_dollar_single_quote(
    ctx: &mut ScanCtx<'_>,
    _env: &CheckerEnv<'_>,
    pos: usize,
    start: usize,
) -> usize {
    let mut p = pos;
    while p < ctx.input.len() {
        if ctx.input[p] == '\\' {
            // escape: skip next
            p += 1;
            if p < ctx.input.len() {
                p += 1;
            }
            continue;
        }
        if ctx.input[p] == '\'' {
            ctx.spans.push(ColorSpan {
                start,
                end: p + 1,
                style: HighlightStyle::String,
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
// scan_double_quote
// -----------------------------------------------------------------------

pub(super) fn scan_double_quote(
    ctx: &mut ScanCtx<'_>,
    _env: &CheckerEnv<'_>,
    pos: usize,
    start: usize,
) -> usize {
    let mut p = pos;
    let mut text_start = start; // includes the opening "

    while p < ctx.input.len() {
        match ctx.input[p] {
            '"' => {
                // Closing double quote
                ctx.spans.push(ColorSpan {
                    start: text_start,
                    end: p + 1,
                    style: HighlightStyle::DoubleString,
                });
                ctx.state.pop_mode();
                return p + 1;
            }
            '\\' => {
                // Escape: skip next char
                p += 1;
                if p < ctx.input.len() {
                    p += 1;
                }
            }
            '$' => {
                // Emit DoubleString for text accumulated so far
                if p > text_start {
                    ctx.spans.push(ColorSpan {
                        start: text_start,
                        end: p,
                        style: HighlightStyle::DoubleString,
                    });
                }
                // Handle $ expansion inside double quotes
                let next = if p + 1 < ctx.input.len() {
                    Some(ctx.input[p + 1])
                } else {
                    None
                };
                match next {
                    Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                        let var_start = p;
                        let mut end = p + 1;
                        while end < ctx.input.len()
                            && (ctx.input[end].is_ascii_alphanumeric() || ctx.input[end] == '_')
                        {
                            end += 1;
                        }
                        ctx.spans.push(ColorSpan {
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
                        ctx.spans.push(ColorSpan {
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
                        while p < ctx.input.len() && ctx.input[p] != '}' {
                            p += 1;
                        }
                        if p < ctx.input.len() {
                            p += 1; // skip }
                        }
                        ctx.spans.push(ColorSpan {
                            start: brace_start,
                            end: p,
                            style: HighlightStyle::Variable,
                        });
                        text_start = p;
                    }
                    Some('(') => {
                        // $( or $(( inside double quotes
                        if p + 2 < ctx.input.len() && ctx.input[p + 2] == '(' {
                            // $(( — arithmetic
                            let arith_start = p;
                            p += 3;
                            while p + 1 < ctx.input.len()
                                && !(ctx.input[p] == ')' && ctx.input[p + 1] == ')')
                            {
                                p += 1;
                            }
                            if p + 1 < ctx.input.len() {
                                p += 2;
                            }
                            ctx.spans.push(ColorSpan {
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
                            while p < ctx.input.len() && depth > 0 {
                                if ctx.input[p] == '(' {
                                    depth += 1;
                                } else if ctx.input[p] == ')' {
                                    depth -= 1;
                                }
                                if depth > 0 {
                                    p += 1;
                                }
                            }
                            if p < ctx.input.len() {
                                p += 1;
                            }
                            ctx.spans.push(ColorSpan {
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
                    ctx.spans.push(ColorSpan {
                        start: text_start,
                        end: p,
                        style: HighlightStyle::DoubleString,
                    });
                }
                let bt_start = p;
                p += 1;
                while p < ctx.input.len() && ctx.input[p] != '`' {
                    if ctx.input[p] == '\\' {
                        p += 1;
                    }
                    p += 1;
                }
                if p < ctx.input.len() {
                    p += 1; // skip closing `
                }
                ctx.spans.push(ColorSpan {
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
