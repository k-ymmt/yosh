# E2E `EXPECT_OUTPUT:` empty-form silent-skip fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix `e2e/run_tests.sh::parse_metadata` so `# EXPECT_OUTPUT:` (no trailing space) parses as "expect empty stdout" instead of silently skipping the stdout assertion.

**Architecture:** Single-function shell change to `parse_metadata()`. Extend the existing `EXPECT_OUTPUT` case branch with an alternative literal pattern for the empty form, then strip the optional single leading space from the captured value. Eight currently-affected test files become real assertions; all of them already produce empty stdout under yosh, so post-fix PASS/FAIL counts are unchanged.

**Tech Stack:** POSIX sh (`/bin/sh`), `cargo build` (debug yosh binary for the harness to invoke).

**Spec:** `docs/superpowers/specs/2026-05-13-e2e-expect-output-empty-form-fix-design.md`

---

## File Structure

**Modified files:**

- `e2e/run_tests.sh` — single-branch edit inside `parse_metadata()` (around line 223). One responsibility: parse one metadata line into shell variables consumed by the main loop.
- `TODO.md` — delete the L114 entry per project rule "Delete completed items rather than marking them with `[x]`" (CLAUDE.md).

**Not modified:**

- `e2e/README.md` — current wording (`EXPECT_OUTPUT` Required: No / exact match) is consistent with post-fix behaviour.
- Any of the 8 affected test files — they already produce empty stdout under yosh; no test-source changes needed.
- Other metadata branches (`EXPECT_EXIT` / `EXPECT_STDERR` / `POSIX_REF` / `DESCRIPTION` / `XFAIL`) — zero usage of the empty form across all 397 files; deferred per spec §3.

---

## Pre-flight

Before starting Task 1, verify the workspace state matches what the plan assumes. If any check fails, stop and reconcile before continuing.

- [ ] **Step 0.1: Confirm git tree is clean**

Run: `git status`
Expected: `nothing to commit, working tree clean` (the spec commit `a1e8eed` is already in main).

- [ ] **Step 0.2: Verify debug yosh binary is buildable and current**

Run: `cargo build`
Expected: build succeeds; `./target/debug/yosh` is executable.

If you do not yet have a debug build, this will take 1–3 minutes. The harness needs it to invoke yosh under test.

- [ ] **Step 0.3: Capture baseline E2E summary**

Run: `./e2e/run_tests.sh 2>&1 | tail -3 | tee /tmp/e2e-baseline.txt`
Expected: a summary line like `Total: N  Passed: P  Failed: F  Timedout: 0  XFail: X  XPass: 0` and exit code 0.

Note the exact counts — Task 6 will diff against them.

---

## Task 1: Reproduce the silent-skip bug (smoke test)

**Files:**
- Modify (temporarily): `e2e/builtin/echo_no_args.sh`

**Goal:** Prove the pre-fix harness silently skips the stdout assertion. After this task the file is reverted; no commit.

- [ ] **Step 1.1: Inject a stdout-violating line into one affected test**

Modify `e2e/builtin/echo_no_args.sh`. Current contents (verified 2026-05-13):

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - echo
# DESCRIPTION: echo with no arguments outputs only a newline
# EXPECT_OUTPUT:
echo
```

Change to:

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - echo
# DESCRIPTION: echo with no arguments outputs only a newline
# EXPECT_OUTPUT:
echo
echo unexpected
```

- [ ] **Step 1.2: Run only that test with the pre-fix harness**

Run: `./e2e/run_tests.sh --filter=echo_no_args`
Expected: `[PASS]  builtin/echo_no_args.sh` (this is the bug — stdout is no longer empty, but the test still passes because `meta_has_expect_output=0`).

If the test FAILs here, the bug has already been fixed by someone else and the plan is obsolete. Stop and investigate.

- [ ] **Step 1.3: Revert the test file**

Restore `e2e/builtin/echo_no_args.sh` to its original contents:

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - echo
# DESCRIPTION: echo with no arguments outputs only a newline
# EXPECT_OUTPUT:
echo
```

Run: `git diff e2e/builtin/echo_no_args.sh`
Expected: empty output (file matches HEAD).

**No commit for Task 1.** This task is a reproduction/verification step only.

---

## Task 2: Apply the harness fix

**Files:**
- Modify: `e2e/run_tests.sh:223-226`

**Goal:** Relax the `EXPECT_OUTPUT` case pattern so both empty and value-bearing forms are accepted.

- [ ] **Step 2.1: Edit `e2e/run_tests.sh::parse_metadata`**

Locate the existing branch (around line 223). Current code:

```sh
            "# EXPECT_OUTPUT: "*)
                meta_expect_output="${_line#"# EXPECT_OUTPUT: "}"
                meta_has_expect_output=1
                ;;
