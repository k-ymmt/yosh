# E2E Test Quality Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tighten 5 existing E2E tests that have quality issues (false-pass risk, weak assertions, duplicates, wrong permissions).

**Architecture:** Pure E2E test file edits under `e2e/`. No production Rust code is touched. Each fix is independently verifiable via `./e2e/run_tests.sh --filter=<name>`.

**Tech Stack:** POSIX shell (test scripts), `e2e/run_tests.sh` (Bash runner), `cargo test` (regression sanity).

**Spec:** `docs/superpowers/specs/2026-04-20-e2e-test-quality-fixes-design.md`

---

## File Inventory

- Modify: `e2e/command_execution/echo_simple.sh` (permissions only)
- Delete: `e2e/posix_spec/2_10_shell_grammar/rule05_for_valid_name_ok.sh`
- Modify: `e2e/posix_spec/2_10_shell_grammar/rule04_case_last_item_no_dsemi.sh` (body + EXPECT_OUTPUT)
- Modify: `e2e/posix_spec/2_14_13_times/times_format.sh` (glob pattern)
- Modify: `e2e/posix_spec/2_14_13_times/times_in_subshell.sh` (glob pattern)
- Modify: `e2e/posix_spec/2_10_shell_grammar/rule10_reserved_quoted_not_recognized.sh` (assertion body)
- Modify: `TODO.md` (delete 5 completed lines)

---

## Preflight

- [ ] **Step 1: Ensure clean working tree**

Run: `git status`
Expected: `nothing to commit, working tree clean` (or only untracked files unrelated to this plan).

- [ ] **Step 2: Build debug binary once (required by e2e runner)**

Run: `cargo build`
Expected: builds successfully. E2E runner expects `target/debug/yosh`.

---

## Task 1: Fix `echo_simple.sh` permissions (755 → 644)

**Files:**
- Modify (permissions only): `e2e/command_execution/echo_simple.sh`

- [ ] **Step 1: Verify current permissions are 755**

Run: `ls -l e2e/command_execution/echo_simple.sh`
Expected: mode starts with `-rwxr-xr-x`.

- [ ] **Step 2: Change mode to 644**

Run: `chmod 644 e2e/command_execution/echo_simple.sh`

- [ ] **Step 3: Verify mode is now 644**

Run: `ls -l e2e/command_execution/echo_simple.sh`
Expected: mode starts with `-rw-r--r--`.

- [ ] **Step 4: Confirm test still passes under runner**

Run: `./e2e/run_tests.sh --filter=echo_simple`
Expected: the test runs and passes (runner reads the file; does not execute it directly).

- [ ] **Step 5: Commit**

```bash
git add e2e/command_execution/echo_simple.sh
git commit -m "$(cat <<'EOF'
test(e2e): normalize echo_simple.sh permissions to 644

E2E scripts are read by the runner, not executed directly. Align with
project convention (644) so the file stays consistent with the rest of
the e2e suite.

Context: TODO.md "Future: E2E Test Expansion" — echo_simple.sh 755 fix.
EOF
)"
```

---

## Task 2: Remove duplicate `rule05_for_valid_name_ok.sh`

**Files:**
- Delete: `e2e/posix_spec/2_10_shell_grammar/rule05_for_valid_name_ok.sh`

Both `rule05_for_valid_name.sh` and `rule05_for_valid_name_ok.sh` guard POSIX §2.10.2 Rule 5 valid-NAME acceptance under the same `POSIX_REF`. The older `rule05_for_valid_name.sh` is the canonical version — delete `_ok.sh`.

- [ ] **Step 1: Confirm both files exist and cover the same case**

Run: `head -10 e2e/posix_spec/2_10_shell_grammar/rule05_for_valid_name.sh`
Expected: `POSIX_REF: 2.10.2 Rule 5 - NAME in for`, body uses `for x in a b; do echo "$x"; done`.

Run: `head -10 e2e/posix_spec/2_10_shell_grammar/rule05_for_valid_name_ok.sh`
Expected: same `POSIX_REF`, body uses `for i in a b c; do echo $i; done`. Same Rule 5 acceptance case.

- [ ] **Step 2: Delete the duplicate**

Run: `git rm e2e/posix_spec/2_10_shell_grammar/rule05_for_valid_name_ok.sh`
Expected: file is staged for deletion.

