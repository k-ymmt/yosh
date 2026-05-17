# SP7 — Defer ulimit / LANG / trap-reset XFAILs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close SP7 of the E2E XFAIL roadmap by rewriting three XFAIL comments to deferred / known-deviation form, recording the missing rationale in `TODO.md`, and marking the roadmap itself as closed — with no source changes.

**Architecture:** Pure documentation pass. Three E2E test files get one-line edits; `TODO.md` gets one new entry under `## Future: POSIX Conformance Bugs`, a heading rename, an intro replacement, and the SP7 line removed. Single commit. The implementation plan is "TDD-shaped" only in that pre-/post-counts on `./e2e/run_tests.sh` and `cargo test` act as the verification gate.

**Tech Stack:** Plain Markdown / shell-script comments. No code.

**Spec:** [`docs/superpowers/specs/2026-05-17-e2e-xfail-sp7-deferred-design.md`](../specs/2026-05-17-e2e-xfail-sp7-deferred-design.md)

---

## File Structure

Files touched in this plan:

- Modify: `e2e/posix_spec/4_required_builtin/ulimit_unknown_option.sh`
  (1 line — XFAIL comment)
- Modify: `e2e/posix_spec/8_env_vars/LANG_default_collate.sh`
  (1 line — XFAIL comment)
- Modify: `e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh`
  (1 line — XFAIL comment)
- Modify: `TODO.md`
  (heading rename, intro paragraph replacement, SP7 line deletion, new
  entry append under `## Future: POSIX Conformance Bugs`)
- Modify: `/Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/project_e2e_xfail_roadmap.md`
  (status update to "SP7 COMPLETE 2026-05-17, roadmap closed")

No new files. No `src/` touched. No tests created.

---

## Task 1: Update the three E2E XFAIL lines

**Files:**
- Modify: `e2e/posix_spec/4_required_builtin/ulimit_unknown_option.sh`
- Modify: `e2e/posix_spec/8_env_vars/LANG_default_collate.sh`
- Modify: `e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh`

- [ ] **Step 1: Confirm baseline E2E counts**

Run:
```bash
./e2e/run_tests.sh 2>&1 | tail -2
```
Expected: `Total: 797  Passed: 784  Failed: 0  Timedout: 0  XFail: 3  Migrated: 10  XPass: 0`

If counts differ, STOP — investigate before continuing. Plan assumes
this baseline.

- [ ] **Step 2: Edit `ulimit_unknown_option.sh`**

Replace the existing `# XFAIL:` line with the new wording. Exact diff:
```diff
-# XFAIL: not yet implemented (TODO: implement ulimit)
+# XFAIL: deferred (TODO: implement ulimit; out of scope for v0.x — tracked in TODO.md)
```

Use the Edit tool with `old_string`:
```
# XFAIL: not yet implemented (TODO: implement ulimit)
```
and `new_string`:
```
# XFAIL: deferred (TODO: implement ulimit; out of scope for v0.x — tracked in TODO.md)
```

- [ ] **Step 3: Edit `LANG_default_collate.sh`**

Replace the existing `# XFAIL:` line. Exact diff:
```diff
-# XFAIL: not yet implemented (TODO: implement locale handling; yosh has no locale support yet)
+# XFAIL: deferred (TODO: locale support — tracked in TODO.md)
```

Use Edit with `old_string`:
```
# XFAIL: not yet implemented (TODO: implement locale handling; yosh has no locale support yet)
```
and `new_string`:
```
# XFAIL: deferred (TODO: locale support — tracked in TODO.md)
```

- [ ] **Step 4: Edit `trap_resets_in_subshell_when_unhandled.sh`**

Replace the existing `# XFAIL:` line. Exact diff:
```diff
-# XFAIL: harness limitation (POSIX trap-in-subshell semantics differ across shells; spec interpretation in flux)
+# XFAIL: known POSIX deviation (trap reset in subshell — interpretation varies across shells)
```

Use Edit with `old_string`:
```
# XFAIL: harness limitation (POSIX trap-in-subshell semantics differ across shells; spec interpretation in flux)
```
and `new_string`:
```
# XFAIL: known POSIX deviation (trap reset in subshell — interpretation varies across shells)
```

