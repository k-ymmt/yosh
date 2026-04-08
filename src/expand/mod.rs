use crate::env::ShellEnv;
use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};

/// Expand a single `Word` into a string.
pub fn expand_word_to_string(env: &ShellEnv, word: &Word) -> String {
    let mut out = String::new();
    for part in &word.parts {
        expand_part(env, part, &mut out);
    }
    out
}

/// Expand a slice of `Word`s into a `Vec<String>`.
pub fn expand_words(env: &ShellEnv, words: &[Word]) -> Vec<String> {
    words.iter().map(|w| expand_word_to_string(env, w)).collect()
}

/// Expand a single `WordPart`, appending the result to `out`.
fn expand_part(env: &ShellEnv, part: &WordPart, out: &mut String) {
    match part {
        WordPart::Literal(s) => out.push_str(s),
        WordPart::SingleQuoted(s) => out.push_str(s),
        WordPart::DollarSingleQuoted(s) => out.push_str(s),
        WordPart::DoubleQuoted(parts) => {
            for inner in parts {
                expand_part(env, inner, out);
            }
        }
        WordPart::Tilde(None) => {
            if let Some(home) = env.vars.get("HOME") {
                out.push_str(home);
            } else {
                out.push('~');
            }
        }
        WordPart::Tilde(Some(user)) => {
            // ~user: not implemented in Phase 2, emit literal
            out.push('~');
            out.push_str(user);
        }
        WordPart::Parameter(param) => expand_param(env, param, out),
        // Phase 3
        WordPart::CommandSub(_) => {}
        WordPart::ArithSub(_) => {}
    }
}

/// Expand a `ParamExpr`, appending the result to `out`.
fn expand_param(env: &ShellEnv, param: &ParamExpr, out: &mut String) {
    match param {
        ParamExpr::Simple(name) => {
            if let Some(val) = env.vars.get(name) {
                out.push_str(val);
            }
        }
        ParamExpr::Positional(n) => {
            if *n > 0 {
                if let Some(val) = env.positional_params.get(n - 1) {
                    out.push_str(val);
                }
            }
        }
        ParamExpr::Special(sp) => expand_special(env, sp, out),
        ParamExpr::Length(name) => {
            let len = env.vars.get(name).map(|v| v.len()).unwrap_or(0);
            out.push_str(&len.to_string());
        }
        ParamExpr::Default {
            name,
            word,
            null_check,
        } => {
            let val = env.vars.get(name);
            let is_unset_or_null = match val {
                None => true,
                Some(v) if *null_check && v.is_empty() => true,
                _ => false,
            };
            if is_unset_or_null {
                if let Some(default_word) = word {
                    let expanded = expand_word_to_string(env, default_word);
                    out.push_str(&expanded);
                }
            } else if let Some(v) = val {
                out.push_str(v);
            }
        }
        // Everything else — Phase 3+
        _ => {}
    }
}

