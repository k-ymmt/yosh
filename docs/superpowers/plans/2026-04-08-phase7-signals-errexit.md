# Phase 7: Signals + Errexit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add full POSIX signal handling, errexit (`set -e`) with strict exception rules, `kill`/`wait` builtins, and process group management.

**Architecture:** Self-pipe trick for async-signal-safe signal delivery (new `src/signal.rs` module). Errexit uses closure-based suppression (`with_errexit_suppressed`) in the Executor to track 5 POSIX exception contexts. Process groups managed via double-`setpgid` pattern. Background job tracking via `BgJob` in `ShellEnv`.

**Tech Stack:** Rust 2024, nix 0.31 (signal, process, fs), libc 0.2

---

### Task 1: Signal Module — Signal Table and Name/Number Conversion

**Files:**
- Create: `src/signal.rs`
- Modify: `src/main.rs:1` (add `mod signal;`)

- [ ] **Step 1: Write the unit tests for signal name/number conversion**

In `src/signal.rs`, create the module with the signal table and tests:

```rust
/// Full signal table for name/number conversion.
/// Platform-dependent signals can be added with cfg attributes.
pub const SIGNAL_TABLE: &[(i32, &str)] = &[
    (1, "HUP"),
    (2, "INT"),
    (3, "QUIT"),
    (6, "ABRT"),
    (9, "KILL"),
    (10, "USR1"),
    (12, "USR2"),
    (13, "PIPE"),
    (14, "ALRM"),
    (15, "TERM"),
];

/// Signals for which the shell registers handlers.
/// Excludes KILL (uncatchable) and PIPE (left default).
pub const HANDLED_SIGNALS: &[(i32, &str)] = &[
    (1, "HUP"),
    (2, "INT"),
    (3, "QUIT"),
    (14, "ALRM"),
    (15, "TERM"),
    (10, "USR1"),
    (12, "USR2"),
];

/// Convert a signal name (e.g. "INT", "SIGINT") to its number.
/// Returns Err for unknown names.
pub fn signal_name_to_number(name: &str) -> Result<i32, String> {
    let upper = name.to_uppercase();
    let stripped = upper.strip_prefix("SIG").unwrap_or(&upper);
    for &(num, n) in SIGNAL_TABLE {
        if n == stripped {
            return Ok(num);
        }
    }
    Err(format!("unknown signal: {}", name))
}

/// Convert a signal number to its canonical name (e.g. 2 -> "INT").
pub fn signal_number_to_name(num: i32) -> Option<&'static str> {
    SIGNAL_TABLE.iter().find(|&&(n, _)| n == num).map(|&(_, name)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_name_to_number() {
        assert_eq!(signal_name_to_number("INT").unwrap(), 2);
        assert_eq!(signal_name_to_number("SIGINT").unwrap(), 2);
        assert_eq!(signal_name_to_number("hup").unwrap(), 1);
        assert_eq!(signal_name_to_number("TERM").unwrap(), 15);
        assert_eq!(signal_name_to_number("KILL").unwrap(), 9);
        assert!(signal_name_to_number("INVALID").is_err());
    }

    #[test]
    fn test_signal_number_to_name() {
        assert_eq!(signal_number_to_name(2), Some("INT"));
        assert_eq!(signal_number_to_name(15), Some("TERM"));
        assert_eq!(signal_number_to_name(9), Some("KILL"));
        assert_eq!(signal_number_to_name(999), None);
    }

    #[test]
    fn test_signal_table_completeness() {
        // Verify all HANDLED_SIGNALS entries exist in SIGNAL_TABLE
        for &(num, name) in HANDLED_SIGNALS {
            assert_eq!(
                signal_name_to_number(name).unwrap(),
                num,
                "HANDLED_SIGNALS entry ({}, {}) not in SIGNAL_TABLE",
                num,
                name
            );
        }
    }
}
```

- [ ] **Step 2: Register the module in main.rs**

Add `mod signal;` to `src/main.rs` after the existing module declarations:

```rust
mod builtin;
mod env;
mod error;
mod exec;
mod expand;
mod lexer;
mod parser;
mod signal;  // NEW
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test signal::tests -v`
Expected: 3 tests pass

- [ ] **Step 4: Commit**

```bash
git add src/signal.rs src/main.rs
git commit -m "feat(phase7/task1): add signal module with signal table and name/number conversion"
```

---

### Task 2: Signal Module — Self-Pipe and Signal Handlers

**Files:**
- Modify: `src/signal.rs`

- [ ] **Step 1: Write the test for self-pipe initialization**

Add to the `tests` module in `src/signal.rs`:

```rust
    #[test]
    fn test_init_signal_handling() {
        // init_signal_handling is idempotent due to OnceLock
        init_signal_handling();
        // self-pipe should be initialized
        let fd = self_pipe_read_fd();
        assert!(fd >= 0);
    }

    #[test]
    fn test_drain_pending_signals_empty() {
        init_signal_handling();
        // No signals pending — should return empty
        let signals = drain_pending_signals();
        assert!(signals.is_empty());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test signal::tests -v`
Expected: FAIL — `init_signal_handling`, `self_pipe_read_fd`, `drain_pending_signals` not defined

- [ ] **Step 3: Implement self-pipe and signal handler registration**

Add to `src/signal.rs` (above the `tests` module):