- [ ] **Step 5: Verify counts still match baseline**

Run:
```bash
./e2e/run_tests.sh 2>&1 | tail -2
```
Expected: `Total: 797  Passed: 784  Failed: 0  Timedout: 0  XFail: 3  Migrated: 10  XPass: 0`
(identical to Step 1; XFAIL counter is insensitive to comment text)

- [ ] **Step 6: Sanity-grep the new wordings**

Run:
```bash
grep -n "^# XFAIL:" e2e/posix_spec/4_required_builtin/ulimit_unknown_option.sh \
                  e2e/posix_spec/8_env_vars/LANG_default_collate.sh \
                  e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh
```
Expected output (3 lines, exact text):
```
e2e/posix_spec/4_required_builtin/ulimit_unknown_option.sh:4:# XFAIL: deferred (TODO: implement ulimit; out of scope for v0.x — tracked in TODO.md)
e2e/posix_spec/8_env_vars/LANG_default_collate.sh:4:# XFAIL: deferred (TODO: locale support — tracked in TODO.md)
e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh:4:# XFAIL: known POSIX deviation (trap reset in subshell — interpretation varies across shells)
```

(Do NOT commit yet — Task 2 also lands in this commit.)

---

## Task 2: Update `TODO.md`

**Files:**
- Modify: `TODO.md`

Four coordinated edits inside one file. Apply them in the order below.

- [ ] **Step 1: Read current `TODO.md` to confirm line ranges**

Run:
```bash
sed -n '1,10p' TODO.md
```
Expected first 10 lines (header block):
```
# TODO

## E2E XFAIL Roadmap

Decomposition of 55 XFAIL tests into 7 sub-projects. See
`docs/superpowers/specs/2026-05-13-e2e-xfail-roadmap-design.md`.

- [ ] SP7 — Deferred / recorded as known deviation (3 tests)

### SP1 follow-ups (non-blocking)
```

If the structure differs (e.g., SP1 already at top), STOP and reconcile
before continuing.

- [ ] **Step 2: Rename the roadmap heading + replace the intro + delete the SP7 line**

Use Edit with `old_string`:
```
## E2E XFAIL Roadmap

Decomposition of 55 XFAIL tests into 7 sub-projects. See
`docs/superpowers/specs/2026-05-13-e2e-xfail-roadmap-design.md`.

- [ ] SP7 — Deferred / recorded as known deviation (3 tests)

### SP1 follow-ups (non-blocking)
```
and `new_string`:
```
## E2E XFAIL Roadmap Follow-ups

Roadmap closed 2026-05-17. Non-blocking follow-ups from SP1–SP6
retained below for tracking.

### SP1 follow-ups (non-blocking)
```

This is a single atomic Edit covering all three sub-changes from spec
§3.5 (heading rename, intro replacement, SP7 line deletion).

- [ ] **Step 3: Append the new `trap-reset` entry to `## Future: POSIX Conformance Bugs`**

