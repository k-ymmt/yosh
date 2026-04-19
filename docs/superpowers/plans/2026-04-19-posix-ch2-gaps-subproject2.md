# POSIX Chapter 2 Gaps — Sub-project 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add representative E2E coverage for POSIX §2.10.2 Grammar Rules 2–10 and §2.10.1 lexical disambiguation; migrate existing §2.10 test `POSIX_REF` headers to Rule-specific values.

**Architecture:** 27 new `.sh` files (24 under `e2e/posix_spec/2_10_shell_grammar/` with `rule0N_` prefix naming, 3 under a new `e2e/posix_spec/2_10_1_lexical/`). Existing 13 files get metadata-only `POSIX_REF` updates. No `src/` changes. Uses the `EXPECT_STDERR` substring-match + `$TEST_TMPDIR` conventions established in Sub-project 1.

**Tech Stack:** POSIX `/bin/sh` test scripts, yosh debug binary (`target/debug/yosh`), existing `e2e/run_tests.sh` harness.

**Spec:** `docs/superpowers/specs/2026-04-19-posix-ch2-gaps-subproject2-design.md`

---

## Prerequisites (before Task 1)

- [ ] **Step 0.1: Build the debug binary**

```bash
cargo build
```
Expected: clean build. Harness hard-codes `./target/debug/yosh`.

- [ ] **Step 0.2: Record baseline E2E status**

```bash
./e2e/run_tests.sh 2>&1 | tail -3
```
Expected: `Total: 337  Passed: 337  Failed: 0  Timedout: 0  XFail: 0  XPass: 0`. If the baseline differs, stop and reconcile — a pre-existing failure would distort later regression checks.

- [ ] **Step 0.3: Confirm heredoc-in-test-fixture harness behavior**

Read `e2e/redirection/heredoc_basic.sh` to confirm the harness handles `.sh` files containing `cat <<DELIM` blocks cleanly. (This is the pattern Rule 3 tests rely on.)

---

## Task 1 (Commit ①): Migrate existing `POSIX_REF` headers

**Files (all in `e2e/posix_spec/2_10_shell_grammar/`):**
- `case_empty_body_is_ok.sh`
- `compound_list_newline_between_commands.sh`
- `empty_brace_group_is_error.sh`
- `empty_subshell_is_error.sh`
- `empty_compound_list_in_if_is_error.sh`
- `empty_elif_body_is_error.sh`
- `empty_else_body_is_error.sh`
- `empty_if_condition_is_error.sh`
- `empty_while_condition_is_error.sh`
- `empty_while_body_is_error.sh`
- `empty_until_condition_is_error.sh`
- `empty_for_body_is_error.sh`
- `terminator_semicolon_equals_newline.sh`

All 13 currently have `# POSIX_REF: 2.10 Shell Grammar`.

### Step 1.1: Update each `POSIX_REF` line

For each file below, replace the `# POSIX_REF: 2.10 Shell Grammar` line with the new value. Use `Edit` tool, one file at a time. Do not touch any other line.

- [ ] `case_empty_body_is_ok.sh` → `# POSIX_REF: 2.10.2 Rule 4 - Case statement termination`
- [ ] `compound_list_newline_between_commands.sh` → `# POSIX_REF: 2.10.2 Rule 9 - Body of compound_list`
- [ ] `empty_brace_group_is_error.sh` → `# POSIX_REF: 2.10.2 Rule 9 - Body of compound_command`
- [ ] `empty_subshell_is_error.sh` → `# POSIX_REF: 2.10.2 Rule 9 - Body of compound_command`
- [ ] `empty_compound_list_in_if_is_error.sh` → `# POSIX_REF: 2.10.2 Rule 9 - Body of compound_list (if)`
- [ ] `empty_elif_body_is_error.sh` → `# POSIX_REF: 2.10.2 Rule 9 - Body of compound_list (elif)`
- [ ] `empty_else_body_is_error.sh` → `# POSIX_REF: 2.10.2 Rule 9 - Body of compound_list (else)`
- [ ] `empty_if_condition_is_error.sh` → `# POSIX_REF: 2.10.2 Rule 9 - Body of compound_list (if cond)`
- [ ] `empty_while_condition_is_error.sh` → `# POSIX_REF: 2.10.2 Rule 9 - Body of compound_list (while cond)`
- [ ] `empty_while_body_is_error.sh` → `# POSIX_REF: 2.10.2 Rule 9 - Body of compound_list (while body)`
- [ ] `empty_until_condition_is_error.sh` → `# POSIX_REF: 2.10.2 Rule 9 - Body of compound_list (until cond)`
- [ ] `empty_for_body_is_error.sh` → `# POSIX_REF: 2.10.2 Rule 9 - Body of compound_list (for body)`
- [ ] `terminator_semicolon_equals_newline.sh` → `# POSIX_REF: 2.10 Shell Grammar - List terminators`

