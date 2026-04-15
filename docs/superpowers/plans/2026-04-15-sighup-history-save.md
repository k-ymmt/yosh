# SIGHUP History Save Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent history loss when the interactive shell exits via signal (SIGHUP, SIGTERM, etc.) or `exit` builtin by routing all exit paths through `Repl::run()`'s cleanup code.

**Architecture:** Add `exit_requested: Option<i32>` to `Executor`. In interactive mode, `handle_default_signal()`, `builtin_exit()`, and `check_errexit()` set this flag instead of calling `std::process::exit()`. `Repl::run()` checks the flag and breaks to the existing cleanup path that saves history.

**Tech Stack:** Rust, POSIX signals, kish shell internals

---

### Task 1: Add `exit_requested` field to `Executor`

**Files:**
- Modify: `src/exec/mod.rs:17-39`

- [ ] **Step 1: Write the failing test**

Add a test in `src/exec/mod.rs` that checks the new field exists and defaults to `None`:

```rust
#[test]
fn exit_requested_defaults_to_none() {
    let exec = Executor::new("kish", vec![]);
    assert_eq!(exec.exit_requested, None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kish exit_requested_defaults_to_none`
Expected: FAIL — `exit_requested` field does not exist

- [ ] **Step 3: Add the field**

In the `Executor` struct definition at `src/exec/mod.rs:17-21`, add the field:

```rust
pub struct Executor {
    pub env: ShellEnv,
    pub plugins: PluginManager,
    errexit_suppressed_depth: usize,
    pub exit_requested: Option<i32>,
}
```

Initialize to `None` in `Executor::new()` at line 24-29:

```rust
pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
    Executor {
        env: ShellEnv::new(shell_name, args),
        plugins: PluginManager::new(),
        errexit_suppressed_depth: 0,
        exit_requested: None,
    }
}
```

Initialize to `None` in `Executor::from_env()` at line 33-38:

```rust
pub fn from_env(env: ShellEnv) -> Self {
    Executor {
        env,
        plugins: PluginManager::new(),
        errexit_suppressed_depth: 0,
        exit_requested: None,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p kish exit_requested_defaults_to_none`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/exec/mod.rs
