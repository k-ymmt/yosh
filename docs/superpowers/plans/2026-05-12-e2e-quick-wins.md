# E2E Quick Wins Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land four small, mechanical E2E cleanups from TODO.md (L112, L116, L124, L125) without changing shell behavior.

**Architecture:** Five sequential commits — (1) L112 POSIX_REF tightening across 15 builtin tests, (2) L116 relocation + recite of 15 `test_*.sh` files, (3) L124+L125 combined `run_tests.sh` edits, (4) TODO.md cleanup, (5) final full-suite verification commit (no code, just a check-and-confirm). All changes are textual; no `src/` or `tests/` (Rust) changes.

**Tech Stack:** POSIX `/bin/sh`, `git`, `grep`, the existing `./e2e/run_tests.sh` harness.

**Spec:** `docs/superpowers/specs/2026-05-12-e2e-quick-wins-design.md`

---

## Pre-flight

The e2e runner needs a debug build to execute. Build once now; do not rebuild between tasks unless something in `src/` changes (nothing in this plan touches `src/`).

- [ ] **Step 1: Verify clean tree**

Run: `git status`
Expected: `nothing to commit, working tree clean` (the spec commit `c8639da` is already on `main`).

- [ ] **Step 2: Build debug yosh**

Run: `cargo build`
Expected: build succeeds. Binary at `./target/debug/yosh`.

- [ ] **Step 3: Baseline E2E run for the affected slice**

Run: `./e2e/run_tests.sh --filter=builtin`
Expected: every test under `e2e/builtin/` reports `[PASS]` (no `[FAIL]` / `[TIME]` / `[XPASS]`). Note the total `Passed:` count from the `── Summary ──` block — call it `BUILTIN_BASE`.

Run: `./e2e/run_tests.sh --filter=2_14_test`
Expected: all 15 `[PASS]`. Note the `Passed:` count as `TEST_BASE` (should be 15).

These two numbers are what we will compare against after each subsequent task.

---

## Task 1: L112 — Tighten generic §2.14 `POSIX_REF` in `e2e/builtin/`

**Files (15, all under `e2e/builtin/`):**
- Modify: `colon_noop.sh`, `source_file.sh`, `eval_basic.sh`, `eval_variable.sh`,
  `exec_no_args.sh`, `exec_replace.sh`, `export_basic.sh`, `export_format.sh`,
  `readonly_basic.sh`, `set_dash_dash.sh`, `set_monitor_flag.sh`,
  `set_positional.sh`, `shift_basic.sh`, `unset_readonly_error.sh`,
  `unset_variable.sh`

Each file's second line is currently:

```sh
# POSIX_REF: 2.14 Special Built-In Utilities
```

We rewrite it per file. The mapping is reproduced here so each step is self-contained (do not page back to the spec while editing).

- [ ] **Step 1: Verify the pre-state**

Run: `grep -l 'POSIX_REF: 2\.14 Special Built-In Utilities' e2e/builtin/*.sh | sort`

Expected output (15 lines):
```
e2e/builtin/colon_noop.sh
e2e/builtin/eval_basic.sh
e2e/builtin/eval_variable.sh
e2e/builtin/exec_no_args.sh
e2e/builtin/exec_replace.sh
e2e/builtin/export_basic.sh
e2e/builtin/export_format.sh
e2e/builtin/readonly_basic.sh
e2e/builtin/set_dash_dash.sh
e2e/builtin/set_monitor_flag.sh
e2e/builtin/set_positional.sh
e2e/builtin/shift_basic.sh
e2e/builtin/source_file.sh
e2e/builtin/unset_readonly_error.sh
e2e/builtin/unset_variable.sh
```

If the count or filenames differ, stop and re-read the spec — something else changed since the plan was written.

- [ ] **Step 2: Rewrite `colon_noop.sh`**

In `e2e/builtin/colon_noop.sh`, change line 2:

Old:
```
# POSIX_REF: 2.14 Special Built-In Utilities
```
New:
```
# POSIX_REF: 2.14.2 colon
```

- [ ] **Step 3: Rewrite `source_file.sh`**

In `e2e/builtin/source_file.sh`, change line 2:

Old:
```
# POSIX_REF: 2.14 Special Built-In Utilities
```
New:
```
# POSIX_REF: 2.14.4 dot
```

