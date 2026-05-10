# POSIX TODO Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Retire 8 small POSIX-compliance follow-ups from `TODO.md` by adding 6 new E2E tests, fixing 5 existing tests, tightening 1 parser unit test, documenting the `POSIX_REF` metadata format, migrating one legacy test, and adding defensive `$TEST_TMPDIR` guards.

**Architecture:** Test-only changes plus a documentation addendum. Two commits on `main`: commit 1 collects all E2E + docs + TODO.md changes; commit 2 isolates the single Rust unit-test edit. No production code is touched.

**Tech Stack:** POSIX shell (E2E tests), Markdown (docs), Rust (one parser unit-test edit).

**Spec:** `docs/superpowers/specs/2026-05-10-posix-todo-cleanup-design.md`

---

## File Inventory

### Files to Create

- `e2e/posix_spec/2_14_13_times/times_rejects_operand.sh`
- `e2e/posix_spec/2_10_1_lexical/line_continuation.sh`
- `e2e/posix_spec/2_07_redirection/dup_input_unquoted_fd.sh`
- `e2e/posix_spec/2_07_redirection/dup_output_unquoted_fd.sh`
- `e2e/posix_spec/2_06_01_tilde_expansion/tilde_mixed_backslash_after_colon.sh`
- `e2e/posix_spec/2_07_redirection/dup_output_stderr_to_stdout.sh`

### Files to Modify

