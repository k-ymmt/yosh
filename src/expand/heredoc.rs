//! POSIX §2.7.4 here-document body expansion.
//!
//! Heredoc expansion is a distinct pipeline from word expansion: there is no
//! field splitting, no pathname expansion, and no tilde expansion. Quoted
//! heredocs (`<<'EOF'`) suppress all expansion; unquoted heredocs perform
//! parameter, arithmetic, and command substitution only — via the shared
//! dollar-scanner in `expand::dollar`.

use super::dollar::expand_string;
use crate::env::ShellEnv;
use crate::parser::ast::WordPart;

/// Expand a here-document body.
/// If `quoted` is true (delimiter was quoted), body is literal — no expansion.
/// If `quoted` is false, parameter expansion, command substitution, and arithmetic
/// expansion are performed (same as double-quote context, but `"` is not special).
/// An arithmetic failure in the body surfaces as the `ShellError` built by
/// `arith::evaluate`; the redirect layer converts it to a redirection error
/// (command aborted, shell continues — matching dash/bash).
///
/// Braced-parameter failures (`${x:?msg}`, `set -u` unset variables) do
/// not return `Err` from the expander — they print their diagnostic and
/// raise `FlowControl::ExpansionError` (see `param::expansion_error`).
/// In a WORD context that aborts a non-interactive shell, but in a
/// heredoc dash keeps the shell alive (empirical 2026-08-26; heredocs are
/// redirections, so POSIX §2.8.1's redirection-error row applies). The
/// redirect layer therefore checks for a newly-raised `ExpansionError`
/// after calling this function and converts it into a redirection error.
pub fn expand_body(
    env: &mut ShellEnv,
    parts: &[WordPart],
    quoted: bool,
) -> crate::error::Result<String> {
    // Lexer::read_heredoc_body always stores the body as a single
    // `WordPart::Literal`; other variants cannot occur.
    let mut raw_body = String::new();
    for part in parts {
        match part {
            WordPart::Literal(s) => raw_body.push_str(s),
            other => {
                debug_assert!(
                    false,
                    "heredoc body must consist of Literal parts, got {:?}",
                    other
                );
            }
        }
    }

    if quoted {
        // Quoted delimiter: no expansion, return literal body
        Ok(raw_body)
    } else {
        // Unquoted delimiter: expand $VAR, $(cmd), `cmd`, $((expr)).
        expand_string(env, &raw_body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;
    use crate::parser::ast::WordPart;

    fn make_env() -> ShellEnv {
        ShellEnv::new("yosh", vec![])
    }

    #[test]
    fn test_expand_heredoc_body_literal() {
        let mut env = make_env();
        let parts = vec![WordPart::Literal("hello world\n".to_string())];
        assert_eq!(
            expand_body(&mut env, &parts, true).unwrap(),
            "hello world\n"
        );
    }

    #[test]
    fn test_expand_heredoc_body_quoted_no_expansion() {
        let mut env = make_env();
        env.vars.set("FOO", "bar").unwrap();
        let parts = vec![WordPart::Literal("value is $FOO\n".to_string())];
        assert_eq!(
            expand_body(&mut env, &parts, true).unwrap(),
            "value is $FOO\n"
        );
    }

    #[test]
    fn test_expand_heredoc_body_unquoted_expands() {
        let mut env = make_env();
        env.vars.set("FOO", "bar").unwrap();
        // The lexer stores the body as a single Literal; the dollar
        // scanner performs the expansion.
        let parts = vec![WordPart::Literal("value is $FOO\n".to_string())];
        assert_eq!(
            expand_body(&mut env, &parts, false).unwrap(),
            "value is bar\n"
        );
    }
}
