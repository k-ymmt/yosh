pub mod arith;
pub mod command_sub;
pub mod field_split;
pub mod param;
pub mod pathname;
pub mod pattern;

mod heredoc;
mod pipeline;
mod scan;
mod tilde;

pub use heredoc::expand_body as expand_heredoc_body;
use pipeline::expand_word_to_fields;
pub(crate) use scan::skip_balanced_parens;
pub(crate) use tilde::expand_tilde_prefix;

use crate::env::ShellEnv;
use crate::parser::ast::Word;

// ─── ExpandedField ──────────────────────────────────────────────────────────

/// A word that has been through parameter/command/arithmetic expansion.
/// Each byte has two independent attribute bits:
///   - `split_protected_mask`: byte must NOT be split on an IFS character
///     (set for quoted bytes and for `Literal`-origin bytes).
///   - `glob_protected_mask`: byte must NOT be treated as a glob metachar
///     (set for quoted bytes only; `Literal`-origin bytes are glob-subject).
///
/// The two masks are independent. The POSIX byte classification is:
///   - `push_quoted`   → split-protected, glob-protected, was_quoted=true
///   - `push_literal`  → split-protected, glob-subject  (POSIX: literal text)
///   - `push_expanded` → split-subject,   glob-subject  (POSIX: $var, $(...), $((...)))
#[derive(Debug, Clone, PartialEq)]
pub struct ExpandedField {
    pub value: String,
    /// Packed bitset: 1 bit per byte. Bit set = protected from IFS splitting.
    split_protected_mask: Vec<u64>,
    /// Packed bitset: 1 bit per byte. Bit set = protected from glob expansion.
    glob_protected_mask: Vec<u64>,
    /// True if any quoting context applied to this field (even if value is empty).
    /// POSIX requires that quoted empty strings like `''` and `""` produce a
    /// zero-length field rather than being removed.
    pub was_quoted: bool,
}

impl ExpandedField {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            split_protected_mask: Vec::new(),
            glob_protected_mask: Vec::new(),
            was_quoted: false,
        }
    }

    /// True iff byte `i` must not be split on an IFS character.
    pub fn is_split_protected(&self, byte_index: usize) -> bool {
        bit_set(&self.split_protected_mask, byte_index)
    }

    /// True iff byte `i` must not be treated as a glob metacharacter.
    pub fn is_glob_protected(&self, byte_index: usize) -> bool {
        bit_set(&self.glob_protected_mask, byte_index)
    }

    /// Append `s` from a quoted context (single quotes, double quotes,
    /// escaped chars, tilde expansion). Bytes are protected from both
    /// field splitting and glob expansion; `was_quoted` becomes true.
    pub fn push_quoted(&mut self, s: &str) {
        let start = self.value.len();
        self.value.push_str(s);
        set_mask_range(&mut self.split_protected_mask, start, s.len());
        set_mask_range(&mut self.glob_protected_mask, start, s.len());
        self.was_quoted = true;
    }

    /// Append `s` from a literal (un-quoted, non-expansion) text part.
    /// Bytes are protected from field splitting (POSIX XCU §2.6.5 restricts
    /// splitting to expansion results only) but remain glob-subject so that
    /// a literal `*.rs` still triggers pathname expansion.
    pub fn push_literal(&mut self, s: &str) {
        let start = self.value.len();
        self.value.push_str(s);
        set_mask_range(&mut self.split_protected_mask, start, s.len());
        // glob_protected_mask intentionally not touched: literal bytes remain
        // glob-subject.
    }

    /// Append `s` from an expansion (parameter, command sub, arithmetic).
    /// Bytes are subject to both field splitting and glob expansion.
    pub fn push_expanded(&mut self, s: &str) {
        let start = self.value.len();
        self.value.push_str(s);
        // Neither mask updated: both default 0 (subject) bits are correct.
        // We still need to ensure mask length covers `value.len()` if a later
        // caller queries `is_*_protected` past the previous mask end —
        // the predicates fall back to false (subject) when reading past the
        // mask, so no explicit resize is needed for correctness.
        let _ = start;
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// Create a field with all bytes marked protected from both splitting
    /// and glob expansion (used for glob-match results that must not be
    /// re-split or re-globbed).
    pub fn all_quoted(value: String) -> Self {
        let len = value.len();
        let needed_words = len.div_ceil(64);
        let mask = vec![u64::MAX; needed_words];
        Self {
            value,
            split_protected_mask: mask.clone(),
            glob_protected_mask: mask,
            was_quoted: false,
        }
    }
}

#[inline]
fn bit_set(mask: &[u64], byte_index: usize) -> bool {
    let word = byte_index / 64;
    let bit = byte_index % 64;
    mask.get(word).is_some_and(|w| w & (1u64 << bit) != 0)
}

#[inline]
fn set_mask_range(mask: &mut Vec<u64>, start: usize, len: usize) {
    if len == 0 {
        return;
    }
    let end = start + len;
    let needed_words = end.div_ceil(64);
    if mask.len() < needed_words {
        mask.resize(needed_words, 0);
    }
    for i in start..end {
        mask[i / 64] |= 1u64 << (i % 64);
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

    #[test]
    fn push_literal_marks_split_protected_only() {
        let mut f = ExpandedField::new();
        f.push_literal("ab");
        assert!(f.is_split_protected(0));
        assert!(f.is_split_protected(1));
        assert!(!f.is_glob_protected(0));
        assert!(!f.is_glob_protected(1));
        assert!(!f.was_quoted);
    }

    #[test]
    fn push_quoted_marks_both_and_was_quoted() {
        let mut f = ExpandedField::new();
        f.push_quoted("ab");
        assert!(f.is_split_protected(0));
        assert!(f.is_split_protected(1));
        assert!(f.is_glob_protected(0));
        assert!(f.is_glob_protected(1));
        assert!(f.was_quoted);
    }

    #[test]
    fn push_expanded_marks_neither() {
        let mut f = ExpandedField::new();
        f.push_expanded("ab");
        assert!(!f.is_split_protected(0));
        assert!(!f.is_split_protected(1));
        assert!(!f.is_glob_protected(0));
        assert!(!f.is_glob_protected(1));
        assert!(!f.was_quoted);
    }

    #[test]
    fn mixed_push_per_byte_independence() {
        // L|E|Q sequence — verify each byte keeps its own attributes
        let mut f = ExpandedField::new();
        f.push_literal("a");
        f.push_expanded("b");
        f.push_quoted("c");
        assert_eq!(f.value, "abc");
        assert!(f.is_split_protected(0) && !f.is_glob_protected(0)); // a: literal
        assert!(!f.is_split_protected(1) && !f.is_glob_protected(1)); // b: expanded
        assert!(f.is_split_protected(2) && f.is_glob_protected(2)); // c: quoted
        assert!(f.was_quoted);
    }

    #[test]
    fn all_quoted_marks_both() {
        let f = ExpandedField::all_quoted("abc".to_string());
        for i in 0..3 {
            assert!(f.is_split_protected(i), "byte {i} split-protected", i = i);
            assert!(f.is_glob_protected(i), "byte {i} glob-protected", i = i);
        }
        assert!(!f.was_quoted);
    }
}
