# POSIX Subshell Trap Reset Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Clear `TrapStore::saved_traps` in non-command-sub subshells (literal `(...)`, pipeline child, background `&`) so `trap` builtin inside such subshells reflects post-reset state instead of the enclosing command-sub's parent snapshot. Closes the only remaining trap-reset XFAIL.

**Architecture:** Add a new `TrapStore::reset_for_subshell` method that wraps `reset_non_ignored()` and additionally clears `saved_traps`. Replace `reset_non_ignored` with `reset_for_subshell` at the three fork-time call sites that are not command substitution. `reset_for_command_sub` (`$(...)`) is unchanged so POSIX §2.14 RATIONALE save/restore pattern is preserved.

**Tech Stack:** Rust 2024 edition, `nix` for fork, `cargo test` for unit/integration, `./e2e/run_tests.sh` for E2E.

**Spec:** `docs/superpowers/specs/2026-05-19-subshell-trap-reset-design.md`

---

## File Structure

**Created files:** none. All changes modify existing files.

**Modified files:**

| Path | Responsibility | Change |
|---|---|---|
| `src/env/traps.rs` | `TrapStore` API + unit tests | Add `reset_for_subshell` + `saved_traps_is_some` test helper + 3 unit tests |
| `src/exec/compound.rs` | `exec_subshell` (literal `(...)`) | One-line method swap |
| `src/exec/pipeline.rs` | pipeline child branch | One-line method swap |
| `src/exec/control.rs` | `exec_async` (background `&`) | One-line method swap |
| `tests/subshell.rs` | subshell integration tests | Add 2 new tests |
| `e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh` | XFAIL test → PASS test | Strip `# XFAIL:` line |
| `TODO.md` | tracking | Remove trap-reset entry from §Future: POSIX Conformance Bugs |

The spec mandates **two commits** total: (1) the design spec (already landed in commit `9ec4799`), and (2) one implementation commit bundling all source/test/XFAIL/TODO changes. All tasks below stage files but defer the actual `git commit` to the final task.

---

## Task 1: Add `saved_traps_is_some` test helper and first failing unit test

**Files:**
- Modify: `src/env/traps.rs` (insert helper after line 119, insert test at end of `mod tests`)

- [ ] **Step 1.1: Add `saved_traps_is_some` test-only inspection helper**

Open `src/env/traps.rs`. After the existing `reset_non_ignored` method (which ends at line 119), add this helper inside the same `impl TrapStore` block. Place it directly after `reset_non_ignored` and before `reset_for_command_sub`:

```rust
    /// Test-only helper: inspect whether `saved_traps` is populated.
    /// Avoids bumping the field's visibility purely for unit tests.
    #[cfg(test)]
    pub(crate) fn saved_traps_is_some(&self) -> bool {
        self.saved_traps.is_some()
    }
```

- [ ] **Step 1.2: Add the first failing unit test**

Inside the `mod tests` block in `src/env/traps.rs` (the existing block beginning with `#[cfg(test)] mod tests {` around line 189), append this test at the bottom of the module (after the last existing test, just before the closing `}` of `mod tests`):

```rust
    #[test]
    fn test_reset_for_subshell_clears_saved_traps() {
        let mut store = TrapStore::default();
        store
            .set_trap("INT", TrapAction::Command("echo parent".into()))
            .unwrap();
        store.reset_for_command_sub();
        assert!(
            store.saved_traps_is_some(),
            "precondition: reset_for_command_sub must populate saved_traps"
        );
        store.reset_for_subshell();
        assert!(
            !store.saved_traps_is_some(),
            "saved_traps must be cleared by reset_for_subshell"
        );
        assert!(
            store.signal_traps.is_empty(),
            "signal_traps must be reset by reset_for_subshell"
        );
    }
```

- [ ] **Step 1.3: Run the test — expect compilation failure**

Run: `cargo test --lib env::traps::tests::test_reset_for_subshell_clears_saved_traps 2>&1 | tail -20`

Expected: compile error — `no method named 'reset_for_subshell' found for struct 'TrapStore'`. This confirms the test is wired correctly and pins behavior before the implementation exists.

---

## Task 2: Implement `TrapStore::reset_for_subshell`

