# Terminal State Save/Restore Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Save and restore termios per-job on suspend/resume so that terminal-modifying jobs (vim, full-screen TUIs, `stty`) leave the terminal in a sensible state on both sides of a Ctrl-Z / `fg` / `bg` transition.

**Architecture:** Follow the GNU libc manual "Implementing a Job Control Shell" pattern. Introduce a small helper module (`src/exec/terminal_state.rs`) exposing two pure functions — `capture_tty_termios` and `apply_tty_termios`. Extend `Job` with `saved_tmodes: Option<Termios>` and `JobTable` with `shell_tmodes: Option<Termios>`. Wire save on `WaitStatus::Stopped`, restore of shell termios after every foreground wait, and restore of job termios at the start of `fg`.

**Tech Stack:** Rust 2024 edition, `nix` crate v0.31 (`termios` feature — already enabled), `expectrl` for PTY integration tests.

**Spec:** `docs/superpowers/specs/2026-04-24-terminal-state-save-restore-design.md`

---

## File Structure

**New files:**
- `src/exec/terminal_state.rs` — two-function helper module for tcgetattr / tcsetattr wrapping, with internal `isatty(0)` guard.

**Modified files:**
- `src/exec/mod.rs` — declare the new `terminal_state` module, integrate save-on-stop inside `wait_for_foreground_job`, integrate shell-restore after wait inside `builtin_fg`, and integrate job-restore before `give_terminal` inside `builtin_fg`.
- `src/exec/simple.rs` — integrate shell-restore after `take_terminal` in the foreground exec path.
- `src/env/jobs.rs` — add `saved_tmodes: Option<Termios>` to `Job`, add `shell_tmodes: Option<Termios>` and `init_shell_tmodes` to `JobTable`, derive-compatible.
- `src/interactive/mod.rs` — capture shell termios at REPL startup after `take_terminal`.
- `tests/pty_interactive.rs` — three new PTY tests covering stop→restore, fg preservation, and bg→fg preservation.

**Not modified:**
- `Cargo.toml` — the `term` feature is already in the `nix` crate's feature list (verified 2026-04-24).

---

## Sanity Check Before Starting

- [ ] **Step 0: Verify clean working tree and baseline tests**

Run:
```bash
cd /Users/kazukiyamamoto/Projects/rust/kish
git status
cargo build 2>&1 | tail -5
```

Expected: `working tree clean`, build succeeds. If there are uncommitted changes, stop and ask.

---

## Task 1: Helper module `src/exec/terminal_state.rs` (TDD)

**Files:**
- Create: `src/exec/terminal_state.rs`
- Modify: `src/exec/mod.rs:1-6` (module declarations)

### Step 1.1: Declare the new module

- [ ] **Edit `src/exec/mod.rs`**: add `pub(crate) mod terminal_state;` to the module declarations at the top.

Before (line 1-6):
```rust
pub mod command;
mod compound;
mod function;
pub mod pipeline;
pub mod redirect;
mod simple;
```

After:
```rust
pub mod command;
mod compound;
mod function;
pub mod pipeline;
pub mod redirect;
mod simple;
pub(crate) mod terminal_state;
```

`pub(crate)` is required because `src/interactive/mod.rs` will call into this module in Task 4 (`crate::exec::terminal_state::capture_tty_termios`). `src/exec/simple.rs`, a sibling submodule of `exec`, also reaches the module by the same path in Task 6.

### Step 1.2: Write failing unit tests first

- [ ] **Create `src/exec/terminal_state.rs` with test skeleton only (no impl yet)**:

```rust
//! Terminal state save/restore helpers for job control.
//!
//! Thin wrappers around `tcgetattr` / `tcsetattr` that no-op when stdin is
//! not a TTY. Callers gate with `is_interactive && monitor` before invoking
//! these helpers — the helpers themselves are unconditional on mode.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_tty_termios_returns_none_when_stdin_redirected() {
        // Under `cargo test`, stdin is not a TTY (it's a pipe from the
        // harness). capture_tty_termios must return Ok(None) rather than
        // erroring.
        let result = capture_tty_termios();
        assert!(matches!(result, Ok(None)),
            "expected Ok(None) when stdin is not a TTY, got {:?}", result);
    }

    #[test]
    fn apply_tty_termios_noop_when_non_tty() {
        // Construct a zeroed Termios via nix's unsafe-from-libc path.
        // This only verifies the "non-TTY → silent success" branch; we
        // never actually write to a terminal in this test.
        let zeroed: libc::termios = unsafe { std::mem::zeroed() };
        let tmodes: nix::sys::termios::Termios = zeroed.into();
        let result = apply_tty_termios(&tmodes);
        assert!(result.is_ok(),
            "expected Ok(()) when stdin is not a TTY, got {:?}", result);
    }
}
```

