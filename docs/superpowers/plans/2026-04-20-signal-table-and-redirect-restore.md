# Signal Table Portability + Redirect Self-Heal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix two correctness bugs in `yosh`: (B) signal name↔number lookups wrong on macOS; (C) `RedirectState::apply` leaves fd table corrupted on partial-failure.

**Architecture:**
- B: Replace numeric literals in `src/signal.rs` with `libc::SIG*` `pub const` values so each target platform gets the correct number at compile time.
- C: Make `RedirectState::apply` self-healing — on `apply_one` failure, call `self.restore()` internally so callers never see a half-applied state.

**Tech Stack:** Rust 2024 edition, `libc` crate (already a dependency), `nix` crate (for `sigaction`, unchanged).

**Spec:** `docs/superpowers/specs/2026-04-20-signal-table-and-redirect-restore-design.md`

---

## File Map

| File | Change | Responsibility |
|------|--------|----------------|
| `src/signal.rs` | Modify 20-49 (SIGNAL_TABLE, HANDLED_SIGNALS), add test in the existing `#[cfg(test)] mod tests` block | Replace Linux-centric literals with `libc::SIG*` consts |
| `src/exec/redirect.rs` | Modify `RedirectState::apply` (34-44), add test in the existing `#[cfg(test)] mod tests` block | Roll back partially-applied redirects on error |
| `TODO.md` | Delete 2 lines (one per commit, per project convention) | Remove completed-bug entries |

---

## Task 1: Signal Table Uses libc Constants

**Goal:** `SIGNAL_TABLE` and `HANDLED_SIGNALS` agree with `libc::SIG*` on every target platform (Linux: identical, macOS: fixes USR1/USR2/CHLD/CONT/STOP/TSTP).

**Files:**
- Modify: `src/signal.rs:20-49` (table constants)
- Test: `src/signal.rs` inside existing `#[cfg(test)] mod tests` block
- Modify: `TODO.md` (remove the SIGNAL_TABLE entry)

---

- [ ] **Step 1.1: Add the portability invariant test**

Append to the `#[cfg(test)] mod tests { ... }` block in `src/signal.rs` (after the last existing test, inside the module):

```rust
#[test]
fn test_signal_table_matches_libc_constants() {
    // Portable check: the table must agree with libc on every entry.
    // Pre-fix this would have failed on macOS for USR1/USR2/CHLD/CONT/STOP/TSTP
    // because the table hard-coded Linux signal numbers.
    for &(num, name) in SIGNAL_TABLE {
        let expected = match name {
            "HUP" => libc::SIGHUP,
            "INT" => libc::SIGINT,
            "QUIT" => libc::SIGQUIT,
            "ABRT" => libc::SIGABRT,
            "KILL" => libc::SIGKILL,
            "USR1" => libc::SIGUSR1,
            "USR2" => libc::SIGUSR2,
            "PIPE" => libc::SIGPIPE,
            "ALRM" => libc::SIGALRM,
            "TERM" => libc::SIGTERM,
            "CHLD" => libc::SIGCHLD,
            "CONT" => libc::SIGCONT,
            "STOP" => libc::SIGSTOP,
            "TSTP" => libc::SIGTSTP,
            "TTIN" => libc::SIGTTIN,
            "TTOU" => libc::SIGTTOU,
            other => panic!("unexpected signal name in table: {other}"),
        };
        assert_eq!(
            num, expected,
            "SIGNAL_TABLE entry for {name} has {num}, libc says {expected}"
        );
    }
}

#[test]
fn test_handled_signals_match_libc_constants() {
    for &(num, name) in HANDLED_SIGNALS {
        let expected = match name {
            "HUP" => libc::SIGHUP,
            "INT" => libc::SIGINT,
            "QUIT" => libc::SIGQUIT,
            "ALRM" => libc::SIGALRM,
            "TERM" => libc::SIGTERM,
            "USR1" => libc::SIGUSR1,
            "USR2" => libc::SIGUSR2,
            other => panic!("unexpected signal name in HANDLED_SIGNALS: {other}"),
        };
        assert_eq!(
            num, expected,
            "HANDLED_SIGNALS entry for {name} has {num}, libc says {expected}"
        );
    }
}
```

- [ ] **Step 1.2: Run the new tests — confirm they pass on Linux pre-fix (regression guard only on this platform)**

Run: `cargo test --lib signal::tests::test_signal_table_matches_libc_constants signal::tests::test_handled_signals_match_libc_constants`

Expected on Linux: **PASS** (existing literals happen to match libc on Linux).
Expected on macOS pre-fix: **FAIL** at USR1/USR2/CHLD/CONT/STOP/TSTP.