```rust
use std::os::fd::RawFd;
use std::sync::OnceLock;

/// Self-pipe fd pair: (read_fd, write_fd)
static SELF_PIPE: OnceLock<(RawFd, RawFd)> = OnceLock::new();

/// Initialize the self-pipe and register signal handlers.
/// Safe to call multiple times — OnceLock ensures single init.
pub fn init_signal_handling() {
    SELF_PIPE.get_or_init(|| {
        let mut fds: [libc::c_int; 2] = [0; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            panic!("kish: failed to create self-pipe");
        }
        // Set both ends to non-blocking and close-on-exec
        for &fd in &fds {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
            let fd_flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            unsafe { libc::fcntl(fd, libc::F_SETFD, fd_flags | libc::FD_CLOEXEC) };
        }

        // Register signal handlers for HANDLED_SIGNALS
        for &(sig, _) in HANDLED_SIGNALS {
            register_handler(sig);
        }

        (fds[0], fds[1])
    });
}

fn register_handler(sig: i32) {
    use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet};
    let handler = SigHandler::Handler(signal_handler);
    let action = SigAction::new(handler, SaFlags::SA_RESTART, SigSet::empty());
    unsafe { sigaction(nix::sys::signal::Signal::try_from(sig).unwrap(), &action) }.ok();
}

extern "C" fn signal_handler(sig: libc::c_int) {
    let byte = sig as u8;
    if let Some(&(_, write_fd)) = SELF_PIPE.get() {
        unsafe {
            libc::write(write_fd, &byte as *const u8 as *const libc::c_void, 1);
        }
    }
}

/// Read and return all pending signal numbers from the self-pipe.
pub fn drain_pending_signals() -> Vec<i32> {
    let mut signals = Vec::new();
    if let Some(&(read_fd, _)) = SELF_PIPE.get() {
        let mut buf = [0u8; 64];
        loop {
            let n = unsafe {
                libc::read(read_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
            };
            if n <= 0 {
                break;
            }
            for i in 0..n as usize {
                signals.push(buf[i] as i32);
            }
        }
    }
    signals
}

/// Returns the read fd of the self-pipe (for use with poll).
pub fn self_pipe_read_fd() -> RawFd {
    SELF_PIPE.get().map(|&(r, _)| r).unwrap_or(-1)
}

/// Set a signal to SIG_IGN.
pub fn ignore_signal(sig: i32) {
    use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet};
    let action = SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty());
    unsafe { sigaction(nix::sys::signal::Signal::try_from(sig).unwrap(), &action) }.ok();
}

/// Set a signal to SIG_DFL.
pub fn default_signal(sig: i32) {
    use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet};
    let action = SigAction::new(SigHandler::SigDfl, SaFlags::empty(), SigSet::empty());
    unsafe { sigaction(nix::sys::signal::Signal::try_from(sig).unwrap(), &action) }.ok();
}

/// Called in child processes after fork.
/// Closes self-pipe fds and resets all handled signals to default.
pub fn reset_child_signals() {
    if let Some(&(read_fd, write_fd)) = SELF_PIPE.get() {
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
    }
    for &(sig, _) in HANDLED_SIGNALS {
        default_signal(sig);
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test signal::tests -v`
Expected: All 5 tests pass

- [ ] **Step 5: Commit**

```bash
git add src/signal.rs
git commit -m "feat(phase7/task2): implement self-pipe signal handling with init, drain, and child reset"
```

---

### Task 3: TrapStore Extensions — `reset_non_ignored` and `get_signal_trap`

**Files:**
- Modify: `src/env/mod.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/env/mod.rs`:

```rust
    #[test]
    fn test_trap_store_reset_non_ignored() {
        let mut store = TrapStore::default();
        store.set_trap("INT", TrapAction::Command("echo caught".to_string())).unwrap();
        store.set_trap("HUP", TrapAction::Ignore).unwrap();
        store.set_trap("TERM", TrapAction::Command("echo term".to_string())).unwrap();
        store.reset_non_ignored();
        // Command traps should be removed
        assert!(store.signal_traps.get(&2).is_none()); // INT removed
        assert!(store.signal_traps.get(&15).is_none()); // TERM removed
        // Ignore traps should be preserved
        assert_eq!(store.signal_traps.get(&1), Some(&TrapAction::Ignore));
        // Exit trap with Command should be removed
        store.set_trap("EXIT", TrapAction::Command("echo bye".to_string())).unwrap();
        store.reset_non_ignored();
        assert!(store.exit_trap.is_none());
    }

    #[test]
    fn test_trap_store_get_signal_trap() {
        let mut store = TrapStore::default();
        store.set_trap("INT", TrapAction::Command("echo caught".to_string())).unwrap();
        assert!(matches!(store.get_signal_trap(2), Some(TrapAction::Command(_))));
        assert!(store.get_signal_trap(15).is_none());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test env::tests -v`
Expected: FAIL — `reset_non_ignored` and `get_signal_trap` not defined

- [ ] **Step 3: Implement the methods**

Add to `impl TrapStore` in `src/env/mod.rs`:

```rust
    /// Reset all non-ignored traps to default (POSIX subshell behavior).
    /// Command traps are removed. Ignore traps are preserved.
    pub fn reset_non_ignored(&mut self) {
        // Reset exit trap if it's a Command
        if matches!(self.exit_trap, Some(TrapAction::Command(_))) {
            self.exit_trap = None;
        }
        // Reset signal traps: remove Command entries, keep Ignore
        self.signal_traps.retain(|_, action| matches!(action, TrapAction::Ignore));
    }

    /// Get the trap action for a signal by number (not EXIT).
    pub fn get_signal_trap(&self, sig: i32) -> Option<&TrapAction> {
        self.signal_traps.get(&sig)
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test env::tests -v`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add src/env/mod.rs
git commit -m "feat(phase7/task3): add TrapStore::reset_non_ignored and get_signal_trap"
```

---

### Task 4: BgJob Tracking in ShellEnv

**Files:**
- Modify: `src/env/mod.rs`

- [ ] **Step 1: Write the failing test**

Add to `tests` module in `src/env/mod.rs`:

```rust
    #[test]
    fn test_bg_jobs() {
        let mut env = ShellEnv::new("kish", vec![]);
        assert!(env.bg_jobs.is_empty());
        env.bg_jobs.push(BgJob { pid: Pid::from_raw(1234), status: None });
        assert_eq!(env.bg_jobs.len(), 1);
        assert!(env.bg_jobs[0].status.is_none());
        env.bg_jobs[0].status = Some(0);
        assert_eq!(env.bg_jobs[0].status, Some(0));
    }
```

Add `use nix::unistd::Pid;` to the test module imports if not already present.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test env::tests::test_bg_jobs -v`
Expected: FAIL — `BgJob` not defined, `bg_jobs` field not present

- [ ] **Step 3: Implement BgJob and add to ShellEnv**

Add the struct above `ShellEnv` in `src/env/mod.rs`:

```rust
/// A background job tracked by the shell.
#[derive(Debug, Clone)]
pub struct BgJob {
    pub pid: Pid,
    pub status: Option<i32>,
}
```

Add the field to `ShellEnv`:

```rust
pub struct ShellEnv {
    // ... existing fields ...
    pub bg_jobs: Vec<BgJob>,  // NEW
}
```

Initialize in `ShellEnv::new`:

```rust
bg_jobs: Vec::new(),
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test env::tests -v`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add src/env/mod.rs
git commit -m "feat(phase7/task4): add BgJob struct and bg_jobs tracking to ShellEnv"
```

---

### Task 5: Errexit Core — `with_errexit_suppressed`, `should_errexit`, `check_errexit`

**Files:**
- Modify: `src/exec/mod.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/exec/mod.rs`:

```rust
    #[test]
    fn test_should_errexit_default_off() {
        let exec = Executor::new("kish", vec![]);
        assert!(!exec.should_errexit());
    }

    #[test]
    fn test_should_errexit_enabled() {
        let mut exec = Executor::new("kish", vec![]);
        exec.env.options.errexit = true;
        assert!(exec.should_errexit());
    }

    #[test]
    fn test_with_errexit_suppressed() {
        let mut exec = Executor::new("kish", vec![]);
        exec.env.options.errexit = true;
        assert!(exec.should_errexit());
        let result = exec.with_errexit_suppressed(|e| {
            assert!(!e.should_errexit());
            42
        });
        assert_eq!(result, 42);
        // Suppression restored
        assert!(exec.should_errexit());
    }

    #[test]
    fn test_with_errexit_suppressed_nested() {
        let mut exec = Executor::new("kish", vec![]);
        exec.env.options.errexit = true;
        exec.with_errexit_suppressed(|e| {
            assert!(!e.should_errexit());
            e.with_errexit_suppressed(|e2| {
                assert!(!e2.should_errexit());
            });
            assert!(!e.should_errexit());
        });
        assert!(exec.should_errexit());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test exec::tests -v`
Expected: FAIL — `should_errexit`, `with_errexit_suppressed` not defined

- [ ] **Step 3: Add the errexit field and methods**

In `src/exec/mod.rs`, update the `Executor` struct:

```rust
pub struct Executor {
    pub env: ShellEnv,
    errexit_suppressed_depth: usize,
}
```

Update `Executor::new`:

```rust
pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
    Executor {
        env: ShellEnv::new(shell_name, args),
        errexit_suppressed_depth: 0,
    }
}
```

Add the methods to `impl Executor`:

```rust
    /// Execute closure within errexit-suppressed context.
    pub fn with_errexit_suppressed<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.errexit_suppressed_depth += 1;
        let result = f(self);
        self.errexit_suppressed_depth -= 1;
        result
    }

    /// Check if errexit is active and not suppressed.
    pub fn should_errexit(&self) -> bool {
        self.env.options.errexit && self.errexit_suppressed_depth == 0
    }

    /// Errexit check after command execution.
    /// Non-zero status + should_errexit() -> execute EXIT trap and exit.
    pub fn check_errexit(&mut self, status: i32) {
        if status != 0 && self.should_errexit() {
            self.execute_exit_trap();
            std::process::exit(status);
        }
    }

    /// Execute the EXIT trap if set.
    pub fn execute_exit_trap(&mut self) {
        if let Some(crate::env::TrapAction::Command(cmd)) = self.env.traps.exit_trap.take() {
            self.with_errexit_suppressed(|exec| {
                exec.eval_string(&cmd);
            });
        }
    }
```

Note: This moves `execute_exit_trap` from `main.rs` into `Executor` where it belongs. Update `main.rs` to call `executor.execute_exit_trap()` instead of the free function.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test exec::tests -v`
Expected: All tests pass (including existing ones)

- [ ] **Step 5: Commit**

```bash
git add src/exec/mod.rs src/main.rs
git commit -m "feat(phase7/task5): add errexit core — with_errexit_suppressed, should_errexit, check_errexit"
```

---

### Task 6: Errexit Integration — exec_body, exec_if, exec_loop, exec_and_or

**Files:**
- Modify: `src/exec/mod.rs`
- Create: `tests/errexit.rs`

- [ ] **Step 1: Write the failing integration tests**

Create `tests/errexit.rs`:

```rust
mod helpers;

use std::process::Command;

fn kish_exec(input: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kish"))
        .args(["-c", input])
        .output()
        .expect("failed to execute kish")
}

#[test]
fn test_errexit_basic() {
    let out = kish_exec("set -e; false; echo unreachable");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_errexit_if_condition_suppressed() {
    let out = kish_exec("set -e; if false; then echo no; fi; echo reached");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "reached\n");
}

#[test]
fn test_errexit_elif_condition_suppressed() {
    let out = kish_exec("set -e; if false; then echo no; elif false; then echo no2; fi; echo reached");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "reached\n");
}

#[test]
fn test_errexit_while_condition_suppressed() {
    let out = kish_exec("set -e; while false; do :; done; echo reached");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "reached\n");
}

#[test]
fn test_errexit_until_condition_suppressed() {
    let out = kish_exec("set -e; until true; do :; done; echo reached");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "reached\n");
}

#[test]
fn test_errexit_negated_pipeline_suppressed() {
    let out = kish_exec("set -e; ! false; echo reached");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "reached\n");
}

#[test]
fn test_errexit_and_or_suppressed() {
    let out = kish_exec("set -e; false || true; echo reached");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "reached\n");
}

#[test]
fn test_errexit_and_or_final_exits() {
    let out = kish_exec("set -e; true && false; echo unreachable");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_errexit_nested_suppression() {
    let out = kish_exec("set -e; if ! false; then echo ok; fi; echo reached");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ok\nreached\n");
}

#[test]
fn test_errexit_subshell() {
    let out = kish_exec("set -e; (false); echo unreachable");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_errexit_function() {
    let out = kish_exec("set -e; f() { false; }; f; echo unreachable");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_errexit_trap_action_suppressed() {
    let out = kish_exec("set -e; trap 'false; echo trap' EXIT; exit 0");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "trap\n");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test errexit -v`