- `e2e/quoting/backslash_special_in_dquotes.sh` (add `` \` `` line)
- `e2e/posix_spec/2_07_redirection/readwrite_bidirectional.sh` → renamed to `readwrite_opens_fd.sh` (via `git mv`)
- `e2e/posix_spec/2_07_redirection/readwrite_param_expansion.sh` (differentiate from basic)
- `e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_user_form.sh` (add NOTE)
- `e2e/posix_spec/2_10_shell_grammar/rule07_not_at_word_position.sh` (add NOTE)
- `e2e/posix_spec/2_10_shell_grammar/rule10_reserved_after_cmd_is_arg.sh` (add NOTE)
- `e2e/posix_spec/2_10_shell_grammar/rule10_reserved_after_pipe_in_cmdpos.sh` (add NOTE)
- `e2e/posix_spec/2_07_redirection/*.sh` (add `$TEST_TMPDIR` guard to 10 existing files using it: `dup_input_basic.sh`, `dup_input_close.sh`, `dup_input_param_expansion.sh`, `dup_output_basic.sh`, `dup_output_close.sh`, `dup_output_param_expansion.sh`, `readwrite_basic.sh`, `readwrite_creates_file.sh`, `readwrite_param_expansion.sh`, `readwrite_bidirectional.sh` — note the bidirectional one is renamed in Task 9, so apply the guard before or after with the correct path)
- `e2e/README.md` (POSIX_REF format contract + Rule 9 taxonomy section)
- `TODO.md` (delete 15 fingerprinted lines)
- `src/parser/simple.rs` (tighten one unit-test assertion at line ~321)

### Files to Delete

- `e2e/redirection/stderr_to_stdout.sh` (legacy, replaced by N6)

---

## Task 1: Establish baseline for `times` operand-rejection behavior

**Why:** The N1 test (`times_rejects_operand.sh`) needs an `EXPECT_EXIT` value matching yosh's actual behavior. POSIX §2.14.13 says `times` takes no operands but doesn't pin a specific exit code. The CLAUDE.md convention is `2 = usage error`, but verify what yosh actually returns before writing the test.

**Files:** none (read-only investigation)

- [ ] **Step 1: Build yosh in debug mode**

```bash
cargo build
```

Expected: build succeeds, produces `target/debug/yosh`.

- [ ] **Step 2: Probe `times foo` exit code and stderr**

```bash
./target/debug/yosh -c 'times foo'; echo "exit=$?"
./target/debug/yosh -c 'times foo' 2>&1 1>/dev/null
```

Record:
- The exit code (e.g., `2`).
- Whether stderr contains a `yosh:` prefix.

- [ ] **Step 3: Decide path forward based on observation**

| Observation | Action in Task 2 |
|---|---|
| Non-zero exit + `yosh:` prefix on stderr | Set `EXPECT_EXIT` to the observed value. Proceed. |
| Exit `0` (yosh accepts operand silently — POSIX violation) | Skip Task 2. Add a TODO.md line `times operand rejection not implemented (POSIX violation)` instead. Document this divergence at end of session. |
| Non-zero exit but no `yosh:` prefix | Set `EXPECT_EXIT` to observed value. Adjust the `grep` line in N1 from `'^yosh:'` to whatever prefix is actually emitted (or remove the prefix check if there's no diagnostic at all). |

Record the chosen EXPECT_EXIT value and any test-body adjustments needed.

- [ ] **Step 4: No commit (investigation only)**

This task is read-only. Move to Task 2 with the observed values.

---

## Task 2: N1 — `times_rejects_operand.sh`

**Why:** Pin POSIX §2.14.13 "times takes no operands" at the E2E layer. Currently no test exists for the operand-rejection path.

**Files:**
- Create: `e2e/posix_spec/2_14_13_times/times_rejects_operand.sh`

- [ ] **Step 1: Create the file**

Replace `<EXIT>` with the value observed in Task 1 (typically `2`).

```sh
#!/bin/sh
# POSIX_REF: 2.14.13 times
# DESCRIPTION: times rejects operands per POSIX (no operands accepted)
# EXPECT_EXIT: <EXIT>
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
err="$TEST_TMPDIR/times_err"
times foo 2>"$err"
status=$?
grep -q '^yosh:' "$err" || exit 99
test "$status" -ne 0
```

- [ ] **Step 2: Set 644 permissions (per CLAUDE.md)**

```bash
chmod 644 e2e/posix_spec/2_14_13_times/times_rejects_operand.sh
```

- [ ] **Step 3: Run the test**

```bash
./e2e/run_tests.sh --filter=times_rejects_operand
```

Expected: PASS.

- [ ] **Step 4: No commit (batched in Task 17)**

---

## Task 3: N2 — `line_continuation.sh`

**Why:** Pin POSIX §2.10.1 backslash-newline line splice. The §2.10.1 directory currently has only `comment_terminates_at_newline.sh`, `operator_extended_max_munch.sh`, `operator_vs_word.sh` — no line-continuation test.

**Files:**
- Create: `e2e/posix_spec/2_10_1_lexical/line_continuation.sh`

- [ ] **Step 1: Create the file**

```sh
#!/bin/sh
# POSIX_REF: 2.10.1 Shell Grammar Lexical Conventions
# DESCRIPTION: backslash-newline is a line continuation (token splice)
# EXPECT_OUTPUT: ab
# EXPECT_EXIT: 0
echo a\
b
```

- [ ] **Step 2: Set 644 permissions**

```bash
chmod 644 e2e/posix_spec/2_10_1_lexical/line_continuation.sh
```

- [ ] **Step 3: Run the test**

```bash
./e2e/run_tests.sh --filter=line_continuation
```

Expected: PASS, output `ab`.

- [ ] **Step 4: No commit (batched in Task 17)**

---

## Task 4: N3 — `dup_input_unquoted_fd.sh`

**Why:** §2.7.5 currently covers quoted parameter expansion in `<&"$fd"` (via `dup_input_param_expansion.sh`). The unquoted form `<&$fd` is not covered, so word-expansion-in-redirect regressions could slip past.

**Files:**
- Create: `e2e/posix_spec/2_07_redirection/dup_input_unquoted_fd.sh`

- [ ] **Step 1: Create the file**

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

- [ ] **Step 2: Set 644 permissions**

```bash
chmod 644 e2e/posix_spec/2_07_redirection/dup_input_unquoted_fd.sh
```

- [ ] **Step 3: Run the test**

```bash
./e2e/run_tests.sh --filter=dup_input_unquoted_fd
```

Expected: PASS, output `hello`.

- [ ] **Step 4: No commit (batched in Task 17)**

---

## Task 5: N4 — `dup_output_unquoted_fd.sh`

**Why:** Symmetric to Task 4 for §2.7.6 output duplication.

**Files:**
- Create: `e2e/posix_spec/2_07_redirection/dup_output_unquoted_fd.sh`

- [ ] **Step 1: Create the file**

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

- [ ] **Step 2: Set 644 permissions**

```bash
chmod 644 e2e/posix_spec/2_07_redirection/dup_output_unquoted_fd.sh
```

- [ ] **Step 3: Run the test**

```bash
./e2e/run_tests.sh --filter=dup_output_unquoted_fd
```

Expected: PASS, output `file:hello`.

- [ ] **Step 4: No commit (batched in Task 17)**

---

## Task 6: N5 — `tilde_mixed_backslash_after_colon.sh`

**Why:** The parser-unit test `assignment_rhs_backslash_tilde_after_colon_stays_literal` (in `src/parser/simple.rs:311`) pins this at the AST level, but no E2E currently exercises the full parser → expander pipeline for `x=foo:\~/bin`.

**Files:**
- Create: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_mixed_backslash_after_colon.sh`

- [ ] **Step 1: Create the file**

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: backslash-tilde after colon stays literal (no tilde expansion)
# EXPECT_OUTPUT: foo:~/bin
# EXPECT_EXIT: 0
x=foo:\~/bin
echo "$x"
```

- [ ] **Step 2: Set 644 permissions**

```bash
chmod 644 e2e/posix_spec/2_06_01_tilde_expansion/tilde_mixed_backslash_after_colon.sh
```

- [ ] **Step 3: Run the test**

```bash
./e2e/run_tests.sh --filter=tilde_mixed_backslash_after_colon
```

Expected: PASS, output `foo:~/bin`.

- [ ] **Step 4: No commit (batched in Task 17)**

---

## Task 7: N6 — `dup_output_stderr_to_stdout.sh` (migration target for legacy file)

**Why:** The §2.7.6 canonical suite has no `2>&1` form (only `>&3` via `dup_output_basic.sh`). Migrate the legacy `e2e/redirection/stderr_to_stdout.sh` here with proper `POSIX_REF` metadata. The legacy file is deleted in Task 15 after this lands.

**Files:**
- Create: `e2e/posix_spec/2_07_redirection/dup_output_stderr_to_stdout.sh`

- [ ] **Step 1: Create the file**

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

- [ ] **Step 2: Set 644 permissions**

```bash
chmod 644 e2e/posix_spec/2_07_redirection/dup_output_stderr_to_stdout.sh
```

- [ ] **Step 3: Run the test**

```bash
./e2e/run_tests.sh --filter=dup_output_stderr_to_stdout
```

Expected: PASS.

- [ ] **Step 4: No commit (batched in Task 17)**

---

## Task 8: Extend `backslash_special_in_dquotes.sh` to cover `` \` ``

**Why:** The TODO line "Double-quote escape coverage gap" calls for compound coverage of all four POSIX §2.2.3 double-quote escapes (`\$`, `\\`, `\"`, `` \` ``). The existing file already covers the first three; only the backtick case is missing. Extend in place rather than creating a new file (keeps all four escapes co-located).

**Files:**
- Modify: `e2e/quoting/backslash_special_in_dquotes.sh`

Current contents (for reference):

```sh
#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Inside double quotes only \$ \` \" \\ \newline are special
# EXPECT_OUTPUT<<END
# $HOME
# "quoted"
# back\slash
# END
echo "\$HOME"
echo "\"quoted\""
echo "back\\slash"
```

- [ ] **Step 1: Extend the EXPECT_OUTPUT block and add the backtick `echo`**

Use the Edit tool. Change the `EXPECT_OUTPUT<<END` block to add a `tick` line and append `echo "tick\`mark"` to the body.

After-state:

```sh
#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Inside double quotes only \$ \` \" \\ \newline are special
# EXPECT_OUTPUT<<END
# $HOME
# "quoted"
# back\slash
# tick`mark
# END
echo "\$HOME"
echo "\"quoted\""
echo "back\\slash"
echo "tick\`mark"
```

- [ ] **Step 2: Run the test**

```bash
./e2e/run_tests.sh --filter=backslash_special_in_dquotes
```

Expected: PASS.

- [ ] **Step 3: No commit (batched in Task 17)**

---

## Task 9: M2 — Rename `readwrite_bidirectional.sh` → `readwrite_opens_fd.sh`

**Why:** The TODO line says the current name and DESCRIPTION overstate the body — it's an open-then-close smoke, not a bidirectional roundtrip.

**Files:**
- Rename: `e2e/posix_spec/2_07_redirection/readwrite_bidirectional.sh` → `e2e/posix_spec/2_07_redirection/readwrite_opens_fd.sh`
- Modify: DESCRIPTION metadata of the renamed file

Current contents (after rename, before edit):

```sh
#!/bin/sh
# POSIX_REF: 2.7.7 Open File Descriptors for Reading and Writing
# DESCRIPTION: N<>file accepts both read and write redirects on the same fd
# EXPECT_OUTPUT omitted: POSIX does not specify read-pointer position after write; only that opening with <> succeeds.
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/rw_bidir"
echo seed > "$f"
exec 3<>"$f"
exec 3<&-
```

- [ ] **Step 1: Rename via `git mv` to preserve history**

```bash
git mv e2e/posix_spec/2_07_redirection/readwrite_bidirectional.sh \
       e2e/posix_spec/2_07_redirection/readwrite_opens_fd.sh
```

- [ ] **Step 2: Edit the DESCRIPTION line**

Change the DESCRIPTION line from:

```
# DESCRIPTION: N<>file accepts both read and write redirects on the same fd
```

to:

```
# DESCRIPTION: N<>file opens the file without error
```

Also rewrite the EXPECT_OUTPUT-omitted comment to reflect the narrowed claim:

```
# EXPECT_OUTPUT omitted: this is an open-then-close smoke, not a roundtrip.
```

After-state:

```sh
#!/bin/sh
# POSIX_REF: 2.7.7 Open File Descriptors for Reading and Writing
# DESCRIPTION: N<>file opens the file without error
# EXPECT_OUTPUT omitted: this is an open-then-close smoke, not a roundtrip.
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/rw_bidir"
echo seed > "$f"
exec 3<>"$f"
exec 3<&-
```

- [ ] **Step 3: Run the test**

```bash
./e2e/run_tests.sh --filter=readwrite_opens_fd
```

Expected: PASS.

- [ ] **Step 4: No commit (batched in Task 17)**

---

## Task 10: M3 — Differentiate `readwrite_param_expansion.sh` from `readwrite_basic.sh`

**Why:** Both currently do `f="$TEST_TMPDIR/..."; echo X 1<>"$f"; cat "$f"`, so they pin the same code path. Move the parameter-expansion path into the redirect target itself so the test pins expansion-inside-redirect specifically.

**Files:**
- Modify: `e2e/posix_spec/2_07_redirection/readwrite_param_expansion.sh`

Current contents:

```sh
#!/bin/sh
# POSIX_REF: 2.7.7 Open File Descriptors for Reading and Writing
# DESCRIPTION: N<>"$file" accepts a filename via parameter expansion
# EXPECT_OUTPUT: roundtrip
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/rw_pe"
echo roundtrip 1<>"$f"
cat "$f"
```

- [ ] **Step 1: Replace the body**

After-state:

```sh
#!/bin/sh
# POSIX_REF: 2.7.7 Open File Descriptors for Reading and Writing
# DESCRIPTION: N<>"${var}/path" expands ${...} inside the redirect target itself
# EXPECT_OUTPUT: roundtrip
# EXPECT_EXIT: 0
echo roundtrip 1<>"${TEST_TMPDIR}/rw_pe_direct"
cat "${TEST_TMPDIR}/rw_pe_direct"
```

- [ ] **Step 2: Run the test**

```bash
./e2e/run_tests.sh --filter=readwrite_param_expansion
```

Expected: PASS.

- [ ] **Step 3: No commit (batched in Task 17)**

---

## Task 11: M4 — Add NOTE comment to `tilde_rhs_user_form.sh`

**Why:** The test omits `EXPECT_OUTPUT` because `~root` resolution is platform-dependent and verifies correctness in-script via `case`. The reason is currently undocumented in the file.

**Files:**
- Modify: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_rhs_user_form.sh`

Current contents:

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde with username resolves via getpwnam when user exists
# EXPECT_EXIT: 0
x=~root/suffix
case "$x" in
    /*/suffix) exit 0 ;;
    '~root/suffix') exit 0 ;;
    *) echo "unexpected: $x" >&2; exit 1 ;;
