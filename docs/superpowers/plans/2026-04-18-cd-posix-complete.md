# POSIX-Complete `cd` Builtin Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the current `cd` builtin with a POSIX §4-compliant implementation that preserves logical paths in `PWD`, supports `-L`/`-P` option parsing, and searches `CDPATH`, flipping the XFAIL at `e2e/posix_spec/2_05_03_shell_variables/pwd_after_cd.sh`.

**Architecture:** Decompose `builtin_cd` into three pure helpers (`parse_cd_options`, `lexical_canonicalize`, `resolve_target`) plus a thin side-effectful entry function that performs `set_current_dir`, updates `PWD`/`OLDPWD`, and optionally prints the new PWD. Side effects stay in the entry function; helpers are table-testable.

**Tech Stack:** Rust 2024 edition, `nix` crate (already in deps), `std::env`, `std::fs`, `std::path::Path`. Tests use Rust's built-in `#[test]` and `tempfile` (already in `dev-dependencies` — verify in Task 0).

**Spec:** `docs/superpowers/specs/2026-04-18-cd-posix-complete-design.md`

---

## File Structure

**Modify:**

- `src/builtin/regular.rs` — rewrite `builtin_cd` (currently lines 5–61), add three private helpers, add `#[cfg(test)] mod tests` at end of file.
- `e2e/posix_spec/2_05_03_shell_variables/pwd_after_cd.sh` — remove `XFAIL:` metadata line.
- `TODO.md` — delete the `§2.5.3 PWD logical path` entry under "POSIX Conformance Gaps (Chapter 2)".

**Create (E2E tests, all `644` permissions, under `e2e/builtin/`):**

- `cd_logical_default.sh`
- `cd_physical_flag.sh`
- `cd_logical_dotdot.sh`
- `cd_double_dash.sh`
- `cd_invalid_option.sh`
- `cd_too_many_args.sh`
- `cd_dash_prints_pwd.sh`
- `cd_cdpath_basic.sh`
- `cd_cdpath_empty_entry.sh`
- `cd_cdpath_not_found.sh`
- `cd_oldpwd_logical.sh`

---

## Task 0: Verify prerequisites

**Files:**
- Check: `Cargo.toml` for `tempfile` in `[dev-dependencies]`

- [ ] **Step 1: Confirm `tempfile` is available**

Run:
```bash
grep -A1 '\[dev-dependencies\]' Cargo.toml | head -20
grep '^tempfile' Cargo.toml
```

Expected: `tempfile = "..."` appears under `[dev-dependencies]`. If it does NOT appear, add it:
```bash
cargo add --dev tempfile
```
and commit that change alone with message `chore: add tempfile dev-dependency for cd tests`.

- [ ] **Step 2: Baseline `cargo test` to confirm clean starting point**

Run: `cargo test --lib 2>&1 | tail -5`
Expected: `test result: ok.` for all test suites.

---

## Task 1: Add `CdMode` enum and `parse_cd_options` helper

**Files:**
- Modify: `src/builtin/regular.rs` (add enum + helper + unit tests at end)

- [ ] **Step 1: Add the test module skeleton and the first failing test**

Append to `src/builtin/regular.rs`:

```rust
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
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test --lib builtin::regular::tests::parse 2>&1 | tail -20`
Expected: compilation error `cannot find type CdMode` / `cannot find function parse_cd_options`.

- [ ] **Step 3: Implement `CdMode` and `parse_cd_options`**

Near the top of `src/builtin/regular.rs`, after the `use` lines, insert:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CdMode {
    Logical,
    Physical,
}

