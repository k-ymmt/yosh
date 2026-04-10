# E2E Test Runner Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 5 issues in `e2e/run_tests.sh` — heredoc parser bug, dead code, missing warning, incomplete metrics, and orphan process prevention.

**Architecture:** All changes are confined to `e2e/run_tests.sh`, a POSIX sh test runner. Tasks are ordered: parser fixes first, then execution changes, then reporting. Each task is independently testable via temporary fixture files + the existing E2E suite.

**Tech Stack:** POSIX sh

---

### Task 1: Fix Heredoc Parser Dropping First Empty Line

**Files:**
- Modify: `e2e/run_tests.sh` (parse_metadata function)

- [ ] **Step 1: Create test fixture exposing the bug**

Create `e2e/_runner_validation/heredoc_empty_first_line.sh`:

```sh
#!/bin/sh
# DESCRIPTION: Verify heredoc parser preserves empty first line
# EXPECT_OUTPUT<<END
# 
# hello
# END
echo ""
echo "hello"
```

- [ ] **Step 2: Run test to verify it fails (bug present)**

Run: `sh e2e/run_tests.sh --filter=_runner_validation/heredoc_empty_first_line`

Expected: `[FAIL]` with `Stdout mismatch` — parser loses the empty first line, so expected="hello" but actual="\nhello".

- [ ] **Step 3: Add `_heredoc_first` flag initialization**

In `parse_metadata()`, add `_heredoc_first=0` after the existing `_heredoc_buf=""` initialization:

```sh
    _in_heredoc=0
    _heredoc_delim=""
    _heredoc_buf=""
    _heredoc_first=0
```

- [ ] **Step 4: Set flag when entering heredoc block**

In the `"# EXPECT_OUTPUT<<"*` case, add `_heredoc_first=1`:

```sh
            "# EXPECT_OUTPUT<<"*)
                # Multi-line heredoc style: # EXPECT_OUTPUT<<DELIM
                _heredoc_delim="${_line#"# EXPECT_OUTPUT<<"}"
                _in_heredoc=1
                _heredoc_buf=""
                _heredoc_first=1
                ;;
```

- [ ] **Step 5: Replace content accumulation logic**

Replace:
```sh
            if [ -n "$_heredoc_buf" ]; then
                _heredoc_buf="${_heredoc_buf}
${_stripped}"
            else
                _heredoc_buf="$_stripped"
            fi
```

With:
```sh
            if [ "$_heredoc_first" = 1 ]; then
                _heredoc_buf="$_stripped"
                _heredoc_first=0
            else
                _heredoc_buf="${_heredoc_buf}
${_stripped}"
            fi
```

- [ ] **Step 6: Run test to verify it passes**

Run: `sh e2e/run_tests.sh --filter=_runner_validation/heredoc_empty_first_line`

Expected: `[PASS]`

- [ ] **Step 7: Run full E2E suite for regression**

Run: `sh e2e/run_tests.sh`

Expected: No new failures.

- [ ] **Step 8: Delete test fixture and commit**

```bash
rm -rf e2e/_runner_validation
git add e2e/run_tests.sh
git commit -m "fix(e2e): preserve empty first line in heredoc parser

_heredoc_buf empty check failed when first content line was empty.
Replace -n check with _heredoc_first flag to track first-line state.

Task: TODO.md E2E Test Runner Improvements — heredoc first empty line"
```

---

### Task 2: Add Warning for Unclosed EXPECT_OUTPUT Heredoc

**Files:**
- Modify: `e2e/run_tests.sh` (parse_metadata function)

- [ ] **Step 1: Create test fixture exposing the issue**

Create `e2e/_runner_validation/unclosed_heredoc.sh`:

```sh
#!/bin/sh
# DESCRIPTION: Test with unclosed heredoc — should trigger warning
# EXPECT_OUTPUT<<END
# some content
echo "hello"
```

- [ ] **Step 2: Run test to verify no warning is produced (current behavior)**

Run: `sh e2e/run_tests.sh --filter=_runner_validation/unclosed_heredoc 2>&1`

