# Terminal-State Save/Restore Follow-up Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land four follow-ups from the terminal-state save/restore code review: tighten `Job.saved_tmodes` to private + accessor, fix the `captured.is_some()` guard in `wait_for_foreground_job`, document the Stopped-arm side effect, and annotate the redundant `Repl::new` guard.

**Architecture:** Mirror the `JobTable.shell_tmodes` pattern (private field + getter/setter). Extract the Stopped-arm state transition into a private `Executor::record_stopped_state` helper so the bug fix (drop `captured.is_some()` guard → unconditional setter call) is unit-testable without a PTY or `fork`. No public API additions outside the workspace.

**Tech Stack:** Rust 2024 edition, `nix::sys::termios`, `libc`, existing `cargo test` runner. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-04-26-terminal-state-followup-design.md`

---

## File Map

| File | Change |
| --- | --- |
| `src/env/jobs.rs` | Make `Job.saved_tmodes` private; add `Job::saved_tmodes()` getter + `Job::set_saved_tmodes()` setter; rewrite existing test to use accessor; add new setter test. |
| `src/exec/mod.rs` | Update `builtin_fg` read site to accessor; replace Stopped-arm inline state-write block with a call to a new `Executor::record_stopped_state` helper (drops the `captured.is_some()` guard); update `wait_for_foreground_job` docstring; add three new unit tests. |
| `src/interactive/mod.rs` | Add a one-line comment to the redundant `is_interactive && monitor` guard in `Repl::new`. |
| `TODO.md` | Delete the four entries (currently lines 10–13). |

---

## Task 1: Private `saved_tmodes` field + accessor pair (TDD)

This task closes TODO L10 and migrates all read/write sites to the accessor. The Stopped-arm `if captured.is_some()` guard is **kept as-is in this task**; it is removed by Task 2 once the helper is in place to make the change testable.

**Files:**
- Modify: `src/env/jobs.rs:25-37` (struct field visibility)
- Modify: `src/env/jobs.rs` impl `Job` block (add accessor pair) — currently the file has no `impl Job` block; add one immediately after the struct definition.
- Modify: `src/env/jobs.rs:493-499` (existing test)
- Modify: `src/env/jobs.rs` test module (add new setter test)
- Modify: `src/exec/mod.rs:696` (read in `builtin_fg`)
- Modify: `src/exec/mod.rs:876-878` (write in Stopped arm — route through setter only; do not drop the guard yet)
- Test: `src/env/jobs.rs::tests::test_job_set_saved_tmodes_overwrites_with_none` (new)

- [ ] **Step 1.1: Write the new failing test for the setter contract**

Add at the end of the existing `mod tests` block in `src/env/jobs.rs` (immediately before the closing `}` of the module, after `test_resolve_prefix_matches_done_job`):

```rust
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
```

- [ ] **Step 1.2: Run the new test to verify it fails**

Run: `cargo test --lib env::jobs::tests::test_job_set_saved_tmodes_overwrites_with_none 2>&1 | tail -20`

Expected: compile error — `set_saved_tmodes` and `saved_tmodes()` (as a method) do not exist on `Job`.

- [ ] **Step 1.3: Make the field private and add the accessor pair**

In `src/env/jobs.rs`, change the field declaration at line 36 from:

```rust
    /// Termios snapshot captured when the job last stopped (SIGTSTP/SIGSTOP).
    /// Used as the restore target on `fg`. `None` for jobs that have never
    /// been stopped, or on non-interactive / non-monitor shell modes.
    pub saved_tmodes: Option<nix::sys::termios::Termios>,
```

to:

```rust
    /// Termios snapshot captured when the job last stopped (SIGTSTP/SIGSTOP).
    /// Used as the restore target on `fg`. `None` for jobs that have never
    /// been stopped, or on non-interactive / non-monitor shell modes.
    saved_tmodes: Option<nix::sys::termios::Termios>,
