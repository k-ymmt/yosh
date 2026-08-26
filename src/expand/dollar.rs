//! Shared `$`-expansion scanner for raw strings.
//!
//! Unquoted here-document bodies (POSIX §2.7.4) and the text inside
//! `$((...))` arithmetic expansion (POSIX §2.6.4) both perform parameter
//! expansion, command substitution, and arithmetic expansion on raw text
//! that the lexer stored as a single literal string. This module is the
//! one scanner for that grammar (the lexer's `read_dollar` handles the
//! token-level case); the two contexts behave identically — expansions
//! substitute their actual (possibly empty) text, and blank-expression
//! handling for arithmetic lives in `arith::evaluate`.

use super::scan::{skip_balanced_braces, skip_balanced_double_parens, skip_balanced_parens};
use super::{arith, command_sub, param};
use crate::env::ShellEnv;
use crate::error::{ExpansionErrorKind, ShellError};
use crate::parser::ast::{ParamExpr, WordPart};

/// Build the error for a `$`-construct left open at end of input
/// (`${x`, `$(cmd`, `$((1+`, `` `cmd ``). The lexer rejects these at
/// token level; this is the equivalent diagnostic for raw-string
/// contexts (heredoc bodies, arithmetic text). A word-context failure
/// aborts a non-interactive shell like any other expansion error; the
/// heredoc path converts it to a redirection error at the redirect
/// boundary (command skipped, shell continues).
fn unterminated_err(what: &str) -> ShellError {
    ShellError::expansion(
        ExpansionErrorKind::UnterminatedExpansion,
        format!("unterminated {}", what),
    )
}