/// Expand a `SpecialParam`, appending the result to `out`.
fn expand_special(env: &ShellEnv, sp: &SpecialParam, out: &mut String) {
    match sp {
        SpecialParam::Question => out.push_str(&env.last_exit_status.to_string()),
        SpecialParam::Dollar => out.push_str(&env.shell_pid.as_raw().to_string()),
        SpecialParam::Zero => out.push_str(&env.shell_name),
        SpecialParam::Hash => out.push_str(&env.positional_params.len().to_string()),
        SpecialParam::At | SpecialParam::Star => {
            out.push_str(&env.positional_params.join(" "));
        }
        // Phase 7
        SpecialParam::Bang => {}
        SpecialParam::Dash => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;
    use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};

    fn make_env() -> ShellEnv {
        let mut env = ShellEnv::new("kish", vec![]);
        // Clear any inherited env vars for predictable tests
        // We'll set vars directly for testing
        env
    }

    #[test]
    fn test_literal() {
        let env = make_env();
        let word = Word::literal("hello");
        assert_eq!(expand_word_to_string(&env, &word), "hello");
    }

    #[test]
    fn test_single_quoted() {
        let env = make_env();
        let word = Word {
            parts: vec![WordPart::SingleQuoted("hello world".to_string())],
        };
        assert_eq!(expand_word_to_string(&env, &word), "hello world");
    }

    #[test]
    fn test_dollar_single_quoted() {
        let env = make_env();
        let word = Word {
            parts: vec![WordPart::DollarSingleQuoted("hello\\nworld".to_string())],
        };
        assert_eq!(expand_word_to_string(&env, &word), "hello\\nworld");
    }

    #[test]
    fn test_double_quoted_literal() {
        let env = make_env();
        let word = Word {
            parts: vec![WordPart::DoubleQuoted(vec![WordPart::Literal(
                "hello".to_string(),
            )])],
        };
        assert_eq!(expand_word_to_string(&env, &word), "hello");
    }

    #[test]
    fn test_simple_param() {
        let mut env = make_env();
        env.vars.set("FOO", "bar").unwrap();
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Simple("FOO".to_string()))],
        };
        assert_eq!(expand_word_to_string(&env, &word), "bar");
    }

    #[test]
    fn test_unset_param() {
        let env = make_env();
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Simple(
                "UNSET_VAR_XYZ".to_string(),
            ))],
        };
        assert_eq!(expand_word_to_string(&env, &word), "");
    }

    #[test]
    fn test_special_question() {
        let mut env = make_env();
        env.last_exit_status = 42;
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(
                SpecialParam::Question,
            ))],
        };
        assert_eq!(expand_word_to_string(&env, &word), "42");
    }

    #[test]
    fn test_special_dollar() {
        let env = make_env();
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::Dollar))],
        };
        let result = expand_word_to_string(&env, &word);
        // Should be a positive integer (the PID)
        let pid: i32 = result.parse().expect("PID should be an integer");
        assert!(pid > 0);
    }

    #[test]
    fn test_special_zero() {
        let env = ShellEnv::new("mykish", vec![]);
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::Zero))],
        };
        assert_eq!(expand_word_to_string(&env, &word), "mykish");
    }

    #[test]
    fn test_positional_param() {
        let env = ShellEnv::new("kish", vec!["first".to_string(), "second".to_string()]);
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Positional(1))],
        };
        assert_eq!(expand_word_to_string(&env, &word), "first");
        let word2 = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Positional(2))],
        };
        assert_eq!(expand_word_to_string(&env, &word2), "second");
    }

    #[test]
    fn test_positional_out_of_range() {
        let env = ShellEnv::new("kish", vec!["only".to_string()]);
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Positional(5))],
        };
        assert_eq!(expand_word_to_string(&env, &word), "");
    }

    #[test]
    fn test_special_hash() {
        let env = ShellEnv::new("kish", vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::Hash))],
        };
        assert_eq!(expand_word_to_string(&env, &word), "3");
    }

    #[test]
    fn test_tilde_none() {
        let mut env = make_env();
        env.vars.set("HOME", "/home/user").unwrap();
        let word = Word {
            parts: vec![WordPart::Tilde(None)],
        };
        assert_eq!(expand_word_to_string(&env, &word), "/home/user");
    }

    #[test]
    fn test_tilde_none_no_home() {
        let mut env = make_env();
        // Ensure HOME is not set (clear if inherited)
        let _ = env.vars.unset("HOME");
        let word = Word {
            parts: vec![WordPart::Tilde(None)],
        };
        assert_eq!(expand_word_to_string(&env, &word), "~");
    }

    #[test]
    fn test_mixed_parts() {
        let mut env = make_env();
        env.vars.set("NAME", "world").unwrap();
        let word = Word {
            parts: vec![
                WordPart::Literal("hello ".to_string()),
                WordPart::Parameter(ParamExpr::Simple("NAME".to_string())),
                WordPart::Literal("!".to_string()),
            ],
        };
        assert_eq!(expand_word_to_string(&env, &word), "hello world!");
    }

    #[test]
    fn test_dollar_in_double_quote() {
        let mut env = make_env();
        env.vars.set("X", "42").unwrap();
        let word = Word {
            parts: vec![WordPart::DoubleQuoted(vec![
                WordPart::Literal("value=".to_string()),
                WordPart::Parameter(ParamExpr::Simple("X".to_string())),
            ])],
        };
        assert_eq!(expand_word_to_string(&env, &word), "value=42");
    }

    #[test]
    fn test_expand_words() {
        let mut env = make_env();
        env.vars.set("A", "foo").unwrap();
        let words = vec![
            Word::literal("hello"),
            Word {
                parts: vec![WordPart::Parameter(ParamExpr::Simple("A".to_string()))],
            },
        ];
        assert_eq!(expand_words(&env, &words), vec!["hello", "foo"]);
    }

    #[test]
    fn test_param_default() {
        let mut env = make_env();
        // Variable is unset: should use the default word
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Default {
                name: "UNSET_VAR".to_string(),
                word: Some(Word::literal("default")),
                null_check: false,
            })],
        };
        assert_eq!(expand_word_to_string(&env, &word), "default");

        // Variable is set: should use the value
        env.vars.set("UNSET_VAR", "actual").unwrap();
        assert_eq!(expand_word_to_string(&env, &word), "actual");
    }

    #[test]
    fn test_param_default_null_check() {
        let mut env = make_env();
        env.vars.set("EMPTY_VAR", "").unwrap();
        // null_check=true + empty value => use default
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Default {
                name: "EMPTY_VAR".to_string(),
                word: Some(Word::literal("fallback")),
                null_check: true,
            })],
        };
        assert_eq!(expand_word_to_string(&env, &word), "fallback");

        // null_check=false + empty value => use empty value
        let word2 = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Default {
                name: "EMPTY_VAR".to_string(),
                word: Some(Word::literal("fallback")),
                null_check: false,
            })],
        };
        assert_eq!(expand_word_to_string(&env, &word2), "");
    }
}