```

Then add an `impl Job` block immediately after the struct definition (so it lives directly after `pub struct Job { ... }` ending around line 37):

```rust
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
```

- [ ] **Step 1.4: Update the existing field-access test to use the accessor**

In `src/env/jobs.rs:493-499`, replace:

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

with:

```rust
#[test]
fn test_job_saved_tmodes_defaults_none() {
    let mut table = JobTable::default();
    let id = table.add_job(pid(42), vec![pid(42)], "cmd", false);
    let job = table.get(id).expect("job should exist");
    assert!(job.saved_tmodes().is_none(),
        "saved_tmodes() should default to None on new job");
}
```

- [ ] **Step 1.5: Update the read site in `builtin_fg`**

In `src/exec/mod.rs` around line 696, replace:

```rust
                let job_t = self
                    .env
                    .process
                    .jobs
                    .get(job_id)
                    .and_then(|j| j.saved_tmodes.clone());
```

with:

```rust
                let job_t = self
                    .env
                    .process
                    .jobs
                    .get(job_id)
                    .and_then(|j| j.saved_tmodes().cloned());
```

- [ ] **Step 1.6: Route the Stopped-arm write through the setter (keep the guard)**

In `src/exec/mod.rs` around lines 873-879, replace:

```rust
                    if let Some(job) = self.env.process.jobs.get_mut(job_id) {
                        job.status = JobStatus::Stopped(sig as i32);
                        job.foreground = false;
                        if captured.is_some() {
                            job.saved_tmodes = captured;
                        }
                    }
```

with:

```rust
                    if let Some(job) = self.env.process.jobs.get_mut(job_id) {
                        job.status = JobStatus::Stopped(sig as i32);
                        job.foreground = false;
                        if captured.is_some() {
                            job.set_saved_tmodes(captured);
                        }
                    }
```

Note: the `captured.is_some()` guard is intentionally **kept** in this task so the bug fix lives at one call site change (Task 2) rather than being entangled with the visibility migration. Ownership is the same as the original code — `is_some()` borrows, and the inner branch moves `captured` into the setter exactly once. Task 2 removes the guard entirely.

- [ ] **Step 1.7: Run the full test suite to verify no regression**

Run: `cargo test --lib env::jobs 2>&1 | tail -30`

Expected: all `env::jobs::tests::*` pass, including the new `test_job_set_saved_tmodes_overwrites_with_none` and the rewritten `test_job_saved_tmodes_defaults_none`.

Run: `cargo build 2>&1 | tail -15`

Expected: clean build (the `builtin_fg` read site and the Stopped-arm write site now compile against the private field).

- [ ] **Step 1.8: Commit**

```bash
git add src/env/jobs.rs src/exec/mod.rs
git commit -m "$(cat <<'EOF'
refactor(jobs): private Job.saved_tmodes + accessor pair

Mirrors the JobTable.shell_tmodes pattern. The "written only by
wait_for_foreground_job on WaitStatus::Stopped" invariant is now
enforced at the type level via the set_saved_tmodes setter.

Closes TODO entry: Job.saved_tmodes is a `pub` field.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Extract `record_stopped_state` helper + drop `captured.is_some()` guard (TDD)

This task closes TODO L11 (the bug). The bug surface is at the call site in `wait_for_foreground_job`, not at the setter — a setter-only test cannot prove the call site invokes the setter unconditionally. Extracting the state transition into `Executor::record_stopped_state` lets a unit test directly prove "captured = None must clear stale `saved_tmodes`," which is the exact invariant the old `captured.is_some()` guard violated.

**Files:**
- Modify: `src/exec/mod.rs` (add `record_stopped_state` method on `Executor`; replace Stopped-arm inline block with a call)
- Modify: `src/exec/mod.rs::tests` (add three new tests)

- [ ] **Step 2.1: Write the failing call-site bug-fix test**

Add to the `mod tests` block in `src/exec/mod.rs` (after the existing `test_with_errexit_suppressed_nested` test, before the closing `}` of the test module):