### Step 1.2: Verify no behavior change

```bash
./e2e/run_tests.sh --filter=2_10_shell_grammar 2>&1 | tail -2
```
Expected: `Total: 13  Passed: 13  Failed: 0  Timedout: 0  XFail: 0  XPass: 0`. Metadata-only change, so tests must all still pass.

### Step 1.3: Commit

```bash
git add e2e/posix_spec/2_10_shell_grammar/
git commit -m "$(cat <<'EOF'
test(2_10): migrate POSIX_REF to Rule-specific values

Upgrades the 13 existing §2.10 E2E tests from the coarse
"2.10 Shell Grammar" to Rule-specific values like
"2.10.2 Rule 9 - Body of compound_list (if)". Metadata-only;
test bodies and results unchanged. Sets up the traceability
matrix that the Rule N coverage in the next commits will
populate.

Prompt: "Future: POSIX Conformance Gaps を対応して"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2 (Commit ②): Command-position Rule tests (Rule 7, 10)

**Files to create (all mode 644, all in `e2e/posix_spec/2_10_shell_grammar/`):**
- `rule07_single_assignment.sh`
- `rule07_transient_assignment.sh`
- `rule07_not_at_word_position.sh`
- `rule10_reserved_after_cmd_is_arg.sh`
- `rule10_reserved_quoted_not_recognized.sh`
- `rule10_reserved_after_pipe_in_cmdpos.sh`

### Step 2.1: Write `rule07_single_assignment.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 7 - Assignment preceding command name
# DESCRIPTION: NAME=value at word position is a persistent assignment
# EXPECT_OUTPUT: bar
# EXPECT_EXIT: 0
FOO=bar
echo "$FOO"
```

Chmod 644:
```bash
chmod 644 e2e/posix_spec/2_10_shell_grammar/rule07_single_assignment.sh
```

### Step 2.2: Write `rule07_transient_assignment.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 7 - Assignment preceding command name
# DESCRIPTION: A=1 cmd sets A for cmd only (transient)
# EXPECT_EXIT: 0
# EXPECT_OUTPUT: 1
A=1 env | grep '^A=1' >/dev/null || { echo "transient A not in env" >&2; exit 1; }
echo 1
```

Chmod 644.

### Step 2.3: Write `rule07_not_at_word_position.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 7 - Assignment preceding command name
# DESCRIPTION: After command name, A=1 is a literal argument, not an assignment
# EXPECT_OUTPUT: A=1
# EXPECT_EXIT: 0
echo A=1
```

Chmod 644.

### Step 2.4: Write `rule10_reserved_after_cmd_is_arg.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 10 - Keyword recognition
# DESCRIPTION: Reserved word after command name is an argument, not a keyword
# EXPECT_OUTPUT: if
# EXPECT_EXIT: 0
echo if
```

Chmod 644.

### Step 2.5: Write `rule10_reserved_quoted_not_recognized.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 10 - Keyword recognition
# DESCRIPTION: A quoted reserved word in command position is looked up as a command, not recognized as a keyword
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
# If quoted 'if' were still recognized as the reserved word, `'if' true` would
# start an incomplete if-statement and yield a syntax error (exit 2). Any other
# exit code (typically 127 command-not-found, or 0 if an 'if' executable
# happens to be on PATH) means reserved-word recognition was correctly
# disabled by the quoting.
'if' true 2>/dev/null
rc=$?
if [ "$rc" -eq 2 ]; then
    echo "syntax error detected (rc=2); quoted 'if' was treated as reserved word" >&2
    exit 1
