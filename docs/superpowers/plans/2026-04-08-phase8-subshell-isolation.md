# Phase 8: Subshell Environment Isolation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix remaining subshell environment isolation gaps and prove POSIX §2.12 compliance through 36 integration tests.

**Architecture:** Fork-based subshell isolation already handles most POSIX requirements. The primary code fix adds trap reset to pipeline child processes. A comprehensive test suite in `tests/subshell.rs` validates all isolation guarantees across `(...)`, pipelines, `$(...)`, and edge cases.

**Tech Stack:** Rust, nix crate (fork/signal), integration tests via `cargo test`

---

### Task 1: Pipeline trap reset fix

**Files:**
- Modify: `src/exec/pipeline.rs:59-68`

- [ ] **Step 1: Add trap reset in pipeline child process**

In `src/exec/pipeline.rs`, inside the `ForkResult::Child` branch of `exec_multi_pipeline` (line 59), add `self.env.traps.reset_non_ignored()` before the existing `signal::reset_child_signals()` call:

```rust
Ok(ForkResult::Child) => {
    // Set process group
    let my_pid = nix::unistd::getpid();
    if i == 0 {
        setpgid(my_pid, my_pid).ok();
    } else {
        setpgid(my_pid, pgid).ok();
    }
    self.env.traps.reset_non_ignored();
    signal::reset_child_signals();
```

- [ ] **Step 2: Run existing tests to verify no regression**

Run: `cargo test`
Expected: All existing tests pass (no regressions).

- [ ] **Step 3: Commit**

```bash
git add src/exec/pipeline.rs
git commit -m "fix(phase8/task1): add trap reset in pipeline subshell children

POSIX §2.12 requires command traps to be reset in subshell environments.
Pipeline child processes were missing traps.reset_non_ignored() call."
```

---

### Task 2: Create subshell test file with helpers and Category 1 tests (subshell `(...)`)

**Files:**
- Create: `tests/subshell.rs`

- [ ] **Step 1: Create `tests/subshell.rs` with helper functions and Category 1 tests**

```rust
mod helpers;

use std::process::Command;

fn kish_exec(input: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kish"))
        .args(["-c", input])
        .output()
        .expect("failed to execute kish")
}

// =============================================================================
// Category 1: ( ... ) Subshell isolation
// =============================================================================

#[test]
fn test_subshell_variable_isolation() {
    let out = kish_exec("X=original; (X=changed); echo $X");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "original");
}

#[test]
fn test_subshell_new_variable_isolation() {
    let out = kish_exec("(Y=new; echo $Y); echo \"${Y:-unset}\"");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "new");
    assert_eq!(lines[1], "unset");
}

#[test]
fn test_subshell_function_isolation() {
    let out = kish_exec("f() { echo original; }; (f() { echo changed; }; f); f");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "changed");
    assert_eq!(lines[1], "original");
}

#[test]
fn test_subshell_new_function_isolation() {
    let out = kish_exec("(g() { echo inside; }; g); g 2>/dev/null; echo $?");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "inside");
    assert_eq!(lines[1], "127"); // g not found in parent
}

#[test]
fn test_subshell_alias_isolation() {
    let out = kish_exec("alias ll='echo parent'; (alias ll='echo child'; ll); ll");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "child");
    assert_eq!(lines[1], "parent");
}

#[test]
fn test_subshell_trap_command_reset() {
    // Command traps should be reset in subshell
    let out = kish_exec("trap 'echo trapped' INT; (trap)");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("INT"));
}

#[test]
fn test_subshell_trap_ignore_inherited() {
    // Ignore traps should be preserved in subshell
    let out = kish_exec("trap '' INT; (trap)");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("INT"));
}

#[test]
fn test_subshell_option_isolation() {
    let out = kish_exec("set +x; (set -x); echo \"$-\"");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.trim().contains('x'));
}

#[test]
fn test_subshell_dollar_dollar_is_parent_pid() {
    // $$ in subshell should be parent shell's PID
    let out = kish_exec("echo $$; (echo $$)");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], lines[1]); // Same PID
}

#[test]
fn test_subshell_exit_status_propagation() {
    let out = kish_exec("(exit 42); echo $?");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "42");
}

#[test]
fn test_subshell_readonly_inherited() {
    let out = kish_exec("X=hello; readonly X; (echo $X)");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello");
}

#[test]
fn test_subshell_positional_params_isolation() {
    let out = kish_exec("set -- a b c; (set -- x y; echo $# $1); echo $# $1");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "2 x");
    assert_eq!(lines[1], "3 a");
}

#[test]
fn test_subshell_cwd_inheritance() {
    let out = kish_exec("cd /tmp; (pwd)");
    assert!(out.status.success());
    // /tmp may resolve to /private/tmp on macOS
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.trim().ends_with("/tmp"));
}

#[test]
fn test_subshell_cwd_isolation() {
    let out = kish_exec("cd /tmp; (cd /); pwd");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.trim().ends_with("/tmp"));
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --test subshell`
Expected: All 14 tests pass.