git commit -m "feat(exec): add exit_requested field to Executor"
```

---

### Task 2: Modify `handle_default_signal()` to use flag in interactive mode

**Files:**
- Modify: `src/exec/mod.rs:116-120`

- [ ] **Step 1: Write the failing test**

Add a test in `src/exec/mod.rs` that verifies `handle_default_signal` sets the flag in interactive mode. Since `handle_default_signal` is private, call `process_pending_signals` after writing a signal to the self-pipe. However, in unit test context the self-pipe may not be initialized. Instead, make `handle_default_signal` `pub(crate)` and test it directly:

```rust
#[test]
fn handle_default_signal_sets_exit_requested_in_interactive_mode() {
    let mut exec = Executor::new("kish", vec![]);
    exec.env.mode.is_interactive = true;
    exec.handle_default_signal(libc::SIGHUP);
    assert_eq!(exec.exit_requested, Some(128 + libc::SIGHUP));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kish handle_default_signal_sets_exit_requested`
Expected: FAIL — currently calls `std::process::exit()` so test process exits

- [ ] **Step 3: Modify `handle_default_signal`**

Change visibility from `fn` to `pub(crate) fn` and add the interactive mode check at `src/exec/mod.rs:116-120`:

```rust
/// Handle a signal with default behavior (terminate).
pub(crate) fn handle_default_signal(&mut self, sig: i32) {
    self.execute_exit_trap();
    if self.env.mode.is_interactive {
        self.exit_requested = Some(128 + sig);
    } else {
        std::process::exit(128 + sig);
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p kish handle_default_signal_sets_exit_requested`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/exec/mod.rs
git commit -m "fix(exec): use exit_requested flag for signals in interactive mode"
```

---

### Task 3: Modify `check_errexit()` to use flag in interactive mode

**Files:**
- Modify: `src/exec/mod.rs:63-69`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn check_errexit_sets_exit_requested_in_interactive_mode() {
    let mut exec = Executor::new("kish", vec![]);
    exec.env.mode.is_interactive = true;
    exec.env.mode.options.errexit = true;
    exec.check_errexit(1);
    assert_eq!(exec.exit_requested, Some(1));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kish check_errexit_sets_exit_requested`
Expected: FAIL — currently calls `std::process::exit()`

- [ ] **Step 3: Modify `check_errexit`**

At `src/exec/mod.rs:63-69`:

```rust
/// Errexit check after command execution.
pub fn check_errexit(&mut self, status: i32) {
    if status != 0 && self.should_errexit() {
        self.execute_exit_trap();
        if self.env.mode.is_interactive {
            self.exit_requested = Some(status);
        } else {
            std::process::exit(status);
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p kish check_errexit_sets_exit_requested`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/exec/mod.rs
git commit -m "fix(exec): use exit_requested flag for errexit in interactive mode"
```

---

### Task 4: Modify `builtin_exit()` to use flag in interactive mode

**Files:**
- Modify: `src/builtin/special.rs:38-53`

- [ ] **Step 1: Write the failing test**

Add a test in `src/builtin/special.rs` (or in a `#[cfg(test)] mod tests` block if one doesn't exist). Since `builtin_exit` is private, test via `exec_special_builtin`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::Executor;

    #[test]
    fn exit_builtin_sets_exit_requested_in_interactive_mode() {
        let mut executor = Executor::new("kish", vec![]);
        executor.env.mode.is_interactive = true;
        let status = exec_special_builtin("exit", &["42".to_string()], &mut executor);
        assert_eq!(status, 42);
        assert_eq!(executor.exit_requested, Some(42));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kish exit_builtin_sets_exit_requested`
Expected: FAIL — currently calls `std::process::exit()`

- [ ] **Step 3: Modify `builtin_exit`**

At `src/builtin/special.rs:38-53`:

```rust
fn builtin_exit(args: &[String], executor: &mut Executor) -> i32 {
    let code = if args.is_empty() {
        executor.env.exec.last_exit_status
    } else {
        match args[0].parse::<i32>() {
            Ok(n) => n & 0xFF,
            Err(_) => {
                eprintln!("kish: exit: {}: numeric argument required", args[0]);
                2
            }
        }
    };
    executor.process_pending_signals();
    executor.execute_exit_trap();
    if executor.env.mode.is_interactive {
        executor.exit_requested = Some(code);
        code
    } else {
        std::process::exit(code);
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p kish exit_builtin_sets_exit_requested`
Expected: PASS

- [ ] **Step 5: Add test for default exit code (no args)**

```rust
#[test]
fn exit_builtin_uses_last_status_when_no_args() {
    let mut executor = Executor::new("kish", vec![]);
    executor.env.mode.is_interactive = true;
    executor.env.exec.last_exit_status = 7;
    exec_special_builtin("exit", &[], &mut executor);
    assert_eq!(executor.exit_requested, Some(7));
}
```

- [ ] **Step 6: Run all tests**

Run: `cargo test -p kish exit_builtin_`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/builtin/special.rs
git commit -m "fix(builtin): use exit_requested flag for exit in interactive mode"
```

---

### Task 5: Add `exit_requested` checks to `Repl::run()`

**Files:**
- Modify: `src/interactive/mod.rs:166-200`

- [ ] **Step 1: Add exit check after command execution loop**

In the `ParseStatus::Complete` branch at `src/interactive/mod.rs:166-170`, add a check after the for loop and break the outer loop:

```rust
ParseStatus::Complete(commands) => {
    // Add to history before executing
    let histsize: usize = self.executor.env.vars.get("HISTSIZE")
        .and_then(|s| s.parse().ok()).unwrap_or(500);
    let histcontrol = self.executor.env.vars.get("HISTCONTROL")
        .unwrap_or("ignoreboth").to_string();
    let cmd_text = input_buffer.trim_end().to_string();
    self.executor.env.history.add(&cmd_text, histsize, &histcontrol);

    for cmd in &commands {
        let status = self.executor.exec_complete_command(cmd);
        self.executor.env.exec.last_exit_status = status;
        if self.executor.exit_requested.is_some() {
            break;
        }
    }
    input_buffer.clear();
}
```

- [ ] **Step 2: Add exit check after `process_pending_signals` in loop body**

After `src/interactive/mod.rs:186`, add a check for `exit_requested` that also covers the `Complete` branch exit:

```rust
// Process any pending signals
self.executor.process_pending_signals();
if let Some(code) = self.executor.exit_requested {
    self.executor.env.exec.last_exit_status = code;
    break;
}
```

- [ ] **Step 3: Skip redundant `execute_exit_trap` after loop**

The post-loop code at `src/interactive/mod.rs:189-190` calls `process_pending_signals()` then `execute_exit_trap()`. When `exit_requested` is set, the exit trap was already executed by `handle_default_signal()`, `check_errexit()`, or `builtin_exit()`. Guard it:

```rust
self.executor.process_pending_signals();
if self.executor.exit_requested.is_none() {
    self.executor.execute_exit_trap();
}
```

- [ ] **Step 4: Run all tests to verify no regressions**

Run: `cargo test -p kish`
Expected: All existing tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/interactive/mod.rs
git commit -m "fix(interactive): check exit_requested to route exits through cleanup path"
```

---

### Task 6: Add exit_requested check in `exec_body` loop

**Files:**
- Modify: `src/exec/compound.rs:60-71`

- [ ] **Step 1: Read current code**

Read `src/exec/compound.rs:60-71` to confirm the current `exec_body` implementation.

- [ ] **Step 2: Add exit_requested check after `check_errexit` and `process_pending_signals`**

In `exec_body`, after `check_errexit` sets `exit_requested` in interactive mode, the loop should stop executing further commands:

```rust
pub(crate) fn exec_body(&mut self, body: &[CompleteCommand]) -> i32 {
    let mut status = 0;
    for cmd in body {
        status = self.exec_complete_command(cmd);
        if self.env.exec.flow_control.is_some() {
            break;
        }
        self.check_errexit(status);
        if self.exit_requested.is_some() {
            break;
        }
        self.process_pending_signals();
        if self.exit_requested.is_some() {
            break;
        }
    }
    status
}
```

- [ ] **Step 3: Run all tests**

Run: `cargo test -p kish`
Expected: All PASS

- [ ] **Step 4: Commit**

```bash
git add src/exec/compound.rs
git commit -m "fix(exec): propagate exit_requested through compound command body"
```

---

### Task 7: Integration test — SIGHUP saves history

**Files:**
- Create: `e2e/history/sighup_saves_history.sh`

- [ ] **Step 1: Write E2E test**

Create `e2e/history/sighup_saves_history.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.4 Shell Execution Environment
# DESCRIPTION: SIGHUP saves command history before exiting
# EXPECT_EXIT: 129

# This test is run by the test harness which starts kish
# with the script. However, SIGHUP history save only applies
# to interactive mode. This test verifies exit code 129 for
# non-interactive SIGHUP. Interactive SIGHUP history save
# is tested in the PTY integration test.
kill -HUP $$
```

Set permissions to 644:

```bash
chmod 644 e2e/history/sighup_saves_history.sh
```

- [ ] **Step 2: Write PTY integration test for interactive SIGHUP history save**

Add to `tests/pty_interactive.rs` (or a new file `tests/sighup_history.rs` if the PTY test file is large). This test starts kish interactively, runs a command, sends SIGHUP, and verifies the history file was written:

```rust
#[test]
fn sighup_saves_history_file() {
    use std::io::Read;

    let dir = tempfile::tempdir().unwrap();
    let histfile = dir.path().join(".kish_history");

    let binary = env!("CARGO_BIN_EXE_kish");
    let mut session = expectrl::spawn(format!(
        r#"{} -c 'HISTFILE={} exec {}'"#,
        "/bin/sh",
        histfile.display(),
        binary,
    ))
    .expect("Failed to spawn kish");

    // Wait for prompt
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Type a command and execute it
    session.send_line("echo sighup_test_marker").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Send SIGHUP to kish
    let pid = session.pid().expect("no pid");
    unsafe {
        libc::kill(pid as i32, libc::SIGHUP);
    }

    // Wait for kish to exit
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Verify history file was written and contains our command
    let mut contents = String::new();
    std::fs::File::open(&histfile)
        .expect("history file should exist")
        .read_to_string(&mut contents)
        .unwrap();
    assert!(
        contents.contains("echo sighup_test_marker"),
        "history file should contain the command, got: {:?}",
        contents
    );
}
```

- [ ] **Step 3: Build and run the PTY test**

Run: `cargo build && cargo test -p kish sighup_saves_history_file -- --nocapture`
Expected: PASS — history file contains the command

- [ ] **Step 4: Commit**

```bash
git add e2e/history/sighup_saves_history.sh tests/pty_interactive.rs
git commit -m "test: add SIGHUP history save integration tests"
```

---

### Task 8: Run full test suite and update TODO.md

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Run full test suite**

Run: `cargo test -p kish`
Expected: All PASS

- [ ] **Step 2: Run E2E tests**

Run: `cargo build && ./e2e/run_tests.sh`
Expected: All PASS (new test may be skipped if harness doesn't support it — that's OK)

- [ ] **Step 3: Remove completed item from TODO.md**

Delete the following line from `TODO.md` under `## History: Known Limitations`:

```
- [ ] SIGHUP history save — verify history is saved before exit on SIGHUP; if `handle_default_signal` calls `std::process::exit()` directly, history may be lost (`src/exec/mod.rs`, `src/interactive/mod.rs`)
```

- [ ] **Step 4: Commit**

```bash
git add TODO.md
git commit -m "docs(TODO): remove completed SIGHUP history save item"
```