Expected: Most tests FAIL (errexit not enforced)

- [ ] **Step 3: Integrate errexit into exec_body**

In `src/exec/mod.rs`, modify `exec_body`:

```rust
    fn exec_body(&mut self, body: &[CompleteCommand]) -> i32 {
        let mut status = 0;
        for cmd in body {
            status = self.exec_complete_command(cmd);
            if self.env.flow_control.is_some() {
                break;
            }
            self.check_errexit(status);
        }
        status
    }
```

- [ ] **Step 4: Integrate errexit suppression into exec_if**

In `src/exec/mod.rs`, modify `exec_if`:

```rust
    fn exec_if(
        &mut self,
        condition: &[CompleteCommand],
        then_part: &[CompleteCommand],
        elif_parts: &[(Vec<CompleteCommand>, Vec<CompleteCommand>)],
        else_part: &Option<Vec<CompleteCommand>>,
    ) -> i32 {
        let cond_status = self.with_errexit_suppressed(|e| e.exec_body(condition));
        if self.env.flow_control.is_some() {
            return cond_status;
        }

        if cond_status == 0 {
            return self.exec_body(then_part);
        }

        for (elif_cond, elif_body) in elif_parts {
            let cond_status = self.with_errexit_suppressed(|e| e.exec_body(elif_cond));
            if self.env.flow_control.is_some() {
                return cond_status;
            }
            if cond_status == 0 {
                return self.exec_body(elif_body);
            }
        }

        if let Some(else_body) = else_part {
            return self.exec_body(else_body);
        }

        0
    }
```

- [ ] **Step 5: Integrate errexit suppression into exec_loop**

In `src/exec/mod.rs`, modify `exec_loop` — change only the condition line:

```rust
    fn exec_loop(
        &mut self,
        condition: &[CompleteCommand],
        body: &[CompleteCommand],
        until: bool,
    ) -> i32 {
        let mut status = 0;
        loop {
            let cond_status = self.with_errexit_suppressed(|e| e.exec_body(condition));
            if self.env.flow_control.is_some() {
                return cond_status;
            }
            let should_run = if until {
                cond_status != 0
            } else {
                cond_status == 0
            };
            if !should_run {
                break;
            }

            status = self.exec_body(body);

            match self.env.flow_control.take() {
                Some(FlowControl::Break(n)) => {
                    if n > 1 {
                        self.env.flow_control = Some(FlowControl::Break(n - 1));
                    }
                    break;
                }
                Some(FlowControl::Continue(n)) => {
                    if n > 1 {
                        self.env.flow_control = Some(FlowControl::Continue(n - 1));
                        break;
                    }
                }
                Some(other) => {
                    self.env.flow_control = Some(other);
                    break;
                }
                None => {}
            }
        }
        status
    }
```

- [ ] **Step 6: Rewrite exec_and_or with errexit suppression**

In `src/exec/mod.rs`, replace `exec_and_or`:

```rust
    pub fn exec_and_or(&mut self, and_or: &AndOrList) -> i32 {
        let has_rest = !and_or.rest.is_empty();

        let mut status = if and_or.first.negated || has_rest {
            self.with_errexit_suppressed(|e| e.exec_pipeline(&and_or.first))
        } else {
            self.exec_pipeline(&and_or.first)
        };

        if self.env.flow_control.is_some() {
            return status;
        }

        for (i, (op, pipeline)) in and_or.rest.iter().enumerate() {
            let is_last = i == and_or.rest.len() - 1;
            let should_run = match op {
                AndOrOp::And => status == 0,
                AndOrOp::Or => status != 0,
            };
            if !should_run {
                continue;
            }

            status = if pipeline.negated || !is_last {
                self.with_errexit_suppressed(|e| e.exec_pipeline(pipeline))
            } else {
                self.exec_pipeline(pipeline)
            };

            if self.env.flow_control.is_some() {
                break;
            }
        }

        self.env.last_exit_status = status;
        status
    }
```

- [ ] **Step 7: Run all tests to verify they pass**

Run: `cargo test -v`
Expected: All tests pass (existing + new errexit tests)

- [ ] **Step 8: Commit**

```bash
git add src/exec/mod.rs tests/errexit.rs
git commit -m "feat(phase7/task6): integrate errexit into exec_body, exec_if, exec_loop, exec_and_or"
```

---

### Task 7: Signal Processing in Executor

**Files:**
- Modify: `src/exec/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add process_pending_signals and handle_default_signal to Executor**

Add to `impl Executor` in `src/exec/mod.rs`. First add the import at the top:

```rust
use crate::signal;
```

Then add the methods:

```rust
    /// Process any pending signals from the self-pipe.
    pub fn process_pending_signals(&mut self) {
        let signals = signal::drain_pending_signals();
        for sig in signals {
            match self.env.traps.get_signal_trap(sig).cloned() {
                Some(crate::env::TrapAction::Command(cmd)) => {
                    self.with_errexit_suppressed(|exec| {
                        exec.eval_string(&cmd);
                    });
                }
                Some(crate::env::TrapAction::Ignore) => {}
                Some(crate::env::TrapAction::Default) | None => {
                    self.handle_default_signal(sig);
                }
            }
        }
    }

    /// Handle a signal with default behavior (terminate).
    fn handle_default_signal(&mut self, sig: i32) {
        self.execute_exit_trap();
        std::process::exit(128 + sig);
    }

    /// Evaluate a string as shell commands (used by trap actions).
    pub fn eval_string(&mut self, input: &str) {
        if let Ok(program) = crate::parser::Parser::new_with_aliases(input, &self.env.aliases).parse_program() {
            self.exec_program(&program);
        }
    }
```

- [ ] **Step 2: Add signal checking to exec_body**

Update `exec_body` to call `process_pending_signals`:

```rust
    fn exec_body(&mut self, body: &[CompleteCommand]) -> i32 {
        let mut status = 0;
        for cmd in body {
            status = self.exec_complete_command(cmd);
            if self.env.flow_control.is_some() {
                break;
            }
            self.check_errexit(status);
            self.process_pending_signals();
        }
        status
    }
