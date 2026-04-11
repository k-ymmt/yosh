# Job Control Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement full POSIX job control — resolve Phase 6 known limitations where `-m` (monitor) and `-b` (notify) flags are settable but have no behavioral effect.

**Architecture:** New `src/env/jobs.rs` module containing `JobTable`, `Job`, `JobStatus` types and terminal control functions. Replace `BgJob`/`Vec<BgJob>` in ShellEnv. Extend `signal.rs` with job-control signals (SIGTSTP, SIGCHLD, etc.). Add `fg`/`bg`/`jobs` regular builtins. Modify pipeline and background execution to register jobs and manage terminal control via `tcsetpgrp`.

**Tech Stack:** Rust (edition 2024), nix 0.31 (unistd, signal, sys::wait), libc 0.2

---

## File Structure

### New Files
- `src/env/jobs.rs` — `JobTable`, `Job`, `JobStatus`, `JobId` type alias, terminal control (`give_terminal`/`take_terminal`), job specifier parsing, POSIX format display

### Modified Files
- `src/env/mod.rs` — Remove `BgJob`, replace `bg_jobs: Vec<BgJob>` with `jobs: JobTable`, remove `last_bg_pid: Option<i32>`, add `shell_pgid: Pid`
- `src/signal.rs` — Add CHLD/CONT/STOP/TSTP/TTIN/TTOU to `SIGNAL_TABLE`, add `init_job_control_signals()` / `reset_job_control_signals()` / `setup_foreground_child_signals()` / `setup_background_child_signals()`
- `src/builtin/mod.rs` — Register `fg`/`bg`/`jobs` as regular builtins, add dispatch
- `src/exec/mod.rs` — Rewrite `exec_async()` for JobTable, enhance `reap_zombies()` with WUNTRACED, migrate `builtin_wait()`, add `fg`/`bg`/`jobs` builtin impls, add `wait_for_foreground_job()`, add notification display method
- `src/exec/pipeline.rs` — Add foreground job registration + `tcsetpgrp` + WUNTRACED wait when monitor mode is active
- `src/exec/command.rs` — Add `wait_child_wuntraced()` returning richer status
- `src/interactive/mod.rs` — Auto-enable monitor mode in `Repl::new()`, display job notifications before prompt
- `src/expand/param.rs` — Change `$!` expansion from `env.last_bg_pid` to `env.jobs.last_bg_pid()`

---

### Task 1: JobTable Core Data Structures

**Files:**
- Create: `src/env/jobs.rs`
- Modify: `src/env/mod.rs:1` (add `pub mod jobs;`)

- [ ] **Step 1: Write failing tests for JobStatus, Job, and JobTable creation**

In `src/env/jobs.rs`:

```rust
use std::collections::HashMap;
use nix::unistd::Pid;

/// Job identifier — 1-based, monotonically increasing.
pub type JobId = u32;

/// Status of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Stopped(i32),         // signal number (e.g. SIGTSTP=20)
    Done(i32),            // exit code
    Terminated(i32),      // killed by signal number
}

/// A single job tracked by the shell.
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

/// Tracks all jobs in the shell.
#[derive(Debug, Clone, Default)]
pub struct JobTable {
    jobs: HashMap<JobId, Job>,
    next_id: JobId,
    current: Option<JobId>,
    previous: Option<JobId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_table_default_is_empty() {
        let table = JobTable::default();
        assert!(table.jobs.is_empty());
        assert_eq!(table.current, None);
        assert_eq!(table.previous, None);
    }

    #[test]
    fn test_job_status_eq() {
        assert_eq!(JobStatus::Running, JobStatus::Running);
        assert_eq!(JobStatus::Done(0), JobStatus::Done(0));
        assert_ne!(JobStatus::Done(0), JobStatus::Done(1));
        assert_eq!(JobStatus::Stopped(20), JobStatus::Stopped(20));
        assert_eq!(JobStatus::Terminated(15), JobStatus::Terminated(15));
    }
}
```

- [ ] **Step 2: Run test to verify it compiles and passes**

Run: `cargo test --lib env::jobs::tests -- --nocapture`
Expected: 2 tests PASS

- [ ] **Step 3: Add `pub mod jobs;` to env/mod.rs**

In `src/env/mod.rs`, after line 1 (`pub mod aliases;`), add:

```rust
pub mod jobs;
```

- [ ] **Step 4: Run full test suite to verify no regressions**

Run: `cargo test --lib`
Expected: All existing tests pass

- [ ] **Step 5: Commit**

```bash
git add src/env/jobs.rs src/env/mod.rs
git commit -m "feat(jobs): add JobTable core data structures (JobStatus, Job, JobTable)"
```

---

### Task 2: JobTable Methods — add_job, remove_job, get, current/previous

**Files:**
- Modify: `src/env/jobs.rs`

- [ ] **Step 1: Write failing tests for add_job and get**

Append to the `tests` module in `src/env/jobs.rs`:

```rust
    #[test]
    fn test_add_job_assigns_incrementing_ids() {
        let mut table = JobTable::default();
        let id1 = table.add_job(Pid::from_raw(100), vec![Pid::from_raw(100)], "sleep 10".into(), false);
        let id2 = table.add_job(Pid::from_raw(200), vec![Pid::from_raw(200)], "cat".into(), false);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn test_add_job_updates_current_previous() {
        let mut table = JobTable::default();
        let id1 = table.add_job(Pid::from_raw(100), vec![Pid::from_raw(100)], "sleep 10".into(), false);
        assert_eq!(table.current, Some(id1));
        assert_eq!(table.previous, None);

        let id2 = table.add_job(Pid::from_raw(200), vec![Pid::from_raw(200)], "cat".into(), false);
        assert_eq!(table.current, Some(id2));
        assert_eq!(table.previous, Some(id1));
    }

    #[test]
    fn test_get_returns_job() {
        let mut table = JobTable::default();
        let id = table.add_job(Pid::from_raw(100), vec![Pid::from_raw(100)], "sleep 10".into(), false);
        let job = table.get(id).unwrap();
        assert_eq!(job.pgid, Pid::from_raw(100));
        assert_eq!(job.command, "sleep 10");
        assert_eq!(job.status, JobStatus::Running);
        assert!(!job.foreground);
    }

    #[test]
    fn test_get_nonexistent_returns_none() {
        let table = JobTable::default();
        assert!(table.get(999).is_none());
    }

    #[test]
    fn test_remove_job_updates_current_previous() {
        let mut table = JobTable::default();
        let id1 = table.add_job(Pid::from_raw(100), vec![Pid::from_raw(100)], "sleep".into(), false);
        let id2 = table.add_job(Pid::from_raw(200), vec![Pid::from_raw(200)], "cat".into(), false);
        // current=id2, previous=id1
        table.remove_job(id2);
        assert_eq!(table.current, Some(id1));
        assert_eq!(table.previous, None);
    }

    #[test]
    fn test_current_job_and_previous_job() {
        let mut table = JobTable::default();
        assert!(table.current_job().is_none());
        assert!(table.previous_job().is_none());

        table.add_job(Pid::from_raw(100), vec![Pid::from_raw(100)], "sleep".into(), false);
        table.add_job(Pid::from_raw(200), vec![Pid::from_raw(200)], "cat".into(), false);

        assert_eq!(table.current_job().unwrap().command, "cat");
        assert_eq!(table.previous_job().unwrap().command, "sleep");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib env::jobs::tests -- --nocapture`