- [ ] **Step 3: Commit**

```bash
git add tests/subshell.rs
git commit -m "test(phase8/task2): add subshell (...) environment isolation tests

14 integration tests for POSIX §2.12 subshell isolation:
variable, function, alias, trap, option, $$, exit status,
readonly, positional params, cwd inheritance and isolation."
```

---

### Task 3: Category 2 tests (pipeline)

**Files:**
- Modify: `tests/subshell.rs`

- [ ] **Step 1: Add pipeline isolation tests to `tests/subshell.rs`**

Append the following to the end of `tests/subshell.rs`:

```rust
// =============================================================================
// Category 2: Pipeline subshell isolation
// =============================================================================

#[test]
fn test_pipeline_variable_isolation() {
    let out = kish_exec("X=original; echo hello | X=changed; echo $X");
    assert!(out.status.success());
    // Note: `echo hello | X=changed` runs X=changed in a subshell (pipeline)
    // But this syntax may not parse as intended. Use a command in the pipeline:
    let out = kish_exec("X=original; echo hello | { X=changed; cat >/dev/null; }; echo $X");
    assert!(out.status.success());
    // Brace group in pipeline still runs in subshell (forked), so X should be original
    // However, POSIX allows last command in pipeline to run in current shell.
    // kish forks all pipeline commands, so parent's X is unaffected.
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "original");
}

#[test]
fn test_pipeline_trap_reset() {
    // Command traps should be reset in pipeline subshell (this is the fix from Task 1)
    let out = kish_exec("trap 'echo trapped' INT; echo hello | trap; cat >/dev/null");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // The `trap` command in the pipeline should NOT show INT trap
    assert!(!stdout.contains("trapped"));
    assert!(!stdout.contains("echo trapped"));
}

#[test]
fn test_pipeline_trap_ignore_preserved() {
    // Ignore traps should be preserved in pipeline subshell
    let out = kish_exec("trap '' INT; echo hello | trap; cat >/dev/null");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("INT"));
}

#[test]
fn test_pipeline_function_isolation() {
    let out = kish_exec("f() { echo original; }; echo x | f() { echo changed; }; f");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "original");
}

#[test]
fn test_pipeline_cwd_isolation() {
    let out = kish_exec("cd /tmp; echo x | cd /; pwd");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.trim().ends_with("/tmp"));
}

#[test]
fn test_pipeline_option_isolation() {
    let out = kish_exec("set +x; echo x | set -x; echo \"$-\"");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.trim().contains('x'));
}

#[test]
fn test_pipeline_exit_status() {
    // Pipeline exit status = last command's exit status
    let out = kish_exec("false | true; echo $?");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0");
}

#[test]
fn test_pipeline_pipefail() {
    let out = kish_exec("set -o pipefail; false | true; echo $?");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "1");
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --test subshell`
Expected: All tests pass (including the new pipeline tests that depend on Task 1's fix).

- [ ] **Step 3: Commit**

```bash
git add tests/subshell.rs
git commit -m "test(phase8/task3): add pipeline subshell isolation tests

8 integration tests for pipeline subshell environments:
variable, trap reset/ignore, function, cwd, option isolation,
exit status, and pipefail."
```

---

### Task 4: Category 3 tests (command substitution)

**Files:**
- Modify: `tests/subshell.rs`

- [ ] **Step 1: Add command substitution isolation tests to `tests/subshell.rs`**

Append the following to the end of `tests/subshell.rs`:

```rust
// =============================================================================
// Category 3: Command substitution $(...) isolation
// =============================================================================

#[test]
fn test_cmdsub_variable_isolation() {
    let out = kish_exec("X=original; Y=$(X=changed; echo $X); echo $X $Y");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "original changed");
}

#[test]
fn test_cmdsub_exit_status() {
    // $? after command substitution reflects the substitution's exit status
    let out = kish_exec("X=$(exit 42); echo $?");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "42");
}

#[test]
fn test_cmdsub_nested_isolation() {
    let out = kish_exec("X=outer; Y=$(X=mid; Z=$(X=inner; echo $X); echo $X $Z); echo $X $Y");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "outer mid inner");
}

#[test]
fn test_cmdsub_trap_isolation() {
    let out = kish_exec("trap 'echo parent' INT; X=$(trap); echo \"${X}\"");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Command trap should be reset in command substitution subshell
    assert!(!stdout.contains("parent"));
}

#[test]
fn test_cmdsub_function_isolation() {
    let out = kish_exec("f() { echo original; }; X=$(f() { echo changed; }; f); f; echo $X");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "original"); // parent's f unchanged
    assert_eq!(lines[1], "changed");  // cmdsub output
}

#[test]
fn test_cmdsub_positional_params_isolation() {
    let out = kish_exec("set -- a b c; X=$(set -- x y; echo $# $1); echo $# $1 $X");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "3 a 2 x");
}

#[test]
fn test_cmdsub_cwd_isolation() {
    let out = kish_exec("cd /tmp; X=$(cd /; pwd); pwd; echo $X");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert!(lines[0].ends_with("/tmp")); // parent cwd unchanged
    assert_eq!(lines[1], "/");           // cmdsub ran in /
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --test subshell`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add tests/subshell.rs
git commit -m "test(phase8/task4): add command substitution isolation tests

7 integration tests for \$(...) subshell environments:
variable, exit status, nested isolation, trap, function,
positional params, and cwd isolation."
```

---

### Task 5: Category 4 tests (edge cases)

**Files:**
- Modify: `tests/subshell.rs`

- [ ] **Step 1: Add edge case tests to `tests/subshell.rs`**

Append the following to the end of `tests/subshell.rs`:

```rust
// =============================================================================
// Category 4: Edge cases
// =============================================================================

#[test]
fn test_nested_subshell() {
    let out = kish_exec("X=1; (X=2; (X=3; echo $X); echo $X); echo $X");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "3");
    assert_eq!(lines[1], "2");
    assert_eq!(lines[2], "1");
}

