# POSIX TODO Cleanup — Design

**Date:** 2026-05-10
**Source prompt:** "TODO.md の中から、POSIX 準拠に関する項目を対応してください"
**Status:** Design approved (sections §1–§4), pending user review of this written spec.

## §1 Goal & Scope

### Goal

Consume the small, independent POSIX-compliance follow-ups currently parked
in `TODO.md` by adding the missing E2E tests, fixing weak/misleading existing
tests, and documenting the `POSIX_REF` metadata format. The aim is to retire
each addressed line from `TODO.md` outright (per the project convention of
deleting completed items rather than marking them `[x]`).

### In Scope

- 6 new E2E test files (POSIX gap fixes).
- 5 existing-test edits (rename, description fix, differentiation, comments).
- 1 Rust unit-test assertion tightening in `src/parser/simple.rs`.
- 1 documentation addendum to `e2e/README.md` (`POSIX_REF` format contract +
  Rule 9 taxonomy note).
- 1 legacy file deletion (after the migrated replacement lands).
- Defensive `: "${TEST_TMPDIR:?TEST_TMPDIR not set}"` insertion across all
  `TEST_TMPDIR`-dependent tests in `e2e/posix_spec/2_07_redirection/` and
  `e2e/posix_spec/2_14_13_times/`.
- Removal of corresponding lines from `TODO.md`.

### Out of Scope

The following POSIX-related items in `TODO.md` are intentionally **not**
addressed in this pass and remain on the backlog:

- Job Control: `disown`, `suspend`, pipeline display in `jobs`,
  `JobTable.shell_tmodes` startup snapshot semantics, Task 7 PTY assertion gap.
- Multi-byte IFS support in UTF-8 locale.
- `fork + run-Rust-shell-code-in-child` POSIX-UB architectural concern.
- Chapter 4 (Utilities) and Chapter 8 (Environment Variables) systematic E2E
  expansion.
- Chapter 2 normative-requirement-granularity deepening (+100–200 tests).
- Builtin test `POSIX_REF` value granularity (e.g., `2.14.3` instead of
  `2.14`).
- `fd_close.sh` actual-fd-close-effect verification.
- `tilde_rhs_command_prefix.sh` external `sh -c` dependency.
- E2E suite occasional transient failures.

## §2 New E2E Tests

All new files use 644 permissions and the existing
`POSIX_REF`/`DESCRIPTION`/`EXPECT_OUTPUT`/`EXPECT_EXIT` metadata header
convention. Each new file that touches `$TEST_TMPDIR` includes the defensive
guard from §3 D1.

### N1. `e2e/posix_spec/2_14_13_times/times_rejects_operand.sh`

Verifies that the `times` builtin rejects operands per POSIX §2.14.13. Yosh
writes a `yosh:`-prefixed error to stderr on rejection. The exact
`EXPECT_EXIT` value is determined at implementation time by running
`yosh -c 'times foo'` once and matching the observed exit status. Fallback
behavior on a divergence is described in §4 "Failure Mode N1".

```sh
#!/bin/sh
# POSIX_REF: 2.14.13 times
# DESCRIPTION: times rejects operands per POSIX (no operands accepted)
# EXPECT_EXIT: <observed>
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
err="$TEST_TMPDIR/times_err"
times foo 2>"$err"
status=$?
grep -q '^yosh:' "$err" || exit 99
test "$status" -ne 0
```

### N2. `e2e/posix_spec/2_10_1_lexical/line_continuation.sh`

Verifies §2.10.1 backslash-newline line splice: `echo a\<newline>b` produces
`ab`.

```sh
#!/bin/sh
# POSIX_REF: 2.10.1 Shell Grammar Lexical Conventions
# DESCRIPTION: backslash-newline is a line continuation (token splice)
# EXPECT_OUTPUT: ab
# EXPECT_EXIT: 0
echo a\
b
```

### N3. `e2e/posix_spec/2_07_redirection/dup_input_unquoted_fd.sh`

Symmetric counterpart to existing `dup_input_param_expansion.sh` (which uses
quoted `<&"$fd"`). Pins the unquoted-fd path so word-expansion-in-redirect
regressions cannot slip past the §2.7.5 suite.

