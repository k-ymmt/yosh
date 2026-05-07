# SP2 — `src/env/jobs.rs` Responsibility Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split `src/env/jobs.rs` (1118 lines) into a `src/env/jobs/` module with six responsibility-focused submodules; centralize the notification state machine in named predicates; introduce `Display for JobStatus` and `JobStatus::is_terminal()`; preserve every public API used by `exec/`, `interactive/`, and external crates.

**Architecture:** `src/env/jobs/mod.rs` becomes a thin facade defining `JobTable` and re-exporting items from five submodules (`model`, `spec`, `notification`, `format`, `terminal`). Each submodule adds its topical methods to `JobTable` via `impl super::JobTable { ... }` blocks. Method call sites do not change — the facade re-exports preserve `crate::env::jobs::*` paths.

**Tech Stack:** Rust 2024 edition, nix 0.27 (`tcsetpgrp`, termios), libc (zeroed termios for tests), criterion (bench compile only).

**Reference Documents:**
- Spec: `docs/superpowers/specs/2026-05-06-sp2-env-jobs-redesign-design.md`
- Umbrella: `docs/superpowers/specs/2026-05-06-large-file-redesign-umbrella-design.md`
- Predecessor plan (SP1): `docs/superpowers/plans/2026-05-06-sp1-plugin-host-redesign-plan.md`

**Line-count Target:** Per umbrella DoD #6, each production file ≤ 400 lines (no documented exception expected for SP2). Spec's stricter ≤ 250 target is an aspiration, not a gate.

**Definition of Done (per umbrella):**
1. `cargo test` PASS (lib + integration).
2. `./e2e/run_tests.sh` PASS (full).
3. `cargo bench --no-run` PASS.
4. `cargo clippy --all-targets -- -D warnings` — only the two pre-existing `doc_lazy_continuation` errors at `src/plugin/mod.rs:98-99` remain (out of scope for this umbrella).
5. `cargo fmt --check` PASS.
6. Each production file in `src/env/jobs/` ≤ 400 lines.
7. README/CLAUDE.md/TODO.md references to `src/env/jobs.rs` still resolve (verified via grep).
8. Public API names and signatures preserved — no diffs in `exec/`, `interactive/`, or `bin/` import sites.

**SP2-Specific DoD Additions:**
9. `tests/pty_interactive.rs` PASS — `set -m` and `fg`/`bg` flows non-flaky over 3 consecutive runs.
10. `e2e/06-jobs/` suite PASS within `./e2e/run_tests.sh`.

---

## File Structure

After all tasks complete, `src/env/jobs/` looks like:

```
src/env/jobs/
  mod.rs          — JobTable struct, storage core (add_job, remove_job, get/get_mut,
                    update_status, find_by_pgid, last_bg_pid, all_jobs, current/previous
                    accessors, is_empty), submodule declarations, public re-exports.
                    Storage tests stay here.
  model.rs        — Job struct + impl, JobStatus enum, JobId type alias,
                    impl Display for JobStatus, JobStatus::is_terminal().
                    JobStatus equality tests, Job saved_tmodes tests, Display test,
                    is_terminal test.
  spec.rs         — JobSpec<'a> enum, JobSpecError enum, pub fn parse_job_spec,
                    impl super::JobTable { resolve_job_spec, resolve, resolve_by }.
                    All parse_* and resolve_* tests.
  notification.rs — impl super::JobTable { pending_notifications, mark_notified,
                    cleanup_notified }, pub(super) fn is_notifiable / is_cleanable /
                    reset_after_status_change.
                    pending_notifications and mark_notified tests, predicate tests.
  format.rs       — impl super::JobTable { format_job, format_job_long, indicator }.
                    format_job tests.
  terminal.rs     — pub fn give_terminal, pub fn take_terminal,
                    impl super::JobTable { set_shell_tmodes, shell_tmodes }.
                    Terminal tests (signature + accessor defaults).
```

**`mod.rs` re-exports** (preserve `crate::env::jobs::*` paths):

```rust
pub use model::{Job, JobId, JobStatus};
pub use spec::{JobSpec, JobSpecError, parse_job_spec};
pub use terminal::{give_terminal, take_terminal};
```

`JobTable` is defined in `mod.rs` itself (so `crate::env::jobs::JobTable` resolves naturally without re-export).

**Visibility rules:**
- `JobTable` fields stay private to `mod.rs`. Submodules access them through Rust's parent-private rule (a child module can read a parent's private items).
- Methods on `JobTable` defined in submodules use `pub fn` (matches existing public API).
- Internal helpers (e.g., `indicator`, `resolve_by`, predicates) use `pub(super)` or `fn` (private).

---

## Task 0: Pre-flight Verification

**Goal:** Confirm baseline build/test state before starting; record current line counts and test counts so deviations are visible during implementation.

**Files:** Read-only inspection. No code changes.

- [ ] **Step 1: Verify clean working tree**

Run:
```bash
git status
```
Expected: `nothing to commit, working tree clean` and current branch is `main`.

- [ ] **Step 2: Run full unit + integration test suite**

Run:
```bash
cargo test --lib --features test-helpers 2>&1 | tail -20
```
Expected: ends with `test result: ok. NNN passed; 0 failed; ...` (where NNN is the lib test count, currently 772).

- [ ] **Step 3: Run e2e suite**

Run:
```bash
./e2e/run_tests.sh 2>&1 | tail -5
```
Expected: `Passed: 393 / 393` (exact count may differ if tests were added; 0 failures is what matters).

- [ ] **Step 4: Confirm baseline file size**

Run:
```bash
wc -l src/env/jobs.rs
```
Expected: `1118 src/env/jobs.rs`.

- [ ] **Step 5: List external callers (read-only sanity check)**

Run:
```bash
rg -n "use crate::env::jobs|crate::env::jobs::" --type rust src/ | grep -v "^src/env/jobs"
```
Expected: All hits use `crate::env::jobs::JobTable | parse_job_spec | give_terminal | take_terminal | JobStatus | JobSpecError | JobId`. No hits reference internal helpers like `format_status`, `indicator`, or `resolve_by`. **If any hit references an internal helper, STOP and escalate** — that helper would need to remain `pub`.

- [ ] **Step 6: Confirm bench compile baseline**

Run:
```bash
cargo bench --no-run 2>&1 | tail -3
```
Expected: `Finished` (no compile errors).

- [ ] **Step 7: No commit for this task**

Pre-flight is informational only. Proceed to Task A1.

---

## Task A1: Convert `jobs.rs` to `jobs/mod.rs` (Pure Rename)

**Goal:** Move file into a module directory without changing a single byte of content. This is the smallest possible step — mechanical rename, then verify nothing broke.

**Files:**
- Create: `src/env/jobs/` (directory)
- Move: `src/env/jobs.rs` → `src/env/jobs/mod.rs`

- [ ] **Step 1: Create the target directory and move the file**

Run:
```bash
mkdir -p src/env/jobs && git mv src/env/jobs.rs src/env/jobs/mod.rs
```
Expected: success, no output. The git history of the file is preserved through `git mv`.

- [ ] **Step 2: Verify cargo build still works**

Run:
```bash
cargo build 2>&1 | tail -3
```
Expected: `Finished` (no errors). Rust treats `src/env/jobs/mod.rs` as the module file for `pub mod jobs;` declared in `src/env/mod.rs:4`.

- [ ] **Step 3: Verify all tests still pass**

Run:
```bash
cargo test --lib 2>&1 | tail -3
```
Expected: `test result: ok. NNN passed; 0 failed`.

- [ ] **Step 4: Confirm line count moved (not duplicated)**

Run:
```bash
wc -l src/env/jobs/mod.rs && ls src/env/ | grep jobs
```
Expected: `1118 src/env/jobs/mod.rs` and the `ls` shows `jobs` directory only (no `jobs.rs`).

- [ ] **Step 5: Verify caller paths unaffected**

Run:
```bash
cargo build 2>&1 | grep -E "error|warning" | head -5
```
Expected: zero errors, zero warnings (or only pre-existing warnings unrelated to env::jobs).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(env/jobs): convert jobs.rs to jobs/mod.rs

Pure rename — first step of SP2. No content changes.
File becomes the module file for `pub mod jobs;` declared in
`src/env/mod.rs`, enabling future per-responsibility submodules.

Original SP2 prompt: split src/env/jobs.rs into model / spec /
notification / format / terminal submodules per
docs/superpowers/specs/2026-05-06-sp2-env-jobs-redesign-design.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
Expected: commit succeeds.

---

## Task A2: Extract `model.rs` (Job, JobStatus, JobId)

**Goal:** Move the data types `Job`, `JobStatus`, `JobId` and their immediate impls to `src/env/jobs/model.rs`. Re-export from `mod.rs` so `crate::env::jobs::Job`, `crate::env::jobs::JobStatus`, and `crate::env::jobs::JobId` continue to resolve. Move the JobStatus equality test and the Job saved_tmodes tests with their target.

**Files:**
- Create: `src/env/jobs/model.rs`
- Modify: `src/env/jobs/mod.rs` (delete the moved items, add `mod model;` and `pub use model::*;`)

- [ ] **Step 1: Create `src/env/jobs/model.rs` with the moved content**

Create the file with this exact content:

