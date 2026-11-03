//! `cd` regular builtin (POSIX XCU `cd`): `-L`/`-P` modes, `CDPATH`
//! search, `cd -`, and lexical canonicalization of logical paths.

use crate::builtin::options::{invalid_flag, scan_flag_words};
use crate::builtin::output::write_stdout_or_report;
use crate::builtin::path::{encode_path, os_path};
use crate::env::ShellEnv;
use crate::error::{RuntimeErrorKind, ShellError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CdMode {
    Logical,
    Physical,
}

pub fn builtin_cd(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // 1. Parse options
    let (mode, operand) = parse_cd_options(args)?;

    // 2. Resolve operand -> target path
    let (target, from_cdpath) = resolve_target(operand.as_deref(), env)?;

    // 3. Capture old PWD (logical if available, else from the kernel)
    let old_pwd = env
        .vars
        .get("PWD")
        .map(|s| s.to_string())
        .or_else(|| std::env::current_dir().ok().map(|p| encode_path(&p)))
        .unwrap_or_else(|| "/".to_string());

    // 4. Compute and apply the chdir per mode
    let chdir_error = |e: std::io::Error| {
        ShellError::runtime(RuntimeErrorKind::IoError, format!("cd: {}: {}", target, e))
    };
    let new_pwd = match mode {
        CdMode::Logical => {
            let candidate = lexical_canonicalize(&target, &old_pwd);
            std::env::set_current_dir(os_path(&candidate)).map_err(chdir_error)?;
            candidate
        }
        CdMode::Physical => {
            std::env::set_current_dir(os_path(&target)).map_err(chdir_error)?;
            encode_path(&std::env::current_dir().map_err(chdir_error)?)
        }
    };

    // 5. Update PWD / OLDPWD
    let _ = env.vars.set("OLDPWD", old_pwd);
    let _ = env.vars.set("PWD", new_pwd.clone());

    // 6. Print new PWD if the operand came from CDPATH or was "-"
    // (the directory change itself stands even if the write fails).
    if from_cdpath {
        return Ok(write_stdout_or_report("cd", &new_pwd, true));
    }

    Ok(0)
}

fn parse_cd_options(args: &[String]) -> Result<(CdMode, Option<String>), ShellError> {
    let mut mode = CdMode::Logical;
    // `-L`/`-P` may repeat or cluster; the last one wins. A lone `-` is
    // the `cd -` operand, not an option.
    let idx = scan_flag_words(args, |ch| {
        match ch {
            'L' => mode = CdMode::Logical,
            'P' => mode = CdMode::Physical,
            _ => return false,
        }
        true
    })
    .map_err(|e| {
        ShellError::runtime(
            RuntimeErrorKind::InvalidArgument,
            invalid_flag("cd", e.flag),
        )
    })?;

    match &args[idx..] {
        [] => Ok((mode, None)),
        [operand] => Ok((mode, Some(operand.clone()))),
        _ => Err(ShellError::runtime(
            RuntimeErrorKind::IoError,
            "cd: too many arguments",
        )),
    }
}

/// Lexical path canonicalization per POSIX §4 cd step 8 (logical mode).
/// Pure string operation: does not touch the filesystem.
/// Handles: leading-`/` absolute vs relative (prepend `pwd`), `.` skip,
/// `..` lexical pop, `//` collapse. When popping past the root, stays
/// at `/`.
fn lexical_canonicalize(path: &str, pwd: &str) -> String {
    let combined: String = if path.starts_with('/') {
        path.to_string()
    } else if path.is_empty() {
        pwd.to_string()
    } else {
        format!("{}/{}", pwd.trim_end_matches('/'), path)
    };

    let mut stack: Vec<&str> = Vec::new();
    for comp in combined.split('/') {
        match comp {
            "" | "." => continue,
            ".." => {
                if stack.last().map(|s| *s != "..").unwrap_or(false) {
                    stack.pop();
                } else if !combined.starts_with('/') {
                    stack.push("..");
                }
                // absolute path: dotdot above root is a no-op
            }
            other => stack.push(other),
        }
    }

    if stack.is_empty() {
        "/".to_string()
    } else {
        let mut out = String::new();
        for c in &stack {
            out.push('/');
            out.push_str(c);
        }
        out
    }
}

/// Resolve a cd operand (or None for HOME) to the target path plus a
/// boolean indicating whether the result came from CDPATH (or was
/// `cd -`); when true the caller must print the new PWD.
fn resolve_target(operand: Option<&str>, env: &ShellEnv) -> Result<(String, bool), ShellError> {
    // Case 1: no operand -> HOME
    let op = match operand {
        None => {
            return match env.vars.get("HOME") {
                Some(h) if !h.is_empty() => Ok((h.to_string(), false)),
                _ => Err(ShellError::runtime(
                    RuntimeErrorKind::IoError,
                    "cd: HOME not set",
                )),
            };
        }
        Some(o) => o,
    };

    // Case 2: `cd -` -> OLDPWD, print the new PWD
    if op == "-" {
        return match env.vars.get("OLDPWD") {
            Some(p) if !p.is_empty() => Ok((p.to_string(), true)),
            _ => Err(ShellError::runtime(
                RuntimeErrorKind::IoError,
                "cd: OLDPWD not set",
            )),
        };
    }

    // Case 3: absolute path -> as-is, no CDPATH
    if op.starts_with('/') {
        return Ok((op.to_string(), false));
    }

    // Case 4: dot-prefixed -> skip CDPATH
    if op == "." || op == ".." || op.starts_with("./") || op.starts_with("../") {
        return Ok((op.to_string(), false));
    }

    // Case 5: CDPATH search
    if let Some(cdpath) = env.vars.get("CDPATH") {
        for entry in cdpath.split(':') {
            let dir = if entry.is_empty() { "." } else { entry };
            let candidate = format!("{}/{}", dir.trim_end_matches('/'), op);
            if let Ok(meta) = std::fs::metadata(os_path(&candidate))
                && meta.is_dir()
            {
                // POSIX cd: the new directory is written only when a
                // *non-empty* CDPATH entry was used; an empty entry (the
                // current directory) prints nothing, as in dash/bash.
                return Ok((candidate, !entry.is_empty()));
            }
        }
    }

    // No CDPATH match: return operand as-is (PWD prefix applied by caller)
    Ok((op.to_string(), false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    // ── parse_cd_options ─────────────────────────────────────────

    #[test]
    fn parse_no_args_defaults_to_logical_none() {
        let (mode, op) = parse_cd_options(&[]).unwrap();
        assert_eq!(mode, CdMode::Logical);
        assert_eq!(op, None);
    }

    #[test]
    fn parse_dash_is_operand_not_option() {
        let (mode, op) = parse_cd_options(&s(&["-"])).unwrap();
        assert_eq!(mode, CdMode::Logical);
        assert_eq!(op.as_deref(), Some("-"));
    }

    #[test]
    fn parse_l_flag() {
        let (mode, op) = parse_cd_options(&s(&["-L"])).unwrap();
        assert_eq!(mode, CdMode::Logical);
        assert_eq!(op, None);
    }

    #[test]
    fn parse_p_flag() {
        let (mode, op) = parse_cd_options(&s(&["-P"])).unwrap();
        assert_eq!(mode, CdMode::Physical);
        assert_eq!(op, None);
    }

    #[test]
    fn parse_flag_with_operand() {
        let (mode, op) = parse_cd_options(&s(&["-P", "/tmp"])).unwrap();
        assert_eq!(mode, CdMode::Physical);
        assert_eq!(op.as_deref(), Some("/tmp"));
    }

    #[test]
    fn parse_combined_flags_last_wins() {
        let (mode, _) = parse_cd_options(&s(&["-LP"])).unwrap();
        assert_eq!(mode, CdMode::Physical);
        let (mode, _) = parse_cd_options(&s(&["-PL"])).unwrap();
        assert_eq!(mode, CdMode::Logical);
    }

    #[test]
    fn parse_separate_flags_last_wins() {
        let (mode, op) = parse_cd_options(&s(&["-L", "-P", "foo"])).unwrap();
        assert_eq!(mode, CdMode::Physical);
        assert_eq!(op.as_deref(), Some("foo"));
    }

    #[test]
    fn parse_double_dash_terminates_options() {
        let (mode, op) = parse_cd_options(&s(&["--", "-foo"])).unwrap();
        assert_eq!(mode, CdMode::Logical);
        assert_eq!(op.as_deref(), Some("-foo"));
    }

    #[test]
    fn parse_invalid_option_errors() {
        let err = parse_cd_options(&s(&["-x"])).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("invalid option"));
    }

    #[test]
    fn parse_too_many_operands_errors() {
        let err = parse_cd_options(&s(&["a", "b"])).unwrap_err();
        assert!(err.to_string().contains("too many arguments"));
    }

    // ── lexical_canonicalize ─────────────────────────────────────

    #[test]
    fn lex_absolute_returned_as_is() {
        assert_eq!(lexical_canonicalize("/tmp", "/Users/foo"), "/tmp");
    }

    #[test]
    fn lex_absolute_with_dotdot() {
        assert_eq!(lexical_canonicalize("/tmp/../etc", "/"), "/etc");
    }

    #[test]
    fn lex_relative_resolves_against_pwd() {
        assert_eq!(lexical_canonicalize("../bar", "/tmp/foo"), "/tmp/bar");
    }

    #[test]
    fn lex_single_dots_skipped() {
        assert_eq!(lexical_canonicalize("./foo/./bar", "/tmp"), "/tmp/foo/bar");
    }

    #[test]
    fn lex_repeated_slashes_collapsed() {
        assert_eq!(lexical_canonicalize("/tmp//foo", "/"), "/tmp/foo");
    }

    #[test]
    fn lex_dotdot_above_root_stays_at_root() {
        assert_eq!(lexical_canonicalize("/..", "/"), "/");
    }

    #[test]
    fn lex_multiple_dotdots_pop_correctly() {
        assert_eq!(lexical_canonicalize("a/b/../..", "/tmp/x"), "/tmp/x");
    }

    #[test]
    fn lex_empty_operand_returns_pwd() {
        assert_eq!(lexical_canonicalize("", "/tmp"), "/tmp");
    }

    #[test]
    fn lex_root_stays_root() {
        assert_eq!(lexical_canonicalize("/", "/tmp"), "/");
    }

    #[test]
    fn lex_trailing_slash_dropped() {
        assert_eq!(lexical_canonicalize("/tmp/", "/"), "/tmp");
    }

    // ── resolve_target ───────────────────────────────────────────

    use crate::env::ShellEnv;

    fn make_env(pairs: &[(&str, &str)]) -> ShellEnv {
        let mut env = ShellEnv::new("yosh", vec![]);
        for name in &["HOME", "OLDPWD", "PWD", "CDPATH"] {
            let _ = env.vars.unset(name);
        }
        for (k, v) in pairs {
            let _ = env.vars.set(k, (*v).to_string());
        }
        env
    }

    #[test]
    fn resolve_none_uses_home() {
        let env = make_env(&[("HOME", "/home/x")]);
        let (target, from_cdpath) = resolve_target(None, &env).unwrap();
        assert_eq!(target, "/home/x");
        assert!(!from_cdpath);
    }

    #[test]
    fn resolve_none_home_unset_errors() {
        let env = make_env(&[]);
        let err = resolve_target(None, &env).unwrap_err();
        assert!(err.to_string().contains("HOME not set"));
    }

    #[test]
    fn resolve_dash_uses_oldpwd_and_sets_from_cdpath() {
        let env = make_env(&[("OLDPWD", "/prev")]);
        let (target, from_cdpath) = resolve_target(Some("-"), &env).unwrap();
        assert_eq!(target, "/prev");
        assert!(from_cdpath, "cd - must print the new PWD");
    }

    #[test]
    fn resolve_dash_oldpwd_unset_errors() {
        let env = make_env(&[]);
        let err = resolve_target(Some("-"), &env).unwrap_err();
        assert!(err.to_string().contains("OLDPWD not set"));
    }

    #[test]
    fn resolve_absolute_passes_through() {
        let env = make_env(&[("CDPATH", "/etc")]);
        let (target, from_cdpath) = resolve_target(Some("/tmp"), &env).unwrap();
        assert_eq!(target, "/tmp");
        assert!(!from_cdpath, "absolute paths skip CDPATH");
    }

    #[test]
    fn resolve_dot_prefix_skips_cdpath() {
        let env = make_env(&[("CDPATH", "/etc")]);
        let (target, from_cdpath) = resolve_target(Some("./foo"), &env).unwrap();
        assert_eq!(target, "./foo");
        assert!(!from_cdpath);
    }

    #[test]
    fn resolve_dotdot_prefix_skips_cdpath() {
        let env = make_env(&[("CDPATH", "/etc")]);
        let (target, from_cdpath) = resolve_target(Some("../foo"), &env).unwrap();
        assert_eq!(target, "../foo");
        assert!(!from_cdpath);
    }

    #[test]
    fn resolve_cdpath_hit() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let cdpath = tmp.path().to_string_lossy().to_string();

        let env = make_env(&[("CDPATH", cdpath.as_str())]);
        let (target, from_cdpath) = resolve_target(Some("sub"), &env).unwrap();
        assert_eq!(target, sub.to_string_lossy());
        assert!(from_cdpath);
    }

    #[test]
    fn resolve_cdpath_miss_falls_through() {
        let tmp = tempfile::tempdir().unwrap();
        let cdpath = tmp.path().to_string_lossy().to_string();
        let env = make_env(&[("CDPATH", cdpath.as_str())]);
        let (target, from_cdpath) = resolve_target(Some("nonexistent_xyz"), &env).unwrap();
        assert_eq!(target, "nonexistent_xyz");
        assert!(!from_cdpath);
    }

    #[test]
    fn resolve_cdpath_empty_entry_is_dot() {
        // Create the probe dir inside the existing cwd rather than
        // `set_current_dir`-ing into a tempdir. Mutating the process-wide
        // cwd races with any parallel test that spawns a subprocess
        // (e.g. `plugin::host::commands` invoking `/bin/sh`): the child
        // inherits the dangling cwd once the tempdir drops, and sh stalls
        // trying to recover.
        //
        // This test still *depends* on a stable cwd (the probe dir is
        // resolved via `./<name>`), so hold the cwd lock against tests
        // that legitimately chdir (see `crate::test_sync`).
        let _cwd = crate::test_sync::lock_cwd();
        let tmp = tempfile::tempdir_in(".").unwrap();
        let name = tmp
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();

        let env = make_env(&[("CDPATH", ":/nonexistent")]);
        let (target, print_pwd) = resolve_target(Some(&name), &env).unwrap();
        assert_eq!(target, format!("./{}", name));
        // A match through an empty (current-directory) entry is silent.
        assert!(!print_pwd);
    }

    #[test]
    fn resolve_cdpath_skips_non_directory_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("regular_file");
        std::fs::write(&file_path, "x").unwrap();

        let cdpath = tmp.path().to_string_lossy().to_string();
        let env = make_env(&[("CDPATH", cdpath.as_str())]);
        let (target, from_cdpath) = resolve_target(Some("regular_file"), &env).unwrap();
        assert_eq!(target, "regular_file");
        assert!(!from_cdpath);
    }
}
