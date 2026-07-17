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
                match env.vars.positional_params().get(n - 1).cloned() {
                    Some(val) => Ok(val),
                    None => {
                        // POSIX set -u: an unset positional parameter is an
                        // expansion error, same as an unset variable.
                        if env.mode.options.nounset {
                            eprintln!("yosh: {}: parameter not set", n);
                            env.exec.last_exit_status = 1;
                            env.exec.flow_control = Some(crate::env::FlowControl::Return(1));
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
                eprintln!("yosh: !: parameter not set");
                env.exec.last_exit_status = 1;
                env.exec.flow_control = Some(crate::env::FlowControl::Return(1));
                return Ok(String::new());
            }
            Ok(expand_special(env, sp))
        }

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
                // POSIX §2.6.2: attempting to assign a positional or special
                // parameter with ${name=word} is an error.
                if is_unassignable_param(name) {
                    eprintln!("yosh: {}: cannot assign in this way", name);
                    env.exec.last_exit_status = 1;
                    env.exec.flow_control = Some(crate::env::FlowControl::Return(1));
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
                    // Errors (readonly) stay ignored, as before.
                    let _ = env.assign_var(name, new_val.as_str());
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
/// Digit-string names are positional parameters (POSIX §2.5.1 — leading
/// zeros are decimal, so `01` ≡ `1`); an index past `$#` is unset (`None`).
/// All other names delegate to `VarStore::get`.
pub(super) fn lookup_var(env: &ShellEnv, name: &str) -> Option<String> {
    if name == "LINENO" {
        return Some(env.exec.lineno.to_string());
    }
    if !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()) {
        let n: usize = name.parse().unwrap_or(0);
        if n == 0 {
            return Some(env.shell_name.clone());
        }
        return env.vars.positional_params().get(n - 1).cloned();
    }
    env.vars.get(name).map(|s| s.to_string())
}

/// True when `name` denotes a positional or special parameter, which
/// `${name=word}` may not assign to (POSIX §2.6.2).
pub(super) fn is_unassignable_param(name: &str) -> bool {
    !name.is_empty()
        && !name
            .bytes()
            .next()
            .is_some_and(|b| b == b'_' || b.is_ascii_alphabetic())
}

pub(super) fn is_unset_or_null_inner(val: &Option<String>, null_check: bool) -> bool {
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
///
/// Fast paths (PERF: avoids the O(n²)+ boundary-scan + backtracking-matcher
/// combination for the common `${x%pattern}` idioms):
///   - `pat` fully literal (no metachars) → a single `ends_with` check;
///     `longest`/`shortest` are equivalent since only one match is possible.
///   - `pat` = `<literal>*` (e.g. the `${x%.*}` idiom) → an anchored
///     `find`/`rfind` scan for the literal instead of testing every
///     candidate suffix boundary against the general matcher.
///   - Anything else (brackets, `?`, a `*` not at the pattern's end, or
///     multiple `*`s) falls back to the general boundary-scan below,
///     which is unchanged.
fn strip_suffix(value: &str, pat: &str, longest: bool) -> String {
    match pattern::classify(pat) {
        pattern::PatternShape::Literal(lit) => {
            return match value.strip_suffix(lit.as_str()) {
                Some(rest) => rest.to_string(),
                None => value.to_string(),
            };
        }
        pattern::PatternShape::LiteralThenStar(lit) => {
            if lit.is_empty() {
                // Pattern is just "*": longest strips everything, shortest
                // strips nothing (matches the general algorithm — see
                // task-2 brief verification harness).
                return if longest {
                    String::new()
                } else {
                    value.to_string()
                };
            }
            let idx = if longest {
                value.find(lit.as_str())
            } else {
                value.rfind(lit.as_str())
            };
            return match idx {
                Some(i) => value[..i].to_string(),
                None => value.to_string(),
            };
        }
        pattern::PatternShape::StarThenLiteral(_) | pattern::PatternShape::General => {}
    }

    // General path: `matches` is anchored (full match), so test each
    // candidate suffix slice. `start` is a char-boundary byte offset; the
    // suffix is `value[start..]`.
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
///
/// Fast paths mirror `strip_suffix` (see its doc comment):
///   - `pat` fully literal → a single `starts_with` check.
///   - `pat` = `*<literal>` (e.g. the `${x##*/}` idiom) → an anchored
///     `find`/`rfind` scan instead of the general boundary loop.
fn strip_prefix(value: &str, pat: &str, longest: bool) -> String {
    match pattern::classify(pat) {
        pattern::PatternShape::Literal(lit) => {
            return match value.strip_prefix(lit.as_str()) {
                Some(rest) => rest.to_string(),
                None => value.to_string(),
            };
        }
        pattern::PatternShape::StarThenLiteral(lit) => {
            if lit.is_empty() {
                // Pattern is just "*": longest strips everything, shortest
                // strips nothing.
                return if longest {
                    String::new()
                } else {
                    value.to_string()
                };
            }
            let idx = if longest {
                value.rfind(lit.as_str())
            } else {
                value.find(lit.as_str())
            };
            return match idx {
                Some(i) => value[i + lit.len()..].to_string(),
                None => value.to_string(),
            };
        }
        pattern::PatternShape::LiteralThenStar(_) | pattern::PatternShape::General => {}
    }

    // General path: `matches` is anchored (full match), so test each
    // candidate prefix slice. `end` is a char-boundary byte offset; the
    // prefix is `value[..end]`.
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

    // ── strip_prefix / strip_suffix fast-path unit tests (Task 2 PERF item 9) ──
    // Directly exercise the private helpers (module-internal test access) to
    // pin fast-path behavior independently of the ParamExpr plumbing above.

    // -- Literal fast path --

    #[test]
    fn strip_suffix_literal_fast_path_match() {
        assert_eq!(strip_suffix("file.txt", ".txt", true), "file");
        assert_eq!(strip_suffix("file.txt", ".txt", false), "file");
    }

    #[test]
    fn strip_suffix_literal_fast_path_no_match() {
        assert_eq!(strip_suffix("file.txt", ".rs", true), "file.txt");
    }

    #[test]
    fn strip_prefix_literal_fast_path_match() {
        assert_eq!(strip_prefix("/a/b/c", "/a/", true), "b/c");
        assert_eq!(strip_prefix("/a/b/c", "/a/", false), "b/c");
    }

    #[test]
    fn strip_prefix_literal_fast_path_no_match() {
        assert_eq!(strip_prefix("/a/b/c", "/x/", true), "/a/b/c");
    }

    #[test]
    fn strip_literal_empty_pattern_is_identity() {
        // classify("") => Literal("") — matches at zero-length boundary,
        // stripping nothing.
        assert_eq!(strip_suffix("hello", "", true), "hello");
        assert_eq!(strip_prefix("hello", "", true), "hello");
    }

    #[test]
    fn strip_literal_pattern_longer_than_value_no_match() {
        assert_eq!(strip_suffix("ab", "abcdef", true), "ab");
        assert_eq!(strip_prefix("ab", "abcdef", true), "ab");
    }

    #[test]
    fn strip_literal_escaped_metachar_is_literal_not_wildcard() {
        // `\*` is a literal '*', not a wildcard — must not match unrelated
        // suffixes/prefixes, and must match a literal trailing/leading '*'.
        assert_eq!(strip_suffix("file*", "\\*", true), "file");
        assert_eq!(strip_suffix("file.txt", "\\*", true), "file.txt");
        assert_eq!(strip_prefix("*file", "\\*", true), "file");
    }

    // -- `*<literal>` / `<literal>*` idiom fast paths --

    #[test]
    fn strip_prefix_star_literal_longest_basename_idiom() {
        // ${x##*/} idiom: strip through the LAST '/'.
        assert_eq!(strip_prefix("/a/b/c.txt", "*/", true), "c.txt");
    }

    #[test]
    fn strip_prefix_star_literal_shortest_idiom() {
        // ${x#*/} idiom: strip through the FIRST '/'.
        assert_eq!(strip_prefix("/a/b/c.txt", "*/", false), "a/b/c.txt");
    }

    #[test]
    fn strip_prefix_star_literal_no_match() {
        assert_eq!(strip_prefix("noslash", "*/", true), "noslash");
    }

    #[test]
    fn strip_suffix_literal_star_shortest_extension_idiom() {
        // ${x%.*} idiom: strip from the LAST '.' onward.
        assert_eq!(strip_suffix("archive.tar.gz", ".*", false), "archive.tar");
    }

    #[test]
    fn strip_suffix_literal_star_longest_idiom() {
        // ${x%%.*} idiom: strip from the FIRST '.' onward.
        assert_eq!(strip_suffix("archive.tar.gz", ".*", true), "archive");
    }

    #[test]
    fn strip_suffix_literal_star_no_match() {
        assert_eq!(strip_suffix("noext", ".*", false), "noext");
    }

    #[test]
    fn strip_prefix_bare_star_longest_strips_everything() {
        assert_eq!(strip_prefix("anything", "*", true), "");
    }

    #[test]
    fn strip_prefix_bare_star_shortest_strips_nothing() {
        assert_eq!(strip_prefix("anything", "*", false), "anything");
    }

    #[test]
    fn strip_suffix_bare_star_longest_strips_everything() {
        assert_eq!(strip_suffix("anything", "*", true), "");
    }

    #[test]
    fn strip_suffix_bare_star_shortest_strips_nothing() {
        assert_eq!(strip_suffix("anything", "*", false), "anything");
    }

    // -- General fallback still correct for non-fast-path shapes --

    #[test]
    fn strip_prefix_question_mark_uses_general_path() {
        // '?' disqualifies the literal/star-literal fast paths.
        assert_eq!(strip_prefix("abc", "??", true), "c");
    }

    #[test]
    fn strip_suffix_bracket_uses_general_path() {
        assert_eq!(strip_suffix("file1", "[0-9]", true), "file");
    }

    #[test]
    fn strip_prefix_star_in_middle_uses_general_path() {
        // '*' not at an edge — StarThenLiteral/LiteralThenStar don't apply,
        // General handles "a*c" style patterns via full backtracking.
        assert_eq!(strip_prefix("axxxc rest", "a*c", true), " rest");
    }

    #[test]
    fn strip_suffix_multiple_stars_uses_general_path() {
        assert_eq!(strip_suffix("aXbYc", "a*b*c", true), "");
    }

    // -- Multibyte correctness for the new fast paths --

    #[test]
    fn strip_prefix_star_literal_multibyte() {
        assert_eq!(strip_prefix("日本/語/x", "*/", true), "x");
    }

    #[test]
    fn strip_suffix_literal_star_multibyte() {
        assert_eq!(strip_suffix("あ.い.う", ".*", false), "あ.い");
    }

    #[test]
    fn strip_literal_multibyte_pattern_fast_path() {
        assert_eq!(strip_prefix("日本語", "日本", true), "語");
        assert_eq!(strip_suffix("日本語", "語", true), "日本");
    }
}