**Files:**
- Modify: `src/env/traps.rs` (insert method after `reset_non_ignored`)

- [ ] **Step 2.1: Add the `reset_for_subshell` method**

In `src/env/traps.rs`, insert the following method inside `impl TrapStore` directly after `reset_non_ignored` (right after the closing `}` of `reset_non_ignored` at line 119, before the `saved_traps_is_some` helper added in Task 1):

```rust
    /// Reset traps for an immediate subshell (literal `(...)`, pipeline child,
    /// background `&`). Clears `saved_traps` so a nested `trap` builtin reflects
    /// the post-reset state of the subshell rather than an enclosing
    /// command-substitution's parent snapshot.
    ///
    /// Distinct from [`Self::reset_for_command_sub`]: command substitution
    /// (`$(...)`) preserves parent traps in `saved_traps` for the POSIX
    /// `traps=$(trap)` save/restore pattern (POSIX §2.14 RATIONALE), but
    /// non-command-sub subshells must show their own (reset) state.
    pub fn reset_for_subshell(&mut self) {
        self.reset_non_ignored();
        self.saved_traps = None;
    }
```

- [ ] **Step 2.2: Run the test — expect PASS**

Run: `cargo test --lib env::traps::tests::test_reset_for_subshell_clears_saved_traps 2>&1 | tail -20`

Expected: `test test_reset_for_subshell_clears_saved_traps ... ok` and overall `1 passed; 0 failed`.

---

## Task 3: Add remaining unit tests for `reset_for_subshell`

**Files:**
- Modify: `src/env/traps.rs` (append tests to `mod tests`)

- [ ] **Step 3.1: Add Ignore-preservation test**

Append the following test inside `mod tests` directly after `test_reset_for_subshell_clears_saved_traps`:

```rust
    #[test]
    fn test_reset_for_subshell_preserves_ignored() {
        // POSIX §2.11: Ignore-action traps must survive subshell entry.
        let mut store = TrapStore::default();
        store.set_trap("HUP", TrapAction::Ignore).unwrap();
        store
            .set_trap("INT", TrapAction::Command("x".into()))
            .unwrap();
        store.reset_for_subshell();
        assert_eq!(
            store.signal_traps.get(&1),
            Some(&TrapAction::Ignore),
            "HUP Ignore must be preserved"
        );
        assert!(
            !store.signal_traps.contains_key(&2),
            "INT Command must be cleared"
        );
    }
```

- [ ] **Step 3.2: Add safe-when-no-saved-traps test**

Append directly after the previous test:

```rust
    #[test]
    fn test_reset_for_subshell_with_no_saved_traps_is_safe() {
        // Calling reset_for_subshell on a store without a prior
        // reset_for_command_sub must not panic and must reset signal_traps.
        let mut store = TrapStore::default();
        store
            .set_trap("INT", TrapAction::Command("x".into()))
            .unwrap();
        // saved_traps is None at this point.
        store.reset_for_subshell();
        assert!(!store.saved_traps_is_some());
        assert!(store.signal_traps.is_empty());
    }
```

- [ ] **Step 3.3: Run all `traps` unit tests**

Run: `cargo test --lib env::traps:: 2>&1 | tail -25`

Expected: all `traps::tests::test_*` PASS, including the three new tests. Existing tests (`test_trap_store_*`, `test_set_trap_with_*`, `test_remove_trap_with_*`) must remain green.

---

## Task 4: Switch `exec_subshell` to `reset_for_subshell`

**Files:**
- Modify: `src/exec/compound.rs:103`

- [ ] **Step 4.1: Replace the call**

In `src/exec/compound.rs`, locate line 103 inside `exec_subshell`:

```rust
                self.env.traps.reset_non_ignored();
```

Replace with:

```rust
                self.env.traps.reset_for_subshell();
```

Surrounding context (the child branch of `match unsafe { fork() }` in `exec_subshell`) is otherwise unchanged.

- [ ] **Step 4.2: Run existing subshell unit tests**

Run: `cargo test --lib exec::compound:: 2>&1 | tail -20`

Expected: all `exec::compound` unit tests PASS (no regression from the call-site swap).

---

## Task 5: Switch pipeline child to `reset_for_subshell`