/// Parse `cd [-L|-P] [operand]` per POSIX §4. Returns `(mode, operand)`.
/// `-` is treated as an operand, never as an option.
/// Combined short flags (`-LP`): last letter wins.
pub(crate) fn parse_cd_options(
    args: &[String],
) -> Result<(CdMode, Option<String>), ShellError> {
    let mut mode = CdMode::Logical;
    let mut iter = args.iter();
    let operand: Option<String>;

    loop {
        match iter.next() {
            None => {
                operand = None;
                break;
            }
            Some(a) if a == "--" => {
                operand = iter.next().cloned();
                if iter.next().is_some() {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::IoError,
                        "cd: too many arguments",
                    ));
                }
                break;
            }
            Some(a) if a == "-" => {
                operand = Some(a.clone());
                if iter.next().is_some() {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::IoError,
                        "cd: too many arguments",
                    ));
                }
                break;
            }
            Some(a) if a.starts_with('-') && a.len() >= 2 => {
                for ch in a[1..].chars() {
                    match ch {
                        'L' => mode = CdMode::Logical,
                        'P' => mode = CdMode::Physical,
                        other => {
                            return Err(ShellError::runtime(
                                RuntimeErrorKind::InvalidArgument,
                                format!("cd: -{}: invalid option", other),
                            ));
                        }
                    }
                }
                // continue parsing
            }
            Some(a) => {
                operand = Some(a.clone());
                if iter.next().is_some() {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::IoError,
                        "cd: too many arguments",
                    ));
                }
                break;
            }
        }
    }

    Ok((mode, operand))
}
```

Also ensure the existing `use crate::error::{RuntimeErrorKind, ShellError};` line is present (it already is at line 2).

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --lib builtin::regular::tests::parse 2>&1 | tail -5`
Expected: `test result: ok. 10 passed`.

- [ ] **Step 5: Commit**

```bash
git add src/builtin/regular.rs
git commit -m "$(cat <<'EOF'
feat(cd): add parse_cd_options helper with CdMode enum

Introduces -L/-P option parsing for the cd builtin per POSIX §4. "-" is
treated as an operand, not an option; combined short flags (e.g. -LP)
use last-letter-wins semantics matching dash/bash.

Task 1/6 of the POSIX-complete cd rewrite. See
docs/superpowers/specs/2026-04-18-cd-posix-complete-design.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Add `lexical_canonicalize` helper

**Files:**
- Modify: `src/builtin/regular.rs` (add helper + tests)

- [ ] **Step 1: Add failing unit tests**

Inside the existing `#[cfg(test)] mod tests { ... }` block (after the `parse_*` tests), append:

```rust
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
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test --lib builtin::regular::tests::lex 2>&1 | tail -20`
Expected: compile error `cannot find function lexical_canonicalize`.

- [ ] **Step 3: Implement `lexical_canonicalize`**

Add below the `parse_cd_options` function in `src/builtin/regular.rs`:

```rust
/// Lexical path canonicalization per POSIX §4 cd step 8 (logical mode).
/// Pure string operation: does not touch the filesystem.
/// Handles: leading-`/` absolute vs relative (prepend `pwd`), `.` skip,
/// `..` lexical pop, `//` collapse. When popping past the root, stays
/// at `/`.
pub(crate) fn lexical_canonicalize(path: &str, pwd: &str) -> String {
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
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --lib builtin::regular::tests::lex 2>&1 | tail -5`
Expected: `test result: ok. 10 passed`.

- [ ] **Step 5: Commit**

```bash
git add src/builtin/regular.rs
git commit -m "$(cat <<'EOF'
feat(cd): add lexical_canonicalize helper for logical path resolution

Pure string-level path canonicalizer implementing POSIX §4 cd step 8
(logical mode): prepends PWD for relative paths, drops ".", pops "..",
collapses repeated slashes. Does not touch the filesystem.

Task 2/6 of the POSIX-complete cd rewrite.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Add `resolve_target` helper (HOME/OLDPWD/CDPATH resolution)

**Files:**
- Modify: `src/builtin/regular.rs`

- [ ] **Step 1: Add failing unit tests**

Inside the existing `#[cfg(test)] mod tests { ... }` block, append:

```rust
    // ── resolve_target ───────────────────────────────────────────

    use crate::env::ShellEnv;

    fn make_env(pairs: &[(&str, &str)]) -> ShellEnv {
        let mut env = ShellEnv::new("yosh", vec![]);
        // Wipe anything that leaks in from the host process env that we
        // care about, then set only what the test asks for.
        for name in &["HOME", "OLDPWD", "PWD", "CDPATH"] {
            let _ = env.vars.unset(name);
        }
        for (k, v) in pairs {
            let _ = env.vars.set(*k, (*v).to_string());
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
        let (target, from_cdpath) =
            resolve_target(Some("nonexistent_xyz"), &env).unwrap();
        assert_eq!(target, "nonexistent_xyz");
        assert!(!from_cdpath);
    }

    #[test]
    fn resolve_cdpath_empty_entry_is_dot() {
        // Leading ":" = current directory first entry
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        // Leading empty entry - the "." search should match "sub"
        let env = make_env(&[("CDPATH", ":/nonexistent")]);
        let (target, from_cdpath) = resolve_target(Some("sub"), &env).unwrap();
        assert!(target.ends_with("sub") || target == "./sub",
                "got: {}", target);
        assert!(from_cdpath);
    }

    #[test]
    fn resolve_cdpath_skips_non_directory_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("regular_file");
        std::fs::write(&file_path, "x").unwrap();

        let cdpath = tmp.path().to_string_lossy().to_string();
        let env = make_env(&[("CDPATH", cdpath.as_str())]);
        // "regular_file" exists under CDPATH entry but is not a directory
        let (target, from_cdpath) =
            resolve_target(Some("regular_file"), &env).unwrap();
        // CDPATH should skip it, fall through to operand-as-relative
        assert_eq!(target, "regular_file");
        assert!(!from_cdpath);
    }
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test --lib builtin::regular::tests::resolve 2>&1 | tail -20`
Expected: compile error `cannot find function resolve_target`.

- [ ] **Step 3: Implement `resolve_target`**

Add below `lexical_canonicalize` in `src/builtin/regular.rs`:

```rust
/// Resolve a cd operand (or None for HOME) to the target path plus a
/// boolean indicating whether the result came from CDPATH (or was
/// `cd -`); when true the caller must print the new PWD.
pub(crate) fn resolve_target(
    operand: Option<&str>,
    env: &ShellEnv,
) -> Result<(String, bool), ShellError> {
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
            if let Ok(meta) = std::fs::metadata(&candidate)
                && meta.is_dir()
            {
                return Ok((candidate, true));
            }
        }
    }

    // No CDPATH match: return operand as-is (PWD prefix applied by caller)
    Ok((op.to_string(), false))
}
```

Note: `metadata()` follows symlinks (as required); if the entry is a
symlink to a non-directory, `is_dir()` returns false and we skip it.

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --lib builtin::regular::tests::resolve 2>&1 | tail -5`
Expected: `test result: ok. 11 passed`.

- [ ] **Step 5: Commit**

```bash
git add src/builtin/regular.rs
git commit -m "$(cat <<'EOF'
feat(cd): add resolve_target helper for CDPATH and HOME/OLDPWD

Implements POSIX §4 cd steps 1-5: HOME default when no operand, OLDPWD
for "cd -" (with from_cdpath=true to trigger the stdout print), absolute
and dot-prefixed paths skip CDPATH, CDPATH entries with "" mean ".".
Entries whose candidate is not an existing directory are silently
skipped; final miss falls through to operand-as-relative.

Task 3/6 of the POSIX-complete cd rewrite.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Rewire `builtin_cd` to use the helpers

**Files:**
- Modify: `src/builtin/regular.rs` (lines 5–61, the existing `builtin_cd` body)

- [ ] **Step 1: Replace `builtin_cd` with the integrated implementation**

Replace the current `builtin_cd` function (lines 5–61) with:

```rust
pub fn builtin_cd(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // 1. Parse options
    let (mode, operand) = match parse_cd_options(args) {
        Ok(v) => v,
        Err(e) => {
            // Exit-1 usage errors (too many arguments) go through IoError.
            // Exit-2 errors (invalid option) go through InvalidArgument.
            // For exit-1 cases we still want to print "yosh: <msg>" but
            // preserve the Err so the caller's standard reporting path
            // runs. Let the existing ShellError display handle that.
            return Err(e);
        }
    };

    // 2. Resolve operand -> target path
    let (target, from_cdpath) = match resolve_target(operand.as_deref(), env) {
        Ok(v) => v,
        Err(e) => return Err(e),
    };

    // 3. Capture old PWD (logical if available, else from the kernel)
    let old_pwd = env
        .vars
        .get("PWD")
        .map(|s| s.to_string())
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "/".to_string());

    // 4. Compute and apply the chdir per mode
    let new_pwd = match mode {
        CdMode::Logical => {
            let candidate = lexical_canonicalize(&target, &old_pwd);
            if let Err(e) = std::env::set_current_dir(&candidate) {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::IoError,
                    format!("cd: {}: {}", target, e),
                ));
            }
            candidate
        }
        CdMode::Physical => {
            if let Err(e) = std::env::set_current_dir(&target) {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::IoError,
                    format!("cd: {}: {}", target, e),
                ));
            }
            match std::env::current_dir() {
                Ok(p) => p.to_string_lossy().into_owned(),
                Err(e) => {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::IoError,
                        format!("cd: {}: {}", target, e),
                    ));
                }
            }
        }
    };

    // 5. Update PWD / OLDPWD
    let _ = env.vars.set("OLDPWD", old_pwd);
    let _ = env.vars.set("PWD", new_pwd.clone());

    // 6. Print new PWD if the operand came from CDPATH or was "-"
    if from_cdpath {
        println!("{}", new_pwd);
    }

    Ok(0)
}
```

Also remove any now-unused `use` statements and verify the file compiles.

- [ ] **Step 2: Build and run all tests**

Run:
```bash
cargo build 2>&1 | tail -10
cargo test --lib 2>&1 | tail -10
```

Expected: clean build; `test result: ok.` for all suites (including the
new parse/lex/resolve tests from Tasks 1–3 — a total of 31 new tests
plus existing ones).

- [ ] **Step 3: Run existing cd E2E tests to confirm no regression**

Run:
```bash
./e2e/run_tests.sh --filter=cd_ 2>&1 | tail -15
```

Expected: `cd_basic.sh` and `cd_dash_oldpwd.sh` both `[PASS]`.

- [ ] **Step 4: Run `cargo clippy` to catch issues**

Run: `cargo clippy --lib 2>&1 | tail -20`
Expected: no warnings in `src/builtin/regular.rs`.

- [ ] **Step 5: Commit**

```bash
git add src/builtin/regular.rs
git commit -m "$(cat <<'EOF'
feat(cd): rewrite builtin_cd using helpers for POSIX §4 compliance

Replaces the monolithic cd body with a pipeline that delegates to
parse_cd_options, resolve_target, and lexical_canonicalize. Logical
mode (-L, default) preserves symlink-friendly paths in PWD/OLDPWD;
physical mode (-P) falls back to the kernel-canonicalized path. The
CDPATH match and "cd -" both trigger stdout printing of the new PWD
per POSIX §4.

Task 4/6 of the POSIX-complete cd rewrite.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Flip XFAIL and add new E2E tests

**Files:**
- Modify: `e2e/posix_spec/2_05_03_shell_variables/pwd_after_cd.sh`
- Create: 11 files under `e2e/builtin/`

- [ ] **Step 1: Remove XFAIL from the existing PWD test**

Edit `e2e/posix_spec/2_05_03_shell_variables/pwd_after_cd.sh`. Delete
this exact line:

```
# XFAIL: PWD resolved to physical path (e.g. /private/tmp on macOS); POSIX 'cd' without -P shall preserve logical path
```

- [ ] **Step 2: Build the debug binary that the E2E runner requires**

Run: `cargo build 2>&1 | tail -5`
Expected: no errors.

- [ ] **Step 3: Verify the flipped XFAIL now passes**

Run:
```bash
./e2e/run_tests.sh --filter=pwd_after_cd 2>&1 | tail -5
```

Expected: `[PASS]  posix_spec/2_05_03_shell_variables/pwd_after_cd.sh`.

- [ ] **Step 4: Create `cd_logical_default.sh`**

Path: `e2e/builtin/cd_logical_default.sh`

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd without -P preserves logical path (no symlink resolution)
# EXPECT_OUTPUT: /tmp
# EXPECT_EXIT: 0
cd /tmp
echo "$PWD"
```

- [ ] **Step 5: Create `cd_physical_flag.sh`**

Path: `e2e/builtin/cd_physical_flag.sh`

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd -P resolves the physical path
# EXPECT_EXIT: 0
cd -P /tmp
# On Linux, /tmp is already physical; on macOS, /tmp -> /private/tmp.
# Accept either.
case "$PWD" in
    /tmp|/private/tmp) exit 0 ;;
    *) echo "unexpected PWD: $PWD" >&2; exit 1 ;;
