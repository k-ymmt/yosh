# SP2 — `src/env/jobs.rs` Responsibility Redesign

Part of the [Large-File Responsibility Redesign Umbrella](2026-05-06-large-file-redesign-umbrella-design.md).

## Current State

`src/env/jobs.rs` is 1118 lines (~480 production + ~630 tests). Six responsibilities live in one file:

1. **Model** — `JobStatus` enum, `Job` struct (with `id`, `pgid`, `pids`, `command`, `status`, `notified`, `foreground`, `saved_tmodes`), `JobId` type alias.
2. **Storage** — `JobTable`'s `HashMap<JobId, Job>`, `next_id` allocator, `current` / `previous` tracking.
3. **Spec resolution** — `JobSpec` enum, `JobSpecError` enum, `parse_job_spec` free fn, `JobTable::resolve_job_spec` / `resolve` / `resolve_by`.
4. **Notification state machine** — `Job.notified` field plus four `JobTable` methods (`update_status`, `pending_notifications`, `mark_notified`, `cleanup_notified`) that mutate or read it from different angles.
5. **Formatting** — `format_job`, `format_job_long`, `indicator`, `format_status` on `JobTable`.
6. **Terminal control** — `JobTable.shell_tmodes` field, `set_shell_tmodes` / `shell_tmodes` accessors, `give_terminal` / `take_terminal` free fns.

## Proposed Structure

```
src/env/jobs/
  mod.rs          — JobTable struct + storage core + facade re-exports
  model.rs        — Job, JobStatus, JobId, impl Display for JobStatus
  spec.rs         — JobSpec, JobSpecError, parse_job_spec, JobTable::resolve_*
  notification.rs — JobTable::{pending_notifications, mark_notified, cleanup_notified}, predicates
  format.rs       — JobTable::{format_job, format_job_long}, indicator helper
  terminal.rs     — give_terminal, take_terminal, JobTable::{set,get}_shell_tmodes
```

| File | Production | Tests | Total |
|---|---|---|---|
| `mod.rs` | ~180 | ~100 | ~280 |
| `model.rs` | ~80 | ~30 | ~110 |
| `spec.rs` | ~140 | ~110 | ~250 |
| `notification.rs` | ~70 | ~70 | ~140 |
| `format.rs` | ~80 | ~40 | ~120 |
| `terminal.rs` | ~50 | ~30 | ~80 |

Total ~980 vs. original 1118 — about 140 lines removed via `Display` consolidation, predicate extraction, and test pruning.

This design uses Rust's "split impl across files" pattern: `JobTable` is defined in `mod.rs`; submodules add `impl super::JobTable { ... }` blocks for their topic-specific methods. Method call sites do not change.

## Responsibility Redesign

### Notification State Machine — Predicate Centralization

The current notification logic is scattered:

```rust
// update_status
job.status = status;
job.notified = false;

// pending_notifications
!j.notified && matches!(j.status, JobStatus::Done(_) | JobStatus::Terminated(_))

// cleanup_notified
j.notified && matches!(j.status, JobStatus::Done(_) | JobStatus::Terminated(_))
```

Three call sites express two policies (`is_notifiable`, `is_cleanable`) inline, and a third (`update_status`) silently couples notification-reset to status mutation. The intent is harder to read than it should be.

Replace with named predicates and a status-class helper on `JobStatus`:

```rust
// model.rs
impl JobStatus {
    /// Done or Terminated — the job has finished and can be reaped.
    pub fn is_terminal(self) -> bool {
        matches!(self, JobStatus::Done(_) | JobStatus::Terminated(_))
    }
}

// notification.rs
pub(super) fn is_notifiable(job: &Job) -> bool {
    !job.notified && job.status.is_terminal()
}

pub(super) fn is_cleanable(job: &Job) -> bool {
    job.notified && job.status.is_terminal()
}

/// Reset notification state after a status change. Called from JobTable::update_status.
pub(super) fn reset_after_status_change(job: &mut Job) {
    job.notified = false;
}
```

`pending_notifications` and `cleanup_notified` use the predicates; `update_status` calls `reset_after_status_change`. The notification module becomes the single source of truth for the state machine.

### `Display for JobStatus`

`JobTable::format_status` is a method on `JobTable` that doesn't use `self`:

```rust
fn format_status(&self, status: JobStatus) -> String {
    match status {
        JobStatus::Running => "Running".to_string(),
        JobStatus::Stopped(sig) => {
            let name = crate::signal::signal_number_to_name(sig).unwrap_or("UNKNOWN");
            format!("Stopped(SIG{})", name)
        }
        JobStatus::Done(0) => "Done".to_string(),
        JobStatus::Done(code) => format!("Done({})", code),
        JobStatus::Terminated(sig) => {
            let name = crate::signal::signal_number_to_name(sig).unwrap_or("UNKNOWN");
            format!("Terminated(SIG{})", name)
        }
    }
}
```

This is `Display`. Move to `model.rs`:

```rust
impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobStatus::Running => write!(f, "Running"),
            JobStatus::Stopped(sig) => {
                let name = crate::signal::signal_number_to_name(*sig).unwrap_or("UNKNOWN");
                write!(f, "Stopped(SIG{})", name)
            }
            JobStatus::Done(0) => write!(f, "Done"),
            JobStatus::Done(code) => write!(f, "Done({})", code),
            JobStatus::Terminated(sig) => {
                let name = crate::signal::signal_number_to_name(*sig).unwrap_or("UNKNOWN");
                write!(f, "Terminated(SIG{})", name)
            }
        }
    }
}
```

