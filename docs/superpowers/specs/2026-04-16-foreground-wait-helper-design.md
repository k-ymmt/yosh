# Foreground Wait Helper Extraction + EINTR Fix Design

## Date

2026-04-16

## Problem

Three locations in the executor duplicate the same foreground job wait pattern (`waitpid(-pgid, WUNTRACED)` loop with Exited/Signaled/Stopped/ECHILD/EINTR handling):

1. `src/exec/simple.rs:399-442` — `exec_external_with_redirects` (single command, monitor mode)
2. `src/exec/pipeline.rs:140-198` — `wait_for_foreground_pipeline` (multi-command pipeline)
3. `src/exec/mod.rs:570-624` — `wait_for_foreground_job` (used by `fg` builtin)

This duplication has already caused a bug: `pipeline.rs` EINTR handling does not call `process_pending_signals()`, meaning signals received during `waitpid` in pipeline execution are silently deferred. Additionally, `mod.rs` Stopped handling does not set `job.foreground = false`, which the other two sites correctly do.

## POSIX Reference

POSIX 2.11 (Job Control): foreground jobs must be waited on with WUNTRACED for stop signal support. EINTR must be handled to allow trap processing during waits.

## Design

### Approach: Extend `wait_for_foreground_job` with result struct (Approach A)

Unify all three wait loops into a single `wait_for_foreground_job` method on `Executor`, returning a `ForegroundWaitResult` struct.

### Result Struct

Placed in `src/exec/mod.rs`:

```rust
pub(crate) struct ForegroundWaitResult {
    /// Exit status of the last process to report
    pub last_status: i32,
    /// Per-process exit statuses (Pid, exit_code) for pipefail support
    pub process_statuses: Vec<(Pid, i32)>,
    /// Whether the job was stopped (e.g., Ctrl+Z)
    pub stopped: bool,
}
```

### Unified Helper

`wait_for_foreground_job` in `src/exec/mod.rs` is modified to:

1. Look up `pgid` from job table
2. Loop on `waitpid(Pid::from_raw(-pgid.as_raw()), Some(WaitPidFlag::WUNTRACED))`
3. Handle each `WaitStatus` variant:
   - **Exited(pid, code)**: `update_status(pid, Done)`, push to `process_statuses`, check `all_job_processes_done` → `mark_notified` + `remove_job`
   - **Signaled(pid, sig)**: `update_status(pid, Terminated)`, push to `process_statuses`, check `all_job_processes_done` → `mark_notified` + `remove_job`
   - **Stopped(pid, sig)**: `update_status(pid, Stopped)`, set `job.status = Stopped`, set `job.foreground = false`, `format_job` + print, return with `stopped = true`
   - **ECHILD**: `remove_job`, break
   - **EINTR**: `process_pending_signals()`, continue
4. Return `ForegroundWaitResult`

### Caller Changes

#### `simple.rs` — `exec_external_with_redirects`

Replace the inline 45-line wait loop (monitor mode branch) with:

```rust
let job_id = self.env.process.jobs.add_job(child, vec![child], full_cmd, true);
jobs::give_terminal(child).ok();
let result = self.wait_for_foreground_job(job_id);
jobs::take_terminal(shell_pgid).ok();
result.last_status
```

#### `pipeline.rs` — `wait_for_foreground_pipeline`

Delete the `wait_for_foreground_pipeline` method entirely. Replace the call site in `exec_multi_pipeline` with:

```rust
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
```

#### `mod.rs` — `builtin_fg`

Change only the return value extraction:

```rust
let result = self.wait_for_foreground_job(job_id);
result.last_status
```

### Bugs Fixed

1. **EINTR in pipeline.rs**: `process_pending_signals()` now called via unified helper
2. **Stopped in mod.rs**: `job.foreground = false` and `job.status = Stopped` now set via unified helper

### Net Code Change

Approximately 105 lines removed, 50 lines added (~55 lines net reduction).

## Testing

No new tests needed. Rationale:

- EINTR fix cannot be reliably triggered in tests (depends on kernel-level syscall interruption timing)
- The refactoring preserves behavior; existing tests serve as regression verification
- Stopped handling fix (mod.rs) is covered by existing PTY tests for `fg` + Ctrl+Z

Completion criteria: `cargo test` + `./e2e/run_tests.sh` all pass.

## Files Modified

- `src/exec/mod.rs` — add `ForegroundWaitResult`, modify `wait_for_foreground_job`, update `builtin_fg`
- `src/exec/simple.rs` — replace inline wait loop with helper call
- `src/exec/pipeline.rs` — delete `wait_for_foreground_pipeline`, update `exec_multi_pipeline`
- `TODO.md` — remove completed items