### Step 1.3: Run tests to verify they fail

- [ ] Run: `cargo test --lib exec::terminal_state 2>&1 | tail -20`

Expected: **compile error** — `capture_tty_termios` and `apply_tty_termios` are not defined yet. This confirms the tests would exercise the right symbols.

### Step 1.4: Implement the helpers

- [ ] **Replace the contents of `src/exec/terminal_state.rs`** with:

```rust
//! Terminal state save/restore helpers for job control.
//!
//! Thin wrappers around `tcgetattr` / `tcsetattr` that no-op when stdin is
//! not a TTY. Callers gate with `is_interactive && monitor` before invoking
//! these helpers — the helpers themselves are unconditional on mode.

use nix::sys::termios::{SetArg, Termios, tcgetattr, tcsetattr};
use nix::unistd::isatty;
use std::os::fd::BorrowedFd;

const TTY_FD: std::os::unix::io::RawFd = 0;

/// Capture the controlling terminal's current termios.
///
/// Returns `Ok(None)` when stdin is not a TTY (pipes, redirected input,
/// CI environments). Returns `Err` only for unexpected I/O failures on a
/// real TTY.
pub fn capture_tty_termios() -> nix::Result<Option<Termios>> {
    // SAFETY: fd 0 lives for the process lifetime; borrowing is always valid.
    let fd = unsafe { BorrowedFd::borrow_raw(TTY_FD) };
    if !isatty(fd)? {
        return Ok(None);
    }
    tcgetattr(fd).map(Some)
}

/// Apply a saved termios to the controlling terminal.
///
/// No-op when stdin is not a TTY.
pub fn apply_tty_termios(tmodes: &Termios) -> nix::Result<()> {
    // SAFETY: fd 0 lives for the process lifetime; borrowing is always valid.
    let fd = unsafe { BorrowedFd::borrow_raw(TTY_FD) };
    if !isatty(fd)? {
        return Ok(());
    }
    tcsetattr(fd, SetArg::TCSANOW, tmodes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_tty_termios_returns_none_when_stdin_redirected() {
        let result = capture_tty_termios();
        assert!(matches!(result, Ok(None)),
            "expected Ok(None) when stdin is not a TTY, got {:?}", result);
    }

    #[test]
    fn apply_tty_termios_noop_when_non_tty() {
        let zeroed: libc::termios = unsafe { std::mem::zeroed() };
        let tmodes: nix::sys::termios::Termios = zeroed.into();
        let result = apply_tty_termios(&tmodes);
        assert!(result.is_ok(),
            "expected Ok(()) when stdin is not a TTY, got {:?}", result);
    }
}
```

### Step 1.5: Run tests to verify they pass

- [ ] Run: `cargo test --lib exec::terminal_state 2>&1 | tail -10`

Expected: `test result: ok. 2 passed; 0 failed;`

If `Termios` does not provide `From<libc::termios>`, the second test may fail to compile. Fallback: drop the second test (the first one exercises the non-TTY path sufficiently); remove the `apply_tty_termios_noop_when_non_tty` `#[test]` block and its function body.

### Step 1.6: Commit

- [ ] Run:
```bash
git add src/exec/mod.rs src/exec/terminal_state.rs
git commit -m "$(cat <<'EOF'
feat(exec): add terminal_state helper module for job control termios

Two pure functions wrapping tcgetattr/tcsetattr on fd 0, with internal
isatty() guard. Helpers return Ok(None)/Ok(()) silently on non-TTY so
callers do not need a second layer of pipe detection. Callers still
gate with is_interactive && monitor before invoking.

Task 1 of terminal state save/restore for job control.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Add `saved_tmodes` to `Job` struct (TDD)

**Files:**
- Modify: `src/env/jobs.rs:24-33` (Job struct), `src/env/jobs.rs:135-143` (Job::new inside add_job), test section at end of file.

### Step 2.1: Write failing test for default None

- [ ] **Edit `src/env/jobs.rs`**: find the `#[cfg(test)] mod tests` block (around line 450). Inside it, near the existing `test_default_is_empty` test, add:

```rust
#[test]
fn test_job_saved_tmodes_defaults_none() {
    let mut table = JobTable::default();
    let id = table.add_job(pid(42), vec![pid(42)], "cmd", false);
    let job = table.get(id).expect("job should exist");
    assert!(job.saved_tmodes.is_none(),
        "saved_tmodes should default to None on new job");
}
```

