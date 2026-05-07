pub mod arith;
pub mod command_sub;
pub mod field_split;
pub mod param;
pub mod pathname;
pub mod pattern;

mod heredoc;
mod scan;
mod tilde;

pub use heredoc::expand_body as expand_heredoc_body;
pub(crate) use scan::skip_balanced_parens;
pub(crate) use tilde::{expand_tilde_prefix, expand_tilde_user};

use crate::env::ShellEnv;
use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};

// ─── ExpandedField ──────────────────────────────────────────────────────────

/// A word that has been through parameter/command/arithmetic expansion.
/// Each byte has a corresponding bit in `quoted_mask`:
///   bit set   = came from a quoted context → protected from field splitting and glob.
///   bit clear = unquoted → subject to field splitting and pathname expansion.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpandedField {
    pub value: String,
    /// Packed bitset: 1 bit per byte of `value`. Bit set = quoted (protected).
    quoted_mask: Vec<u64>,
    /// True if any quoting context was applied to this field (even if value is empty).
    /// POSIX requires that quoted empty strings like `''` and `""` produce a
    /// zero-length field rather than being removed.
    pub was_quoted: bool,
}

impl ExpandedField {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            quoted_mask: Vec::new(),
            was_quoted: false,
        }
    }

    /// Check if byte at `byte_index` is quoted (protected from splitting/glob).
    pub fn is_quoted(&self, byte_index: usize) -> bool {
        let word = byte_index / 64;
        let bit = byte_index % 64;
        self.quoted_mask
            .get(word)
            .is_some_and(|w| w & (1u64 << bit) != 0)
    }

    /// Append `s` marking each byte as **quoted** (protected).
    pub fn push_quoted(&mut self, s: &str) {
        let start = self.value.len();
        self.value.push_str(s);
        self.set_range(start, s.len(), true);
        self.was_quoted = true;
    }

    /// Append `s` marking each byte as **unquoted** (splittable/globbable).
    pub fn push_unquoted(&mut self, s: &str) {
        let start = self.value.len();
        self.value.push_str(s);
        self.set_range(start, s.len(), false);
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// Create a field with all bytes marked as quoted.
    pub fn all_quoted(value: String) -> Self {
        let len = value.len();
        let needed_words = len.div_ceil(64);
        let mask = vec![u64::MAX; needed_words];
        Self {
            value,
            quoted_mask: mask,
            was_quoted: false,
        }
    }

    fn set_range(&mut self, start: usize, len: usize, quoted: bool) {
        if len == 0 {
            return;
        }
        let end = start + len;
        let needed_words = end.div_ceil(64);
        self.quoted_mask.resize(needed_words, 0);
        if quoted {
            for i in start..end {
                self.quoted_mask[i / 64] |= 1u64 << (i % 64);
            }
        }
        // unquoted: bits are already 0 from resize
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
pub fn expand_word(env: &mut ShellEnv, word: &Word) -> crate::error::Result<Vec<String>> {
    let fields = expand_word_to_fields(env, word)?;
    let fields = field_split::split(env, fields);
    let fields = if env.mode.options.noglob {
        fields
    } else {
        pathname::expand(env, fields)
    };
    Ok(fields
        .into_iter()
        .filter(|f| !f.is_empty() || f.was_quoted)
        .map(|f| f.value)
        .collect())
}

/// Expand a slice of `Word`s — each word is expanded independently,
/// then all resulting fields are concatenated.
pub fn expand_words(env: &mut ShellEnv, words: &[Word]) -> crate::error::Result<Vec<String>> {
    let mut result = Vec::new();
    for word in words {
        result.extend(expand_word(env, word)?);
    }
    Ok(result)
}

/// Expand a `Word` to a single `String`, suitable for assignments and
/// redirect targets (no field splitting, no glob).
pub fn expand_word_to_string(env: &mut ShellEnv, word: &Word) -> crate::error::Result<String> {
    let fields = expand_word_to_fields(env, word)?;
    // Concatenate all fields (there is normally only one here, but $@ inside
    // double quotes can produce multiple — join them with a space in that case).
    Ok(fields
        .into_iter()
        .map(|f| f.value)
        .collect::<Vec<_>>()
        .join(" "))
}

// ─── Stage 1: expand to ExpandedField list ──────────────────────────────────

/// Expand a `Word` into a list of `ExpandedField`s (before field splitting).
fn expand_word_to_fields(
    env: &mut ShellEnv,
    word: &Word,
) -> crate::error::Result<Vec<ExpandedField>> {
    let mut fields = vec![ExpandedField::new()];
    for part in &word.parts {
        expand_part_to_fields(env, part, &mut fields, false)?;
    }
    Ok(fields)
}

/// Expand one `WordPart`, appending into `fields`.
/// `in_double_quote` is true when we are inside `DoubleQuoted(...)`.
fn expand_part_to_fields(
    env: &mut ShellEnv,
    part: &WordPart,
    fields: &mut Vec<ExpandedField>,
    in_double_quote: bool,
) -> crate::error::Result<()> {
    match part {
        // ── Quoted literals ───────────────────────────────────────────────
        WordPart::Literal(s) => {
            if in_double_quote {
                fields.last_mut().unwrap().push_quoted(s);
            } else {
                fields.last_mut().unwrap().push_unquoted(s);
            }
        }
        WordPart::EscapedLiteral(s) => {
            // Expand identically to Literal — the escape served its purpose
            // at parse time by suppressing tilde recognition. The escape also
            // removes the "subject to field splitting" property, so escaped
            // text must be treated as quoted content for IFS purposes.
            fields.last_mut().unwrap().push_quoted(s);
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
            // Mark as quoted even when parts is empty (e.g. "")
            fields.last_mut().unwrap().was_quoted = true;
            for inner in parts {
                expand_part_to_fields(env, inner, fields, true)?;
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
            expand_param_to_fields(env, param, fields, in_double_quote)?;
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
        WordPart::ArithSub(expr) => match arith::evaluate(env, expr) {
            Ok(result) => {
                if in_double_quote {
                    fields.last_mut().unwrap().push_quoted(&result);
                } else {
                    fields.last_mut().unwrap().push_unquoted(&result);
                }
            }
            Err(msg) => {
                return Err(crate::error::ShellError::expansion(
                    crate::error::ExpansionErrorKind::InvalidArithmetic,
                    msg,
                ));
            }
        },
    }
    Ok(())
}

/// Expand a `ParamExpr` into `fields`.
fn expand_param_to_fields(
    env: &mut ShellEnv,
    param: &ParamExpr,
    fields: &mut Vec<ExpandedField>,
    in_double_quote: bool,
) -> crate::error::Result<()> {
    match param {
        // "$@" inside double quotes: each positional parameter becomes its own field.
        ParamExpr::Special(SpecialParam::At) if in_double_quote => {
            let params = env.vars.positional_params().to_vec();
            if params.is_empty() {
                // "$@" with no params → produces nothing (not even an empty field)
                // Remove the last (empty) field if it is empty.
                if fields.last().map(|f| f.is_empty()).unwrap_or(false) {
                    fields.pop();
                }
                return Ok(());
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
            let joined = env.vars.positional_params().join(&sep.to_string());
            fields.last_mut().unwrap().push_quoted(&joined);
        }

        // Unquoted $@: each positional parameter becomes its own field,
        // with content unquoted (subject to IFS splitting and glob).
        ParamExpr::Special(SpecialParam::At) if !in_double_quote => {
            let params = env.vars.positional_params().to_vec();
            if params.is_empty() {
                return Ok(());
            }
            for (i, p) in params.iter().enumerate() {
                if i == 0 {
                    fields.last_mut().unwrap().push_unquoted(p);
                } else {
                    fields.push(ExpandedField::new());
                    fields.last_mut().unwrap().push_unquoted(p);
                }
            }
        }

        // Everything else: expand to a string, then push.
        _ => {
            let value = param::expand(env, param)?;
            if in_double_quote {
                fields.last_mut().unwrap().push_quoted(&value);
            } else {
                fields.last_mut().unwrap().push_unquoted(&value);
            }
        }
    }
    Ok(())
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
        ShellEnv::new("yosh", vec![])
    }

    // ── Basic pipeline ──

    #[test]
    fn test_expand_word_basic() {
        let mut env = make_env();
        let word = Word::literal("hello");
        assert_eq!(expand_word(&mut env, &word).unwrap(), vec!["hello"]);
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
        assert_eq!(
            expand_words(&mut env, &words).unwrap(),
            vec!["hello", "foo"]
        );
    }

    // ── "$@" splitting ──

    #[test]
    fn test_dollar_at_in_double_quotes_splits() {
        let mut env = ShellEnv::new(
            "yosh",
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        // "$@"
        let word = Word {
            parts: vec![WordPart::DoubleQuoted(vec![WordPart::Parameter(
                ParamExpr::Special(SpecialParam::At),
            )])],
        };
        assert_eq!(expand_word(&mut env, &word).unwrap(), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_dollar_at_empty_params_produces_nothing() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let word = Word {
            parts: vec![WordPart::DoubleQuoted(vec![WordPart::Parameter(
                ParamExpr::Special(SpecialParam::At),
            )])],
        };
        let result = expand_word(&mut env, &word).unwrap();
        assert!(result.is_empty(), "expected empty, got {:?}", result);
    }

    // ── Unquoted $@ splitting ──

    #[test]
    fn test_unquoted_dollar_at_splits_per_param() {
        let mut env = ShellEnv::new(
            "yosh",
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        // Unquoted $@ — each positional param becomes its own field
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::At))],
        };
        let fields = expand_word_to_fields(&mut env, &word).unwrap();
        assert_eq!(fields.len(), 3, "expected 3 fields, got {:?}", fields);
        assert_eq!(fields[0].value, "a");
        assert_eq!(fields[1].value, "b");
        assert_eq!(fields[2].value, "c");
        // All bytes should be unquoted (subject to IFS splitting)
        assert!((0..fields[0].value.len()).all(|i| !fields[0].is_quoted(i)));
        assert!((0..fields[1].value.len()).all(|i| !fields[1].is_quoted(i)));
        assert!((0..fields[2].value.len()).all(|i| !fields[2].is_quoted(i)));
    }

    #[test]
    fn test_unquoted_dollar_at_empty_produces_nothing() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::At))],
        };
        let fields = expand_word_to_fields(&mut env, &word).unwrap();
        assert!(
            fields.len() <= 1,
            "expected 0 or 1 fields, got {:?}",
            fields
        );
    }

    // ── "$*" joining ──

    #[test]
    fn test_dollar_star_in_double_quotes_joins() {
        let mut env = ShellEnv::new(
            "yosh",
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        // IFS defaults to space; "$*" → "a b c"
        let word = Word {
            parts: vec![WordPart::DoubleQuoted(vec![WordPart::Parameter(
                ParamExpr::Special(SpecialParam::Star),
            )])],
        };
        assert_eq!(expand_word(&mut env, &word).unwrap(), vec!["a b c"]);
    }

    // ── ~root expansion ──

    #[test]
    fn test_tilde_root_starts_with_slash() {
        let mut env = make_env();
        let word = Word {
            parts: vec![WordPart::Tilde(Some("root".to_string()))],
        };
        let result = expand_word_to_string(&mut env, &word).unwrap();
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
        assert_eq!(expand_word_to_string(&mut env, &word).unwrap(), "hello");
    }

    #[test]
    fn test_single_quoted() {
        let mut env = make_env();
        let word = Word {
            parts: vec![WordPart::SingleQuoted("hello world".to_string())],
        };
        assert_eq!(
            expand_word_to_string(&mut env, &word).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn test_dollar_single_quoted() {
        let mut env = make_env();
        let word = Word {
            parts: vec![WordPart::DollarSingleQuoted("hello\\nworld".to_string())],
        };
        assert_eq!(
            expand_word_to_string(&mut env, &word).unwrap(),
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
        assert_eq!(expand_word_to_string(&mut env, &word).unwrap(), "hello");
    }

    #[test]
    fn test_simple_param() {
        let mut env = make_env();
        env.vars.set("FOO", "bar").unwrap();
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Simple("FOO".to_string()))],
        };
        assert_eq!(expand_word_to_string(&mut env, &word).unwrap(), "bar");
    }

    #[test]
    fn test_unset_param() {
        let mut env = make_env();
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Simple(
                "UNSET_VAR_XYZ".to_string(),
            ))],
        };
        assert_eq!(expand_word_to_string(&mut env, &word).unwrap(), "");
    }

    #[test]
    fn test_special_question() {
        let mut env = make_env();
        env.exec.last_exit_status = 42;
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(
                SpecialParam::Question,
            ))],
        };
        assert_eq!(expand_word_to_string(&mut env, &word).unwrap(), "42");
    }

    #[test]
    fn test_special_dollar() {
        let mut env = make_env();
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(
                SpecialParam::Dollar,
            ))],
        };
        let result = expand_word_to_string(&mut env, &word).unwrap();
        let pid: i32 = result.parse().expect("PID should be an integer");
        assert!(pid > 0);
    }

    #[test]
    fn test_special_zero() {
        let mut env = ShellEnv::new("myyosh", vec![]);
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::Zero))],
        };
        assert_eq!(expand_word_to_string(&mut env, &word).unwrap(), "myyosh");
    }

    #[test]
    fn test_positional_param() {
        let mut env = ShellEnv::new("yosh", vec!["first".to_string(), "second".to_string()]);
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Positional(1))],
        };
        assert_eq!(expand_word_to_string(&mut env, &word).unwrap(), "first");
        let word2 = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Positional(2))],
        };
        assert_eq!(expand_word_to_string(&mut env, &word2).unwrap(), "second");
    }

    #[test]
    fn test_positional_out_of_range() {
        let mut env = ShellEnv::new("yosh", vec!["only".to_string()]);
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Positional(5))],
        };
        assert_eq!(expand_word_to_string(&mut env, &word).unwrap(), "");
    }

    #[test]
    fn test_special_hash() {
        let mut env = ShellEnv::new(
            "yosh",
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::Hash))],
        };
        assert_eq!(expand_word_to_string(&mut env, &word).unwrap(), "3");
    }

    #[test]
    fn test_tilde_none() {
        let mut env = make_env();
        env.vars.set("HOME", "/home/user").unwrap();
        let word = Word {
            parts: vec![WordPart::Tilde(None)],
        };
        assert_eq!(
            expand_word_to_string(&mut env, &word).unwrap(),
            "/home/user"
        );
    }

    #[test]
    fn test_tilde_none_no_home() {
        let mut env = make_env();
        let _ = env.vars.unset("HOME");
        let word = Word {
            parts: vec![WordPart::Tilde(None)],
        };
        assert_eq!(expand_word_to_string(&mut env, &word).unwrap(), "~");
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
        assert_eq!(
            expand_word_to_string(&mut env, &word).unwrap(),
            "hello world!"
        );
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
        assert_eq!(expand_word_to_string(&mut env, &word).unwrap(), "value=42");
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
        assert_eq!(expand_word_to_string(&mut env, &word).unwrap(), "default");

        env.vars.set("UNSET_VAR", "actual").unwrap();
        assert_eq!(expand_word_to_string(&mut env, &word).unwrap(), "actual");
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
        assert_eq!(expand_word_to_string(&mut env, &word).unwrap(), "fallback");

        let word2 = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Default {
                name: "EMPTY_VAR".to_string(),
                word: Some(Word::literal("fallback")),
                null_check: false,
            })],
        };
        assert_eq!(expand_word_to_string(&mut env, &word2).unwrap(), "");
    }
}