Expected: `[PASS]` with NO warning on stderr. The test passes because `meta_has_expect_output` stays 0 (no stdout validation).

- [ ] **Step 3: Add unclosed heredoc warning**

After the `done < "$_file"` line at the end of `parse_metadata()`, add:

```sh
    done < "$_file"

    if [ "$_in_heredoc" = 1 ]; then
        printf "Warning: unclosed EXPECT_OUTPUT heredoc (delimiter '%s') in %s\n" \
            "$_heredoc_delim" "$_file" >&2
    fi
```

- [ ] **Step 4: Run test to verify warning appears**

Run: `sh e2e/run_tests.sh --filter=_runner_validation/unclosed_heredoc 2>&1 | grep "Warning:"`

Expected: `Warning: unclosed EXPECT_OUTPUT heredoc (delimiter 'END') in .../unclosed_heredoc.sh`

- [ ] **Step 5: Run full E2E suite for regression**

Run: `sh e2e/run_tests.sh`

Expected: No new failures, no warnings (all existing tests have properly closed heredocs).

- [ ] **Step 6: Delete test fixture and commit**

```bash
rm -rf e2e/_runner_validation
git add e2e/run_tests.sh
git commit -m "feat(e2e): warn on unclosed EXPECT_OUTPUT heredoc

parse_metadata() now checks _in_heredoc at end of file and prints
a warning to stderr if the delimiter was never closed.

Task: TODO.md E2E Test Runner Improvements — unclosed heredoc warning"
```

---

### Task 3: Remove No-Op `normalize_trailing()` Function

**Files:**
- Modify: `e2e/run_tests.sh` (function definition + call sites)

- [ ] **Step 1: Delete function definition and comment**

Remove these lines:

```sh
# ── Helper: strip trailing newlines from a string ────────────────────
# We normalize by removing the final trailing newline(s) for comparison.
normalize_trailing() {
    # Use printf %s to strip trailing newline, then awk to handle content
    printf '%s' "$1"
}
```

- [ ] **Step 2: Replace call sites with inline printf**

Replace:
```sh
            _norm_expected=$(normalize_trailing "$meta_expect_output")
            _norm_actual=$(normalize_trailing "$actual_stdout")
```

With:
```sh
            _norm_expected=$(printf '%s' "$meta_expect_output")
            _norm_actual=$(printf '%s' "$actual_stdout")
```

- [ ] **Step 3: Run full E2E suite for regression**

Run: `sh e2e/run_tests.sh`

Expected: All tests pass — behavior is identical since `$()` was already doing the trailing newline stripping.

- [ ] **Step 4: Commit**

```bash
git add e2e/run_tests.sh
git commit -m "refactor(e2e): remove no-op normalize_trailing() function

\$() command substitution already strips trailing newlines, making
the function redundant. Inline printf '%s' at call sites instead.

Task: TODO.md E2E Test Runner Improvements — normalize_trailing no-op"
```

---

### Task 4: Use `exec` in Timeout Handler to Prevent Orphan Processes

**Files:**
- Modify: `e2e/run_tests.sh` (test execution + exit code reading)

- [ ] **Step 1: Replace subshell command with exec**

Replace:
```sh
    (
        "$SHELL_UNDER_TEST" "$test_file" >"$_stdout_file" 2>"$_stderr_file"
        echo $? >"$_exit_file"
    ) &
```

With:
```sh
    (
        exec "$SHELL_UNDER_TEST" "$test_file" >"$_stdout_file" 2>"$_stderr_file"
    ) &
```

- [ ] **Step 2: Capture exit code from `wait` return value**

Replace:
```sh
    wait "$_pid" 2>/dev/null
    kill "$_timer_pid" 2>/dev/null
```

With:
```sh
    wait "$_pid" 2>/dev/null
    _wait_status=$?
    kill "$_timer_pid" 2>/dev/null
```

- [ ] **Step 3: Change exit code reading to use wait status + timeout marker**

Replace:
```sh
    # Read results
    if [ -f "$_exit_file" ]; then
        actual_exit=$(cat "$_exit_file")
    else
        actual_exit="unknown"
    fi
```