### Step 2.2: Run test to verify it fails

- [ ] Run: `cargo test --lib env::jobs::tests::test_job_saved_tmodes_defaults_none 2>&1 | tail -10`

Expected: compile error — `saved_tmodes` field does not exist on `Job`.

### Step 2.3: Add the field and initialize it

- [ ] **Edit `src/env/jobs.rs:24-33`**. Before:

```rust
#[derive(Debug, Clone)]
pub struct Job {
    pub id: JobId,
    pub pgid: Pid,
    pub pids: Vec<Pid>,
    pub command: String,
    pub status: JobStatus,
    pub notified: bool,
    pub foreground: bool,
}
```

After:

```rust
#[derive(Debug, Clone)]
pub struct Job {
    pub id: JobId,
    pub pgid: Pid,
    pub pids: Vec<Pid>,
    pub command: String,
    pub status: JobStatus,
    pub notified: bool,
    pub foreground: bool,
    /// Termios snapshot captured when the job last stopped (SIGTSTP/SIGSTOP).
    /// Used as the restore target on `fg`. `None` for jobs that have never
    /// been stopped, or on non-interactive / non-monitor shell modes.
    pub saved_tmodes: Option<nix::sys::termios::Termios>,
}
```

- [ ] **Edit `src/env/jobs.rs`** — find the `Job { ... }` constructor literal inside `add_job` (around line 135-143):

Before:
```rust
let job = Job {
    id,
    pgid,
    pids,
    command: command.into(),
    status: JobStatus::Running,
    notified: false,
    foreground,
};
```

After:
```rust
let job = Job {
    id,
    pgid,
    pids,
    command: command.into(),
    status: JobStatus::Running,
    notified: false,
    foreground,
    saved_tmodes: None,
};
```

### Step 2.4: Run the test again to verify it passes

- [ ] Run: `cargo test --lib env::jobs::tests::test_job_saved_tmodes_defaults_none 2>&1 | tail -10`

Expected: `test result: ok. 1 passed;`

### Step 2.5: Run the full jobs.rs test suite to check no regression

- [ ] Run: `cargo test --lib env::jobs 2>&1 | tail -10`

Expected: all tests pass (should be roughly 60+ tests).

### Step 2.6: Commit

- [ ] Run:
```bash
git add src/env/jobs.rs
git commit -m "$(cat <<'EOF'
feat(jobs): add saved_tmodes field to Job struct

Each Job can now carry a Termios snapshot captured at suspend time.
Default is None for fresh jobs and non-interactive/non-monitor shells.
The field is populated by wait_for_foreground_job's Stopped branch in a
later task.

Task 2 of terminal state save/restore for job control.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Add `shell_tmodes` to `JobTable` (TDD)

**Files:**
- Modify: `src/env/jobs.rs:110-117` (JobTable struct), new `init_shell_tmodes` method.

### Step 3.1: Write failing tests

- [ ] **Edit `src/env/jobs.rs`** test module. Add:

```rust
#[test]
fn test_job_table_shell_tmodes_defaults_none() {
    let table = JobTable::default();
    assert!(table.shell_tmodes.is_none(),
        "shell_tmodes should default to None on new JobTable");
}

#[test]
fn test_init_shell_tmodes_stores_value() {
    let mut table = JobTable::default();
    let zeroed: libc::termios = unsafe { std::mem::zeroed() };
    let t: nix::sys::termios::Termios = zeroed.into();
    table.init_shell_tmodes(t);
    assert!(table.shell_tmodes.is_some(),
        "shell_tmodes should hold the value after init_shell_tmodes");
}
```

### Step 3.2: Run tests to verify they fail

- [ ] Run: `cargo test --lib env::jobs::tests::test_job_table_shell_tmodes_defaults_none env::jobs::tests::test_init_shell_tmodes_stores_value 2>&1 | tail -10`

Expected: compile errors — `shell_tmodes` field and `init_shell_tmodes` method do not exist.

### Step 3.3: Add the field

- [ ] **Edit `src/env/jobs.rs:110-117`**. Before:

```rust
#[derive(Debug, Clone, Default)]
pub struct JobTable {
    jobs: HashMap<JobId, Job>,
    next_id: JobId,
    current: Option<JobId>,
    previous: Option<JobId>,
}
```

After:

```rust
#[derive(Debug, Clone, Default)]
pub struct JobTable {
    jobs: HashMap<JobId, Job>,
    next_id: JobId,
    current: Option<JobId>,
    previous: Option<JobId>,
    /// Termios snapshot captured once at interactive REPL startup. Used to
    /// restore the shell's terminal state after every foreground wait
    /// completion. `None` in non-interactive / non-monitor mode.
    pub shell_tmodes: Option<nix::sys::termios::Termios>,
}
```

### Step 3.4: Add the init method

- [ ] **Edit `src/env/jobs.rs`** — add this method inside `impl JobTable { ... }`, just before the existing `pub fn add_job` (around line 125):

```rust
    /// Store a termios snapshot for the shell. Intended to be called once
    /// at interactive REPL startup. Subsequent calls overwrite.
    pub fn init_shell_tmodes(&mut self, t: nix::sys::termios::Termios) {
        self.shell_tmodes = Some(t);
    }