#[test]
fn test_subshell_exit_no_parent() {
    let out = kish_exec("(exit 1); echo still_running");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "still_running");
}

#[test]
fn test_subshell_errexit() {
    // set -e in subshell should not affect parent
    let out = kish_exec("(set -e; false); echo $?");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "1");
}

#[test]
fn test_subshell_errexit_inherited() {
    // Parent's set -e should be inherited by subshell
    let out = kish_exec("set -e; (false); echo unreachable");
    // The subshell fails with exit 1, and errexit causes parent to exit too
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("unreachable"));
}

#[test]
fn test_umask_inheritance() {
    let out = kish_exec("umask 027; (umask)");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0027");
}

#[test]
fn test_umask_isolation() {
    let out = kish_exec("umask 022; (umask 077); umask");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0022");
}

#[test]
fn test_fd_inheritance() {
    // Subshell should inherit open file descriptors
    let out = kish_exec("exec 3>/tmp/kish-fd-test-$$; (echo hello >&3); cat /tmp/kish-fd-test-$$; rm -f /tmp/kish-fd-test-$$");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello");
}

#[test]
fn test_export_and_non_export_in_subshell() {
    let out = kish_exec("A=exported; export A; B=local; (echo $A $B)");
    assert!(out.status.success());
    // Both exported and non-exported vars are available in subshell (fork copies all)
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "exported local");
}

#[test]
fn test_last_bg_pid_inheritance() {
    // Use kish_exec_timeout pattern for background process safety
    let out = kish_exec("true & PARENT_BG=$!; CHILD_BG=$(echo $!); echo \"$PARENT_BG $CHILD_BG\"");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parts: Vec<&str> = stdout.trim().split_whitespace().collect();
    assert_eq!(parts.len(), 2);
    // $! should be inherited by subshell (same value)
    assert_eq!(parts[0], parts[1]);
}

#[test]
fn test_deeply_nested_isolation() {
    let out = kish_exec("X=0; (X=1; (X=2; (X=3; echo $X); echo $X); echo $X); echo $X");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines, vec!["3", "2", "1", "0"]);
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --test subshell`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add tests/subshell.rs
git commit -m "test(phase8/task5): add subshell edge case tests

10 integration tests for edge cases: nested subshells, exit isolation,
errexit interaction, umask inheritance/isolation, fd inheritance,
export/non-export vars, \$! inheritance, deeply nested isolation."
```

---

### Task 6: Update TODO.md and final verification

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Run the full test suite**

Run: `cargo test`
Expected: All tests pass (existing + 39 new subshell tests).

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings.

- [ ] **Step 3: Update TODO.md**

In `TODO.md`, change the Phase 8 entry from:

```markdown
- [ ] Phase 8: Subshell environment isolation
```

to:

```markdown
- [x] Phase 8: Subshell environment isolation
```

Also change the Phase 5 known limitation from:

```markdown
- [ ] Subshell environment isolation is basic (fork-based) — full isolation deferred to Phase 8
```

to:

```markdown
- [x] Subshell environment isolation is basic (fork-based) — full isolation deferred to Phase 8
```

- [ ] **Step 4: Commit**

```bash
git add TODO.md
git commit -m "chore(phase8/task6): mark Phase 8 complete in TODO.md

Phase 8 subshell environment isolation: 1 pipeline trap fix,
39 integration tests covering POSIX §2.12 compliance."
```
