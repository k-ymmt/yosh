# Phase 7 Known Limitations — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix two Phase 7 known limitations — `wait` signal return status and `kill 0` process group targeting.

**Architecture:** Two independent, minimal fixes. Fix 1 changes one line in `builtin_wait()`. Fix 2 adds a `shell_pgid` parameter to `builtin_kill()` and substitutes PID 0 with the shell's process group.

**Tech Stack:** Rust, nix crate (Pid, signal::kill), POSIX process groups

---

### Task 1: Fix `wait` signal return status

**Files:**
- Modify: `src/exec/mod.rs:897`

- [ ] **Step 1: Change `signals[0]` to `*signals.last().unwrap()`**

In `src/exec/mod.rs`, inside `builtin_wait()`, change line 897:

```rust
// Before:
last_status = 128 + signals[0];

// After:
last_status = 128 + *signals.last().unwrap();
```

This uses the last-received signal for the return status, matching bash's last-writer-wins behavior.

- [ ] **Step 2: Run existing tests to verify no regressions**

Run: `cargo test --test signals -- test_wait`
Expected: All `test_wait_basic`, `test_wait_pid`, `test_wait_nonexistent_pid` pass.

- [ ] **Step 3: Commit**

```bash
git add src/exec/mod.rs
git commit -m "fix(wait): use last signal for return status on multi-signal interruption"
```

---

### Task 2: Fix `kill 0` to target shell's process group

**Files:**
- Modify: `src/builtin/mod.rs:3,31,39,165,183-199`
- Test: `tests/signals.rs`

- [ ] **Step 1: Write the failing E2E test**

Add to `tests/signals.rs`:

```rust
#[test]
fn test_kill_0_targets_shell_pgid() {
    // In a pipeline subshell, `kill 0` should target the shell's process group,
    // not the pipeline's process group. We verify by using a trap + kill 0 in
    // a subshell pipeline command — if kill 0 incorrectly targets only the
    // pipeline group, the trap on the shell won't fire.
    let (stdout, _stderr, code) = kish_exec_timeout(
        "trap 'echo trapped' TERM; (kill -TERM 0); echo after",
        5,
    );
    assert_eq!(code, Some(0));
    let stdout_str = stdout.trim();
    // The trap should fire because kill 0 targets the shell's process group
    assert!(stdout_str.contains("trapped"), "expected trap to fire, got: {}", stdout_str);
    assert!(stdout_str.contains("after"), "expected execution to continue, got: {}", stdout_str);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test signals test_kill_0_targets_shell_pgid -- --nocapture`
Expected: FAIL — `kill 0` currently sends to the subshell's process group, not the shell's.

- [ ] **Step 3: Add `Pid` import and update `builtin_kill` signature**

In `src/builtin/mod.rs`, add the `Pid` import:

```rust
use crate::env::ShellEnv;
use nix::unistd::Pid;
```

Change the `builtin_kill` function signature and add PID 0 handling:

```rust
fn builtin_kill(args: &[String], shell_pgid: Pid) -> i32 {
    if args.is_empty() {
        eprintln!("kish: kill: usage: kill [-s sigspec | -signum] pid...");
        return 2;
    }

    if args[0] == "-l" {
        return kill_list(&args[1..]);
    }

    let (sig_num, pid_args) = match parse_kill_signal(args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("kish: kill: {}", msg);
            return 2;
        }
    };

    let mut status = 0;
    for pid_str in pid_args {
        let pid: i32 = match pid_str.parse() {
            Ok(n) => n,
            Err(_) => {
                eprintln!("kish: kill: {}: invalid pid", pid_str);
                status = 1;
                continue;
            }
        };
        // PID 0 means "caller's process group" to the kernel, but in a pipeline
        // subshell the caller's group is the pipeline's group. Substitute the
        // shell's original process group so kill 0 behaves as POSIX expects.
        let target = if pid == 0 {
            Pid::from_raw(-shell_pgid.as_raw())
        } else {
            Pid::from_raw(pid)
        };
        if let Err(e) = nix::sys::signal::kill(
            target,
            nix::sys::signal::Signal::try_from(sig_num).ok(),
        ) {
            eprintln!("kish: kill: ({}) - {}", pid_str, e);
            status = 1;
        }
    }
    status
}
```

- [ ] **Step 4: Update `exec_regular_builtin` to pass `shell_pgid`**

In `src/builtin/mod.rs`, update the `kill` arm in `exec_regular_builtin`:

```rust
        "kill" => builtin_kill(args, env.shell_pgid),
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --test signals test_kill_0_targets_shell_pgid -- --nocapture`
Expected: PASS

- [ ] **Step 6: Run full signal test suite**

Run: `cargo test --test signals`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/builtin/mod.rs tests/signals.rs
git commit -m "fix(kill): kill 0 targets shell process group, not pipeline subshell group"
```

---

### Task 3: Update TODO.md and final verification

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Remove Phase 7 section from TODO.md**

Delete the entire `## Phase 7: Known Limitations` section (lines 3-6) from `TODO.md`, since both items are now resolved.

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "docs: resolve Phase 7 known limitations in TODO.md"
```
