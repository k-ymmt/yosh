# E2E `EXPECT_OUTPUT:` empty-form silent-skip fix

**Date:** 2026-05-13
**Status:** Design
**Scope:** TODO.md "Future: E2E Test Expansion" L114
**Files touched:** `e2e/run_tests.sh`, `TODO.md`

## 1. Problem

`e2e/run_tests.sh::parse_metadata` matches `EXPECT_OUTPUT` with the case
pattern `"# EXPECT_OUTPUT: "*)` — note the trailing space. The pattern only
matches the value-bearing form `# EXPECT_OUTPUT: <value>`. Eight existing
test files use the no-trailing-space form `# EXPECT_OUTPUT:` to mean
"expect empty stdout":

- `e2e/redirection/heredoc_empty.sh`
- `e2e/redirection/dev_null.sh`
- `e2e/control_flow/for_empty.sh`
- `e2e/control_flow/if_false.sh`
- `e2e/control_flow/while_false_no_exec.sh`
- `e2e/command_execution/prefix_assignment_external.sh`
- `e2e/builtin/echo_no_args.sh`
- `e2e/variable_and_expansion/unset_variable.sh`

For these files the case branch does not fire, `meta_has_expect_output`
stays `0`, and the stdout-comparison block (`run_tests.sh:341-351`) is
skipped entirely. The eight tests pass today as long as the exit code is
`0`, regardless of stdout. The test harness silently disables the very
assertion the test author wrote.

All eight tests currently *do* produce empty stdout under yosh (verified
2026-05-13), so the fix is purely a harness improvement — the eight
tests will continue to pass, but now actually verify their stdout claim.

## 2. Goal

Make `# EXPECT_OUTPUT:` (no trailing space, no value) parse as
"expect empty stdout" — equivalent to `# EXPECT_OUTPUT: ` followed by
nothing.

## 3. Non-goals

- **Other metadata fields** (`EXPECT_EXIT`, `EXPECT_STDERR`, `POSIX_REF`,
  `DESCRIPTION`, `XFAIL`) have the same trailing-space-required pattern
  but zero usage examples of the empty form across the 397-file suite
  (verified 2026-05-13). Fixing them risks spec-semantics arguments
  (e.g., should `# EXPECT_EXIT:` mean "exit 0" or "any exit"?) without
  any user benefit today. Defer until a real use case appears.
- **`e2e/README.md`** is not modified. The current "`EXPECT_OUTPUT`
  Required: No / Expected stdout (exact match)" wording is consistent
  with the post-fix behaviour and does not promise a particular spelling
  for "expect empty stdout".
- **TODO L112 / L113** (Chapter 4/8 expansion, normative-requirement
  granularity) are unrelated coverage-expansion items.

## 4. Design

### 4.1 Architecture

Single-function change to `parse_metadata()` in `e2e/run_tests.sh`.
Add a second alternative to the existing `EXPECT_OUTPUT` case branch so
both the empty form (`# EXPECT_OUTPUT:`, exact literal match) and the
existing value-bearing form (`# EXPECT_OUTPUT: …`) share one handler.

**Before** (`e2e/run_tests.sh:223-226`):

```sh
"# EXPECT_OUTPUT: "*)
    meta_expect_output="${_line#"# EXPECT_OUTPUT: "}"
    meta_has_expect_output=1
    ;;
```

**After:**

```sh
"# EXPECT_OUTPUT:"|"# EXPECT_OUTPUT: "*)
    meta_expect_output="${_line#"# EXPECT_OUTPUT:"}"
    meta_expect_output="${meta_expect_output# }"
    meta_has_expect_output=1
    ;;
```

### 4.2 Behaviour matrix

| Input line                       | Match? | `meta_expect_output` | `meta_has_expect_output` | Net effect              |
|----------------------------------|--------|----------------------|--------------------------|-------------------------|
| `# EXPECT_OUTPUT:`               | yes    | `""`                 | `1`                      | **new:** assert empty   |
| `# EXPECT_OUTPUT: hello`         | yes    | `"hello"`            | `1`                      | unchanged               |
| `# EXPECT_OUTPUT: ` (one trailing space) | yes | `""`            | `1`                      | unchanged               |
| `# EXPECT_OUTPUT:hello`          | no     | (untouched)          | (untouched, `0`)         | silent-skip (unchanged) |
| `# EXPECT_OUTPUT<<END`           | other branch | heredoc value | `1`                      | unchanged               |

The `# EXPECT_OUTPUT:hello` form (colon, no space, value) remains
silent-skipped, matching today's behaviour. A grep across all 397 test
files confirmed zero occurrences of that form, so the fix introduces no
behavioural change for any existing file other than the eight target
files.

### 4.3 Data flow

**Pre-fix:**
1. `parse_metadata` reads `# EXPECT_OUTPUT:` line
2. case `"# EXPECT_OUTPUT: "*` does not match (missing trailing space)
3. `meta_has_expect_output` stays `0`
4. Main loop (line 341) skips stdout comparison
5. Test passes purely on exit-code match

**Post-fix:**
1. `parse_metadata` reads `# EXPECT_OUTPUT:` line
2. case `"# EXPECT_OUTPUT:"` matches exactly
3. `_line#"# EXPECT_OUTPUT:"` yields `""`
4. `${meta_expect_output# }` strips the optional single leading space
   (`""` unchanged; for `"# EXPECT_OUTPUT: hello"` strips the space)
5. `meta_has_expect_output=1`
6. Main loop asserts `actual_stdout == ""`

### 4.4 Error handling

No new error paths. The fix removes a silent-skip; it does not introduce
a new failure mode. The eight affected files all produce empty stdout
under yosh today, so the harness output for those tests is unchanged.

## 5. Testing

### 5.1 Regression check

Run `./e2e/run_tests.sh` before and after the change and compare the
summary line (`Total / Passed / Failed / Timedout / XFail / XPass`). All
counts must be identical.

### 5.2 New-assertion smoke (one-time manual)

To prove the post-fix harness actually asserts what the test author
wrote, perform the following round-trip once during implementation:

1. Add a stray `echo unexpected` to
   `e2e/builtin/echo_no_args.sh` (or any one of the eight files).
2. Run the **pre-fix** harness — the test must still pass (demonstrates
   the silent-skip bug).
3. Apply the fix.
4. Run the **post-fix** harness — the test must fail with
   `Stdout mismatch`.
5. Revert the stray `echo unexpected`.

This is verification-only and is not committed.

### 5.3 Existing-form non-regression

Of the 397 test files, 196 use the standard `# EXPECT_OUTPUT: …`
form. They must produce identical PASS/FAIL outcomes after the change.
Covered by §5.1's full-suite diff — no additional check needed.

## 6. Cleanup

- Delete the TODO.md L114 entry per project rule "Delete completed
  items rather than marking them with `[x]`" (CLAUDE.md, "TODO.md"
  section).

## 7. Out of scope

- Same-shape fix for `EXPECT_EXIT` / `EXPECT_STDERR` / `POSIX_REF` /
  `DESCRIPTION` / `XFAIL` (zero current usage).
- L112 chapter-by-chapter coverage expansion.
- L113 normative-requirement granularity expansion.
- Any change to `e2e/README.md`.

## 8. Rollback

`git revert <commit>` restores the prior pattern. The eight affected
files are not modified, so revert is a pure harness rollback with no
data migration concerns.
