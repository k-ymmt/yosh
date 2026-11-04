pub mod arith;
pub mod command_sub;
pub mod field_split;
pub mod param;
pub mod pathname;
pub mod pattern;

mod dollar;
mod field;
mod heredoc;
mod pipeline;
mod scalar;
// pub(crate): `interactive::completion::segment_start` reuses the
// quote-aware balanced-paren scanner to skip `$(...)` command
// substitutions when finding pipeline-segment boundaries.
pub(crate) mod scan;
mod tilde;

pub use field::ExpandedField;
pub use heredoc::expand_body as expand_heredoc_body;
use pipeline::expand_word_to_fields;
use scalar::scalar_word;
pub(crate) use tilde::expand_tilde_prefix;

use crate::env::ShellEnv;
use crate::parser::ast::{Word, WordPart};

/// Expand a single `Word` through the full POSIX pipeline:
///   1. Parameter / command-sub / arithmetic expansion
///   2. Field splitting (IFS)
///   3. Pathname expansion (glob)
///   4. Quote removal  ← callers receive plain `String`s
#[allow(dead_code)] // library API (benches, tests); the shell uses `expand_words`
pub fn expand_word(env: &mut ShellEnv, word: &Word) -> crate::error::Result<Vec<String>> {
    let mut out = Vec::new();
    expand_word_into(env, word, &mut out)?;
    Ok(out)
}

/// [`expand_word`], appending the resulting fields to `out`.
fn expand_word_into(
    env: &mut ShellEnv,
    word: &Word,
    out: &mut Vec<String>,
) -> crate::error::Result<()> {
    if let Some(r) = scalar_word(env, word, false) {
        let (text, quoted) = r?;
        if !text.is_empty() || quoted {
            out.push(text);
        }
        return Ok(());
    }
    let fields = expand_word_to_fields(env, word)?;
    let fields = field_split::split(env, fields);
    let fields = if env.mode.options.noglob {
        fields
    } else {
        pathname::expand(env, fields)
    };
    out.extend(
        fields
            .into_iter()
            .filter(|f| !f.is_empty() || f.was_quoted)
            .map(ExpandedField::into_string),
    );
    Ok(())
}

/// Expand a slice of `Word`s — each word is expanded independently,
/// then all resulting fields are concatenated.
pub fn expand_words(env: &mut ShellEnv, words: &[Word]) -> crate::error::Result<Vec<String>> {
    let mut result = Vec::with_capacity(words.len());
    for word in words {
        expand_word_into(env, word, &mut result)?;
    }
    Ok(result)
}

/// Expand a `Word` to a single `String`, suitable for assignments and
/// redirect targets (no field splitting, no glob).
pub fn expand_word_to_string(env: &mut ShellEnv, word: &Word) -> crate::error::Result<String> {
    if let Some(r) = scalar_word(env, word, true) {
        return r.map(|(text, _)| text);
    }
    let fields = expand_word_to_fields(env, word)?;
    // Concatenate all fields (there is normally only one here, but $@/$*
    // can produce multiple). Each field's `scalar_join_sep` names the
    // separator preceding it: IFS[0] for `$*`'s fields, space otherwise.
    let mut result = String::new();
    for (i, f) in fields.into_iter().enumerate() {
        if i > 0 {
            result.push_str(f.scalar_separator());
        }
        result.push_str(&f.into_string());
    }
    Ok(result)
}

/// Expand a `Word` to a single pattern `String` for `case` matching and
/// the `${x%pat}` / `${x#pat}` strip forms (no field splitting, no glob).
///
/// Like `expand_word_to_string`, but glob metacharacters that originate
/// from quoted text (glob-protected bytes in the `ExpandedField` masks)
/// are backslash-escaped so the pattern matcher treats them literally —
/// POSIX §2.6.2 / §2.13.1: quoting prevents pattern characters from being
/// treated as such. The matcher (`pattern::matches`) already honors `\x`
/// escapes. Metachar bytes are ASCII, so byte-indexed mask lookups are
/// UTF-8 safe.
pub fn expand_word_to_pattern(env: &mut ShellEnv, word: &Word) -> crate::error::Result<String> {
    // Unquoted literal text is used as-is (nothing to escape).
    if let [WordPart::Literal(s)] = word.parts.as_slice() {
        return Ok(s.clone());
    }
    let fields = expand_word_to_fields(env, word)?;
    let mut result = String::new();
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            result.push_str(f.scalar_separator());
        }
        for (bi, ch) in f.value.char_indices() {
            // `]` is escaped too: a quoted `]` must not close a bracket
            // expression opened by an unquoted `[` (POSIX §2.13.1 —
            // `sr[c"]"` stays literal and does not match `src`). The
            // matcher's `parse_bracket` treats `\]` as a literal member
            // that cannot close the class, mirroring the mask-aware
            // closer scan in `pathname::bracket_has_close`.
            if pattern::needs_pattern_escape(ch) && f.is_glob_protected(bi) {
                result.push('\\');
            }
            result.push(ch);
        }
    }
    Ok(result)
}