**Files:**
- Modify: `src/exec/pipeline.rs:79`

- [ ] **Step 5.1: Replace the call**

In `src/exec/pipeline.rs`, locate line 79 inside the `Ok(ForkResult::Child)` branch of the pipeline fork loop:

```rust
                    self.env.traps.reset_non_ignored();
```

Replace with:

```rust
                    self.env.traps.reset_for_subshell();
```

Surrounding context (the `ignored_signals()` capture on the prior line and the `setup_foreground_child_signals`/`reset_child_signals` branch on the next line) is unchanged.

- [ ] **Step 5.2: Run pipeline unit tests**

Run: `cargo test --lib exec::pipeline:: 2>&1 | tail -15`

Expected: all pipeline unit tests PASS.

---

## Task 6: Switch `exec_async` to `reset_for_subshell`

**Files:**
- Modify: `src/exec/control.rs:128`

- [ ] **Step 6.1: Replace the call**

In `src/exec/control.rs`, locate line 128 inside `exec_async`'s `Ok(ForkResult::Child)` branch:

```rust
                self.env.traps.reset_non_ignored();
```

Replace with:

```rust
                self.env.traps.reset_for_subshell();
```

Surrounding context (the `ignored_signals()` capture, the `monitor` mode branch, the comment about `setpgid` isolation, and the trailing `exit_child(status)` call) is unchanged.

- [ ] **Step 6.2: Run control unit tests**

Run: `cargo test --lib exec::control:: 2>&1 | tail -15`

Expected: all `exec::control` unit tests PASS.

---

## Task 7: Add integration tests for nested-cmdsub and pipeline scenarios

**Files:**
- Modify: `tests/subshell.rs` (append two new `#[test]` functions)

- [ ] **Step 7.1: Locate the insertion point**

In `tests/subshell.rs`, find `fn test_cmdsub_trap_isolation()` (around line 236). The two new tests will be appended after this function. They belong in the "command substitution" section since they exercise `$(...)`-related behavior.

- [ ] **Step 7.2: Append the nested-cmdsub test**

Directly after the closing `}` of `test_cmdsub_trap_isolation`, append:

```rust
#[test]
fn test_nested_subshell_inside_cmdsub_shows_reset_traps() {
    // POSIX §2.11: a literal (...) subshell nested inside $(...) must
    // show its own reset trap state, not the enclosing command-sub's
    // parent snapshot. Closes XFAIL trap_resets_in_subshell_when_unhandled.
    let out = yosh_exec(
        "trap 'echo parent' USR1; out=$( (trap) ); \
         case \"$out\" in *USR1*) echo bad ;; *) echo ok ;; esac",
    );
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
}

#[test]
fn test_pipeline_child_clears_saved_traps() {
    // Pipeline child must also clear saved_traps so `(trap)` on the
    // right side of a pipe inside $(...) shows the reset state.
    let out = yosh_exec(
        "trap 'echo parent' USR1; \
         out=$(echo dummy | (trap)); \
         case \"$out\" in *USR1*) echo bad ;; *) echo ok ;; esac",
    );
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
}
```

- [ ] **Step 7.3: Build and run the new tests**

The integration-test crate requires the binary to be rebuilt first because it shells out to the compiled `yosh`:

Run: `cargo test --test subshell test_nested_subshell_inside_cmdsub_shows_reset_traps test_pipeline_child_clears_saved_traps 2>&1 | tail -20`

Expected: both tests PASS.

- [ ] **Step 7.4: Run the full `subshell` integration suite**

Run: `cargo test --test subshell 2>&1 | tail -15`

Expected: every test in `tests/subshell.rs` PASS, including the existing `test_subshell_trap_command_reset`, `test_subshell_trap_ignore_inherited`, and `test_cmdsub_trap_isolation`.

---

## Task 8: Strip XFAIL from the closure E2E test

**Files:**
- Modify: `e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh`

- [ ] **Step 8.1: Remove the XFAIL line**

Open `e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh`. The current contents are:

```sh
#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: Subshell starts with traps reset to default for signals not caught in parent
# XFAIL: known POSIX deviation (trap reset in subshell — interpretation varies across shells)
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
trap 'echo parent' USR1
out=$( (trap) )
case "$out" in
    *USR1*) echo unexpected ;;
    *) echo ok ;;
esac
```