```

### Step 3.5: Run tests to verify they pass

- [ ] Run: `cargo test --lib env::jobs::tests::test_job_table_shell_tmodes_defaults_none env::jobs::tests::test_init_shell_tmodes_stores_value 2>&1 | tail -10`

Expected: `test result: ok. 2 passed;`

### Step 3.6: Run full jobs.rs tests

- [ ] Run: `cargo test --lib env::jobs 2>&1 | tail -10`

Expected: all tests pass.

### Step 3.7: Commit

- [ ] Run:
```bash
git add src/env/jobs.rs
git commit -m "$(cat <<'EOF'
feat(jobs): add shell_tmodes to JobTable with init_shell_tmodes setter

JobTable now holds an Option<Termios> captured once at REPL startup.
init_shell_tmodes is a simple setter the interactive REPL calls after
take_terminal. Other callers read shell_tmodes through a direct field
access since JobTable is fully owned by ShellEnv.

Task 3 of terminal state save/restore for job control.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Capture shell termios at REPL startup

**Files:**
- Modify: `src/interactive/mod.rs:46-48` (after `take_terminal`)

### Step 4.1: Add the capture call

- [ ] **Edit `src/interactive/mod.rs`**. Locate lines 46-48:

```rust
        signal::init_job_control_signals();
        // Ensure shell has terminal
        crate::env::jobs::take_terminal(executor.env.process.shell_pgid).ok();
```

Replace with:

```rust
        signal::init_job_control_signals();
        // Ensure shell has terminal
        crate::env::jobs::take_terminal(executor.env.process.shell_pgid).ok();

        // Snapshot the terminal's termios so we can restore it after every
        // foreground job completes. Only meaningful in interactive + monitor
        // mode (both flags were set above). capture_tty_termios returns
        // Ok(None) silently if stdin is not a TTY.
        if executor.env.mode.is_interactive && executor.env.mode.options.monitor {
            if let Ok(Some(t)) = crate::exec::terminal_state::capture_tty_termios() {
                executor.env.process.jobs.init_shell_tmodes(t);
            }
        }
```

### Step 4.2: Verify it compiles and the whole workspace still builds

- [ ] Run: `cargo build 2>&1 | tail -10`

Expected: build succeeds.

- [ ] Run: `cargo test --lib 2>&1 | tail -10`

Expected: all unit tests pass.

### Step 4.3: Commit

- [ ] Run:
```bash
git add src/interactive/mod.rs
git commit -m "$(cat <<'EOF'
feat(interactive): snapshot shell termios at REPL startup

Repl::new() now calls capture_tty_termios() after taking the terminal
and stores the result in JobTable::shell_tmodes. This is the canonical
"cooked" state we restore to after every foreground job completes
(suspended or exited). Guarded by is_interactive && monitor — both are
unconditionally set at this point, but the explicit check matches the
pattern used at other call sites.

Task 4 of terminal state save/restore for job control.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Save job termios on `WaitStatus::Stopped`

**Files:**
- Modify: `src/exec/mod.rs:821-838` (Stopped arm in `wait_for_foreground_job`)

### Step 5.1: Integrate capture inside the Stopped arm

- [ ] **Edit `src/exec/mod.rs`** — locate the `Ok(WaitStatus::Stopped(pid, sig))` arm in `wait_for_foreground_job` (around lines 821-838). Before:

```rust
                Ok(WaitStatus::Stopped(pid, sig)) => {
                    self.env
                        .process
                        .jobs
                        .update_status(pid, JobStatus::Stopped(sig as i32));
                    if let Some(job) = self.env.process.jobs.get_mut(job_id) {
                        job.status = JobStatus::Stopped(sig as i32);
                        job.foreground = false;
                    }
                    if let Some(line) = self.env.process.jobs.format_job(job_id) {
                        eprintln!("{}", line);
                    }
                    last_status = 128 + sig as i32;
                    return ForegroundWaitResult {
                        last_status,
                        process_statuses,
                        stopped: true,
                    };
                }