fi
echo ok
```

Chmod 644.

### Step 2.6: Write `rule10_reserved_after_pipe_in_cmdpos.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 10 - Keyword recognition
# DESCRIPTION: Reserved word is recognized in command position after a pipe
# EXPECT_OUTPUT: x
# EXPECT_EXIT: 0
echo x | if true; then cat; fi
```

Chmod 644.

### Step 2.7: Run filter

- [ ] Run:
```bash
./e2e/run_tests.sh --filter=rule07 2>&1 | tail -2
./e2e/run_tests.sh --filter=rule10 2>&1 | tail -2
```
Expected: each filter reports `Total: 3  Passed: 3  Failed: 0` (or Failed: 0 with XFail > 0 if implementation gaps surface).

For any failure:
- If it is an exit-code mismatch only, update `EXPECT_EXIT` to match yosh's actual code.
- If it is a genuine behavioral divergence, add `# XFAIL: <concise reason>` as a new line between `# DESCRIPTION:` and `# EXPECT_EXIT:`. Record the reason referencing yosh's observed behavior (e.g., "yosh emits `A=1 env` error 'A is readonly' — needs scope audit").

### Step 2.8: Commit

```bash
git add e2e/posix_spec/2_10_shell_grammar/rule07_*.sh e2e/posix_spec/2_10_shell_grammar/rule10_*.sh
git commit -m "$(cat <<'EOF'
test(2_10): add §2.10.2 Rule 7 / Rule 10 E2E coverage

Six tests covering assignment at command prefix (single, transient,
not-at-word-position) and keyword recognition boundaries (reserved
word as argument, quoted reserved word in command position, reserved
word after pipe). Rule 1 coverage folded into Rule 10 because the two
Rules share the same command-position tokenization path.

Prompt: "Future: POSIX Conformance Gaps を対応して"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3 (Commit ③): Redirection-context Rule tests (Rule 2, 3)

**Files (all mode 644, all in `e2e/posix_spec/2_10_shell_grammar/`):**
- `rule02_filename_reserved_word.sh`
- `rule02_filename_leading_dash.sh`
- `rule03_heredoc_delim_reserved_word.sh`
- `rule03_heredoc_delim_quoted_reserved_word.sh`

### Step 3.1: Write `rule02_filename_reserved_word.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 2 - Redirection filename
# DESCRIPTION: A reserved word is treated as a plain filename in a redirection target
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
echo hi > "$TEST_TMPDIR/if"
cat "$TEST_TMPDIR/if"
```

Chmod 644.

### Step 3.2: Write `rule02_filename_leading_dash.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 2 - Redirection filename
# DESCRIPTION: A redirection filename may begin with '-' (not treated as an option)
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
echo hi > "$TEST_TMPDIR/-flag"
cat -- "$TEST_TMPDIR/-flag"
```

Chmod 644.

### Step 3.3: Write `rule03_heredoc_delim_reserved_word.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 3 - Here-document delimiter
# DESCRIPTION: A reserved word may be used as an unquoted here-document delimiter
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
cat <<if
hello
if
```

Chmod 644.

### Step 3.4: Write `rule03_heredoc_delim_quoted_reserved_word.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 3 - Here-document delimiter
# DESCRIPTION: Quoted reserved-word delimiter disables body expansion and still ends at the literal delimiter
# EXPECT_OUTPUT: $X
# EXPECT_EXIT: 0
X=notexpanded
cat <<'if'
$X
if
```

Chmod 644.

### Step 3.5: Run filter

- [ ] Run:
```bash
./e2e/run_tests.sh --filter=rule02 2>&1 | tail -2
./e2e/run_tests.sh --filter=rule03 2>&1 | tail -2
```
Expected: each `Total: 2  Passed: 2  Failed: 0` (or with XFail).

XFAIL any unresolved failure using the same rules as Task 2.

### Step 3.6: Commit

```bash
git add e2e/posix_spec/2_10_shell_grammar/rule02_*.sh e2e/posix_spec/2_10_shell_grammar/rule03_*.sh
git commit -m "$(cat <<'EOF'
test(2_10): add §2.10.2 Rule 2 / Rule 3 E2E coverage

Four tests covering filename tokenization in redirection targets
(reserved word as filename, leading-dash filename) and here-document
delimiter tokenization (reserved word delim unquoted/quoted).
Verifies Rules 2 and 3 disable reserved-word recognition in these
contexts.

Prompt: "Future: POSIX Conformance Gaps を対応して"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4 (Commit ④): Loop / case / function Rule tests (Rule 4, 5, 6, 8, 9)

**Files (all mode 644, all in `e2e/posix_spec/2_10_shell_grammar/`):**

