//! Parameter expansion (POSIX §2.6.2): dispatch on the `ParamExpr`
//! form, and the fatal expansion errors it reports. Name lookup lives
//! in `lookup`, the pattern-removal forms in `strip`.

mod lookup;
mod strip;

use std::borrow::Cow;

use super::{expand_word_to_pattern, expand_word_to_string};
use crate::env::ShellEnv;
use crate::parser::ast::{ParamExpr, SpecialParam};
use lookup::{expand_special, lookup_var_nounset};
pub(super) use lookup::{
    is_unassignable_param, is_unset_or_null_inner, lookup_var, set_scalar_value,
};
pub(super) use strip::apply_strip;

/// Report a fatal expansion error (POSIX §2.8.1): print the diagnostic,
/// set `$?` to 1, and raise `FlowControl::ExpansionError` so the current
/// command is aborted, function-call boundaries do not swallow it, and a
/// non-interactive shell exits with status 1 (interactive shells abort
/// the current command line only).
pub(super) fn expansion_error(env: &mut ShellEnv, msg: std::fmt::Arguments<'_>) {
    eprintln!("yosh: {msg}");
    env.exec.last_exit_status = 1;
    env.exec.flow_control = Some(crate::env::FlowControl::ExpansionError);
}

/// The `set -u` expansion error for the unset parameter `name`.
fn parameter_not_set(env: &mut ShellEnv, name: impl std::fmt::Display) {
    expansion_error(env, format_args!("{}: parameter not set", name));
}