esac
```

- [ ] **Step 1: Insert an `EXPECT_OUTPUT omitted` comment after `EXPECT_EXIT`**

After-state:

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde with username resolves via getpwnam when user exists
# EXPECT_OUTPUT omitted: ~root resolution is platform-dependent; correctness is verified in-script via case.
# EXPECT_EXIT: 0
x=~root/suffix
case "$x" in
    /*/suffix) exit 0 ;;
    '~root/suffix') exit 0 ;;
    *) echo "unexpected: $x" >&2; exit 1 ;;
esac
```

- [ ] **Step 2: Run the test**

```bash
./e2e/run_tests.sh --filter=tilde_rhs_user_form
```

Expected: PASS (behavior unchanged).

- [ ] **Step 3: No commit (batched in Task 17)**

---

## Task 12: M5 — Add NOTE comments to 3 Rule 7/10 weak-intent tests

**Why:** Each of these 3 tests passes today but doesn't document what a regression failure would look like. Add inline `# NOTE:` comments mirroring the pattern from `rule10_reserved_quoted_not_recognized.sh` (which already documents expected vs. buggy observables).

**Files (modify all 3):**
- `e2e/posix_spec/2_10_shell_grammar/rule07_not_at_word_position.sh`
- `e2e/posix_spec/2_10_shell_grammar/rule10_reserved_after_cmd_is_arg.sh`
- `e2e/posix_spec/2_10_shell_grammar/rule10_reserved_after_pipe_in_cmdpos.sh`