Rule 4: `rule04_case_last_item_no_dsemi.sh`, `rule04_case_empty_pattern_list_not_allowed.sh`
Rule 5: `rule05_for_valid_name.sh`, `rule05_for_invalid_name.sh`, `rule05_for_reserved_word_rejected.sh`
Rule 6: `rule06_for_in_recognized.sh`, `rule06_for_without_in_uses_positional.sh`, `rule06_case_in_recognized.sh`
Rule 8: `rule08_function_valid_name.sh`, `rule08_function_reserved_name_rejected.sh`, `rule08_function_invalid_name.sh`
Rule 9: `rule09_function_body_brace_group.sh`, `rule09_function_body_subshell.sh`, `rule09_function_body_simple_cmd_rejected.sh`

### Step 4.1: Write `rule04_case_last_item_no_dsemi.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 4 - Case statement termination
# DESCRIPTION: The last case item may omit ;; before esac
# EXPECT_OUTPUT: a
# EXPECT_EXIT: 0
case x in
    a) echo a ;;
    x) echo a
esac
```

Chmod 644.

### Step 4.2: Write `rule04_case_empty_pattern_list_not_allowed.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 4 - Case statement termination
# DESCRIPTION: A case item must begin with at least one pattern
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
case x in
    ) echo nothing ;;
esac
```

Chmod 644.

### Step 4.3: Write `rule05_for_valid_name.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 5 - NAME in for
# DESCRIPTION: A valid NAME after 'for' is accepted
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
for x in a b; do
    echo "$x"
done
```

Chmod 644.

### Step 4.4: Write `rule05_for_invalid_name.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 5 - NAME in for
# DESCRIPTION: A name cannot start with a digit
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
for 1x in a; do
    :
done
```

Chmod 644.

### Step 4.5: Write `rule05_for_reserved_word_rejected.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 5 - NAME in for
# DESCRIPTION: A reserved word is not a valid NAME
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
for if in a; do
    :
done
```

Chmod 644.

### Step 4.6: Write `rule06_for_in_recognized.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 6 - Third word of for/case recognized as 'in'
# DESCRIPTION: The keyword 'in' is recognized as the third word of for
# EXPECT_OUTPUT<<END
# 1
# 2
# END
# EXPECT_EXIT: 0
for x in 1 2; do
    echo "$x"
done
```

Chmod 644.

### Step 4.7: Write `rule06_for_without_in_uses_positional.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 6 - Third word of for/case recognized as 'in'
# DESCRIPTION: A for with no 'in' word iterates the positional parameters
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
set -- a b
for x do
    echo "$x"
done
```

Chmod 644.

### Step 4.8: Write `rule06_case_in_recognized.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 6 - Third word of for/case recognized as 'in'
# DESCRIPTION: The keyword 'in' is recognized as the third word of case
# EXPECT_OUTPUT: y
# EXPECT_EXIT: 0
case x in
    x) echo y ;;
esac
```

Chmod 644.

### Step 4.9: Write `rule08_function_valid_name.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 8 - NAME in function
# DESCRIPTION: A valid NAME is accepted as a function name
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
f() { echo ok; }
f
```

Chmod 644.

### Step 4.10: Write `rule08_function_reserved_name_rejected.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 8 - NAME in function
# DESCRIPTION: A reserved word is not a valid function name per POSIX
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
if() { :; }
```

Chmod 644.

### Step 4.11: Write `rule08_function_invalid_name.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 8 - NAME in function
# DESCRIPTION: A function name cannot start with a digit
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
1f() { :; }
```

Chmod 644.

### Step 4.12: Write `rule09_function_body_brace_group.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 9 - Body of function (compound_command)
# DESCRIPTION: A brace group is a valid function body
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
f() { echo ok; }
f
```

Chmod 644.

### Step 4.13: Write `rule09_function_body_subshell.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 9 - Body of function (compound_command)
# DESCRIPTION: A subshell is a valid function body (a compound_command)
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
f() ( echo ok )
f
```

Chmod 644.

### Step 4.14: Write `rule09_function_body_simple_cmd_rejected.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.2 Rule 9 - Body of function (compound_command)
# DESCRIPTION: The function body must be a compound_command; a simple command is not allowed
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
f() echo ok
```

Chmod 644.

### Step 4.15: Run filter

- [ ] Run:
```bash
./e2e/run_tests.sh --filter=rule04 2>&1 | tail -2
./e2e/run_tests.sh --filter=rule05 2>&1 | tail -2
./e2e/run_tests.sh --filter=rule06 2>&1 | tail -2
./e2e/run_tests.sh --filter=rule08 2>&1 | tail -2
./e2e/run_tests.sh --filter=rule09 2>&1 | tail -2
```
Expected: each filter reports `Failed: 0`. Sum of all is 14 tests.

High-risk XFAIL candidates (per spec risk table):
- `rule09_function_body_simple_cmd_rejected.sh` — yosh may accept this.
- `rule06_for_without_in_uses_positional.sh` — yosh may not implement the no-`in` form.
- `rule08_function_reserved_name_rejected.sh` — yosh may accept `if()`.

For each failure, add `# XFAIL: <specific reason naming yosh's observed behavior>` between `# DESCRIPTION:` and the first `# EXPECT_…` line. Example: `# XFAIL: yosh accepts simple-command function bodies; spec grammar requires a compound_command`.