/// Expand a `ParamExpr` to a String.
pub fn expand(env: &mut ShellEnv, param: &ParamExpr) -> crate::error::Result<String> {
    // A form with a word expands that word recursively, and the parser
    // allows hundreds of `${x:-${x:-...}}` levels: past the end of the
    // stack fail with an expansion error instead of overflowing (an
    // `eval` deep in a recursive function starts with little stack left).
    if !matches!(
        param,
        ParamExpr::Simple(_)
            | ParamExpr::Positional(_)
            | ParamExpr::Special(_)
            | ParamExpr::Length(_)
    ) && crate::exec::stack_guard::descent_stack_exhausted()
    {
        expansion_error(env, format_args!("maximum nesting level exceeded"));
        return Ok(String::new());
    }
    match param {
        // ── Simple variable ──────────────────────────────────────────────
        ParamExpr::Simple(name) => match lookup_var(env, name) {
            Some(val) => Ok(val.into_owned()),
            None => {
                if env.mode.options.nounset {
                    parameter_not_set(env, name);
                }
                Ok(String::new())
            }
        },

        // ── Positional parameters ────────────────────────────────────────
        ParamExpr::Positional(n) => {
            if *n > 0 {
                match env.vars.positional_params().get(n - 1).cloned() {
                    Some(val) => Ok(val),
                    None => {
                        // POSIX set -u: an unset positional parameter is an
                        // expansion error, same as an unset variable.
                        if env.mode.options.nounset {
                            parameter_not_set(env, n);
                        }
                        Ok(String::new())
                    }
                }
            } else {
                Ok(String::new())
            }
        }

        // ── Special parameters ───────────────────────────────────────────
        ParamExpr::Special(sp) => {
            // POSIX set -u: `$!` is the only special parameter that can be
            // unset (no asynchronous list has run yet). `@`/`*` are exempt
            // by §2.5.2; the rest are always set.
            if env.mode.options.nounset
                && matches!(sp, SpecialParam::Bang)
                && env.process.jobs.last_bg_pid().is_none()
            {
                parameter_not_set(env, "!");
                return Ok(String::new());
            }
            Ok(expand_special(env, sp))
        }

        // ── ${#name} — character count ───────────────────────────────────
        ParamExpr::Length(name) => {
            let Ok(val) = lookup_var_nounset(env, name) else {
                return Ok(String::new());
            };
            let len = val.map(|v| v.chars().count()).unwrap_or(0);
            Ok(len.to_string())
        }

        // ── ${name:-word} / ${name-word} ─────────────────────────────────
        ParamExpr::Default {
            name,
            word,
            null_check,
        } => {
            let val = lookup_var(env, name);
            let is_unset_or_null = is_unset_or_null_inner(val.as_deref(), *null_check);
            if is_unset_or_null {
                match word.as_ref() {
                    Some(w) => expand_word_to_string(env, w),
                    None => Ok(String::new()),
                }
            } else {
                Ok(val.map(Cow::into_owned).unwrap_or_default())
            }
        }

        // ── ${name:=word} / ${name=word} ─────────────────────────────────
        ParamExpr::Assign {
            name,
            word,
            null_check,
        } => {
            let val = lookup_var(env, name);
            let is_unset_or_null = is_unset_or_null_inner(val.as_deref(), *null_check);
            if is_unset_or_null {
                // POSIX §2.6.2: attempting to assign a positional or special
                // parameter with ${name=word} is an error.
                if is_unassignable_param(name) {
                    expansion_error(env, format_args!("{}: cannot assign in this way", name));
                    return Ok(String::new());
                }
                let new_val = match word.as_ref() {
                    Some(w) => expand_word_to_string(env, w)?,
                    None => String::new(),
                };
                // LINENO is a computed pseudo-variable (see `lookup_var`);
                // assigning it here would resurrect a real `VarStore`
                // entry and thrash the environ cache on every command.
                // Matches bash: `${LINENO:=x}` reads the current line but
                // does not make the assignment stick.
                if name != "LINENO" {
                    // assign_var (not vars.set): invalidates the utility
                    // hash when PATH is assigned via `${PATH:=...}`.
                    // A readonly target is an expansion error, like
                    // `${x?}` (dash/bash: `r: is read only`).
                    if let Err(msg) = env.assign_var(name, new_val.as_str()) {
                        expansion_error(env, format_args!("{msg}"));
                        return Ok(String::new());
                    }
                }
                Ok(new_val)
            } else {
                Ok(val.map(Cow::into_owned).unwrap_or_default())
            }
        }

        // ── ${name:?word} / ${name?word} ─────────────────────────────────
        ParamExpr::Error {
            name,
            word,
            null_check,
        } => {
            let val = lookup_var(env, name);
            let is_unset_or_null = is_unset_or_null_inner(val.as_deref(), *null_check);
            if is_unset_or_null {
                let msg = match word.as_ref() {
                    Some(w) => expand_word_to_string(env, w)?,
                    None => "parameter null or not set".to_string(),
                };
                // POSIX: non-interactive shell shall exit with non-zero
                // status. Every other shell prefixes the variable name.
                expansion_error(env, format_args!("{}: {}", name, msg));
                Ok(String::new())
            } else {
                Ok(val.map(Cow::into_owned).unwrap_or_default())
            }
        }

        // ── ${name:+word} / ${name+word} ─────────────────────────────────
        ParamExpr::Alt {
            name,
            word,
            null_check,
        } => {
            let val = lookup_var(env, name);
            let is_unset_or_null = is_unset_or_null_inner(val.as_deref(), *null_check);
            if is_unset_or_null {
                // Not set (or null with colon) — return empty
                Ok(String::new())
            } else {
                // Set and non-null — return the word
                match word.as_ref() {
                    Some(w) => expand_word_to_string(env, w),
                    None => Ok(String::new()),
                }
            }
        }

        // The four strip forms expand their pattern via
        // `expand_word_to_pattern` so quoted glob metacharacters match
        // literally (POSIX §2.6.2: "the quoting shall prevent the pattern
        // characters from being treated as pattern characters").

        // ${name%pattern}, ${name%%pattern}, ${name#pattern},
        // ${name##pattern}: remove the shortest / longest matching suffix
        // or prefix.
        ParamExpr::StripShortSuffix(name, pattern_word)
        | ParamExpr::StripLongSuffix(name, pattern_word)
        | ParamExpr::StripShortPrefix(name, pattern_word)
        | ParamExpr::StripLongPrefix(name, pattern_word) => {
            // `${@#pat}` / `${*%pat}` (unspecified by POSIX): as in dash
            // and bash, each positional parameter loses the pattern, then
            // they are joined the way `$@` / `$*` join here.
            if let Some(star) = positional_name(name) {
                let pat = expand_word_to_pattern(env, pattern_word)?;
                let sep = if star {
                    super::pipeline::ifs_join_separator(env)
                } else {
                    " ".to_string()
                };
                let parts: Vec<String> = env
                    .vars
                    .positional_params()
                    .iter()
                    .map(|p| apply_strip(param, p, &pat))
                    .collect();
                return Ok(parts.join(&sep));
            }
            let Ok(value) = lookup_var_nounset(env, name) else {
                return Ok(String::new());
            };
            // Owned: expanding the pattern may assign the variable
            // (`${x#${x=y}}`), and the value read before it is the one
            // the pattern is removed from.
            let value = value.map(Cow::into_owned).unwrap_or_default();
            let pat = expand_word_to_pattern(env, pattern_word)?;
            Ok(apply_strip(param, &value, &pat))
        }
    }
}