```

- [ ] **Step 3: Initialize signal handling and add shutdown processing in main.rs**

In `src/main.rs`, add `signal::init_signal_handling()` at the start of `run_string`:

```rust
fn run_string(input: &str, shell_name: String, positional: Vec<String>) -> i32 {
    signal::init_signal_handling();
    let mut executor = Executor::new(shell_name, positional);
    // ... rest unchanged
```

Replace the `execute_exit_trap` free function call with the Executor method, and add signal processing:

```rust
    executor.process_pending_signals();
    executor.execute_exit_trap();
    status
}
```

Remove the old free function `execute_exit_trap` from `main.rs` — it's now an Executor method. Also update the parse error path:

```rust
            Err(e) => {
                eprintln!("{}", e);
                executor.execute_exit_trap();
                return 2;
            }
```
```

- [ ] **Step 4: Update subshell to reset traps and signals**

In `src/exec/mod.rs`, update `exec_subshell`:

```rust
    fn exec_subshell(&mut self, body: &[CompleteCommand]) -> i32 {
        match unsafe { fork() } {
            Err(e) => {
                eprintln!("kish: fork: {}", e);
                1
            }
            Ok(ForkResult::Child) => {
                self.env.traps.reset_non_ignored();
                signal::reset_child_signals();
                let status = self.exec_body(body);
                std::process::exit(status);
            }
            Ok(ForkResult::Parent { child }) => command::wait_child(child),
        }
    }
```

- [ ] **Step 5: Run all tests**

Run: `cargo test -v`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add src/exec/mod.rs src/main.rs
git commit -m "feat(phase7/task7): add signal processing to executor and main loop"
```

---

### Task 8: Process Group Management in Pipeline and External Commands

**Files:**
- Modify: `src/exec/pipeline.rs`
- Modify: `src/exec/mod.rs` (exec_external_with_redirects)

- [ ] **Step 1: Add setpgid to pipeline child processes**

In `src/exec/pipeline.rs`, add import at top:

```rust
use nix::unistd::setpgid;
use crate::signal;
```

Modify `exec_multi_pipeline` to add process group management. Replace the fork loop and wait section:

```rust
    fn exec_multi_pipeline(&mut self, pipeline: &Pipeline) -> i32 {
        let n = pipeline.commands.len();
        assert!(n >= 2);

        let mut pipes: Vec<(RawFd, RawFd)> = Vec::with_capacity(n - 1);
        for _ in 0..n - 1 {
            match create_pipe() {
                Ok(fds) => pipes.push(fds),
                Err(e) => {
                    eprintln!("kish: pipe: {}", e);
                    close_all_pipes(&pipes);
                    return 1;
                }
            }
        }

        let mut children: Vec<Pid> = Vec::with_capacity(n);
        let mut pgid = Pid::from_raw(0);

        for (i, cmd) in pipeline.commands.iter().enumerate() {
            match unsafe { fork() } {
                Err(e) => {
                    eprintln!("kish: fork: {}", e);
                    close_all_pipes(&pipes);
                    return 1;
                }
                Ok(ForkResult::Child) => {
                    // Set process group
                    let my_pid = nix::unistd::getpid();
                    if i == 0 {
                        setpgid(my_pid, my_pid).ok();
                    } else {
                        setpgid(my_pid, pgid).ok();
                    }
                    signal::reset_child_signals();

                    // Set up stdin from previous pipe's read end
                    if i > 0 {
                        let read_fd = pipes[i - 1].0;
                        if unsafe { libc::dup2(read_fd, 0) } == -1 {
                            eprintln!("kish: dup2: {}", std::io::Error::last_os_error());
                            unsafe { libc::_exit(1) };
                        }
                    }
                    // Set up stdout to next pipe's write end
                    if i < n - 1 {
                        let write_fd = pipes[i].1;
                        if unsafe { libc::dup2(write_fd, 1) } == -1 {
                            eprintln!("kish: dup2: {}", std::io::Error::last_os_error());
                            unsafe { libc::_exit(1) };
                        }
                    }

                    close_all_pipes(&pipes);

                    let status = self.exec_command(cmd);
                    std::process::exit(status);
                }
                Ok(ForkResult::Parent { child }) => {
                    if i == 0 {
                        pgid = child;
                    }
                    // Double setpgid to avoid race
                    setpgid(child, pgid).ok();
                    children.push(child);
                }
            }
        }

        close_all_pipes(&pipes);

        let mut last_status = 0;
        let mut max_nonzero = 0;
        for (idx, child) in children.into_iter().enumerate() {
            let status = wait_for_child(child);
            if status != 0 {
                max_nonzero = status;
            }
            if idx == n - 1 {
                last_status = status;
            }
        }

        if self.env.options.pipefail {
            max_nonzero
        } else {
            last_status
        }
    }