Delete only the `# XFAIL:` line (line 4). The file should become:

```sh
#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: Subshell starts with traps reset to default for signals not caught in parent
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
trap 'echo parent' USR1
out=$( (trap) )
case "$out" in
    *USR1*) echo unexpected ;;
    *) echo ok ;;
esac
```

- [ ] **Step 8.2: Rebuild yosh so the runner picks up the source changes**

The E2E runner does NOT auto-rebuild:

Run: `cargo build 2>&1 | tail -5`

Expected: clean build (no warnings).

- [ ] **Step 8.3: Run the previously-XFAIL test**

Run: `./e2e/run_tests.sh --filter=trap_resets_in_subshell_when_unhandled 2>&1 | tail -10`

Expected: `Passed: 1 Failed: 0 ... XFail: 0`. The test now PASSes (not XPASSes — the XFAIL marker is gone).

---

## Task 9: Remove TODO.md entry

**Files:**
- Modify: `TODO.md` (delete 9-line entry inside §Future: POSIX Conformance Bugs)

- [ ] **Step 9.1: Delete the trap-reset bullet**

In `TODO.md`, locate the `## Future: POSIX Conformance Bugs` section. The current contents include this 9-line entry that must be deleted (lines 388-395 at time of writing):

```markdown
- [ ] Subshell trap reset for uncaught signals not implemented — yosh
      inherits the parent's `trap 'cmd' SIG` action into subshells
      instead of resetting non-caught signals to their default action.
      POSIX §2.11 leaves the precise semantics open to interpretation
      and major shells (bash, dash, ksh) diverge on which signals are
      reset and when. Recorded as a known deviation rather than a fix
      target. XFAIL test:
      `e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh`.
```

Delete the entire bullet (all 8 indented lines plus the leading `- [ ]` line). After the edit, the only entry remaining under `## Future: POSIX Conformance Bugs` should be the locale entry:

```markdown
- [ ] Locale support not implemented — `LANG` / `LC_*` / `NLSPATH` are
      accepted as variables but do not affect collation, character
      classification, message localization, or message catalogs.
      XFAIL test:
      `e2e/posix_spec/8_env_vars/LANG_default_collate.sh` (other
      `LC_*` tests currently pass via default-C-locale semantics).
```

Also update the intro paragraph two lines above, which currently reads:

```markdown
The 2026-05-13 Ch4+Ch8 E2E expansion surfaced 16 POSIX shall/must
deviations as XFAILs. SP1–SP5 fixed all but the two below, which SP7
(2026-05-17) recorded as deferred / known deviation. Each remaining
entry points to the XFAIL test that documents the expected POSIX
behavior.
```

Replace with:

```markdown
The 2026-05-13 Ch4+Ch8 E2E expansion surfaced 16 POSIX shall/must
deviations as XFAILs. SP1–SP5 plus the 2026-05-19 trap-reset fix
closed all but the locale entry below. Each remaining entry points
to the XFAIL test that documents the expected POSIX behavior.
```

- [ ] **Step 9.2: Sanity-check the section**

Run: `grep -A 30 '^## Future: POSIX Conformance Bugs' TODO.md`

Expected: the section header, the (updated) intro paragraph, and a single bullet about Locale. No mention of "Subshell trap reset" or "trap_resets_in_subshell".

---

## Task 10: Full acceptance run and commit

**Files:**
- Stage and commit all modifications from Tasks 1-9.

- [ ] **Step 10.1: Build clean**

Run: `cargo build 2>&1 | tail -5`

Expected: clean build, no warnings.

