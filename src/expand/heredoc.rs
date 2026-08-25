//! POSIX §2.7.4 here-document body expansion.
//!
//! Heredoc expansion is a distinct pipeline from word expansion: there is no
//! field splitting, no pathname expansion, and no tilde expansion. Quoted
//! heredocs (`<<'EOF'`) suppress all expansion; unquoted heredocs perform
//! parameter, arithmetic, and command substitution only — via the shared
//! dollar-scanner in `expand::dollar`.

use super::dollar::{DollarMode, expand_string};
use crate::env::ShellEnv;
use crate::parser::ast::WordPart;

/// Expand a here-document body.
/// If `quoted` is true (delimiter was quoted), body is literal — no expansion.
/// If `quoted` is false, parameter expansion, command substitution, and arithmetic
/// expansion are performed (same as double-quote context, but `"` is not special).
pub fn expand_body(env: &mut ShellEnv, parts: &[WordPart], quoted: bool) -> String {
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
        raw_body
    } else {
        // Unquoted delimiter: expand $VAR, $(cmd), `cmd`, $((expr)).
        expand_string(env, &raw_body, DollarMode::Heredoc)
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
        // The lexer stores the body as a single Literal; the dollar
        // scanner performs the expansion.
        let parts = vec![WordPart::Literal("value is $FOO\n".to_string())];
        assert_eq!(expand_body(&mut env, &parts, false), "value is bar\n");
    }
}