(`source_file.sh` tests the POSIX `.` builtin via `. file`. `source` is a bash/ksh extension; the POSIX citation must be §2.14.4 `dot`.)

- [ ] **Step 4: Rewrite the two `eval` files**

In `e2e/builtin/eval_basic.sh`, change line 2 to:
```
# POSIX_REF: 2.14.5 eval
```

In `e2e/builtin/eval_variable.sh`, change line 2 to:
```
# POSIX_REF: 2.14.5 eval
```

- [ ] **Step 5: Rewrite the two `exec` files**

In `e2e/builtin/exec_no_args.sh`, change line 2 to:
```
# POSIX_REF: 2.14.6 exec
```

In `e2e/builtin/exec_replace.sh`, change line 2 to:
```
# POSIX_REF: 2.14.6 exec
```

- [ ] **Step 6: Rewrite the two `export` files**

In `e2e/builtin/export_basic.sh`, change line 2 to:
```
# POSIX_REF: 2.14.8 export
```

In `e2e/builtin/export_format.sh`, change line 2 to:
```
# POSIX_REF: 2.14.8 export
```

- [ ] **Step 7: Rewrite `readonly_basic.sh`**

In `e2e/builtin/readonly_basic.sh`, change line 2 to:
```
# POSIX_REF: 2.14.9 readonly
```

- [ ] **Step 8: Rewrite the three `set` files**

In `e2e/builtin/set_dash_dash.sh`, change line 2 to:
```
# POSIX_REF: 2.14.11 set
```

In `e2e/builtin/set_monitor_flag.sh`, change line 2 to:
```
# POSIX_REF: 2.14.11 set
```

In `e2e/builtin/set_positional.sh`, change line 2 to:
```
# POSIX_REF: 2.14.11 set
```

(`set_monitor_flag.sh` cites the `set` builtin, not §2.11 Job Control. This is deliberate — see spec §3.1 notes. `set_monitor_off.sh` keeps its existing `2.11 Job Control` citation.)

- [ ] **Step 9: Rewrite `shift_basic.sh`**

In `e2e/builtin/shift_basic.sh`, change line 2 to:
```
# POSIX_REF: 2.14.12 shift
```

- [ ] **Step 10: Rewrite the two `unset` files**

In `e2e/builtin/unset_readonly_error.sh`, change line 2 to:
```
# POSIX_REF: 2.14.15 unset
```

In `e2e/builtin/unset_variable.sh`, change line 2 to:
```
# POSIX_REF: 2.14.15 unset
```

- [ ] **Step 11: Verify the post-state**

Run: `grep -E 'POSIX_REF: 2\.14 Special Built-In Utilities' e2e/builtin/*.sh`
Expected: no matches (exit code 1; `grep` prints nothing).

Run: `grep -E 'POSIX_REF: 2\.14\.[0-9]+' e2e/builtin/*.sh | sort | uniq -c`
Expected output (15 lines total, grouped):
```
   1 e2e/builtin/colon_noop.sh:# POSIX_REF: 2.14.2 colon
   2 e2e/builtin/eval_basic.sh:# POSIX_REF: 2.14.5 eval
...
```
The exact count per subsection: 1 (colon), 1 (dot), 2 (eval), 2 (exec), 2 (export), 1 (readonly), 3 (set), 1 (shift), 2 (unset) = 15.

- [ ] **Step 12: Re-run the builtin slice**

Run: `./e2e/run_tests.sh --filter=builtin`
Expected: `Passed:` equals `BUILTIN_BASE` (from Pre-flight Step 3). No new `[FAIL]` / `[TIME]`. (POSIX_REF is metadata — the runner does not validate its content, so PASS counts must be unchanged.)

- [ ] **Step 13: Commit**