- [ ] **Step 10.2: Clippy gate**

Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -20`

Expected: no warnings, no errors.

- [ ] **Step 10.3: Full unit + integration test run**

Run: `cargo test 2>&1 | tail -30`

Expected: all tests PASS. New unit tests in `env::traps::tests` and new integration tests in `tests/subshell.rs` are visible in the output.

If `cargo test` times out at the default 2 min, rerun in the background (`run_in_background: true` for the Bash tool) and let it complete; full suite typically finishes within 3-7 min on this machine.

- [ ] **Step 10.4: Full E2E run**

Run: `./e2e/run_tests.sh 2>&1 | tail -10`

Expected: `Total: 797 Passed: 785 Failed: 0 Timedout: 0 XFail: 2 Migrated: 10 XPass: 0` (XFail dropped from 3 to 2; Passed went up by 1).

- [ ] **Step 10.5: Verify TODO.md cleanup**

Run these checks in parallel:

```bash
grep -c "Subshell trap reset" TODO.md
grep -c "trap_resets_in_subshell" TODO.md
grep -c "^## Future: POSIX Conformance Bugs$" TODO.md
grep -A 1 "Roadmap closed" TODO.md
```

Expected output:
- First two: `0` (entry removed)
- Third: `1` (section header still present)
- Fourth: any non-empty line (roadmap-closed intro untouched)

- [ ] **Step 10.6: Stage all files**

Run:

```bash
git add src/env/traps.rs \
        src/exec/compound.rs \
        src/exec/pipeline.rs \
        src/exec/control.rs \
        tests/subshell.rs \
        e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh \
        TODO.md
git status
```

Expected: 7 files staged, no other modified files in the working tree.

- [ ] **Step 10.7: Commit**

Run:

```bash
git commit -m "$(cat <<'EOF'
fix(exec): clear saved_traps in literal subshell/pipeline/async forks

POSIX §2.11 requires non-ignored traps to be reset on subshell entry,
and yosh already does so via TrapStore::reset_non_ignored. However,
when a literal (...) subshell is nested inside $(...), the inner
fork inherited saved_traps from the outer command-sub's
reset_for_command_sub snapshot, causing trap (no args) to display the
parent's traps instead of the inner subshell's (reset) state.

Add TrapStore::reset_for_subshell that wraps reset_non_ignored and
additionally clears saved_traps. Wire it into exec_subshell,
pipeline-child, and exec_async fork branches. The $(...) path keeps
reset_for_command_sub so POSIX §2.14 RATIONALE traps=$(trap)
save/restore pattern still works.

Closes the last XFAIL recorded by SP7 as a known POSIX deviation:
e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh.

Spec: docs/superpowers/specs/2026-05-19-subshell-trap-reset-design.md
Task: brainstormed from "TODO.md の Future: POSIX Conformance Bugs を対応して"
(trap reset only; locale support deferred to a separate spec).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

Expected: commit succeeds, `git status` clean.

- [ ] **Step 10.8: Confirm final state**

Run: `git log --oneline -3`

Expected:
- HEAD: `fix(exec): clear saved_traps in literal subshell/pipeline/async forks`
- HEAD~1: `docs(trap-reset): add design spec for POSIX subshell trap reset` (already landed)
- HEAD~2: prior commit on `main`

---

## Self-Review Notes

**Spec coverage:** Each numbered section in `2026-05-19-subshell-trap-reset-design.md` maps to plan tasks:
- §3.1 (new method) → Task 2
- §3.2 (3 call-site replacements) → Tasks 4, 5, 6
- §4.2 (3 unit tests + helper) → Tasks 1, 3
- §4.3 (2 integration tests) → Task 7
- §4.4 (XFAIL strip + TODO cleanup) → Tasks 8, 9
- §5 (acceptance criteria) → Task 10
- §6 (two-commit shape: spec already in 9ec4799 + one impl commit) → Task 10.7

**Placeholder scan:** No "TBD", "TODO", "fill in", or "similar to" placeholders. All code blocks contain the actual text the engineer types/pastes.

**Type / name consistency:**
- Method name `reset_for_subshell` is identical across spec, tests, and all three call sites.
- Helper name `saved_traps_is_some` is consistent between Task 1 (definition) and Task 3 (usage).
- Field name `signal_traps` matches existing source.
- `TrapAction::Command(..)` / `TrapAction::Ignore` enum variants match `src/env/traps.rs:5`.

**TDD ordering:** Task 1 writes the failing test before Task 2 implements. Tasks 3, 7 add additional tests that should pass immediately after the implementation in Task 2 (and the call-site swaps in Tasks 4-6, respectively).

**Commit discipline:** The spec mandates a single implementation commit. All file modifications stage progressively across Tasks 1-9, and Task 10 performs the single commit after the full acceptance run.