/// Expand dollar references (`$VAR`, `${VAR}`, `$(cmd)`, `` `cmd` ``,
/// `$((expr))`) in a raw string, per the POSIX double-quote-like context
/// shared by unquoted heredoc bodies and arithmetic expressions
/// (no field splitting, no pathname expansion, `"` not special).
/// Expansions substitute their actual (possibly empty) text in both
/// contexts — `$((1${x}2))` with `x` unset is `12`, matching bash/dash;
/// an entirely blank arithmetic expression evaluates to 0 in
/// `arith::evaluate`. A nested arithmetic failure propagates as the
/// `ShellError` built by `arith::evaluate`, so both contexts share one
/// error channel.
pub(super) fn expand_string(env: &mut ShellEnv, s: &str) -> crate::error::Result<String> {
    let bytes = s.as_bytes();
    let mut result = String::new();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            i += 1;
            match bytes[i] {
                b'{' => {
                    // ${...} — find matching } (quote-aware, skipping
                    // nested $(...)/`...`/${...} constructs)
                    i += 1;
                    let start = i;
                    i = skip_balanced_braces(bytes, i);
                    if i >= bytes.len() {
                        // EOF before the closing `}` — do not fabricate
                        // one (bash/dash both diagnose this).
                        return Err(unterminated_err("parameter expansion"));
                    }
                    let inner = &s[start..i];
                    i += 1; // skip }
                    // Re-lex the full `${...}` so conditional (`${x:-w}`),
                    // length (`${#x}`), and strip forms all apply. Quoting
                    // inside the braces follows normal word rules per POSIX
                    // §2.7.4. Fall back to a plain lookup if the lexer
                    // rejects the expression.
                    result.push_str(&expand_braced_param(env, inner));
                }
                b'(' => {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'(' {
                        // $((...)) — arithmetic (recurses back through
                        // arith::evaluate, which calls expand_string on the
                        // strictly shorter inner text)
                        i += 2;
                        let start = i;
                        i = skip_balanced_double_parens(bytes, i);
                        if i + 1 >= bytes.len() || bytes[i] != b')' || bytes[i + 1] != b')' {
                            return Err(unterminated_err("arithmetic expansion"));
                        }
                        let expr = &s[start..i];
                        i += 2; // skip ))
                        result.push_str(&arith::evaluate(env, expr)?);
                    } else {
                        // $(...) — command substitution
                        i += 1;
                        let start = i;
                        i = skip_balanced_parens(bytes, i);
                        if i >= bytes.len() {
                            return Err(unterminated_err("command substitution"));
                        }
                        let cmd_str = &s[start..i];
                        i += 1; // skip )
                        run_command_sub(env, cmd_str, &mut result);
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
                    result.push_str(
                        &param::expand(env, &ParamExpr::Special(sp)).unwrap_or_default(),
                    );
                    i += 1;
                }
                ch if (b'1'..=b'9').contains(&ch) => {
                    let n = (ch - b'0') as usize;
                    result.push_str(
                        &param::expand(env, &ParamExpr::Positional(n)).unwrap_or_default(),
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
                    result.push_str(
                        &param::expand(env, &ParamExpr::Simple(name.to_string()))
                            .unwrap_or_default(),
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
            if i >= bytes.len() {
                return Err(unterminated_err("backtick command substitution"));
            }
            i += 1; // skip closing `
            run_command_sub(env, &cmd_str, &mut result);
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

/// Parse and execute a command substitution body, pushing its output
/// verbatim (trailing newlines are already stripped by
/// `command_sub::execute` — the standard command-substitution strip; no
/// further trimming, so `$((1$(printf ' ')2))` sees `1 2` and fails like
/// bash/dash instead of collapsing to `12`). An unparsable body
/// substitutes nothing.
fn run_command_sub(env: &mut ShellEnv, cmd_str: &str, result: &mut String) {
    if let Ok(program) = crate::parser::Parser::new(cmd_str).parse_program() {
        result.push_str(&command_sub::execute(env, &program));
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
    fn simple_var() {
        let mut env = make_env();
        env.vars.set("FOO", "bar").unwrap();
        assert_eq!(expand_string(&mut env, "v=$FOO.").unwrap(), "v=bar.");
    }

    #[test]
    fn unset_var_empty() {
        let mut env = make_env();
        assert_eq!(expand_string(&mut env, "[$nope]").unwrap(), "[]");
    }

    #[test]
    fn unset_var_substitutes_empty_not_zero() {
        // Regression: the old arith pre-pass injected "0" for empty
        // expansions, so $((1${x}2)) became 102; bash/dash give 12.
        let mut env = make_env();
        assert_eq!(expand_string(&mut env, "1${nope}2").unwrap(), "12");
        assert_eq!(expand_string(&mut env, "$nope + 1").unwrap(), " + 1");
    }

    #[test]
    fn braced_default() {
        let mut env = make_env();
        assert_eq!(expand_string(&mut env, "${x:-3} + 1").unwrap(), "3 + 1");
    }

    #[test]
    fn braced_length() {
        let mut env = make_env();
        env.vars.set("a", "hello").unwrap();
        assert_eq!(expand_string(&mut env, "${#a}+1").unwrap(), "5+1");
    }

    #[test]
    fn nested_arith() {
        let mut env = make_env();
        assert_eq!(expand_string(&mut env, "$((1+1)) + 1").unwrap(), "2 + 1");
    }

    #[test]
    fn multibyte_text_is_preserved() {
        let mut env = make_env();
        env.vars.set("x", "1").unwrap();
        assert_eq!(expand_string(&mut env, "日本$x語").unwrap(), "日本1語");
    }

    #[test]
    fn escapes_dollar_backtick_backslash() {
        let mut env = make_env();
        env.vars.set("x", "1").unwrap();
        assert_eq!(expand_string(&mut env, r"\$x \\ \a").unwrap(), r"$x \ \a");
    }

    #[test]
    fn line_continuation_removed() {
        let mut env = make_env();
        assert_eq!(expand_string(&mut env, "a\\\nb").unwrap(), "ab");
    }

    // ── nested constructs inside ${...} (a `}` inside a nested $(...),
    //    `` `...` ``, or ${...} must not close the outer brace) ──────────

    #[test]
    fn brace_scan_skips_nested_command_sub() {
        // ${x:-$(printf %s })} — the `}` inside $() belongs to the
        // command substitution. Command-sub output cannot be observed
        // under libtest (see note above), but the scan must consume the
        // whole construct and leave no trailing `)}` residue.
        let mut env = make_env();
        let out = expand_string(&mut env, "<${x:-$(printf %s })}>").unwrap();
        assert!(
            !out.contains(")}"),
            "nested $() `}}` closed the outer ${{}}: {:?}",
            out
        );
        assert!(out.starts_with('<') && out.ends_with('>'), "got {:?}", out);
    }

    #[test]
    fn brace_scan_skips_nested_backtick() {
        let mut env = make_env();
        let out = expand_string(&mut env, "<${x:-`printf %s }`}>").unwrap();
        assert!(
            !out.contains("`}"),
            "nested backtick `}}` closed the outer ${{}}: {:?}",
            out
        );
        assert!(out.starts_with('<') && out.ends_with('>'), "got {:?}", out);
    }

    #[test]
    fn brace_scan_handles_nested_braced_param() {
        let mut env = make_env();
        env.vars.set("y", "inner").unwrap();
        assert_eq!(
            expand_string(&mut env, "<${x:-${y:-}}>").unwrap(),
            "<inner>"
        );
        let mut env = make_env();
        assert_eq!(expand_string(&mut env, "<${x:-${y:-}}>").unwrap(), "<>");
    }

    // ── unterminated constructs at end of input are diagnosed, not
    //    silently accepted ─────────────────────────────────────────────

    #[test]
    fn unterminated_brace_is_error() {
        let mut env = make_env();
        let err = expand_string(&mut env, "${x").unwrap_err();
        assert_eq!(
            err.kind,
            crate::error::ShellErrorKind::Expansion(
                crate::error::ExpansionErrorKind::UnterminatedExpansion
            )
        );
        assert!(err.message.contains("parameter expansion"), "{}", err);
    }

    #[test]
    fn unterminated_command_sub_is_error() {
        let mut env = make_env();
        let err = expand_string(&mut env, "$(echo hi").unwrap_err();
        assert!(err.message.contains("command substitution"), "{}", err);
    }

    #[test]
    fn unterminated_arith_is_error() {
        let mut env = make_env();
        let err = expand_string(&mut env, "$((1+2").unwrap_err();
        assert!(err.message.contains("arithmetic expansion"), "{}", err);
        // A single `)` does not close `$((`.
        let err = expand_string(&mut env, "$((1+2)").unwrap_err();
        assert!(err.message.contains("arithmetic expansion"), "{}", err);
    }

    #[test]
    fn unterminated_backtick_is_error() {
        let mut env = make_env();
        let err = expand_string(&mut env, "`echo hi").unwrap_err();
        assert!(err.message.contains("backtick"), "{}", err);
    }

    #[test]
    fn terminal_brace_still_ok() {
        // Closing `}` as the very last byte must not be treated as
        // unterminated.
        let mut env = make_env();
        assert_eq!(expand_string(&mut env, "${x:-ok}").unwrap(), "ok");
    }
}