### Step 4.16: Commit

```bash
git add e2e/posix_spec/2_10_shell_grammar/rule04_*.sh \
        e2e/posix_spec/2_10_shell_grammar/rule05_*.sh \
        e2e/posix_spec/2_10_shell_grammar/rule06_*.sh \
        e2e/posix_spec/2_10_shell_grammar/rule08_*.sh \
        e2e/posix_spec/2_10_shell_grammar/rule09_*.sh
git commit -m "$(cat <<'EOF'
test(2_10): add §2.10.2 Rule 4/5/6/8/9 E2E coverage

Fourteen tests spanning case termination, for-loop NAME validity,
'in' keyword recognition, function NAME validity, and function body
grammar (compound_command required). Any test that uncovers a yosh
implementation gap is XFAIL'd with a concrete reason and recorded
in TODO.md in the consolidation commit.

Prompt: "Future: POSIX Conformance Gaps を対応して"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5 (Commit ⑤): §2.10.1 lexical + TODO.md cleanup

**Files to create (all mode 644):**
- `e2e/posix_spec/2_10_1_lexical/operator_vs_word.sh`
- `e2e/posix_spec/2_10_1_lexical/operator_extended_max_munch.sh`
- `e2e/posix_spec/2_10_1_lexical/comment_terminates_at_newline.sh`

### Step 5.1: Write `operator_vs_word.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.1 Shell Grammar Lexical Conventions
# DESCRIPTION: The '&&' control operator is recognized without surrounding whitespace
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
echo a&&echo b
```

Chmod 644.

### Step 5.2: Write `operator_extended_max_munch.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.1 Shell Grammar Lexical Conventions
# DESCRIPTION: The longest operator token wins: '||' is a single token, not two '|'
# EXPECT_OUTPUT: a
# EXPECT_EXIT: 0
echo a||echo b
```

Chmod 644. (The `||` short-circuits because the left side succeeds; `echo b` never runs. If `||` were two `|` operators, the pipeline would still run `echo b` — so this distinguishes the two lexings.)

### Step 5.3: Write `comment_terminates_at_newline.sh`

- [ ] Create with:

```sh
#!/bin/sh
# POSIX_REF: 2.10.1 Shell Grammar Lexical Conventions
# DESCRIPTION: A '#' comment runs to end of line; the next line is an independent command
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
echo a # this comment must not consume the next line
echo b
```

Chmod 644.

### Step 5.4: Run filter

- [ ] Run:
```bash
./e2e/run_tests.sh --filter=2_10_1_lexical 2>&1 | tail -2
```
Expected: `Total: 3  Passed: 3  Failed: 0` (or with XFail).

XFAIL any failure.

### Step 5.5: Full E2E regression check

- [ ] Run:
```bash
./e2e/run_tests.sh 2>&1 | tail -5
```
Expected: `Failed: 0`. Total should be `337 + 27 = 364` (XFail count may be non-zero).

If `Failed > 0`: find the failing file (per-file output above the summary), either correct the test or XFAIL it. No `src/` changes.

### Step 5.6: Rust regression check

- [ ] Run:
```bash
cargo test --lib 2>&1 | tail -5
```
Expected: `test result: ok.` with 620+ passing (matches Sub-project 1 baseline). If any unit test newly fails, stop and investigate — this plan does not modify Rust code.

Note: `cargo test --test interactive` has a pre-existing hang in `test_classify_incomplete_if/while` tracked in TODO.md (recorded during Sub-project 1). Do NOT run the interactive integration test; use `--lib` only.

### Step 5.7: Update TODO.md — remove / rewrite the two gap items

- [ ] Open `TODO.md`. Under `## Future: POSIX Conformance Gaps (Chapter 2)`.