- [ ] **Step 1: Edit `rule07_not_at_word_position.sh`**

Current contents:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 7 - Assignment preceding command name
# DESCRIPTION: After command name, A=1 is a literal argument, not an assignment
# EXPECT_OUTPUT: A=1
# EXPECT_EXIT: 0
echo A=1
```

After-state:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 7 - Assignment preceding command name
# DESCRIPTION: After command name, A=1 is a literal argument, not an assignment
# EXPECT_OUTPUT: A=1
# EXPECT_EXIT: 0
# NOTE: If the parser regressed and treated `A=1` as an assignment despite
# its non-leading position, the assignment would consume the token and `echo`
# would print an empty line — i.e., observed output would be empty rather
# than `A=1`.
echo A=1
```

- [ ] **Step 2: Edit `rule10_reserved_after_cmd_is_arg.sh`**

Current contents:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 10 - Keyword recognition
# DESCRIPTION: Reserved word after command name is an argument, not a keyword
# EXPECT_OUTPUT: if
# EXPECT_EXIT: 0
echo if
```

After-state:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 10 - Keyword recognition
# DESCRIPTION: Reserved word after command name is an argument, not a keyword
# EXPECT_OUTPUT: if
# EXPECT_EXIT: 0
# NOTE: If `if` were recognized as a reserved word in non-command position,
# parsing would fall into an incomplete if-statement and yield a syntax
# error (exit 2) instead of printing the literal `if`.
echo if
```