```rust
//! Core data types for the job table: `Job`, `JobStatus`, and `JobId`.
//!
//! These types are observed by `exec/job_control`, `exec/control`, and
//! the `jobs` builtin. Public-API names and signatures are preserved.

pub type JobId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Stopped(i32),    // signal number (e.g. SIGTSTP=20)
    Done(i32),       // exit code
    Terminated(i32), // killed by signal number
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: JobId,
    pub pgid: nix::unistd::Pid,
    pub pids: Vec<nix::unistd::Pid>,
    pub command: String,
    pub status: JobStatus,
    pub notified: bool,
    pub foreground: bool,
    /// Termios snapshot captured when the job last stopped (SIGTSTP/SIGSTOP).
    /// Used as the restore target on `fg`. `None` for jobs that have never
    /// been stopped, or on non-interactive / non-monitor shell modes.
    pub(super) saved_tmodes: Option<nix::sys::termios::Termios>,
}

impl Job {
    /// Termios snapshot captured the last time this job stopped
    /// (SIGTSTP/SIGSTOP), or `None` if it has never stopped or capture was
    /// unavailable (non-interactive/non-monitor or stdin not a TTY).
    pub fn saved_tmodes(&self) -> Option<&nix::sys::termios::Termios> {
        self.saved_tmodes.as_ref()
    }

    /// Replace the saved termios snapshot. Intended only for the
    /// `WaitStatus::Stopped` branch of foreground-wait — passing `None`
    /// is valid and clears any previously stored value, which is what
    /// the GNU libc manual job-control pattern requires after a
    /// mid-session `exec 0</dev/null` redirects stdin away from the TTY.
    pub fn set_saved_tmodes(&mut self, t: Option<nix::sys::termios::Termios>) {
        self.saved_tmodes = t;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_status_equality() {
        assert_eq!(JobStatus::Running, JobStatus::Running);
        assert_eq!(JobStatus::Done(0), JobStatus::Done(0));
        assert_ne!(JobStatus::Done(0), JobStatus::Done(1));
        assert_eq!(JobStatus::Stopped(20), JobStatus::Stopped(20));
        assert_eq!(JobStatus::Terminated(9), JobStatus::Terminated(9));
    }
}
```

**Note:** The `saved_tmodes` field is now `pub(super)` (was private). This is required because `add_job` lives in `mod.rs` (parent module), which constructs `Job` and would otherwise be unable to set the field. Rust's child-private rule allows the parent to read child-private items — but `pub(super)` on a child-module field exposes it to the parent explicitly. Equivalent visibility result, slightly clearer intent.

- [ ] **Step 2: Modify `src/env/jobs/mod.rs` — remove moved items, add module declaration and re-exports**

In `src/env/jobs/mod.rs`, delete these regions:

1. **Lines 6-55** (the entire `JobId` / `JobStatus` / `Job` / `impl Job` block, from `pub type JobId = u32;` through the end of `impl Job { ... }`).
2. **The single test `test_job_status_equality`** in `#[cfg(test)] mod tests` (currently lines 547-553 — the test that just compares enum variants).

Then add at the **top** of `src/env/jobs/mod.rs` (just below the `use` statements):

```rust
mod model;

pub use model::{Job, JobId, JobStatus};
```

After both edits, the top of `mod.rs` should look like:

```rust
use nix::unistd::Pid;
use std::collections::HashMap;
use std::os::fd::BorrowedFd;
use std::os::unix::io::RawFd;

mod model;

pub use model::{Job, JobId, JobStatus};

// ---------------------------------------------------------------------------
// JobSpec (POSIX §3.204 Job Control Job ID)
// ---------------------------------------------------------------------------
```

(The `JobSpec` block is still in `mod.rs` at this point — it moves in Task B1.)

- [ ] **Step 3: Move the Job saved_tmodes tests to `model.rs`**

The two tests `test_job_saved_tmodes_defaults_none` and `test_job_set_saved_tmodes_overwrites_with_none` exercise `Job::saved_tmodes()` and `Job::set_saved_tmodes()` (Job-level state). The spec assigns them to `model.rs`.

Cut them from `mod.rs::tests` (currently at lines 511-519 and 1093-1117) and append them to `model.rs::tests`. Each test calls `JobTable::default()` and `add_job` — these are in `super::super::JobTable` from inside `model::tests`.

The `model.rs::tests` block becomes:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::jobs::JobTable;
    use nix::unistd::Pid;

    fn pid(n: i32) -> Pid {
        Pid::from_raw(n)
    }

    #[test]
    fn test_job_status_equality() {
        assert_eq!(JobStatus::Running, JobStatus::Running);
        assert_eq!(JobStatus::Done(0), JobStatus::Done(0));
        assert_ne!(JobStatus::Done(0), JobStatus::Done(1));
        assert_eq!(JobStatus::Stopped(20), JobStatus::Stopped(20));
        assert_eq!(JobStatus::Terminated(9), JobStatus::Terminated(9));
    }

    #[test]
    fn test_job_saved_tmodes_defaults_none() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(42), vec![pid(42)], "cmd", false);
        let job = table.get(id).expect("job should exist");
        assert!(
            job.saved_tmodes().is_none(),
            "saved_tmodes() should default to None on new job"
        );
    }

    #[test]
    fn test_job_set_saved_tmodes_overwrites_with_none() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(42), vec![pid(42)], "cmd", false);

        let zeroed: libc::termios = unsafe { std::mem::zeroed() };
        let t: nix::sys::termios::Termios = zeroed.into();

        table
            .get_mut(id)
            .expect("job should exist")
            .set_saved_tmodes(Some(t));
        assert!(
            table.get(id).unwrap().saved_tmodes().is_some(),
            "saved_tmodes() should return Some after set_saved_tmodes(Some(_))"
        );

        table
            .get_mut(id)
            .expect("job should exist")
            .set_saved_tmodes(None);
        assert!(
            table.get(id).unwrap().saved_tmodes().is_none(),
            "saved_tmodes() should return None after set_saved_tmodes(None)"
        );
    }
}
```

- [ ] **Step 4: Run lib tests**

Run:
```bash
cargo test --lib env::jobs 2>&1 | tail -15
```
Expected: all `env::jobs::*` tests pass, including the new `model::tests::*` paths. Look for `test result: ok. NN passed; 0 failed`.

- [ ] **Step 5: Run full lib + integration test suite**

Run:
```bash
cargo test --lib --features test-helpers 2>&1 | tail -3
```
Expected: `test result: ok. NNN passed; 0 failed`.

- [ ] **Step 6: Verify external callers compile (no API breakage)**

Run:
```bash
cargo build --bins 2>&1 | tail -3
```
Expected: `Finished` (no errors). The re-exports `pub use model::{Job, JobId, JobStatus};` preserve every external-caller path.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(env/jobs): extract Job, JobStatus, JobId to jobs/model.rs

SP2 PR-A step 2. Move the data types and their immediate impls
out of mod.rs. Mechanical relocation — no behavior change.

Re-exports in mod.rs preserve the public API path
crate::env::jobs::{Job, JobId, JobStatus}. Field saved_tmodes is
pub(super) (matches Rust's child-module visibility rule).

Tests covering JobStatus equality and Job saved_tmodes lifecycle
follow their target into model.rs.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
Expected: commit succeeds.

---

## Task A3: Extract `terminal.rs` (give/take_terminal + shell_tmodes accessors)

**Goal:** Move terminal-control items to `src/env/jobs/terminal.rs`. Three items move: the two free functions `give_terminal` / `take_terminal`, and the two `JobTable` methods `set_shell_tmodes` / `shell_tmodes`. Re-export the free functions; methods need no re-export (they live on `JobTable` via `impl super::JobTable`).

**Files:**
- Create: `src/env/jobs/terminal.rs`
- Modify: `src/env/jobs/mod.rs` (delete moved items, add `mod terminal;` and `pub use terminal::{give_terminal, take_terminal};`)

- [ ] **Step 1: Create `src/env/jobs/terminal.rs`**

Create the file with this content:

```rust
//! Terminal control for job-control mode.
//!
//! Wraps `tcsetpgrp(2)` for transferring terminal ownership between the
//! shell and foreground job process groups, and stores the shell's own
//! termios snapshot on `JobTable` so it can be restored after each
//! foreground wait completion.

use nix::unistd::Pid;
use std::os::fd::BorrowedFd;
use std::os::unix::io::RawFd;

const TERMINAL_FD: RawFd = 0;

/// Give the terminal to the specified process group.
pub fn give_terminal(pgid: Pid) -> Result<(), nix::Error> {
    // SAFETY: TERMINAL_FD (0) is stdin, which lives for the process lifetime.
    let fd = unsafe { BorrowedFd::borrow_raw(TERMINAL_FD) };
    nix::unistd::tcsetpgrp(fd, pgid)
}

/// Reclaim the terminal for the shell process group.
pub fn take_terminal(shell_pgid: Pid) -> Result<(), nix::Error> {
    // SAFETY: TERMINAL_FD (0) is stdin, which lives for the process lifetime.
    let fd = unsafe { BorrowedFd::borrow_raw(TERMINAL_FD) };
    nix::unistd::tcsetpgrp(fd, shell_pgid)
}

impl super::JobTable {
    /// Store the shell's termios snapshot. The interactive REPL calls
    /// this once at startup after `take_terminal`. Calling again
    /// overwrites the previous value; callers must not rely on this
    /// for re-initialization after fork.
    pub fn set_shell_tmodes(&mut self, t: nix::sys::termios::Termios) {
        self.shell_tmodes = Some(t);
    }