If on Linux and both tests pass, proceed — the test will pin the invariant after refactor. If on macOS, failure here is expected and confirms the bug; continue to Step 1.3.

- [ ] **Step 1.3: Replace numeric literals in SIGNAL_TABLE with libc constants**

Replace `src/signal.rs:20-38` with:

```rust
/// Full signal table for name/number conversion.
pub const SIGNAL_TABLE: &[(i32, &str)] = &[
    (libc::SIGHUP, "HUP"),
    (libc::SIGINT, "INT"),
    (libc::SIGQUIT, "QUIT"),
    (libc::SIGABRT, "ABRT"),
    (libc::SIGKILL, "KILL"),
    (libc::SIGUSR1, "USR1"),
    (libc::SIGUSR2, "USR2"),
    (libc::SIGPIPE, "PIPE"),
    (libc::SIGALRM, "ALRM"),
    (libc::SIGTERM, "TERM"),
    (libc::SIGCHLD, "CHLD"),
    (libc::SIGCONT, "CONT"),
    (libc::SIGSTOP, "STOP"),
    (libc::SIGTSTP, "TSTP"),
    (libc::SIGTTIN, "TTIN"),
    (libc::SIGTTOU, "TTOU"),
];
```

- [ ] **Step 1.4: Replace numeric literals in HANDLED_SIGNALS**

Replace `src/signal.rs:41-49` with:

```rust
/// Signals for which the shell registers handlers.
pub const HANDLED_SIGNALS: &[(i32, &str)] = &[
    (libc::SIGHUP, "HUP"),
    (libc::SIGINT, "INT"),
    (libc::SIGQUIT, "QUIT"),
    (libc::SIGALRM, "ALRM"),
    (libc::SIGTERM, "TERM"),
    (libc::SIGUSR1, "USR1"),
    (libc::SIGUSR2, "USR2"),
];
```

- [ ] **Step 1.5: Verify both new tests pass, plus all existing signal tests**

Run: `cargo test --lib signal::tests`

Expected: all tests PASS on both Linux and macOS. No test marked FAILED.

- [ ] **Step 1.6: Run full test suite to catch any collateral damage**

Run: `cargo test`

Expected: full suite green. No test regressions.

If any existing test fails, investigate — the refactor should be semantically identical on Linux. On macOS, some tests that were accidentally passing due to the bug canceling out may now fail legitimately; treat them as further bugs to fix in a follow-up TODO entry rather than reverting this change.

- [ ] **Step 1.7: Format check**

Run: `rustfmt --edition 2024 --check src/signal.rs`

Expected: no output (clean). If rustfmt reports diffs, apply with `rustfmt --edition 2024 src/signal.rs` and re-run the check. Do not use `cargo fmt --check -- src/signal.rs` — TODO.md notes a known rustfmt bug where it mis-parses edition 2024 let-chains when invoked that way.

- [ ] **Step 1.8: Remove the SIGNAL_TABLE entry from TODO.md**

Delete the line at `TODO.md` that begins with:

```
- [ ] `SIGNAL_TABLE` Linux-centric numbering — `src/signal.rs:20-37` hard-codes
```

(Full line is the single bullet ending with `…via `libc::SIGUSR1` etc. at runtime (`src/signal.rs`).`)

Per project convention (`CLAUDE.md` TODO.md section): delete completed items rather than marking them with `[x]`.

- [ ] **Step 1.9: Commit**

```bash
git add src/signal.rs TODO.md
git commit -m "$(cat <<'EOF'
fix(signal): resolve SIGNAL_TABLE numbers via libc constants

SIGNAL_TABLE and HANDLED_SIGNALS hard-coded Linux signal numbers,
causing name<->number lookups to be wrong on macOS for USR1/USR2
(10/12 on Linux, 30/31 on macOS) plus CHLD/CONT/STOP/TSTP which also
differ. capture_ignored_on_entry walked the table and would mis-report
a macOS parent that ignored SIGBUS as "ignoring USR1".

Using libc::SIG* pub const values lets rustc pick the correct number
per target platform at compile time with no runtime cost.

Adds regression tests that pin the tables against libc on every target.

Original task: "TODO.md の中から優先度が高いものを対応してください。"
Spec: docs/superpowers/specs/2026-04-20-signal-table-and-redirect-restore-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Expected: commit succeeds, hook passes, `git log -1 --oneline` shows the new commit on `main`.

---

## Task 2: RedirectState::apply Self-Heals on Failure

**Goal:** After `apply` returns — whether `Ok` or `Err` — the shell's fd table is in a consistent state. On `Err`, any partially-applied redirects are rolled back and `saved_fds` is empty.

**Files:**
- Modify: `src/exec/redirect.rs:34-44` (the `apply` method)
- Test: `src/exec/redirect.rs` inside existing `#[cfg(test)] mod tests` block
- Modify: `TODO.md` (remove the Special-builtin redirect-error entry)