- [ ] **Step 3: Edit `rule10_reserved_after_pipe_in_cmdpos.sh`**

Current contents:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 10 - Keyword recognition
# DESCRIPTION: Reserved word is recognized in command position after a pipe
# EXPECT_OUTPUT: x
# EXPECT_EXIT: 0
echo x | if true; then cat; fi
```

After-state:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 10 - Keyword recognition
# DESCRIPTION: Reserved word is recognized in command position after a pipe
# EXPECT_OUTPUT: x
# EXPECT_EXIT: 0
# NOTE: If `if` after the pipe were NOT recognized as a reserved word,
# the parser would treat `if` as an external command name; lookup would
# fail and exit 127 (command not found) with empty output, instead of
# the if-statement running `cat` and printing `x`.
echo x | if true; then cat; fi
```

- [ ] **Step 4: Run the modified tests**

```bash
./e2e/run_tests.sh --filter=rule07
./e2e/run_tests.sh --filter=rule10
```

Expected: all PASS (behavior unchanged; only comments added).

- [ ] **Step 5: No commit (batched in Task 17)**

---

## Task 13: D1 — Add `$TEST_TMPDIR` defensive guard to existing redirection tests

**Why:** Standalone invocation of these tests (not via `run_tests.sh`) silently writes to root-relative paths if `TEST_TMPDIR` is unset. The guard converts that into a clear error.

**Files (modify each):**
- `e2e/posix_spec/2_07_redirection/dup_input_basic.sh`
- `e2e/posix_spec/2_07_redirection/dup_input_close.sh`
- `e2e/posix_spec/2_07_redirection/dup_input_param_expansion.sh`
- `e2e/posix_spec/2_07_redirection/dup_output_basic.sh`
- `e2e/posix_spec/2_07_redirection/dup_output_close.sh`
- `e2e/posix_spec/2_07_redirection/dup_output_param_expansion.sh`
- `e2e/posix_spec/2_07_redirection/readwrite_basic.sh`
- `e2e/posix_spec/2_07_redirection/readwrite_creates_file.sh`
- `e2e/posix_spec/2_07_redirection/readwrite_param_expansion.sh` (already edited in Task 10 — re-edit to add guard)
- `e2e/posix_spec/2_07_redirection/readwrite_opens_fd.sh` (renamed in Task 9)

The `e2e/posix_spec/2_14_13_times/` directory has no `$TEST_TMPDIR`-using tests today (verified by grep), so no edits there. The new N1 test in Task 2 includes the guard from inception.

The 6 newly-created files in Tasks 2–7 already include the guard.

**Guard line to add (immediately after the metadata header block, before any executable line):**

```sh
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
```

- [ ] **Step 1: For each file, insert the guard line**

For `dup_input_basic.sh` example, the file currently is:

```sh
#!/bin/sh
# POSIX_REF: 2.7.5 Duplicating an Input File Descriptor
# DESCRIPTION: <&N duplicates input fd N to fd 0 for the command
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/dup_in"
...
```

Use Edit to insert the guard after the metadata block:

```sh
#!/bin/sh
# POSIX_REF: 2.7.5 Duplicating an Input File Descriptor
# DESCRIPTION: <&N duplicates input fd N to fd 0 for the command
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
f="$TEST_TMPDIR/dup_in"
...
```

Apply to each of the 10 files. The exact metadata header lines vary per file — preserve each file's existing header verbatim and insert the guard on the line after the last `#`-prefixed metadata line.

- [ ] **Step 2: Run the affected directory's tests**

```bash
./e2e/run_tests.sh --filter=2_07_redirection
```

Expected: all PASS.