```

Replace with:

```sh
            "# EXPECT_OUTPUT:"|"# EXPECT_OUTPUT: "*)
                meta_expect_output="${_line#"# EXPECT_OUTPUT:"}"
                meta_expect_output="${meta_expect_output# }"
                meta_has_expect_output=1
                ;;
```

Three precise changes:
1. Case pattern gains literal alternative `"# EXPECT_OUTPUT:"` before `|`.
2. Parameter expansion prefix shortens from `"# EXPECT_OUTPUT: "` (trailing space) to `"# EXPECT_OUTPUT:"` (no trailing space).
3. New line `meta_expect_output="${meta_expect_output# }"` strips the optional single leading space from the captured value.

No other lines change.

- [ ] **Step 2.2: Verify the diff**

Run: `git diff e2e/run_tests.sh`
Expected: exactly the three changes above in the single branch. Nothing outside that branch should be modified.

---

## Task 3: Verify the fix catches the bug

**Files:**
- Modify (temporarily): `e2e/builtin/echo_no_args.sh`

**Goal:** With the harness fix applied but uncommitted, repeat Task 1's injection and confirm the test now FAILs.

- [ ] **Step 3.1: Re-inject the stdout-violating line**

Modify `e2e/builtin/echo_no_args.sh` to:

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - echo
# DESCRIPTION: echo with no arguments outputs only a newline
# EXPECT_OUTPUT:
echo
echo unexpected
```

- [ ] **Step 3.2: Run the targeted test against the post-fix harness**

Run: `./e2e/run_tests.sh --filter=echo_no_args`
Expected: `[FAIL]  builtin/echo_no_args.sh` with reason `Stdout mismatch`.

If the test PASSes, the fix in Task 2 did not take effect. Re-check the diff and re-run.

- [ ] **Step 3.3: Revert the test file**

Restore `e2e/builtin/echo_no_args.sh` to:

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - echo
# DESCRIPTION: echo with no arguments outputs only a newline
# EXPECT_OUTPUT:
echo
```

Run: `git diff e2e/builtin/echo_no_args.sh`
Expected: empty output.

**No commit for Task 3.** Verification only.

---

## Task 4: Verify each of the 8 affected files still passes

**Files:** none modified.

**Goal:** Run each of the 8 files individually under the post-fix harness and confirm they all PASS.

- [ ] **Step 4.1: Run each affected file with `--filter`**

Run:

```sh
for t in heredoc_empty dev_null for_empty if_false while_false_no_exec prefix_assignment_external echo_no_args unset_variable; do
    ./e2e/run_tests.sh --filter="$t" 2>&1 | grep -E '^\[(PASS|FAIL|XFAIL|TIME)\]'
done
```

Expected output (order may vary; `unset_variable` matches two files):

```
[PASS]  builtin/echo_no_args.sh
[PASS]  builtin/unset_variable.sh
[PASS]  command_execution/prefix_assignment_external.sh
[PASS]  control_flow/for_empty.sh
[PASS]  control_flow/if_false.sh
[PASS]  control_flow/while_false_no_exec.sh
[PASS]  redirection/dev_null.sh
[PASS]  redirection/heredoc_empty.sh
[PASS]  variable_and_expansion/unset_variable.sh
```

Notes:
- `builtin/unset_variable.sh` does not use `# EXPECT_OUTPUT:` (verified during exploration); it still appears here because `--filter=unset_variable` matches both files.
- The eight files listed in the spec all produce empty stdout under yosh, so they remain PASS.

If any file FAILs, capture the failure with `./e2e/run_tests.sh --filter=<name> --verbose` and investigate before continuing.

---

## Task 5: Run the full E2E suite and diff against baseline

**Files:** none modified.

**Goal:** Prove the fix has zero PASS/FAIL impact on the remaining 196 standard-form files (and the rest of the suite).

- [ ] **Step 5.1: Run the full suite**

Run: `./e2e/run_tests.sh 2>&1 | tail -3 | tee /tmp/e2e-postfix.txt`
Expected: summary line and exit code 0.

- [ ] **Step 5.2: Diff against baseline**

Run: `diff /tmp/e2e-baseline.txt /tmp/e2e-postfix.txt`
Expected: empty output (identical summaries).

If the diff is non-empty, investigate. Acceptable: none. The eight affected files all produce empty stdout pre- and post-fix; total PASS count is invariant.

---

## Task 6: Remove the TODO entry

**Files:**
- Modify: `TODO.md` (delete L114 entry)

**Goal:** Apply the CLAUDE.md rule "Delete completed items rather than marking them with `[x]`".

- [ ] **Step 6.1: Delete the L114 bullet**