esac
```

- [ ] **Step 6: Create `cd_logical_dotdot.sh`**

Path: `e2e/builtin/cd_logical_dotdot.sh`

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd /tmp/../etc resolves to /etc lexically
# EXPECT_OUTPUT: /etc
# EXPECT_EXIT: 0
cd /tmp/../etc
echo "$PWD"
```

- [ ] **Step 7: Create `cd_double_dash.sh`**

Path: `e2e/builtin/cd_double_dash.sh`

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd -- treats the following argument as an operand even if it starts with -
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/-foo"
cd "$TEST_TMPDIR"
cd -- -foo
case "$PWD" in
    */-foo) exit 0 ;;
    *) echo "unexpected PWD: $PWD" >&2; exit 1 ;;
esac
```

- [ ] **Step 8: Create `cd_invalid_option.sh`**

Path: `e2e/builtin/cd_invalid_option.sh`

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd with an invalid option exits 2 with an error message
# EXPECT_EXIT: 2
# EXPECT_STDERR: invalid option
cd -x
```

- [ ] **Step 9: Create `cd_too_many_args.sh`**

Path: `e2e/builtin/cd_too_many_args.sh`

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd rejects more than one operand
# EXPECT_EXIT: 1
# EXPECT_STDERR: too many arguments
cd /tmp /etc
```

- [ ] **Step 10: Create `cd_dash_prints_pwd.sh`**

Path: `e2e/builtin/cd_dash_prints_pwd.sh`

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd - prints the new PWD on stdout
# EXPECT_OUTPUT: /tmp
# EXPECT_EXIT: 0
cd /tmp
cd /etc
cd - > "$TEST_TMPDIR/out"
cat "$TEST_TMPDIR/out"
```