```

- [ ] **Step 2: Add signal reset to external command fork**

In `src/exec/mod.rs`, in `exec_external_with_redirects`, add `signal::reset_child_signals()` at the start of the `ForkResult::Child` branch:

```rust
            Ok(ForkResult::Child) => {
                signal::reset_child_signals();

                // Apply redirects (no need to save, we're in the child)
                let mut redir_state = RedirectState::new();
                // ... rest unchanged
```

- [ ] **Step 3: Run all tests**

Run: `cargo test -v`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add src/exec/pipeline.rs src/exec/mod.rs
git commit -m "feat(phase7/task8): add process group management to pipelines and signal reset to child processes"
```

---

### Task 9: Async Command Execution with Process Groups

**Files:**
- Modify: `src/exec/mod.rs`

- [ ] **Step 1: Write integration test for background job tracking**

Add to `tests/errexit.rs` (we'll rename/restructure later — for now just add):

```rust
#[test]
fn test_background_job_last_pid() {
    let out = kish_exec("true & echo $!");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // $! should be a valid PID (a number > 0)
    let pid: i32 = stdout.trim().parse().expect("$! should be a number");
    assert!(pid > 0);
}
```

- [ ] **Step 2: Implement exec_async and update exec_complete_command**

In `src/exec/mod.rs`, add the `exec_async` method:

```rust
    /// Execute a command asynchronously (background with &).
    fn exec_async(&mut self, and_or: &AndOrList) -> i32 {
        match unsafe { fork() } {
            Err(e) => {
                eprintln!("kish: fork: {}", e);
                1
            }
            Ok(ForkResult::Child) => {
                let pid = nix::unistd::getpid();
                nix::unistd::setpgid(pid, pid).ok();
                // POSIX: async commands in non-interactive shell ignore SIGINT/SIGQUIT
                signal::ignore_signal(libc::SIGINT);
                signal::ignore_signal(libc::SIGQUIT);
                signal::reset_child_signals();
                let status = self.exec_and_or(and_or);
                std::process::exit(status);
            }
            Ok(ForkResult::Parent { child }) => {
                nix::unistd::setpgid(child, child).ok();
                self.env.bg_jobs.push(crate::env::BgJob {
                    pid: child,
                    status: None,
                });
                self.env.last_bg_pid = Some(child.as_raw());
                0
            }
        }
    }
```

Update `exec_complete_command` to use `exec_async` instead of the inline fork:

```rust
    pub fn exec_complete_command(&mut self, cmd: &CompleteCommand) -> i32 {
        self.reap_zombies();

        let mut status = 0;

        for (and_or, separator) in &cmd.items {
            if separator == &Some(SeparatorOp::Amp) {
                status = self.exec_async(and_or);
            } else {
                status = self.exec_and_or(and_or);
            }
            if self.env.flow_control.is_some() {
                break;
            }
        }

        self.env.last_exit_status = status;
        status
    }
```

Update `reap_zombies` to also update bg_jobs status:

```rust
    fn reap_zombies(&mut self) {
        loop {
            match nix::sys::wait::waitpid(
                nix::unistd::Pid::from_raw(-1),
                Some(nix::sys::wait::WaitPidFlag::WNOHANG),
            ) {
                Ok(nix::sys::wait::WaitStatus::Exited(pid, code)) => {
                    if let Some(job) = self.env.bg_jobs.iter_mut().find(|j| j.pid == pid) {
                        job.status = Some(code);
                    }
                }
                Ok(nix::sys::wait::WaitStatus::Signaled(pid, sig, _)) => {
                    if let Some(job) = self.env.bg_jobs.iter_mut().find(|j| j.pid == pid) {
                        job.status = Some(128 + sig as i32);
                    }
                }
                Ok(nix::sys::wait::WaitStatus::StillAlive) => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    }
```

- [ ] **Step 3: Run all tests**

Run: `cargo test -v`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add src/exec/mod.rs tests/errexit.rs
git commit -m "feat(phase7/task9): implement async command execution with process groups and bg_jobs tracking"
```

---

### Task 10: `kill` Builtin

**Files:**
- Modify: `src/builtin/mod.rs`

- [ ] **Step 1: Write unit tests for kill argument parsing**

Add to the `tests` module in `src/builtin/mod.rs`:

```rust
    #[test]
    fn test_classify_kill_wait() {
        assert!(matches!(classify_builtin("kill"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("wait"), BuiltinKind::Regular));
    }
```

- [ ] **Step 2: Write integration tests for kill**

Create `tests/signals.rs`:

```rust
mod helpers;

use std::process::Command;

fn kish_exec(input: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kish"))
        .args(["-c", input])
        .output()
        .expect("failed to execute kish")
}

#[test]
fn test_kill_default_sigterm() {
    let out = kish_exec("sleep 100 & kill $!; wait $!; echo $?");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "143"); // 128 + 15 (SIGTERM)
}

#[test]
fn test_kill_dash_s() {
    let out = kish_exec("sleep 100 & kill -s INT $!; wait $!; echo $?");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "130"); // 128 + 2 (SIGINT)
}

#[test]
fn test_kill_dash_9() {
    let out = kish_exec("sleep 100 & kill -9 $!; wait $!; echo $?");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "137"); // 128 + 9 (SIGKILL)
}

#[test]
fn test_kill_dash_signal_name() {
    let out = kish_exec("sleep 100 & kill -INT $!; wait $!; echo $?");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "130");
}

#[test]
fn test_kill_list() {
    let out = kish_exec("kill -l");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("HUP"));
    assert!(stdout.contains("INT"));
    assert!(stdout.contains("TERM"));
}

#[test]
fn test_kill_list_status() {
    let out = kish_exec("kill -l 130");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "INT");
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --test signals -v`
Expected: FAIL — `kill` and `wait` not recognized as builtins

- [ ] **Step 4: Implement kill builtin**

In `src/builtin/mod.rs`, add `"kill" | "wait"` to `classify_builtin`:

```rust
pub fn classify_builtin(name: &str) -> BuiltinKind {
    match name {
        "break" | ":" | "continue" | "." | "eval" | "exec" | "exit" | "export"
        | "readonly" | "return" | "set" | "shift" | "times" | "trap" | "unset" => {
            BuiltinKind::Special
        }
        "cd" | "echo" | "true" | "false" | "alias" | "unalias" | "kill" | "wait" => {
            BuiltinKind::Regular
        }
        _ => BuiltinKind::NotBuiltin,
    }
}
```

Add `"kill"` and `"wait"` to `exec_regular_builtin`:

```rust
pub fn exec_regular_builtin(name: &str, args: &[String], env: &mut ShellEnv) -> i32 {
    match name {
        "cd" => builtin_cd(args, env),
        "true" => 0,
        "false" => 1,
        "echo" => builtin_echo(args),
        "alias" => builtin_alias(args, env),
        "unalias" => builtin_unalias(args, env),
        "kill" => builtin_kill(args),
        "wait" => 0, // placeholder — real implementation in Task 11
        _ => {
            eprintln!("kish: {}: not a regular builtin", name);
            1
        }
    }
}
```

Add the kill implementation:

```rust
fn builtin_kill(args: &[String]) -> i32 {
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
        if let Err(e) = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::try_from(sig_num).ok(),
        ) {
            eprintln!("kish: kill: ({}) - {}", pid_str, e);
            status = 1;
        }
    }
    status
}