- [ ] **Step 3: Confirm the surviving test still passes**

Run: `./e2e/run_tests.sh --filter=rule05_for_valid_name`
Expected: exactly 1 test runs (`rule05_for_valid_name.sh`) and passes.

- [ ] **Step 4: Commit**

```bash
git commit -m "$(cat <<'EOF'
test(e2e): drop duplicate rule05_for_valid_name_ok.sh

Both files guard the same POSIX §2.10.2 Rule 5 valid-NAME case under
the same POSIX_REF. Keep the canonical rule05_for_valid_name.sh and
remove the redundant _ok.sh copy.

Context: TODO.md "Future: E2E Test Expansion" — rule05 duplicate.
EOF
)"
```

---

## Task 3: Strengthen `rule04_case_last_item_no_dsemi.sh`

**Files:**
- Modify: `e2e/posix_spec/2_10_shell_grammar/rule04_case_last_item_no_dsemi.sh`

Current body prints `a` from both arms, so a regression where the first arm wrongly matches `x` would still produce `a` and the test would false-pass. Make the two arms emit distinct output.

- [ ] **Step 1: Replace file contents**

Overwrite `e2e/posix_spec/2_10_shell_grammar/rule04_case_last_item_no_dsemi.sh` with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 4 - Case statement termination
# DESCRIPTION: The last case item may omit ;; before esac
# EXPECT_OUTPUT: LAST
# EXPECT_EXIT: 0
case x in
    a) echo FIRST ;;
    x) echo LAST
esac
```

- [ ] **Step 2: Confirm permissions remain 644**

Run: `ls -l e2e/posix_spec/2_10_shell_grammar/rule04_case_last_item_no_dsemi.sh`
Expected: mode starts with `-rw-r--r--`.

- [ ] **Step 3: Run the test**

Run: `./e2e/run_tests.sh --filter=rule04_case_last_item_no_dsemi`
Expected: the single test passes. Actual output is `LAST`.

- [ ] **Step 4: Commit**

```bash
git add e2e/posix_spec/2_10_shell_grammar/rule04_case_last_item_no_dsemi.sh
git commit -m "$(cat <<'EOF'
test(e2e): tighten rule04 case no-dsemi assertion

Both arms previously echoed "a", so a regression where the first arm
wrongly matched "x" would still pass. Emit FIRST vs LAST so the test
observes which arm actually executed.

Context: TODO.md "Future: E2E Test Expansion" — rule04 weak assertion.
EOF
)"
```

---

## Task 4: Tighten `times_format.sh` glob

**Files:**
- Modify: `e2e/posix_spec/2_14_13_times/times_format.sh`

`builtin_times` in `src/builtin/special.rs` formats each time as `{}m{:.3}s` (always decimal), so the stricter glob is safe.

- [ ] **Step 1: Replace both occurrences of the loose glob**

In `e2e/posix_spec/2_14_13_times/times_format.sh`, replace both lines matching:

```sh
    *m*s\ *m*s) ;;
```

with:

```sh
    [0-9]*m[0-9]*.[0-9]*s\ [0-9]*m[0-9]*.[0-9]*s) ;;
```

Final file contents should be:

```sh
#!/bin/sh
# POSIX_REF: 2.14.13 times
# DESCRIPTION: times prints two lines in "NmS.sssS NmS.sssS" shape
# EXPECT_OUTPUT omitted: CPU-time values are non-deterministic; shape verified in-script.
# EXPECT_EXIT: 0
out=$(times)
line1=$(echo "$out" | sed -n '1p')
line2=$(echo "$out" | sed -n '2p')
case "$line1" in
    [0-9]*m[0-9]*.[0-9]*s\ [0-9]*m[0-9]*.[0-9]*s) ;;
    *) echo "bad line1: $line1" >&2; exit 1 ;;
esac
case "$line2" in
    [0-9]*m[0-9]*.[0-9]*s\ [0-9]*m[0-9]*.[0-9]*s) ;;
    *) echo "bad line2: $line2" >&2; exit 1 ;;