    /// Return the shell's snapshot of its termios, if one was captured.
    pub fn shell_tmodes(&self) -> Option<&nix::sys::termios::Termios> {
        self.shell_tmodes.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::jobs::JobTable;

    #[test]
    fn test_terminal_functions_compile() {
        // This test verifies the functions exist and have the correct
        // signatures.  We cannot actually call tcsetpgrp in a unit test
        // (no controlling terminal), so we just take function pointers.
        let _: fn(Pid) -> Result<(), nix::Error> = give_terminal;
        let _: fn(Pid) -> Result<(), nix::Error> = take_terminal;
    }

    #[test]
    fn test_job_table_shell_tmodes_defaults_none() {
        let table = JobTable::default();
        assert!(
            table.shell_tmodes().is_none(),
            "shell_tmodes should default to None on new JobTable"
        );
    }

    #[test]
    fn test_set_shell_tmodes_stores_value() {
        let mut table = JobTable::default();
        let zeroed: libc::termios = unsafe { std::mem::zeroed() };
        let t: nix::sys::termios::Termios = zeroed.into();
        table.set_shell_tmodes(t);
        assert!(
            table.shell_tmodes().is_some(),
            "shell_tmodes should hold the value after set_shell_tmodes"
        );
    }
}
```

**Note:** `JobTable.shell_tmodes` is currently a private field of a struct in `mod.rs`. The submodule `terminal.rs` accesses it through Rust's parent-private rule (parents see child-private items, but child modules ALSO see parent-private items because both are in the same crate's module hierarchy). Actually, more precisely: in Rust, a child module can access private items of its parent module. So `terminal.rs` (child of `mod.rs`) accessing `mod.rs`'s private field `shell_tmodes` is allowed.

- [ ] **Step 2: Remove moved items from `src/env/jobs/mod.rs`**

Delete these regions in `src/env/jobs/mod.rs`:

1. **Lines for `set_shell_tmodes` and `shell_tmodes`** inside `impl JobTable { ... }`. Currently around lines 153-160 — the two methods with their doc comments:
```rust
/// Store the shell's termios snapshot...
pub fn set_shell_tmodes(&mut self, t: nix::sys::termios::Termios) { ... }

/// Return the shell's snapshot of its termios...
pub fn shell_tmodes(&self) -> Option<&nix::sys::termios::Termios> { ... }
```

2. **The `Task 5: Terminal control` block** (currently lines 466-484) — the `const TERMINAL_FD`, `give_terminal`, and `take_terminal` definitions.

3. **Three tests** in `#[cfg(test)] mod tests`:
   - `test_terminal_functions_compile`
   - `test_job_table_shell_tmodes_defaults_none`
   - `test_set_shell_tmodes_stores_value`

- [ ] **Step 3: Add module declaration and re-exports to `src/env/jobs/mod.rs`**

In `src/env/jobs/mod.rs`, just below the existing `mod model;` / `pub use model::*;` block, add:

```rust
mod terminal;

pub use terminal::{give_terminal, take_terminal};
```

The top of `mod.rs` should now look like:

```rust
use nix::unistd::Pid;
use std::collections::HashMap;
// (the BorrowedFd and RawFd uses can be removed if unused — verify with cargo build)

mod model;
mod terminal;

pub use model::{Job, JobId, JobStatus};
pub use terminal::{give_terminal, take_terminal};
```

- [ ] **Step 4: Remove now-unused imports from `mod.rs`**

After moving the terminal items, `BorrowedFd` and `RawFd` are no longer used in `mod.rs`. Run:
```bash
cargo build 2>&1 | grep "unused import" | head -3
```
Expected: warnings about unused `BorrowedFd` / `RawFd` in `src/env/jobs/mod.rs`.

Remove these lines from the top of `src/env/jobs/mod.rs`:
```rust
use std::os::fd::BorrowedFd;
use std::os::unix::io::RawFd;
```

- [ ] **Step 5: Run lib tests**

Run:
```bash
cargo test --lib env::jobs 2>&1 | tail -10
```
Expected: all tests pass; `terminal::tests::*` and `model::tests::*` are listed.

- [ ] **Step 6: Run full test suite**

Run:
```bash
cargo test --lib --features test-helpers 2>&1 | tail -3
```
Expected: `test result: ok. NNN passed; 0 failed`.

- [ ] **Step 7: Verify external callers (give_terminal, take_terminal) still compile**

Run:
```bash
rg -n "crate::env::jobs::(give|take)_terminal" --type rust src/ | head -5
```
Expected: hits in `src/exec/pipeline.rs:128`, `pipeline.rs:132`, `src/interactive/mod.rs:48`. These resolve through the `pub use terminal::{give_terminal, take_terminal};` re-export in `mod.rs`.

Then:
```bash
cargo build 2>&1 | tail -3
```
Expected: `Finished`.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(env/jobs): extract terminal control to jobs/terminal.rs

SP2 PR-A step 3. Move give_terminal / take_terminal and the
shell_tmodes accessors out of mod.rs. Mechanical relocation —
no behavior change.

Re-exports preserve crate::env::jobs::{give_terminal, take_terminal};
methods on JobTable resolve through the impl super::JobTable block
in the new submodule.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
Expected: commit succeeds. **PR-A is now complete** (jobs/mod.rs, jobs/model.rs, jobs/terminal.rs).

---

## Task B1: Extract `spec.rs` (JobSpec, parse_job_spec, resolve_*)

**Goal:** Move job-specifier parsing and resolution to `src/env/jobs/spec.rs`. Five items move: types `JobSpec<'_>` and `JobSpecError`, the free fn `parse_job_spec`, and the three methods `resolve_job_spec` / `resolve` / `resolve_by`. Plus all the `parse_*` and `resolve_*` tests.

**Files:**
- Create: `src/env/jobs/spec.rs`
- Modify: `src/env/jobs/mod.rs` (delete moved items, add `mod spec;` and re-exports)

- [ ] **Step 1: Create `src/env/jobs/spec.rs`**

Create the file with this content:

```rust
//! POSIX §3.204 job-control job specifier parsing and resolution.
//!
//! `parse_job_spec` is a pure parser; `JobTable::resolve_job_spec` and
//! `JobTable::resolve` look the parsed spec up against the current
//! job table state. `resolve_by` is the shared scan-and-disambiguate
//! helper for Prefix/Substring matching.

use super::JobId;

/// Parsed form of a POSIX job specifier string such as `%%`, `%1`, `%vim`.
///
/// Borrows from the input string so parsing is zero-allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobSpec<'a> {
    /// `%%` or `%+` — current job
    Current,
    /// `%-` — previous job
    Previous,
    /// `%n` — job with numeric id
    Numeric(JobId),
    /// `%string` — command begins with string
    Prefix(&'a str),
    /// `%?string` — command contains string
    Substring(&'a str),
}

/// Error returned by `parse_job_spec` and `JobTable::resolve`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobSpecError {
    /// Input is not a syntactically valid job specifier.
    Malformed,
    /// Parse succeeded but no job matches the spec.
    NoSuchJob,
    /// A Prefix or Substring spec matched two or more jobs.
    Ambiguous,
}

/// Parse a POSIX job specifier string into a `JobSpec`.
///
/// Disambiguation order (earliest match wins):
/// 1. `"%%"` / `"%+"` → `Current`
/// 2. `"%-"` → `Previous`
/// 3. `"%<digits>"` with non-empty digit run → `Numeric`
/// 4. `"%?<rest>"` with non-empty `rest` → `Substring`
/// 5. `"%<rest>"` with non-empty `rest` → `Prefix`
/// 6. Otherwise → `Malformed`
pub fn parse_job_spec(s: &str) -> Result<JobSpec<'_>, JobSpecError> {
    let rest = s.strip_prefix('%').ok_or(JobSpecError::Malformed)?;

    match rest {
        "" => Err(JobSpecError::Malformed),
        "%" | "+" => Ok(JobSpec::Current),
        "-" => Ok(JobSpec::Previous),
        _ => {
            // Pure digit run → Numeric
            if rest.bytes().all(|b| b.is_ascii_digit()) {
                return rest
                    .parse::<JobId>()
                    .map(JobSpec::Numeric)
                    .map_err(|_| JobSpecError::Malformed);
            }

            // "?<rest>" → Substring
            if let Some(sub) = rest.strip_prefix('?') {
                if sub.is_empty() {
                    return Err(JobSpecError::Malformed);
                }
                return Ok(JobSpec::Substring(sub));
            }

            // Everything else with non-empty rest → Prefix
            Ok(JobSpec::Prefix(rest))
        }
    }
}

impl super::JobTable {
    /// Resolve a job specification string to a JobId.
    ///
    /// Supported forms (see `parse_job_spec` for syntax):
    /// - `%%` / `%+` — current job
    /// - `%-` — previous job
    /// - `%n` — job by numeric id
    /// - `%string` — command begins with string
    /// - `%?string` — command contains string
    ///
    /// Returns `Err(Ambiguous)` when a Prefix/Substring spec matches 2+ jobs.
    pub fn resolve_job_spec(&self, spec: &str) -> Result<JobId, JobSpecError> {
        self.resolve(parse_job_spec(spec)?)
    }

    /// Resolve a parsed `JobSpec` to a `JobId`.
    ///
    /// Matching is performed against `Job.command` (full command line),
    /// case-sensitive, across all job statuses (Running, Stopped, Done,
    /// Terminated) — bash-compatible.
    ///
    /// Returns:
    /// - `Ok(id)` if exactly one job matches
    /// - `Err(NoSuchJob)` if no job matches
    /// - `Err(Ambiguous)` if two or more jobs match (Prefix/Substring only)
    pub fn resolve(&self, spec: JobSpec<'_>) -> Result<JobId, JobSpecError> {
        match spec {
            JobSpec::Current => self.current.ok_or(JobSpecError::NoSuchJob),
            JobSpec::Previous => self.previous.ok_or(JobSpecError::NoSuchJob),
            JobSpec::Numeric(n) => {
                if self.jobs.contains_key(&n) {
                    Ok(n)
                } else {
                    Err(JobSpecError::NoSuchJob)
                }
            }
            JobSpec::Prefix(s) => self.resolve_by(|cmd| cmd.starts_with(s)),
            JobSpec::Substring(s) => self.resolve_by(|cmd| cmd.contains(s)),
        }
    }

    /// Internal helper: scan all jobs and collapse match count to a Result.
    fn resolve_by<F>(&self, mut pred: F) -> Result<JobId, JobSpecError>
    where
        F: FnMut(&str) -> bool,
    {
        let mut matched: Option<JobId> = None;
        for job in self.jobs.values() {
            if pred(&job.command) {
                if matched.is_some() {
                    return Err(JobSpecError::Ambiguous);
                }
                matched = Some(job.id);
            }
        }
        matched.ok_or(JobSpecError::NoSuchJob)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::jobs::JobTable;
    use nix::unistd::Pid;

    fn pid(n: i32) -> Pid {
        Pid::from_raw(n)
    }

    // -----------------------------------------------------------------------
    // resolve_job_spec
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_job_spec_numeric() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "x", false);
        assert_eq!(table.resolve_job_spec("%1"), Ok(id));
    }

    #[test]
    fn test_resolve_job_spec_percent_percent() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "x", false);
        assert_eq!(table.resolve_job_spec("%%"), Ok(id));
    }

    #[test]
    fn test_resolve_job_spec_plus() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "x", false);
        assert_eq!(table.resolve_job_spec("%+"), Ok(id));
    }

    #[test]
    fn test_resolve_job_spec_minus() {
        let mut table = JobTable::default();
        let id1 = table.add_job(pid(1), vec![pid(1)], "a", false);
        let _id2 = table.add_job(pid(2), vec![pid(2)], "b", false);
        assert_eq!(table.resolve_job_spec("%-"), Ok(id1));
    }

    #[test]
    fn test_resolve_job_spec_invalid() {
        let table = JobTable::default();
        // "%99" — syntactically valid Numeric(99) but no such job
        assert_eq!(table.resolve_job_spec("%99"), Err(JobSpecError::NoSuchJob));
        // "foo" — doesn't start with '%'
        assert_eq!(table.resolve_job_spec("foo"), Err(JobSpecError::Malformed));
        // "%abc" — Prefix("abc") against empty table → NoSuchJob (previously Malformed)
        assert_eq!(table.resolve_job_spec("%abc"), Err(JobSpecError::NoSuchJob));
    }

    #[test]
    fn test_resolve_job_spec_ambiguous() {
        let mut table = JobTable::default();
        table.add_job(pid(1), vec![pid(1)], "sleep 10", false);
        table.add_job(pid(2), vec![pid(2)], "sleep 20", false);
        assert_eq!(
            table.resolve_job_spec("%sleep"),
            Err(JobSpecError::Ambiguous)
        );
    }

    // -----------------------------------------------------------------------
    // parse_job_spec
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_current_percent() {
        assert_eq!(parse_job_spec("%%"), Ok(JobSpec::Current));
    }

    #[test]
    fn test_parse_current_plus() {
        assert_eq!(parse_job_spec("%+"), Ok(JobSpec::Current));
    }

    #[test]
    fn test_parse_previous() {
        assert_eq!(parse_job_spec("%-"), Ok(JobSpec::Previous));
    }

    #[test]
    fn test_parse_numeric() {
        assert_eq!(parse_job_spec("%1"), Ok(JobSpec::Numeric(1)));
        assert_eq!(parse_job_spec("%42"), Ok(JobSpec::Numeric(42)));
    }

    #[test]
    fn test_parse_numeric_overflow() {
        assert_eq!(
            parse_job_spec("%99999999999999999999"),
            Err(JobSpecError::Malformed)
        );
    }

    #[test]
    fn test_parse_prefix() {
        assert_eq!(parse_job_spec("%foo"), Ok(JobSpec::Prefix("foo")));
        assert_eq!(parse_job_spec("%vim"), Ok(JobSpec::Prefix("vim")));
    }

    #[test]
    fn test_parse_substring() {
        assert_eq!(parse_job_spec("%?bar"), Ok(JobSpec::Substring("bar")));
        assert_eq!(parse_job_spec("%?READ"), Ok(JobSpec::Substring("READ")));
    }

    #[test]
    fn test_parse_prefix_hyphen() {
        // "%-foo" is NOT %- followed by "foo" — it is a Prefix("-foo")
        assert_eq!(parse_job_spec("%-foo"), Ok(JobSpec::Prefix("-foo")));
    }

    #[test]
    fn test_parse_prefix_double_percent() {
        // "%%foo" is NOT Current followed by "foo" — it is Prefix("%foo")
        assert_eq!(parse_job_spec("%%foo"), Ok(JobSpec::Prefix("%foo")));
    }

    #[test]
    fn test_parse_malformed_empty() {
        assert_eq!(parse_job_spec(""), Err(JobSpecError::Malformed));
    }

    #[test]
    fn test_parse_malformed_bare_percent() {
        assert_eq!(parse_job_spec("%"), Err(JobSpecError::Malformed));
    }

    #[test]
    fn test_parse_malformed_bare_question() {
        assert_eq!(parse_job_spec("%?"), Err(JobSpecError::Malformed));
    }

    #[test]
    fn test_parse_malformed_no_percent() {
        assert_eq!(parse_job_spec("foo"), Err(JobSpecError::Malformed));
        assert_eq!(parse_job_spec("1"), Err(JobSpecError::Malformed));
    }

    // -----------------------------------------------------------------------
    // JobTable::resolve
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_current() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "x", false);
        assert_eq!(table.resolve(JobSpec::Current), Ok(id));
    }

    #[test]
    fn test_resolve_current_unset() {
        let table = JobTable::default();
        assert_eq!(
            table.resolve(JobSpec::Current),
            Err(JobSpecError::NoSuchJob)
        );
    }

    #[test]
    fn test_resolve_previous() {
        let mut table = JobTable::default();
        let id1 = table.add_job(pid(1), vec![pid(1)], "a", false);
        let _id2 = table.add_job(pid(2), vec![pid(2)], "b", false);
        assert_eq!(table.resolve(JobSpec::Previous), Ok(id1));
    }

    #[test]
    fn test_resolve_previous_unset() {
        let mut table = JobTable::default();
        let _id = table.add_job(pid(1), vec![pid(1)], "a", false);
        // Only one job added — previous is unset
        assert_eq!(
            table.resolve(JobSpec::Previous),
            Err(JobSpecError::NoSuchJob)
        );
    }

    #[test]
    fn test_resolve_numeric_hit() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "x", false);
        assert_eq!(table.resolve(JobSpec::Numeric(id)), Ok(id));
    }

    #[test]
    fn test_resolve_numeric_miss() {
        let table = JobTable::default();
        assert_eq!(
            table.resolve(JobSpec::Numeric(99)),
            Err(JobSpecError::NoSuchJob)
        );
    }

    #[test]
    fn test_resolve_prefix_single() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "vim README.md", false);
        table.add_job(pid(2), vec![pid(2)], "sleep 30", false);
        assert_eq!(table.resolve(JobSpec::Prefix("vim")), Ok(id));
    }

    #[test]
    fn test_resolve_prefix_none() {
        let mut table = JobTable::default();
        table.add_job(pid(1), vec![pid(1)], "sleep 30", false);
        assert_eq!(
            table.resolve(JobSpec::Prefix("vim")),
            Err(JobSpecError::NoSuchJob)
        );
    }

    #[test]
    fn test_resolve_prefix_ambiguous() {
        let mut table = JobTable::default();
        table.add_job(pid(1), vec![pid(1)], "sleep 10", false);
        table.add_job(pid(2), vec![pid(2)], "sleep 20", false);
        assert_eq!(
            table.resolve(JobSpec::Prefix("sleep")),
            Err(JobSpecError::Ambiguous)
        );
    }

    #[test]
    fn test_resolve_substring_single() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "vim README.md", false);
        table.add_job(pid(2), vec![pid(2)], "sleep 30", false);
        assert_eq!(table.resolve(JobSpec::Substring("EADME")), Ok(id));
    }

    #[test]
    fn test_resolve_substring_none() {
        let mut table = JobTable::default();
        table.add_job(pid(1), vec![pid(1)], "sleep 30", false);
        assert_eq!(
            table.resolve(JobSpec::Substring("vim")),
            Err(JobSpecError::NoSuchJob)
        );
    }

    #[test]
    fn test_resolve_substring_ambiguous() {
        let mut table = JobTable::default();
        table.add_job(pid(1), vec![pid(1)], "cat foo", false);
        table.add_job(pid(2), vec![pid(2)], "grep foo", false);
        assert_eq!(
            table.resolve(JobSpec::Substring("foo")),
            Err(JobSpecError::Ambiguous)
        );
    }

    #[test]
    fn test_resolve_prefix_matches_done_job() {
        // bash-compatible: Prefix matches all statuses, including Done
        use crate::env::jobs::JobStatus;
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "vim foo", false);
        if let Some(job) = table.get_mut(id) {
            job.status = JobStatus::Done(0);
        }
        assert_eq!(table.resolve(JobSpec::Prefix("vim")), Ok(id));
    }
}
```

- [ ] **Step 2: Remove moved items from `src/env/jobs/mod.rs`**

In `src/env/jobs/mod.rs`, delete:

1. The `JobSpec` enum, `JobSpecError` enum, and `parse_job_spec` function (currently the block from `// JobSpec (POSIX §3.204 ...)` through end of `parse_job_spec`).

2. Inside `impl JobTable { ... }`, the three methods:
   - `pub fn resolve_job_spec`
   - `pub fn resolve`
   - `fn resolve_by<F>` (private helper)

3. From `#[cfg(test)] mod tests`, delete all the tests now in `spec.rs::tests`:
   - `test_resolve_job_spec_*` (5 tests)
   - `test_parse_*` (12 tests)
   - `test_resolve_current`, `test_resolve_current_unset`, `test_resolve_previous`, `test_resolve_previous_unset`, `test_resolve_numeric_hit`, `test_resolve_numeric_miss`, `test_resolve_prefix_single`, `test_resolve_prefix_none`, `test_resolve_prefix_ambiguous`, `test_resolve_substring_single`, `test_resolve_substring_none`, `test_resolve_substring_ambiguous`, `test_resolve_prefix_matches_done_job` (13 tests)

That is 30 tests total (5 + 12 + 13). After this deletion, only storage/state-machine tests remain in `mod.rs::tests`.

- [ ] **Step 3: Add module declaration and re-exports in `mod.rs`**

In `src/env/jobs/mod.rs`, just below the existing `mod model; mod terminal;` block, add:

```rust
mod spec;

pub use spec::{JobSpec, JobSpecError, parse_job_spec};
```

The top of `mod.rs` should now be:

```rust
use nix::unistd::Pid;
use std::collections::HashMap;

mod model;
mod spec;
mod terminal;

pub use model::{Job, JobId, JobStatus};
pub use spec::{JobSpec, JobSpecError, parse_job_spec};
pub use terminal::{give_terminal, take_terminal};
```

(`mod` declarations alphabetized; `pub use` alphabetized.)

- [ ] **Step 4: Run lib tests scoped to env::jobs**

Run:
```bash
cargo test --lib env::jobs 2>&1 | tail -10
```
Expected: all tests pass; you should see entries like `env::jobs::spec::tests::test_parse_current_percent`.

- [ ] **Step 5: Run full lib + integration suite**

Run:
```bash
cargo test --lib --features test-helpers 2>&1 | tail -3
```
Expected: `test result: ok. NNN passed; 0 failed`.

- [ ] **Step 6: Verify external callers (if any) still compile**

Run:
```bash
rg -n "JobSpec|JobSpecError|parse_job_spec" --type rust src/ | grep -v "src/env/jobs/" | head -10
```
Expected: hits in `src/exec/job_control.rs` (using `JobSpecError`). Confirm:

```bash
cargo build 2>&1 | tail -3
```
Expected: `Finished`.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(env/jobs): extract job-spec parsing/resolution to jobs/spec.rs

SP2 PR-B. Move JobSpec, JobSpecError, parse_job_spec, and the three
JobTable resolve_* methods (incl. private resolve_by helper) out of
mod.rs. Mechanical relocation — no behavior change.

Re-exports preserve crate::env::jobs::{JobSpec, JobSpecError,
parse_job_spec}. Methods on JobTable resolve through the impl
super::JobTable block in the new submodule. 30 tests follow
their target.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
Expected: commit succeeds. **PR-B is now complete.**

---

## Task C1: Add `Display for JobStatus` (TDD)

**Goal:** Add `impl Display for JobStatus` to `model.rs` so `format!("{}", status)` produces the same string `format_status` produces today. Test-driven: write failing test first, implement, pass.

**Files:**
- Modify: `src/env/jobs/model.rs` (add `Display` impl + test)

- [ ] **Step 1: Write failing tests**

In `src/env/jobs/model.rs::tests`, append these tests (one per `JobStatus` variant):

```rust
#[test]
fn test_display_running() {
    assert_eq!(JobStatus::Running.to_string(), "Running");
}

#[test]
fn test_display_done_success() {
    // Done(0) renders as bare "Done" (no exit code)
    assert_eq!(JobStatus::Done(0).to_string(), "Done");
}

#[test]
fn test_display_done_nonzero() {
    assert_eq!(JobStatus::Done(2).to_string(), "Done(2)");
    assert_eq!(JobStatus::Done(127).to_string(), "Done(127)");
}

#[test]
fn test_display_stopped_known_signal() {
    // SIGTSTP = 20 on Linux/macOS — signal_number_to_name returns "TSTP"
    let s = JobStatus::Stopped(20).to_string();
    assert!(s.starts_with("Stopped(SIG"), "got: {}", s);
}

#[test]
fn test_display_stopped_unknown_signal() {
    // 99 is not a real signal — falls back to "UNKNOWN"
    assert_eq!(JobStatus::Stopped(99).to_string(), "Stopped(SIGUNKNOWN)");
}

#[test]
fn test_display_terminated_known_signal() {
    // SIGKILL = 9
    let s = JobStatus::Terminated(9).to_string();
    assert!(s.starts_with("Terminated(SIG"), "got: {}", s);
}

#[test]
fn test_display_terminated_unknown_signal() {
    assert_eq!(JobStatus::Terminated(99).to_string(), "Terminated(SIGUNKNOWN)");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:
```bash
cargo test --lib env::jobs::model::tests::test_display 2>&1 | tail -10
```
Expected: compile error — `the trait `Display` is not implemented for `JobStatus`` (or similar). At minimum, `JobStatus::Running.to_string()` will fail to compile.

- [ ] **Step 3: Add `impl Display for JobStatus` to `model.rs`**

In `src/env/jobs/model.rs`, after the `JobStatus` enum definition (and before the `Job` struct), add:

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

- [ ] **Step 4: Run tests to verify they pass**

Run:
```bash
cargo test --lib env::jobs::model::tests::test_display 2>&1 | tail -10
```
Expected: `test result: ok. 7 passed; 0 failed` (one per Display test).

- [ ] **Step 5: Run full test suite**

Run:
```bash
cargo test --lib --features test-helpers 2>&1 | tail -3
```
Expected: `test result: ok. NNN passed; 0 failed`. The original `format_status`-based tests in `mod.rs` still pass since `format_status` is still used by `format_job` / `format_job_long` (refactored in Task C3).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat(env/jobs): impl Display for JobStatus

SP2 PR-C step 1. Add Display impl to model.rs that produces the
exact same strings as JobTable::format_status.

This is a strict capability addition (callers can now use
format!("{}", status) where they would have failed to compile
before). format_status remains untouched — Task C3 refactors
format_job/format_job_long to use Display, then deletes
format_status.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
Expected: commit succeeds.

---

## Task C2: Add `JobStatus::is_terminal()` (TDD)

**Goal:** Add `is_terminal()` method to `JobStatus` returning `true` for `Done` / `Terminated` and `false` for `Running` / `Stopped`. This becomes the building block for `is_notifiable` / `is_cleanable` predicates in Task C5.

**Files:**
- Modify: `src/env/jobs/model.rs` (add method + tests)

- [ ] **Step 1: Write failing tests**

In `src/env/jobs/model.rs::tests`, append:

```rust
#[test]
fn test_is_terminal_running_false() {
    assert!(!JobStatus::Running.is_terminal());
}

#[test]
fn test_is_terminal_stopped_false() {
    // Stopped is paused, not finished — not terminal
    assert!(!JobStatus::Stopped(20).is_terminal());
}

#[test]
fn test_is_terminal_done_true() {
    assert!(JobStatus::Done(0).is_terminal());
    assert!(JobStatus::Done(127).is_terminal());
}

#[test]
fn test_is_terminal_terminated_true() {
    assert!(JobStatus::Terminated(9).is_terminal());
    assert!(JobStatus::Terminated(15).is_terminal());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:
```bash
cargo test --lib env::jobs::model::tests::test_is_terminal 2>&1 | tail -5
```
Expected: compile error — `no method named `is_terminal` found for enum `JobStatus``.

- [ ] **Step 3: Add `is_terminal` method to `JobStatus`**

In `src/env/jobs/model.rs`, just after the `Display` impl, add:

```rust
impl JobStatus {
    /// Done or Terminated — the job has finished and can be reaped.
    ///
    /// `Running` and `Stopped` (paused) are not terminal: a stopped job
    /// can resume via SIGCONT.
    pub fn is_terminal(self) -> bool {
        matches!(self, JobStatus::Done(_) | JobStatus::Terminated(_))
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run:
```bash
cargo test --lib env::jobs::model::tests::test_is_terminal 2>&1 | tail -5
```
Expected: `test result: ok. 4 passed; 0 failed`.

- [ ] **Step 5: Run full test suite**

Run:
```bash
cargo test --lib --features test-helpers 2>&1 | tail -3
```
Expected: `test result: ok. NNN passed; 0 failed`.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat(env/jobs): add JobStatus::is_terminal()

SP2 PR-C step 2. Add is_terminal() returning true for Done/Terminated
and false for Running/Stopped. Becomes the building block for
notification predicates in upcoming task.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
Expected: commit succeeds.

---

## Task C3: Refactor `format_job` / `format_job_long` to use `Display`, Delete `format_status`

**Goal:** Replace inline `self.format_status(job.status)` calls with `job.status` (relying on the new `Display` impl). Delete the now-unused private `format_status` method. The `format_job_*` methods stay in `mod.rs` for now; they move to `format.rs` in Task C4.

**Files:**
- Modify: `src/env/jobs/mod.rs` (refactor `format_job`, `format_job_long`, delete `format_status`)

- [ ] **Step 1: Run baseline `format_job_*` tests**

Run:
```bash
cargo test --lib env::jobs::tests::test_format 2>&1 | tail -10
```
Expected: 3 tests pass (`test_format_job_running`, `test_format_job_done`, `test_format_job_nonexistent`). These tests assert on substrings (`"Running"`, `"Done"`, `"sleep 10"`) so they will continue to pass after the Display refactor.

- [ ] **Step 2: Refactor `format_job` to use Display**

In `src/env/jobs/mod.rs`, replace the existing `format_job` method body. Locate this block:

```rust
pub fn format_job(&self, id: JobId) -> Option<String> {
    let job = self.jobs.get(&id)?;
    let indicator = self.indicator(id);
    let status_str = self.format_status(job.status);
    Some(format!(
        "[{}]{}  {}  {}",
        job.id, indicator, status_str, job.command
    ))
}
```

Replace with:

```rust
pub fn format_job(&self, id: JobId) -> Option<String> {
    let job = self.jobs.get(&id)?;
    let indicator = self.indicator(id);
    Some(format!(
        "[{}]{}  {}  {}",
        job.id, indicator, job.status, job.command
    ))
}
```

(`{}` on `job.status` invokes `Display`. The intermediate `let status_str` binding is no longer needed.)

- [ ] **Step 3: Refactor `format_job_long` to use Display**

In `src/env/jobs/mod.rs`, locate:

```rust
pub fn format_job_long(&self, id: JobId) -> Option<String> {
    let job = self.jobs.get(&id)?;
    let indicator = self.indicator(id);
    let status_str = self.format_status(job.status);
    Some(format!(
        "[{}]{} {}  {}  {}",
        job.id,
        indicator,
        job.pgid.as_raw(),
        status_str,
        job.command
    ))
}
```

Replace with:

```rust
pub fn format_job_long(&self, id: JobId) -> Option<String> {
    let job = self.jobs.get(&id)?;
    let indicator = self.indicator(id);
    Some(format!(
        "[{}]{} {}  {}  {}",
        job.id,
        indicator,
        job.pgid.as_raw(),
        job.status,
        job.command
    ))
}
```

- [ ] **Step 4: Delete the `format_status` method**

In `src/env/jobs/mod.rs`, locate the entire `fn format_status` block:

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

Delete it entirely. (Display in `model.rs` now covers every output that `format_status` produced.)

- [ ] **Step 5: Verify `format_job_*` tests still pass**

Run:
```bash
cargo test --lib env::jobs::tests::test_format 2>&1 | tail -5
```
Expected: 3 tests still pass.

- [ ] **Step 6: Verify no other callers of `format_status` exist**

Run:
```bash
rg -n "format_status" --type rust src/
```
Expected: zero hits. (`format_status` was a private method only ever called from `format_job` and `format_job_long`.)

- [ ] **Step 7: Run full test suite**

Run:
```bash
cargo test --lib --features test-helpers 2>&1 | tail -3
```
Expected: `test result: ok. NNN passed; 0 failed`.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(env/jobs): use Display for JobStatus, drop format_status

SP2 PR-C step 3. format_job and format_job_long now interpolate
job.status directly via {}. The private fn format_status (which
duplicated what Display now expresses) is removed.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
Expected: commit succeeds.

---

## Task C4: Extract `format.rs`

**Goal:** Move `format_job`, `format_job_long`, and the private `indicator` helper to `src/env/jobs/format.rs` as an `impl super::JobTable` block. Move the three `test_format_job_*` tests with the target.

**Files:**
- Create: `src/env/jobs/format.rs`
- Modify: `src/env/jobs/mod.rs` (delete moved methods + tests, add `mod format;`)

- [ ] **Step 1: Create `src/env/jobs/format.rs`**

Create the file with this content:

```rust
//! POSIX-style job formatting for the `jobs` builtin and related
//! status reporting.
//!
//! Two output forms:
//! - `format_job`: short form `[n]+  Status  command`
//! - `format_job_long`: long form `[n]+ PID  Status  command`
//!
//! The indicator character is `+` for the current job, `-` for the
//! previous job, and a space otherwise.

use super::JobId;

impl super::JobTable {
    /// Format a job in POSIX short form: `[n]+  Status  command`
    ///
    /// The indicator character is `+` for the current job, `-` for the
    /// previous job, and a space otherwise.
    pub fn format_job(&self, id: JobId) -> Option<String> {
        let job = self.jobs.get(&id)?;
        let indicator = self.indicator(id);
        Some(format!(
            "[{}]{}  {}  {}",
            job.id, indicator, job.status, job.command
        ))
    }

    /// Format a job in long form: `[n]+ PID  Status  command`
    pub fn format_job_long(&self, id: JobId) -> Option<String> {
        let job = self.jobs.get(&id)?;
        let indicator = self.indicator(id);
        Some(format!(
            "[{}]{} {}  {}  {}",
            job.id,
            indicator,
            job.pgid.as_raw(),
            job.status,
            job.command
        ))
    }

    fn indicator(&self, id: JobId) -> char {
        if self.current == Some(id) {
            '+'
        } else if self.previous == Some(id) {
            '-'
        } else {
            ' '
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::jobs::{JobStatus, JobTable};
    use nix::unistd::Pid;

    fn pid(n: i32) -> Pid {
        Pid::from_raw(n)
    }

    #[test]
    fn test_format_job_running() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(100), vec![pid(100)], "sleep 10", false);
        let s = table.format_job(id).expect("format should succeed");
        assert!(s.contains("[1]"), "should contain job id");
        assert!(s.contains('+'), "current job should have + indicator");
        assert!(s.contains("Running"), "should contain Running status");
        assert!(s.contains("sleep 10"), "should contain command");
    }

    #[test]
    fn test_format_job_done() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(200), vec![pid(200)], "true", false);
        table.update_status(pid(200), JobStatus::Done(0));
        let s = table.format_job(id).expect("format should succeed");
        assert!(s.contains("Done"), "should contain Done status");
    }

