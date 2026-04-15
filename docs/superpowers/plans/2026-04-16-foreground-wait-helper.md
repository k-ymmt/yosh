# Foreground Wait Helper Extraction + EINTR Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unify three duplicated foreground job wait loops into a single `wait_for_foreground_job` helper, fixing EINTR handling in pipeline.rs and Stopped handling in mod.rs.

**Architecture:** Add a `ForegroundWaitResult` struct to `src/exec/mod.rs`, rewrite `wait_for_foreground_job` to use process-count tracking (replacing the flawed `all_job_processes_done` check), then migrate callers in `simple.rs` and `pipeline.rs` to use the unified helper. The `builtin_fg` caller already uses `wait_for_foreground_job` and only needs return-type adaptation.

**Tech Stack:** Rust, nix crate (waitpid, Pid, WaitStatus, WaitPidFlag)

---

### Task 1: Add `ForegroundWaitResult` struct and rewrite `wait_for_foreground_job`

**Files:**
- Modify: `src/exec/mod.rs:570-633` (replace `wait_for_foreground_job` and remove `all_job_processes_done`)

- [ ] **Step 1: Add `ForegroundWaitResult` struct**

Add the struct just before the `impl Executor` block at the top of `src/exec/mod.rs` (after the existing `use` statements, before `pub struct Executor`):

```rust
/// Result of waiting for a foreground job.
pub(crate) struct ForegroundWaitResult {
    /// Exit status of the last process to report.
    pub last_status: i32,
    /// Per-process exit statuses (pid, exit_code) in reporting order — used by pipefail.
    pub process_statuses: Vec<(nix::unistd::Pid, i32)>,
    /// Whether the job was stopped (e.g., Ctrl+Z) rather than exiting.
    pub stopped: bool,
}
```

- [ ] **Step 2: Rewrite `wait_for_foreground_job`**

Replace the existing `wait_for_foreground_job` method (lines 569-624) with the unified implementation. This uses a process-count approach instead of the flawed `all_job_processes_done` check, and fixes both the EINTR bug (from pipeline.rs) and the Stopped `job.foreground = false` bug (from mod.rs):

```rust
    /// Wait for a foreground job to complete or stop.
    ///
    /// Returns a `ForegroundWaitResult` containing the last exit status,
    /// per-process statuses (for pipefail), and whether the job was stopped.
    fn wait_for_foreground_job(&mut self, job_id: crate::env::jobs::JobId) -> ForegroundWaitResult {
        use crate::env::jobs::JobStatus;
        use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};

        let (pgid, total_processes) = match self.env.process.jobs.get(job_id) {
            Some(j) => (j.pgid, j.pids.len()),
            None => return ForegroundWaitResult {
                last_status: 1,
                process_statuses: Vec::new(),
                stopped: false,
            },
        };

        let mut last_status = 0;
        let mut process_statuses: Vec<(nix::unistd::Pid, i32)> = Vec::new();

        loop {
            if process_statuses.len() >= total_processes {
                self.env.process.jobs.mark_notified(job_id);
                self.env.process.jobs.remove_job(job_id);
                break;
            }

            match waitpid(nix::unistd::Pid::from_raw(-pgid.as_raw()), Some(WaitPidFlag::WUNTRACED)) {
                Ok(WaitStatus::Exited(pid, code)) => {
                    self.env.process.jobs.update_status(pid, JobStatus::Done(code));
                    last_status = code;
                    process_statuses.push((pid, code));
                }
                Ok(WaitStatus::Signaled(pid, sig, _)) => {
                    let code = 128 + sig as i32;
                    self.env.process.jobs.update_status(pid, JobStatus::Terminated(sig as i32));
                    last_status = code;
                    process_statuses.push((pid, code));
                }
                Ok(WaitStatus::Stopped(pid, sig)) => {
                    self.env.process.jobs.update_status(pid, JobStatus::Stopped(sig as i32));
                    if let Some(job) = self.env.process.jobs.get_mut(job_id) {
                        job.status = JobStatus::Stopped(sig as i32);
                        job.foreground = false;
                    }
                    if let Some(line) = self.env.process.jobs.format_job(job_id) {
                        eprintln!("{}", line);
                    }
                    last_status = 128 + sig as i32;
                    return ForegroundWaitResult { last_status, process_statuses, stopped: true };
                }
                Err(nix::errno::Errno::ECHILD) => {
                    self.env.process.jobs.remove_job(job_id);
                    break;
                }
                Err(nix::errno::Errno::EINTR) => {
                    self.process_pending_signals();
                    continue;
                }
                _ => break,
            }
        }

        ForegroundWaitResult { last_status, process_statuses, stopped: false }
    }
```