Locate the bullet in `TODO.md` under `## Future: E2E Test Expansion`. Current text (one bullet, multi-line):

```markdown
- [ ] `e2e/run_tests.sh` `# EXPECT_OUTPUT:` empty-form silently skips stdout check — the case pattern at L223 is `"# EXPECT_OUTPUT: "*` (trailing space required), so 8 existing test files using the no-trailing-space `# EXPECT_OUTPUT:` form (`heredoc_empty.sh`, `dev_null.sh`, `for_empty.sh`, `if_false.sh`, `while_false_no_exec.sh`, `prefix_assignment_external.sh`, `echo_no_args.sh`, `unset_variable.sh`) silently disable the stdout assertion rather than asserting empty stdout. Fix: relax the case pattern to also match the no-trailing-space form, then re-verify those 8 files. Discovered 2026-05-12 during fd_close.sh strengthening.
```

Delete the entire bullet (including the leading `- [ ] ` and trailing newline). The two surrounding bullets (L112 and L113) remain untouched.

- [ ] **Step 6.2: Verify the diff**

Run: `git diff TODO.md`
Expected: a `-` diff covering exactly the deleted bullet, no insertions, no changes to L112 or L113.

---

## Task 7: Commit the fix

**Files:** `e2e/run_tests.sh`, `TODO.md`

**Goal:** Single commit containing the harness fix and the TODO cleanup.

- [ ] **Step 7.1: Stage the two files**

Run:

```sh
git add e2e/run_tests.sh TODO.md
git status
```

Expected: `Changes to be committed:` listing exactly those two files. No other files staged or untracked from this work (Tasks 1 and 3 reverted their edits).

- [ ] **Step 7.2: Commit**

Run:

```sh
git commit -m "$(cat <<'EOF'
fix(e2e): accept EXPECT_OUTPUT empty form as 'expect empty stdout' (TODO L114)

run_tests.sh::parse_metadata previously matched only the trailing-space
form `# EXPECT_OUTPUT: <value>`. The eight test files using the
no-trailing-space form `# EXPECT_OUTPUT:` to mean "expect empty stdout"
silently disabled the stdout assertion — meta_has_expect_output stayed
0 and the comparison block was skipped entirely. Relax the case branch
to accept both forms; the eight files now assert empty stdout (all of
them already produce empty stdout under yosh, so the full-suite
summary is unchanged).

Other metadata fields (EXPECT_EXIT / EXPECT_STDERR / POSIX_REF /
DESCRIPTION / XFAIL) have the same trailing-space-required pattern but
zero usage examples of the empty form across all 397 test files; left
unchanged per the design spec.

Original prompt: pick the highest-priority item from TODO.md "Future:
E2E Test Expansion" and address it. L114 selected over L112/L113
because it is a silent correctness bug in the harness rather than
coverage expansion.

Spec: docs/superpowers/specs/2026-05-13-e2e-expect-output-empty-form-fix-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Expected: commit succeeds with the message above; pre-commit hooks (if any) pass.

- [ ] **Step 7.3: Verify the commit**

Run: `git log -1 --stat`
Expected: 1 commit touching `e2e/run_tests.sh` and `TODO.md` only.

---

## Task 8: Post-merge sanity check (full suite + cargo test)

**Files:** none modified.

**Goal:** Per CLAUDE.md ("MUST run tests at the end of work sessions"), run both the E2E harness and the Rust test suite to confirm no regressions.

- [ ] **Step 8.1: Re-run the E2E suite**

Run: `./e2e/run_tests.sh 2>&1 | tail -3`
Expected: same summary line as `/tmp/e2e-postfix.txt`, exit code 0.

- [ ] **Step 8.2: Run cargo test (skip the wasm-component plugin crates)**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass. The default-members invocation skips the wasm plugin crates per CLAUDE.md guidance ("Avoid cargo build --workspace and cargo test --workspace").

This may take several minutes. If you have `release.sh test` available locally, that's the per-binary-parallel variant — either is acceptable.

If any test fails, investigate before declaring the task complete. The fix only touches `e2e/run_tests.sh`, so Rust test regressions would be unrelated (a separate issue to surface).

---

## Done criteria

- [ ] `e2e/run_tests.sh` accepts both `# EXPECT_OUTPUT:` and `# EXPECT_OUTPUT: <value>` forms.
- [ ] Pre-fix vs post-fix smoke (Task 1 vs Task 3) confirms the silent-skip is now caught.
- [ ] Eight target files all PASS post-fix (Task 4).
- [ ] Full E2E suite summary unchanged vs baseline (Task 5).
- [ ] `TODO.md` L114 entry removed (Task 6).
- [ ] Single commit landed on `main` (Task 7).
- [ ] `cargo test` passes (Task 8).