    #[test]
    fn test_format_job_nonexistent() {
        let table = JobTable::default();
        assert!(table.format_job(99).is_none());
    }
}
```

- [ ] **Step 2: Remove moved items from `src/env/jobs/mod.rs`**

In `src/env/jobs/mod.rs`, delete:

1. The `format_job` method (recently refactored in Task C3).
2. The `format_job_long` method.
3. The private `indicator` helper.
4. From `#[cfg(test)] mod tests`, the three tests `test_format_job_running`, `test_format_job_done`, `test_format_job_nonexistent`.

- [ ] **Step 3: Add `mod format;` to `mod.rs`**

In `src/env/jobs/mod.rs`, add `mod format;` to the module-declaration block (alphabetized):

```rust
mod format;
mod model;
mod spec;
mod terminal;
```

(No re-export needed — `format_job` and `format_job_long` are methods on `JobTable`, accessed as `JobTable::format_job` from outside.)

- [ ] **Step 4: Run lib tests**

Run:
```bash
cargo test --lib env::jobs::format 2>&1 | tail -10
```
Expected: 3 `format::tests::*` pass.

Then full suite:
```bash
cargo test --lib --features test-helpers 2>&1 | tail -3
```
Expected: `test result: ok. NNN passed; 0 failed`.

- [ ] **Step 5: Verify external compilation**