```rust
#[test]
fn record_stopped_state_clears_stale_saved_tmodes_on_none_capture() {
    use crate::env::jobs::JobStatus;
    use nix::unistd::Pid;
    let mut exec = Executor::new("yosh", vec![]);
    let pid = Pid::from_raw(12345);
    let id = exec
        .env
        .process
        .jobs
        .add_job(pid, vec![pid], "test-cmd", true);

    // Pre-populate saved_tmodes as if a previous stop captured a TTY snapshot.
    let zeroed: libc::termios = unsafe { std::mem::zeroed() };
    let t: nix::sys::termios::Termios = zeroed.into();
    exec.env
        .process
        .jobs
        .get_mut(id)
        .unwrap()
        .set_saved_tmodes(Some(t));
    assert!(
        exec.env
            .process
            .jobs
            .get(id)
            .unwrap()
            .saved_tmodes()
            .is_some(),
        "precondition: saved_tmodes should be populated before the simulated stop",
    );

    // Simulate the next stop where capture_tty_termios() returned Ok(None)
    // (e.g., after `exec 0</dev/null` redirected stdin away from the TTY).
    exec.record_stopped_state(id, libc::SIGTSTP, None);

    let job = exec
        .env
        .process
        .jobs
        .get(id)
        .expect("job should still be in table");
    assert!(
        job.saved_tmodes().is_none(),
        "stale termios must be cleared when capture returns None",
    );
    assert!(matches!(job.status, JobStatus::Stopped(_)));
    assert!(!job.foreground);
}
```

- [ ] **Step 2.2: Run the failing test to verify it fails**

Run: `cargo test --lib record_stopped_state_clears_stale_saved_tmodes_on_none_capture 2>&1 | tail -15`

Expected: compile error — `Executor::record_stopped_state` does not exist.

- [ ] **Step 2.3: Add the `record_stopped_state` helper**

In `src/exec/mod.rs`, add a new method to `impl Executor` immediately after `restore_shell_termios_if_interactive` (currently at lines 799-805) and before `wait_for_foreground_job` (currently at line 811):

```rust
    /// Apply the per-job state transition for `WaitStatus::Stopped`.
    ///
    /// Pure over `(job_id, sig, captured)`: writes the Stopped status,
    /// clears the foreground flag, and stores the captured termios —
    /// including `None`, which intentionally clears any previously saved
    /// snapshot. Preserves glibc-manual semantics across mid-session
    /// `exec 0</dev/null`: a stale snapshot from a TTY the shell no
    /// longer drives must not survive into a later `fg`.
    ///
    /// Silently no-ops if `job_id` is no longer in the table; the caller
    /// (`wait_for_foreground_job`) already tolerates that race.
    fn record_stopped_state(
        &mut self,
        job_id: crate::env::jobs::JobId,
        sig: i32,
        captured: Option<nix::sys::termios::Termios>,
    ) {
        use crate::env::jobs::JobStatus;
        if let Some(job) = self.env.process.jobs.get_mut(job_id) {
            job.status = JobStatus::Stopped(sig);
            job.foreground = false;
            job.set_saved_tmodes(captured);
        }
    }
```

- [ ] **Step 2.4: Replace the Stopped-arm inline block with a call to the helper**

In `src/exec/mod.rs` around lines 873-879 (the block that Task 1 already routed through the setter), replace:

```rust
                    if let Some(job) = self.env.process.jobs.get_mut(job_id) {
                        job.status = JobStatus::Stopped(sig as i32);
                        job.foreground = false;
                        if captured.is_some() {
                            job.set_saved_tmodes(captured);
                        }
                    }
```

with:

```rust
                    self.record_stopped_state(job_id, sig as i32, captured);
```

This drops the `captured.is_some()` guard (the bug fix) and folds the per-job state writes into the helper.

- [ ] **Step 2.5: Run the bug-fix test to verify it passes**

