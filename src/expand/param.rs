use crate::env::ShellEnv;
use crate::parser::ast::{ParamExpr, SpecialParam};
use super::{expand_word_to_string, pattern};

/// Expand a `ParamExpr` to a String.
pub fn expand(env: &mut ShellEnv, param: &ParamExpr) -> String {
    match param {
        // ── Simple variable ──────────────────────────────────────────────
        ParamExpr::Simple(name) => {
            match env.vars.get(name) {
                Some(val) => val.to_string(),
                None => {
                    if env.options.nounset {
                        eprintln!("kish: {}: parameter not set", name);
                        env.last_exit_status = 1;
                        env.flow_control = Some(crate::env::FlowControl::Return(1));
                    }
                    String::new()
                }
            }
        }

        // ── Positional parameters ────────────────────────────────────────
        ParamExpr::Positional(n) => {
            if *n > 0 {
                env.positional_params.get(n - 1).cloned().unwrap_or_default()
            } else {
                String::new()
            }
        }

        // ── Special parameters ───────────────────────────────────────────
        ParamExpr::Special(sp) => expand_special(env, sp),

        // ── ${#name} — character count ───────────────────────────────────
        ParamExpr::Length(name) => {
            let len = env.vars.get(name).map(|v| v.chars().count()).unwrap_or(0);
            len.to_string()
        }

        // ── ${name:-word} / ${name-word} ─────────────────────────────────
        ParamExpr::Default { name, word, null_check } => {
            let val = env.vars.get(name).map(|s| s.to_string());
            let is_unset_or_null = is_unset_or_null_inner(&val, *null_check);
            if is_unset_or_null {
                word.as_ref()
                    .map(|w| expand_word_to_string(env, w))
                    .unwrap_or_default()
            } else {
                val.unwrap_or_default()
            }
        }

        // ── ${name:=word} / ${name=word} ─────────────────────────────────
        ParamExpr::Assign { name, word, null_check } => {
            let val = env.vars.get(name).map(|s| s.to_string());
            let is_unset_or_null = is_unset_or_null_inner(&val, *null_check);
            if is_unset_or_null {
                let new_val = word.as_ref()
                    .map(|w| expand_word_to_string(env, w))
                    .unwrap_or_default();
                let _ = env.vars.set(name, &new_val);
                new_val
            } else {
                val.unwrap_or_default()
            }
        }

        // ── ${name:?word} / ${name?word} ─────────────────────────────────
        ParamExpr::Error { name, word, null_check } => {
            let val = env.vars.get(name).map(|s| s.to_string());
            let is_unset_or_null = is_unset_or_null_inner(&val, *null_check);
            if is_unset_or_null {
                let msg = word.as_ref()
                    .map(|w| expand_word_to_string(env, w))
                    .unwrap_or_else(|| format!("{}: parameter null or not set", name));
                eprintln!("kish: {}", msg);
                // POSIX: non-interactive shell shall exit with non-zero status
                env.last_exit_status = 1;
                env.flow_control = Some(crate::env::FlowControl::Return(1));
                String::new()
            } else {
                val.unwrap_or_default()
            }
        }

        // ── ${name:+word} / ${name+word} ─────────────────────────────────
        ParamExpr::Alt { name, word, null_check } => {
            let val = env.vars.get(name).map(|s| s.to_string());
            let is_unset_or_null = is_unset_or_null_inner(&val, *null_check);
            if is_unset_or_null {
                // Not set (or null with colon) — return empty
                String::new()
            } else {
                // Set and non-null — return the word
                word.as_ref()
                    .map(|w| expand_word_to_string(env, w))
                    .unwrap_or_default()
            }
        }

        // ── ${name%pattern} — remove shortest suffix ─────────────────────
        ParamExpr::StripShortSuffix(name, pattern_word) => {
            let value = env.vars.get(name).unwrap_or("").to_string();
            let pat = expand_word_to_string(env, pattern_word);
            strip_suffix(&value, &pat, false)
        }

        // ── ${name%%pattern} — remove longest suffix ──────────────────────
        ParamExpr::StripLongSuffix(name, pattern_word) => {
            let value = env.vars.get(name).unwrap_or("").to_string();
            let pat = expand_word_to_string(env, pattern_word);
            strip_suffix(&value, &pat, true)
        }

        // ── ${name#pattern} — remove shortest prefix ─────────────────────
        ParamExpr::StripShortPrefix(name, pattern_word) => {
            let value = env.vars.get(name).unwrap_or("").to_string();
            let pat = expand_word_to_string(env, pattern_word);
            strip_prefix(&value, &pat, false)
        }

        // ── ${name##pattern} — remove longest prefix ──────────────────────
        ParamExpr::StripLongPrefix(name, pattern_word) => {
            let value = env.vars.get(name).unwrap_or("").to_string();
            let pat = expand_word_to_string(env, pattern_word);
            strip_prefix(&value, &pat, true)
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn is_unset_or_null_inner(val: &Option<String>, null_check: bool) -> bool {
    match val {
        None => true,
        Some(v) if null_check && v.is_empty() => true,
        _ => false,
    }
}

fn expand_special(env: &ShellEnv, sp: &SpecialParam) -> String {
    match sp {
        SpecialParam::Question => env.last_exit_status.to_string(),
        SpecialParam::Dollar => env.shell_pid.as_raw().to_string(),
        SpecialParam::Zero => env.shell_name.clone(),
        SpecialParam::Hash => env.positional_params.len().to_string(),
        SpecialParam::At | SpecialParam::Star => env.positional_params.join(" "),
        SpecialParam::Bang => env.last_bg_pid.map(|p| p.to_string()).unwrap_or_default(),
        SpecialParam::Dash => env.options.to_flag_string(),
    }
}

/// Remove a suffix matching `pat` from `value`.
/// If `longest` is true, try the longest match; otherwise the shortest.
fn strip_suffix(value: &str, pat: &str, longest: bool) -> String {
    let chars: Vec<char> = value.chars().collect();
    let n = chars.len();

    if longest {
        // Try from index 0 upward (largest possible suffix = whole string)
        for start in 0..=n {
            let suffix: String = chars[start..].iter().collect();
            if pattern::matches(pat, &suffix) {
                let prefix: String = chars[..start].iter().collect();
                return prefix;
            }
        }
    } else {
        // Try from index n downward (smallest possible suffix)
        for start in (0..=n).rev() {
            let suffix: String = chars[start..].iter().collect();
            if pattern::matches(pat, &suffix) {
                let prefix: String = chars[..start].iter().collect();
                return prefix;
            }
        }
    }
    value.to_string()
}

/// Remove a prefix matching `pat` from `value`.
/// If `longest` is true, try the longest match; otherwise the shortest.
fn strip_prefix(value: &str, pat: &str, longest: bool) -> String {
    let chars: Vec<char> = value.chars().collect();
    let n = chars.len();

    if longest {
        // Try from n down to 0 (largest prefix first)
        for end in (0..=n).rev() {
            let prefix: String = chars[..end].iter().collect();
            if pattern::matches(pat, &prefix) {
                let suffix: String = chars[end..].iter().collect();
                return suffix;
            }
        }
    } else {
        // Try from 0 upward (smallest prefix first)
        for end in 0..=n {
            let prefix: String = chars[..end].iter().collect();
            if pattern::matches(pat, &prefix) {
                let suffix: String = chars[end..].iter().collect();
                return suffix;
            }
        }
    }
    value.to_string()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;
    use crate::parser::ast::{ParamExpr, SpecialParam, Word};

    fn make_env() -> ShellEnv {
        ShellEnv::new("kish", vec![])
    }

    // ── Simple ──
    #[test]
    fn test_simple_set() {
        let mut env = make_env();
        env.vars.set("FOO", "bar").unwrap();
        assert_eq!(expand(&mut env, &ParamExpr::Simple("FOO".to_string())), "bar");
    }

    #[test]
    fn test_simple_unset() {
        let mut env = make_env();
        assert_eq!(expand(&mut env, &ParamExpr::Simple("UNSET_XYZ".to_string())), "");
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
        );
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
        );
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
        );
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
        );
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
        );
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
        );
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
        );
        assert_eq!(result, "");
    }

    // ── StripShortSuffix (${name%pattern}) ──
    #[test]
    fn test_strip_short_suffix() {
        let mut env = make_env();
        env.vars.set("FILE", "file.txt").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripShortSuffix(
                "FILE".to_string(),
                Word::literal(".*"),
            ),
        );
        assert_eq!(result, "file");
    }

    #[test]
    fn test_strip_short_suffix_no_match() {
        let mut env = make_env();
        env.vars.set("FILE", "file").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripShortSuffix(
                "FILE".to_string(),
                Word::literal(".*"),
            ),
        );
        assert_eq!(result, "file");
    }

    // ── StripLongPrefix (${name##pattern}) ──
    #[test]
    fn test_strip_long_prefix() {
        let mut env = make_env();
        env.vars.set("PATH_VAR", "/a/b/c").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripLongPrefix(
                "PATH_VAR".to_string(),
                Word::literal("*/"),
            ),
        );
        assert_eq!(result, "c");
    }

    #[test]
    fn test_strip_short_prefix() {
        let mut env = make_env();
        env.vars.set("PATH_VAR", "/a/b/c").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::StripShortPrefix(
                "PATH_VAR".to_string(),
                Word::literal("*/"),
            ),
        );
        // Shortest prefix matching "*/" — stops at the first "/"
        assert_eq!(result, "a/b/c");
    }

    // ── Length (${#name}) ──
    #[test]
    fn test_length() {
        let mut env = make_env();
        env.vars.set("STR", "hello").unwrap();
        let result = expand(
            &mut env,
            &ParamExpr::Length("STR".to_string()),
        );
        assert_eq!(result, "5");
    }

    #[test]
    fn test_length_unset() {
        let mut env = make_env();
        let result = expand(
            &mut env,
            &ParamExpr::Length("UNSET_XYZ".to_string()),
        );
        assert_eq!(result, "0");
    }

    // ── Special params ──
    #[test]
    fn test_special_question() {
        let mut env = make_env();
        env.last_exit_status = 42;
        let result = expand(&mut env, &ParamExpr::Special(SpecialParam::Question));
        assert_eq!(result, "42");
    }

    #[test]
    fn test_special_at_joins() {
        let mut env = ShellEnv::new("kish", vec!["a".to_string(), "b".to_string()]);
        let result = expand(&mut env, &ParamExpr::Special(SpecialParam::At));
        assert_eq!(result, "a b");
    }
}