- [ ] **Step 3: Smoke-check the guard works as a guard**

```bash
unset TEST_TMPDIR
sh e2e/posix_spec/2_07_redirection/dup_input_basic.sh
echo "exit=$?"
```

Expected: non-zero exit, error message mentioning `TEST_TMPDIR not set`. (The test harness sets `TEST_TMPDIR` automatically; this is to verify the guard fires when invoked outside the harness.)

- [ ] **Step 4: No commit (batched in Task 17)**

---

## Task 14: D2 — Add POSIX_REF format contract to `e2e/README.md`

**Why:** The current `POSIX_REF` convention mixes `2.10.2 Rule N - <Name>`, `2.10.2 Rule N - <Name> (<discriminator>)`, `2.10 Shell Grammar - <Topic>`, and `2.X.Y <Section Name>` forms. A naive grep like `grep -E 'POSIX_REF: 2\.10\.2 Rule'` will miss topic-form entries. Document the accepted shapes plus the Rule 9 taxonomy disambiguation.

**Files:**
- Modify: `e2e/README.md`

- [ ] **Step 1: Read the current `e2e/README.md`**

Use Read to get the full current contents and locate the end of the existing "Writing Tests" section. The new section is appended after it.

- [ ] **Step 2: Append the new section**

Insert the following block after the existing "Writing Tests" section:

````markdown
## POSIX_REF Format Contract

Test files declare which POSIX clause they pin via the `POSIX_REF`
metadata line. The accepted shapes are:

- `2.X.Y <Section Name>` — for ordinary section references.
  Example: `POSIX_REF: 2.6.1 Tilde Expansion`
- `2.10.2 Rule N - <Name>` — for tests that pin a specific grammar rule.
  Example: `POSIX_REF: 2.10.2 Rule 1 - First Word`
- `2.10.2 Rule N - <Name> (<discriminator>)` — for multi-case rules.
  Example: `POSIX_REF: 2.10.2 Rule 10 - Keyword (after pipe)`
- `2.10 Shell Grammar - <Topic>` — for cross-rule grammar topics.
  Example: `POSIX_REF: 2.10 Shell Grammar - Terminator Equality`

A naive grep for one shape misses the others. To enumerate all
§2.10.2-related tests, use:

```sh
grep -RE 'POSIX_REF: 2\.10' e2e/posix_spec/
```

### Rule 9 Taxonomy

The label "Rule 9" in test names covers literal POSIX Rule 9 (function
body) plus its grammar-level generalizations: compound_command body and
compound_list body. Generalized cases carry a parenthetical
`<ctx>` discriminator (e.g., `Rule 9 - Body of compound_list (if-then)`)
to disambiguate them from the literal Rule 9 (`Rule 9 - Body of
function`).
````

- [ ] **Step 3: Verify the README still reads cleanly**

```bash
head -100 e2e/README.md
```

Expected: the new section appears after Writing Tests, with no markdown
formatting issues.

- [ ] **Step 4: No commit (batched in Task 17)**

---

## Task 15: F1 — Delete legacy `e2e/redirection/stderr_to_stdout.sh`

**Why:** Task 7 (N6) created the migration target `e2e/posix_spec/2_07_redirection/dup_output_stderr_to_stdout.sh` with proper `POSIX_REF` metadata. The legacy file is now redundant.

**Files:**
- Delete: `e2e/redirection/stderr_to_stdout.sh`

- [ ] **Step 1: Confirm the legacy file still exists**

```bash
ls -l e2e/redirection/stderr_to_stdout.sh
```

Expected: file exists.

- [ ] **Step 2: Confirm the migration target exists**

```bash
ls -l e2e/posix_spec/2_07_redirection/dup_output_stderr_to_stdout.sh
```

Expected: file exists (created in Task 7).

- [ ] **Step 3: Delete via `git rm`**

```bash
git rm e2e/redirection/stderr_to_stdout.sh
```

- [ ] **Step 4: Re-run the full E2E suite to confirm no breakage**

```bash
./e2e/run_tests.sh
```

Expected: all PASS, total count is `(previous count) + 6 - 1 = previous count + 5` (6 new tests added in Tasks 2–7, 1 legacy removed).

- [ ] **Step 5: No commit (batched in Task 17)**

---

## Task 16: F2 — Remove addressed lines from `TODO.md`

**Why:** Per the project convention `feedback_todo_format` (delete completed items, do not mark `[x]`). Each line is identified by a unique fingerprint string from the spec.

**Files:**
- Modify: `TODO.md`

The 15 lines to delete (each fingerprint matches exactly one line in the current `TODO.md`):