Run: `cargo test --lib record_stopped_state_clears_stale_saved_tmodes_on_none_capture 2>&1 | tail -15`

Expected: PASS.

- [ ] **Step 2.6: Add the positive-case helper test**

Add immediately after the bug-fix test in `src/exec/mod.rs::tests`:

```rust
#[test]
fn record_stopped_state_stores_some_capture() {
    use crate::env::jobs::JobStatus;
    use nix::unistd::Pid;
    let mut exec = Executor::new("yosh", vec![]);
    let pid = Pid::from_raw(12346);
    let id = exec
        .env
        .process
        .jobs
        .add_job(pid, vec![pid], "test-cmd", true);

    let zeroed: libc::termios = unsafe { std::mem::zeroed() };
    let t: nix::sys::termios::Termios = zeroed.into();

    exec.record_stopped_state(id, libc::SIGTSTP, Some(t));

    let job = exec
        .env
        .process
        .jobs
        .get(id)
        .expect("job should still be in table");
    assert!(
        job.saved_tmodes().is_some(),
        "Some capture must be stored",
    );
    assert!(matches!(job.status, JobStatus::Stopped(_)));
    assert!(!job.foreground);
}
```

- [ ] **Step 2.7: Add the no-op-on-unknown-job helper test**

Add immediately after the positive-case test:

```rust
#[test]
fn record_stopped_state_no_op_on_unknown_job() {
    let mut exec = Executor::new("yosh", vec![]);
    // job_id 9999 was never added; the helper must silently no-op
    // (the same race-tolerance the caller, `wait_for_foreground_job`,
    // already exhibits when a job is removed between waitpid and the
    // state-write).
    exec.record_stopped_state(9999, libc::SIGTSTP, None);
    assert!(exec.env.process.jobs.get(9999).is_none());
}
```

- [ ] **Step 2.8: Run all three new tests to verify they pass**

Run: `cargo test --lib record_stopped_state 2>&1 | tail -20`

Expected: 3 tests pass (`record_stopped_state_clears_stale_saved_tmodes_on_none_capture`, `record_stopped_state_stores_some_capture`, `record_stopped_state_no_op_on_unknown_job`).

- [ ] **Step 2.9: Commit**

```bash
git add src/exec/mod.rs
git commit -m "$(cat <<'EOF'
fix(exec): clear saved_tmodes on None capture after suspend

Drops the `captured.is_some()` guard in wait_for_foreground_job's
Stopped arm. Under the old guard, a job that stopped after a
mid-session `exec 0</dev/null` retained a stale termios from an
earlier stop, which a later `fg` would then try to apply to a TTY
the shell no longer drives. Matches glibc-manual semantics.

The Stopped-arm state transition is extracted into a private
Executor::record_stopped_state helper so the bug fix is unit-testable
without a PTY or fork. Three tests pin the behavior:
- *_clears_stale_saved_tmodes_on_none_capture (the bug fix)
- *_stores_some_capture (positive symmetry)
- *_no_op_on_unknown_job (race contract)

Closes TODO entry: wait_for_foreground_job Stopped-arm
captured.is_some() guard.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Update `wait_for_foreground_job` docstring (TODO L12)

**Files:**
- Modify: `src/exec/mod.rs:807-810` (docstring above `wait_for_foreground_job`)

- [ ] **Step 3.1: Replace the docstring**

In `src/exec/mod.rs` lines 807-810, replace:

```rust
    /// Wait for a foreground job to complete or stop.
    ///
    /// Returns a `ForegroundWaitResult` containing the last exit status,
    /// per-process statuses (for pipefail), and whether the job was stopped.