`format.rs` shrinks accordingly:

```rust
impl super::JobTable {
    pub fn format_job(&self, id: JobId) -> Option<String> {
        let job = self.jobs.get(&id)?;
        let indicator = self.indicator(id);
        Some(format!("[{}]{}  {}  {}", job.id, indicator, job.status, job.command))
    }

    pub fn format_job_long(&self, id: JobId) -> Option<String> {
        let job = self.jobs.get(&id)?;
        let indicator = self.indicator(id);
        Some(format!("[{}]{} {}  {}  {}", job.id, indicator, job.pgid.as_raw(), job.status, job.command))
    }

    fn indicator(&self, id: JobId) -> char {
        if self.current == Some(id) { '+' }
        else if self.previous == Some(id) { '-' }
        else { ' ' }
    }
}
```

`format_status` is deleted.

### Terminal Module

`JobTable.shell_tmodes` field stays on `JobTable` (it's storage). Accessors and the free fns move to `terminal.rs`:

```rust
// terminal.rs
const TERMINAL_FD: RawFd = 0;

pub fn give_terminal(pgid: Pid) -> Result<(), nix::Error> { ... }
pub fn take_terminal(shell_pgid: Pid) -> Result<(), nix::Error> { ... }

impl super::JobTable {
    pub fn set_shell_tmodes(&mut self, t: nix::sys::termios::Termios) { ... }
    pub fn shell_tmodes(&self) -> Option<&nix::sys::termios::Termios> { ... }
}
```

Re-export from `mod.rs`: `pub use terminal::{give_terminal, take_terminal};`.

### Visibility

`pub fn parse_job_spec` is exported from `crate::env::jobs::parse_job_spec` but only `JobTable::resolve_job_spec` is the documented entry point. Tightening to `pub(crate)` is **out of scope** for SP2 — handled in a separate visibility-tightening spec following the 2026-05-05 parser pattern.

## Test Reorganization

| Existing Tests | New Location |
|---|---|
| `test_default_is_empty`, `test_add_job_*`, `test_remove_job_*`, `test_get_*`, `test_update_status_*`, `test_find_by_pgid`, `test_last_bg_*`, `test_all_jobs_*` | `mod.rs` |
| `test_job_status_equality`, `test_job_saved_tmodes_*` (Job-level state) | `model.rs` |
| `test_resolve_*`, `test_parse_*` | `spec.rs` |
| `test_pending_notifications_*`, `test_mark_notified_*` | `notification.rs` |
| `test_format_job_*` | `format.rs` |
| `test_job_table_shell_tmodes_*`, `test_set_shell_tmodes_*` | `terminal.rs` |

A new test in `model.rs` verifies `Display for JobStatus` (one assertion per variant). Existing `format_status` tests in `format.rs` either delete or are restated in the same form against `JobStatus.to_string()`.

## PR Breakdown

1. **PR-A — Scaffolding.** Create `src/env/jobs/`. Move `Job` / `JobStatus` to `model.rs` with no code changes. Move terminal-related items (`give_terminal`, `take_terminal`, `set/get_shell_tmodes`) to `terminal.rs`. `JobTable` and all other methods stay in `mod.rs`. Tests stay where their target moves. Pure relocation, zero behavior change.
2. **PR-B — Spec module.** Move `parse_job_spec`, `JobSpec`, `JobSpecError`, and the three `resolve_*` methods to `spec.rs`. Move spec-related tests.
3. **PR-C — Notification + format redesign.** Move `pending_notifications` / `mark_notified` / `cleanup_notified` to `notification.rs`. Introduce `is_notifiable` / `is_cleanable` predicates and `JobStatus::is_terminal`. Move `format_job` / `format_job_long` to `format.rs`. Add `Display for JobStatus` in `model.rs`. Delete `format_status`. Move corresponding tests.

PR-A and PR-B are mechanical. PR-C is the responsibility-redesign body.

## Risks

- **`Display for JobStatus` is observable** — anyone using `format!("{}", status)` now succeeds where it would have failed to compile before. This is a strict capability addition, not a breaking change.
- **`parse_job_spec` signature must be preserved** — `pub fn(&str) -> Result<JobSpec<'_>, JobSpecError>`. The lifetime tie to the input slice is part of the API.
- **Multiple `impl JobTable` blocks** — supported by Rust, transparent to callers, transparent to rustdoc (which collapses them on a single page).
- **Job-control behavior is touched** — `tests/pty_interactive.rs` (`set -m`, `fg`/`bg`) and the `e2e/06-jobs/` suite must pass without flake. Run both before declaring DoD.
- **Caller paths unchanged** — `crate::env::jobs::JobTable`, `crate::env::jobs::parse_job_spec`, `crate::env::jobs::give_terminal`, etc. all resolve identically because `jobs.rs` becomes `jobs/mod.rs`. Existing imports in `exec/`, `builtin/`, `interactive/` are untouched.

## Definition of Done

- `cargo test` PASS (unit + integration).
- `./e2e/run_tests.sh` PASS (full).
- `tests/pty_interactive.rs` PASS — `set -m` and `fg`/`bg` flows verified non-flaky over 3 runs.
- Each production file ≤ 250 lines (`mod.rs` ~180 lines is well within).
- TODO.md entries about `JobTable::update_status per-process status tracking` and similar future items are preserved.
- Caller imports (`crate::env::jobs::*`) require zero diff.