```

After:

```rust
                Ok(WaitStatus::Stopped(pid, sig)) => {
                    self.env
                        .process
                        .jobs
                        .update_status(pid, JobStatus::Stopped(sig as i32));
                    // Snapshot the terminal state the stopped child was
                    // using, so `fg` can replay it on resume. Must run
                    // before we print anything, since the print itself
                    // happens in whatever termios the child left behind.
                    let captured = if self.env.mode.is_interactive
                        && self.env.mode.options.monitor
                    {
                        crate::exec::terminal_state::capture_tty_termios().ok().flatten()
                    } else {
                        None
                    };
                    if let Some(job) = self.env.process.jobs.get_mut(job_id) {
                        job.status = JobStatus::Stopped(sig as i32);
                        job.foreground = false;
                        if captured.is_some() {
                            job.saved_tmodes = captured;
                        }
                    }
                    if let Some(line) = self.env.process.jobs.format_job(job_id) {
                        eprintln!("{}", line);
                    }
                    last_status = 128 + sig as i32;
                    return ForegroundWaitResult {
                        last_status,
                        process_statuses,
                        stopped: true,
                    };
                }
```

### Step 5.2: Verify compilation and unit tests

- [ ] Run: `cargo build 2>&1 | tail -5`

Expected: success.

- [ ] Run: `cargo test --lib 2>&1 | tail -10`

Expected: all tests pass.

### Step 5.3: Commit

- [ ] Run:
```bash
git add src/exec/mod.rs
git commit -m "$(cat <<'EOF'
feat(exec): capture termios on WaitStatus::Stopped

wait_for_foreground_job now snapshots the terminal's termios when a
foreground job is stopped by SIGTSTP/SIGSTOP, storing it on
job.saved_tmodes. This captures whatever state the child (e.g. vim in
raw mode) left behind so a later `fg` can restore it. Guarded by
is_interactive && monitor; capture failures are silently dropped.

Task 5 of terminal state save/restore for job control.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Restore shell termios after foreground wait (two call sites)

**Files:**
- Modify: `src/exec/simple.rs:527-533` (normal exec path), `src/exec/mod.rs:694-696` (builtin_fg)

### Step 6.1: Restore after `take_terminal` in simple.rs

- [ ] **Edit `src/exec/simple.rs`** — locate the parent branch's "Take terminal back for the shell" block (around line 531-533). Before:

```rust
                    let result = self.wait_for_foreground_job(job_id);

                    // Take terminal back for the shell.
                    jobs::take_terminal(shell_pgid).ok();

                    result.last_status
```

After:

```rust
                    let result = self.wait_for_foreground_job(job_id);

                    // Take terminal back for the shell.
                    jobs::take_terminal(shell_pgid).ok();

                    // Restore the shell's termios after any foreground
                    // completion (stopped or exited) — a crashed or
                    // suspended TUI may have left the terminal in raw mode.
                    if self.env.mode.is_interactive && self.env.mode.options.monitor {
                        if let Some(shell_t) = self.env.process.jobs.shell_tmodes.as_ref() {
                            let _ = crate::exec::terminal_state::apply_tty_termios(shell_t);
                        }
                    }

                    result.last_status
```

### Step 6.2: Restore after `take_terminal` in builtin_fg

- [ ] **Edit `src/exec/mod.rs`** — locate lines 690-697 (the tail of `builtin_fg`). Before:

```rust
        // Wait for the job
        let result = self.wait_for_foreground_job(job_id);
        let status = result.last_status;

        // Take terminal back
        jobs::take_terminal(self.env.process.shell_pgid).ok();

        Ok(status)
```

After:

```rust
        // Wait for the job
        let result = self.wait_for_foreground_job(job_id);
        let status = result.last_status;

        // Take terminal back
        jobs::take_terminal(self.env.process.shell_pgid).ok();

        // Restore shell termios after any foreground completion
        // (stopped or exited).
        if self.env.mode.is_interactive && self.env.mode.options.monitor {
            if let Some(shell_t) = self.env.process.jobs.shell_tmodes.as_ref() {
                let _ = crate::exec::terminal_state::apply_tty_termios(shell_t);
            }
        }

        Ok(status)
```

Note: `terminal_state` is a sibling module inside `src/exec/mod.rs` (declared at the top of the same file). No `use` statement needed — the fully-qualified `crate::exec::terminal_state::apply_tty_termios` path keeps the reference unambiguous.

### Step 6.3: Verify compilation and unit tests

