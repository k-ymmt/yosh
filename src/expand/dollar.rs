//! Shared `$`-expansion scanner for raw strings.
//!
//! Unquoted here-document bodies (POSIX §2.7.4) and the text inside
//! `$((...))` arithmetic expansion (POSIX §2.6.4) both perform parameter
//! expansion, command substitution, and arithmetic expansion on raw text
//! that the lexer stored as a single literal string. This module is the
//! one scanner for that grammar (the lexer's `read_dollar` handles the
//! token-level case); the two contexts differ only in the small policy
//! captured by [`DollarMode`].

use super::scan::{skip_balanced_braces, skip_balanced_double_parens, skip_balanced_parens};
use super::{arith, command_sub, param};
use crate::env::ShellEnv;
use crate::parser::ast::{ParamExpr, WordPart};

/// Context policy for [`expand_string`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DollarMode {
    /// Unquoted here-document body: expansion results are substituted
    /// verbatim.
    Heredoc,
    /// Arithmetic-expression pre-pass: an expansion that produces an
    /// empty string substitutes `"0"` instead (the arithmetic parser
    /// needs an operand; matches the historical arith behavior where
    /// unset variables default to 0), and command-substitution output is
    /// whitespace-trimmed before the empty check.
    Arith,
}

/// Expand dollar references (`$VAR`, `${VAR}`, `$(cmd)`, `` `cmd` ``,
/// `$((expr))`) in a raw string, per the POSIX double-quote-like context
/// shared by unquoted heredoc bodies and arithmetic expressions
/// (no field splitting, no pathname expansion, `"` not special).
/// A nested arithmetic failure propagates as the `ShellError` built by
/// `arith::evaluate`, so both contexts share one error channel.
pub(super) fn expand_string(
    env: &mut ShellEnv,
    s: &str,
    mode: DollarMode,
) -> crate::error::Result<String> {
    let bytes = s.as_bytes();
    let mut result = String::new();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            i += 1;
            match bytes[i] {
                b'{' => {
                    // ${...} — find matching } (quote-aware)
                    i += 1;
                    let start = i;
                    i = skip_balanced_braces(bytes, i);
                    let inner = &s[start..i];
                    if i < bytes.len() {
                        i += 1;
                    } // skip }
                    // Re-lex the full `${...}` so conditional (`${x:-w}`),
                    // length (`${#x}`), and strip forms all apply. Quoting
                    // inside the braces follows normal word rules per POSIX
                    // §2.7.4. Fall back to a plain lookup if the lexer
                    // rejects the expression.
                    push_expansion(&mut result, &expand_braced_param(env, inner), mode);
                }
                b'(' => {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'(' {
                        // $((...)) — arithmetic (recurses back through
                        // arith::evaluate, which calls expand_string on the
                        // strictly shorter inner text)
                        i += 2;
                        let start = i;
                        i = skip_balanced_double_parens(bytes, i);
                        let expr = &s[start..i];
                        if i + 1 < bytes.len() {
                            i += 2;
                        } // skip ))
                        result.push_str(&arith::evaluate(env, expr)?);
                    } else {
                        // $(...) — command substitution
                        i += 1;
                        let start = i;
                        i = skip_balanced_parens(bytes, i);
                        let cmd_str = &s[start..i];
                        if i < bytes.len() {
                            i += 1;
                        } // skip )
                        run_command_sub(env, cmd_str, &mut result, mode);
                    }
                }
                b'@' | b'*' | b'#' | b'?' | b'-' | b'$' | b'!' | b'0' => {
                    let sp = match bytes[i] {
                        b'@' => crate::parser::ast::SpecialParam::At,
                        b'*' => crate::parser::ast::SpecialParam::Star,
                        b'#' => crate::parser::ast::SpecialParam::Hash,
                        b'?' => crate::parser::ast::SpecialParam::Question,
                        b'-' => crate::parser::ast::SpecialParam::Dash,
                        b'$' => crate::parser::ast::SpecialParam::Dollar,
                        b'!' => crate::parser::ast::SpecialParam::Bang,
                        b'0' => crate::parser::ast::SpecialParam::Zero,
                        _ => unreachable!(),
                    };
                    push_expansion(
                        &mut result,
                        &param::expand(env, &ParamExpr::Special(sp)).unwrap_or_default(),
                        mode,
                    );
                    i += 1;
                }
                ch if (b'1'..=b'9').contains(&ch) => {
                    let n = (ch - b'0') as usize;
                    push_expansion(
                        &mut result,
                        &param::expand(env, &ParamExpr::Positional(n)).unwrap_or_default(),
                        mode,
                    );
                    i += 1;
                }
                ch if ch.is_ascii_alphabetic() || ch == b'_' => {
                    let start = i;
                    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_')
                    {
                        i += 1;
                    }
                    let name = &s[start..i];
                    // Route through param::expand so set -u (nounset)
                    // applies here too.
                    push_expansion(
                        &mut result,
                        &param::expand(env, &ParamExpr::Simple(name.to_string()))
                            .unwrap_or_default(),
                        mode,
                    );
                }
                _ => {
                    result.push('$');
                    // Don't advance — the current byte is not part of the expansion
                }
            }
        } else if bytes[i] == b'\\' && i + 1 < bytes.len() {
            // Backslash escapes only $, `, \, newline (POSIX §2.7.4
            // heredoc rules; same set as inside double quotes)
            let next = bytes[i + 1];
            match next {
                b'$' | b'`' | b'\\' => {
                    result.push(next as char);
                    i += 2;
                }
                b'\n' => {
                    // Line continuation
                    i += 2;
                }
                _ => {
                    result.push('\\');
                    i += 1;
                }
            }
        } else if bytes[i] == b'`' {
            // Backtick command substitution. Within the backticks,
            // backslash retains its literal meaning except before
            // $, `, \ (POSIX §2.6.3) — same unescaping as the lexer's
            // read_backtick.
            i += 1;
            let mut cmd_str = String::new();
            while i < bytes.len() && bytes[i] != b'`' {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    match bytes[i + 1] {
                        b'$' | b'`' | b'\\' => {
                            cmd_str.push(bytes[i + 1] as char);
                            i += 2;
                            continue;
                        }
                        _ => {
                            cmd_str.push('\\');
                            i += 1;
                            continue;
                        }
                    }
                }
                let ch = s[i..].chars().next().expect("i is on a char boundary");
                cmd_str.push(ch);
                i += ch.len_utf8();
            }
            if i < bytes.len() {
                i += 1;
            } // skip closing `
            run_command_sub(env, &cmd_str, &mut result, mode);
        } else {
            // Copy the full (possibly multi-byte) character — indexing by
            // byte and casting through `as char` would decode UTF-8 bytes
            // as Latin-1 and corrupt non-ASCII text.
            let ch = s[i..].chars().next().expect("i is on a char boundary");
            result.push(ch);
            i += ch.len_utf8();
        }
    }
    Ok(result)
}