- [ ] **Step 11: Create `cd_cdpath_basic.sh`**

Path: `e2e/builtin/cd_cdpath_basic.sh`

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd with CDPATH finds the operand under a CDPATH entry and prints the new PWD
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/sub"
CDPATH="$TEST_TMPDIR" cd sub > "$TEST_TMPDIR/out"
case "$PWD" in
    *"/sub") ;;
    *) echo "unexpected PWD: $PWD" >&2; exit 1 ;;
esac
grep -q sub "$TEST_TMPDIR/out" || { echo "stdout missing sub"; exit 1; }
```

- [ ] **Step 12: Create `cd_cdpath_empty_entry.sh`**

Path: `e2e/builtin/cd_cdpath_empty_entry.sh`

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: An empty CDPATH entry (leading colon) means current directory
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/sub"
cd "$TEST_TMPDIR"
CDPATH=":/nonexistent" cd sub
case "$PWD" in
    *"/sub") exit 0 ;;
    *) echo "unexpected PWD: $PWD" >&2; exit 1 ;;
esac
```

- [ ] **Step 13: Create `cd_cdpath_not_found.sh`**

Path: `e2e/builtin/cd_cdpath_not_found.sh`

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: When CDPATH has no match, cd falls back to normal resolution and errors if the operand does not exist
# EXPECT_EXIT: 1
CDPATH=/tmp cd nonexistent_xyz_zzz
```

- [ ] **Step 14: Create `cd_oldpwd_logical.sh`**

Path: `e2e/builtin/cd_oldpwd_logical.sh`

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: OLDPWD stores the logical previous PWD
# EXPECT_OUTPUT: /tmp
# EXPECT_EXIT: 0
cd /tmp
cd /etc
echo "$OLDPWD"
```

- [ ] **Step 15: Set permissions on all new files to 644**