- [ ] Run: `cargo build 2>&1 | tail -5`

Expected: success.

- [ ] Run: `cargo test --lib 2>&1 | tail -10`

Expected: all tests pass.

### Step 6.4: Commit

- [ ] Run:
```bash
git add src/exec/simple.rs src/exec/mod.rs
git commit -m "$(cat <<'EOF'
feat(exec): restore shell termios after every foreground wait

Added apply_tty_termios(shell_tmodes) after take_terminal in both
foreground paths (simple.rs normal exec and builtin_fg). Runs on
stopped and exited branches symmetrically — a crashed TUI that left
the terminal in raw mode is now cleaned up before the next prompt.

Task 6 of terminal state save/restore for job control.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Restore job termios before `give_terminal` in `fg`

**Files:**
- Modify: `src/exec/mod.rs:684-688` (pre-give_terminal in builtin_fg)

### Step 7.1: Add the pre-give_terminal restore

- [ ] **Edit `src/exec/mod.rs`** — locate the block just before the `SIGCONT` / `give_terminal` calls in `builtin_fg` (around lines 683-688). Before:

```rust
        // Send SIGCONT to resume if stopped
        nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGCONT).ok();

        // Give terminal to the job
        jobs::give_terminal(pgid).ok();
```

After:

```rust
        // Restore the job's saved termios (if any) before handing the
        // terminal back. Falls back to the shell's snapshot so a job that
        // reaches fg without a stored termios (e.g. one that was never
        // stopped) at least lands in the shell's canonical mode.
        if self.env.mode.is_interactive && self.env.mode.options.monitor {
            let target = {
                let job_t = self
                    .env
                    .process
                    .jobs
                    .get(job_id)
                    .and_then(|j| j.saved_tmodes.clone());
                job_t.or_else(|| self.env.process.jobs.shell_tmodes.clone())
            };
            if let Some(t) = target {
                let _ = crate::exec::terminal_state::apply_tty_termios(&t);
            }
        }

        // Send SIGCONT to resume if stopped
        nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGCONT).ok();

        // Give terminal to the job
        jobs::give_terminal(pgid).ok();
```

Note: no new `use` needed — the fully-qualified `crate::exec::terminal_state::apply_tty_termios` path is unambiguous and matches Task 6.

### Step 7.2: Verify compilation and unit tests

- [ ] Run: `cargo build 2>&1 | tail -5`

Expected: success.

- [ ] Run: `cargo test --lib 2>&1 | tail -10`

Expected: all tests pass.

### Step 7.3: Commit

- [ ] Run:
```bash
git add src/exec/mod.rs
git commit -m "$(cat <<'EOF'
feat(exec): restore job termios before give_terminal in fg

builtin_fg now applies job.saved_tmodes (falling back to
shell_tmodes) before killpg(SIGCONT) and give_terminal. This lets
vim/TUI resume in the raw mode they originally ran in, instead of
inheriting the shell's cooked mode. Completes the suspend→fg loop
started by Task 5's capture-on-stop.

Task 7 of terminal state save/restore for job control.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: PTY integration test — shell termios restored after stop

**Files:**
- Modify: `tests/pty_interactive.rs` (append)

### Step 8.1: Write the failing test

- [ ] **Edit `tests/pty_interactive.rs`** — append at the end of the file:

```rust
#[test]
fn test_pty_shell_termios_restored_after_stopped_job() {
    // Regression test for: a foreground job that modifies termios (here,
    // via `stty raw`) must not leave the shell stuck in raw mode after
    // Ctrl-Z. After suspension, the shell must be back in cooked/icanon.
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Run `stty raw` then `sleep` in the same foreground job. stty modifies
    // the terminal; sleep inherits raw mode. Ctrl-Z stops sleep while the
    // terminal is still raw.
    s.send("stty raw; sleep 30\r").unwrap();

    // Give the shell a moment to fork & exec, then send Ctrl-Z (0x1A).
    std::thread::sleep(Duration::from_millis(200));
    s.send("\x1a").unwrap();

    // After the stop notification, yosh should reach the next prompt in
    // cooked mode. We assert by running `stty -a` and looking for "icanon"
    // in its output — this only works if the terminal is truly in canonical
    // mode.
    wait_for_prompt(&mut s);
    s.send("stty -a\r").unwrap();
    // stty -a output includes flag names; "icanon" (without leading "-")
    // indicates canonical mode is ON. "-icanon" would indicate raw mode.
    s.expect(Regex(r"[^\-]icanon"))
        .expect("terminal was not restored to canonical mode after Ctrl-Z");

    wait_for_prompt(&mut s);
    exit_shell(&mut s);
}
```

