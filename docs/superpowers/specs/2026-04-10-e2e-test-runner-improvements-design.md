# E2E Test Runner Improvements Design

## Overview

Five improvements to `e2e/run_tests.sh` addressing orphan processes, dead code, missing warnings, incomplete metrics, and a heredoc parsing bug.

## Target File

`e2e/run_tests.sh` — POSIX sh E2E test runner for kish.

## Changes

### 1. Timeout Handler: Use `exec` to Prevent Orphan Processes

**Problem:** The test runner launches kish inside a subshell `(...)`. When the timeout fires, it kills the subshell PID, but any child processes forked by kish become orphans.

**Solution:** Use `exec` inside the subshell so that kish replaces the subshell process. Killing `$_pid` then kills kish directly.

**Implementation:**
- Replace `"$SHELL_UNDER_TEST" "$test_file" > ... 2> ...; echo $? > "$_exit_file"` with `exec "$SHELL_UNDER_TEST" "$test_file" > ... 2> ...`
- Remove `echo $? > "$_exit_file"` (unreachable after `exec`)
- Capture exit code from `wait $_pid` return value (`$?`)
- Keep `_exit_file` only as a timeout marker — timer process writes `"timeout"` to it
- After `wait`, check: if `_exit_file` contains `"timeout"`, set `actual_exit="timeout"`; otherwise use `$?`

### 2. Remove `normalize_trailing()` No-Op Function

**Problem:** `normalize_trailing()` uses `printf '%s'` to strip trailing newlines, but callers invoke it via `$()` command substitution which already strips trailing newlines. The function adds no value.

**Solution:** Delete the function and its comment block (lines 64-69). Replace call sites with direct `printf '%s'` inside `$()`:
```sh
_norm_expected=$(printf '%s' "$meta_expect_output")
_norm_actual=$(printf '%s' "$actual_stdout")
```

### 3. Warn on Unclosed `EXPECT_OUTPUT` Heredoc

**Problem:** If a test file has `# EXPECT_OUTPUT<<DELIM` but the closing `# DELIM` is missing, `parse_metadata()` silently ignores it. `meta_has_expect_output` stays 0, so the test runs without stdout validation — hiding a metadata authoring mistake.

**Solution:** After the `while` loop in `parse_metadata()`, check if `_in_heredoc` is still 1. If so, print a warning to stderr:
```sh
if [ "$_in_heredoc" = 1 ]; then
    printf "Warning: unclosed EXPECT_OUTPUT heredoc (delimiter '%s') in %s\n" \
        "$_heredoc_delim" "$_file" >&2
fi
```

The test continues execution with no stdout check (existing behavior), but the developer is alerted to fix the metadata.

### 4. Add `timedout` Counter Separate from `failed`

**Problem:** Timed-out tests increment the `failed` counter, making it impossible to distinguish real failures from timeouts in the summary.

**Solution:**
- Add `timedout=0` counter alongside existing counters
- When a test times out, increment `timedout` instead of `failed`
- Display result as `[TIME]` instead of `[FAIL]`
- Add `Timedout: N` to summary output
- Include `timedout > 0` in the exit code failure condition

### 5. Fix Heredoc Parser Dropping First Empty Line

**Problem:** In the heredoc content accumulator, the code checks `if [ -n "$_heredoc_buf" ]` to decide whether to prepend a newline. When the first content line is empty (after stripping `# `), both `_heredoc_buf` and `_stripped` are empty, so `_heredoc_buf="$_stripped"` assigns empty to empty — the line is silently lost.

**Solution:** Replace the `-n` check with a `_heredoc_first` flag:
- Set `_heredoc_first=1` when entering a heredoc block (alongside `_in_heredoc=1`)
- On first line (`_heredoc_first=1`): assign `_heredoc_buf="$_stripped"` unconditionally, set `_heredoc_first=0`
- On subsequent lines: append with newline prefix `_heredoc_buf="${_heredoc_buf}\n${_stripped}"`

This correctly preserves empty lines at any position in the heredoc content.

## Testing Strategy

- **Item 1 (exec):** Run existing E2E tests to verify no regression. Manually test with a long-running script to verify timeout kills kish properly.
- **Item 2 (normalize_trailing removal):** Run existing E2E tests — stdout comparisons must still pass.
- **Item 3 (unclosed heredoc warning):** Create a temporary test file with an unclosed heredoc, verify warning appears on stderr.
- **Item 4 (timedout counter):** Create a temporary test that sleeps beyond `TIMEOUT`, verify `[TIME]` label and summary counter.
- **Item 5 (heredoc first empty line):** Create a test file with an empty first line in `EXPECT_OUTPUT` heredoc, verify correct matching.

## Scope

All changes are confined to `e2e/run_tests.sh`. No changes to kish source code or existing test files.
