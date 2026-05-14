# SP2 Required-Builtin Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close 5 XFAIL tests in `e2e/posix_spec/4_required_builtin/` by (1) adding `jobs` option/spec diagnostics, (2) implementing native `type` builtin, and (3) implementing native `hash` builtin with a per-shell utility-location cache and PATH-change invalidation.

**Architecture:** Three independent groups in sequence: G1 (jobs, state-free) → G2 (type, state-free) → G3 (hash, introduces `ShellEnv.utility_hash` + reroutes `lookup_in_path`). Each group is a single commit; SP2 closes with a fourth `TODO.md` cleanup commit.

**Tech Stack:** Rust 2024 edition, `nix` for unistd/wait, `tempfile` for tests. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-05-14-e2e-xfail-sp2-required-builtin-design.md`

---

## Group G1 — `jobs` builtin diagnostics

### Task 1: Add `JobsOpts` struct and `parse_options` helper

**Files:**
- Modify: `src/exec/job_control.rs` (add before `impl Executor` block, around line 30)

- [ ] **Step 1: Write the failing unit tests**

Add these tests inside the existing `#[cfg(test)] mod tests` block at the bottom of `src/exec/job_control.rs`. If no test module exists yet, append this block:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_options_recognizes_long_flag() {
        let args = vec!["-l".to_string()];
        let opts = parse_options(&args).unwrap();
        assert!(opts.long_format);
        assert!(!opts.pgid_only);
        assert_eq!(opts.operands, Vec::<String>::new());
    }

    #[test]
    fn parse_options_recognizes_pgid_flag() {
        let args = vec!["-p".to_string()];
        let opts = parse_options(&args).unwrap();
        assert!(!opts.long_format);
        assert!(opts.pgid_only);
    }

    #[test]
    fn parse_options_clustered_flags() {
        let args = vec!["-lp".to_string()];
        let opts = parse_options(&args).unwrap();
        assert!(opts.long_format);
        assert!(opts.pgid_only);
    }

    #[test]
    fn parse_options_double_dash_ends_flags() {
        let args = vec!["--".to_string(), "%1".to_string()];
        let opts = parse_options(&args).unwrap();
        assert_eq!(opts.operands, vec!["%1".to_string()]);
    }

    #[test]
    fn parse_options_rejects_unknown_flag() {
        let args = vec!["-x".to_string()];
        let err = parse_options(&args).unwrap_err();
        assert!(err.contains("jobs:") && err.contains("-x"));
    }

    #[test]
    fn parse_options_collects_operands_after_flags() {
        let args = vec!["-l".to_string(), "%1".to_string(), "%2".to_string()];
        let opts = parse_options(&args).unwrap();
        assert!(opts.long_format);
        assert_eq!(opts.operands, vec!["%1".to_string(), "%2".to_string()]);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib exec::job_control::tests
```

Expected: compile errors (parse_options / JobsOpts not defined).

- [ ] **Step 3: Add the `JobsOpts` struct and `parse_options` function**

Insert after the `strip_job_spec_prefix` function (around line 28 of `src/exec/job_control.rs`):

```rust
/// Parsed form of a `jobs [-l|-p] [--] [job_spec...]` invocation.
struct JobsOpts {
    long_format: bool,
    pgid_only: bool,
    operands: Vec<String>,
}

/// Parse `jobs` flags + operands. Returns `Err(message)` on unknown
/// option; `message` is already prefixed (e.g., `"jobs: -x: invalid option"`)
/// for the caller to write to stderr verbatim.
fn parse_options(args: &[String]) -> Result<JobsOpts, String> {
    let mut long_format = false;
    let mut pgid_only = false;
    let mut idx = 0;

    while idx < args.len() {
        let a = &args[idx];
        if a == "--" {
            idx += 1;
            break;
        }
        if !a.starts_with('-') || a == "-" {
            break;
        }
        for ch in a[1..].chars() {
            match ch {
                'l' => long_format = true,
                'p' => pgid_only = true,
                other => return Err(format!("jobs: -{}: invalid option", other)),
            }
        }
        idx += 1;
    }

    let operands = args[idx..].to_vec();
    Ok(JobsOpts {
        long_format,
        pgid_only,
        operands,
    })
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib exec::job_control::tests
```

Expected: all 6 tests PASS.

- [ ] **Step 5: Commit not yet** — bundled with Task 2 / Task 3 into one G1 commit.

---

### Task 2: Rewrite `builtin_jobs` to use options + validate specs

**Files:**
- Modify: `src/exec/job_control.rs:174-203` (replace current `builtin_jobs` body)

- [ ] **Step 1: Replace `builtin_jobs` implementation**

Replace the existing `builtin_jobs` method (lines 174-203) with:

```rust
    pub(super) fn builtin_jobs(&mut self, args: &[String]) -> Result<i32, ShellError> {
        let opts = match parse_options(args) {
            Ok(o) => o,
            Err(msg) => {
                eprintln!("yosh: {}", msg);
                return Ok(1);
            }
        };

        // Decide which job IDs to print.
        let mut exit_status = 0;
        let job_ids: Vec<crate::env::jobs::JobId> = if opts.operands.is_empty() {
            self.env.process.jobs.all_jobs().map(|j| j.id).collect()
        } else {
            let mut resolved = Vec::with_capacity(opts.operands.len());
            for spec in &opts.operands {
                match self.env.process.jobs.resolve_job_spec(spec) {
                    Ok(id) => resolved.push(id),
                    Err(JobSpecError::Ambiguous) => {
                        let display = strip_job_spec_prefix(spec);
                        eprintln!("yosh: jobs: {}: ambiguous job spec", display);
                        exit_status = 1;
                    }
                    Err(_) => {
                        eprintln!("yosh: jobs: {}: no such job", spec);
                        exit_status = 1;
                    }
                }
            }
            resolved
        };

        for id in &job_ids {
            if opts.pgid_only {
                if let Some(job) = self.env.process.jobs.get(*id) {
                    println!("{}", job.pgid.as_raw());
                }
            } else if opts.long_format {
                if let Some(line) = self.env.process.jobs.format_job_long(*id) {
                    println!("{}", line);
                }
            } else if let Some(line) = self.env.process.jobs.format_job(*id) {
                println!("{}", line);
            }
        }

        // Mark done/terminated jobs as notified.
        let pending = self.env.process.jobs.pending_notifications();
        for id in pending {
            self.env.process.jobs.mark_notified(id);
        }

        Ok(exit_status)
    }
```

- [ ] **Step 2: Run all unit tests to verify no regression**

```bash
cargo test --lib
```

Expected: existing tests still pass; `parse_options_*` tests still pass; no new failures.

- [ ] **Step 3: Commit not yet** — bundled with Task 1 / Task 3.

---

### Task 3: Remove XFAIL from G1 E2E tests and commit

**Files:**
- Modify: `e2e/posix_spec/4_required_builtin/jobs_unknown_spec.sh:6` (delete XFAIL line)
- Modify: `e2e/posix_spec/4_required_builtin/jobs_invalid_option.sh:6` (delete XFAIL line)

- [ ] **Step 1: Remove the `# XFAIL: …` line from `jobs_unknown_spec.sh`**

Current line 6: `# XFAIL: non-POSIX deviation (yosh returns 0 for unknown job spec)`

Delete that line entirely.

- [ ] **Step 2: Remove the `# XFAIL: …` line from `jobs_invalid_option.sh`**

Current line 6: `# XFAIL: non-POSIX deviation (yosh returns 0 for unknown option)`

Delete that line entirely.

- [ ] **Step 3: Verify both E2E tests pass**

```bash
cargo build && ./e2e/run_tests.sh --filter=jobs_unknown_spec.sh
./e2e/run_tests.sh --filter=jobs_invalid_option.sh
```

Expected: both PASS.

- [ ] **Step 4: Verify no E2E regression**

```bash
./e2e/run_tests.sh --filter=jobs_
```

Expected: all `jobs_*.sh` tests PASS.

- [ ] **Step 5: Run unit tests one more time**

```bash
cargo test --lib
```

Expected: green.

- [ ] **Step 6: Commit G1**

```bash
git add src/exec/job_control.rs \
  e2e/posix_spec/4_required_builtin/jobs_unknown_spec.sh \
  e2e/posix_spec/4_required_builtin/jobs_invalid_option.sh
git commit -m "$(cat <<'EOF'
fix(builtin): jobs validates options and job specs

Task: TODO.md の SP2 を対応して下さい

Reject unknown options (e.g., `jobs -x`) and unknown job specs (e.g.,
`jobs %99`) with exit 1 and a diagnostic on stderr, matching POSIX
§1.4 jobs requirements. Removes the XFAIL marker from
jobs_unknown_spec.sh and jobs_invalid_option.sh.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Group G2 — Native `type` builtin

### Task 4: Create `src/builtin/type.rs` with `format_type_line` helper

**Files:**
- Create: `src/builtin/type.rs` (referenced from `mod.rs` as `pub mod r#type;` — the file itself is named without the raw-identifier prefix)

- [ ] **Step 1: Create the file with the failing unit tests**

```rust
//! POSIX `type` builtin.
//!
//! `type name...` — for each name, write to stdout how it would be
//! interpreted if used as a command name. Recognizes aliases,
//! reserved words, functions, special/regular builtins, and external
//! commands resolvable via PATH.
//!
//! Output formats match bash/dash conventions:
//! - `<name> is aliased to '<value>'`
//! - `<name> is a shell keyword`
//! - `<name> is a function`
//! - `<name> is a special shell builtin`
//! - `<name> is a shell builtin`
//! - `<name> is <path>`

use crate::builtin::BuiltinKind;
use crate::builtin::resolve::{CommandKind, resolve_command_kind};
use crate::env::ShellEnv;
use crate::error::ShellError;

/// Render the `type` line for a single name.
///
/// Returns `(stdout_line, optional_stderr_line, per_operand_exit)`.
/// `stderr_line` is `Some` only when the name is not found, and
/// `per_operand_exit` is 1 in that case (0 otherwise).
pub(crate) fn format_type_line(env: &ShellEnv, name: &str) -> (String, Option<String>, i32) {
    match resolve_command_kind(env, name) {
        CommandKind::Alias(val) => {
            let escaped = val.replace('\'', r"'\''");
            (
                format!("{} is aliased to '{}'", name, escaped),
                None,
                0,
            )
        }
        CommandKind::Keyword => (format!("{} is a shell keyword", name), None, 0),
        CommandKind::Function => (format!("{} is a function", name), None, 0),
        CommandKind::Builtin(BuiltinKind::Special) => (
            format!("{} is a special shell builtin", name),
            None,
            0,
        ),
        CommandKind::Builtin(BuiltinKind::Regular) => {
            (format!("{} is a shell builtin", name), None, 0)
        }
        CommandKind::Builtin(BuiltinKind::NotBuiltin) => {
            // Cannot happen — resolve_command_kind never returns this.
            (
                String::new(),
                Some(format!("yosh: type: {}: not found", name)),
                1,
            )
        }
        CommandKind::External(p) => (format!("{} is {}", name, p.to_string_lossy()), None, 0),
        CommandKind::NotFound => (
            String::new(),
            Some(format!("yosh: type: {}: not found", name)),
            1,
        ),
    }
}

/// Execute `type` with the given arguments.
pub fn builtin_type(args: &[String], env: &ShellEnv) -> Result<i32, ShellError> {
    if args.is_empty() {
        eprintln!("yosh: type: usage: type name...");
        return Ok(2);
    }

    let mut exit_status = 0;
    for name in args {
        let (stdout_line, stderr_line, per_exit) = format_type_line(env, name);
        if !stdout_line.is_empty() {
            println!("{}", stdout_line);
        }
        if let Some(s) = stderr_line {
            eprintln!("{}", s);
        }
        if per_exit != 0 {
            exit_status = per_exit;
        }
    }
    Ok(exit_status)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_with_path(path: &str) -> ShellEnv {
        let mut env = ShellEnv::new("yosh", vec![]);
        let _ = env.vars.set("PATH", path);
        env
    }

    #[test]
    fn alias_line() {
        let mut env = env_with_path("/bin:/usr/bin");
        env.aliases.set("ll", "ls -l");
        let (out, err, ex) = format_type_line(&env, "ll");
        assert_eq!(out, "ll is aliased to 'ls -l'");
        assert!(err.is_none());
        assert_eq!(ex, 0);
    }

    #[test]
    fn alias_single_quote_escaping() {
        let mut env = env_with_path("/bin:/usr/bin");
        env.aliases.set("q", "echo 'hi'");
        let (out, _, _) = format_type_line(&env, "q");
        assert_eq!(out, r"q is aliased to 'echo '\''hi'\'''");
    }

    #[test]
    fn keyword_line() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, _, ex) = format_type_line(&env, "if");
        assert_eq!(out, "if is a shell keyword");
        assert_eq!(ex, 0);
    }

    #[test]
    fn function_line() {
        use crate::parser::ast::{CompoundCommand, CompoundCommandKind, FunctionDef};
        let mut env = env_with_path("/bin:/usr/bin");
        env.functions.insert(
            "myfn".to_string(),
            FunctionDef {
                name: "myfn".to_string(),
                body: CompoundCommand {
                    kind: CompoundCommandKind::BraceGroup { body: Vec::new() },
                    line: 0,
                },
                line: 0,
            },
        );
        let (out, _, ex) = format_type_line(&env, "myfn");
        assert_eq!(out, "myfn is a function");
        assert_eq!(ex, 0);
    }

    #[test]
    fn special_builtin_line() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, _, _) = format_type_line(&env, "export");
        assert_eq!(out, "export is a special shell builtin");
    }

    #[test]
    fn regular_builtin_line() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, _, _) = format_type_line(&env, "cd");
        assert_eq!(out, "cd is a shell builtin");
    }

    #[test]
    fn external_line() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, _, ex) = format_type_line(&env, "sh");
        assert!(out.starts_with("sh is "));
        assert!(out.contains("sh"));
        assert_eq!(ex, 0);
    }

    #[test]
    fn not_found_line() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, err, ex) = format_type_line(&env, "definitely_no_such_cmd_12345");
        assert_eq!(out, "");
        assert_eq!(
            err.unwrap(),
            "yosh: type: definitely_no_such_cmd_12345: not found"
        );
        assert_eq!(ex, 1);
    }

    #[test]
    fn builtin_type_no_args_returns_usage_error() {
        let env = env_with_path("/bin:/usr/bin");
        let r = builtin_type(&[], &env).unwrap();
        assert_eq!(r, 2);
    }

    #[test]
    fn builtin_type_multi_operand_mixed_success_and_not_found() {
        let env = env_with_path("/bin:/usr/bin");
        let args = vec!["cd".to_string(), "definitely_no_such_cmd_xyz".to_string()];
        let r = builtin_type(&args, &env).unwrap();
        assert_eq!(r, 1);
    }

    #[test]
    fn builtin_type_all_found_returns_zero() {
        let env = env_with_path("/bin:/usr/bin");
        let args = vec!["cd".to_string(), "export".to_string()];
        let r = builtin_type(&args, &env).unwrap();
        assert_eq!(r, 0);
    }
}
```

- [ ] **Step 2: Run tests — they will not compile yet because the module isn't declared**

```bash
cargo test --lib builtin::type
```

Expected: compile failure (`mod r#type` not declared in `builtin/mod.rs`).

- [ ] **Step 3: Move on to Task 5** to wire the module before re-running.

---

### Task 5: Wire `type` into `src/builtin/mod.rs`

**Files:**
- Modify: `src/builtin/mod.rs:1-5` (add `pub mod r#type;`)
- Modify: `src/builtin/mod.rs:10-16` (`BUILTIN_NAMES`)
- Modify: `src/builtin/mod.rs:31-39` (`classify_builtin`)
- Modify: `src/builtin/mod.rs:42-80` (`exec_regular_builtin`)

- [ ] **Step 1: Add module declaration**

At the top of `src/builtin/mod.rs`, change the existing list:

```rust
pub mod command;
pub mod regular;
pub mod resolve;
pub mod special;
pub mod test;
```

to:

```rust
pub mod command;
pub mod regular;
pub mod resolve;
pub mod special;
pub mod test;
pub mod r#type;
```

- [ ] **Step 2: Append `"type"` to `BUILTIN_NAMES`**

Change (current lines 10-16):

```rust
pub const BUILTIN_NAMES: &[&str] = &[
    // Special builtins
    "break", ":", "continue", ".", "eval", "exec", "exit", "export", "readonly", "return", "set",
    "shift", "times", "trap", "unset", "fc", // Regular builtins
    "cd", "command", "echo", "true", "false", "alias", "unalias", "kill", "wait", "fg", "bg",
    "jobs", "umask", "test", "[",
];
```

to:

```rust
pub const BUILTIN_NAMES: &[&str] = &[
    // Special builtins
    "break", ":", "continue", ".", "eval", "exec", "exit", "export", "readonly", "return", "set",
    "shift", "times", "trap", "unset", "fc", // Regular builtins
    "cd", "command", "echo", "true", "false", "alias", "unalias", "kill", "wait", "fg", "bg",
    "jobs", "umask", "test", "[", "type",
];
```

- [ ] **Step 3: Add `"type"` to `classify_builtin` Regular arm**

Change (current lines 33-36):

```rust
        "cd" | "command" | "echo" | "true" | "false" | "alias" | "unalias" | "kill" | "wait"
        | "fg" | "bg" | "jobs" | "umask" | "test" | "[" => BuiltinKind::Regular,
```

to:

```rust
        "cd" | "command" | "echo" | "true" | "false" | "alias" | "unalias" | "kill" | "wait"
        | "fg" | "bg" | "jobs" | "umask" | "test" | "[" | "type" => BuiltinKind::Regular,
```

- [ ] **Step 4: Dispatch `"type"` in `exec_regular_builtin`**

In `exec_regular_builtin` (after the `"test" | "[" => Ok(test::builtin_test(name, args)),` arm, before the `_ =>` fallback), add:

```rust
        "type" => r#type::builtin_type(args, env),
```

So the match looks like:

```rust
        "test" | "[" => Ok(test::builtin_test(name, args)),
        "type" => r#type::builtin_type(args, env),
        _ => {
```

- [ ] **Step 5: Run unit tests**

```bash
cargo test --lib builtin::type
```

Expected: all 11 tests in `builtin::r#type::tests` PASS.

```bash
cargo test --lib builtin::tests
```

Expected: `test_builtin_names_consistent_with_classify` still passes (it walks `BUILTIN_NAMES` and checks `classify_builtin != NotBuiltin`).

- [ ] **Step 6: Run full test suite**

```bash
cargo test --lib
```

Expected: green.

- [ ] **Step 7: Commit not yet** — bundled with Task 6.

---

### Task 6: Remove XFAIL from G2 E2E tests and commit

**Files:**
- Modify: `e2e/posix_spec/4_required_builtin/type_alias.sh:4` (delete XFAIL line)
- Modify: `e2e/posix_spec/4_required_builtin/type_function.sh:4` (delete XFAIL line)

- [ ] **Step 1: Remove XFAIL line from `type_alias.sh`**

Delete line 4: `# XFAIL: non-POSIX deviation (yosh has no native type builtin; ...)`.

- [ ] **Step 2: Remove XFAIL line from `type_function.sh`**

Delete line 4: `# XFAIL: non-POSIX deviation (yosh has no native type builtin; ...)`.

- [ ] **Step 3: Verify all `type_*.sh` E2E tests PASS**

```bash
cargo build && ./e2e/run_tests.sh --filter=type_
```

Expected: all 5 `type_*.sh` tests PASS, no regressions on the previously-passing 3.

- [ ] **Step 4: Run unit tests**

```bash
cargo test --lib
```

Expected: green.

- [ ] **Step 5: Commit G2**

```bash
git add src/builtin/type.rs src/builtin/mod.rs \
  e2e/posix_spec/4_required_builtin/type_alias.sh \
  e2e/posix_spec/4_required_builtin/type_function.sh
git commit -m "$(cat <<'EOF'
feat(builtin): native type builtin

Task: TODO.md の SP2 を対応して下さい

Adds a native POSIX `type` builtin that resolves names through
yosh's own alias / function / builtin / PATH lookup state rather
than delegating to /usr/bin/type. Reuses resolve_command_kind so
classification stays consistent with `command -V`.

Removes XFAIL from type_alias.sh and type_function.sh.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Group G3 — Native `hash` builtin + utility-hash cache

### Task 7: Add `utility_hash` field + `assign_var` / `unset_var` helpers

**Files:**
- Modify: `src/env/mod.rs:8-46` (imports + ShellEnv struct + new())
- Modify: `src/env/mod.rs:82-107` (add tests)

- [ ] **Step 1: Write the failing tests**

Append to the existing `#[cfg(test)] mod tests` block in `src/env/mod.rs`:

```rust
    #[test]
    fn assign_var_clears_utility_hash_on_path_change() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.utility_hash
            .insert("foo".to_string(), std::path::PathBuf::from("/bin/foo"));
        env.assign_var("PATH", "/new").unwrap();
        assert!(env.utility_hash.is_empty());
    }

    #[test]
    fn assign_var_leaves_utility_hash_for_non_path_var() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.utility_hash
            .insert("foo".to_string(), std::path::PathBuf::from("/bin/foo"));
        env.assign_var("OTHER", "x").unwrap();
        assert_eq!(env.utility_hash.len(), 1);
    }

    #[test]
    fn unset_var_clears_utility_hash_on_path_unset() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.assign_var("PATH", "/x").unwrap();
        env.utility_hash
            .insert("foo".to_string(), std::path::PathBuf::from("/bin/foo"));
        env.unset_var("PATH").unwrap();
        assert!(env.utility_hash.is_empty());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib env::tests
```

Expected: compile errors (`utility_hash` field absent, `assign_var` / `unset_var` undefined).

- [ ] **Step 3: Add `PathBuf` to imports**

In `src/env/mod.rs` near line 9, change:

```rust
use std::collections::HashMap;
use std::sync::OnceLock;
```

to:

```rust
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
```

- [ ] **Step 4: Add `utility_hash` field to `ShellEnv`**

Inside `pub struct ShellEnv { ... }`, add this field after `default_path_cache`:

```rust
    /// POSIX hash table: utility name → resolved absolute path.
    /// Auto-populated by `find_in_path` / `lookup_in_path` cache misses
    /// and by explicit `hash utility...` invocations. Cleared by
    /// `hash -r` and on `PATH` reassignment (POSIX §2.5.3).
    pub utility_hash: HashMap<String, PathBuf>,
```

- [ ] **Step 5: Initialize `utility_hash` in `ShellEnv::new`**

In `ShellEnv::new`, after the `default_path_cache: OnceLock::new(),` line, add:

```rust
            utility_hash: HashMap::new(),
```

- [ ] **Step 6: Add `assign_var` and `unset_var` helpers**

Inside `impl ShellEnv { ... }`, after the `pub fn new(...)` definition, add:

```rust
    /// Set a shell variable. If `name == "PATH"`, the utility hash
    /// table is cleared after the successful assignment (POSIX
    /// §2.5.3). Returns `Err` only if the variable is readonly.
    pub fn assign_var(
        &mut self,
        name: &str,
        value: impl Into<String>,
    ) -> Result<(), String> {
        self.vars.set(name, value)?;
        if name == "PATH" {
            self.utility_hash.clear();
        }
        Ok(())
    }

    /// Unset a shell variable. If `name == "PATH"`, the utility hash
    /// table is cleared after the successful unset.
    pub fn unset_var(&mut self, name: &str) -> Result<(), String> {
        self.vars.unset(name)?;
        if name == "PATH" {
            self.utility_hash.clear();
        }
        Ok(())
    }
```

- [ ] **Step 7: Run tests**

```bash
cargo test --lib env::tests
```

Expected: 3 new tests PASS, existing tests still PASS.

- [ ] **Step 8: Run full unit suite**

```bash
cargo test --lib
```

Expected: green.

- [ ] **Step 9: Commit not yet** — G3 is one bundled commit.

---

### Task 8: Migrate user-facing PATH-mutation call sites to `assign_var` / `unset_var`

**Files:**
- Modify: `src/builtin/special.rs:109` (`builtin_export`)
- Modify: `src/builtin/special.rs:165` (`builtin_unset`)
- Modify: `src/exec/simple.rs:586` (runtime assignment)
- Modify: `src/exec/simple.rs:599` (runtime unset path)
- Modify: `src/builtin/command.rs:147` (`command -p` temp PATH override)
- Modify: `src/builtin/resolve.rs:65` (CDPATH/`-p` temp PATH override)

**Out of scope for this migration** (acknowledge as follow-up TODO at the end of the SP2 closure):
- `src/expand/param.rs:75` (`${var:=...}` expansion)
- `src/expand/arith.rs:591,603` (arithmetic assignment)
- `src/exec/compound.rs:229` (`for var in ...` loop variable)
- `src/plugin/host/variables.rs:19,35` (plugin-set variables)
- `src/env/default_path.rs:72,155` (bootstrap-time PATH default; cache is empty here)
- `src/exec/simple.rs:59` (`LINENO`), `compound.rs:22` (same)
- `src/interactive/mod.rs:71-74` (HIST* startup, never PATH)
- `src/builtin/regular.rs:62-63` (PWD/OLDPWD only)
- `src/builtin/regular.rs:697,700` (env-only exec child scope)

- [ ] **Step 1: Read `src/builtin/special.rs:100-120` to confirm the `builtin_export` assignment branch**

```bash
sed -n '100,120p' src/builtin/special.rs
```

- [ ] **Step 2: Change `builtin_export` assignment to use `assign_var`**

In `src/builtin/special.rs` around line 109, change:

```rust
            if let Err(e) = env.vars.set(name, raw_value) {
```

to:

```rust
            if let Err(e) = env.assign_var(name, raw_value) {
```

(The rest of the error-handling expression stays the same.)

- [ ] **Step 3: Change `builtin_unset` unset call to use `unset_var`**

In `src/builtin/special.rs` around line 165, change:

```rust
        } else if let Err(e) = env.vars.unset(name) {
```

to:

```rust
        } else if let Err(e) = env.unset_var(name) {
```

- [ ] **Step 4: Change runtime assignment in `src/exec/simple.rs:586`**

Locate the line:

```rust
            let _ = self.env.vars.set(&assignment.name, value);
```

Change to:

```rust
            let _ = self.env.assign_var(&assignment.name, value);
```

- [ ] **Step 5: Change runtime path at `src/exec/simple.rs:596-599`**

The current block (around lines 593-600) is:

```rust
                if let Some(val) = old_val {
                    let _ = self.env.vars.set(&name, val);
                } else {
                    let _ = self.env.vars.unset(&name);
                }
```

Change both calls:

```rust
                if let Some(val) = old_val {
                    let _ = self.env.assign_var(&name, val);
                } else {
                    let _ = self.env.unset_var(&name);
                }
```

- [ ] **Step 6: Read `src/builtin/command.rs:140-170` to find the temp-PATH save/restore pattern**

```bash
sed -n '140,170p' src/builtin/command.rs
```

You should see a pattern that:
1. Saves old PATH
2. Sets PATH to default via `env.vars.set("PATH", path)`
3. Performs the lookup
4. Restores old PATH via another `env.vars.set("PATH", saved)` or unset

- [ ] **Step 7: Change both `vars.set("PATH", ...)` calls in `src/builtin/command.rs:147` and the matching restore to use `assign_var`**

Change every `env.vars.set("PATH", ...)` in this block to `env.assign_var("PATH", ...)`. Same for restore. If the restore path uses `env.vars.unset("PATH")`, change to `env.unset_var("PATH")`.

(Cache clearing each round is correct: the explicit `command -p` is intentionally invalidating any hash from before, and clearing again on restore prevents stale entries from leaking the default-PATH lookup.)

- [ ] **Step 8: Similarly migrate `src/builtin/resolve.rs:65`**

In `src/builtin/resolve.rs` around line 65 (the `command -p`-style temporary PATH override):

```rust
        let _ = env.vars.set("PATH", path);
```

Change to:

```rust
        let _ = env.assign_var("PATH", path);
```

(Audit the surrounding lines to ensure there's no symmetric restore that also needs updating; if there is, do both.)

- [ ] **Step 9: Run unit tests to confirm no breakage**

```bash
cargo test --lib
```

Expected: green.

- [ ] **Step 10: Audit that no `env.vars.set("PATH"` call sites remain in production code**

```bash
grep -rn 'env\.vars\.set("PATH"' src/ | grep -v 'tests' | grep -v 'src/env/default_path.rs'
```

Expected: no output (zero hits). The `default_path.rs` exception is documented in Step 0 above.

- [ ] **Step 11: Commit not yet** — G3 bundle.

---

### Task 9: Extend `find_in_path` / `lookup_in_path` to take a cache and auto-hash

**Files:**
- Modify: `src/exec/command.rs:10-26` (`find_in_path`)
- Modify: `src/exec/command.rs:43-71` (`lookup_in_path`)
- Modify: `src/exec/command.rs:86-149` (tests)

- [ ] **Step 1: Write the failing tests**

Add the following tests inside the existing `#[cfg(test)] mod tests` block of `src/exec/command.rs`:

```rust
    #[test]
    fn find_in_path_cache_hit_returns_cached_path() {
        use std::collections::HashMap;
        let path_var = env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".to_string());
        let mut cache = HashMap::new();
        let canonical_sh = find_in_path("sh", &path_var, &mut cache).unwrap();
        // Cache should now contain "sh".
        assert_eq!(cache.get("sh"), Some(&canonical_sh));

        // Subsequent call must return the same path.
        let again = find_in_path("sh", &path_var, &mut cache).unwrap();
        assert_eq!(again, canonical_sh);
    }

    #[test]
    fn find_in_path_skips_cache_for_slash_paths() {
        use std::collections::HashMap;
        let mut cache = HashMap::new();
        // /bin/sh exists on macOS and Linux; the slash form bypasses cache.
        let _ = find_in_path("/bin/sh", "/bin:/usr/bin", &mut cache);
        assert!(cache.is_empty());
    }

    #[test]
    fn find_in_path_falls_back_when_cached_file_missing() {
        use std::collections::HashMap;
        use std::path::PathBuf;
        let mut cache = HashMap::new();
        cache.insert(
            "sh".to_string(),
            PathBuf::from("/nonexistent/fake_sh_12345"),
        );
        let path_var = env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".to_string());
        let result = find_in_path("sh", &path_var, &mut cache);
        // Must fall through to PATH walk and find real sh.
        assert!(result.is_some());
        let p = result.unwrap();
        assert!(p.exists());
        // Cache should be refreshed to the real path.
        assert_eq!(cache.get("sh"), Some(&p));
    }
```

- [ ] **Step 2: Run tests to verify they fail to compile**

```bash
cargo test --lib exec::command::tests
```

Expected: compile errors (signature mismatch — the existing fns don't take `cache`).

- [ ] **Step 3: Update `find_in_path` signature and add cache logic**

Replace `find_in_path` (current lines 10-26) with:

```rust
/// Search each directory in `path_var` for `cmd`, consulting a cache first.
///
/// If `cmd` contains '/', the cache is bypassed (POSIX: pathnames with
/// '/' are not subject to PATH search). On a cache hit whose path still
/// exists and is executable, the cached path is returned without
/// re-walking PATH. On miss or stale cache entry, falls through to the
/// PATH walk and inserts a fresh entry on success (auto-hash).
pub fn find_in_path(
    cmd: &str,
    path_var: &str,
    cache: &mut std::collections::HashMap<String, std::path::PathBuf>,
) -> Option<PathBuf> {
    if cmd.contains('/') {
        return walk_path(cmd, path_var);
    }
    if let Some(cached) = cache.get(cmd)
        && is_executable_file(cached)
    {
        return Some(cached.clone());
    }
    let found = walk_path(cmd, path_var)?;
    cache.insert(cmd.to_string(), found.clone());
    Some(found)
}

fn walk_path(cmd: &str, path_var: &str) -> Option<PathBuf> {
    for dir in path_var.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = PathBuf::from(dir).join(cmd);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable_file(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    if !p.is_file() {
        return false;
    }
    matches!(
        std::fs::metadata(p),
        Ok(meta) if meta.permissions().mode() & 0o111 != 0
    )
}
```

- [ ] **Step 4: Update `lookup_in_path` signature and add cache logic**

Replace `lookup_in_path` (current lines 43-71) with:

```rust
/// Walk each directory in `path_var` and report whether `cmd` exists and
/// is executable. Unlike [`find_in_path`], this distinguishes the
/// "exists but not executable" case so callers can return the correct
/// POSIX exit status (126 vs 127). Cache is consulted only for the
/// `Executable` case; non-executable hits are not cached.
pub fn lookup_in_path(
    cmd: &str,
    path_var: &str,
    cache: &mut std::collections::HashMap<String, std::path::PathBuf>,
) -> PathLookup {
    if !cmd.contains('/')
        && let Some(cached) = cache.get(cmd)
        && is_executable_file(cached)
    {
        return PathLookup::Executable(cached.clone());
    }
    let mut seen_non_exec: Option<PathBuf> = None;
    for dir in path_var.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = PathBuf::from(dir).join(cmd);
        if !candidate.is_file() {
            continue;
        }
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(&candidate) {
            Ok(meta) if meta.permissions().mode() & 0o111 != 0 => {
                if !cmd.contains('/') {
                    cache.insert(cmd.to_string(), candidate.clone());
                }
                return PathLookup::Executable(candidate);
            }
            Ok(_) => {
                if seen_non_exec.is_none() {
                    seen_non_exec = Some(candidate);
                }
            }
            Err(_) => continue,
        }
    }
    match seen_non_exec {
        Some(p) => PathLookup::NotExecutable(p),
        None => PathLookup::NotFound,
    }
}
```

- [ ] **Step 5: Update the existing tests in `src/exec/command.rs` to pass a `HashMap`**

The existing tests (`find_in_path_finds_sh`, `find_in_path_returns_none_for_nonexistent`, `lookup_in_path_finds_executable`, `lookup_in_path_reports_not_found_for_missing`, `lookup_in_path_reports_not_executable`) all call without a cache. Update each to pass an empty `HashMap::new()`.

For example, the existing:

```rust
    #[test]
    fn find_in_path_finds_sh() {
        let path_var = env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".to_string());
        let result = find_in_path("sh", &path_var);
        assert!(result.is_some(), "should find 'sh' in PATH");
    }
```

becomes:

```rust
    #[test]
    fn find_in_path_finds_sh() {
        use std::collections::HashMap;
        let path_var = env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".to_string());
        let mut cache = HashMap::new();
        let result = find_in_path("sh", &path_var, &mut cache);
        assert!(result.is_some(), "should find 'sh' in PATH");
    }
```

Apply the same shape (declare local cache, pass `&mut cache`) to all 5 existing tests in this file.

- [ ] **Step 6: Run tests**

```bash
cargo test --lib exec::command::tests
```

Expected: all 8 tests (5 existing + 3 new) PASS.

- [ ] **Step 7: Commit not yet** — G3 bundle.

---

### Task 10: Update all callers of `find_in_path` / `lookup_in_path`

**Files:**
- Modify: `src/exec/simple.rs:679` (calls `lookup_in_path`)
- Modify: `src/exec/simple.rs:725` (calls `lookup_in_path`)
- Modify: `src/builtin/resolve.rs:52` (calls `find_in_path`)
- Modify: `src/builtin/special.rs:394` (calls `find_in_path`)
- Modify: `src/env/default_path.rs:137` (calls `find_in_path` in a test)

- [ ] **Step 1: Update `src/exec/simple.rs:679`**

Locate the line (full context: this is inside a method on `Executor`, so `self.env` is available):

```rust
        match lookup_in_path(name, &dp) {
```

Change to:

```rust
        match lookup_in_path(name, &dp, &mut self.env.utility_hash) {
```

- [ ] **Step 2: Update `src/exec/simple.rs:725`**

Same shape:

```rust
        match lookup_in_path(name, &path_var) {
```

→

```rust
        match lookup_in_path(name, &path_var, &mut self.env.utility_hash) {
```

- [ ] **Step 3: Update `src/builtin/resolve.rs:52`**

Read lines 36-60 first to understand the function signature.

```bash
sed -n '36,60p' src/builtin/resolve.rs
```

`resolve_command_kind(env: &ShellEnv, name)` takes an immutable `&ShellEnv`. To thread a mutable cache through, change the signature to:

```rust
pub fn resolve_command_kind(env: &mut ShellEnv, name: &str) -> CommandKind {
```

And update the body's `find_in_path` call (line 52, in the `if let Some(path_var) = env.vars.get("PATH")` branch):

```rust
        && let Some(p) = find_in_path(name, path_var)
```

→

```rust
        && let Some(p) = find_in_path(name, path_var, &mut env.utility_hash)
```

Wait — `env.vars.get("PATH")` borrows `env` immutably, and the same expression can't simultaneously borrow `&mut env.utility_hash`. Restructure:

```rust
    // Replace lines 50-55 (the External branch) with:
    if let Some(path_var) = env.vars.get("PATH").map(|s| s.to_string()) {
        if let Some(p) = find_in_path(name, &path_var, &mut env.utility_hash) {
            return CommandKind::External(p);
        }
    }
    CommandKind::NotFound
```

The `.map(|s| s.to_string())` releases the borrow on `env.vars` so the mutable borrow on `env.utility_hash` is valid.

Also update the existing test at `src/builtin/resolve.rs:65` (`env.vars.set("PATH", path)`) — Task 8 already handled this if it's the call site we listed; otherwise change it now:

```bash
sed -n '60,80p' src/builtin/resolve.rs
```

If `env.vars.set("PATH", path)` is still present at line ~65 within this file, change it to `env.assign_var("PATH", path)`.

- [ ] **Step 4: Update callers of `resolve_command_kind`**

Because the signature is now `&mut ShellEnv`, find all callers:

```bash
grep -rn 'resolve_command_kind(' src/
```

Update each (likely in `src/builtin/command.rs::render_brief` and `render_verbose` and in `src/builtin/type.rs::format_type_line`) to take `&mut ShellEnv`.

For `src/builtin/type.rs::format_type_line`, change:

```rust
pub(crate) fn format_type_line(env: &ShellEnv, name: &str) -> (String, Option<String>, i32) {
```

to:

```rust
pub(crate) fn format_type_line(env: &mut ShellEnv, name: &str) -> (String, Option<String>, i32) {
```

And change the `builtin_type` signature accordingly:

```rust
pub fn builtin_type(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
```

Then in `src/builtin/mod.rs::exec_regular_builtin`, the dispatcher already passes `env: &mut ShellEnv` (it's signature `fn exec_regular_builtin(name: &str, args: &[String], env: &mut ShellEnv) -> i32`), so the call `r#type::builtin_type(args, env)` works unchanged.

For `src/builtin/command.rs`, update `render_brief` and `render_verbose` signatures the same way (`&ShellEnv` → `&mut ShellEnv`). Update their callers (search for `render_brief(` and `render_verbose(`) accordingly.

Also update the unit tests in `src/builtin/command.rs::tests` and `src/builtin/resolve.rs::tests` to declare the env as `let mut env = ...` and pass `&mut env`.

Also update the unit tests in `src/builtin/type.rs::tests` similarly:

Change every `format_type_line(&env, name)` / `builtin_type(&args, &env)` to `format_type_line(&mut env, name)` / `builtin_type(&args, &mut env)`. The `env_with_path` helper already returns `ShellEnv` (not `&ShellEnv`) so the local binding can be `let mut env = ...`.

- [ ] **Step 5: Update `src/builtin/special.rs:394`**

The current call:

```rust
        match crate::exec::command::find_in_path(cmd, &path_var) {
```

Change to:

```rust
        match crate::exec::command::find_in_path(cmd, &path_var, &mut env.utility_hash) {
```

(Verify `env` is `&mut ShellEnv` in this method's signature; if not, propagate the mut up the chain to the call site.)

- [ ] **Step 6: Update `src/env/default_path.rs:137` (test only)**

This is inside `#[cfg(test)] mod tests`. Change:

```rust
            find_in_path("sh", dp).is_some(),
```

to:

```rust
            find_in_path("sh", dp, &mut std::collections::HashMap::new()).is_some(),
```

- [ ] **Step 7: Run the full unit test suite**

```bash
cargo test --lib
```

Expected: green. Compilation errors at this stage usually indicate a missed signature update; the compiler error messages will name the file:line.

- [ ] **Step 8: Commit not yet** — bundled with G3.

---

### Task 11: Create `src/builtin/hash.rs`

**Files:**
- Create: `src/builtin/hash.rs`

- [ ] **Step 1: Write the file with failing unit tests + implementation**

```rust
//! POSIX `hash` builtin.
//!
//! `hash [-r] [name...]`
//!
//! - No args: print the utility-hash cache (one path per line, sorted
//!   by name).
//! - `-r`: clear the cache.
//! - `name...`: for each name, record its location. If `name` contains
//!   `/`, the path is taken as-is and validated. Otherwise it is
//!   searched via PATH and inserted into the cache.

use std::path::PathBuf;

use crate::env::ShellEnv;
use crate::error::ShellError;
use crate::exec::command::find_in_path;

pub fn builtin_hash(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // Parse leading -X flags.
    let mut clear = false;
    let mut idx = 0;
    while idx < args.len() {
        let a = &args[idx];
        if a == "--" {
            idx += 1;
            break;
        }
        if !a.starts_with('-') || a == "-" {
            break;
        }
        for ch in a[1..].chars() {
            match ch {
                'r' => clear = true,
                other => {
                    eprintln!("yosh: hash: -{}: invalid option", other);
                    return Ok(1);
                }
            }
        }
        idx += 1;
    }

    let operands = &args[idx..];

    if clear {
        env.utility_hash.clear();
    }

    if operands.is_empty() {
        if clear {
            return Ok(0);
        }
        // List the cache, sorted by name for determinism.
        let mut names: Vec<&String> = env.utility_hash.keys().collect();
        names.sort();
        for name in names {
            if let Some(path) = env.utility_hash.get(name) {
                println!("{}", path.display());
            }
        }
        return Ok(0);
    }

    let mut exit_status = 0;
    for name in operands {
        if name.contains('/') {
            let path = PathBuf::from(name);
            if !is_executable(&path) {
                eprintln!("yosh: hash: {}: not found", name);
                exit_status = 1;
                continue;
            }
            // POSIX: cache the basename; the operand path becomes the value.
            let basename = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| name.clone());
            env.utility_hash.insert(basename, path);
        } else {
            let path_var = env.vars.get("PATH").unwrap_or("").to_string();
            match find_in_path(name, &path_var, &mut env.utility_hash) {
                Some(_) => {
                    // find_in_path already inserted into the cache.
                }
                None => {
                    eprintln!("yosh: hash: {}: not found", name);
                    exit_status = 1;
                }
            }
        }
    }
    Ok(exit_status)
}

fn is_executable(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    if !p.is_file() {
        return false;
    }
    matches!(
        std::fs::metadata(p),
        Ok(meta) if meta.permissions().mode() & 0o111 != 0
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_with_path(path: &str) -> ShellEnv {
        let mut env = ShellEnv::new("yosh", vec![]);
        let _ = env.vars.set("PATH", path);
        env
    }

    #[test]
    fn r_flag_clears_cache() {
        let mut env = env_with_path("/bin:/usr/bin");
        env.utility_hash
            .insert("foo".to_string(), PathBuf::from("/bin/foo"));
        let args = vec!["-r".to_string()];
        let r = builtin_hash(&args, &mut env).unwrap();
        assert_eq!(r, 0);
        assert!(env.utility_hash.is_empty());
    }

    #[test]
    fn slash_path_to_nonexistent_returns_error() {
        let mut env = env_with_path("/bin:/usr/bin");
        let args = vec!["/no/such/cmd_definitely_missing_12345".to_string()];
        let r = builtin_hash(&args, &mut env).unwrap();
        assert_eq!(r, 1);
        assert!(env.utility_hash.is_empty());
    }

    #[test]
    fn name_lookup_succeeds_for_sh() {
        let path_var = std::env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".to_string());
        let mut env = env_with_path(&path_var);
        let args = vec!["sh".to_string()];
        let r = builtin_hash(&args, &mut env).unwrap();
        assert_eq!(r, 0);
        assert!(env.utility_hash.contains_key("sh"));
    }

    #[test]
    fn no_args_empty_cache_returns_zero() {
        let mut env = env_with_path("/bin:/usr/bin");
        let r = builtin_hash(&[], &mut env).unwrap();
        assert_eq!(r, 0);
    }

    #[test]
    fn invalid_option_returns_one() {
        let mut env = env_with_path("/bin:/usr/bin");
        let args = vec!["-x".to_string()];
        let r = builtin_hash(&args, &mut env).unwrap();
        assert_eq!(r, 1);
    }

    #[test]
    fn nonexistent_name_returns_one() {
        let mut env = env_with_path("/bin:/usr/bin");
        let args = vec!["definitely_no_such_cmd_98765".to_string()];
        let r = builtin_hash(&args, &mut env).unwrap();
        assert_eq!(r, 1);
    }

    #[test]
    fn r_with_operand_clears_then_lookups() {
        let path_var = std::env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".to_string());
        let mut env = env_with_path(&path_var);
        env.utility_hash
            .insert("stale".to_string(), PathBuf::from("/old/stale"));
        let args = vec!["-r".to_string(), "sh".to_string()];
        let r = builtin_hash(&args, &mut env).unwrap();
        assert_eq!(r, 0);
        assert!(!env.utility_hash.contains_key("stale"));
        assert!(env.utility_hash.contains_key("sh"));
    }
}
```

- [ ] **Step 2: Run tests — they will not compile yet because the module isn't declared**

```bash
cargo test --lib builtin::hash
```

Expected: compile failure (`mod hash` not declared).

- [ ] **Step 3: Move on to Task 12** to wire the module.

---

### Task 12: Wire `hash` into `src/builtin/mod.rs`

**Files:**
- Modify: `src/builtin/mod.rs:1-6` (module declarations)
- Modify: `src/builtin/mod.rs` `BUILTIN_NAMES`
- Modify: `src/builtin/mod.rs` `classify_builtin`
- Modify: `src/builtin/mod.rs` `exec_regular_builtin`

- [ ] **Step 1: Add module declaration**

In `src/builtin/mod.rs` near the top, change the module list to include `pub mod hash;`:

```rust
pub mod command;
pub mod hash;
pub mod regular;
pub mod resolve;
pub mod special;
pub mod test;
pub mod r#type;
```

- [ ] **Step 2: Append `"hash"` to `BUILTIN_NAMES`**

Change:

```rust
    "cd", "command", "echo", "true", "false", "alias", "unalias", "kill", "wait", "fg", "bg",
    "jobs", "umask", "test", "[", "type",
```

to:

```rust
    "cd", "command", "echo", "true", "false", "alias", "unalias", "kill", "wait", "fg", "bg",
    "jobs", "umask", "test", "[", "type", "hash",
```

- [ ] **Step 3: Add `"hash"` to `classify_builtin` Regular arm**

```rust
        "cd" | "command" | "echo" | "true" | "false" | "alias" | "unalias" | "kill" | "wait"
        | "fg" | "bg" | "jobs" | "umask" | "test" | "[" | "type" | "hash" => BuiltinKind::Regular,
```

- [ ] **Step 4: Dispatch `"hash"` in `exec_regular_builtin`**

Add the arm right after the `"type"` dispatch:

```rust
        "type" => r#type::builtin_type(args, env),
        "hash" => hash::builtin_hash(args, env),
```

- [ ] **Step 5: Run unit tests**

```bash
cargo test --lib builtin::hash
```

Expected: all 7 tests in `builtin::hash::tests` PASS.

- [ ] **Step 6: Run full unit suite**

```bash
cargo test --lib
```

Expected: green.

- [ ] **Step 7: Commit not yet** — bundled with G3.

---

### Task 13: Remove XFAIL from `hash_unknown_cmd.sh` and commit G3

**Files:**
- Modify: `e2e/posix_spec/4_required_builtin/hash_unknown_cmd.sh:4` (delete XFAIL line)

- [ ] **Step 1: Remove the XFAIL line**

Delete line 4: `# XFAIL: non-POSIX deviation (yosh has no native hash builtin; ...)`.

- [ ] **Step 2: Verify all `hash_*.sh` E2E tests PASS**

```bash
cargo build && ./e2e/run_tests.sh --filter=hash_
```

Expected: all 4 `hash_*.sh` tests PASS, including `hash_unknown_cmd.sh`.

- [ ] **Step 3: Verify no `type_*.sh` or `jobs_*.sh` regressions**

```bash
./e2e/run_tests.sh --filter=type_
./e2e/run_tests.sh --filter=jobs_
```

Expected: all PASS.

- [ ] **Step 4: Run the full E2E suite to catch indirect regressions**

```bash
./e2e/run_tests.sh
```

Expected: zero new failures vs. pre-SP2 baseline.

- [ ] **Step 5: Run the full unit suite**

```bash
cargo test --lib
```

Expected: green.

- [ ] **Step 6: Format and lint**

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

Expected: no fmt drift, no new clippy warnings (the `src/plugin/mod.rs:98-99` `doc_lazy_continuation` is pre-existing and out of scope).

- [ ] **Step 7: Commit G3**

```bash
git add src/env/mod.rs src/exec/command.rs \
  src/builtin/hash.rs src/builtin/mod.rs \
  src/builtin/special.rs src/builtin/resolve.rs \
  src/builtin/command.rs src/builtin/type.rs \
  src/exec/simple.rs src/env/default_path.rs \
  e2e/posix_spec/4_required_builtin/hash_unknown_cmd.sh
git commit -m "$(cat <<'EOF'
feat(builtin): native hash with PATH cache invalidation

Task: TODO.md の SP2 を対応して下さい

Adds a native POSIX `hash` builtin and a per-shell utility-hash
table on ShellEnv. find_in_path / lookup_in_path now consult and
populate the cache (auto-hash); resolve_command_kind threads the
cache through. New ShellEnv::assign_var / unset_var helpers clear
the cache on every PATH mutation, routed through builtin_export /
builtin_unset / runtime variable assignment / `command -p`.

Removes XFAIL from hash_unknown_cmd.sh.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Closure

### Task 14: Update `TODO.md` and close SP2

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Delete the SP2 line under `## E2E XFAIL Roadmap`**

Open `TODO.md` and delete the line:

```
- [ ] SP2 — Required-builtin diagnostics + native `type`/`hash` (5 tests)
```

- [ ] **Step 2: Delete the `type name...` entry under `## Future: POSIX Required Builtin Implementation`**

Delete the 4-line block:

```
- [ ] `type name...` — identify command kind (function / builtin / alias
      / external path). Currently uses `/usr/bin/type`. XFAIL tests:
      `e2e/posix_spec/4_required_builtin/type_*.sh` (2 of 5 remain XFAIL —
      session-local aliases and functions not visible to external wrapper)
```

- [ ] **Step 3: Delete the `hash [-r] [cmd]` entry under the same section**

Delete the 4-line block:

```
- [ ] `hash [-r] [cmd]` — utility-location cache. Currently uses
      `/usr/bin/hash`. XFAIL tests:
      `e2e/posix_spec/4_required_builtin/hash_*.sh` (1 of 4 remains XFAIL —
      exit-status mismatch for unknown command)
```

- [ ] **Step 4: Delete the `jobs returns exit 0 for an unknown job spec…` entry under `## Future: POSIX Conformance Bugs`**

Delete the 3-line block:

```
- [ ] `jobs` returns exit 0 for an unknown job spec or unknown option.
      POSIX requires exit 1 with a diagnostic. XFAIL tests:
      `e2e/posix_spec/4_required_builtin/jobs_unknown_spec.sh`,
      `jobs_invalid_option.sh`.
```

- [ ] **Step 5: Append an SP2 follow-ups subsection under `### SP1 follow-ups (non-blocking)`**

Add a new subsection so out-of-scope migration call sites are tracked:

```markdown
### SP2 follow-ups (non-blocking)

- [ ] Migrate remaining variable-setting call sites to `env.assign_var` so
      PATH cache invalidation is total. Pending paths:
      `${var:=value}` in `src/expand/param.rs:75`; arithmetic assignment in
      `src/expand/arith.rs:591,603`; `for` loop variable in
      `src/exec/compound.rs:229`; plugin-set variables in
      `src/plugin/host/variables.rs:19,35`. Each path could in principle
      set `PATH` but no current XFAIL test exercises it.
- [ ] `hash` listing format omits `hits=N` count. POSIX leaves the
      format implementation-defined; bash includes hit counts. Track
      hit counts on the cache entries if a tooling consumer asks
      (`src/builtin/hash.rs`).
```

- [ ] **Step 6: Verify `cargo fmt` is clean**

```bash
cargo fmt --all -- --check
```

Expected: zero output.

- [ ] **Step 7: Run the full e2e suite once more before closing**

```bash
./e2e/run_tests.sh
```

Expected: 5 fewer XFAILs vs. SP2 baseline; zero new failures.

- [ ] **Step 8: Commit the closure**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
chore(sp2): close SP2 — remove roadmap entry and record follow-ups

Task: TODO.md の SP2 を対応して下さい

Removes the SP2 roadmap entry, the `type` and `hash` "Future:
POSIX Required Builtin" entries, and the `jobs` POSIX-conformance
entry. Records SP2 follow-ups (remaining vars.set call sites,
hash hit-count format) under SP2 follow-ups.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Acceptance

SP2 is complete when:

- All 5 SP2 E2E tests PASS under `./e2e/run_tests.sh` with their `# XFAIL: …` lines removed.
- The six previously-passing `type_*.sh` / `hash_*.sh` regression tests still PASS.
- All `jobs_*.sh` E2E tests still PASS (including `jobs %1`-style PTY tests).
- `cargo test --lib` green.
- `cargo fmt --all -- --check` clean.
- `cargo clippy --all-targets -- -D warnings` clean (excluding the pre-existing `src/plugin/mod.rs:98-99` umbrella exception).
- `grep -rn 'env\.vars\.set("PATH"' src/ | grep -v tests | grep -v src/env/default_path.rs` returns zero hits.
- `TODO.md` reflects SP2 closure and the SP2 follow-ups subsection.
- Four commits land in this exact order:
  1. `fix(builtin): jobs validates options and job specs`
  2. `feat(builtin): native type builtin`
  3. `feat(builtin): native hash with PATH cache invalidation`
  4. `chore(sp2): close SP2 — remove roadmap entry and record follow-ups`