Run:
```bash
cargo build 2>&1 | tail -3
```
Expected: `Finished`.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(env/jobs): extract format methods to jobs/format.rs

SP2 PR-C step 4. Move format_job, format_job_long, and the
private indicator helper into a dedicated submodule via
impl super::JobTable. format_job_* tests follow.

No public API change — methods on JobTable resolve identically.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
Expected: commit succeeds.

---

## Task C5: Add Notification Predicates and Apply Them (TDD)

**Goal:** Introduce the three notification-state-machine predicates `is_notifiable`, `is_cleanable`, `reset_after_status_change` as private free functions in `mod.rs` (they move to `notification.rs` in Task C6). Then refactor `pending_notifications`, `cleanup_notified`, and `update_status` to use them. The notification module becomes the single source of truth for the state machine.

**Files:**
- Modify: `src/env/jobs/mod.rs` (add predicates + refactor three methods + add predicate tests)

- [ ] **Step 1: Write failing tests for predicates**

In `src/env/jobs/mod.rs::tests`, append:

```rust
// -----------------------------------------------------------------------
// Notification predicates
// -----------------------------------------------------------------------

#[test]
fn test_is_notifiable_done_unnotified() {
    let job = Job {
        id: 1,
        pgid: pid(1),
        pids: vec![pid(1)],
        command: "x".to_string(),
        status: JobStatus::Done(0),
        notified: false,
        foreground: false,
        saved_tmodes: None,
    };
    assert!(is_notifiable(&job));
}

#[test]
fn test_is_notifiable_done_already_notified() {
    let job = Job {
        id: 1,
        pgid: pid(1),
        pids: vec![pid(1)],
        command: "x".to_string(),
        status: JobStatus::Done(0),
        notified: true,
        foreground: false,
        saved_tmodes: None,
    };
    assert!(!is_notifiable(&job));
}

#[test]
fn test_is_notifiable_running_is_false() {
    let job = Job {
        id: 1,
        pgid: pid(1),
        pids: vec![pid(1)],
        command: "x".to_string(),
        status: JobStatus::Running,
        notified: false,
        foreground: false,
        saved_tmodes: None,
    };
    assert!(!is_notifiable(&job));
}

#[test]
fn test_is_cleanable_done_notified() {
    let job = Job {
        id: 1,
        pgid: pid(1),
        pids: vec![pid(1)],
        command: "x".to_string(),
        status: JobStatus::Terminated(9),
        notified: true,
        foreground: false,
        saved_tmodes: None,
    };
    assert!(is_cleanable(&job));
}

#[test]
fn test_is_cleanable_unnotified_is_false() {
    let job = Job {
        id: 1,
        pgid: pid(1),
        pids: vec![pid(1)],
        command: "x".to_string(),
        status: JobStatus::Done(0),
        notified: false,
        foreground: false,
        saved_tmodes: None,
    };
    assert!(!is_cleanable(&job));
}

#[test]
fn test_reset_after_status_change_clears_notified() {
    let mut job = Job {
        id: 1,
        pgid: pid(1),
        pids: vec![pid(1)],
        command: "x".to_string(),
        status: JobStatus::Running,
        notified: true,
        foreground: false,
        saved_tmodes: None,
    };
    reset_after_status_change(&mut job);
    assert!(!job.notified);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:
```bash
cargo test --lib env::jobs::tests::test_is_notifiable 2>&1 | tail -5
```
Expected: compile error — `cannot find function `is_notifiable` in this scope`.

- [ ] **Step 3: Add the predicates to `mod.rs`**

In `src/env/jobs/mod.rs`, just below the existing `mod` / `pub use` block at the top of the file (and above the `JobTable` struct), add:

```rust
// ---------------------------------------------------------------------------
// Notification state-machine predicates
// ---------------------------------------------------------------------------