Run:
```bash
chmod 644 \
  e2e/builtin/cd_logical_default.sh \
  e2e/builtin/cd_physical_flag.sh \
  e2e/builtin/cd_logical_dotdot.sh \
  e2e/builtin/cd_double_dash.sh \
  e2e/builtin/cd_invalid_option.sh \
  e2e/builtin/cd_too_many_args.sh \
  e2e/builtin/cd_dash_prints_pwd.sh \
  e2e/builtin/cd_cdpath_basic.sh \
  e2e/builtin/cd_cdpath_empty_entry.sh \
  e2e/builtin/cd_cdpath_not_found.sh \
  e2e/builtin/cd_oldpwd_logical.sh
```

- [ ] **Step 16: Run all new cd tests**

Run:
```bash
./e2e/run_tests.sh --filter=cd_ 2>&1 | tail -20
```

Expected: every new `cd_*` test reports `[PASS]`. If any fails, debug
and fix the implementation or the test (consult the spec to decide
which is wrong) before committing.

- [ ] **Step 17: Commit**

```bash
git add e2e/posix_spec/2_05_03_shell_variables/pwd_after_cd.sh e2e/builtin/cd_*.sh
git commit -m "$(cat <<'EOF'
test(cd): flip PWD XFAIL and add POSIX §4 cd E2E coverage

- e2e/posix_spec/.../pwd_after_cd.sh: XFAIL removed; now PASSes.
- e2e/builtin/cd_*.sh: 11 new tests for logical default, -P flag,
  lexical ../ resolution, --, invalid option, too-many-args, cd -
  stdout printing, CDPATH basic/empty-entry/miss, OLDPWD logical.

Task 5/6 of the POSIX-complete cd rewrite.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Clean up TODO.md and run final verification

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Remove the completed TODO entry**

Edit `TODO.md`. Delete this exact line (project convention: delete
rather than mark `[x]`):

```
- [ ] §2.5.3 PWD logical path — `cd` resolves PWD to the physical path (e.g., `/tmp` -> `/private/tmp` on macOS); POSIX `cd` without `-P` shall preserve the logical path unless dot-dot resolution occurs (see `e2e/posix_spec/2_05_03_shell_variables/pwd_after_cd.sh` XFAIL)
```

- [ ] **Step 2: Run the full test suite**

Run:
```bash
cargo test --lib 2>&1 | tail -5
cargo build 2>&1 | tail -3
./e2e/run_tests.sh 2>&1 | tail -5
```

Expected:
- `cargo test`: `test result: ok.` everywhere.
- `cargo build`: no errors.
- E2E summary: `XFail: 3, XPass: 0, Failed: 0, Timedout: 0`. The three
  remaining XFAILs are the §2.6.1 tilde, §2.10 empty compound_list,
  and §2.5.3 LINENO gaps (sub-projects 2–4).

- [ ] **Step 3: Run clippy and fmt**

Run:
```bash
cargo fmt --check 2>&1 | head -20
cargo clippy --all-targets 2>&1 | tail -20
```

Expected: `cargo fmt --check` prints nothing and returns 0; clippy
reports no new warnings in `src/builtin/regular.rs`.

If fmt reports diffs: run `cargo fmt` and include the changes in
this task's commit.

- [ ] **Step 4: Commit**

```bash
git add TODO.md
# Include any cargo-fmt result if it touched files.
git commit -m "$(cat <<'EOF'
chore(cd): remove completed §2.5.3 PWD TODO entry

The PWD logical-path conformance gap is closed by the POSIX-complete cd
rewrite (tasks 1-5). Per project convention, completed TODO entries are
deleted rather than marked [x].

Task 6/6 of the POSIX-complete cd rewrite.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Completion Criteria (verify once all tasks are done)

1. `cargo test --lib` — all green.
2. `cargo clippy --all-targets` — no new warnings in `src/builtin/regular.rs`.
3. `cargo fmt --check` — clean.
4. `./e2e/run_tests.sh` summary line shows: `XFail: 3, XPass: 0, Failed: 0, Timedout: 0`.
5. `git log --oneline | head -10` shows six focused commits (Tasks 1–6), each with its task number in the body.
6. `TODO.md` no longer lists the `§2.5.3 PWD logical path` item.