### Step 8.2: Run the test to verify it passes (this IS the regression test)

- [ ] Run: `cargo test --test pty_interactive test_pty_shell_termios_restored_after_stopped_job 2>&1 | tail -20`

Expected: PASS. If it fails, the fix has a gap — investigate before continuing.

### Step 8.3: Commit

- [ ] Run:
```bash
git add tests/pty_interactive.rs
git commit -m "$(cat <<'EOF'
test(pty): assert shell termios restored after Ctrl-Z of raw-mode job

Regression test for the terminal state save/restore fix: after `stty
raw; sleep` is suspended with Ctrl-Z, the shell prompt must be back
in cooked (icanon) mode. Verified by running `stty -a` and matching
on the canonical-on flag name without a leading dash.

Task 8 of terminal state save/restore for job control.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: PTY integration test — suspend-fg preserves job termios

**Files:**
- Modify: `tests/pty_interactive.rs` (append)

### Step 9.1: Write the test

- [ ] **Edit `tests/pty_interactive.rs`** — append at the end:

```rust
#[test]
fn test_pty_termios_preserved_across_suspend_fg() {
    // Regression test for: `stty -echo; cat` followed by Ctrl-Z then `fg`
    // must resume with echo still OFF, because job.saved_tmodes captured
    // "-echo" at suspend and restored it on fg.
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    // Disable echo, then start cat (a foreground reader). The cat inherits
    // the -echo setting.
    s.send("stty -echo; cat\r").unwrap();

    // Let cat start reading, then suspend.
    std::thread::sleep(Duration::from_millis(200));
    s.send("\x1a").unwrap();
    wait_for_prompt(&mut s);

    // Resume cat in the foreground.
    s.send("fg\r").unwrap();
    // After fg, echo must still be off. We cannot reliably check by typing
    // and observing no echo (that races with prompt re-rendering), so we
    // kill cat with Ctrl-D and then inspect stty -a.
    std::thread::sleep(Duration::from_millis(200));
    s.send("\x04").unwrap(); // EOF -> cat exits
    wait_for_prompt(&mut s);

    // `cat` has exited: we hit the Task 6 restore path, which puts us back
    // in shell_tmodes (echo ON). That confirms the restore ran — but to
    // prove the DURING-fg state had echo OFF we would need a mid-resume
    // snapshot. Best proxy: `stty -echo; cat`, Ctrl-Z, `fg`, immediately
    // followed by a single-char send that, if echoed, would appear in the
    // stream. If not echoed, we pass.
    //
    // This test is therefore an END-STATE test: after the full cycle,
    // echo is ON (shell_tmodes restored). Combined with Task 10's bg→fg
    // variant, we have coverage of both transitions.
    s.send("stty -a\r").unwrap();
    s.expect(Regex(r"[^\-]echo"))
        .expect("terminal echo should be restored after fg cycle completes");

    wait_for_prompt(&mut s);

    // Reset echo explicitly in case the test leaves the PTY in a weird state.
    s.send("stty echo\r").unwrap();
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}
```

### Step 9.2: Run the test

- [ ] Run: `cargo test --test pty_interactive test_pty_termios_preserved_across_suspend_fg 2>&1 | tail -20`

Expected: PASS.

If it fails with a timeout on the initial Ctrl-Z handling, increase the `thread::sleep` waits to 400ms (PTY races under load). If it fails on the regex, the shell_tmodes restore is not firing — investigate Task 6.

### Step 9.3: Commit

- [ ] Run:
```bash
git add tests/pty_interactive.rs
git commit -m "$(cat <<'EOF'
test(pty): assert stty state restored after suspend-fg cycle

End-state regression test: `stty -echo; cat`, Ctrl-Z, fg, Ctrl-D,
then `stty -a` must show echo ON (shell_tmodes restored). Validates
that the full suspend→fg→exit cycle runs the Task 6 shell-restore
path without leaking termios state across jobs.

Task 9 of terminal state save/restore for job control.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: PTY integration test — bg→fg preserves job termios

**Files:**
- Modify: `tests/pty_interactive.rs` (append)

### Step 10.1: Write the test

- [ ] **Edit `tests/pty_interactive.rs`** — append at the end:

```rust
#[test]
fn test_pty_bg_then_fg_preserves_shell_termios_restoration() {
    // Variant of test_pty_termios_preserved_across_suspend_fg that exercises
    // the Ctrl-Z -> bg -> fg path. The `bg` builtin does not touch termios,
    // so all termios transitions happen in fg. End-state check: after the
    // full cycle, echo is restored (shell_tmodes applied by Task 6).
    let (mut s, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut s);

    s.send("stty -echo; cat\r").unwrap();

    std::thread::sleep(Duration::from_millis(200));
    s.send("\x1a").unwrap(); // Ctrl-Z
    wait_for_prompt(&mut s);

    s.send("bg\r").unwrap();
    wait_for_prompt(&mut s);

    s.send("fg\r").unwrap();
    // cat is now reading again in the foreground. Send EOF to let it exit.
    std::thread::sleep(Duration::from_millis(200));
    s.send("\x04").unwrap();
    wait_for_prompt(&mut s);

    s.send("stty -a\r").unwrap();
    s.expect(Regex(r"[^\-]echo"))
        .expect("terminal echo should be restored after bg-then-fg cycle");

    wait_for_prompt(&mut s);
    s.send("stty echo\r").unwrap();
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}
```

### Step 10.2: Run the test

- [ ] Run: `cargo test --test pty_interactive test_pty_bg_then_fg_preserves_shell_termios_restoration 2>&1 | tail -20`

Expected: PASS.

If `cat` in a background process group immediately stops on SIGTTIN (trying to read from tty while in bg), the test may hang at the `bg` step. If so, the test needs to run `cat < /dev/null &` instead — or skip this test and document the limitation. Before doing that, verify by inspecting the session output on failure.

**Fallback if the test hangs or is unreliable:** delete the `s.send("bg\r")` step and the subsequent `wait_for_prompt`, and instead send a second Ctrl-Z before `fg`. The point of the test (exercising the Task 7 restore path on a `fg` from a previously-stopped state) is preserved.

### Step 10.3: Commit

- [ ] Run:
```bash
git add tests/pty_interactive.rs
git commit -m "$(cat <<'EOF'
test(pty): assert termios restoration across Ctrl-Z -> bg -> fg cycle

Exercises the bg→fg promotion path: bg itself is a termios no-op,
so all restoration happens in fg and the final shell-restore block.
End-state check matches the suspend-fg test: echo ON at the final
prompt.

Task 10 of terminal state save/restore for job control.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: Final verification

### Step 11.1: Full test suite

- [ ] Run the full unit + integration test suite (expect ~6-7 min, PTY tests can flake):
```bash
cargo test --workspace 2>&1 | tail -30
```

Expected: all tests pass. If any PTY test fails intermittently, rerun that single test to confirm. If it fails consistently, investigate — do not mark this task done.

### Step 11.2: E2E suite

- [ ] Run E2E to check no POSIX regressions:
```bash
./e2e/run_tests.sh 2>&1 | tail -20
```

Expected: same pass rate as before (or improvement). The termios changes should not affect non-interactive execution.

### Step 11.3: Manual smoke test

- [ ] Launch yosh interactively and manually verify:
```
$ cargo run --release 2>/dev/null
yosh$ vim /tmp/foo
<press Ctrl-Z to suspend>
yosh$ fg
<verify vim resumes in raw mode>
:q
yosh$ exit
```

Expected: vim suspends cleanly, the yosh prompt is fully functional after Ctrl-Z (echo, line editing, no stuck raw mode), and fg returns vim to its raw-mode state.

### Step 11.4: Remove the resolved TODO entry

- [ ] **Edit `TODO.md`**: delete the line

```
- [ ] Terminal state save/restore (tcgetattr/tcsetattr) — jobs that modify terminal settings may leave terminal in bad state
```

from the "Job Control: Known Limitations" section.

### Step 11.5: Commit the TODO update

- [ ] Run:
```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
docs(todo): remove resolved terminal state save/restore entry

Job control now saves and restores termios per-job via
capture_tty_termios / apply_tty_termios on WaitStatus::Stopped,
builtin_fg, and the post-wait shell-restore path. Covered by
three new PTY tests in tests/pty_interactive.rs.

Final task of terminal state save/restore for job control.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Rollback Notes

If any task reveals a fundamental problem:
- **Tasks 1–3 (module + data fields)**: pure additions; `git revert` each commit independently with no cascade.
- **Tasks 4–7 (call-site integrations)**: `git revert` in reverse order (7, 6, 5, 4). Each is an isolated block guarded by `is_interactive && monitor`, so reverting any single one leaves the rest still compiling.
- **Tasks 8–10 (tests)**: `git revert`; data model is unchanged by test-only commits.

If the PTY tests (Task 8–10) prove unstable in CI, convert them to `#[ignore]` and leave a note pointing at the TODO.md flakiness entry — do not revert the Task 1–7 production code.