/// `Some(is_star)` when `name` is `@` or `*`.
fn positional_name(name: &str) -> Option<bool> {
    match name {
        "*" => Some(true),
        "@" => Some(false),
        _ => None,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;
    use crate::parser::ast::{ParamExpr, SpecialParam, Word};

    fn make_env() -> ShellEnv {
        ShellEnv::new("yosh", vec![])
    }

    #[test]
    fn set_scalar_value_agrees_with_expand() {
        let mut env = ShellEnv::new("yosh", vec!["p1".to_string(), String::new()]);
        env.vars.set("x", "val").unwrap();
        env.vars.set("empty", "").unwrap();
        env.exec.lineno = 7;
        let simple = |n: &str| ParamExpr::Simple(n.to_string());
        for p in [
            simple("x"),
            simple("empty"),
            simple("LINENO"),
            simple("1"),
            simple("01"),
            simple("0"),
            simple("?"),
            ParamExpr::Positional(1),
            ParamExpr::Positional(2),
        ] {
            let got = set_scalar_value(&env, &p).map(Cow::into_owned);
            assert_eq!(got, Some(expand(&mut env, &p).unwrap()), "{p:?}");
        }
        // Unset or other forms take the general path (`set -u` there).
        for p in [
            simple("unset"),
            simple("3"),
            ParamExpr::Positional(0),
            ParamExpr::Positional(3),
            ParamExpr::Length("x".to_string()),
            ParamExpr::Special(SpecialParam::Question),
        ] {
            assert_eq!(set_scalar_value(&env, &p), None, "{p:?}");
        }
    }

    // ── PATH cache invalidation through ${PATH:=word} (SP2) ──

    #[test]
    fn assign_expansion_to_path_clears_utility_hash() {
        let mut env = make_env();
        env.vars.unset("PATH").unwrap();
        env.utility_hash.insert(
            "foo".to_string(),
            crate::env::HashEntry::new(std::path::PathBuf::from("/bin/foo")),
        );
        let expr = ParamExpr::Assign {
            name: "PATH".to_string(),
            word: Some(Word::literal("/newpath")),
            null_check: false,
        };
        assert_eq!(expand(&mut env, &expr).unwrap(), "/newpath");
        assert_eq!(env.vars.get("PATH"), Some("/newpath"));
        assert!(
            env.utility_hash.is_empty(),
            "${{PATH:=...}} must invalidate the utility hash"
        );
    }

    // ── Simple ──
    #[test]
    fn test_simple_set() {
        let mut env = make_env();
        env.vars.set("FOO", "bar").unwrap();
        assert_eq!(
            expand(&mut env, &ParamExpr::Simple("FOO".to_string())).unwrap(),
            "bar"
        );
    }

    #[test]
    fn test_simple_unset() {
        let mut env = make_env();
        assert_eq!(
            expand(&mut env, &ParamExpr::Simple("UNSET_XYZ".to_string())).unwrap(),
            ""
        );
    }

    // ── LINENO (computed pseudo-variable, TODO PERF item 1) ──

    #[test]
    fn test_lineno_reads_exec_state_not_varstore() {
        let mut env = make_env();
        env.exec.lineno = 42;
        assert_eq!(
            expand(&mut env, &ParamExpr::Simple("LINENO".to_string())).unwrap(),
            "42"
        );
        // Must not have materialized a real VarStore entry.
        assert_eq!(env.vars.get("LINENO"), None);
    }

    #[test]
    fn test_lineno_length() {
        let mut env = make_env();
        env.exec.lineno = 12345;
        assert_eq!(
            expand(&mut env, &ParamExpr::Length("LINENO".to_string())).unwrap(),
            "5"
        );
    }

    #[test]
    fn test_lineno_default_form_reads_current_value() {
        // ${LINENO:-x} — LINENO is always "set", so the default word is
        // never used.
        let mut env = make_env();
        env.exec.lineno = 7;
        let result = expand(
            &mut env,
            &ParamExpr::Default {
                name: "LINENO".to_string(),
                word: Some(Word::literal("fallback")),
                null_check: true,
            },
        )
        .unwrap();
        assert_eq!(result, "7");
    }

    #[test]
    fn test_lineno_assign_form_does_not_persist() {
        // ${LINENO:=x} must not create a real VarStore entry (would
        // defeat the environ-cache gating).
        //
        // Coverage note: `lookup_var` always returns a non-empty value
        // for LINENO (even line 0 stringifies to "0"), so the Assign
        // arm's write branch is unreachable for LINENO and this test
        // exercises only the read path — it passes with or without the
        // `name != "LINENO"` guard, which is defense-in-depth. The
        // reachable assignment path (arithmetic `$((LINENO=...))`) is
        // covered by expand::arith::tests::
        // test_lineno_arith_does_not_persist_assignment.
        let mut env = make_env();
        env.exec.lineno = 3;
        let result = expand(
            &mut env,
            &ParamExpr::Assign {
                name: "LINENO".to_string(),
                word: Some(Word::literal("ignored")),
                null_check: true,
            },
        )
        .unwrap();
        assert_eq!(result, "3");
        assert_eq!(env.vars.get("LINENO"), None);
    }

    // ── Assign (${name:=word}) ──
    #[test]
    fn test_assign_unset_assigns_and_returns() {
        let mut env = make_env();
        let result = expand(
            &mut env,
            &ParamExpr::Assign {
                name: "MYVAR".to_string(),
                word: Some(Word::literal("default_val")),
                null_check: false,
            },
        )
        .unwrap();
        assert_eq!(result, "default_val");
        assert_eq!(env.vars.get("MYVAR"), Some("default_val"));
    }

    #[test]
    fn test_assign_set_keeps_and_returns() {
        let mut env = make_env();
        env.vars.set("MYVAR", "existing").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::Assign {
                name: "MYVAR".to_string(),
                word: Some(Word::literal("new_val")),
                null_check: false,
            },
        )
        .unwrap();
        assert_eq!(result, "existing");
        assert_eq!(env.vars.get("MYVAR"), Some("existing"));
    }

    #[test]
    fn test_assign_null_check_empty_assigns() {
        let mut env = make_env();
        env.vars.set("MYVAR", "").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::Assign {
                name: "MYVAR".to_string(),
                word: Some(Word::literal("filled")),
                null_check: true,
            },
        )
        .unwrap();
        assert_eq!(result, "filled");
        assert_eq!(env.vars.get("MYVAR"), Some("filled"));
    }

    // ── Alt (${name:+word}) ──
    #[test]
    fn test_alt_set_returns_word() {
        let mut env = make_env();
        env.vars.set("FOO", "anything").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::Alt {
                name: "FOO".to_string(),
                word: Some(Word::literal("alt_val")),
                null_check: true,
            },
        )
        .unwrap();
        assert_eq!(result, "alt_val");
    }

    #[test]
    fn test_alt_unset_returns_empty() {
        let mut env = make_env();
        let result = expand(
            &mut env,
            &ParamExpr::Alt {
                name: "UNSET_XYZ".to_string(),
                word: Some(Word::literal("alt_val")),
                null_check: true,
            },
        )
        .unwrap();
        assert_eq!(result, "");
    }

    // ── Error (${name:?word}) ──
    #[test]
    fn test_error_set_returns_value() {
        let mut env = make_env();
        env.vars.set("FOO", "val").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::Error {
                name: "FOO".to_string(),
                word: Some(Word::literal("err msg")),
                null_check: false,
            },
        )
        .unwrap();
        assert_eq!(result, "val");
    }

    #[test]
    fn test_error_unset_returns_empty() {
        let mut env = make_env();
        let result = expand(
            &mut env,
            &ParamExpr::Error {
                name: "UNSET_XYZ".to_string(),
                word: Some(Word::literal("err msg")),
                null_check: false,
            },
        )
        .unwrap();
        assert_eq!(result, "");
    }

    // ── StripShortSuffix (${name%pattern}) ──
    #[test]
    fn test_strip_short_suffix() {
        let mut env = make_env();
        env.vars.set("FILE", "file.txt").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripShortSuffix("FILE".to_string(), Word::literal(".*")),
        )
        .unwrap();
        assert_eq!(result, "file");
    }

    #[test]
    fn test_strip_short_suffix_no_match() {
        let mut env = make_env();
        env.vars.set("FILE", "file").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripShortSuffix("FILE".to_string(), Word::literal(".*")),
        )
        .unwrap();
        assert_eq!(result, "file");
    }

    // ── StripLongPrefix (${name##pattern}) ──
    #[test]
    fn test_strip_long_prefix() {
        let mut env = make_env();
        env.vars.set("PATH_VAR", "/a/b/c").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripLongPrefix("PATH_VAR".to_string(), Word::literal("*/")),
        )
        .unwrap();
        assert_eq!(result, "c");
    }

    #[test]
    fn test_strip_short_prefix() {
        let mut env = make_env();
        env.vars.set("PATH_VAR", "/a/b/c").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripShortPrefix("PATH_VAR".to_string(), Word::literal("*/")),
        )
        .unwrap();
        // Shortest prefix matching "*/" — stops at the first "/"
        assert_eq!(result, "a/b/c");
    }

    // ── Multibyte boundary safety (added for the Layer-2 &str rewrite) ──
    #[test]
    fn test_strip_short_suffix_multibyte_ascii_pat() {
        let mut env = make_env();
        env.vars.set("V", "日本語.txt").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripShortSuffix("V".to_string(), Word::literal(".txt")),
        )
        .unwrap();
        assert_eq!(result, "日本語");
    }

    #[test]
    fn test_strip_short_prefix_multibyte_literal() {
        let mut env = make_env();
        env.vars.set("V", "日本語").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripShortPrefix("V".to_string(), Word::literal("日")),
        )
        .unwrap();
        assert_eq!(result, "本語");
    }

    #[test]
    fn test_strip_short_suffix_multibyte_literal() {
        let mut env = make_env();
        env.vars.set("V", "日本語").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripShortSuffix("V".to_string(), Word::literal("語")),
        )
        .unwrap();
        assert_eq!(result, "日本");
    }

    #[test]
    fn test_strip_long_prefix_multibyte_star() {
        let mut env = make_env();
        env.vars.set("V", "あいうえお").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripLongPrefix("V".to_string(), Word::literal("*う")),
        )
        .unwrap();
        assert_eq!(result, "えお");
    }

    #[test]
    fn test_strip_long_suffix_multibyte_star_all() {
        let mut env = make_env();
        env.vars.set("V", "日本語").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripLongSuffix("V".to_string(), Word::literal("*")),
        )
        .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_strip_short_prefix_multibyte_question() {
        let mut env = make_env();
        env.vars.set("V", "あい").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripShortPrefix("V".to_string(), Word::literal("?")),
        )
        .unwrap();
        assert_eq!(result, "い");
    }

    // ── Length (${#name}) ──
    #[test]
    fn test_length() {
        let mut env = make_env();
        env.vars.set("STR", "hello").unwrap();
        let result = expand(&mut env, &ParamExpr::Length("STR".to_string())).unwrap();
        assert_eq!(result, "5");
    }

    #[test]
    fn test_length_unset() {
        let mut env = make_env();
        let result = expand(&mut env, &ParamExpr::Length("UNSET_XYZ".to_string())).unwrap();
        assert_eq!(result, "0");
    }

    // ── Special params ──
    #[test]
    fn test_special_question() {
        let mut env = make_env();
        env.exec.last_exit_status = 42;
        let result = expand(&mut env, &ParamExpr::Special(SpecialParam::Question)).unwrap();
        assert_eq!(result, "42");
    }

    #[test]
    fn test_special_at_joins() {
        let mut env = ShellEnv::new("yosh", vec!["a".to_string(), "b".to_string()]);
        let result = expand(&mut env, &ParamExpr::Special(SpecialParam::At)).unwrap();
        assert_eq!(result, "a b");
    }
}