- [ ] **Step 3: Remove `all_job_processes_done`**

Delete the `all_job_processes_done` method (lines 626-633 in the original file). It is only used by the old `wait_for_foreground_job` and is replaced by the process-count approach.

```rust
// DELETE this entire method:
    /// Check if all processes in a job have finished (Done or Terminated).
    fn all_job_processes_done(&self, job_id: crate::env::jobs::JobId) -> bool {
        use crate::env::jobs::JobStatus;
        match self.env.process.jobs.get(job_id) {
            Some(job) => matches!(job.status, JobStatus::Done(_) | JobStatus::Terminated(_)),
            None => true,
        }
    }
```

- [ ] **Step 4: Update `builtin_fg` to use the new return type**

In `builtin_fg` (line 507 in the original), change from:

```rust
        let status = self.wait_for_foreground_job(job_id);
```

To:

```rust
        let result = self.wait_for_foreground_job(job_id);
        let status = result.last_status;
```

The rest of `builtin_fg` (lines 509-512) remains unchanged — it already uses `status` after this point.

- [ ] **Step 5: Verify compilation**

Run: `cargo build 2>&1 | head -30`
Expected: Build succeeds. There will be warnings about unused imports in `simple.rs` and `pipeline.rs` (the old wait loops still reference types that will be cleaned up in Tasks 2 and 3). The key is no errors.

- [ ] **Step 6: Run tests to verify no regressions**

Run: `cargo test 2>&1 | tail -5`
Expected: All existing tests pass. The `builtin_fg` path now uses the updated helper with correct Stopped handling.

- [ ] **Step 7: Commit**

```bash
git add src/exec/mod.rs
git commit -m "refactor(exec): rewrite wait_for_foreground_job with ForegroundWaitResult

Replace all_job_processes_done (flawed: checked overall job status instead
of per-process completion) with process-count tracking. Add Stopped
job.foreground=false fix. EINTR handling preserved from existing impl.

Task: foreground wait helper extraction + EINTR fix"
```

---

### Task 2: Migrate `simple.rs` to use unified helper

**Files:**
- Modify: `src/exec/simple.rs:378-452` (monitor-mode branch of `exec_external_with_redirects`)

- [ ] **Step 1: Replace the inline wait loop in `exec_external_with_redirects`**

In `src/exec/simple.rs`, the `Ok(ForkResult::Parent { child })` branch (starting at line 378) has a monitor-mode `if monitor { ... }` block. Replace the entire monitor block (lines 379-447) with:

```rust
                if monitor {
                    // Ensure child is in its own process group (race-free: both
                    // parent and child call setpgid).
                    nix::unistd::setpgid(child, child).ok();

                    let full_cmd = std::iter::once(cmd)
                        .chain(args.iter().map(|s| s.as_str()))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let job_id = self.env.process.jobs.add_job(
                        child,
                        vec![child],
                        full_cmd,
                        true,
                    );

                    // Hand terminal to the child's process group.
                    jobs::give_terminal(child).ok();

                    let result = self.wait_for_foreground_job(job_id);

                    // Take terminal back for the shell.
                    jobs::take_terminal(shell_pgid).ok();

                    result.last_status
                } else {
                    wait_child(child)
                }
```

- [ ] **Step 2: Remove unused imports from `simple.rs`**

After the migration, these imports at the top of `simple.rs` are no longer needed:

```rust
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
```