With:
```sh
    # Read results — exit code from wait, timeout from marker file
    actual_exit=$_wait_status
    if [ -f "$_exit_file" ] && [ "$(cat "$_exit_file")" = "timeout" ]; then
        actual_exit="timeout"
    fi
```

- [ ] **Step 4: Run full E2E suite for regression**

Run: `sh e2e/run_tests.sh`

Expected: All tests pass — exit codes are captured identically via `wait`.

- [ ] **Step 5: Commit**

```bash
git add e2e/run_tests.sh
git commit -m "fix(e2e): use exec in timeout handler to prevent orphan processes

exec replaces the subshell with kish, so killing the PID kills kish
directly instead of only the parent subshell. Exit code is now captured
from wait return value; _exit_file is used only as a timeout marker.

Task: TODO.md E2E Test Runner Improvements — exec timeout handler"
```

---

### Task 5: Add `timedout` Counter Separate from `failed`

**Files:**
- Modify: `e2e/run_tests.sh` (counters, result reporting, summary, exit condition)

- [ ] **Step 1: Add `timedout` counter**

After the existing counter initialization block, add `timedout=0`:

```sh
total=0
passed=0
failed=0
xfailed=0
xpassed=0
timedout=0
```

- [ ] **Step 2: Add timeout branch to result reporting (before xfail)**

Replace the entire result reporting section:

```sh
    # ── Report result ────────────────────────────────────────────
    if [ -n "$meta_xfail" ]; then
        # Expected failure
        if [ "$_test_ok" = 1 ]; then
            xpassed=$((xpassed + 1))
            printf "${YELLOW}[XPASS]${RESET} %s (expected failure: %s)\n" "$rel_path" "$meta_xfail"
        else
            xfailed=$((xfailed + 1))
            printf "${CYAN}[XFAIL]${RESET} %s (%s)\n" "$rel_path" "$meta_xfail"
        fi
    else
        if [ "$_test_ok" = 1 ]; then
            passed=$((passed + 1))
            printf "${GREEN}[PASS]${RESET}  %s\n" "$rel_path"
        else
            failed=$((failed + 1))
            printf "${RED}[FAIL]${RESET}  %s\n" "$rel_path"
            printf "        %s\n" "$_failure_reason"
        fi
    fi
```

With:

```sh
    # ── Report result ────────────────────────────────────────────
    if [ "$actual_exit" = "timeout" ]; then
        timedout=$((timedout + 1))
        printf "${YELLOW}[TIME]${RESET}  %s\n" "$rel_path"
        printf "        Timed out after ${TIMEOUT}s\n"
    elif [ -n "$meta_xfail" ]; then
        # Expected failure
        if [ "$_test_ok" = 1 ]; then
            xpassed=$((xpassed + 1))
            printf "${YELLOW}[XPASS]${RESET} %s (expected failure: %s)\n" "$rel_path" "$meta_xfail"
        else
            xfailed=$((xfailed + 1))
            printf "${CYAN}[XFAIL]${RESET} %s (%s)\n" "$rel_path" "$meta_xfail"
        fi
    else
        if [ "$_test_ok" = 1 ]; then
            passed=$((passed + 1))
            printf "${GREEN}[PASS]${RESET}  %s\n" "$rel_path"
        else
            failed=$((failed + 1))
            printf "${RED}[FAIL]${RESET}  %s\n" "$rel_path"
            printf "        %s\n" "$_failure_reason"
        fi
    fi
```

- [ ] **Step 3: Guard verbose output for timed-out tests**

Replace the verbose output block:

```sh
    # Verbose output
    if [ "$VERBOSE" = 1 ]; then
        printf "        ${BOLD}Description:${RESET} %s\n" "${meta_description:-<none>}"
        [ -n "$meta_posix_ref" ] && printf "        ${BOLD}POSIX ref:${RESET}   %s\n" "$meta_posix_ref"
        printf "        ${BOLD}Exit code:${RESET}   %s (expected %s)\n" "$actual_exit" "$meta_expect_exit"
        if [ "$meta_has_expect_output" = 1 ]; then
            printf "        ${BOLD}Expected stdout:${RESET}\n"
            printf "          |%s\n" "$meta_expect_output"
            printf "        ${BOLD}Actual stdout:${RESET}\n"
            printf "          |%s\n" "$actual_stdout"
        fi
        if [ -n "$meta_expect_stderr" ]; then
            printf "        ${BOLD}Expected stderr substring:${RESET} %s\n" "$meta_expect_stderr"
            printf "        ${BOLD}Actual stderr:${RESET} %s\n" "$actual_stderr"
        fi
        printf "\n"
    fi
```

