use super::{expand_word_to_string, pattern};
use crate::env::ShellEnv;
use crate::parser::ast::{ParamExpr, SpecialParam};

/// Expand a `ParamExpr` to a String.
pub fn expand(env: &mut ShellEnv, param: &ParamExpr) -> crate::error::Result<String> {
    match param {
        // ── Simple variable ──────────────────────────────────────────────
        ParamExpr::Simple(name) => match lookup_var(env, name) {
            Some(val) => Ok(val),
            None => {
                if env.mode.options.nounset {
                    eprintln!("yosh: {}: parameter not set", name);
                    env.exec.last_exit_status = 1;
                    env.exec.flow_control = Some(crate::env::FlowControl::Return(1));
                }
                Ok(String::new())
            }
        },

        // ── Positional parameters ────────────────────────────────────────
        ParamExpr::Positional(n) => {
            if *n > 0 {
                Ok(env
                    .vars
                    .positional_params()
                    .get(n - 1)
                    .cloned()
                    .unwrap_or_default())
            } else {
                Ok(String::new())
            }
        }

        // ── Special parameters ───────────────────────────────────────────
        ParamExpr::Special(sp) => Ok(expand_special(env, sp)),

        // ── ${#name} — character count ───────────────────────────────────
        ParamExpr::Length(name) => {
            let len = lookup_var(env, name)
                .map(|v| v.chars().count())
                .unwrap_or(0);
            Ok(len.to_string())
        }

        // ── ${name:-word} / ${name-word} ─────────────────────────────────
        ParamExpr::Default {
            name,
            word,
            null_check,
        } => {
            let val = lookup_var(env, name);
            let is_unset_or_null = is_unset_or_null_inner(&val, *null_check);
            if is_unset_or_null {
                match word.as_ref() {
                    Some(w) => expand_word_to_string(env, w),
                    None => Ok(String::new()),
                }
            } else {
                Ok(val.unwrap_or_default())
            }
        }

        // ── ${name:=word} / ${name=word} ─────────────────────────────────
        ParamExpr::Assign {
            name,
            word,
            null_check,
        } => {
            let val = lookup_var(env, name);
            let is_unset_or_null = is_unset_or_null_inner(&val, *null_check);
            if is_unset_or_null {
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
                    let _ = env.vars.set(name, &new_val);
                }
                Ok(new_val)
            } else {
                Ok(val.unwrap_or_default())
            }
        }

        // ── ${name:?word} / ${name?word} ─────────────────────────────────
        ParamExpr::Error {
            name,
            word,
            null_check,
        } => {
            let val = lookup_var(env, name);
            let is_unset_or_null = is_unset_or_null_inner(&val, *null_check);
            if is_unset_or_null {
                let msg = match word.as_ref() {
                    Some(w) => expand_word_to_string(env, w)?,
                    None => format!("{}: parameter null or not set", name),
                };
                eprintln!("yosh: {}", msg);
                // POSIX: non-interactive shell shall exit with non-zero status
                env.exec.last_exit_status = 1;
                env.exec.flow_control = Some(crate::env::FlowControl::Return(1));
                Ok(String::new())
            } else {
                Ok(val.unwrap_or_default())
            }
        }

        // ── ${name:+word} / ${name+word} ─────────────────────────────────
        ParamExpr::Alt {
            name,
            word,
            null_check,
        } => {
            let val = lookup_var(env, name);
            let is_unset_or_null = is_unset_or_null_inner(&val, *null_check);
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

        // ── ${name%pattern} — remove shortest suffix ─────────────────────
        ParamExpr::StripShortSuffix(name, pattern_word) => {
            let value = lookup_var(env, name).unwrap_or_default();
            let pat = expand_word_to_string(env, pattern_word)?;
            Ok(strip_suffix(&value, &pat, false))
        }

        // ── ${name%%pattern} — remove longest suffix ──────────────────────
        ParamExpr::StripLongSuffix(name, pattern_word) => {
            let value = lookup_var(env, name).unwrap_or_default();
            let pat = expand_word_to_string(env, pattern_word)?;
            Ok(strip_suffix(&value, &pat, true))
        }

        // ── ${name#pattern} — remove shortest prefix ─────────────────────
        ParamExpr::StripShortPrefix(name, pattern_word) => {
            let value = lookup_var(env, name).unwrap_or_default();
            let pat = expand_word_to_string(env, pattern_word)?;
            Ok(strip_prefix(&value, &pat, false))
        }

        // ── ${name##pattern} — remove longest prefix ──────────────────────
        ParamExpr::StripLongPrefix(name, pattern_word) => {
            let value = lookup_var(env, name).unwrap_or_default();
            let pat = expand_word_to_string(env, pattern_word)?;
            Ok(strip_prefix(&value, &pat, true))
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Resolve a variable by name, intercepting `LINENO` as a computed
/// pseudo-variable backed by `env.exec.lineno` rather than a real
/// `VarStore` entry (see `ExecState::lineno` doc comment for rationale).
/// All other names delegate to `VarStore::get`.
fn lookup_var(env: &ShellEnv, name: &str) -> Option<String> {
    if name == "LINENO" {
        return Some(env.exec.lineno.to_string());
    }
    env.vars.get(name).map(|s| s.to_string())
}

fn is_unset_or_null_inner(val: &Option<String>, null_check: bool) -> bool {
    match val {
        None => true,
        Some(v) if null_check && v.is_empty() => true,
        _ => false,
    }
}

fn expand_special(env: &ShellEnv, sp: &SpecialParam) -> String {
    match sp {
        SpecialParam::Question => env.exec.last_exit_status.to_string(),
        SpecialParam::Dollar => env.process.shell_pid.as_raw().to_string(),
        SpecialParam::Zero => env.shell_name.clone(),
        SpecialParam::Hash => env.vars.positional_params().len().to_string(),
        SpecialParam::At | SpecialParam::Star => env.vars.positional_params().join(" "),
        SpecialParam::Bang => env
            .process
            .jobs
            .last_bg_pid()
            .map(|p| p.as_raw().to_string())
            .unwrap_or_default(),
        SpecialParam::Dash => env.mode.options.to_flag_string(),
    }
}

/// All char-boundary byte offsets of `v`, ascending: `0, b1, …, v.len()`.
///
/// `DoubleEndedIterator` so callers iterate longest-first via `rev()` or
/// shortest-first forward. For an empty string this yields just `[0]`.
fn boundaries(v: &str) -> impl DoubleEndedIterator<Item = usize> + '_ {
    v.char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(v.len()))
}

/// Remove a suffix matching `pat` from `value`.
/// If `longest` is true, try the longest match; otherwise the shortest.
fn strip_suffix(value: &str, pat: &str, longest: bool) -> String {
    // `matches` is anchored (full match), so test each candidate suffix slice.
    // `start` is a char-boundary byte offset; the suffix is `value[start..]`.
    let cut =
        |start: usize| pattern::matches(pat, &value[start..]).then(|| value[..start].to_string());
    let found = if longest {
        // smallest start = longest suffix first
        boundaries(value).find_map(cut)
    } else {
        // largest start = shortest suffix first
        boundaries(value).rev().find_map(cut)
    };
    found.unwrap_or_else(|| value.to_string())
}

/// Remove a prefix matching `pat` from `value`.
/// If `longest` is true, try the longest match; otherwise the shortest.
fn strip_prefix(value: &str, pat: &str, longest: bool) -> String {
    // `matches` is anchored (full match), so test each candidate prefix slice.
    // `end` is a char-boundary byte offset; the prefix is `value[..end]`.
    let cut = |end: usize| pattern::matches(pat, &value[..end]).then(|| value[end..].to_string());
    let found = if longest {
        // largest end = longest prefix first
        boundaries(value).rev().find_map(cut)
    } else {
        // smallest end = shortest prefix first
        boundaries(value).find_map(cut)
    };
    found.unwrap_or_else(|| value.to_string())
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