1. `\`times\` operand rejection test missing`
2. `§2.10.1 backslash-newline line-continuation test missing`
3. `dup_input_*.sh\` missing unquoted-fd variant`
4. `dup_output_*.sh\` missing unquoted-fd variant`
5. `E2E counterpart for \`x=foo:\\~/bin\` escape case missing`
6. `Double-quote escape coverage gap`
7. `tilde_rhs_user_form.sh\` documents absence of \`EXPECT_OUTPUT\``
8. `readwrite_bidirectional.sh\` description and name overstate body`
9. `readwrite_basic.sh\` and \`readwrite_param_expansion.sh\` are near-duplicates`
10. `Legacy \`e2e/redirection/stderr_to_stdout.sh\` migration`
11. `E2E test defensive \`$TEST_TMPDIR\` check`
12. `Rule 7/10 weak-intent tests need failure-signature comments`
13. `POSIX_REF format contract is undocumented`
14. `Rule 9 taxonomy needs a disambiguation note`
15. `assignment_rhs_param_then_escaped_tilde_stays_literal\` assertion is loose`

- [ ] **Step 1: For each fingerprint, locate and delete the corresponding line**

For each of the 15 fingerprints, use the Edit tool to delete that bullet line entirely (including the leading `- [ ] ` and the trailing newline). Use a sufficiently unique substring to avoid ambiguity.

Example for fingerprint #1:

Use Edit's old_string set to the full line `- [ ] \`times\` operand rejection test missing — POSIX §2.14.13 says \`times\` takes no operands. Add \`times_rejects_operand.sh\` verifying non-zero exit (and \`yosh:\` stderr prefix) for \`times foo\` (\`e2e/posix_spec/2_14_13_times/\`).` (with the actual surrounding quoting/punctuation from the file) and new_string set to empty.

If the Edit tool does not match because the line wraps differently, expand old_string to include leading/trailing newlines so the deletion is unambiguous.

- [ ] **Step 2: Verify all 15 lines are gone**

```bash
for s in \
  "operand rejection test missing" \
  "backslash-newline line-continuation test missing" \
  "missing unquoted-fd variant" \
  "escape case missing" \
  "Double-quote escape coverage gap" \
  "documents absence" \
  "overstate body" \
  "near-duplicates" \
  "stderr_to_stdout.sh\` migration" \
  "TEST_TMPDIR" \
  "failure-signature comments" \
  "POSIX_REF format contract is undocumented" \
  "Rule 9 taxonomy" \
  "assertion is loose"; do
  if grep -q "$s" TODO.md; then
    echo "STILL PRESENT: $s"
  fi
done
```