fn parse_kill_signal(args: &[String]) -> Result<(i32, &[String]), String> {
    if args[0] == "-s" {
        if args.len() < 3 {
            return Err("option requires an argument -- s".to_string());
        }
        let sig = crate::signal::signal_name_to_number(&args[1])?;
        Ok((sig, &args[2..]))
    } else if args[0] == "--" {
        Ok((libc::SIGTERM, &args[1..]))
    } else if args[0].starts_with('-') && args[0].len() > 1 {
        let spec = &args[0][1..];
        if let Ok(num) = spec.parse::<i32>() {
            Ok((num, &args[1..]))
        } else {
            let sig = crate::signal::signal_name_to_number(spec)?;
            Ok((sig, &args[1..]))
        }
    } else {
        Ok((libc::SIGTERM, args))
    }
}

fn kill_list(args: &[String]) -> i32 {
    if args.is_empty() {
        let names: Vec<&str> = crate::signal::SIGNAL_TABLE
            .iter()
            .map(|&(_, name)| name)
            .collect();
        println!("{}", names.join(" "));
        return 0;
    }
    for arg in args {
        if let Ok(num) = arg.parse::<i32>() {
            // Number -> name (subtract 128 if > 128)
            let sig = if num > 128 { num - 128 } else { num };
            match crate::signal::signal_number_to_name(sig) {
                Some(name) => println!("{}", name),
                None => {
                    eprintln!("kish: kill: {}: invalid signal number", arg);
                    return 1;
                }
            }
        } else {
            // Name -> number
            match crate::signal::signal_name_to_number(arg) {
                Ok(num) => println!("{}", num),
                Err(e) => {
                    eprintln!("kish: kill: {}", e);
                    return 1;
                }
            }
        }
    }
    0
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -v`
Expected: All tests pass (kill tests may need wait — see Task 11)

- [ ] **Step 6: Commit**

```bash
git add src/builtin/mod.rs tests/signals.rs
git commit -m "feat(phase7/task10): implement kill builtin with all syntax forms and kill -l"
```

---

### Task 11: `wait` Builtin

**Files:**
- Modify: `src/builtin/mod.rs`
- Modify: `src/exec/mod.rs` (wait needs Executor access)

- [ ] **Step 1: Write integration tests for wait**

Add to `tests/signals.rs`:

```rust
#[test]
fn test_wait_basic() {
    let out = kish_exec("sleep 0.1 & wait; echo done");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "done");
}

#[test]
fn test_wait_pid() {
    let out = kish_exec("sleep 0.1 & pid=$!; wait $pid; echo $?");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0");
}

#[test]
fn test_wait_nonexistent_pid() {
    let out = kish_exec("wait 99999");
    assert_eq!(out.status.code(), Some(127));
}
```

- [ ] **Step 2: Implement wait builtin**

The `wait` builtin needs access to the `Executor` (for `bg_jobs` and `process_pending_signals`). Since regular builtins currently only get `&mut ShellEnv`, we need to handle `wait` as a special case in `exec_simple_command`.

In `src/exec/mod.rs`, in `exec_simple_command`, add a check for `"wait"` before the `classify_builtin` match — insert right before the `match classify_builtin(...)` block:

```rust
        // wait needs Executor access (bg_jobs + signal processing)
        if command_name == "wait" {
            let saved = self.apply_temp_assignments(&cmd.assignments);
            let mut redirect_state = RedirectState::new();
            if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                eprintln!("kish: {}", e);
                self.restore_assignments(saved);
                self.env.last_exit_status = 1;
                return 1;
            }
            let status = self.builtin_wait(&args);
            redirect_state.restore();
            self.restore_assignments(saved);
            self.env.last_exit_status = status;
            return status;
        }
```

Add the `builtin_wait` method to `impl Executor`:

```rust
    /// POSIX wait builtin: wait for background jobs.
    fn builtin_wait(&mut self, args: &[String]) -> i32 {
        use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};

        let target_pids: Vec<Pid> = if args.is_empty() {
            self.env
                .bg_jobs
                .iter()
                .filter(|j| j.status.is_none())
                .map(|j| j.pid)
                .collect()
        } else {
            let mut pids = Vec::new();
            for arg in args {
                match arg.parse::<i32>() {
                    Ok(n) => pids.push(Pid::from_raw(n)),
                    Err(_) => {
                        eprintln!("kish: wait: {}: not a pid", arg);
                        return 2;
                    }
                }
            }
            pids
        };

        if target_pids.is_empty() {
            return self.env.last_exit_status;
        }

        let mut last_status = 0;

        for pid in &target_pids {
            // Check if already reaped in bg_jobs
            if let Some(job) = self.env.bg_jobs.iter().find(|j| j.pid == *pid) {
                if let Some(s) = job.status {
                    last_status = s;
                    continue;
                }
            }

            loop {
                match waitpid(*pid, Some(WaitPidFlag::WNOHANG)) {
                    Ok(WaitStatus::Exited(_, code)) => {
                        if let Some(job) =
                            self.env.bg_jobs.iter_mut().find(|j| j.pid == *pid)
                        {
                            job.status = Some(code);
                        }
                        last_status = code;
                        break;
                    }
                    Ok(WaitStatus::Signaled(_, sig, _)) => {
                        let code = 128 + sig as i32;
                        if let Some(job) =
                            self.env.bg_jobs.iter_mut().find(|j| j.pid == *pid)
                        {
                            job.status = Some(code);
                        }
                        last_status = code;
                        break;
                    }
                    Ok(WaitStatus::StillAlive) => {
                        // Poll: wait for either self-pipe signal or SIGCHLD (EINTR)
                        let pipe_fd = signal::self_pipe_read_fd();
                        if pipe_fd < 0 {
                            // No self-pipe — fallback to blocking wait
                            match waitpid(*pid, None) {
                                Ok(WaitStatus::Exited(_, code)) => {
                                    last_status = code;
                                    break;
                                }
                                Ok(WaitStatus::Signaled(_, sig, _)) => {
                                    last_status = 128 + sig as i32;
                                    break;
                                }
                                _ => break,
                            }
                        }
                        let mut fds = [nix::poll::PollFd::new(
                            unsafe { std::os::fd::BorrowedFd::borrow_raw(pipe_fd) },
                            nix::poll::PollFlags::POLLIN,
                        )];
                        match nix::poll::poll(&mut fds, nix::poll::PollTimeout::NONE) {
                            Ok(_)
                                if fds[0]
                                    .revents()
                                    .is_some_and(|r| r.contains(nix::poll::PollFlags::POLLIN)) =>
                            {
                                // Signal arrived via self-pipe
                                let signals = signal::drain_pending_signals();
                                if !signals.is_empty() {
                                    self.process_pending_signals();
                                    last_status = 128 + signals[0];
                                    return last_status;
                                }
                            }
                            Err(nix::errno::Errno::EINTR) => {
                                // SIGCHLD — loop back to try waitpid again
                            }
                            _ => {
                                // Fallback — try waitpid again
                            }
                        }
                    }
                    Err(nix::errno::Errno::ECHILD) => {
                        // No such child process
                        eprintln!("kish: wait: pid {} is not a child of this shell", pid);
                        last_status = 127;
                        break;
                    }
                    Err(_) | Ok(_) => break,
                }
            }
        }

        last_status
    }