/// True if the job has finished but the user has not yet been told.
fn is_notifiable(job: &Job) -> bool {
    !job.notified && job.status.is_terminal()
}

/// True if the job has finished AND the user has been told — safe to remove.
fn is_cleanable(job: &Job) -> bool {
    job.notified && job.status.is_terminal()
}

/// Reset notification state after a status change. Called from
/// `JobTable::update_status` whenever the status mutates so the
/// new state will be reported.
fn reset_after_status_change(job: &mut Job) {
    job.notified = false;
}
```

- [ ] **Step 4: Run tests to verify predicates work**

Run:
```bash
cargo test --lib env::jobs::tests::test_is_notifiable 2>&1 | tail -5
cargo test --lib env::jobs::tests::test_is_cleanable 2>&1 | tail -5
cargo test --lib env::jobs::tests::test_reset_after 2>&1 | tail -5
```
Expected: each batch reports `test result: ok` with the corresponding pass count.

- [ ] **Step 5: Refactor `update_status` to use `reset_after_status_change`**

In `src/env/jobs/mod.rs`, locate the existing `update_status`:

```rust
pub fn update_status(&mut self, pid: Pid, status: JobStatus) {
    if let Some(job) = self.jobs.values_mut().find(|j| j.pids.contains(&pid)) {
        job.status = status;
        job.notified = false;
    }
}
```

Replace with:

```rust
pub fn update_status(&mut self, pid: Pid, status: JobStatus) {
    if let Some(job) = self.jobs.values_mut().find(|j| j.pids.contains(&pid)) {
        job.status = status;
        reset_after_status_change(job);
    }
}
```

- [ ] **Step 6: Refactor `pending_notifications` to use `is_notifiable`**

In `src/env/jobs/mod.rs`, locate the existing `pending_notifications`:

```rust
pub fn pending_notifications(&self) -> Vec<JobId> {
    let mut ids: Vec<JobId> = self
        .jobs
        .values()
        .filter(|j| {
            !j.notified && matches!(j.status, JobStatus::Done(_) | JobStatus::Terminated(_))
        })
        .map(|j| j.id)
        .collect();
    ids.sort();
    ids
}
```

Replace with:

```rust
pub fn pending_notifications(&self) -> Vec<JobId> {
    let mut ids: Vec<JobId> = self
        .jobs
        .values()
        .filter(|j| is_notifiable(j))
        .map(|j| j.id)
        .collect();
    ids.sort();
    ids
}
```

- [ ] **Step 7: Refactor `cleanup_notified` to use `is_cleanable`**

In `src/env/jobs/mod.rs`, locate the existing `cleanup_notified`:

```rust
pub fn cleanup_notified(&mut self) {
    let to_remove: Vec<JobId> = self
        .jobs
        .values()
        .filter(|j| {
            j.notified && matches!(j.status, JobStatus::Done(_) | JobStatus::Terminated(_))
        })
        .map(|j| j.id)
        .collect();
    for id in to_remove {
        self.remove_job(id);
    }
}
```

Replace with:

```rust
pub fn cleanup_notified(&mut self) {
    let to_remove: Vec<JobId> = self
        .jobs
        .values()
        .filter(|j| is_cleanable(j))
        .map(|j| j.id)
        .collect();
    for id in to_remove {
        self.remove_job(id);
    }
}
```

- [ ] **Step 8: Verify behavior tests still pass**

Run:
```bash
cargo test --lib env::jobs::tests::test_pending 2>&1 | tail -5
cargo test --lib env::jobs::tests::test_mark_notified 2>&1 | tail -5
cargo test --lib env::jobs::tests::test_update_status 2>&1 | tail -5
```
Expected: each batch reports `test result: ok`. The behavior is preserved — predicates are a refactoring, not a logic change.

- [ ] **Step 9: Run full test suite**

Run:
```bash
cargo test --lib --features test-helpers 2>&1 | tail -3
```
Expected: `test result: ok. NNN passed; 0 failed`.

- [ ] **Step 10: Verify no inline matches! over `Done|Terminated` remain in `jobs/mod.rs`**

Run:
```bash
rg -n "matches!\(.*Done.*Terminated|matches!\(.*Terminated.*Done" src/env/jobs/mod.rs
```
Expected: zero hits. (The two inline `matches!` in `pending_notifications` and `cleanup_notified` are now expressed via `is_notifiable` / `is_cleanable`.)

- [ ] **Step 11: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(env/jobs): centralize notification state machine in predicates

SP2 PR-C step 5. Introduce is_notifiable, is_cleanable, and
reset_after_status_change as private helpers in mod.rs.
update_status, pending_notifications, and cleanup_notified now
delegate the state-machine question to one named predicate per
intent rather than three duplicated inline expressions.

No behavior change — predicate definitions inline-equivalent to
the prior matches! and !notified expressions, plus
JobStatus::is_terminal() shared with the model layer.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
Expected: commit succeeds.

---

## Task C6: Extract `notification.rs`

**Goal:** Move `pending_notifications`, `mark_notified`, `cleanup_notified` (impls on `JobTable`), the three predicates, and the corresponding tests to `src/env/jobs/notification.rs`. `update_status` stays in `mod.rs` (it's the storage-level mutator); it imports `notification::reset_after_status_change`.

**Files:**
- Create: `src/env/jobs/notification.rs`
- Modify: `src/env/jobs/mod.rs` (delete moved items, add `mod notification;`, fix `update_status` import)

- [ ] **Step 1: Create `src/env/jobs/notification.rs`**

Create the file with this content:

```rust
//! Notification state machine for terminated jobs.
//!
//! The interactive shell reports each finished job to the user
//! exactly once. This module centralizes the predicates that
//! distinguish "newly finished" from "already announced" from
//! "still alive".
//!
//! State transitions:
//! - `Running`/`Stopped` → not notifiable, not cleanable.
//! - `Done`/`Terminated` with `notified == false` → notifiable.
//! - `Done`/`Terminated` with `notified == true` → cleanable.
//!
//! `JobTable::update_status` resets `notified` whenever a job's
//! status changes so a job that was reported, then re-runs (e.g.,
//! `bg` after `Stopped`), gets re-reported on its next finish.

