pub mod arith;
pub mod command_sub;
pub mod field_split;
pub mod param;
pub mod pathname;
pub mod pattern;

use crate::env::ShellEnv;
use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};

// ─── ExpandedField ──────────────────────────────────────────────────────────

/// A word that has been through parameter/command/arithmetic expansion.
/// Each byte has a corresponding `quoted_mask` entry:
///   `true`  = came from a quoted context → protected from field splitting and glob.
///   `false` = unquoted → subject to field splitting and pathname expansion.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpandedField {
    pub value: String,
    /// One bool per *byte* (not char) of `value`.
    pub quoted_mask: Vec<bool>,
}

impl ExpandedField {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            quoted_mask: Vec::new(),
        }
    }

    /// Append `s` marking each byte as **quoted** (protected).
    pub fn push_quoted(&mut self, s: &str) {
        self.value.push_str(s);
        self.quoted_mask
            .extend(std::iter::repeat(true).take(s.len()));
    }

    /// Append `s` marking each byte as **unquoted** (splittable/globbable).
    pub fn push_unquoted(&mut self, s: &str) {
        self.value.push_str(s);
        self.quoted_mask
            .extend(std::iter::repeat(false).take(s.len()));
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }
}

impl Default for ExpandedField {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Public API ─────────────────────────────────────────────────────────────

/// Expand a single `Word` through the full POSIX pipeline:
///   1. Parameter / command-sub / arithmetic expansion
///   2. Field splitting (IFS)
///   3. Pathname expansion (glob)
///   4. Quote removal  ← callers receive plain `String`s
pub fn expand_word(env: &mut ShellEnv, word: &Word) -> Vec<String> {
    let fields = expand_word_to_fields(env, word);
    let fields = field_split::split(env, fields);
    let fields = pathname::expand(env, fields);
    fields
        .into_iter()
        .filter(|f| !f.is_empty())
        .map(|f| f.value)
        .collect()
}

/// Expand a slice of `Word`s — each word is expanded independently,
/// then all resulting fields are concatenated.
pub fn expand_words(env: &mut ShellEnv, words: &[Word]) -> Vec<String> {
    let mut result = Vec::new();
    for word in words {
        result.extend(expand_word(env, word));
    }
    result
}

/// Expand a `Word` to a single `String`, suitable for assignments and
/// redirect targets (no field splitting, no glob).
pub fn expand_word_to_string(env: &mut ShellEnv, word: &Word) -> String {
    let fields = expand_word_to_fields(env, word);
    // Concatenate all fields (there is normally only one here, but $@ inside
    // double quotes can produce multiple — join them with a space in that case).
    fields
        .into_iter()
        .map(|f| f.value)
        .collect::<Vec<_>>()
        .join(" ")
}

// ─── Stage 1: expand to ExpandedField list ──────────────────────────────────

/// Expand a `Word` into a list of `ExpandedField`s (before field splitting).
fn expand_word_to_fields(env: &mut ShellEnv, word: &Word) -> Vec<ExpandedField> {
    let mut fields = vec![ExpandedField::new()];
    for part in &word.parts {
        expand_part_to_fields(env, part, &mut fields, false);
    }
    fields
}

/// Expand one `WordPart`, appending into `fields`.
/// `in_double_quote` is true when we are inside `DoubleQuoted(...)`.
fn expand_part_to_fields(
    env: &mut ShellEnv,
    part: &WordPart,
    fields: &mut Vec<ExpandedField>,
    in_double_quote: bool,
) {
    match part {
        // ── Quoted literals ───────────────────────────────────────────────
        WordPart::Literal(s) => {
            if in_double_quote {
                fields.last_mut().unwrap().push_quoted(s);
            } else {
                fields.last_mut().unwrap().push_unquoted(s);
            }
        }
        WordPart::SingleQuoted(s) => {
            // Single quotes protect everything
            fields.last_mut().unwrap().push_quoted(s);
        }
        WordPart::DollarSingleQuoted(s) => {
            // $'...' also protects from splitting/glob
            fields.last_mut().unwrap().push_quoted(s);
        }

        // ── Double-quoted group ───────────────────────────────────────────
        WordPart::DoubleQuoted(parts) => {
            for inner in parts {
                expand_part_to_fields(env, inner, fields, true);
            }
        }

        // ── Tilde expansion ───────────────────────────────────────────────
        WordPart::Tilde(None) => {
            let home = env.vars.get("HOME").map(|s| s.to_string());
            let result = home.unwrap_or_else(|| "~".to_string());
            fields.last_mut().unwrap().push_quoted(&result);
        }
        WordPart::Tilde(Some(user)) => {
            let result = expand_tilde_user(user);
            fields.last_mut().unwrap().push_quoted(&result);
        }

        // ── Parameter expansion ───────────────────────────────────────────
        WordPart::Parameter(param) => {
            expand_param_to_fields(env, param, fields, in_double_quote);
        }

        // ── Command substitution ──────────────────────────────────────────
        WordPart::CommandSub(program) => {
            let output = command_sub::execute(env, program);
            if in_double_quote {
                fields.last_mut().unwrap().push_quoted(&output);
            } else {
                fields.last_mut().unwrap().push_unquoted(&output);
            }
        }

        // ── Arithmetic expansion ──────────────────────────────────────────
        WordPart::ArithSub(expr) => {
            let result = arith::evaluate(env, expr);
            if in_double_quote {
                fields.last_mut().unwrap().push_quoted(&result);
            } else {
                fields.last_mut().unwrap().push_unquoted(&result);
            }
        }
    }
}

/// Expand `~username` using `getpwnam`.
fn expand_tilde_user(user: &str) -> String {
    use std::ffi::CString;
    let c_user = match CString::new(user) {
        Ok(s) => s,
        Err(_) => return format!("~{}", user),
    };
    // SAFETY: getpwnam is reentrant enough for single-threaded shell use.
    let pw = unsafe { libc::getpwnam(c_user.as_ptr()) };
    if pw.is_null() {
        return format!("~{}", user);
    }
    let dir = unsafe { std::ffi::CStr::from_ptr((*pw).pw_dir) };
    dir.to_string_lossy().into_owned()
}

/// Expand a `ParamExpr` into `fields`.
fn expand_param_to_fields(
    env: &mut ShellEnv,
    param: &ParamExpr,
    fields: &mut Vec<ExpandedField>,
    in_double_quote: bool,
) {
    match param {
        // "$@" inside double quotes: each positional parameter becomes its own field.
        ParamExpr::Special(SpecialParam::At) if in_double_quote => {
            let params = env.positional_params.clone();
            if params.is_empty() {
                // "$@" with no params → produces nothing (not even an empty field)
                // Remove the last (empty) field if it is empty.
                if fields.last().map(|f| f.is_empty()).unwrap_or(false) {
                    fields.pop();
                }
                return;
            }
            for (i, p) in params.iter().enumerate() {
                if i == 0 {
                    fields.last_mut().unwrap().push_quoted(p);
                } else {
                    fields.push(ExpandedField::new());
                    fields.last_mut().unwrap().push_quoted(p);
                }
            }
        }

        // "$*" inside double quotes: join all positional params with IFS[0].
        ParamExpr::Special(SpecialParam::Star) if in_double_quote => {
            let sep = ifs_first_char(env);
            let joined = env.positional_params.join(&sep.to_string());
            fields.last_mut().unwrap().push_quoted(&joined);
        }

        // Everything else: expand to a string, then push.
        _ => {
            let value = param::expand(env, param);
            if in_double_quote {
                fields.last_mut().unwrap().push_quoted(&value);
            } else {
                fields.last_mut().unwrap().push_unquoted(&value);
            }
        }
    }
}

/// Return the first character of IFS, defaulting to space.
fn ifs_first_char(env: &ShellEnv) -> char {
    env.vars
        .get("IFS")
        .and_then(|s| s.chars().next())
        .unwrap_or(' ')
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;
    use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};