**If no XFAIL was added in Tasks 2–5:** delete both lines:
```
- [ ] §2.10.1 Shell Grammar Lexical Conventions — dedicated tests to be added when lexer spec-compliance is revisited
- [ ] §2.10.2 Shell Grammar Rules — dedicated grammar-rule tests to be added
```

**If XFAILs exist:** replace the corresponding line with a concrete entry naming the XFAIL'd file(s) and yosh's observed behavior. Template:
```
- [ ] §2.10.2 Rule N <Rule Name> — `e2e/posix_spec/2_10_shell_grammar/ruleNN_<name>.sh` XFAIL: <yosh's observed behavior, e.g., "yosh accepts simple-command function bodies; POSIX grammar requires a compound_command">
```

Keep the `## Future: POSIX Conformance Gaps (Chapter 2)` heading, any surviving sub-project items (§2.6.1 mixed WordPart, §2.6.1 escape, §2.11 ignored-on-entry), and surrounding blank lines intact.

### Step 5.8: Commit

```bash
git add e2e/posix_spec/2_10_1_lexical/ TODO.md
git commit -m "$(cat <<'EOF'
test(2_10_1): add §2.10.1 lexical conventions E2E coverage + close gap items

Three §2.10.1 tests (operator recognition without whitespace, max-munch
operator tokenization, comment-terminator handling). Closes the §2.10.1
and §2.10.2 TODO.md gap items; any XFAIL'd Rule N is rewritten with a
concrete technical reason naming the yosh divergence.

Prompt: "Future: POSIX Conformance Gaps を対応して"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

### Step 5.9: Final verification

- [ ] Run:
```bash
./e2e/run_tests.sh --filter=2_10_ 2>&1 | tail -2
git status
git log --oneline 87d47c3..HEAD
```
Expected:
- First: `Failed: 0` with Total = 40 (13 existing + 24 new in `2_10_shell_grammar/` + 3 in `2_10_1_lexical/` = 40).
- `git status`: `nothing to commit, working tree clean`.
- `git log`: exactly 5 commits (one per Task) + the plan and any fixup commits from Task N's filter-and-XFAIL loop.

---

## Success Criteria (restated from spec)

- `./e2e/run_tests.sh --filter=2_10_` → `Failed: 0`, Total 40 (pass + XFAIL).
- Full `./e2e/run_tests.sh` → no regressions; new Total = baseline + 27.
- `cargo test --lib` → 620+ passing (pre-existing interactive-test hang is unrelated).
- TODO.md's §2.10.1 and §2.10.2 items are removed on clean pass, or rewritten with concrete XFAIL references.
- 27 new files: mode `644`, `#!/bin/sh`, `POSIX_REF` + `DESCRIPTION` headers.
- 13 migrated files: updated `POSIX_REF` values per Task 1.
- 5 commits (one per Task) plus any fixup commits from Task N's XFAIL iterations.

## Notes for the executor

- **Do NOT modify `src/`.** If a test reveals an implementation gap, XFAIL with a specific reason; the fix belongs to a later sub-project.
- **Do NOT migrate `e2e/control_flow/`, `e2e/pipeline_and_list/`, etc.** That reorganization is a separate TODO.
- **Heredoc in test scripts** (Task 3 Step 3.3–3.4): the harness executes the test with yosh, so `cat <<if` inside the test is interpreted by yosh, not by the host harness's shell. If the test hangs or produces unexpected output, suspect `<<if` delimiter recognition rather than harness plumbing.
- **`rule10_reserved_quoted_not_recognized.sh` exit-code discrimination:** the test distinguishes by exit code alone — syntax error (exit 2) means reserved-word recognition was NOT disabled (test fails); any other exit code (typically 127 not-found, or 0 if an `if` executable exists on PATH) means quoting successfully disabled it. This is more robust than a PATH-based skip because `command -v if` matches the reserved word itself on many POSIX shells, making a `command -v` skip always fire.
- **Mode 644**: always `chmod 644` after creating; do not use `755` (project convention).
- **EXPECT_EXIT for syntax errors**: use `2` (yosh's usage/syntax code per CLAUDE.md). If yosh emits `1` for a specific case, update the test to `1` and note in the description that the actual exit is observed rather than prescribed.
- **Rule 9 brace-body test is identical to `rule08_function_valid_name.sh` in effect**: kept separate to give Rule 9 explicit traceability. This is intentional, not a bug.