```

Remove the placeholder `"wait" => 0` from `exec_regular_builtin` in `src/builtin/mod.rs` (it will no longer be reached since we intercept in `exec_simple_command`):

```rust
        "wait" => {
            // Handled in Executor::exec_simple_command for access to bg_jobs
            eprintln!("kish: wait: internal error — should be handled by executor");
            1
        }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -v`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add src/exec/mod.rs src/builtin/mod.rs tests/signals.rs
git commit -m "feat(phase7/task11): implement wait builtin with self-pipe signal interruption"
```

---

### Task 12: Signal Trap Execution Integration Tests

**Files:**
- Modify: `tests/signals.rs`

- [ ] **Step 1: Write signal trap integration tests**

Add to `tests/signals.rs`:

```rust
#[test]
fn test_trap_int_execution() {
    let out = kish_exec("trap 'echo caught' INT; kill -INT $$; echo after");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("caught"));
    assert!(stdout.contains("after"));
}

#[test]
fn test_trap_reset() {
    // After resetting, trap should not fire
    let out = kish_exec("trap 'echo x' INT; trap - INT; trap");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Should NOT contain a trap for INT
    assert!(!stdout.contains("INT"));
}

#[test]
fn test_subshell_trap_reset() {
    let out = kish_exec("trap 'echo x' INT; (trap)");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Subshell should have no traps (Command traps reset)
    assert!(!stdout.contains("INT"));
}

#[test]
fn test_subshell_ignore_preserved() {
    let out = kish_exec("trap '' INT; (trap -p INT)");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Ignore trap should be preserved in subshell
    assert!(stdout.contains("INT"));
}

#[test]
fn test_kill_zero_self_group() {
    // kill 0 sends to own process group — shell should catch via trap
    let out = kish_exec("trap 'echo caught' TERM; kill -TERM 0; echo after");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("caught"));
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --test signals -v`
Expected: All tests pass

- [ ] **Step 3: Commit**

```bash
git add tests/signals.rs
git commit -m "feat(phase7/task12): add signal trap execution integration tests"
```

---

### Task 13: Verify Exit Trap + Errexit Integration

**Files:**
- No new changes needed — `execute_exit_trap` already uses `with_errexit_suppressed` from Task 5.

- [ ] **Step 1: Verify the errexit trap test passes**

Run: `cargo test --test errexit test_errexit_trap_action_suppressed -v`
Expected: PASS — trap action runs even with `false` inside it, because `execute_exit_trap` wraps in `with_errexit_suppressed`.

If the test fails, check that `Executor::execute_exit_trap` (added in Task 5) uses `with_errexit_suppressed` correctly.

- [ ] **Step 2: Run all tests**

Run: `cargo test -v`
Expected: All tests pass

---

### Task 14: Update builtin_exit to Process Pending Signals

**Files:**
- Modify: `src/builtin/special.rs`

- [ ] **Step 1: Update builtin_exit to accept &mut Executor**

Change `builtin_exit` signature and update the dispatch in `exec_special_builtin`:

In `src/builtin/special.rs`, update the dispatch:

```rust
pub fn exec_special_builtin(name: &str, args: &[String], executor: &mut Executor) -> i32 {
    match name {
        ":" => 0,
        "exit" => builtin_exit(args, executor),
        // ... rest unchanged
```

Update `builtin_exit`:

```rust
fn builtin_exit(args: &[String], executor: &mut Executor) -> i32 {
    let code = if args.is_empty() {
        executor.env.last_exit_status
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
    std::process::exit(code);
}
```

- [ ] **Step 2: Run all tests**

Run: `cargo test -v`
Expected: All tests pass

- [ ] **Step 3: Commit**

```bash
git add src/builtin/special.rs
git commit -m "feat(phase7/task14): update builtin_exit to process pending signals before exit"
```

---

### Task 15: Final Integration Tests and Cleanup

**Files:**
- Modify: `tests/errexit.rs`
- Modify: `tests/signals.rs`
- Modify: `TODO.md`

- [ ] **Step 1: Run the full test suite**

Run: `cargo test -v`
Expected: All tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -W clippy::all`
Expected: No warnings (fix any that appear)

- [ ] **Step 3: Update TODO.md**

Remove resolved Phase 7 items and add any new known limitations:

```markdown
## Phase 7: Known Limitations

- [ ] `wait` signal interruption — if multiple signals arrive simultaneously during `wait`, only the first is used for the return status
- [ ] `kill 0` in pipeline subshell sends to pipeline's process group, not the shell's
```

Remove from "Remaining Phases":
```markdown
- [ ] Phase 7: Signals and errexit
```

Update Phase 6 resolved items:
- Remove: `trap` signal execution (INT, HUP, etc.) not implemented
- Remove: `-e` (errexit) flag is settable but behavior is not implemented
- Remove: `builtin_exit` calls `process::exit` directly — needs change for EXIT trap support in Phase 7

- [ ] **Step 4: Commit**

```bash
git add tests/ TODO.md
git commit -m "feat(phase7/task15): final integration tests, clippy cleanup, and TODO.md update"
```