Remove this line. The remaining imports (`CString`, `fork`/`ForkResult`, builtins, jobs, expand, signal, command::wait_child, RedirectState, Executor`) are still used.

Also remove this import which is no longer needed:

```rust
use crate::env::jobs::{self, JobStatus};
```

Replace it with just:

```rust
use crate::env::jobs;
```

Since `JobStatus` is no longer referenced directly in `simple.rs`.

- [ ] **Step 3: Verify compilation**

Run: `cargo build 2>&1 | head -30`
Expected: Build succeeds with no errors and no warnings about unused imports in simple.rs.

- [ ] **Step 4: Run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/exec/simple.rs
git commit -m "refactor(exec): migrate simple.rs to use wait_for_foreground_job

Replace 45-line inline wait loop in exec_external_with_redirects
monitor-mode branch with unified helper call.

Task: foreground wait helper extraction + EINTR fix"
```

---

### Task 3: Migrate `pipeline.rs` to use unified helper

**Files:**
- Modify: `src/exec/pipeline.rs:110-198` (monitor-mode branch + `wait_for_foreground_pipeline` method)

- [ ] **Step 1: Replace monitor-mode branch in `exec_multi_pipeline`**

In `src/exec/pipeline.rs`, replace the monitor-mode block (lines 110-117) in `exec_multi_pipeline`:

From:
```rust
        if self.env.mode.options.monitor {
            // Monitor mode: register as foreground job and use WUNTRACED wait
            let cmd_str = "(pipeline)".to_string();
            let job_id = self.env.process.jobs.add_job(pgid, children.clone(), cmd_str, true);
            crate::env::jobs::give_terminal(pgid).ok();
            let status = self.wait_for_foreground_pipeline(job_id, &children, n);
            crate::env::jobs::take_terminal(self.env.process.shell_pgid).ok();
            status
        } else {
```

To:
```rust
        if self.env.mode.options.monitor {
            let cmd_str = "(pipeline)".to_string();
            let job_id = self.env.process.jobs.add_job(pgid, children.clone(), cmd_str, true);
            crate::env::jobs::give_terminal(pgid).ok();

            let result = self.wait_for_foreground_job(job_id);

            crate::env::jobs::take_terminal(self.env.process.shell_pgid).ok();

            if result.stopped {
                result.last_status
            } else if self.env.mode.options.pipefail {
                let mut ordered = vec![0i32; n];
                for (pid, code) in &result.process_statuses {
                    if let Some(idx) = children.iter().position(|c| c == pid) {
                        ordered[idx] = *code;
                    }
                }
                ordered.iter().rev().find(|&&s| s != 0).copied().unwrap_or(0)
            } else {
                result.last_status
            }
        } else {
```

- [ ] **Step 2: Delete `wait_for_foreground_pipeline` method**

Remove the entire `wait_for_foreground_pipeline` method (lines 140-198 in the original file):

```rust
// DELETE this entire method:
    fn wait_for_foreground_pipeline(&mut self, job_id: crate::env::jobs::JobId, children: &[Pid], n: usize) -> i32 {
        // ... all lines through closing brace ...
    }
```

- [ ] **Step 3: Remove unused imports from `pipeline.rs`**

After the migration, these imports are no longer used in pipeline.rs:

```rust
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
```

Remove this line. The remaining imports (`RawFd`, `fork`/`setpgid`/`ForkResult`/`Pid`, `Pipeline`, `signal`, `Executor`) are still used.

- [ ] **Step 4: Verify compilation**

Run: `cargo build 2>&1 | head -30`
Expected: Build succeeds with no errors or warnings.

- [ ] **Step 5: Run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/exec/pipeline.rs
git commit -m "refactor(exec): migrate pipeline.rs to use wait_for_foreground_job

Delete wait_for_foreground_pipeline method. Fixes EINTR bug: pipeline
waitpid now calls process_pending_signals() via unified helper.
Pipefail logic moved inline to exec_multi_pipeline using
ForegroundWaitResult.process_statuses.

Task: foreground wait helper extraction + EINTR fix"
```

---

### Task 4: Full regression test and TODO cleanup

**Files:**
- Modify: `TODO.md` (remove completed items)

- [ ] **Step 1: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: All tests pass.

- [ ] **Step 2: Run E2E tests**

Run: `./e2e/run_tests.sh 2>&1 | tail -10`
Expected: All E2E tests pass (requires debug build from earlier `cargo build`).

- [ ] **Step 3: Remove completed items from TODO.md**

Remove these two lines from `TODO.md`:

```
- [ ] Extract shared foreground job wait helper — `exec_external_with_redirects` (`src/exec/simple.rs`) and `wait_for_foreground_pipeline` (`src/exec/pipeline.rs`) duplicate the same setpgid/give_terminal/WUNTRACED-wait/take_terminal pattern; consider a `wait_for_foreground_job(job_id, pgid)` helper
- [ ] `wait_for_foreground_pipeline` EINTR handling — does not call `process_pending_signals()` on EINTR unlike the simple.rs equivalent; signals received during `waitpid` are silently deferred (`src/exec/pipeline.rs`)
```

- [ ] **Step 4: Commit**

```bash
git add TODO.md
git commit -m "docs(TODO): remove completed foreground wait helper and EINTR items

Completed: unified wait_for_foreground_job helper extracts duplicated
foreground job wait pattern from simple.rs, pipeline.rs, and mod.rs.
Fixed pipeline.rs EINTR handling and mod.rs Stopped handling bugs.

Task: foreground wait helper extraction + EINTR fix"
```