/// Push one expansion result, applying the arith empty-operand policy.
fn push_expansion(result: &mut String, val: &str, mode: DollarMode) {
    if mode == DollarMode::Arith && val.is_empty() {
        result.push('0');
    } else {
        result.push_str(val);
    }
}

/// Parse and execute a command substitution body, pushing its output.
/// Arith mode trims the output (trailing newlines are already stripped
/// by `command_sub::execute`; interior trailing spaces would still parse,
/// but the historical arith behavior trimmed both ends) and substitutes
/// "0" for an empty result.
fn run_command_sub(env: &mut ShellEnv, cmd_str: &str, result: &mut String, mode: DollarMode) {
    if let Ok(program) = crate::parser::Parser::new(cmd_str).parse_program() {
        let output = command_sub::execute(env, &program);
        match mode {
            DollarMode::Heredoc => result.push_str(&output),
            DollarMode::Arith => push_expansion(result, output.trim(), mode),
        }
    } else if mode == DollarMode::Arith {
        // Historical arith behavior: an unparsable substitution counts as 0.
        result.push('0');
    }
}

/// Expand the inside of a `${...}` by re-lexing it as a full parameter
/// expansion. `inner` is the text between the braces.
fn expand_braced_param(env: &mut ShellEnv, inner: &str) -> String {
    let input = format!("${{{}}}", inner);
    let mut lexer = crate::lexer::Lexer::new(&input);
    if let Ok(tok) = lexer.next_token()
        && let crate::lexer::token::Token::Word(word) = tok.token
        && let [WordPart::Parameter(p)] = word.parts.as_slice()
    {
        return param::expand(env, p).unwrap_or_default();
    }
    env.vars.get(inner).unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;

    fn make_env() -> ShellEnv {
        ShellEnv::new("yosh", vec![])
    }

    // Command substitution cannot be asserted in unit tests: libtest
    // captures the forked child's Rust-level stdout before it reaches the
    // OS pipe (see expand::tests). Covered by e2e tests instead.

    #[test]
    fn heredoc_mode_simple_var() {
        let mut env = make_env();
        env.vars.set("FOO", "bar").unwrap();
        assert_eq!(
            expand_string(&mut env, "v=$FOO.", DollarMode::Heredoc).unwrap(),
            "v=bar."
        );
    }

    #[test]
    fn heredoc_mode_unset_var_empty() {
        let mut env = make_env();
        assert_eq!(
            expand_string(&mut env, "[$nope]", DollarMode::Heredoc).unwrap(),
            "[]"
        );
    }

    #[test]
    fn arith_mode_unset_var_is_zero() {
        let mut env = make_env();
        assert_eq!(
            expand_string(&mut env, "$nope + 1", DollarMode::Arith).unwrap(),
            "0 + 1"
        );
    }

    #[test]
    fn arith_mode_braced_default() {
        let mut env = make_env();
        assert_eq!(
            expand_string(&mut env, "${x:-3} + 1", DollarMode::Arith).unwrap(),
            "3 + 1"
        );
    }

    #[test]
    fn arith_mode_braced_length() {
        let mut env = make_env();
        env.vars.set("a", "hello").unwrap();
        assert_eq!(
            expand_string(&mut env, "${#a}+1", DollarMode::Arith).unwrap(),
            "5+1"
        );
    }

    #[test]
    fn arith_mode_nested_arith() {
        let mut env = make_env();
        assert_eq!(
            expand_string(&mut env, "$((1+1)) + 1", DollarMode::Arith).unwrap(),
            "2 + 1"
        );
    }

    #[test]
    fn multibyte_text_is_preserved() {
        let mut env = make_env();
        env.vars.set("x", "1").unwrap();
        assert_eq!(
            expand_string(&mut env, "日本$x語", DollarMode::Arith).unwrap(),
            "日本1語"
        );
        assert_eq!(
            expand_string(&mut env, "日本$x語", DollarMode::Heredoc).unwrap(),
            "日本1語"
        );
    }

    #[test]
    fn escapes_dollar_backtick_backslash() {
        let mut env = make_env();
        env.vars.set("x", "1").unwrap();
        assert_eq!(
            expand_string(&mut env, r"\$x \\ \a", DollarMode::Heredoc).unwrap(),
            r"$x \ \a"
        );
    }

    #[test]
    fn line_continuation_removed() {
        let mut env = make_env();
        assert_eq!(
            expand_string(&mut env, "a\\\nb", DollarMode::Heredoc).unwrap(),
            "ab"
        );
    }
}