The new entry must be the LAST item in `## Future: POSIX Conformance
Bugs` (the section that ends with the "Reserved word not recognized
after an assignment prefix" entry, just before `## Future: Release
Skill Enhancements`).

Use Edit with `old_string`:
```
- [ ] Reserved word not recognized after an assignment prefix — `x=1 if true; then echo y; fi`
      triggers exit 127 ("if: not found") instead of treating `if` as the command-position
      reserved word. POSIX §2.4 requires reserved-word recognition regardless of leading
      assignment prefixes. XFAIL test:
      `e2e/posix_spec/2_04_reserved_words/reserved_after_assignment_recognized.sh`.

## Future: Release Skill Enhancements
```
and `new_string`:
```
- [ ] Reserved word not recognized after an assignment prefix — `x=1 if true; then echo y; fi`
      triggers exit 127 ("if: not found") instead of treating `if` as the command-position
      reserved word. POSIX §2.4 requires reserved-word recognition regardless of leading
      assignment prefixes. XFAIL test:
      `e2e/posix_spec/2_04_reserved_words/reserved_after_assignment_recognized.sh`.
- [ ] Subshell trap reset for uncaught signals not implemented — yosh
      inherits the parent's `trap 'cmd' SIG` action into subshells
      instead of resetting non-caught signals to their default action.
      POSIX §2.11 leaves the precise semantics open to interpretation
      and major shells (bash, dash, ksh) diverge on which signals are
      reset and when. Recorded as a known deviation rather than a fix
      target. XFAIL test:
      `e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh`.

## Future: Release Skill Enhancements
```

- [ ] **Step 4: Verify TODO.md changes with greps**

Run all five greps in one shell line:
```bash
echo "SP7=$(grep -c "SP7" TODO.md) HEAD=$(grep -c "^## E2E XFAIL Roadmap Follow-ups$" TODO.md) CLOSURE=$(grep -c "Roadmap closed 2026-05-17" TODO.md) TRAP=$(grep -c "Subshell trap reset for uncaught signals" TODO.md) SP1FU=$(grep -c "^### SP1 follow-ups" TODO.md) SP6FU=$(grep -c "^### SP6 follow-ups" TODO.md)"
```
Expected output:
```
SP7=0 HEAD=1 CLOSURE=1 TRAP=1 SP1FU=1 SP6FU=1
```

If any count is off, fix before continuing.

- [ ] **Step 5: Visually inspect heading area**

Run:
```bash
sed -n '1,10p' TODO.md
```
Expected first 10 lines:
```
# TODO

## E2E XFAIL Roadmap Follow-ups

Roadmap closed 2026-05-17. Non-blocking follow-ups from SP1–SP6
retained below for tracking.

### SP1 follow-ups (non-blocking)

- [ ] `exec_function_call` does not clear `env.exec.loop_depth` on entry, so `break`/`continue` inside a function called from a loop affects the caller's loop. Matches dash; bash treats it as out-of-loop. Decide intent and either save/restore `loop_depth` on function entry or document the deviation (`src/exec/function.rs`).
```

(SP1 follow-up content should immediately follow without any orphaned
SP7 fragments.)

---

## Task 3: Verify, update memory, and commit

**Files:**
- Modify: `/Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/project_e2e_xfail_roadmap.md`

- [ ] **Step 1: Final full E2E run**

Run:
```bash
./e2e/run_tests.sh 2>&1 | tail -2
```
Expected: `Total: 797  Passed: 784  Failed: 0  Timedout: 0  XFail: 3  Migrated: 10  XPass: 0`

Counts must match the baseline from Task 1 Step 1 exactly. If `XFail`
becomes 4+ or any test moves to `Failed`, the XFAIL comment edit broke
the test parser — re-inspect the comment syntax.

- [ ] **Step 2: Run `cargo build`**

Run:
```bash
cargo build 2>&1 | tail -5
```
Expected: build succeeds (no errors). May print warnings; ignore.

(Sanity only — SP7 touches no Rust code, but the project convention is
to confirm green before committing.)

- [ ] **Step 3: Run `cargo test` (background, ~6-7 min on this project)**

Run in foreground (since this plan has no concurrent work):
```bash
cargo test 2>&1 | tail -20
```
Expected: all tests pass. Look for `test result: ok.` on the final line
of each binary's summary.

If a flaky timeout fires (see memory `feedback_cargo_build_slow`),
re-run once. If it persists, STOP and surface to the user — SP7
should not be blamed for an unrelated flake but must be confirmed.

- [ ] **Step 4: Update auto-memory `project_e2e_xfail_roadmap.md`**

Read the current memory file first (Read tool), then update:

1. Frontmatter `description:` field — change to:
   ```
   description: "55-XFAIL-test decomposition roadmap; SP1-SP7 complete (2026-05-17), roadmap fully closed"
   ```
2. The status header line — change:
   ```
   **Status (as of 2026-05-16):**
   ```
   to:
   ```
   **Status (as of 2026-05-17):**
   ```
3. The `**SP7 pending**` line — replace with:
   ```
   - **SP7 COMPLETE** (2026-05-17): 3 tests — Deferred / known POSIX deviation (ulimit, LANG/locale, trap reset in subshell). Spec `2026-05-17-e2e-xfail-sp7-deferred-design.md`. Plan `2026-05-17-e2e-xfail-sp7-deferred.md`. Documentation-only (no source changes): XFAIL comments rewritten to `deferred (…)` / `known POSIX deviation (…)`, new `## Future: POSIX Conformance Bugs` entry for trap-reset, `## E2E XFAIL Roadmap` renamed to `... Follow-ups` and intro replaced with closure marker. SP1–SP6 follow-up subsections retained.
   ```
4. The closing-summary line — change:
   ```
   After SP1+SP2+SP3+SP4+SP5+SP6: 55 - 11 - 5 - 9 - 9 - 8 - 10 = 3 XFails remain (matches `./e2e/run_tests.sh` output `XFail: 3 Migrated: 10`).
   ```
   to:
   ```
   After SP1+SP2+SP3+SP4+SP5+SP6+SP7: 55 - 11 - 5 - 9 - 9 - 8 - 10 - 3 = 0 unaccounted XFails; 3 remain as documented deferrals/deviations (matches `./e2e/run_tests.sh` output `XFail: 3 Migrated: 10`).
   ```

Leave the `**Lessons learned in …**` blocks below untouched.

- [ ] **Step 5: Verify nothing else changed**

Run:
```bash
git status
```
Expected: 3 E2E test files, `TODO.md`, and the memory file shown as
modified. No untracked files (the spec was committed earlier as
`eafce57`, the spec correction as `1676b17`).

If the plan file itself is untracked, that is fine — it gets added in
Step 6.

- [ ] **Step 6: Commit**

Run:
```bash
git add e2e/posix_spec/4_required_builtin/ulimit_unknown_option.sh \
        e2e/posix_spec/8_env_vars/LANG_default_collate.sh \
        e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh \
        TODO.md \
        docs/superpowers/plans/2026-05-17-e2e-xfail-sp7-deferred.md
git commit -m "$(cat <<'EOF'
chore(sp7): close SP7 — record 3 XFAILs as deferred / known deviation

SP7 of the E2E XFAIL roadmap is documentation-only. Three remaining
XFAIL tests are rewritten to `deferred (…)` or `known POSIX deviation
(…)` form per spec §3.1–§3.3. TODO.md gains a rationale entry for the
trap-reset-in-subshell case, and the roadmap section is renamed to
`## E2E XFAIL Roadmap Follow-ups` with a closure marker. The SP1–SP6
non-blocking follow-up subsections are retained verbatim.

Memory `project_e2e_xfail_roadmap.md` updated separately by the
agent runtime (not part of this commit).

Spec: docs/superpowers/specs/2026-05-17-e2e-xfail-sp7-deferred-design.md
Plan: docs/superpowers/plans/2026-05-17-e2e-xfail-sp7-deferred.md

Original task: TODO.md の SP7 を対応して

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git status
```
Expected:
- Commit succeeds with the message above.
- `git status` shows `nothing to commit, working tree clean` (the
  memory file lives outside the repo, so it is not staged).

- [ ] **Step 7: Final sanity check**

Run:
```bash
git log --oneline -3
```
Expected (top entry is the SP7 closure):
```
<sha> chore(sp7): close SP7 — record 3 XFAILs as deferred / known deviation
1676b17 docs(sp7): correct spec — preserve SPN follow-up subsections
eafce57 docs(sp7): add SP7 closure design — defer ulimit/LANG/trap-reset XFAILs
```

Plan is complete. The E2E XFAIL roadmap is closed.

---

## Acceptance Criteria (mirrors spec §4)

1. The three E2E test files carry the exact XFAIL wording from spec
   §3.1–§3.3.
2. `./e2e/run_tests.sh` reports the same counts before and after.
3. `cargo build` and `cargo test` stay green.
4. `TODO.md` contains the new `Subshell trap reset for uncaught
   signals` entry under `## Future: POSIX Conformance Bugs`.
5. `TODO.md` heading is renamed to `## E2E XFAIL Roadmap Follow-ups`,
   the intro paragraph carries the closure marker, the SP7 line is
   gone, and the `### SPN follow-ups (non-blocking)` blocks are
   unchanged.
6. The spec and plan files are both committed under
   `docs/superpowers/{specs,plans}/`.
7. The `project_e2e_xfail_roadmap` auto-memory entry reflects
   "SP7 COMPLETE (2026-05-17); roadmap fully closed".