```bash
git add e2e/builtin/colon_noop.sh e2e/builtin/source_file.sh \
        e2e/builtin/eval_basic.sh e2e/builtin/eval_variable.sh \
        e2e/builtin/exec_no_args.sh e2e/builtin/exec_replace.sh \
        e2e/builtin/export_basic.sh e2e/builtin/export_format.sh \
        e2e/builtin/readonly_basic.sh \
        e2e/builtin/set_dash_dash.sh e2e/builtin/set_monitor_flag.sh \
        e2e/builtin/set_positional.sh \
        e2e/builtin/shift_basic.sh \
        e2e/builtin/unset_readonly_error.sh e2e/builtin/unset_variable.sh
git commit -m "$(cat <<'EOF'
docs(e2e): tighten POSIX_REF to §2.14 subsections (TODO L112)

Replace the generic `2.14 Special Built-In Utilities` citation with the
per-builtin POSIX subsection number across 15 files in e2e/builtin/.
Aligns with the existing `2.14.13 times` citation under
e2e/posix_spec/2_14_13_times/ and makes POSIX_REF greppable by builtin.

Original prompt: "TODO.md の E2E 関連について対応を行って下さい。"
Plan: docs/superpowers/plans/2026-05-12-e2e-quick-wins.md Task 1

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Run: `git status`
Expected: `nothing to commit, working tree clean`.

---

## Task 2: L116 — Relocate `2_14_test/` into `e2e/builtin/`

**Files:**
- Move: 15 files from `e2e/posix_spec/2_14_test/` → `e2e/builtin/`
- Modify each moved file's line 2 (`POSIX_REF: 2.14 test` → `4 Utilities - test`)
- Delete: empty directory `e2e/posix_spec/2_14_test/`

- [ ] **Step 1: Verify the pre-state**

Run: `ls e2e/posix_spec/2_14_test/`
Expected: 15 `test_*.sh` files listed.

Run: `grep -l 'POSIX_REF: 2\.14 test' e2e/posix_spec/2_14_test/*.sh | wc -l`
Expected: `15`.

Run: `ls e2e/builtin/test_*.sh 2>/dev/null | wc -l`
Expected: `0` (no name collisions — the destination has no `test_*.sh` yet).

- [ ] **Step 2: `git mv` the 15 files**

Run:
```bash
git mv e2e/posix_spec/2_14_test/test_bracket_requires_closing.sh e2e/builtin/
git mv e2e/posix_spec/2_14_test/test_file_exists.sh             e2e/builtin/
git mv e2e/posix_spec/2_14_test/test_file_readable.sh           e2e/builtin/
git mv e2e/posix_spec/2_14_test/test_file_regular.sh            e2e/builtin/
git mv e2e/posix_spec/2_14_test/test_file_symlink.sh            e2e/builtin/
git mv e2e/posix_spec/2_14_test/test_integer_compare.sh         e2e/builtin/
git mv e2e/posix_spec/2_14_test/test_integer_parse_error.sh     e2e/builtin/
git mv e2e/posix_spec/2_14_test/test_isatty_fd.sh               e2e/builtin/
git mv e2e/posix_spec/2_14_test/test_negation.sh                e2e/builtin/
git mv e2e/posix_spec/2_14_test/test_no_args.sh                 e2e/builtin/
git mv e2e/posix_spec/2_14_test/test_paren_grouping.sh          e2e/builtin/
git mv e2e/posix_spec/2_14_test/test_string_eq_neq.sh           e2e/builtin/
git mv e2e/posix_spec/2_14_test/test_string_nonempty.sh         e2e/builtin/
git mv e2e/posix_spec/2_14_test/test_too_many_args.sh           e2e/builtin/
git mv e2e/posix_spec/2_14_test/test_unknown_operator.sh        e2e/builtin/
```

Run: `ls e2e/posix_spec/2_14_test/`
Expected: empty (no files).

Run: `ls e2e/builtin/test_*.sh | wc -l`
Expected: `15`.

- [ ] **Step 3: Rewrite the `POSIX_REF` line in each moved file**

For each of the 15 files now at `e2e/builtin/test_*.sh`, replace line 2:

Old (in every file):
```
# POSIX_REF: 2.14 test
```
New (in every file):
```
# POSIX_REF: 4 Utilities - test
```

The 15 paths are:
```
e2e/builtin/test_bracket_requires_closing.sh
e2e/builtin/test_file_exists.sh
e2e/builtin/test_file_readable.sh
e2e/builtin/test_file_regular.sh
e2e/builtin/test_file_symlink.sh
e2e/builtin/test_integer_compare.sh
e2e/builtin/test_integer_parse_error.sh
e2e/builtin/test_isatty_fd.sh
e2e/builtin/test_negation.sh
e2e/builtin/test_no_args.sh
e2e/builtin/test_paren_grouping.sh
e2e/builtin/test_string_eq_neq.sh
e2e/builtin/test_string_nonempty.sh
e2e/builtin/test_too_many_args.sh
e2e/builtin/test_unknown_operator.sh
```

- [ ] **Step 4: Remove the now-empty source directory**

Run: `rmdir e2e/posix_spec/2_14_test`
Expected: succeeds silently. (If `rmdir` complains "Directory not empty", a file was missed in Step 2 — re-check.)

Run: `test -d e2e/posix_spec/2_14_test && echo EXISTS || echo GONE`
Expected: `GONE`.

- [ ] **Step 5: Verify the post-state**

Run: `grep -E 'POSIX_REF: 2\.14 test' e2e/builtin/*.sh`
Expected: no matches.

Run: `grep -E 'POSIX_REF: 4 Utilities - test' e2e/builtin/test_*.sh | wc -l`
Expected: `15`.

Run: `grep -RE 'posix_spec/2_14_test' e2e/ src/ tests/ docs/superpowers/specs/ docs/superpowers/plans/ 2>/dev/null`
Expected: only the *current* plan and the spec mention the old path (in historical references). If anything under `e2e/`, `src/`, or `tests/` still references the old path, stop and investigate. (The `target/package/` snapshots are publish-time artifacts and intentionally not in the grep set.)

- [ ] **Step 6: Run the moved tests in their new location**

Run: `./e2e/run_tests.sh --filter=builtin/test_`
Expected: all 15 moved tests `[PASS]` (under their new `e2e/builtin/test_*.sh` paths).

Run: `./e2e/run_tests.sh --filter=2_14_test`
Expected: zero tests selected (`Total: 0`). (The old directory is gone; the filter matches no path.)

- [ ] **Step 7: Re-run the full builtin slice**

Run: `./e2e/run_tests.sh --filter=builtin`
Expected: `Passed:` equals `BUILTIN_BASE + TEST_BASE` (from Pre-flight Step 3). No `[FAIL]` / `[TIME]`.

- [ ] **Step 8: Commit**

```bash
git add e2e/builtin/ e2e/posix_spec/
git commit -m "$(cat <<'EOF'
test(e2e): relocate test/[ tests to e2e/builtin/ and fix POSIX_REF (TODO L116)

The 15 files under e2e/posix_spec/2_14_test/ cited POSIX §2.14 but the
test utility is in XCU Chapter 4, not §2.14 (Special Built-In Utilities).
Move them into e2e/builtin/ — alongside cd_*.sh, echo_*.sh, etc., which
already cite `4 Utilities - <name>` — and rewrite POSIX_REF to
`4 Utilities - test`. The empty 2_14_test/ directory is removed.

Original prompt: "TODO.md の E2E 関連について対応を行って下さい。"
Plan: docs/superpowers/plans/2026-05-12-e2e-quick-wins.md Task 2

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Run: `git status`
Expected: `nothing to commit, working tree clean`.

---

## Task 3: L124 + L125 — `run_tests.sh` comment + help text

**Files:**
- Modify: `e2e/run_tests.sh:116` (help text)
- Modify: `e2e/run_tests.sh:~290` (insert comment before the timer subshell)

Combined into one commit because both touch the same runner file.

- [ ] **Step 1: Verify the pre-state of the help line**

Run: `grep -n 'YOSH_E2E_NO_TIMEOUT=1' e2e/run_tests.sh`
Expected: one match on line 116:
```
116:    printf "  YOSH_E2E_NO_TIMEOUT=1  Skip per-test timeout (local use only)\n"
```

- [ ] **Step 2: Rewrite the help line (L125)**

In `e2e/run_tests.sh`, replace this single line:

Old:
```
    printf "  YOSH_E2E_NO_TIMEOUT=1  Skip per-test timeout (local use only)\n"
```
New (two lines — the second is a wrapped continuation aligned to the description column):
```
    printf "  YOSH_E2E_NO_TIMEOUT=1  Skip per-test timeout; never set in CI or\n"
    printf "                         release.sh (individual runaway tests will hang forever)\n"
```

Column alignment: the new continuation line has exactly 25 spaces before `release.sh`. That matches `  YOSH_E2E_NO_TIMEOUT=1  ` (2 + 21 + 2 = 25 chars) so the wrapped text lines up under the description column visually.

- [ ] **Step 3: Verify the help output**

Run: `./e2e/run_tests.sh --help`
Expected: the `Environment:` block reads:
```
  YOSH_E2E_NO_TIMEOUT=1  Skip per-test timeout; never set in CI or
                         release.sh (individual runaway tests will hang forever)
```
with `release.sh` aligned under `Skip` visually.

- [ ] **Step 4: Verify the pre-state of the timer subshell**

Run: `grep -n '^        (' e2e/run_tests.sh | head -5`
Expected: at least one match — the second match (after the help shift) should be the timer subshell at roughly the line that was 290 (it has shifted by +1 because Step 2 inserted one extra printf). Use:

Run: `grep -n 'sleep "\$TIMEOUT"' e2e/run_tests.sh`
Expected: one match, currently the line just below the `(` that starts the timer subshell.

- [ ] **Step 5: Insert the race-condition comment (L124)**

In `e2e/run_tests.sh`, find the exact block (currently appearing right after the `else` of the `YOSH_E2E_NO_TIMEOUT` check):

Old:
```sh
    else
        (
            sleep "$TIMEOUT"
            kill -9 "$_pid" 2>/dev/null && echo "timeout" >"$_exit_file"
        ) &
        _timer_pid=$!
    fi
```
New:
```sh
    else
        # Single-shot watchdog: SIGKILL the test if it outlives $TIMEOUT.
        # Benign race — if the test exits just as the timer fires, kill -9
        # returns ESRCH and we skip writing the "timeout" marker. The exit
        # code from `wait $_pid` below is the authoritative result; the
        # marker branch is diagnostic only, so the race cannot corrupt
        # pass/fail accounting.
        (
            sleep "$TIMEOUT"
            kill -9 "$_pid" 2>/dev/null && echo "timeout" >"$_exit_file"
        ) &
        _timer_pid=$!
    fi
```

Indentation of the comment block is 8 spaces (matches the `(` line).

- [ ] **Step 6: Sanity-run the suite to confirm the runner still parses**

Run: `./e2e/run_tests.sh --filter=builtin`
Expected: all `[PASS]`, same count as Task 2 Step 7. (We are not changing executable logic — just inserting comment lines and editing a printf — but running once confirms no accidental edit broke shell syntax.)

- [ ] **Step 7: Commit**

```bash
git add e2e/run_tests.sh
git commit -m "$(cat <<'EOF'
chore(e2e): document watchdog race + sharpen NO_TIMEOUT help (TODO L124, L125)

L124: Add a comment above the single-shot watchdog subshell explaining
the benign ESRCH race — if the test exits as the timer fires, kill -9
returns ESRCH and the "timeout" marker is skipped, but the wait $_pid
exit code is authoritative so pass/fail accounting is correct.

L125: Replace the soft "(local use only)" qualifier on YOSH_E2E_NO_TIMEOUT
with explicit guidance ("never set in CI or release.sh; individual
runaway tests will hang forever") so the consequence is visible at
--help time.

Original prompt: "TODO.md の E2E 関連について対応を行って下さい。"
Plan: docs/superpowers/plans/2026-05-12-e2e-quick-wins.md Task 3

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Run: `git status`
Expected: `nothing to commit, working tree clean`.

---

## Task 4: TODO.md cleanup

**Files:**
- Modify: `TODO.md` — delete four bullets

Per `CLAUDE.md`, completed TODOs are *deleted*, not marked `[x]`.

- [ ] **Step 1: Confirm the bullets are still there**

Run: `grep -n -E 'L112|L116|L124|L125' TODO.md`
Expected: no matches (TODO bullets are not labelled with the line numbers — the spec uses them for reference only).

Run: `grep -nE 'POSIX_REF values could use more specific|2_14_test/. POSIX_REF mis-citation|e2e runner timer race-condition|YOSH_E2E_NO_TIMEOUT. help wording' TODO.md`
Expected: four matches — one per bullet to remove.

Record the line numbers; they should be near 112, 116, 124, 125 in the current `TODO.md`.

- [ ] **Step 2: Delete the four bullets**

Open `TODO.md` and delete (do not comment out, do not mark `[x]`) these four complete entries:

**Bullet under "Future: E2E Test Expansion":**
```
- [ ] Builtin test POSIX_REF values could use more specific section numbers (e.g., `2.14.3` instead of `2.14 Special Built-In Utilities`)
```

**Bullet under "Future: E2E Test Expansion":**
```
- [ ] `e2e/posix_spec/2_14_test/` POSIX_REF mis-citation — all 15 files cite `POSIX_REF: 2.14 test`, but POSIX §2.14 is "Special Built-In Utilities" which does NOT include `test`. The `test` utility is in POSIX XCU Chapter 4 (Utilities). Update these references to `4 test` or `XCU test`, and move the directory out from under `posix_spec/` (or rename to indicate Chapter 4) if a §4 convention is established. Discovered 2026-05-12 during POSIX TODO cleanup batch.
```

**Bullet under "Future: Release Skill Enhancements":**
```
- [ ] e2e runner timer race-condition comment — `( sleep $TIMEOUT && kill -9 $_pid && echo timeout )` has a benign race: if the test exits just as `sleep` expires, `kill -9` returns ESRCH and the marker is not written. Behavior is correct (`wait $_pid` already captured the real exit code) but the race is undocumented. Add a short comment above the subshell explaining this (`e2e/run_tests.sh:186-192`). Code-review follow-up from 2026-04-22 release-perf work.
```

**Bullet under "Future: Release Skill Enhancements":**
```
- [ ] `YOSH_E2E_NO_TIMEOUT` help wording — current `--help` text says "local use only"; tighten to "never set in CI or release.sh; individual runaway tests will hang forever" to prevent accidental production use (`e2e/run_tests.sh:35`). Code-review follow-up from 2026-04-22 release-perf work.
```

When deleting, also remove the now-orphaned blank lines so the section headers and surrounding bullets remain visually clean (no double blank line).

- [ ] **Step 3: Verify the post-state**

Run: `grep -nE 'POSIX_REF values could use more specific|2_14_test/. POSIX_REF mis-citation|e2e runner timer race-condition|YOSH_E2E_NO_TIMEOUT. help wording' TODO.md`
Expected: no matches.

Run: `grep -c '^- \[ \]' TODO.md`
Expected: previous TODO count minus 4. Sanity-check the surrounding sections still parse (each `## ` header is followed by at least one bullet — no header was emptied out by the deletions).

- [ ] **Step 4: Commit**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
docs(todo): drop E2E quick-win items (L112, L116, L124, L125)

Per CLAUDE.md, completed TODOs are deleted (not marked [x]). The four
items addressed in this batch — POSIX_REF tightening, 2_14_test
relocation, watchdog comment, NO_TIMEOUT help wording — are removed.

Original prompt: "TODO.md の E2E 関連について対応を行って下さい。"
Plan: docs/superpowers/plans/2026-05-12-e2e-quick-wins.md Task 4

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Run: `git status`
Expected: `nothing to commit, working tree clean`.

---

## Task 5: Full-suite verification

No code changes — this task confirms the whole suite stays green and produces a clean log to attach to the wrap-up summary.

- [ ] **Step 1: Run the full E2E suite**

Run: `./e2e/run_tests.sh`
Expected: the `── Summary ──` block shows `Failed: 0`, `Timedout: 0`, and `XPass: 0`. (`XFail:` may be non-zero — those are pre-existing known-limitation tests recorded with `XFAIL:` headers and are not regressions.)

If the runner reports a failure log path (`Failure details captured in: ...`), inspect it; nothing in this plan should introduce a new failure.

- [ ] **Step 2: Run Rust unit + integration tests**

Run: `cargo test --features test-helpers` (background-friendly; ~6–7 minutes per the cargo-build-slow memory)

Expected: all tests pass. No file in `src/` or `tests/` was touched by this plan, so this is a regression safety net only.

- [ ] **Step 3: Confirm git is clean and ahead**

Run: `git log --oneline main ^origin/main | head -10`
Expected: at least four new commits since the spec commit (`c8639da`):
1. `docs(e2e): tighten POSIX_REF to §2.14 subsections (TODO L112)`
2. `test(e2e): relocate test/[ tests to e2e/builtin/ and fix POSIX_REF (TODO L116)`
3. `chore(e2e): document watchdog race + sharpen NO_TIMEOUT help (TODO L124, L125)`
4. `docs(todo): drop E2E quick-win items (L112, L116, L124, L125)`

Run: `git status`
Expected: `nothing to commit, working tree clean`.

- [ ] **Step 4: Hand back to user**

No commit on this task. Summarize: spec ID, plan ID, four commit SHAs, E2E summary numbers, and whether `cargo test` was green.