esac
```

- [ ] **Step 2: Run the test**

Run: `./e2e/run_tests.sh --filter=times_format`
Expected: test passes. Actual `times` output (e.g., `0m0.000s 0m0.000s`) matches the new glob.

---

## Task 5: Tighten `times_in_subshell.sh` glob

**Files:**
- Modify: `e2e/posix_spec/2_14_13_times/times_in_subshell.sh`

Identical pattern change as Task 4, applied to the subshell variant.

- [ ] **Step 1: Replace both occurrences of the loose glob**

Final file contents should be:

```sh
#!/bin/sh
# POSIX_REF: 2.14.13 times
# DESCRIPTION: times works inside a subshell; output retains two-line shape
# EXPECT_OUTPUT omitted: CPU-time values are non-deterministic; shape verified in-script.
# EXPECT_EXIT: 0
out=$( ( times ) )
line1=$(echo "$out" | sed -n '1p')
line2=$(echo "$out" | sed -n '2p')
case "$line1" in
    [0-9]*m[0-9]*.[0-9]*s\ [0-9]*m[0-9]*.[0-9]*s) ;;
    *) echo "bad line1 in subshell: $line1" >&2; exit 1 ;;
esac
case "$line2" in
    [0-9]*m[0-9]*.[0-9]*s\ [0-9]*m[0-9]*.[0-9]*s) ;;
    *) echo "bad line2 in subshell: $line2" >&2; exit 1 ;;
esac
```

- [ ] **Step 2: Run the test**

Run: `./e2e/run_tests.sh --filter=times_in_subshell`
Expected: test passes.

- [ ] **Step 3: Commit both times fixes together**

```bash
git add e2e/posix_spec/2_14_13_times/times_format.sh e2e/posix_spec/2_14_13_times/times_in_subshell.sh
git commit -m "$(cat <<'EOF'
test(e2e): tighten times glob to digit-bounded shape

"*m*s *m*s" matched nonsense like "msms ms ms". Use
"[0-9]*m[0-9]*.[0-9]*s [0-9]*m[0-9]*.[0-9]*s" to require digits around
m/./s. yosh's builtin_times emits "{}m{:.3}s" so the decimal is always
present.

Context: TODO.md "Future: E2E Test Expansion" — times glob too permissive.
EOF
)"
```

---

## Task 6: Strengthen `rule10_reserved_quoted_not_recognized.sh`

**Files:**
- Modify: `e2e/posix_spec/2_10_shell_grammar/rule10_reserved_quoted_not_recognized.sh`

Current assertion is "exit ≠ 2" which false-passes on exit 0 if an `if` executable happens to be on PATH. Strengthen by forcing command lookup to miss (`PATH=/nonexistent/path`) and asserting exit == 127.

- [ ] **Step 1: Replace file contents**

Overwrite `e2e/posix_spec/2_10_shell_grammar/rule10_reserved_quoted_not_recognized.sh` with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 10 - Keyword recognition
# DESCRIPTION: A quoted reserved word in command position is looked up as a command, not recognized as a keyword
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
# If quoted 'if' were still recognized as the reserved word, `'if' true`
# would start an incomplete if-statement and yield a syntax error (exit 2).
# To rule out the false-pass where an 'if' executable happens to exist on
# PATH and returns 0, we force command lookup to miss via PATH=/nonexistent
# and assert the outcome is exactly 127 (command not found).
PATH=/nonexistent/path 'if' true 2>/dev/null
rc=$?
if [ "$rc" -eq 2 ]; then
    echo "syntax error detected (rc=2); quoted 'if' was treated as reserved word" >&2
    exit 1
fi
if [ "$rc" -ne 127 ]; then
    echo "expected 127 (command not found), got $rc" >&2
    exit 1
fi
echo ok
```

- [ ] **Step 2: Confirm permissions remain 644**

Run: `ls -l e2e/posix_spec/2_10_shell_grammar/rule10_reserved_quoted_not_recognized.sh`
Expected: mode starts with `-rw-r--r--`.

- [ ] **Step 3: Run the test**

Run: `./e2e/run_tests.sh --filter=rule10_reserved_quoted_not_recognized`
Expected: test passes. Quoted `'if'` goes through command lookup; with PATH=/nonexistent the lookup fails and yosh returns 127.

- [ ] **Step 4: Commit**

```bash
git add e2e/posix_spec/2_10_shell_grammar/rule10_reserved_quoted_not_recognized.sh
git commit -m "$(cat <<'EOF'
test(e2e): strengthen rule10 quoted reserved word assertion

Previously accepted any exit != 2, which false-passes on exit 0 if an
'if' executable exists on PATH. Force command lookup to miss via
PATH=/nonexistent/path and assert rc == 127 exactly.

Context: TODO.md "Future: E2E Test Expansion" — rule10 false-pass risk.
EOF
)"
```

