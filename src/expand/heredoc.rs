//! POSIX §2.7.4 here-document body expansion.
//!
//! Heredoc expansion is a distinct pipeline from word expansion: there is no
//! field splitting, no pathname expansion, and no tilde expansion. Quoted
//! heredocs (`<<'EOF'`) suppress all expansion; unquoted heredocs perform
//! parameter, arithmetic, and command substitution only.

use super::scan::{skip_balanced_braces, skip_balanced_double_parens, skip_balanced_parens};
use super::tilde::expand_tilde_user;
use super::{arith, command_sub, param};
use crate::env::ShellEnv;
use crate::parser::ast::{ParamExpr, WordPart};

/// Expand a here-document body.
/// If `quoted` is true (delimiter was quoted), body is literal — no expansion.
/// If `quoted` is false, parameter expansion, command substitution, and arithmetic
/// expansion are performed (same as double-quote context, but `"` is not special).
pub fn expand_body(env: &mut ShellEnv, parts: &[WordPart], quoted: bool) -> String {
    // First, get the raw body text
    let mut raw_body = String::new();
    for part in parts {
        match part {
            WordPart::Literal(s) => raw_body.push_str(s),
            _ => {
                // If parts already contain expansion nodes (from lexer), expand them
                expand_part(env, part, &mut raw_body);
            }
        }
    }

    if quoted {
        // Quoted delimiter: no expansion, return literal body
        raw_body
    } else {
        // Unquoted delimiter: expand $VAR, $(cmd), $((expr)) in the body text.
        // The lexer stores the body as a single Literal string, so we need to
        // process it character by character for dollar expansions.
        expand_string(env, &raw_body)
    }
}

/// Expand dollar references ($VAR, ${VAR}, $(cmd), $((expr))) in a raw string.
/// Used for unquoted here-document bodies where the lexer stored everything as literal text.
fn expand_string(env: &mut ShellEnv, s: &str) -> String {
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
                    let name = &s[start..i];
                    if i < bytes.len() {
                        i += 1;
                    } // skip }
                    // Simple lookup (conditional forms not supported in heredoc string expansion)
                    result.push_str(env.vars.get(name).unwrap_or(""));
                }
                b'(' => {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'(' {
                        // $((...)) — arithmetic
                        i += 2;
                        let start = i;
                        i = skip_balanced_double_parens(bytes, i);
                        let expr = &s[start..i];
                        if i + 1 < bytes.len() {
                            i += 2;
                        } // skip ))
                        match arith::evaluate(env, expr) {
                            Ok(val) => result.push_str(&val),
                            Err(msg) => {
                                eprintln!("yosh: arithmetic: {}", msg);
                                env.exec.last_exit_status = 1;
                                result.push('0');
                            }
                        }
                    } else {
                        // $(...) — command substitution
                        i += 1;
                        let start = i;
                        i = skip_balanced_parens(bytes, i);
                        let cmd_str = &s[start..i];
                        if i < bytes.len() {
                            i += 1;
                        } // skip )
                        // Parse and execute
                        if let Ok(program) = crate::parser::Parser::new(cmd_str).parse_program() {
                            result.push_str(&command_sub::execute(env, &program));
                        }
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
                    result
                        .push_str(&param::expand(env, &ParamExpr::Special(sp)).unwrap_or_default());
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
                    result.push_str(env.vars.get(name).unwrap_or(""));
                }
                _ => {
                    result.push('$');
                    // Don't advance — the current byte is not part of the expansion
                }
            }
        } else if bytes[i] == b'\\' && i + 1 < bytes.len() {
            // Backslash in heredoc: only escapes $, `, \, newline
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
            // Backtick command substitution
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != b'`' {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            let cmd_str = &s[start..i];
            if i < bytes.len() {
                i += 1;
            } // skip closing `
            if let Ok(program) = crate::parser::Parser::new(cmd_str).parse_program() {
                result.push_str(&command_sub::execute(env, &program));
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    result
}

fn expand_part(env: &mut ShellEnv, part: &WordPart, out: &mut String) {
    match part {
        WordPart::Literal(s) => out.push_str(s),
        WordPart::EscapedLiteral(s) => out.push_str(s),
        WordPart::Parameter(p) => {
            let expanded = param::expand(env, p).unwrap_or_default();
            out.push_str(&expanded);
        }
        WordPart::CommandSub(program) => {
            let output = command_sub::execute(env, program);
            out.push_str(&output);
        }
        WordPart::ArithSub(expr) => match arith::evaluate(env, expr) {
            Ok(val) => out.push_str(&val),
            Err(msg) => {
                eprintln!("yosh: arithmetic: {}", msg);
                env.exec.last_exit_status = 1;
                out.push('0');
            }
        },
        WordPart::SingleQuoted(s) | WordPart::DollarSingleQuoted(s) => out.push_str(s),
        WordPart::DoubleQuoted(parts) => {
            for p in parts {
                expand_part(env, p, out);
            }
        }
        WordPart::Tilde(None) => {
            let home = env.vars.get("HOME").map(|s| s.to_string());
            out.push_str(&home.unwrap_or_else(|| "~".to_string()));
        }
        WordPart::Tilde(Some(user)) => {
            out.push_str(&expand_tilde_user(user));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;
    use crate::parser::ast::{ParamExpr, WordPart};

    fn make_env() -> ShellEnv {
        ShellEnv::new("yosh", vec![])
    }

    #[test]
    fn test_expand_heredoc_body_literal() {
        let mut env = make_env();
        let parts = vec![WordPart::Literal("hello world\n".to_string())];
        assert_eq!(expand_body(&mut env, &parts, true), "hello world\n");
    }

    #[test]
    fn test_expand_heredoc_body_quoted_no_expansion() {
        let mut env = make_env();
        env.vars.set("FOO", "bar").unwrap();
        let parts = vec![WordPart::Literal("value is $FOO\n".to_string())];
        assert_eq!(expand_body(&mut env, &parts, true), "value is $FOO\n");
    }

    #[test]
    fn test_expand_heredoc_body_unquoted_expands() {
        let mut env = make_env();
        env.vars.set("FOO", "bar").unwrap();
        let parts = vec![
            WordPart::Literal("value is ".to_string()),
            WordPart::Parameter(ParamExpr::Simple("FOO".to_string())),
            WordPart::Literal("\n".to_string()),
        ];
        assert_eq!(expand_body(&mut env, &parts, false), "value is bar\n");
    }
}
