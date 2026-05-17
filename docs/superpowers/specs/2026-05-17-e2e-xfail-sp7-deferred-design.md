# E2E XFAIL SP7 — Deferred / Known Deviation Closure

**Date:** 2026-05-17
**Status:** Design (closure-only, no source changes)
**Type:** Documentation
**Parent roadmap:** [`2026-05-13-e2e-xfail-roadmap-design.md`](./2026-05-13-e2e-xfail-roadmap-design.md)

## 1. Background

Sub-project 7 (SP7) is the final phase of the E2E XFAIL roadmap. After
SP1–SP6 closed 52 of the original 55 XFAIL tests by implementing
behavior or migrating to the PTY harness, three tests remain that the
roadmap explicitly defers:

- `e2e/posix_spec/4_required_builtin/ulimit_unknown_option.sh`
- `e2e/posix_spec/8_env_vars/LANG_default_collate.sh`
- `e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh`

The roadmap §SP7 prescribes that each remaining XFAIL be rewritten to
either `deferred (…)` or `known POSIX deviation (…)` form, with a
matching rationale paragraph in `TODO.md`. No source changes are
required; the tests remain XFAIL.

## 2. Scope

In scope:

- Rewrite the `# XFAIL: …` line on the three test files to the
  roadmap-prescribed wording.
- Add a `TODO.md` entry for the `trap_resets_in_subshell` case under
  `## Future: POSIX Conformance Bugs` (the only missing rationale; the
  ulimit and locale entries already exist).
- Delete the `- [ ] SP7 — Deferred / recorded as known deviation (3
  tests)` line from `TODO.md`. The roadmap is fully closed once SP7
  lands, but the `### SP1 follow-ups (non-blocking)` through
  `### SP6 follow-ups (non-blocking)` subsections that live under the
  same `##` heading are still active technical-debt items that must
  remain. Therefore:
    - Rename `## E2E XFAIL Roadmap` to `## E2E XFAIL Roadmap Follow-ups`
      to truthfully reflect the post-closure contents.
    - Replace the existing intro paragraph (`Decomposition of 55 XFAIL
      tests into 7 sub-projects. See ...`) with a one-line closure
      marker: `Roadmap closed 2026-05-17. Non-blocking follow-ups from
      SP1–SP6 retained below for tracking.`
    - Delete only the SP7 line; leave the `### SPN follow-ups` blocks
      untouched.
- Commit this design spec under `docs/superpowers/specs/`.

Out of scope:

- Editing the existing TODO.md entries for ulimit (§Future: POSIX
  Required Builtin Implementation) and LANG/locale (§Future: POSIX
  Conformance Bugs). Both already record the deferral rationale.
- Any change to `src/`, `tests/`, or `e2e/run_tests.sh`.
- Implementing `ulimit`, locale support, or trap-reset semantics —
  these are the very things SP7 records as deferred.

## 3. Changes

### 3.1 `e2e/posix_spec/4_required_builtin/ulimit_unknown_option.sh`

```diff
-# XFAIL: not yet implemented (TODO: implement ulimit)
+# XFAIL: deferred (TODO: implement ulimit; out of scope for v0.x — tracked in TODO.md)
```

### 3.2 `e2e/posix_spec/8_env_vars/LANG_default_collate.sh`

```diff
-# XFAIL: not yet implemented (TODO: implement locale handling; yosh has no locale support yet)
+# XFAIL: deferred (TODO: locale support — tracked in TODO.md)
```

### 3.3 `e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh`

```diff
-# XFAIL: harness limitation (POSIX trap-in-subshell semantics differ across shells; spec interpretation in flux)
+# XFAIL: known POSIX deviation (trap reset in subshell — interpretation varies across shells)
```

### 3.4 `TODO.md` — append to `## Future: POSIX Conformance Bugs`

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

### 3.5 `TODO.md` — close the roadmap, retain follow-ups

Three coordinated edits inside the existing top section:

1. Rename the `## E2E XFAIL Roadmap` heading to
   `## E2E XFAIL Roadmap Follow-ups`.
2. Replace the existing two-line intro paragraph

   ```markdown
   Decomposition of 55 XFAIL tests into 7 sub-projects. See
   `docs/superpowers/specs/2026-05-13-e2e-xfail-roadmap-design.md`.
   ```

   with

   ```markdown
   Roadmap closed 2026-05-17. Non-blocking follow-ups from SP1–SP6
   retained below for tracking.
   ```

3. Delete the `- [ ] SP7 — Deferred / recorded as known deviation (3
   tests)` line.

The `### SP1 follow-ups (non-blocking)` through
`### SP6 follow-ups (non-blocking)` subsections that live under this
heading are unaffected — they are active technical-debt items and
remain in place.

## 4. Acceptance criteria

1. The three test files carry the exact XFAIL wording from §3.1–§3.3
   (no other lines changed).
2. `./e2e/run_tests.sh` reports the same counts as before this work:
   `Total: 797 Passed: 784 Failed: 0 Timedout: 0 XFail: 3 Migrated: 10 XPass: 0`.
3. `cargo build` and `cargo test` stay green (sanity, no source
   changes expected to affect them).
4. `TODO.md` contains the new `Subshell trap reset for uncaught
   signals` entry under `## Future: POSIX Conformance Bugs`.
5. `TODO.md` heading is renamed to
   `## E2E XFAIL Roadmap Follow-ups`, the intro paragraph carries the
   closure marker, the `SP7 — Deferred / recorded as known deviation`
   line is gone, and the `### SPN follow-ups (non-blocking)` blocks
   are unchanged.
6. This spec is committed under `docs/superpowers/specs/`.
7. The `project_e2e_xfail_roadmap` auto-memory entry is updated to
   reflect "SP7 COMPLETE (2026-05-17); roadmap fully closed".

## 5. Test plan

1. Pre-change baseline: confirm `./e2e/run_tests.sh` reports `XFail: 3
   Migrated: 10` (already verified at design time).
2. Per-test re-run after edits: each of the three filenames continues
   to report `XFAIL` (no XPASS regression).
3. Full E2E run: counts identical to baseline.
4. `cargo build` + `cargo test` green.
5. Grep checks on `TODO.md`:
   - `grep -c "SP7" TODO.md` → 0
   - `grep -c "^## E2E XFAIL Roadmap Follow-ups$" TODO.md` → 1
   - `grep -c "Roadmap closed 2026-05-17" TODO.md` → 1
   - `grep -c "Subshell trap reset for uncaught signals" TODO.md` → 1
   - `grep -c "^### SP1 follow-ups" TODO.md` → 1 (sanity: SP1 block preserved)
   - `grep -c "^### SP6 follow-ups" TODO.md` → 1 (sanity: SP6 block preserved)

## 6. Commit shape

Single commit (documentation-only). Includes the spec, the three
test-file edits, and the `TODO.md` changes together. There is no
implementation phase to separate, so further decomposition would add
overhead without value. Suggested message shape, consistent with the
SP-closure commits from SP1–SP6:

```
chore(sp7): close SP7 — record 3 XFAILs as deferred / known deviation
```

## 7. Acceptance for roadmap closure

After SP7 lands, the parent roadmap
(`2026-05-13-e2e-xfail-roadmap-design.md` §6 "Acceptance Criterion")
is satisfied: zero XFAIL in SP1–SP6, three documented XFAILs in SP7,
each backed by a `TODO.md` rationale.