use super::{Job, JobId};

/// True if the job has finished but the user has not yet been told.
pub(super) fn is_notifiable(job: &Job) -> bool {
    !job.notified && job.status.is_terminal()
}

/// True if the job has finished AND the user has been told — safe to remove.
pub(super) fn is_cleanable(job: &Job) -> bool {
    job.notified && job.status.is_terminal()
}

/// Reset notification state after a status change. Called from
/// `JobTable::update_status` whenever the status mutates so the
/// new state will be reported.
pub(super) fn reset_after_status_change(job: &mut Job) {
    job.notified = false;
}

impl super::JobTable {
    /// Return ids of jobs that have finished (Done or Terminated) but have
    /// not yet been notified, sorted in ascending order.
    ///
    /// Stopped jobs are excluded — they are notified immediately at stop time
    /// by the caller, not deferred.
    pub fn pending_notifications(&self) -> Vec<JobId> {
        let mut ids: Vec<JobId> = self
            .jobs
            .values()
            .filter(|j| is_notifiable(j))
            .map(|j| j.id)
            .collect();
        ids.sort();
        ids
    }

    /// Mark a job as notified (the status change has been reported to the
    /// user).
    pub fn mark_notified(&mut self, id: JobId) {
        if let Some(job) = self.jobs.get_mut(&id) {
            job.notified = true;
        }
    }

    /// Remove all jobs that are both notified AND in a terminal state
    /// (Done or Terminated).
    pub fn cleanup_notified(&mut self) {
        let to_remove: Vec<JobId> = self
            .jobs
            .values()
            .filter(|j| is_cleanable(j))
            .map(|j| j.id)
            .collect();
        for id in to_remove {
            self.remove_job(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::jobs::{JobStatus, JobTable};
    use nix::unistd::Pid;

    fn pid(n: i32) -> Pid {
        Pid::from_raw(n)
    }

    fn job_with(status: JobStatus, notified: bool) -> Job {
        Job {
            id: 1,
            pgid: pid(1),
            pids: vec![pid(1)],
            command: "x".to_string(),
            status,
            notified,
            foreground: false,
            saved_tmodes: None,
        }
    }

    // -----------------------------------------------------------------------
    // Predicate tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_notifiable_done_unnotified() {
        assert!(is_notifiable(&job_with(JobStatus::Done(0), false)));
    }

    #[test]
    fn test_is_notifiable_done_already_notified() {
        assert!(!is_notifiable(&job_with(JobStatus::Done(0), true)));
    }

    #[test]
    fn test_is_notifiable_running_is_false() {
        assert!(!is_notifiable(&job_with(JobStatus::Running, false)));
    }

    #[test]
    fn test_is_cleanable_terminated_notified() {
        assert!(is_cleanable(&job_with(JobStatus::Terminated(9), true)));
    }

    #[test]
    fn test_is_cleanable_unnotified_is_false() {
        assert!(!is_cleanable(&job_with(JobStatus::Done(0), false)));
    }

    #[test]
    fn test_reset_after_status_change_clears_notified() {
        let mut job = job_with(JobStatus::Running, true);
        reset_after_status_change(&mut job);
        assert!(!job.notified);
    }

    // -----------------------------------------------------------------------
    // pending_notifications
    // -----------------------------------------------------------------------

    #[test]
    fn test_pending_notifications_empty_when_running() {
        let mut table = JobTable::default();
        table.add_job(pid(1), vec![pid(1)], "sleep", false);
        assert!(table.pending_notifications().is_empty());
    }

    #[test]
    fn test_pending_notifications_non_empty_when_done() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "ls", false);
        table.update_status(pid(1), JobStatus::Done(0));

        let pending = table.pending_notifications();
        assert_eq!(pending, vec![id]);
    }

    #[test]
    fn test_pending_notifications_sorted() {
        let mut table = JobTable::default();
        let id1 = table.add_job(pid(1), vec![pid(1)], "a", false);
        let id2 = table.add_job(pid(2), vec![pid(2)], "b", false);
        table.update_status(pid(2), JobStatus::Done(0));
        table.update_status(pid(1), JobStatus::Terminated(9));

        let pending = table.pending_notifications();
        assert_eq!(pending, vec![id1, id2]);
    }

    // -----------------------------------------------------------------------
    // mark_notified clears pending
    // -----------------------------------------------------------------------

    #[test]
    fn test_mark_notified_clears_pending() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "ls", false);
        table.update_status(pid(1), JobStatus::Done(0));
        assert!(!table.pending_notifications().is_empty());

        table.mark_notified(id);
        assert!(table.pending_notifications().is_empty());
    }
}
```

- [ ] **Step 2: Remove moved items from `src/env/jobs/mod.rs`**

In `src/env/jobs/mod.rs`, delete:

1. The three private free functions `is_notifiable`, `is_cleanable`, `reset_after_status_change` (the block introduced in Task C5).
2. The three methods `pending_notifications`, `mark_notified`, `cleanup_notified` from inside `impl JobTable`.
3. From `#[cfg(test)] mod tests`, remove the predicate tests added in Task C5 (`test_is_notifiable_*`, `test_is_cleanable_*`, `test_reset_after_status_change_clears_notified`) AND the `test_pending_notifications_*` and `test_mark_notified_clears_pending` tests. They live in `notification.rs::tests` now.

- [ ] **Step 3: Update `update_status` to import `reset_after_status_change` from notification**

In `src/env/jobs/mod.rs`, the `update_status` method currently calls `reset_after_status_change(job)` (the free function). After the move, this function lives in `super::notification::reset_after_status_change`. There are two clean options:

**Option A** (preferred — explicit path, no import clutter):

```rust
pub fn update_status(&mut self, pid: Pid, status: JobStatus) {
    if let Some(job) = self.jobs.values_mut().find(|j| j.pids.contains(&pid)) {
        job.status = status;
        notification::reset_after_status_change(job);
    }
}
```

This works because `mod notification;` exposes the module path within `mod.rs`. Use this option.

- [ ] **Step 4: Add `mod notification;` to `mod.rs`**

In the module-declaration block in `mod.rs`, add `notification` (alphabetized):

```rust
mod format;
mod model;
mod notification;
mod spec;
mod terminal;
```

(No `pub use` re-export — `pending_notifications` and friends are methods on `JobTable`.)

- [ ] **Step 5: Run lib tests**

Run:
```bash
cargo test --lib env::jobs::notification 2>&1 | tail -10
```
Expected: 10 `notification::tests::*` pass.

Then full:
```bash
cargo test --lib --features test-helpers 2>&1 | tail -3
```
Expected: `test result: ok. NNN passed; 0 failed`.

- [ ] **Step 6: Verify external callers (`cleanup_notified` from `exec/job_control`) still compile**

Run:
```bash
rg -n "cleanup_notified" --type rust src/
```
Expected: hits at `src/exec/job_control.rs:525` (caller) and `src/env/jobs/notification.rs` (definition). The caller resolves through `JobTable::cleanup_notified`.

```bash
cargo build 2>&1 | tail -3
```
Expected: `Finished`.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(env/jobs): extract notification state machine to jobs/notification.rs

SP2 PR-C step 6 (final extraction). Move pending_notifications,
mark_notified, cleanup_notified, and the three predicates
(is_notifiable, is_cleanable, reset_after_status_change) into
their own module. update_status now calls
notification::reset_after_status_change explicitly.