```

with:

```rust
    /// Wait for a foreground job to complete or stop.
    ///
    /// Returns a `ForegroundWaitResult` containing the last exit status,
    /// per-process statuses (for pipefail), and whether the job was stopped.
    ///
    /// Side effect: on `WaitStatus::Stopped`, captures the current TTY
    /// termios (or `None` when stdin is not a TTY / non-interactive /
    /// non-monitor) and hands it to `record_stopped_state`, which writes
    /// it to `job.saved_tmodes` so a later `fg` can replay it. The capture
    /// is always written — including `None` overwrites — to avoid keeping
    /// a stale snapshot across `exec 0</dev/null` style redirections.
```

- [ ] **Step 3.2: Verify the build still succeeds**

Run: `cargo build 2>&1 | tail -5`

Expected: clean build (docstring change only).

- [ ] **Step 3.3: Commit**

```bash
git add src/exec/mod.rs
git commit -m "$(cat <<'EOF'
docs(exec): document wait_for_foreground_job saved_tmodes side-effect

Future `grep saved_tmodes` will now land on wait_for_foreground_job
in addition to the helper that performs the write.

Closes TODO entry: wait_for_foreground_job docstring does not mention
the new job.saved_tmodes side-effect.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Add `Repl::new` guard comment (TODO L13)

**Files:**
- Modify: `src/interactive/mod.rs:51-58` (comment block + guard around `set_shell_tmodes`)

- [ ] **Step 4.1: Replace the existing comment block**

In `src/interactive/mod.rs` lines 50-58, replace:

```rust
        // Snapshot the terminal's termios so we can restore it after every
        // foreground job completes. Only meaningful in interactive + monitor
        // mode (both flags were set above). capture_tty_termios returns
        // Ok(None) silently if stdin is not a TTY.
        if executor.env.mode.is_interactive && executor.env.mode.options.monitor {
            if let Ok(Some(t)) = crate::exec::terminal_state::capture_tty_termios() {
                executor.env.process.jobs.set_shell_tmodes(t);
            }
        }
```

with:

```rust
        // Snapshot the terminal's termios so we can restore it after every
        // foreground job completes. Only meaningful in interactive + monitor
        // mode (both flags were set above). capture_tty_termios returns
        // Ok(None) silently if stdin is not a TTY.
        //
        // The `is_interactive && monitor` check is documentation-only at
        // this site (the flags are unconditionally true two lines above),
        // but mirrors the symmetric guard inside `wait_for_foreground_job`'s
        // `restore_shell_termios_if_interactive`, where the check IS
        // load-bearing. Keep both in sync so a future "simplification"
        // does not drop one and leave the other dangling.
        if executor.env.mode.is_interactive && executor.env.mode.options.monitor {
            if let Ok(Some(t)) = crate::exec::terminal_state::capture_tty_termios() {
                executor.env.process.jobs.set_shell_tmodes(t);
            }
        }
```

- [ ] **Step 4.2: Verify the build still succeeds**

Run: `cargo build 2>&1 | tail -5`

Expected: clean build (comment-only change).

- [ ] **Step 4.3: Commit**