    fn make_env() -> ShellEnv {
        ShellEnv::new("kish", vec![])
    }

    // ── Basic pipeline ──

    #[test]
    fn test_expand_word_basic() {
        let mut env = make_env();
        let word = Word::literal("hello");
        assert_eq!(expand_word(&mut env, &word), vec!["hello"]);
    }

    #[test]
    fn test_expand_words_basic() {
        let mut env = make_env();
        env.vars.set("A", "foo").unwrap();
        let words = vec![
            Word::literal("hello"),
            Word {
                parts: vec![WordPart::Parameter(ParamExpr::Simple("A".to_string()))],
            },
        ];
        assert_eq!(expand_words(&mut env, &words), vec!["hello", "foo"]);
    }

    // ── "$@" splitting ──

    #[test]
    fn test_dollar_at_in_double_quotes_splits() {
        let mut env = ShellEnv::new(
            "kish",
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        // "$@"
        let word = Word {
            parts: vec![WordPart::DoubleQuoted(vec![WordPart::Parameter(
                ParamExpr::Special(SpecialParam::At),
            )])],
        };
        assert_eq!(expand_word(&mut env, &word), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_dollar_at_empty_params_produces_nothing() {
        let mut env = ShellEnv::new("kish", vec![]);
        let word = Word {
            parts: vec![WordPart::DoubleQuoted(vec![WordPart::Parameter(
                ParamExpr::Special(SpecialParam::At),
            )])],
        };
        let result = expand_word(&mut env, &word);
        assert!(result.is_empty(), "expected empty, got {:?}", result);
    }

    // ── "$*" joining ──

    #[test]
    fn test_dollar_star_in_double_quotes_joins() {
        let mut env = ShellEnv::new(
            "kish",
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        // IFS defaults to space; "$*" → "a b c"
        let word = Word {
            parts: vec![WordPart::DoubleQuoted(vec![WordPart::Parameter(
                ParamExpr::Special(SpecialParam::Star),
            )])],
        };
        assert_eq!(expand_word(&mut env, &word), vec!["a b c"]);
    }

    // ── ~root expansion ──

    #[test]
    fn test_tilde_root_starts_with_slash() {
        let mut env = make_env();
        let word = Word {
            parts: vec![WordPart::Tilde(Some("root".to_string()))],
        };
        let result = expand_word_to_string(&mut env, &word);
        // Either expands to a path starting with "/" or falls back to "~root"
        assert!(
            result.starts_with('/') || result == "~root",
            "unexpected tilde-root result: {}",
            result
        );
    }

    // ── Legacy tests (adapted to &mut env) ──

    #[test]
    fn test_literal() {
        let mut env = make_env();
        let word = Word::literal("hello");
        assert_eq!(expand_word_to_string(&mut env, &word), "hello");
    }

    #[test]
    fn test_single_quoted() {
        let mut env = make_env();
        let word = Word {
            parts: vec![WordPart::SingleQuoted("hello world".to_string())],
        };
        assert_eq!(expand_word_to_string(&mut env, &word), "hello world");
    }

    #[test]
    fn test_dollar_single_quoted() {
        let mut env = make_env();
        let word = Word {
            parts: vec![WordPart::DollarSingleQuoted("hello\\nworld".to_string())],
        };
        assert_eq!(
            expand_word_to_string(&mut env, &word),
            "hello\\nworld"
        );
    }

    #[test]
    fn test_double_quoted_literal() {
        let mut env = make_env();
        let word = Word {
            parts: vec![WordPart::DoubleQuoted(vec![WordPart::Literal(
                "hello".to_string(),
            )])],
        };
        assert_eq!(expand_word_to_string(&mut env, &word), "hello");
    }

    #[test]
    fn test_simple_param() {
        let mut env = make_env();
        env.vars.set("FOO", "bar").unwrap();
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Simple("FOO".to_string()))],
        };
        assert_eq!(expand_word_to_string(&mut env, &word), "bar");
    }

    #[test]
    fn test_unset_param() {
        let mut env = make_env();
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Simple(
                "UNSET_VAR_XYZ".to_string(),
            ))],
        };
        assert_eq!(expand_word_to_string(&mut env, &word), "");
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
        assert_eq!(expand_word_to_string(&mut env, &word), "42");
    }

    #[test]
    fn test_special_dollar() {
        let mut env = make_env();
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::Dollar))],
        };
        let result = expand_word_to_string(&mut env, &word);
        let pid: i32 = result.parse().expect("PID should be an integer");
        assert!(pid > 0);
    }

    #[test]
    fn test_special_zero() {
        let mut env = ShellEnv::new("mykish", vec![]);
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::Zero))],
        };
        assert_eq!(expand_word_to_string(&mut env, &word), "mykish");
    }

    #[test]
    fn test_positional_param() {
        let mut env = ShellEnv::new("kish", vec!["first".to_string(), "second".to_string()]);
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Positional(1))],
        };
        assert_eq!(expand_word_to_string(&mut env, &word), "first");
        let word2 = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Positional(2))],
        };
        assert_eq!(expand_word_to_string(&mut env, &word2), "second");
    }

    #[test]
    fn test_positional_out_of_range() {
        let mut env = ShellEnv::new("kish", vec!["only".to_string()]);
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Positional(5))],
        };
        assert_eq!(expand_word_to_string(&mut env, &word), "");
    }

    #[test]
    fn test_special_hash() {
        let mut env =
            ShellEnv::new("kish", vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::Hash))],
        };
        assert_eq!(expand_word_to_string(&mut env, &word), "3");
    }

    #[test]
    fn test_tilde_none() {
        let mut env = make_env();
        env.vars.set("HOME", "/home/user").unwrap();
        let word = Word {
            parts: vec![WordPart::Tilde(None)],
        };
        assert_eq!(expand_word_to_string(&mut env, &word), "/home/user");
    }

    #[test]
    fn test_tilde_none_no_home() {
        let mut env = make_env();
        let _ = env.vars.unset("HOME");
        let word = Word {
            parts: vec![WordPart::Tilde(None)],
        };
        assert_eq!(expand_word_to_string(&mut env, &word), "~");
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
        assert_eq!(expand_word_to_string(&mut env, &word), "hello world!");
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
        assert_eq!(expand_word_to_string(&mut env, &word), "value=42");
    }

    #[test]
    fn test_param_default() {
        let mut env = make_env();
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Default {
                name: "UNSET_VAR".to_string(),
                word: Some(Word::literal("default")),
                null_check: false,
            })],
        };
        assert_eq!(expand_word_to_string(&mut env, &word), "default");

        env.vars.set("UNSET_VAR", "actual").unwrap();
        assert_eq!(expand_word_to_string(&mut env, &word), "actual");
    }

    #[test]
    fn test_param_default_null_check() {
        let mut env = make_env();
        env.vars.set("EMPTY_VAR", "").unwrap();
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Default {
                name: "EMPTY_VAR".to_string(),
                word: Some(Word::literal("fallback")),
                null_check: true,
            })],
        };
        assert_eq!(expand_word_to_string(&mut env, &word), "fallback");

        let word2 = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Default {
                name: "EMPTY_VAR".to_string(),
                word: Some(Word::literal("fallback")),
                null_check: false,
            })],
        };
        assert_eq!(expand_word_to_string(&mut env, &word2), "");
    }
}