The notification module is the single source of truth for the
state machine; mod.rs holds only the storage core.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
Expected: commit succeeds. **PR-C is now complete.**

---

## Task C7: Final Verification, TODO.md Cleanup, and Formatting

**Goal:** Confirm every DoD item, run extended verification (e2e, bench, clippy, fmt), update `TODO.md` if necessary, and apply `cargo fmt` to clean up any wrap differences.

**Files:**
- Modify (possibly): `TODO.md` (if any entry references `src/env/jobs.rs` directly)
- Modify (possibly): files in `src/env/jobs/` if `cargo fmt --check` rewraps

- [ ] **Step 1: Confirm file inventory**

Run:
```bash
ls -la src/env/jobs/
wc -l src/env/jobs/*.rs
```
Expected: `format.rs`, `mod.rs`, `model.rs`, `notification.rs`, `spec.rs`, `terminal.rs`. Each file ≤ 400 lines (per umbrella DoD #6).

- [ ] **Step 2: Run cargo fmt check**

Run:
```bash
cargo fmt --check 2>&1 | head -20
```
Expected: no diff. If diffs exist, run `cargo fmt` and stage the changes for the verification commit at the end.

- [ ] **Step 3: Run full unit + integration test suite**

Run:
```bash
cargo test --lib --features test-helpers 2>&1 | tail -5
```
Expected: `test result: ok. NNN passed; 0 failed`. Note the count and verify it matches Task 0 baseline (or is higher by the count of new tests added in C1, C2, C5).

- [ ] **Step 4: Run integration tests**

Run:
```bash
cargo test --features test-helpers --test plugin 2>&1 | tail -3
```
Expected: `test result: ok. 24 passed; 0 failed` (or whatever the baseline was in Task 0).

- [ ] **Step 5: Run e2e suite**

Run:
```bash
./e2e/run_tests.sh 2>&1 | tail -3
```
Expected: `Passed: 393 / 393` (or whatever the baseline was). Special attention to `e2e/06-jobs/`.

- [ ] **Step 6: Run pty interactive tests (3x for flake check)**

PTY tests can be flaky. Run three times:
```bash
for i in 1 2 3; do
    echo "Run $i..."
    cargo test --test pty_interactive 2>&1 | tail -3
done
```
Expected: each run reports `test result: ok` with 0 failures. **If any single run fails, do NOT pass DoD** — investigate.

- [ ] **Step 7: Verify bench compile**

Run:
```bash
cargo bench --no-run 2>&1 | tail -3
```
Expected: `Finished`.

- [ ] **Step 8: Run clippy**

Run:
```bash
cargo clippy --all-targets -- -D warnings 2>&1 | tail -20
```
Expected: only the two pre-existing `doc_lazy_continuation` errors at `src/plugin/mod.rs:98-99`. No new errors or warnings from `src/env/jobs/`.

If clippy reports new issues in `src/env/jobs/`, fix them (most likely candidates: `clippy::redundant_field_names`, `clippy::needless_borrow`, etc.) and re-run.

- [ ] **Step 9: Verify TODO.md does not reference `src/env/jobs.rs`**

Run:
```bash
rg -n "env/jobs\.rs|env/jobs.rs" TODO.md
```
Expected: zero hits. If any hit exists, the entry is now stale (the file is `src/env/jobs/mod.rs` plus submodules) — update or remove the reference. Per the umbrella, the only TODO entry SP1 cleared was about `src/plugin/host.rs`; SP2 has no specific entry to clear unless one was added since.

- [ ] **Step 10: Verify CLAUDE.md and README.md still resolve**

Run:
```bash
rg -n "env/jobs|env::jobs" CLAUDE.md README.md 2>/dev/null
```
Expected: any references are either to the module path `crate::env::jobs` (still valid) or the directory `src/env/jobs/` (still valid). No reference to a single `jobs.rs` file remains accurate.

- [ ] **Step 11: Verify external callers compile and behave correctly**

Run:
```bash
cargo build --bins 2>&1 | tail -3
```
Expected: `Finished`.

```bash
rg -n "use crate::env::jobs" --type rust src/ | head -10
```
Expected: same import sites as Task 0 baseline (Step 5). All paths unchanged.

- [ ] **Step 12: Diff-check public API**

Run:
```bash
cargo doc --no-deps --document-private-items 2>&1 | tail -3
```
Expected: `Finished`. Then:
```bash
ls target/doc/yosh/env/jobs/
```
Expected: index.html exists with `JobTable`, `Job`, `JobStatus`, `JobId`, `JobSpec`, `JobSpecError`, `parse_job_spec`, `give_terminal`, `take_terminal` all listed. If anything is missing, a re-export is incomplete — investigate.

- [ ] **Step 13: Commit any remaining `cargo fmt` rewraps**

If Step 2 surfaced any `cargo fmt` differences, they were applied during this task. Commit them:

```bash
git status
```

If `git status` shows changes:
```bash
git add -A
git commit -m "$(cat <<'EOF'
style: apply rustfmt rewraps after SP2 redesign

SP2 final cleanup. Applies any line-wrap differences cargo fmt
flagged after the responsibility split. No semantic change.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

If `git status` shows clean tree, skip this commit step.

- [ ] **Step 14: Final summary check**

Confirm the SP2 PR breakdown landed exactly as planned:
```bash
git log --oneline -15
```
Expected (most recent first):
- `style: apply rustfmt rewraps...` (Task C7 cleanup, optional)
- `refactor(env/jobs): extract notification state machine to jobs/notification.rs` (Task C6)
- `refactor(env/jobs): centralize notification state machine in predicates` (Task C5)
- `refactor(env/jobs): extract format methods to jobs/format.rs` (Task C4)
- `refactor(env/jobs): use Display for JobStatus, drop format_status` (Task C3)
- `feat(env/jobs): add JobStatus::is_terminal()` (Task C2)
- `feat(env/jobs): impl Display for JobStatus` (Task C1)
- `refactor(env/jobs): extract job-spec parsing/resolution to jobs/spec.rs` (Task B1)
- `refactor(env/jobs): extract terminal control to jobs/terminal.rs` (Task A3)
- `refactor(env/jobs): extract Job, JobStatus, JobId to jobs/model.rs` (Task A2)
- `refactor(env/jobs): convert jobs.rs to jobs/mod.rs` (Task A1)

11 commits total (10 SP2 commits + optional fmt cleanup). **All DoD criteria are now met. SP2 is complete.**

---

## Risk Mitigation Notes

These risks were called out in the SP2 spec; the plan's defenses are listed inline:

| Risk | Defense |
|---|---|
| `Display for JobStatus` is observable to callers (capability addition) | Tasks C1–C2 add it as a strict addition. C3 swaps internal `format_status` calls to use it. No external caller is forced to adopt; callers using `format_status` directly would have failed to compile (it's private), so there are no such callers. |
| `parse_job_spec` signature change | Plan Task B1 preserves `pub fn parse_job_spec(s: &str) -> Result<JobSpec<'_>, JobSpecError>` byte-for-byte. The lifetime tie to the input slice is unchanged. |
| Multiple `impl JobTable` blocks may surprise rustdoc | Verified in plan Task C7 Step 12 (`cargo doc --no-deps`). rustdoc collapses them to a single page automatically. |
| Job-control behavior touched (`set -m`, `fg`/`bg`) | Plan Task C7 Step 6 runs `tests/pty_interactive.rs` 3x consecutively. Step 5 confirms `e2e/06-jobs/` passes. |
| Caller imports unchanged | Plan Task A1 verifies `crate::env::jobs::*` paths unchanged via `rg`; Tasks A2/A3/B1/C4/C6 each include a "verify external callers compile" step. |

---

## Plan Self-Review

This section is the implementer's own pre-flight before starting work. The author of this plan ran the self-review checklist and recorded findings here.

**1. Spec coverage:**
- ✅ §"Proposed Structure" → covered by the File Structure section + Tasks A1–C6.
- ✅ §"Notification State Machine — Predicate Centralization" → Tasks C5 + C6.
- ✅ §"Display for JobStatus" → Task C1.
- ✅ §"Terminal Module" → Task A3.
- ✅ §"Visibility" (out of scope, noted in spec) → no task; visibility-tightening deferred to a future spec.
- ✅ §"Test Reorganization" table → mapped to specific tasks (model: A2 + C1/C2; spec: B1; notification: C5/C6; format: C4; terminal: A3; mod.rs storage: stays).
- ✅ §"PR Breakdown" 3-PR plan → A1+A2+A3 = PR-A; B1 = PR-B; C1–C6 = PR-C. C7 is final verification (not a separate PR — runs alongside the last task's commit).
- ✅ §"Risks" → addressed in the Risk Mitigation Notes section above.
- ✅ §"Definition of Done" → expanded into the 10-item DoD list (umbrella 1–8 plus SP2-specific 9–10).

**2. Placeholder scan:** None of "TBD", "TODO", "implement later", "fill in details", "Add appropriate error handling", "Similar to Task N", or unfilled code blocks. All steps include exact code, file paths, commands, and expected output.

**3. Type consistency:**
- `JobId` defined in `model.rs` (Task A2), used by `spec.rs` (Task B1, `use super::JobId`), `format.rs` (Task C4, `use super::JobId`), `notification.rs` (Task C6, `use super::{Job, JobId}`).
- `JobStatus::is_terminal()` defined in C2, consumed in C5 inline (`job.status.is_terminal()`) and C6 (predicates moved as-is).
- `Job` field `saved_tmodes` is `pub(super)` in model.rs (Task A2). `add_job` in mod.rs constructs Job via field-init `saved_tmodes: None` — works because `mod.rs` is the parent of `model.rs`. ✅
- `JobTable` private fields (`jobs`, `next_id`, `current`, `previous`, `shell_tmodes`) accessed from submodules via Rust's child-module visibility rule. ✅
- Predicate signatures: `is_notifiable(&Job) -> bool`, `is_cleanable(&Job) -> bool`, `reset_after_status_change(&mut Job)`. Consistent across C5 and C6. ✅
- `parse_job_spec(s: &str) -> Result<JobSpec<'_>, JobSpecError>` preserved verbatim from line 98 of original `jobs.rs`. ✅

No issues found.