---

- [ ] **Step 2.1: Write the failing rollback test**

Append to the `#[cfg(test)] mod tests { ... }` block in `src/exec/redirect.rs` (after the last existing test, inside the module):

```rust
#[test]
fn test_apply_rolls_back_on_second_redirect_failure() {
    // Two redirects: first targets a valid tmp file (fd 1), second targets
    // a path whose parent directory does not exist (fd 2) so open() fails.
    // Pre-fix: saved_fds is non-empty after Err and fd 1 remains dup2'd over
    // the tmp file, so a subsequent libc::write(1, ...) leaks into the tmp file.
    // Post-fix: apply() calls self.restore() internally, saved_fds is empty,
    // and fd 1 points back at the pre-apply target.

    let mut env = make_env();
    let tmp_ok = std::env::temp_dir().join("yosh_apply_rollback_ok.txt");
    // Remove any stale file from a prior test run.
    let _ = std::fs::remove_file(&tmp_ok);
    let bad_path = "/no/such/dir/should-not-exist-yosh-test/file.txt";

    let redirects = vec![
        Redirect {
            fd: Some(1),
            kind: RedirectKind::Output(Word::literal(tmp_ok.to_str().unwrap())),
        },
        Redirect {
            fd: Some(2),
            kind: RedirectKind::Output(Word::literal(bad_path)),
        },
    ];

    // Save original fd 1 outside RedirectState so we can restore it at the end
    // (cargo test captures stdout; we must not leave fd 1 corrupted for sibling tests).
    let orig_stdout = unsafe { libc::dup(1) };
    assert!(orig_stdout >= 0, "dup(1) failed");

    let mut state = RedirectState::new();
    let result = state.apply(&redirects, &mut env, true);
    assert!(result.is_err(), "expected apply to fail on the bad path");

    // Post-condition 1: rollback emptied saved_fds.
    assert!(
        state.saved_fds.is_empty(),
        "saved_fds should be empty after rollback, got {} entries",
        state.saved_fds.len()
    );

    // Post-condition 2: writes to fd 1 should not land in tmp_ok.
    let marker = b"post-rollback-marker\n";
    unsafe {
        libc::write(1, marker.as_ptr() as *const _, marker.len());
    }

    let written = std::fs::read_to_string(&tmp_ok).unwrap_or_default();

    // Cleanup BEFORE assertion so a failure still cleans up.
    unsafe {
        libc::dup2(orig_stdout, 1);
        libc::close(orig_stdout);
    }
    let _ = std::fs::remove_file(&tmp_ok);

    assert!(
        !written.contains("post-rollback-marker"),
        "fd 1 should not still point at tmp_ok after rollback; tmp_ok contained: {written:?}"
    );
}
```

- [ ] **Step 2.2: Run the new test — confirm it FAILS without the fix**

Run: `cargo test --lib exec::redirect::tests::test_apply_rolls_back_on_second_redirect_failure -- --nocapture`