---

## Task 7: Full regression sweep

- [ ] **Step 1: Run the full E2E suite**

Run: `./e2e/run_tests.sh`
Expected: all tests pass, no new failures introduced by the above fixes.

- [ ] **Step 2: Run unit + integration tests**

Run: `cargo test`
Expected: all tests pass (sanity check — plan touches no Rust code, but run to confirm).

- [ ] **Step 3: If any failure appears, STOP**

Diagnose before proceeding. Do not skip this gate; proceeding without a green sweep means a regression could hide inside the TODO.md cleanup commit.

---

## Task 8: Clean up TODO.md

**Files:**
- Modify: `TODO.md` — delete 5 lines

Per project convention (stored in memory), delete completed items entirely rather than using `[x]` markers.

- [ ] **Step 1: Delete the 5 completed items from the "Future: E2E Test Expansion" section**

Remove these specific lines from `TODO.md`:

1. `- [ ] Builtin test POSIX_REF values could use more specific section numbers (e.g., '2.14.3' instead of '2.14 Special Built-In Utilities')` — **DO NOT DELETE** (not in scope)

Only delete these 5:

- `- [ ] e2e/command_execution/echo_simple.sh has '755' permissions — should be '644' to match project convention`
- `- [ ] times_format.sh / times_in_subshell.sh glob too permissive — *m*s\ *m*s matches strings like 'msms ms ms'. Tighten to [0-9]*m[0-9]*.[0-9]*s\ [0-9]*m[0-9]*.[0-9]*s to enforce digits + decimal point (e2e/posix_spec/2_14_13_times/)`
- `- [ ] rule04_case_last_item_no_dsemi.sh both arms print the same output — a) echo a and x) echo a both print a, so the test verifies only "no syntax error" rather than "the no-;; arm executed". Restructure to a) echo FIRST ;; x) echo LAST with EXPECT_OUTPUT: LAST for tighter assertion (e2e/posix_spec/2_10_shell_grammar/rule04_case_last_item_no_dsemi.sh)`
- `- [ ] rule10_reserved_quoted_not_recognized.sh false-pass on exotic PATH — test passes on any exit ≠ 2, but a rogue /usr/bin/if executable would yield exit 0 and pass without exercising the command-lookup path. Strengthen by prefixing PATH=/nonexistent env 'if' true so the lookup definitely misses (e2e/posix_spec/2_10_shell_grammar/rule10_reserved_quoted_not_recognized.sh)`
- `- [ ] rule05_for_valid_name_ok.sh duplicates existing rule05_for_valid_name.sh — both files guard the same POSIX §2.10.2 Rule 5 valid-NAME regression (for <var> in a b [c]; do echo ...; done) under the same POSIX_REF. Consolidate into one (e2e/posix_spec/2_10_shell_grammar/)`

(The exact text varies slightly — match against the current TODO.md content and delete the 5 specific list items.)

- [ ] **Step 2: Verify TODO.md still parses as expected**

Run: `grep -c "^- \[ \]" TODO.md`
Expected: count is reduced by 5 vs. the pre-change count. No stray `[x]` markers introduced.

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
chore(todo): remove completed E2E test quality fixes

Delete the five TODO entries addressed by this change set:
- echo_simple.sh 644 permission fix
- rule05_for_valid_name duplicate removal
- rule04 case no-dsemi stronger assertion
- times glob digit-bounded tightening
- rule10 quoted reserved word 127-assertion
EOF
)"
```

---

## Self-Review Notes

Checked against `docs/superpowers/specs/2026-04-20-e2e-test-quality-fixes-design.md`:

- §Scope item 1 (echo_simple.sh 644) → Task 1 ✓
- §Scope item 2 (rule05 duplicate) → Task 2 ✓
- §Scope item 3 (rule04 weak assertion) → Task 3 ✓
- §Scope item 4 (times globs) → Tasks 4 + 5 ✓
- §Scope item 5 (rule10 false-pass) → Task 6 ✓
- §Verification → Task 7 ✓
- TODO.md cleanup (project convention) → Task 8 ✓

No TBD/placeholder text. All file paths absolute. All test assertions include expected outputs. No types or helpers introduced.