```sh
#!/bin/sh
# POSIX_REF: 2.7.5 Duplicating an Input File Descriptor
# DESCRIPTION: <&N where N is unquoted parameter expansion still duplicates
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
f="$TEST_TMPDIR/dup_in_unquoted"
echo hello > "$f"
exec 3< "$f"
fd=3
cat <&$fd
exec 3<&-
```

### N4. `e2e/posix_spec/2_07_redirection/dup_output_unquoted_fd.sh`

Symmetric counterpart to N3 for §2.7.6.

```sh
#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: >&N where N is unquoted parameter expansion still duplicates
# EXPECT_OUTPUT: file:hello
# EXPECT_EXIT: 0
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
f="$TEST_TMPDIR/dup_out_unquoted"
exec 3> "$f"
fd=3
echo hello >&$fd
exec 3>&-
printf 'file:'
cat "$f"
```

### N5. `e2e/posix_spec/2_06_01_tilde_expansion/tilde_mixed_backslash_after_colon.sh`

Pipeline-level E2E counterpart to the existing parser-unit test
`assignment_rhs_backslash_tilde_after_colon_stays_literal`.

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: backslash-tilde after colon stays literal (no tilde expansion)
# EXPECT_OUTPUT: foo:~/bin
# EXPECT_EXIT: 0
x=foo:\~/bin
echo "$x"
```

### N6. `e2e/posix_spec/2_07_redirection/dup_output_stderr_to_stdout.sh`

Migration target for the legacy `e2e/redirection/stderr_to_stdout.sh`.
Restores `2>&1` coverage to the §2.7.6 canonical suite (the existing
`dup_output_basic.sh` covers `>&3` only).

```sh
#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: 2>&1 redirects stderr to stdout (canonical 2>&1 form)
# EXPECT_EXIT: 0
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
echo error_msg > "$TEST_TMPDIR/combined.txt" 2>&1
result=$(cat "$TEST_TMPDIR/combined.txt")
test "$result" = "error_msg"
```

### Decision: extend, don't fork the double-quote escape test

Rather than create a new `backslash_dq_special_chars.sh`, the existing
`e2e/quoting/backslash_special_in_dquotes.sh` (which already covers `\$`,
`\\`, `\"`) is extended in-place to add the missing `` \` ``
(backtick) case. This keeps all four POSIX §2.2.3 double-quote escapes
(`\$`, `\\`, `\"`, `` \` ``) co-located in a single file.

## §3 Test Edits, Documentation, and Cleanup

### M1. `src/parser/simple.rs` — tighten escaped-tilde-after-param assertion

The existing test `assignment_rhs_param_then_escaped_tilde_stays_literal`
currently asserts only the negative shape with
`!any(matches!(p, Tilde(_)))`. Replaced with a full structural
`assert_eq!(parts, vec![…])` mirroring the sibling
`assignment_rhs_param_then_tilde_no_colon_stays_literal`. The exact
`vec!` literal is derived at implementation time from the test's existing
input string and the sibling test's structure (both should be parallel).

### M2. `e2e/posix_spec/2_07_redirection/readwrite_bidirectional.sh` —
       rename to `readwrite_opens_fd.sh` + reword DESCRIPTION

Use `git mv` to preserve history. New DESCRIPTION:
`N<>file opens the file without error`. The body
(`exec 3<>file; exec 3<&-`) is unchanged — only the file name and one
header line.

### M3. `e2e/posix_spec/2_07_redirection/readwrite_param_expansion.sh` —
       differentiate from `readwrite_basic.sh`

Replace the variable-via-assignment redirect target with an inline
`${TEST_TMPDIR}/...` expansion in the redirect itself, so the test pins
parameter-expansion-inside-the-redirect-target rather than the prior
assignment expansion. Body becomes:

```sh
echo roundtrip 1<>"${TEST_TMPDIR}/rw_pe_direct"
cat "${TEST_TMPDIR}/rw_pe_direct"
```

### M4. `e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_user_form.sh` —
       NOTE comment

Add a single comment block before the test body explaining why
`EXPECT_OUTPUT` is intentionally omitted (`~root` resolution is
platform-dependent; correctness is verified in-script via `case`).

### M5. Rule 7/10 weak-intent NOTE comments

Add `# NOTE:` comments to:

- `e2e/posix_spec/2_10_shell_grammar/rule07_not_at_word_position.sh`
- `e2e/posix_spec/2_10_shell_grammar/rule10_reserved_after_cmd_is_arg.sh`
- `e2e/posix_spec/2_10_shell_grammar/rule10_reserved_after_pipe_in_cmdpos.sh`

mirroring the pattern from `rule10_reserved_quoted_not_recognized.sh`,
explaining "expected observable vs. regression-time observable" for each.
The exact prose is drafted per file at implementation time.

### D1. Defensive `$TEST_TMPDIR` guard

For every `.sh` file in `e2e/posix_spec/2_07_redirection/` and
`e2e/posix_spec/2_14_13_times/` that references `$TEST_TMPDIR`, insert

```sh
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
```

immediately after the metadata header comment block (i.e., before any
substantive command). Discovered targets are determined at implementation
time via `grep -l TEST_TMPDIR e2e/posix_spec/2_07_redirection/*.sh
e2e/posix_spec/2_14_13_times/*.sh`. Tests that don't reference it are
left untouched. New files (N1, N3, N4, N6) include the guard from
inception per §2.

### D2. `e2e/README.md` — `POSIX_REF` format contract + Rule 9 taxonomy

A new section, inserted after the existing "Writing Tests" section.

Format contract (accepted shapes):

- `2.X.Y <Section Name>` — e.g., `2.6.1 Tilde Expansion`
- `2.10.2 Rule N - <Name>` — e.g., `2.10.2 Rule 1 - First Word`
- `2.10.2 Rule N - <Name> (<discriminator>)` — for multi-case rules
- `2.10 Shell Grammar - <Topic>` — for cross-rule topics like
  terminator equality

The contract clarifies that a comprehensive grep needs to allow either
`Rule N` or topic forms, since `grep -E 'POSIX_REF: 2\.10\.2 Rule'` alone
will miss topic-shaped entries.

Rule 9 taxonomy note (one paragraph): the label "Rule 9" in test names
covers literal POSIX Rule 9 (function body) plus its grammar-level
generalizations to compound_command body and compound_list body
(distinguished by a parenthetical `<ctx>`).

### F1. Delete legacy `e2e/redirection/stderr_to_stdout.sh`

After N6 lands, the legacy file is removed via `git rm`. The §2.7.6
canonical suite under `e2e/posix_spec/2_07_redirection/` becomes the sole
home for `2>&1` and friends.

### F2. Remove addressed lines from `TODO.md`

Per the project convention (`TODO.md format` memory: delete completed
items, do not mark `[x]`). The lines to delete are identified by their
unique leading-text fingerprint — line numbers will have shifted by then.
Fingerprints (exact, unique within `TODO.md`):

- `times` operand rejection test missing — POSIX §2.14.13
- §2.10.1 backslash-newline line-continuation test missing
- `dup_input_*.sh` missing unquoted-fd variant
- `dup_output_*.sh` missing unquoted-fd variant
- E2E counterpart for `x=foo:\~/bin` escape case missing
- Double-quote escape coverage gap
- `tilde_rhs_user_form.sh` documents absence of `EXPECT_OUTPUT`
- `readwrite_bidirectional.sh` description and name overstate body
- `readwrite_basic.sh` and `readwrite_param_expansion.sh` are near-duplicates
- Legacy `e2e/redirection/stderr_to_stdout.sh` migration
- E2E test defensive `$TEST_TMPDIR` check
- Rule 7/10 weak-intent tests need failure-signature comments
- POSIX_REF format contract is undocumented
- Rule 9 taxonomy needs a disambiguation note
- `assignment_rhs_param_then_escaped_tilde_stays_literal` assertion is loose

15 lines total.

## §4 Verification Strategy

### Approach

This work is mostly test additions and test fixes; no production logic
changes. Verification has two halves:

1. The new and modified tests behave as intended (they pass and pin the
   intended POSIX behavior).
2. No existing test regresses.

### Steps

Run in order:

1. **Build:** `cargo build` (CLAUDE.md: debug build is required for E2E).
2. **Targeted new-test runs:**
   ```
   ./e2e/run_tests.sh --filter=times_rejects_operand
   ./e2e/run_tests.sh --filter=line_continuation
   ./e2e/run_tests.sh --filter=dup_input_unquoted_fd
   ./e2e/run_tests.sh --filter=dup_output_unquoted_fd
   ./e2e/run_tests.sh --filter=tilde_mixed_backslash_after_colon
   ./e2e/run_tests.sh --filter=dup_output_stderr_to_stdout
   ```
3. **Targeted modified-test runs:**
   ```
   ./e2e/run_tests.sh --filter=readwrite_opens_fd
   ./e2e/run_tests.sh --filter=readwrite_param_expansion
   ./e2e/run_tests.sh --filter=tilde_rhs_user_form
   ./e2e/run_tests.sh --filter=rule07
   ./e2e/run_tests.sh --filter=rule10
   ./e2e/run_tests.sh --filter=backslash_special_in_dquotes
   ```
4. **Full E2E regression:** `./e2e/run_tests.sh` — must show same pass
   count as pre-change baseline plus the 6 new tests, with no new failures.
5. **Rust unit test for M1:**
   `cargo test -p yosh -- assignment_rhs_param_then_escaped_tilde`
   plus the sibling tests in the same module, to confirm the structural
   `vec!` assertion holds and parallel tests still pass.
6. **Full Rust test suite (background, per timeout-risk memory):**
   `cargo test`.

### Failure Mode N1

If yosh accepts `times foo` silently (i.e., POSIX violation, not just an
exit-code mismatch), N1 cannot be added as-is. Two acceptable
contingencies:

- **Preferred:** drop N1 from this PR, add a TODO line
  `times operand rejection not implemented` (POSIX-violation, separate
  scope), and ship the other 7 items.
- **Alternative:** mark N1 `XFAIL` (per the existing harness convention)
  and keep it in to register the gap in tests.

If the divergence is only the exact exit code (e.g., yosh returns 1
instead of 2), record the observed value in `EXPECT_EXIT` and proceed.
The "yosh's exit code matches CLAUDE.md's `2 = usage error` convention"
question is a separate hygiene issue tracked outside this PR.

### Other Failure Modes

Standard: investigate root cause per `superpowers:systematic-debugging`,
fix, re-run from step 2.

## §5 Commits & Branch Strategy

### Branch

Direct work on `main` (per project memory `feedback_main_branch_direct`).

### Commit Plan

Two commits, in this order:

1. **`test(e2e): add POSIX gap tests and quality fixes`**
   - 6 new files (N1–N6).
   - 5 existing-file edits (M2–M5, plus the `\`` extension to
     `backslash_special_in_dquotes.sh`).
   - 1 file deletion (F1).
   - `e2e/README.md` addendum (D2).
   - `$TEST_TMPDIR` guard insertion across affected files (D1).
   - `TODO.md` line removals (F2).
   - Body must include the source prompt verbatim per global CLAUDE.md.

2. **`test(parser): tighten escaped-tilde-after-param assignment assertion`**
   - `src/parser/simple.rs` — M1 only.
   - Body must include the source prompt verbatim per global CLAUDE.md.

### Rollback

Each commit is independently revertable: commit 1 is test-only and never
changes shipped binaries or library behavior; commit 2 strengthens a unit
test assertion and changes nothing executable.

## §6 Acceptance

The work is complete when:

- All 6 new E2E tests pass.
- All 5 modified E2E tests still pass and embody the intended distinction.
- The `\`` case is exercised in `backslash_special_in_dquotes.sh`.
- The legacy `e2e/redirection/stderr_to_stdout.sh` is gone.
- `$TEST_TMPDIR`-dependent tests in `2_07_redirection/` and `2_14_13_times/`
  guard the variable.
- `e2e/README.md` documents the `POSIX_REF` format contract and the
  Rule 9 taxonomy.
- `cargo test` and `./e2e/run_tests.sh` are clean (or have no new
  failures relative to the pre-change baseline).
- The 15 fingerprinted lines are gone from `TODO.md`.
- Both commits land on `main` with the source-prompt provenance line in
  their bodies.