Expected: FAIL — `add_job`, `get`, `remove_job`, `current_job`, `previous_job` not defined

- [ ] **Step 3: Implement add_job, remove_job, get, get_mut, current_job, previous_job**

Add to `impl JobTable` block in `src/env/jobs.rs`:

```rust
impl JobTable {
    /// Add a new job to the table. Returns the assigned job ID.
    pub fn add_job(&mut self, pgid: Pid, pids: Vec<Pid>, command: String, foreground: bool) -> JobId {
        self.next_id += 1;
        let id = self.next_id;
        let job = Job {
            id,
            pgid,
            pids,
            command,
            status: JobStatus::Running,
            notified: false,
            foreground,
        };
        self.jobs.insert(id, job);
        // Update current/previous
        self.previous = self.current;
        self.current = Some(id);
        id
    }

    /// Remove a job by ID. Updates current/previous tracking.
    pub fn remove_job(&mut self, id: JobId) {
        self.jobs.remove(&id);
        if self.current == Some(id) {
            self.current = self.previous.take();
            // Find next most recent as new previous
            if self.current.is_some() {
                self.previous = self.jobs.keys()
                    .filter(|&&k| Some(k) != self.current)
                    .max()
                    .copied();
            }
        } else if self.previous == Some(id) {
            self.previous = self.jobs.keys()
                .filter(|&&k| Some(k) != self.current)
                .max()
                .copied();
        }
    }

    /// Get a job by ID.
    pub fn get(&self, id: JobId) -> Option<&Job> {
        self.jobs.get(&id)
    }

    /// Get a mutable reference to a job by ID.
    pub fn get_mut(&mut self, id: JobId) -> Option<&mut Job> {
        self.jobs.get_mut(&id)
    }

    /// Get the current job (%+ / %%).
    pub fn current_job(&self) -> Option<&Job> {
        self.current.and_then(|id| self.jobs.get(&id))
    }

    /// Get the previous job (%-).
    pub fn previous_job(&self) -> Option<&Job> {
        self.previous.and_then(|id| self.jobs.get(&id))
    }

    /// Get the current job ID.
    pub fn current_id(&self) -> Option<JobId> {
        self.current
    }

    /// Get the previous job ID.
    pub fn previous_id(&self) -> Option<JobId> {
        self.previous
    }

    /// Returns true if the table has no jobs.
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib env::jobs::tests -- --nocapture`
Expected: All tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/env/jobs.rs
git commit -m "feat(jobs): implement JobTable add/remove/get/current/previous methods"
```

---

### Task 3: JobTable Methods — update_status, find_by_pgid, last_bg_pid, all_jobs

**Files:**
- Modify: `src/env/jobs.rs`

- [ ] **Step 1: Write failing tests**

Append to `tests` module in `src/env/jobs.rs`:

```rust
    #[test]
    fn test_update_status_by_pid() {
        let mut table = JobTable::default();
        table.add_job(Pid::from_raw(100), vec![Pid::from_raw(100)], "sleep".into(), false);
        table.update_status(Pid::from_raw(100), JobStatus::Done(0));
        assert_eq!(table.get(1).unwrap().status, JobStatus::Done(0));
    }

    #[test]
    fn test_update_status_unknown_pid_is_noop() {
        let mut table = JobTable::default();
        table.add_job(Pid::from_raw(100), vec![Pid::from_raw(100)], "sleep".into(), false);
        table.update_status(Pid::from_raw(999), JobStatus::Done(0));
        assert_eq!(table.get(1).unwrap().status, JobStatus::Running);
    }

    #[test]
    fn test_find_by_pgid() {
        let mut table = JobTable::default();
        table.add_job(Pid::from_raw(100), vec![Pid::from_raw(100)], "sleep".into(), false);
        assert!(table.find_by_pgid(Pid::from_raw(100)).is_some());
        assert!(table.find_by_pgid(Pid::from_raw(999)).is_none());
    }

    #[test]
    fn test_last_bg_pid() {
        let mut table = JobTable::default();
        assert_eq!(table.last_bg_pid(), None);
        table.add_job(Pid::from_raw(100), vec![Pid::from_raw(100)], "sleep".into(), false);
        assert_eq!(table.last_bg_pid(), Some(Pid::from_raw(100)));
        table.add_job(Pid::from_raw(200), vec![Pid::from_raw(200), Pid::from_raw(201)], "cat | sort".into(), true);
        // last_bg_pid returns most recent background job's pgid
        assert_eq!(table.last_bg_pid(), Some(Pid::from_raw(100)));
    }

    #[test]
    fn test_all_jobs_sorted_by_id() {
        let mut table = JobTable::default();
        table.add_job(Pid::from_raw(300), vec![Pid::from_raw(300)], "third".into(), false);
        table.add_job(Pid::from_raw(100), vec![Pid::from_raw(100)], "fourth".into(), false);
        let jobs: Vec<JobId> = table.all_jobs().map(|j| j.id).collect();
        assert_eq!(jobs, vec![1, 2]);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib env::jobs::tests -- --nocapture`
Expected: FAIL — methods not defined

- [ ] **Step 3: Implement the methods**

Add to `impl JobTable` in `src/env/jobs.rs`:

```rust
    /// Update the status of the job containing the given PID.
    pub fn update_status(&mut self, pid: Pid, status: JobStatus) {
        for job in self.jobs.values_mut() {
            if job.pids.contains(&pid) {
                job.status = status;
                job.notified = false;
                return;
            }
        }
    }

    /// Find a job by its process group ID.
    pub fn find_by_pgid(&self, pgid: Pid) -> Option<&Job> {
        self.jobs.values().find(|j| j.pgid == pgid)
    }

    /// Find a mutable job by its process group ID.
    pub fn find_by_pgid_mut(&mut self, pgid: Pid) -> Option<&mut Job> {
        self.jobs.values_mut().find(|j| j.pgid == pgid)
    }

    /// Return the PID of the most recently started background job (for $!).
    pub fn last_bg_pid(&self) -> Option<Pid> {
        self.jobs.values()
            .filter(|j| !j.foreground)
            .max_by_key(|j| j.id)
            .map(|j| j.pgid)
    }

    /// Iterate over all jobs, sorted by ID.
    pub fn all_jobs(&self) -> impl Iterator<Item = &Job> {
        let mut jobs: Vec<&Job> = self.jobs.values().collect();
        jobs.sort_by_key(|j| j.id);
        jobs.into_iter()
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib env::jobs::tests -- --nocapture`
Expected: All tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/env/jobs.rs
git commit -m "feat(jobs): implement update_status, find_by_pgid, last_bg_pid, all_jobs"
```

---

### Task 4: JobTable Methods — resolve_job_spec, pending_notifications, format_job

**Files:**
- Modify: `src/env/jobs.rs`

- [ ] **Step 1: Write failing tests**

Append to `tests` module in `src/env/jobs.rs`:

```rust
    #[test]
    fn test_resolve_job_spec_percent_n() {
        let mut table = JobTable::default();
        table.add_job(Pid::from_raw(100), vec![Pid::from_raw(100)], "sleep".into(), false);
        assert_eq!(table.resolve_job_spec("%1"), Some(1));
        assert_eq!(table.resolve_job_spec("%2"), None);
    }

    #[test]
    fn test_resolve_job_spec_current_previous() {
        let mut table = JobTable::default();
        let id1 = table.add_job(Pid::from_raw(100), vec![Pid::from_raw(100)], "sleep".into(), false);
        let id2 = table.add_job(Pid::from_raw(200), vec![Pid::from_raw(200)], "cat".into(), false);
        assert_eq!(table.resolve_job_spec("%%"), Some(id2));
        assert_eq!(table.resolve_job_spec("%+"), Some(id2));
        assert_eq!(table.resolve_job_spec("%-"), Some(id1));
    }

    #[test]
    fn test_resolve_job_spec_invalid() {
        let table = JobTable::default();
        assert_eq!(table.resolve_job_spec("%abc"), None);
        assert_eq!(table.resolve_job_spec(""), None);
    }

    #[test]
    fn test_pending_notifications() {
        let mut table = JobTable::default();
        table.add_job(Pid::from_raw(100), vec![Pid::from_raw(100)], "sleep".into(), false);
        table.add_job(Pid::from_raw(200), vec![Pid::from_raw(200)], "cat".into(), false);

        // No notifications while Running
        assert!(table.pending_notifications().is_empty());

        // Mark one as Done
        table.update_status(Pid::from_raw(100), JobStatus::Done(0));
        let pending = table.pending_notifications();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0], 1);
    }

    #[test]
    fn test_mark_notified() {
        let mut table = JobTable::default();
        table.add_job(Pid::from_raw(100), vec![Pid::from_raw(100)], "sleep".into(), false);
        table.update_status(Pid::from_raw(100), JobStatus::Done(0));
        table.mark_notified(1);
        assert!(table.pending_notifications().is_empty());
    }

    #[test]
    fn test_format_job_running() {
        let mut table = JobTable::default();
        table.add_job(Pid::from_raw(100), vec![Pid::from_raw(100)], "sleep 10".into(), false);
        let output = table.format_job(1);
        assert!(output.is_some());
        let s = output.unwrap();
        assert!(s.contains("[1]"));
        assert!(s.contains("Running"));
        assert!(s.contains("sleep 10"));
    }

    #[test]
    fn test_format_job_done() {
        let mut table = JobTable::default();
        table.add_job(Pid::from_raw(100), vec![Pid::from_raw(100)], "sleep 1".into(), false);
        table.update_status(Pid::from_raw(100), JobStatus::Done(0));
        let s = table.format_job(1).unwrap();
        assert!(s.contains("Done"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib env::jobs::tests -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement the methods**

Add to `impl JobTable` in `src/env/jobs.rs`:

```rust
    /// Parse a job specifier string and return the matching job ID.
    /// Supports: %n, %%, %+, %-
    pub fn resolve_job_spec(&self, spec: &str) -> Option<JobId> {
        if spec == "%%" || spec == "%+" {
            return self.current;
        }
        if spec == "%-" {
            return self.previous;
        }
        if let Some(num_str) = spec.strip_prefix('%') {
            if let Ok(n) = num_str.parse::<JobId>() {
                if self.jobs.contains_key(&n) {
                    return Some(n);
                }
            }
        }
        None
    }

    /// Return IDs of jobs that have completed/terminated but not yet been notified.
    /// Stopped jobs are excluded — their notification is handled immediately at stop time.
    pub fn pending_notifications(&self) -> Vec<JobId> {
        let mut ids: Vec<JobId> = self.jobs.values()
            .filter(|j| !j.notified && matches!(j.status, JobStatus::Done(_) | JobStatus::Terminated(_)))
            .map(|j| j.id)
            .collect();
        ids.sort();
        ids
    }

    /// Mark a job as notified.
    pub fn mark_notified(&mut self, id: JobId) {
        if let Some(job) = self.jobs.get_mut(&id) {
            job.notified = true;
        }
    }

    /// Format a job for display in POSIX format.
    /// Returns `[n]+  Status  command` or `[n]-  Status  command`.
    pub fn format_job(&self, id: JobId) -> Option<String> {
        let job = self.jobs.get(&id)?;
        let indicator = if self.current == Some(id) {
            "+"
        } else if self.previous == Some(id) {
            "-"
        } else {
            " "
        };

        let status_str = match job.status {
            JobStatus::Running => "Running".to_string(),
            JobStatus::Stopped(sig) => {
                let name = crate::signal::signal_number_to_name(sig)
                    .unwrap_or("STOP");
                format!("Stopped(SIG{})", name)
            }
            JobStatus::Done(code) => {
                if code == 0 {
                    "Done".to_string()
                } else {
                    format!("Done({})", code)
                }
            }
            JobStatus::Terminated(sig) => {
                let name = crate::signal::signal_number_to_name(sig)
                    .unwrap_or("UNKNOWN");
                format!("Terminated(SIG{})", name)
            }
        };

        Some(format!("[{}]{:<2} {:<24}{}", id, indicator, status_str, job.command))
    }

    /// Format a job with PID for `jobs -l`.
    pub fn format_job_long(&self, id: JobId) -> Option<String> {
        let job = self.jobs.get(&id)?;
        let indicator = if self.current == Some(id) {
            "+"
        } else if self.previous == Some(id) {
            "-"
        } else {
            " "
        };

        let status_str = match job.status {
            JobStatus::Running => "Running".to_string(),
            JobStatus::Stopped(_) => "Stopped".to_string(),
            JobStatus::Done(code) => {
                if code == 0 { "Done".to_string() } else { format!("Done({})", code) }
            }
            JobStatus::Terminated(sig) => {
                let name = crate::signal::signal_number_to_name(sig).unwrap_or("UNKNOWN");
                format!("Terminated(SIG{})", name)
            }
        };

        Some(format!("[{}]{:<2} {:<8} {:<24}{}", id, indicator, job.pgid.as_raw(), status_str, job.command))
    }

    /// Remove all jobs that have been notified as done/terminated.
    pub fn cleanup_notified(&mut self) {
        let to_remove: Vec<JobId> = self.jobs.values()
            .filter(|j| j.notified && matches!(j.status, JobStatus::Done(_) | JobStatus::Terminated(_)))
            .map(|j| j.id)
            .collect();
        for id in to_remove {
            self.remove_job(id);
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib env::jobs::tests -- --nocapture`
Expected: All tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/env/jobs.rs
git commit -m "feat(jobs): implement resolve_job_spec, pending_notifications, format_job"
```

---

### Task 5: Terminal Control Functions

**Files:**
- Modify: `src/env/jobs.rs`

- [ ] **Step 1: Write the terminal control functions**

Add at the top of `src/env/jobs.rs` (after the imports):

```rust
use std::os::unix::io::RawFd;

/// File descriptor for terminal control (stdin).
const TERMINAL_FD: RawFd = 0;

/// Give terminal control to the specified process group.
pub fn give_terminal(pgid: Pid) -> Result<(), nix::Error> {
    nix::unistd::tcsetpgrp(TERMINAL_FD, pgid)
}

/// Take terminal control back to the shell's process group.
pub fn take_terminal(shell_pgid: Pid) -> Result<(), nix::Error> {
    nix::unistd::tcsetpgrp(TERMINAL_FD, shell_pgid)
}
```

- [ ] **Step 2: Write a compile-check test**

Append to `tests` module:

```rust
    #[test]
    fn test_terminal_functions_exist() {
        // Smoke test: functions are callable (will fail in CI without TTY, but compile-checks)
        // Don't actually call tcsetpgrp in unit tests — it requires a real TTY.
        let _ = give_terminal as fn(Pid) -> Result<(), nix::Error>;
        let _ = take_terminal as fn(Pid) -> Result<(), nix::Error>;
    }
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --lib env::jobs::tests -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/env/jobs.rs
git commit -m "feat(jobs): add give_terminal/take_terminal for tcsetpgrp control"
```

---

### Task 6: Signal Table and Job Control Signal Functions

**Files:**
- Modify: `src/signal.rs`

- [ ] **Step 1: Write failing tests for new signal table entries**

Append to `tests` module in `src/signal.rs`:

```rust
    #[test]
    fn test_signal_table_has_job_control_signals() {
        assert_eq!(signal_name_to_number("CHLD").unwrap(), 17);
        assert_eq!(signal_name_to_number("CONT").unwrap(), 18);
        assert_eq!(signal_name_to_number("STOP").unwrap(), 19);
        assert_eq!(signal_name_to_number("TSTP").unwrap(), 20);
        assert_eq!(signal_name_to_number("TTIN").unwrap(), 21);
        assert_eq!(signal_name_to_number("TTOU").unwrap(), 22);
    }

    #[test]
    fn test_signal_number_to_name_job_control() {
        assert_eq!(signal_number_to_name(17), Some("CHLD"));
        assert_eq!(signal_number_to_name(20), Some("TSTP"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib signal::tests -- --nocapture`
Expected: FAIL — signals not in table

- [ ] **Step 3: Add job control signals to SIGNAL_TABLE**

In `src/signal.rs`, modify `SIGNAL_TABLE` to add the new entries:

```rust
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
    (17, "CHLD"),
    (18, "CONT"),
    (19, "STOP"),
    (20, "TSTP"),
    (21, "TTIN"),
    (22, "TTOU"),
];
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib signal::tests -- --nocapture`
Expected: PASS

- [ ] **Step 5: Add job control signal setup functions**

Add to `src/signal.rs` after the `reset_child_signals` function:

```rust
/// Set up job control signals for the shell process itself.
/// Ignores SIGTSTP, SIGTTIN, SIGTTOU so the shell is not stopped.
/// Adds SIGCHLD to the self-pipe handler.
pub fn init_job_control_signals() {
    // Ignore terminal stop signals for the shell itself
    ignore_signal(libc::SIGTSTP);
    ignore_signal(libc::SIGTTIN);
    ignore_signal(libc::SIGTTOU);

    // Register SIGCHLD handler via self-pipe so we can detect child state changes
    let sa = SigAction::new(
        SigHandler::Handler(signal_handler),
        SaFlags::SA_RESTART,
        SigSet::empty(),
    );
    let sig = Signal::try_from(libc::SIGCHLD).expect("SIGCHLD is valid");
    unsafe {
        sigaction(sig, &sa).expect("sigaction(SIGCHLD) failed");
    }
}

/// Reset job control signals to defaults.
/// Called when monitor mode is disabled.
pub fn reset_job_control_signals() {
    default_signal(libc::SIGTSTP);
    default_signal(libc::SIGTTIN);
    default_signal(libc::SIGTTOU);
    default_signal(libc::SIGCHLD);
}

/// Set up signals for a foreground child process.
/// Restores SIGTSTP, SIGTTIN, SIGTTOU to SIG_DFL so the child can be stopped.
pub fn setup_foreground_child_signals(ignored: &[i32]) {
    reset_child_signals(ignored);
    // Ensure job control signals are at defaults for the child
    if !ignored.contains(&libc::SIGTSTP) {
        default_signal(libc::SIGTSTP);
    }
    if !ignored.contains(&libc::SIGTTIN) {
        default_signal(libc::SIGTTIN);
    }
    if !ignored.contains(&libc::SIGTTOU) {
        default_signal(libc::SIGTTOU);
    }
}

/// Set up signals for a background child process.
/// Ignores SIGTTIN to prevent background reads from stopping the process.
pub fn setup_background_child_signals(ignored: &[i32]) {
    reset_child_signals(ignored);
    ignore_signal(libc::SIGTTIN);
    if !ignored.contains(&libc::SIGTSTP) {
        default_signal(libc::SIGTSTP);
    }
    if !ignored.contains(&libc::SIGTTOU) {
        default_signal(libc::SIGTTOU);
    }
}
```

- [ ] **Step 6: Write compile-check test**

Append to `tests` module:

```rust
    #[test]
    fn test_job_control_signal_functions_exist() {
        // Compile check: functions are callable
        let _ = init_job_control_signals as fn();
        let _ = reset_job_control_signals as fn();
        let _ = setup_foreground_child_signals as fn(&[i32]);
        let _ = setup_background_child_signals as fn(&[i32]);
    }
```

- [ ] **Step 7: Run full test suite**

Run: `cargo test --lib`
Expected: All tests PASS

- [ ] **Step 8: Commit**

```bash
git add src/signal.rs
git commit -m "feat(signal): add job control signals to table and signal setup functions"
```

---

### Task 7: Replace BgJob with JobTable in ShellEnv

**Files:**
- Modify: `src/env/mod.rs`

- [ ] **Step 1: Update ShellEnv to use JobTable**

In `src/env/mod.rs`:

1. Add import at top: `use jobs::JobTable;`
2. Remove the `BgJob` struct (lines 279-284)
3. In `ShellEnv`, replace `bg_jobs: Vec<BgJob>` with `pub jobs: JobTable`, remove `last_bg_pid: Option<i32>`, add `pub shell_pgid: Pid`
4. In `ShellEnv::new()`, replace `bg_jobs: Vec::new()` with `jobs: JobTable::default()`, remove `last_bg_pid: None`, add `shell_pgid: getpid()`

The updated `ShellEnv` struct:

```rust
pub struct ShellEnv {
    pub vars: VarStore,
    pub last_exit_status: i32,
    pub shell_pid: Pid,
    pub shell_pgid: Pid,
    pub shell_name: String,
    pub functions: HashMap<String, FunctionDef>,
    pub flow_control: Option<FlowControl>,
    pub options: ShellOptions,
    pub traps: TrapStore,
    pub aliases: AliasStore,
    pub jobs: JobTable,
    pub expansion_error: bool,
    pub is_interactive: bool,
}
```

Updated `ShellEnv::new()`:

```rust
    pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
        let mut vars = VarStore::from_environ();
        vars.set_positional_params(args);
        ShellEnv {
            vars,
            last_exit_status: 0,
            shell_pid: getpid(),
            shell_pgid: nix::unistd::getpgrp(),
            shell_name: shell_name.into(),
            functions: HashMap::new(),
            flow_control: None,
            options: ShellOptions::default(),
            traps: TrapStore::default(),
            aliases: AliasStore::default(),
            jobs: JobTable::default(),
            expansion_error: false,
            is_interactive: false,
        }
    }
```

- [ ] **Step 2: Update tests in env/mod.rs**

Remove the `test_bg_jobs` test. Update `test_shell_env_construction` if it references `bg_jobs`.

Replace `test_bg_jobs` with:

```rust
    #[test]
    fn test_jobs_table() {
        let env = ShellEnv::new("kish", vec![]);
        assert!(env.jobs.is_empty());
    }

    #[test]
    fn test_shell_pgid() {
        let env = ShellEnv::new("kish", vec![]);
        assert!(env.shell_pgid.as_raw() > 0);
    }
```

- [ ] **Step 3: Attempt to compile — expect errors in dependent files**

Run: `cargo check 2>&1 | head -50`
Expected: Compilation errors in `exec/mod.rs` and `expand/param.rs` referencing `bg_jobs` and `last_bg_pid`

- [ ] **Step 4: Fix exec/mod.rs — update exec_async**

In `src/exec/mod.rs`, update `exec_async()` (around line 682):

Replace:
```rust
            Ok(ForkResult::Parent { child }) => {
                nix::unistd::setpgid(child, child).ok();
                self.env.bg_jobs.push(crate::env::BgJob {
                    pid: child,
                    status: None,
                });
                self.env.last_bg_pid = Some(child.as_raw());
                0
            }
```

With:
```rust
            Ok(ForkResult::Parent { child }) => {
                nix::unistd::setpgid(child, child).ok();
                let cmd_str = format!("{}", and_or); // will fix display in a moment
                let job_id = self.env.jobs.add_job(child, vec![child], cmd_str, false);
                eprintln!("[{}] {}", job_id, child.as_raw());
                0
            }
```

Note: `and_or` does not implement Display. Use a placeholder `"(background)".to_string()` for now — we will improve command string capture in Task 10.

Temporary version:
```rust
            Ok(ForkResult::Parent { child }) => {
                nix::unistd::setpgid(child, child).ok();
                let job_id = self.env.jobs.add_job(child, vec![child], "(background)".into(), false);
                eprintln!("[{}] {}", job_id, child.as_raw());
                0
            }
```

- [ ] **Step 5: Fix exec/mod.rs — update reap_zombies**

Replace the `reap_zombies` method with:

```rust
    fn reap_zombies(&mut self) {
        use crate::env::jobs::JobStatus;
        loop {
            match nix::sys::wait::waitpid(
                nix::unistd::Pid::from_raw(-1),
                Some(nix::sys::wait::WaitPidFlag::WNOHANG | nix::sys::wait::WaitPidFlag::WUNTRACED),
            ) {
                Ok(nix::sys::wait::WaitStatus::Exited(pid, code)) => {
                    self.env.jobs.update_status(pid, JobStatus::Done(code));
                }
                Ok(nix::sys::wait::WaitStatus::Signaled(pid, sig, _)) => {
                    self.env.jobs.update_status(pid, JobStatus::Terminated(sig as i32));
                }
                Ok(nix::sys::wait::WaitStatus::Stopped(pid, sig)) => {
                    self.env.jobs.update_status(pid, JobStatus::Stopped(sig as i32));
                }
                Ok(nix::sys::wait::WaitStatus::StillAlive) => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    }
```

- [ ] **Step 6: Fix exec/mod.rs — update builtin_wait**

Replace all `self.env.bg_jobs` references in `builtin_wait` with `self.env.jobs` usage. The target_pids section:

```rust
    fn builtin_wait(&mut self, args: &[String]) -> i32 {
        use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
        use nix::unistd::Pid;

        let target_pids: Vec<Pid> = if args.is_empty() {
            self.env.jobs.all_jobs()
                .filter(|j| matches!(j.status, crate::env::jobs::JobStatus::Running))
                .flat_map(|j| j.pids.iter().copied())
                .collect()
        } else {
            let mut pids = Vec::new();
            for arg in args {
                // Support %n job spec
                if arg.starts_with('%') {
                    match self.env.jobs.resolve_job_spec(arg) {
                        Some(id) => {
                            if let Some(job) = self.env.jobs.get(id) {
                                pids.push(job.pgid);
                            }
                        }
                        None => {
                            eprintln!("kish: wait: {}: no such job", arg);
                            return 2;
                        }
                    }
                } else {
                    match arg.parse::<i32>() {
                        Ok(n) => pids.push(Pid::from_raw(n)),
                        Err(_) => {
                            eprintln!("kish: wait: {}: not a pid", arg);
                            return 2;
                        }
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
            // Check if already reaped in job table
            for job in self.env.jobs.all_jobs() {
                if job.pids.contains(pid) {
                    match job.status {
                        crate::env::jobs::JobStatus::Done(code) => {
                            last_status = code;
                            continue;
                        }
                        crate::env::jobs::JobStatus::Terminated(sig) => {
                            last_status = 128 + sig;
                            continue;
                        }
                        _ => {}
                    }
                }
            }

            loop {
                match waitpid(*pid, Some(WaitPidFlag::WNOHANG)) {
                    Ok(WaitStatus::Exited(_, code)) => {
                        self.env.jobs.update_status(*pid, crate::env::jobs::JobStatus::Done(code));
                        last_status = code;
                        break;
                    }
                    Ok(WaitStatus::Signaled(_, sig, _)) => {
                        let code = 128 + sig as i32;
                        self.env.jobs.update_status(*pid, crate::env::jobs::JobStatus::Terminated(sig as i32));
                        last_status = code;
                        break;
                    }
                    Ok(WaitStatus::StillAlive) => {
                        let pipe_fd = signal::self_pipe_read_fd();
                        let mut fds = [nix::poll::PollFd::new(
                            unsafe { std::os::fd::BorrowedFd::borrow_raw(pipe_fd) },
                            nix::poll::PollFlags::POLLIN,
                        )];
                        match nix::poll::poll(&mut fds, nix::poll::PollTimeout::from(50u16)) {
                            Ok(_)
                                if fds[0]
                                    .revents()
                                    .is_some_and(|r| r.contains(nix::poll::PollFlags::POLLIN)) =>
                            {
                                let signals = signal::drain_pending_signals();
                                if !signals.is_empty() {
                                    self.process_pending_signals();
                                    last_status = 128 + signals[0];
                                    return last_status;
                                }
                            }
                            Err(nix::errno::Errno::EINTR) => {}
                            _ => {}
                        }
                    }
                    Err(nix::errno::Errno::ECHILD) => {
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

- [ ] **Step 7: Fix expand/param.rs — update $! expansion**

In `src/expand/param.rs`, in the `expand_special` function, change:

```rust
        SpecialParam::Bang => env.last_bg_pid.map(|p| p.to_string()).unwrap_or_default(),
```

to:

```rust
        SpecialParam::Bang => env.jobs.last_bg_pid().map(|p| p.as_raw().to_string()).unwrap_or_default(),
```

- [ ] **Step 8: Run cargo check to verify compilation**

Run: `cargo check`
Expected: Compiles successfully (may have warnings)

- [ ] **Step 9: Run full test suite**

Run: `cargo test`
Expected: All tests PASS (some e2e tests may need adjustment)

- [ ] **Step 10: Commit**

```bash
git add src/env/mod.rs src/exec/mod.rs src/expand/param.rs
git commit -m "refactor(env): replace BgJob/Vec<BgJob> with JobTable in ShellEnv

Migrate exec_async, reap_zombies, builtin_wait, and $! expansion
to use the new JobTable."
```

---

### Task 8: Register fg/bg/jobs as Regular Builtins

**Files:**
- Modify: `src/builtin/mod.rs`

- [ ] **Step 1: Write failing test for builtin classification**

Append to `tests` module in `src/builtin/mod.rs`:

```rust
    #[test]
    fn test_classify_fg_bg_jobs() {
        assert!(matches!(classify_builtin("fg"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("bg"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("jobs"), BuiltinKind::Regular));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib builtin::tests::test_classify_fg_bg_jobs`
Expected: FAIL

- [ ] **Step 3: Add fg/bg/jobs to classify_builtin and exec_regular_builtin**

In `src/builtin/mod.rs`, update `classify_builtin`:

```rust
        "cd" | "echo" | "true" | "false" | "alias" | "unalias" | "kill" | "wait"
        | "fg" | "bg" | "jobs" => BuiltinKind::Regular,
```

In `exec_regular_builtin`, add stubs that delegate to Executor (like `wait`):

```rust
        "fg" | "bg" | "jobs" => {
            // Handled in Executor::exec_simple_command — needs Executor access
            eprintln!("kish: {}: internal error", name);
            1
        }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib builtin::tests -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/builtin/mod.rs
git commit -m "feat(builtin): register fg/bg/jobs as regular builtins"
```

---

### Task 9: Implement fg/bg/jobs Builtins in Executor

**Files:**
- Modify: `src/exec/mod.rs`

- [ ] **Step 1: Add jobs/fg/bg dispatch in exec_simple_command**

In `src/exec/mod.rs`, after the `wait` special-case block (around line 460), add:

```rust
        // fg/bg/jobs need Executor access for job table + terminal control
        if command_name == "fg" || command_name == "bg" || command_name == "jobs" {
            let saved = self.apply_temp_assignments(&cmd.assignments);
            let mut redirect_state = RedirectState::new();
            if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                eprintln!("kish: {}", e);
                self.restore_assignments(saved);
                self.env.last_exit_status = 1;
                return 1;
            }
            let status = match command_name.as_str() {
                "fg" => self.builtin_fg(&args),
                "bg" => self.builtin_bg(&args),
                "jobs" => self.builtin_jobs(&args),
                _ => unreachable!(),
            };
            redirect_state.restore();
            self.restore_assignments(saved);
            self.env.last_exit_status = status;
            return status;
        }
```

- [ ] **Step 2: Implement builtin_jobs**

Add to `impl Executor` in `src/exec/mod.rs`:

```rust
    fn builtin_jobs(&mut self, args: &[String]) -> i32 {
        let long_format = args.contains(&"-l".to_string());
        let pgid_only = args.contains(&"-p".to_string());

        // Collect job IDs first to avoid borrow issues
        let job_ids: Vec<crate::env::jobs::JobId> = self.env.jobs.all_jobs().map(|j| j.id).collect();

        for id in &job_ids {
            if pgid_only {
                if let Some(job) = self.env.jobs.get(*id) {
                    println!("{}", job.pgid.as_raw());
                }
            } else if long_format {
                if let Some(line) = self.env.jobs.format_job_long(*id) {
                    println!("{}", line);
                }
            } else if let Some(line) = self.env.jobs.format_job(*id) {
                println!("{}", line);
            }
        }

        // Mark done/terminated jobs as notified
        let pending = self.env.jobs.pending_notifications();
        for id in pending {
            self.env.jobs.mark_notified(id);
        }

        0
    }
```

- [ ] **Step 3: Implement builtin_fg**

```rust
    fn builtin_fg(&mut self, args: &[String]) -> i32 {
        use crate::env::jobs::{self, JobStatus};

        if !self.env.options.monitor {
            eprintln!("kish: fg: no job control");
            return 1;
        }

        let job_id = if args.is_empty() {
            match self.env.jobs.current_id() {
                Some(id) => id,
                None => {
                    eprintln!("kish: fg: no current job");
                    return 1;
                }
            }
        } else {
            match self.env.jobs.resolve_job_spec(&args[0]) {
                Some(id) => id,
                None => {
                    eprintln!("kish: fg: {}: no such job", args[0]);
                    return 1;
                }
            }
        };

        let (pgid, command) = {
            let job = match self.env.jobs.get(job_id) {
                Some(j) => j,
                None => {
                    eprintln!("kish: fg: job not found");
                    return 1;
                }
            };
            (job.pgid, job.command.clone())
        };

        // Print the command being foregrounded
        eprintln!("{}", command);

        // Update job state
        if let Some(job) = self.env.jobs.get_mut(job_id) {
            job.foreground = true;
            if matches!(job.status, JobStatus::Stopped(_)) {
                job.status = JobStatus::Running;
            }
        }

        // Send SIGCONT to resume if stopped
        nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGCONT).ok();

        // Give terminal to the job
        jobs::give_terminal(pgid).ok();

        // Wait for the job
        let status = self.wait_for_foreground_job(job_id);

        // Take terminal back
        jobs::take_terminal(self.env.shell_pgid).ok();

        status
    }
```

- [ ] **Step 4: Implement builtin_bg**

```rust
    fn builtin_bg(&mut self, args: &[String]) -> i32 {
        use crate::env::jobs::JobStatus;

        if !self.env.options.monitor {
            eprintln!("kish: bg: no job control");
            return 1;
        }

        let job_id = if args.is_empty() {
            match self.env.jobs.current_id() {
                Some(id) => id,
                None => {
                    eprintln!("kish: bg: no current job");
                    return 1;
                }
            }
        } else {
            match self.env.jobs.resolve_job_spec(&args[0]) {
                Some(id) => id,
                None => {
                    eprintln!("kish: bg: {}: no such job", args[0]);
                    return 1;
                }
            }
        };

        let pgid = {
            let job = match self.env.jobs.get(job_id) {
                Some(j) => j,
                None => {
                    eprintln!("kish: bg: job not found");
                    return 1;
                }
            };
            if !matches!(job.status, JobStatus::Stopped(_)) {
                eprintln!("kish: bg: job {} not stopped", job_id);
                return 1;
            }
            job.pgid
        };

        // Update job state
        if let Some(job) = self.env.jobs.get_mut(job_id) {
            job.status = JobStatus::Running;
            job.foreground = false;
            eprintln!("[{}]+ {} &", job.id, job.command);
        }

        // Send SIGCONT
        nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGCONT).ok();

        0
    }
```

- [ ] **Step 5: Implement wait_for_foreground_job**

```rust
    /// Wait for a foreground job to complete or stop.
    fn wait_for_foreground_job(&mut self, job_id: crate::env::jobs::JobId) -> i32 {
        use crate::env::jobs::JobStatus;
        use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};

        let pgid = match self.env.jobs.get(job_id) {
            Some(j) => j.pgid,
            None => return 1,
        };

        let mut last_status = 0;

        loop {
            match waitpid(nix::unistd::Pid::from_raw(-pgid.as_raw()), Some(WaitPidFlag::WUNTRACED)) {
                Ok(WaitStatus::Exited(pid, code)) => {
                    self.env.jobs.update_status(pid, JobStatus::Done(code));
                    last_status = code;
                    // Check if all processes in job are done
                    if self.all_job_processes_done(job_id) {
                        self.env.jobs.mark_notified(job_id);
                        self.env.jobs.remove_job(job_id);
                        break;
                    }
                }
                Ok(WaitStatus::Signaled(pid, sig, _)) => {
                    let code = 128 + sig as i32;
                    self.env.jobs.update_status(pid, JobStatus::Terminated(sig as i32));
                    last_status = code;
                    if self.all_job_processes_done(job_id) {
                        self.env.jobs.mark_notified(job_id);
                        self.env.jobs.remove_job(job_id);
                        break;
                    }
                }
                Ok(WaitStatus::Stopped(pid, sig)) => {
                    self.env.jobs.update_status(pid, JobStatus::Stopped(sig as i32));
                    // Job was stopped (e.g., Ctrl+Z)
                    if let Some(line) = self.env.jobs.format_job(job_id) {
                        eprintln!("{}", line);
                    }
                    last_status = 128 + sig as i32;
                    break;
                }
                Err(nix::errno::Errno::ECHILD) => {
                    // No more children in this process group
                    self.env.jobs.remove_job(job_id);
                    break;
                }
                Err(nix::errno::Errno::EINTR) => {
                    // Interrupted by signal — process it and continue waiting
                    self.process_pending_signals();
                    continue;
                }
                _ => break,
            }
        }

        last_status
    }

    /// Check if all processes in a job have finished (Done or Terminated).
    fn all_job_processes_done(&self, job_id: crate::env::jobs::JobId) -> bool {
        use crate::env::jobs::JobStatus;
        match self.env.jobs.get(job_id) {
            Some(job) => matches!(job.status, JobStatus::Done(_) | JobStatus::Terminated(_)),
            None => true,
        }
    }
```

- [ ] **Step 6: Add job notification display method**

```rust
    /// Display pending job notifications and clean up completed jobs.
    pub fn display_job_notifications(&mut self) {
        let pending = self.env.jobs.pending_notifications();
        for id in &pending {
            if let Some(line) = self.env.jobs.format_job(*id) {
                eprintln!("{}", line);
            }
            self.env.jobs.mark_notified(*id);
        }
        self.env.jobs.cleanup_notified();
    }
```

- [ ] **Step 7: Run cargo check**

Run: `cargo check`
Expected: Compiles

- [ ] **Step 8: Run full test suite**

Run: `cargo test`
Expected: All tests PASS

- [ ] **Step 9: Commit**

```bash
git add src/exec/mod.rs
git commit -m "feat(exec): implement fg/bg/jobs builtins with terminal control and job wait"
```

---

### Task 10: Pipeline Foreground Job Registration

**Files:**
- Modify: `src/exec/pipeline.rs`

- [ ] **Step 1: Update exec_multi_pipeline for monitor mode**

In `src/exec/pipeline.rs`, update `exec_multi_pipeline` to register foreground jobs and use terminal control when monitor mode is active.

Replace the parent wait loop (after `close_all_pipes(&pipes)`) with:

```rust
        // Parent: close all pipe fds
        close_all_pipes(&pipes);

        if self.env.options.monitor {
            // Register as foreground job
            let cmd_str = pipeline.commands.iter()
                .map(|c| format!("{:?}", c))  // simplified — improve later
                .collect::<Vec<_>>()
                .join(" | ");
            let job_id = self.env.jobs.add_job(pgid, children.clone(), cmd_str, true);

            // Give terminal to the pipeline
            crate::env::jobs::give_terminal(pgid).ok();

            // Wait with WUNTRACED
            let status = self.wait_for_foreground_pipeline(job_id, &children, n);

            // Take terminal back
            crate::env::jobs::take_terminal(self.env.shell_pgid).ok();

            if self.env.options.pipefail {
                // For pipefail, we need to check all statuses
                // The wait_for_foreground_pipeline already handles this
                status
            } else {
                status
            }
        } else {
            // Non-monitor mode: existing behavior
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

- [ ] **Step 2: Add wait_for_foreground_pipeline method**

Add to `impl Executor` in `src/exec/pipeline.rs`:

```rust
    /// Wait for a foreground pipeline job. Returns the exit status.
    fn wait_for_foreground_pipeline(&mut self, job_id: crate::env::jobs::JobId, children: &[Pid], n: usize) -> i32 {
        use crate::env::jobs::JobStatus;
        use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};

        let pgid = match self.env.jobs.get(job_id) {
            Some(j) => j.pgid,
            None => return 1,
        };

        let mut statuses = vec![0i32; n];
        let mut remaining = n;

        loop {
            if remaining == 0 {
                break;
            }

            match waitpid(Pid::from_raw(-pgid.as_raw()), Some(WaitPidFlag::WUNTRACED)) {
                Ok(WaitStatus::Exited(pid, code)) => {
                    if let Some(idx) = children.iter().position(|&c| c == pid) {
                        statuses[idx] = code;
                        remaining -= 1;
                    }
                    self.env.jobs.update_status(pid, JobStatus::Done(code));
                }
                Ok(WaitStatus::Signaled(pid, sig, _)) => {
                    let code = 128 + sig as i32;
                    if let Some(idx) = children.iter().position(|&c| c == pid) {
                        statuses[idx] = code;
                        remaining -= 1;
                    }
                    self.env.jobs.update_status(pid, JobStatus::Terminated(sig as i32));
                }
                Ok(WaitStatus::Stopped(pid, sig)) => {
                    // Pipeline was stopped — mark all as stopped
                    self.env.jobs.update_status(pid, JobStatus::Stopped(sig as i32));
                    if let Some(job) = self.env.jobs.get_mut(job_id) {
                        job.status = JobStatus::Stopped(sig as i32);
                        job.foreground = false;
                    }
                    if let Some(line) = self.env.jobs.format_job(job_id) {
                        eprintln!("{}", line);
                    }
                    return 128 + sig as i32;
                }
                Err(nix::errno::Errno::ECHILD) => break,
                Err(nix::errno::Errno::EINTR) => continue,
                _ => break,
            }
        }

        // All children done — remove job
        self.env.jobs.mark_notified(job_id);
        self.env.jobs.remove_job(job_id);

        if self.env.options.pipefail {
            statuses.iter().rev().find(|&&s| s != 0).copied().unwrap_or(0)
        } else {
            statuses.last().copied().unwrap_or(0)
        }
    }
```

- [ ] **Step 3: Update child signal setup for monitor mode**

In the `ForkResult::Child` branch of `exec_multi_pipeline`, update signal setup:

Replace:
```rust
                Ok(ForkResult::Child) => {
                    // Set process group
                    let my_pid = nix::unistd::getpid();
                    if i == 0 {
                        setpgid(my_pid, my_pid).ok();
                    } else {
                        setpgid(my_pid, pgid).ok();
                    }
                    let ignored = self.env.traps.ignored_signals();
                    self.env.traps.reset_non_ignored();
                    signal::reset_child_signals(&ignored);
```

With:
```rust
                Ok(ForkResult::Child) => {
                    // Set process group
                    let my_pid = nix::unistd::getpid();
                    if i == 0 {
                        setpgid(my_pid, my_pid).ok();
                    } else {
                        setpgid(my_pid, pgid).ok();
                    }
                    let ignored = self.env.traps.ignored_signals();
                    self.env.traps.reset_non_ignored();
                    if self.env.options.monitor {
                        signal::setup_foreground_child_signals(&ignored);
                    } else {
                        signal::reset_child_signals(&ignored);
                    }
```

- [ ] **Step 4: Run cargo check**

Run: `cargo check`
Expected: Compiles

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/exec/pipeline.rs
git commit -m "feat(pipeline): add foreground job registration and WUNTRACED wait for monitor mode"
```

---

### Task 11: Background Job Signal Setup for Monitor Mode

**Files:**
- Modify: `src/exec/mod.rs`

- [ ] **Step 1: Update exec_async child signal setup**

In `exec_async()`, update the child branch to use monitor-aware signal setup:

Replace:
```rust
            Ok(ForkResult::Child) => {
                let ignored = self.env.traps.ignored_signals();
                self.env.traps.reset_non_ignored();
                signal::reset_child_signals(&ignored);

                let pid = nix::unistd::getpid();
                nix::unistd::setpgid(pid, pid).ok();
```

With:
```rust
            Ok(ForkResult::Child) => {
                let ignored = self.env.traps.ignored_signals();
                self.env.traps.reset_non_ignored();

                let pid = nix::unistd::getpid();
                nix::unistd::setpgid(pid, pid).ok();

                if self.env.options.monitor {
                    signal::setup_background_child_signals(&ignored);
                } else {
                    signal::reset_child_signals(&ignored);
                }
```

- [ ] **Step 2: Run cargo check and tests**

Run: `cargo test`
Expected: All PASS

- [ ] **Step 3: Commit**

```bash
git add src/exec/mod.rs
git commit -m "feat(exec): use monitor-aware signal setup for background jobs"
```

---

### Task 12: Interactive Mode — Auto-enable Monitor and Job Notifications

**Files:**
- Modify: `src/interactive/mod.rs`

- [ ] **Step 1: Update Repl::new to enable monitor mode**

In `src/interactive/mod.rs`, update `Repl::new`:

```rust
    pub fn new(shell_name: String) -> Self {
        signal::init_signal_handling();
        let mut executor = Executor::new(shell_name, vec![]);
        executor.env.is_interactive = true;
        executor.env.options.monitor = true;
        // Set up job control signals
        signal::init_job_control_signals();
        // Ensure shell is in its own process group and has terminal
        let shell_pgid = nix::unistd::getpgrp();
        executor.env.shell_pgid = shell_pgid;
        // Put shell in foreground
        crate::env::jobs::take_terminal(shell_pgid).ok();
        Self {
            executor,
            line_editor: LineEditor::new(),
        }
    }
```

- [ ] **Step 2: Add job notification display before prompt**

In the `Repl::run()` loop, add notification display before showing the prompt. Add after the `loop {` line:

```rust
            // Display job notifications before prompt
            self.executor.reap_zombies();
            self.executor.display_job_notifications();
```

Note: `reap_zombies` is currently private. Make it `pub(crate)` in `src/exec/mod.rs`.

- [ ] **Step 3: Make reap_zombies pub(crate)**

In `src/exec/mod.rs`, change:
```rust
    fn reap_zombies(&mut self) {
```
to:
```rust
    pub(crate) fn reap_zombies(&mut self) {
```

- [ ] **Step 4: Add -b notify support in exec_complete_command**

In `src/exec/mod.rs`, in `exec_complete_command`, after `self.reap_zombies();`, add:

```rust
        // -b flag: immediate job notification
        if self.env.options.notify {
            self.display_job_notifications();
        }
```

- [ ] **Step 5: Run cargo check and tests**

Run: `cargo test`
Expected: All PASS

- [ ] **Step 6: Commit**

```bash
git add src/interactive/mod.rs src/exec/mod.rs
git commit -m "feat(interactive): auto-enable monitor mode and job notifications in REPL"
```

---

### Task 13: E2E Tests for Job Control

**Files:**
- Create: `e2e/builtin/jobs_basic.sh`
- Create: `e2e/builtin/fg_no_monitor.sh`
- Create: `e2e/builtin/bg_no_monitor.sh`
- Create: `e2e/builtin/jobs_background.sh`

- [ ] **Step 1: Create jobs_basic test**

```bash
#!/bin/sh
# POSIX_REF: 2.11 Job Control
# DESCRIPTION: jobs builtin lists background jobs
# EXPECT_EXIT: 0
# EXPECT_STDERR_CONTAINS: [1]
sleep 0.1 &
jobs
wait
```

Write to `e2e/builtin/jobs_basic.sh`.

- [ ] **Step 2: Create fg_no_monitor test**

```bash
#!/bin/sh
# POSIX_REF: 2.11 Job Control
# DESCRIPTION: fg errors when monitor mode disabled (scripts)
# EXPECT_EXIT: 1
# EXPECT_STDERR_CONTAINS: no job control
fg
```

Write to `e2e/builtin/fg_no_monitor.sh`.

- [ ] **Step 3: Create bg_no_monitor test**

```bash
#!/bin/sh
# POSIX_REF: 2.11 Job Control
# DESCRIPTION: bg errors when monitor mode disabled (scripts)
# EXPECT_EXIT: 1
# EXPECT_STDERR_CONTAINS: no job control
bg
```

Write to `e2e/builtin/bg_no_monitor.sh`.

- [ ] **Step 4: Create background job tracking test**

```bash
#!/bin/sh
# POSIX_REF: 2.11 Job Control
# DESCRIPTION: background job is tracked with job number and PID
# EXPECT_EXIT: 0
# EXPECT_STDERR_MATCH: ^\[1\] [0-9]+$
sleep 0.1 &
wait
```

Write to `e2e/builtin/jobs_background.sh`.

- [ ] **Step 5: Build and run E2E tests**

Run: `cargo build && sh e2e/run_tests.sh --filter=builtin/jobs --verbose && sh e2e/run_tests.sh --filter=builtin/fg_no --verbose && sh e2e/run_tests.sh --filter=builtin/bg_no --verbose`
Expected: Tests PASS

- [ ] **Step 6: Commit**

```bash
git add e2e/builtin/jobs_basic.sh e2e/builtin/fg_no_monitor.sh e2e/builtin/bg_no_monitor.sh e2e/builtin/jobs_background.sh
git commit -m "test(e2e): add job control E2E tests for jobs, fg, bg builtins"
```

---

### Task 14: Update TODO.md

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Remove resolved Phase 6 limitations from TODO.md**

Remove these lines from `## Phase 6: Known Limitations`:

```
- [ ] `-m` (monitor) flag is settable but job control is not implemented — deferred to future phase
- [ ] `-b` (notify) flag is settable but has no effect — depends on `-m`
```

Also remove these same items from `## Phase 7: Known Limitations` (they were duplicated there).

Update the `## Future: Interactive Mode Enhancements` section — remove the `Job control` line since it's now implemented:

Remove:
```
- [ ] Job control — `-m` flag, fg/bg/jobs builtins, process group management, SIGTSTP/SIGCONT
```

Add under a new section `## Job Control: Known Limitations`:

```
- [ ] `%string` / `%?string` job specifiers — prefix/substring matching not implemented
- [ ] `disown` builtin — not implemented (non-POSIX extension)
- [ ] `suspend` builtin — not implemented
- [ ] Terminal state save/restore (tcgetattr/tcsetattr) — jobs that modify terminal settings may leave terminal in bad state
- [ ] Pipeline command display in `jobs` output uses debug format — improve to reconstruct shell syntax
```

- [ ] **Step 2: Run full test suite for final verification**

Run: `cargo test`
Expected: All tests PASS

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "docs: update TODO.md — resolve Phase 6 job control limitations, add new known limitations"
```

---

### Task 15: Final Integration Test and Verification

**Files:**
- All modified files

- [ ] **Step 1: Run full unit test suite**

Run: `cargo test --lib`
Expected: All tests PASS

- [ ] **Step 2: Run integration tests**

Run: `cargo test --test '*'`
Expected: All tests PASS

- [ ] **Step 3: Run E2E tests**

Run: `cargo build && sh e2e/run_tests.sh`
Expected: All tests PASS (or known XFAILs only)

- [ ] **Step 4: Manual smoke test (if TTY available)**

Start `./target/debug/kish` interactively and verify:
1. `sleep 10 &` → shows `[1] <pid>`
2. `jobs` → shows `[1]+ Running  sleep 10`
3. `fg` → brings sleep to foreground
4. `Ctrl+Z` → stops the job, shows `Stopped`
5. `bg` → resumes in background
6. `set +m; fg` → shows "no job control"
7. `set -b` → background completion notification appears immediately

- [ ] **Step 5: Final commit if any fixes needed**

If fixes were needed during verification, commit them:
```bash
git add -A
git commit -m "fix: address issues found during final job control verification"
```