Expected: **FAIL**. One of these two assertions should trip:
- `saved_fds should be empty after rollback` (saved_fds has 1 entry pre-fix), or
- `fd 1 should not still point at tmp_ok` (the marker was written into tmp_ok because fd 1 is still dup2'd).

If the test unexpectedly passes, stop and investigate — the fixture's "bad path" may be accidentally valid on this machine, or sibling tests may have already left fd 1 corrupted.

- [ ] **Step 2.3: Modify `apply` to roll back on failure**

Replace `src/exec/redirect.rs:34-44` (the `apply` method) with:

```rust
    /// Apply a list of redirects.
    /// If `save` is true (builtin case), save the original fds so they can be restored.
    ///
    /// On failure, any redirects already applied within this call are rolled back
    /// (via `self.restore()`), so the returned `Err` always reports a state where
    /// the caller's fd table is unchanged. `save=false` leaves `saved_fds` empty,
    /// so the rollback is a no-op in that case.
    pub fn apply(
        &mut self,
        redirects: &[Redirect],
        env: &mut ShellEnv,
        save: bool,
    ) -> Result<(), String> {
        for redirect in redirects {
            if let Err(e) = self.apply_one(redirect, env, save) {
                self.restore();
                return Err(e);
            }
        }
        Ok(())
    }
```

- [ ] **Step 2.4: Run the new test — confirm it now PASSES**

Run: `cargo test --lib exec::redirect::tests::test_apply_rolls_back_on_second_redirect_failure`

Expected: **PASS**.

- [ ] **Step 2.5: Run all redirect module tests**

Run: `cargo test --lib exec::redirect::tests`

Expected: all tests PASS, including the pre-existing `test_redirect_output_and_restore` and `test_redirect_input`.

- [ ] **Step 2.6: Run full test suite to catch regressions**

Run: `cargo test`

Expected: full suite green. In particular, the Special-builtin / Regular-builtin / `command` / fg/bg/jobs error paths in `src/exec/simple.rs` now receive a `RedirectState` whose `saved_fds` is empty — subsequent `restore_assignments(saved)` + `return Err(...)` unchanged. Double-restore protection: if any future caller adds a redundant `.restore()` after a failed `apply`, it is a harmless no-op on an empty vec.

- [ ] **Step 2.7: Format check**

Run: `rustfmt --edition 2024 --check src/exec/redirect.rs`

Expected: no output (clean). If diffs reported, apply with `rustfmt --edition 2024 src/exec/redirect.rs` and re-run.

- [ ] **Step 2.8: Remove the Special-builtin redirect-error entry from TODO.md**

Delete the line at `TODO.md` that begins with:

```
- [ ] Special-builtin redirect-error early-return omits `redirect_state.restore()`
```

(Full line ends with `…audit and fix consistently (`src/exec/simple.rs`).`)

- [ ] **Step 2.9: Commit**

```bash
git add src/exec/redirect.rs TODO.md
git commit -m "$(cat <<'EOF'
fix(exec/redirect): self-heal apply() on partial-failure

RedirectState::apply iterates redirects and returns Err on the first
failure. Previously, already-saved fds were left in saved_fds and the
target fds stayed dup2'd over whatever the successful redirects opened,
because the four call sites in src/exec/simple.rs (fg/bg/jobs, command,
Special builtin, Regular builtin) all early-return without calling
restore(), and Drop only closes saved copies without dup2-ing them back.

apply() now calls self.restore() internally before returning Err, so
every caller sees a clean state regardless of outcome. save=false paths
(exec no-args) are unaffected because saved_fds stays empty.

Adds regression test that triggers a two-redirect partial failure and
verifies (a) saved_fds is empty after Err and (b) fd 1 no longer points
at the first redirect's file.

Original task: "TODO.md の中から優先度が高いものを対応してください。"
Spec: docs/superpowers/specs/2026-04-20-signal-table-and-redirect-restore-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Expected: commit succeeds.

---

## Task 3: Final Verification

- [ ] **Step 3.1: Confirm both commits landed cleanly**

Run: `git log --oneline -5`

Expected: top two commits are (in order) the redirect-restore fix then the signal-table fix, both on `main`.

- [ ] **Step 3.2: Full test suite one more time**

Run: `cargo test`

Expected: clean pass.

- [ ] **Step 3.3: Confirm TODO.md has 2 fewer lines**

Run: `grep -c "SIGNAL_TABLE Linux-centric" TODO.md; grep -c "Special-builtin redirect-error" TODO.md`

Expected: both commands output `0`.

---

## Self-Review Notes

**Spec coverage:**
- B (SIGNAL_TABLE fix) → Task 1 ✓
- C (RedirectState self-heal) → Task 2 ✓
- Testing for B (libc invariant test) → Step 1.1 ✓
- Testing for C (partial-failure rollback test) → Step 2.1 ✓
- TODO.md cleanup per commit → Steps 1.8 / 2.8 ✓
- Commit granularity (two separate commits) → Steps 1.9 / 2.9 ✓

**Placeholder scan:** No TBD/TODO/"appropriate"/"handle edge cases" placeholders. Every code block is the literal insertion.

**Type consistency:** `SIGNAL_TABLE: &[(i32, &str)]` signature preserved in Task 1. `RedirectState::apply` signature preserved in Task 2 (pub modifier, same `Result<(), String>` return).

**Out-of-scope items (noted in spec, deferred to new TODO.md entries if desired):**
- Auditing non-`src/exec/simple.rs` callers of `apply` — now protected by the self-healing contract, but explicit audit is recommended.
- Extending `SIGNAL_TABLE` with SIGBUS/SIGIO/SIGPROF/SIGWINCH.
- macOS CI job.

These are not tasks in this plan; they can be added to TODO.md by a follow-up commit if the user wants.