/// Expand a prompt-style string (PS1/PS2/PS4): parameter expansion,
/// command substitution and arithmetic expansion; no field splitting,
/// no pathname expansion, no tilde expansion.
///
/// `raw` is scanned as the body of a here-document rather than wrapped
/// in `"..."` and lexed as a word: a `"` in the value is an ordinary
/// character (`PS4='say "hi" + '` traces `say "hi" + ...`), and `\`
/// escapes only `$`, `` ` ``, `\` and newline — dash's `expandstr`
/// semantics.
///
/// Prompt expansion is a side channel of the command being read or
/// traced, so it must not disturb the shell state that command sees:
/// `$?` survives a command substitution in the prompt (`PS1='$(true)$ '`
/// keeps the status of the previous command), and an expansion error
/// (`set -u` with an unset variable, `${x?msg}`, a bad arithmetic
/// expression) is diagnosed but neither aborts the shell nor leaves a
/// pending `FlowControl::ExpansionError` that would cancel the next
/// command. On error the raw value is used literally, as dash and bash
/// display it.
pub fn expand_dquoted(env: &mut ShellEnv, raw: &str) -> String {
    let saved_status = env.exec.last_exit_status;
    let saved_flow = env.exec.flow_control.take();
    let result = dollar::expand_string(env, raw);
    let raised = env.exec.flow_control.take();
    env.exec.flow_control = saved_flow;
    env.exec.last_exit_status = saved_status;
    match result {
        // `${x?msg}` / nounset failures print their own diagnostic and
        // raise ExpansionError instead of returning Err.
        Ok(_) if raised == Some(crate::env::FlowControl::ExpansionError) => raw.to_string(),
        Ok(s) => s,
        Err(e) => {
            crate::byteenc::write_stderr_decoded_line(&e.to_string());
            raw.to_string()
        }
    }
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
    fn expand_dquoted_expands_parameter() {
        let mut env = make_env();
        env.vars.set("x", "hi").unwrap();
        assert_eq!(expand_dquoted(&mut env, "v=$x"), "v=hi");
    }

    #[test]
    fn expand_dquoted_unset_param_is_empty() {
        let mut env = make_env();
        assert_eq!(expand_dquoted(&mut env, "[$nope]"), "[]");
    }

    #[test]
    fn expand_dquoted_plain_literal_unchanged() {
        let mut env = make_env();
        assert_eq!(expand_dquoted(&mut env, "+ "), "+ ");
    }

    // Command substitution that produces stdout (e.g. `$(echo ok)`) cannot be
    // asserted here: libtest captures the forked builtin's Rust-level stdout
    // before it reaches the OS pipe, so the substitution reads back empty. The
    // path works at runtime (verified via `echo "x=$(echo ok)"`) and exit-status
    // propagation is covered by the exec/simple command-sub tests.

    #[test]
    fn expand_dquoted_arithmetic() {
        let mut env = make_env();
        assert_eq!(expand_dquoted(&mut env, "$((1+2))"), "3");
    }

    #[test]
    fn expand_dquoted_double_quote_is_literal() {
        // Here-document-like body: `"` is ordinary, `\` escapes only
        // `$ \` backtick newline (dash expandstr semantics).
        let mut env = make_env();
        env.vars.set("x", "v").unwrap();
        assert_eq!(expand_dquoted(&mut env, "say \"hi\" + "), "say \"hi\" + ");
        assert_eq!(expand_dquoted(&mut env, r#"a\"b\$x $x"#), r#"a\"b$x v"#);
    }

    #[test]
    fn expand_dquoted_nounset_error_is_contained() {
        let mut env = make_env();
        env.mode.options.nounset = true;
        env.exec.last_exit_status = 7;
        assert_eq!(expand_dquoted(&mut env, "$NOPE> "), "$NOPE> ");
        assert_eq!(env.exec.flow_control, None);
        assert_eq!(env.exec.last_exit_status, 7);
    }

    #[test]
    fn expand_dquoted_param_error_is_contained() {
        let mut env = make_env();
        assert_eq!(expand_dquoted(&mut env, "${u?boom} "), "${u?boom} ");
        assert_eq!(env.exec.flow_control, None);
        assert_eq!(expand_dquoted(&mut env, "$((1/0)) "), "$((1/0)) ");
        assert_eq!(env.exec.flow_control, None);
    }

    #[test]
    fn expand_dquoted_preserves_exit_status() {
        let mut env = make_env();
        env.exec.last_exit_status = 1;
        let _ = expand_dquoted(&mut env, "$(true)P$ ");
        assert_eq!(env.exec.last_exit_status, 1);
    }

    // ── expand_word_to_pattern: quoted metachars match literally (H2) ──

    #[test]
    fn pattern_quoted_star_is_escaped() {
        let mut env = make_env();
        let word = Word {
            parts: vec![WordPart::DoubleQuoted(vec![WordPart::Literal(
                "a*".to_string(),
            )])],
        };
        assert_eq!(expand_word_to_pattern(&mut env, &word).unwrap(), "a\\*");
    }

    #[test]
    fn pattern_literal_star_stays_glob() {
        let mut env = make_env();
        let word = Word::literal("a*");
        assert_eq!(expand_word_to_pattern(&mut env, &word).unwrap(), "a*");
    }

    #[test]
    fn pattern_quoted_expansion_is_escaped() {
        let mut env = make_env();
        env.vars.set("p", "*?[").unwrap();
        // "$p" — quoted parameter expansion: all metachars protected
        let word = Word {
            parts: vec![WordPart::DoubleQuoted(vec![WordPart::Parameter(
                ParamExpr::Simple("p".to_string()),
            )])],
        };
        assert_eq!(
            expand_word_to_pattern(&mut env, &word).unwrap(),
            "\\*\\?\\["
        );
    }

    #[test]
    fn pattern_unquoted_expansion_stays_glob() {
        let mut env = make_env();
        env.vars.set("p", "*").unwrap();
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Simple("p".to_string()))],
        };
        assert_eq!(expand_word_to_pattern(&mut env, &word).unwrap(), "*");
    }

    #[test]
    fn pattern_quoted_close_bracket_is_escaped() {
        let mut env = make_env();
        // sr[c"]" — unquoted `sr[c` + quoted `]`: the quoted `]` must be
        // escaped so it cannot close the unquoted `[` (POSIX §2.13.1;
        // bash: `case src in sr[c"]") ...` does NOT match).
        let word = Word {
            parts: vec![
                WordPart::Literal("sr[c".to_string()),
                WordPart::DoubleQuoted(vec![WordPart::Literal("]".to_string())]),
            ],
        };
        let pat = expand_word_to_pattern(&mut env, &word).unwrap();
        assert_eq!(pat, "sr[c\\]");
        // Renderer and matcher agree: the rendered pattern is literal.
        assert!(!pattern::matches(&pat, "src"));
        assert!(pattern::matches(&pat, "sr[c]"));
    }

    #[test]
    fn pattern_unquoted_close_bracket_not_escaped() {
        let mut env = make_env();
        let word = Word::literal("sr[c]");
        let pat = expand_word_to_pattern(&mut env, &word).unwrap();
        assert_eq!(pat, "sr[c]");
        assert!(pattern::matches(&pat, "src"));
    }

    #[test]
    fn pattern_multibyte_offsets_stay_aligned() {
        let mut env = make_env();
        // Quoted multibyte text followed by a quoted star: the mask is
        // byte-indexed, so the star's byte offset must be checked, not
        // its char offset.
        let word = Word {
            parts: vec![WordPart::DoubleQuoted(vec![WordPart::Literal(
                "日本*".to_string(),
            )])],
        };
        assert_eq!(expand_word_to_pattern(&mut env, &word).unwrap(), "日本\\*");
    }
}