Expected: no output. (The `dup_input_*` and `dup_output_*` patterns share `missing unquoted-fd variant` so one grep covers both #3 and #4.)

- [ ] **Step 3: No commit (batched in Task 17)**

---

## Task 17: Verify, then create commit 1

**Why:** Single commit covering all E2E + docs + TODO.md changes, isolating commit 2 (parser unit-test) for clean revertability.

- [ ] **Step 1: Run the full E2E suite**

```bash
./e2e/run_tests.sh 2>&1 | tee /tmp/e2e_after.log
```

Expected: all tests pass. The total count should be `<baseline> + 5` (6 new tests added in Tasks 2–7, 1 legacy removed in Task 15).

If failures appear, debug per `superpowers:systematic-debugging` and resolve before proceeding.

- [ ] **Step 2: Run the full Rust test suite (background)**

```bash
cargo test
```

Expected: all tests pass, baseline unchanged. (Run in background per `cargo build/test timeout risk` memory; typical duration ~6–7 min.)

- [ ] **Step 3: Inspect `git status` and `git diff --stat`**

```bash
git status --short
git diff --stat
```

Expected: roughly
- 6 new files created
- 1 file renamed (M2)
- 1 file deleted (F1)
- ~15 files modified (M3, M4, M5×3, D1×10, D2, F2 contents, plus the in-place edits to N1's directory if any, and Task 8's extension)

Total `git diff --stat` line count should be modest (under ~250 line changes) since this is test-only work.

- [ ] **Step 4: Stage and commit**

```bash
git add e2e/ TODO.md
git commit -m "$(cat <<'EOF'
test(e2e): add POSIX gap tests and quality fixes

Adds 6 new POSIX-conformance E2E tests (times-rejects-operand,
line-continuation, dup-input/output unquoted-fd, tilde-mixed-backslash,
2>&1 migration), tightens 5 existing tests, extends double-quote
escape coverage, documents the POSIX_REF metadata format and Rule 9
taxonomy in e2e/README.md, deletes legacy e2e/redirection/stderr_to_stdout.sh,
and adds defensive \$TEST_TMPDIR guards across §2.7 redirection tests.

Source prompt: TODO.md の中から、POSIX 準拠に関する項目を対応してください

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 5: Verify commit landed cleanly**

```bash
git log -1 --stat
```

Expected: commit shown with the listed files; no remaining unstaged changes
in the affected directories.

---

## Task 18: M1 — Tighten `assignment_rhs_param_then_escaped_tilde_stays_literal`

**Why:** The current assertion `assert!(!parts.iter().any(|p| matches!(p, Tilde(_))))` only catches the user-visible "tilde was wrongly produced" bug. It would not detect shape regressions (e.g., `/bin` being dropped). Tighten to a full structural `assert_eq!(parts, vec![…])` mirroring the sibling `assignment_rhs_param_then_tilde_no_colon_stays_literal` test.

**Files:**
- Modify: `src/parser/simple.rs` around line 321

- [ ] **Step 1: Read the current sibling test as the style anchor**

Read lines ~285–340 of `src/parser/simple.rs` to capture:
- The full body of `assignment_rhs_param_then_tilde_no_colon_stays_literal` (line ~298), to use as the `vec!` template.
- The full body of `assignment_rhs_param_then_escaped_tilde_stays_literal` (line ~321), to identify the input string and what `parts` should contain.

Record the input literal that the loose-assertion test uses (likely something like `x=$y:\~/bin`) and derive the expected `vec!` parts from the input shape, mirroring the sibling test's pattern.

The expected `vec!` parts for input `x=$y:\~/bin` should be (using the AST module's `WordPart` variants exactly as the sibling test names them):

```rust
vec![
    Parameter(/* the param node corresponding to $y */),
    Literal(":".to_string()),
    EscapedLiteral("~".to_string()),
    Literal("/bin".to_string()),
]
```

The exact constructor names (`Parameter`, `Literal`, `EscapedLiteral`) and any `ParamExpansion`-style wrapping should be copied verbatim from the sibling. **Do not invent variant names**; if the sibling uses different ones, match them exactly.

- [ ] **Step 2: Replace the loose assertion**

Use the Edit tool. Replace the current assertion line:

```rust
assert!(!parts.iter().any(|p| matches!(p, Tilde(_))));
```

with the structural form:

```rust
assert_eq!(parts, vec![
    /* exact constructor calls mirroring sibling test */
]);
```

The replacement vector is the one assembled in Step 1.

- [ ] **Step 3: Run the targeted test**

```bash
cargo test -p yosh assignment_rhs_param_then_escaped_tilde
```

Expected: PASS. If the assertion fails, the `vec!` shape was guessed
incorrectly — adjust the constructors to match the actual `parts`
contents (printed in the assertion failure message).

- [ ] **Step 4: Run all sibling assignment-RHS tests**

```bash
cargo test -p yosh -- parser::simple::tests::assignment_rhs
```

Expected: all PASS, including the sibling tests `assignment_rhs_param_then_tilde_no_colon_stays_literal` and `assignment_rhs_backslash_tilde_after_colon_stays_literal`.

- [ ] **Step 5: Run the full Rust test suite**

```bash
cargo test
```

(Run in background per timeout memory.) Expected: full suite passes.

- [ ] **Step 6: Stage and commit**

```bash
git add src/parser/simple.rs
git commit -m "$(cat <<'EOF'
test(parser): tighten escaped-tilde-after-param assignment assertion

Replaces the loose negative-shape check with a full structural
assert_eq! mirroring assignment_rhs_param_then_tilde_no_colon_stays_literal,
so future regressions that drop or reshape the trailing /bin segment
are caught at unit-test level (previously only a tilde-presence
regression was detected).

Source prompt: TODO.md の中から、POSIX 準拠に関する項目を対応してください

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 7: Verify both commits landed**

```bash
git log -2 --stat
```

Expected: commits 1 and 2 both shown, with commit 2 touching only
`src/parser/simple.rs`.

---

## Acceptance Criteria

The implementation is complete when:

- [ ] All 6 new E2E tests pass on their own (Tasks 2–7).
- [ ] The 5 modified E2E tests still pass and embody the intended distinction (Tasks 8–12).
- [ ] All §2.7 redirection tests using `$TEST_TMPDIR` carry the guard (Task 13).
- [ ] `e2e/README.md` documents POSIX_REF format and Rule 9 taxonomy (Task 14).
- [ ] `e2e/redirection/stderr_to_stdout.sh` is gone (Task 15).
- [ ] The 15 fingerprinted lines are absent from `TODO.md` (Task 16).
- [ ] `cargo test` passes after Task 18.
- [ ] `./e2e/run_tests.sh` is clean.
- [ ] Two commits land on `main` with the source-prompt provenance line.