```bash
git add src/interactive/mod.rs
git commit -m "$(cat <<'EOF'
docs(interactive): annotate redundant Repl::new tmodes guard

Records why the is_interactive && monitor check stays at this site
even though both flags are unconditionally true two lines above:
symmetry with the load-bearing guard in
restore_shell_termios_if_interactive.

Closes TODO entry: Repl::new is_interactive && monitor guard
redundant.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Clean up TODO.md

**Files:**
- Modify: `TODO.md` (delete the four "Job Control: Known Limitations" entries currently at L10-L13; the L9 entry that documents the `shell_tmodes` snapshot-vs-runtime-`stty` deviation is **kept** — it is intentionally out of scope per the spec.)

- [ ] **Step 5.1: Delete resolved entries from TODO.md**

In `TODO.md`, locate the four entries currently at L10, L11, L12, L13 (the L9 entry about `JobTable.shell_tmodes` not refreshing on runtime `stty` is **out of scope** and stays). Delete the following lines verbatim (each is a single bullet starting with `- [ ]`):

1. The `Job.saved_tmodes` is a `pub` field entry (currently L10).
2. The `wait_for_foreground_job` Stopped-arm `captured.is_some()` guard entry (currently L11).
3. The `wait_for_foreground_job` docstring entry (currently L12).
4. The `Repl::new` `is_interactive && monitor` guard entry (currently L13).

Per CLAUDE.md convention: delete the lines entirely; do not mark with `[x]`.

- [ ] **Step 5.2: Verify the four entries are gone**

Run: `grep -nE 'captured\.is_some|saved_tmodes is a|saved_tmodes. side-effect|is_interactive && monitor' TODO.md`

Expected: no matches.

- [ ] **Step 5.3: Commit**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
docs(todo): remove terminal-state followups resolved by this branch

Drops the four entries closed by:
- private Job.saved_tmodes + accessor pair
- captured.is_some() guard fix in wait_for_foreground_job
- wait_for_foreground_job docstring update
- Repl::new redundant-guard annotation

L9 (shell_tmodes vs runtime stty) is intentionally kept — matches
glibc manual semantics, revisit if user reports surface.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Final verification

**Files:** none (verification only).

- [ ] **Step 6.1: Run the full unit test suite**

Run: `cargo test --lib 2>&1 | tail -25`

Expected: all tests pass. Pay particular attention to:
- `env::jobs::tests::test_job_saved_tmodes_defaults_none` (rewritten to use accessor)
- `env::jobs::tests::test_job_set_saved_tmodes_overwrites_with_none` (new)
- `exec::tests::record_stopped_state_clears_stale_saved_tmodes_on_none_capture` (new — bug fix)
- `exec::tests::record_stopped_state_stores_some_capture` (new)
- `exec::tests::record_stopped_state_no_op_on_unknown_job` (new)

- [ ] **Step 6.2: Run the integration tests**

Run: `cargo test --test interactive --test signals --test subshell --test pty_interactive 2>&1 | tail -25`

Expected: all tests pass. The PTY tests (`pty_interactive`) exercise the unchanged Ctrl-Z → bg → fg termios cycle and the post-fg `stty` round-trip; both should remain green by API compatibility.

- [ ] **Step 6.3: Run rustfmt check on the modified files**

Per the project's CLAUDE.md note about `cargo fmt --check -- <path>` misreading edition, invoke rustfmt directly:

Run: `rustfmt --edition 2024 --check src/env/jobs.rs src/exec/mod.rs src/interactive/mod.rs 2>&1 | tail -10`

Expected: no diff output (clean format).

- [ ] **Step 6.4: Run clippy on the workspace**

Run: `cargo clippy --workspace --all-targets 2>&1 | tail -25`

Expected: no new warnings introduced. Pre-existing warnings are acceptable.

- [ ] **Step 6.5: Inspect the branch summary**

Run: `git log --oneline main..HEAD`

Expected: 5 commits in this order (Task 1 → Task 5):

```
refactor(jobs): private Job.saved_tmodes + accessor pair
fix(exec): clear saved_tmodes on None capture after suspend
docs(exec): document wait_for_foreground_job saved_tmodes side-effect
docs(interactive): annotate redundant Repl::new tmodes guard
docs(todo): remove terminal-state followups resolved by this branch
```

Run: `git diff --stat main..HEAD`

Expected modified files: `src/env/jobs.rs`, `src/exec/mod.rs`, `src/interactive/mod.rs`, `TODO.md`. No other files.

---

## Out of Scope (intentionally not addressed)

- TODO L9 — `JobTable.shell_tmodes` is a one-time startup snapshot that runtime `stty` does not refresh. Matches glibc-manual semantics. Revisit only if a user reports surface.
- E2E test for the `exec 0</dev/null` → Ctrl-Z → `fg` PTY trigger. The `expectrl` harness has no clean way to redirect the shell's stdin away from the master fd. Coverage is via the Task 2 unit tests around `record_stopped_state`.