With:

```sh
    # Verbose output
    if [ "$VERBOSE" = 1 ]; then
        printf "        ${BOLD}Description:${RESET} %s\n" "${meta_description:-<none>}"
        [ -n "$meta_posix_ref" ] && printf "        ${BOLD}POSIX ref:${RESET}   %s\n" "$meta_posix_ref"
        if [ "$actual_exit" != "timeout" ]; then
            printf "        ${BOLD}Exit code:${RESET}   %s (expected %s)\n" "$actual_exit" "$meta_expect_exit"
            if [ "$meta_has_expect_output" = 1 ]; then
                printf "        ${BOLD}Expected stdout:${RESET}\n"
                printf "          |%s\n" "$meta_expect_output"
                printf "        ${BOLD}Actual stdout:${RESET}\n"
                printf "          |%s\n" "$actual_stdout"
            fi
            if [ -n "$meta_expect_stderr" ]; then
                printf "        ${BOLD}Expected stderr substring:${RESET} %s\n" "$meta_expect_stderr"
                printf "        ${BOLD}Actual stderr:${RESET} %s\n" "$actual_stderr"
            fi
        fi
        printf "\n"
    fi
```

- [ ] **Step 4: Add `Timedout` to summary output**

Replace:

```sh
printf "\n${BOLD}── Summary ──${RESET}\n"
printf "Total: %d  " "$total"
printf "${GREEN}Passed: %d${RESET}  " "$passed"
printf "${RED}Failed: %d${RESET}  " "$failed"
printf "${CYAN}XFail: %d${RESET}  " "$xfailed"
printf "${YELLOW}XPass: %d${RESET}\n" "$xpassed"
```

With:

```sh
printf "\n${BOLD}── Summary ──${RESET}\n"
printf "Total: %d  " "$total"
printf "${GREEN}Passed: %d${RESET}  " "$passed"
printf "${RED}Failed: %d${RESET}  " "$failed"
printf "${YELLOW}Timedout: %d${RESET}  " "$timedout"
printf "${CYAN}XFail: %d${RESET}  " "$xfailed"
printf "${YELLOW}XPass: %d${RESET}\n" "$xpassed"
```

- [ ] **Step 5: Add `timedout` to exit code failure condition**

Replace:

```sh
if [ "$failed" -gt 0 ] || [ "$xpassed" -gt 0 ]; then
    exit 1
fi
```

With:

```sh
if [ "$failed" -gt 0 ] || [ "$xpassed" -gt 0 ] || [ "$timedout" -gt 0 ]; then
    exit 1
fi
```

- [ ] **Step 6: Run full E2E suite for regression**

Run: `sh e2e/run_tests.sh`

Expected: All tests pass. Summary now shows `Timedout: 0`.

- [ ] **Step 7: Commit**

```bash
git add e2e/run_tests.sh
git commit -m "feat(e2e): add timedout counter separate from failed count

Timed-out tests now display as [TIME] and are counted separately in the
summary. Timeout takes priority over xfail in reporting.

Task: TODO.md E2E Test Runner Improvements — timedout counter"
```

---

### Task 6: Update TODO.md

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Remove completed items from E2E Test Runner Improvements section**

Delete the 5 completed items from the `## E2E Test Runner Improvements` section. If the section is now empty, delete the section header too.

- [ ] **Step 2: Run full E2E suite one final time**

Run: `sh e2e/run_tests.sh`

Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "docs: remove completed E2E Test Runner Improvements from TODO.md

All 5 items implemented: exec timeout handler, normalize_trailing removal,
unclosed heredoc warning, timedout counter, heredoc first empty line fix."
```
