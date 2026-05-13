# E2E Test Expansion: Chapter 2 Normative-Clause Deepening — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add ~142 new POSIX E2E tests across `e2e/posix_spec/2_*/` covering XCU Chapter 2 §2.1–§2.13 at normative-clause granularity. Each `shall`/`must`/`should` clause maps to at least one dedicated test file.

**Architecture:** Four sequential phases, each split into commit-shippable sub-phases (16 sub-phase commits + 1 cleanup). No production code changes. Reuses the existing `POSIX_REF` / `XFAIL` harness in `e2e/run_tests.sh`. yosh deviations / unimplemented cases are registered as `XFAIL`; XPASS is the natural completion signal.

**Tech Stack:** POSIX sh test files (`/bin/sh`), `e2e/run_tests.sh` harness, `./target/debug/yosh` shell-under-test.

**Spec:** `docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch2-deepening-design.md`

---

## File Structure

**New directories (under `e2e/posix_spec/`):**

- `2_01_shell_introduction/` — P1.1
- `2_02_quoting/` — P1.1
- `2_05_01_positional_params/` — P1.2
- `2_05_02_special_params/` — P1.3
- `2_06_02_parameter_expansion/` — P2.2
- `2_06_03_command_substitution/` — P2.3
- `2_06_04_arithmetic_expansion/` — P2.4
- `2_06_05_field_splitting/` — P2.5
- `2_06_06_pathname_expansion/` — P2.6
- `2_06_07_quote_removal/` — P2.7
- `2_07_04_heredoc/` — P3.1
- `2_09_01_simple_commands/` — P3.3
- `2_09_02_pipelines/` — P3.3
- `2_09_03_lists/` — P3.3
- `2_09_04_compound_commands/` — P3.3
- `2_09_05_function_definition/` — P3.3
- `2_12_shell_exec_env/` — P3.5

**Existing directories supplemented in-place:**

- `2_03_token_recognition/` (P4.1)
- `2_04_reserved_words/` (P4.1)
- `2_05_03_shell_variables/` (P1.3 supplement)
- `2_06_01_tilde_expansion/` (P2.1)
- `2_07_redirection/` (P3.1)
- `2_08_01_consequences_of_shell_errors/` (P3.2)
- `2_10_shell_grammar/` (P4.1)
- `2_11_signals_and_error_handling/` (P3.4)
- `2_13_pattern_matching/` (P4.1)

**Modified files:**

- `TODO.md` — delete the `Future: E2E Test Expansion` section after Task 17. Add bullets to `Future: POSIX Conformance Bugs` for newly-discovered XFAIL surface (handled inline per task).

**Not modified:**

- `e2e/run_tests.sh` — accepts arbitrary `POSIX_REF` labels as free text; no code change needed.
- `e2e/README.md` — POSIX_REF Format Contract already lists the shapes used here (`2.X.Y <Subsection Name>`, `2.10.2 Rule N - <Name>`, `2.10 Shell Grammar - <Topic>`).
- yosh production source — XFAILs document expected behavior of unimplemented features but do not implement them.

---

## Canonical Test File Template

Every test file in this plan follows the harness contract documented in `e2e/README.md`:

```sh
#!/bin/sh
# POSIX_REF: <section reference>
# DESCRIPTION: <one-line behavior summary>
# EXPECT_OUTPUT: <expected stdout, exact match>
# EXPECT_EXIT: <expected exit code, integer>
<shell body>
```

Optional metadata:

- `# EXPECT_STDERR: <substring>` — substring match on stderr.
- `# EXPECT_OUTPUT<<END` / `# END` — multiline expected output block. Every interior line is prefixed with `# `.
- `# XFAIL: <reason>` — marks the test as expected-fail. Categories:
  - `not yet implemented (TODO: implement <X>)` — missing builtin / option / feature.
  - `non-POSIX deviation (<description>)` — intentional or known yosh divergence.
  - `harness limitation (<description>)` — PTY-only, locale, signal-timing, etc.

All file paths are relative to the repo root. All test files have mode `644` (per CLAUDE.md).

---

## Pre-flight

Before Task 1, confirm the baseline.

- [ ] **Step 0.1: Confirm git tree is clean**

Run: `git status`
Expected: `nothing to commit, working tree clean`. The spec commit (`25ab569`) is already in main.

- [ ] **Step 0.2: Build debug yosh**

Run: `cargo build`
Expected: build succeeds (1–3 min cold).

- [ ] **Step 0.3: Capture baseline E2E summary**

Run: `./e2e/run_tests.sh 2>&1 | tail -3 | tee /tmp/e2e-ch2-baseline.txt`
Expected: a summary line `Total: N  Passed: P  Failed: F  Timedout: 0  XFail: X  XPass: 0` and exit code 0. Record `N`, `P`, `F`, `X`. Task 17 diffs against these numbers.

---

## Task 1 (P1.1): 2.1 Shell Introduction + 2.2 Quoting

**Files:**
- Create dir: `e2e/posix_spec/2_01_shell_introduction/`
- Create dir: `e2e/posix_spec/2_02_quoting/`
- Create 14 test files (2 in `2_01_*/`, 12 in `2_02_*/`)

- [ ] **Step 1.1: Create directories**

```sh
mkdir -p e2e/posix_spec/2_01_shell_introduction e2e/posix_spec/2_02_quoting
```

- [ ] **Step 1.2: Write 2.1 Shell Introduction tests (2 files)**

Create `e2e/posix_spec/2_01_shell_introduction/c_option_invocation.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.1 Shell Introduction
# DESCRIPTION: -c option executes the argument string and exits
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
./target/debug/yosh -c 'echo hello'
```

Create `e2e/posix_spec/2_01_shell_introduction/stdin_invocation.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.1 Shell Introduction
# DESCRIPTION: shell reads commands from stdin when no script operand is given
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
printf 'echo hi\n' | ./target/debug/yosh
```

- [ ] **Step 1.3: Write 2.2.1 Escape tests (3 files)**

Create `e2e/posix_spec/2_02_quoting/escape_dollar.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.2.1 Escape Character (Backslash)
# DESCRIPTION: Backslash preserves literal value of dollar sign
# EXPECT_OUTPUT: $HOME
# EXPECT_EXIT: 0
echo \$HOME
```

Create `e2e/posix_spec/2_02_quoting/escape_newline_continuation.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.2.1 Escape Character (Backslash)
# DESCRIPTION: Backslash-newline is line continuation, removed from input
# EXPECT_OUTPUT: ab
# EXPECT_EXIT: 0
echo a\
b
```

Create `e2e/posix_spec/2_02_quoting/escape_preserves_metachar.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.2.1 Escape Character (Backslash)
# DESCRIPTION: Backslash preserves literal value of glob metacharacter
# EXPECT_OUTPUT: *
# EXPECT_EXIT: 0
echo \*
```

- [ ] **Step 1.4: Write 2.2.2 Single-Quote tests (3 files)**

Create `e2e/posix_spec/2_02_quoting/single_literal_dollar.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.2.2 Single-Quotes
# DESCRIPTION: Single-quotes preserve literal $HOME
# EXPECT_OUTPUT: $HOME
# EXPECT_EXIT: 0
echo '$HOME'
```

Create `e2e/posix_spec/2_02_quoting/single_no_variable_expansion.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.2.2 Single-Quotes
# DESCRIPTION: Single-quotes suppress variable expansion
# EXPECT_OUTPUT: $x*
# EXPECT_EXIT: 0
x=v
echo '$x*'
```

Create `e2e/posix_spec/2_02_quoting/single_preserves_backslash.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.2.2 Single-Quotes
# DESCRIPTION: Backslash is literal inside single-quotes (no escape interpretation)
# EXPECT_OUTPUT: \\
# EXPECT_EXIT: 0
echo '\\'
```

- [ ] **Step 1.5: Write 2.2.3 Double-Quote tests (6 files)**

Create `e2e/posix_spec/2_02_quoting/double_param_expansion.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Parameter expansion occurs inside double-quotes
# EXPECT_OUTPUT: value
# EXPECT_EXIT: 0
x=value
echo "$x"
```

Create `e2e/posix_spec/2_02_quoting/double_preserves_singlequote.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Single quote is literal inside double quotes
# EXPECT_OUTPUT: '
# EXPECT_EXIT: 0
echo "'"
```

Create `e2e/posix_spec/2_02_quoting/double_escape_dollar.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Backslash escapes $ inside double-quotes
# EXPECT_OUTPUT: $x
# EXPECT_EXIT: 0
x=value
echo "\$x"
```

Create `e2e/posix_spec/2_02_quoting/double_escape_dquote.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Backslash escapes embedded double-quote
# EXPECT_OUTPUT: "
# EXPECT_EXIT: 0
echo "\""
```

Create `e2e/posix_spec/2_02_quoting/double_escape_backslash.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Backslash escapes itself inside double-quotes
# EXPECT_OUTPUT: \
# EXPECT_EXIT: 0
echo "\\"
```

Create `e2e/posix_spec/2_02_quoting/double_other_backslash_literal.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Backslash is literal when followed by non-special char inside double-quotes
# EXPECT_OUTPUT: \a
# EXPECT_EXIT: 0
echo "\a"
```

- [ ] **Step 1.6: Set permissions on new test files**

```sh
chmod 644 e2e/posix_spec/2_01_shell_introduction/*.sh e2e/posix_spec/2_02_quoting/*.sh
```

- [ ] **Step 1.7: Run the new tests**

```sh
./e2e/run_tests.sh --filter=2_01_shell_introduction
./e2e/run_tests.sh --filter=2_02_quoting
```

Expected: All PASS. If any FAIL, capture the diff with `--verbose` and decide between (a) fixing the test body if the assertion was wrong, or (b) adding `# XFAIL: <reason>` if yosh diverges from POSIX.

- [ ] **Step 1.8: Commit**

```sh
git add e2e/posix_spec/2_01_shell_introduction e2e/posix_spec/2_02_quoting
git commit -m "$(cat <<'EOF'
test(e2e): add §2.1 shell intro + §2.2 quoting tests (P1.1)

Adds 14 normative-clause tests for shell startup contract and
quoting semantics (escape, single-quote, double-quote). First
sub-phase of the Ch2 deepening rolling plan.

Spec: docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch2-deepening-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2 (P1.2): 2.5.1 Positional Parameters

**Files:**
- Create dir: `e2e/posix_spec/2_05_01_positional_params/`
- Create 6 test files

- [ ] **Step 2.1: Create directory**

```sh
mkdir -p e2e/posix_spec/2_05_01_positional_params
```

- [ ] **Step 2.2: Write the 6 tests**

Create `e2e/posix_spec/2_05_01_positional_params/positional_basic.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: $1 $2 $3 access first three positional parameters
# EXPECT_OUTPUT: a b c
# EXPECT_EXIT: 0
set -- a b c
echo "$1 $2 $3"
```

Create `e2e/posix_spec/2_05_01_positional_params/positional_set_resets.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: set -- replaces (not appends) the positional parameters
# EXPECT_OUTPUT: 1:z
# EXPECT_EXIT: 0
set -- x y w
set -- z
echo "$#:$1"
```

Create `e2e/posix_spec/2_05_01_positional_params/positional_at_quoted.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: "$@" expands to separate words preserving whitespace
# EXPECT_OUTPUT<<END
# a b
# c
# END
# EXPECT_EXIT: 0
set -- "a b" c
for w in "$@"; do
    echo "$w"
done
```

Create `e2e/posix_spec/2_05_01_positional_params/positional_star_quoted_joins.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: "$*" joins positional parameters with first IFS character
# EXPECT_OUTPUT: a,b,c
# EXPECT_EXIT: 0
set -- a b c
IFS=,
echo "$*"
```

Create `e2e/posix_spec/2_05_01_positional_params/positional_brace_double_digit.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: ${10} accesses tenth positional; $10 is $1 followed by literal 0
# EXPECT_OUTPUT<<END
# ten
# one0
# END
# EXPECT_EXIT: 0
set -- one two three four five six seven eight nine ten
echo "${10}"
echo "$10"
```

Create `e2e/posix_spec/2_05_01_positional_params/positional_count.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: $# expands to the number of positional parameters
# EXPECT_OUTPUT: 3
# EXPECT_EXIT: 0
set -- a b c
echo $#
```

- [ ] **Step 2.3: Set permissions and run**

```sh
chmod 644 e2e/posix_spec/2_05_01_positional_params/*.sh
./e2e/run_tests.sh --filter=2_05_01_positional_params
```

Expected: All PASS. If `positional_brace_double_digit.sh` fails because yosh doesn't accept `${10}`, add `# XFAIL: not yet implemented (TODO: implement multi-digit positional brace expansion)` before the script body.

- [ ] **Step 2.4: Commit**

```sh
git add e2e/posix_spec/2_05_01_positional_params
git commit -m "$(cat <<'EOF'
test(e2e): add §2.5.1 positional parameter tests (P1.2)

Six normative-clause tests covering basic access, set --
replacement, "$@" / "$*" quoting semantics, ${NN} double-digit
form, and $# count.

Spec: docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch2-deepening-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3 (P1.3): 2.5.2 Special Parameters + 2.5.3 Shell Variables supplement

**Files:**
- Create dir: `e2e/posix_spec/2_05_02_special_params/`
- Create 7 test files in `2_05_02_*/`
- Create 1 test file in existing `2_05_03_shell_variables/` (supplement)

- [ ] **Step 3.1: Create directory**

```sh
mkdir -p e2e/posix_spec/2_05_02_special_params
```

- [ ] **Step 3.2: Write 2.5.2 Special Parameter tests (7 files)**

Create `e2e/posix_spec/2_05_02_special_params/question_after_success.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $? is 0 after a successful command
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
true
echo $?
```

Create `e2e/posix_spec/2_05_02_special_params/question_after_failure.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $? is 1 after a failed command
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
false
echo $?
```

Create `e2e/posix_spec/2_05_02_special_params/hash_no_args.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $# is 0 when no positional parameters are set
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
set --
echo $#
```

Create `e2e/posix_spec/2_05_02_special_params/dollar_pid.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $$ is set to a non-empty integer (shell process ID)
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
case "$$" in
    ''|*[!0-9]*) echo "bad pid: $$" ;;
    *) echo ok ;;
esac
```

Create `e2e/posix_spec/2_05_02_special_params/dollar_zero.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $0 expands to the shell or script name (non-empty)
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
case "$0" in
    '') echo "empty \$0" ;;
    *) echo ok ;;
esac
```

Create `e2e/posix_spec/2_05_02_special_params/dash_contains_options.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $- contains currently-set option letters
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
set -e
case "$-" in
    *e*) echo ok ;;
    *) echo "missing e in: $-" ;;
esac
```

Create `e2e/posix_spec/2_05_02_special_params/bang_background_pid.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $! is the PID of the most recent background command
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
sleep 0 &
case "$!" in
    ''|*[!0-9]*) echo "bad bang: $!" ;;
    *) echo ok ;;
esac
wait
```

- [ ] **Step 3.3: Write 2.5.3 supplement (1 file)**

Create `e2e/posix_spec/2_05_03_shell_variables/path_default_is_set.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: PATH is non-empty at shell startup
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
case "$PATH" in
    '') echo empty ;;
    *) echo ok ;;
esac
```

- [ ] **Step 3.4: Set permissions and run**

```sh
chmod 644 e2e/posix_spec/2_05_02_special_params/*.sh e2e/posix_spec/2_05_03_shell_variables/path_default_is_set.sh
./e2e/run_tests.sh --filter=2_05_02_special_params
./e2e/run_tests.sh --filter=2_05_03_shell_variables
```

Expected: All PASS. (`$PPID` is currently empty in yosh per TODO.md "POSIX Conformance Bugs" — but we are not adding a `$PPID` test here; the existing `8_env_vars/PPID_is_set.sh` already covers it as XFAIL.)

- [ ] **Step 3.5: Commit**

```sh
git add e2e/posix_spec/2_05_02_special_params e2e/posix_spec/2_05_03_shell_variables/path_default_is_set.sh
git commit -m "$(cat <<'EOF'
test(e2e): add §2.5.2 special params + §2.5.3 supplement (P1.3)

Seven §2.5.2 tests covering $?, $#, $$, $0, $-, $!, plus a
§2.5.3 supplement asserting PATH is set at startup.

Spec: docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch2-deepening-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4 (P2.1): 2.6.1 Tilde Expansion supplement

**Files:**
- Create 3 files in existing `e2e/posix_spec/2_06_01_tilde_expansion/`

- [ ] **Step 4.1: Write supplement tests**

Create `e2e/posix_spec/2_06_01_tilde_expansion/tilde_pathname_search.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde in PATH is expanded once at assignment
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
HOME=/tmp
PATH=~/bin:$PATH
case "$PATH" in
    /tmp/bin:*) echo ok ;;
    *) echo "bad PATH: $PATH" ;;
esac
```

Create `e2e/posix_spec/2_06_01_tilde_expansion/tilde_unset_home_is_implementation_defined.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Behavior of tilde with unset HOME is implementation-defined; yosh preserves literal ~
# EXPECT_OUTPUT: ~
# EXPECT_EXIT: 0
unset HOME
echo ~
```

Create `e2e/posix_spec/2_06_01_tilde_expansion/tilde_assignment_value.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde at start of assignment value expands to HOME
# EXPECT_OUTPUT: /tmp/foo
# EXPECT_EXIT: 0
HOME=/tmp
x=~/foo
echo "$x"
```

- [ ] **Step 4.2: Set permissions and run**

```sh
chmod 644 e2e/posix_spec/2_06_01_tilde_expansion/tilde_pathname_search.sh e2e/posix_spec/2_06_01_tilde_expansion/tilde_unset_home_is_implementation_defined.sh e2e/posix_spec/2_06_01_tilde_expansion/tilde_assignment_value.sh
./e2e/run_tests.sh --filter=2_06_01_tilde_expansion
```

Expected: All PASS (subject to verifying `tilde_unset_home_*` actually matches yosh behavior; if yosh expands to empty string instead, change `EXPECT_OUTPUT:` to empty or add `XFAIL: non-POSIX deviation (...)`).

- [ ] **Step 4.3: Commit**

```sh
git add e2e/posix_spec/2_06_01_tilde_expansion/tilde_pathname_search.sh e2e/posix_spec/2_06_01_tilde_expansion/tilde_unset_home_is_implementation_defined.sh e2e/posix_spec/2_06_01_tilde_expansion/tilde_assignment_value.sh
git commit -m "$(cat <<'EOF'
test(e2e): add §2.6.1 tilde expansion supplement (P2.1)

Three edge cases not yet covered by the existing 23 tilde tests:
PATH assignment expansion, unset-HOME behavior, RHS assignment
literal.

Spec: docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch2-deepening-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5 (P2.2): 2.6.2 Parameter Expansion

**Files:**
- Create dir: `e2e/posix_spec/2_06_02_parameter_expansion/`
- Create 16 test files

- [ ] **Step 5.1: Create directory**

```sh
mkdir -p e2e/posix_spec/2_06_02_parameter_expansion
```

- [ ] **Step 5.2: Write basic brace + default forms (5 files)**

Create `e2e/posix_spec/2_06_02_parameter_expansion/braces_concat.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${name} delimits the parameter from following text
# EXPECT_OUTPUT: abcd
# EXPECT_EXIT: 0
x=ab
echo "${x}cd"
```

Create `e2e/posix_spec/2_06_02_parameter_expansion/default_unset.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var:-word} substitutes word when var is unset
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
unset x
echo "${x:-hello}"
```

Create `e2e/posix_spec/2_06_02_parameter_expansion/default_empty.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var:-word} substitutes word when var is empty
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
x=
echo "${x:-hello}"
```

Create `e2e/posix_spec/2_06_02_parameter_expansion/default_set.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var:-word} substitutes var when var is set and non-empty
# EXPECT_OUTPUT: value
# EXPECT_EXIT: 0
x=value
echo "${x:-hello}"
```

Create `e2e/posix_spec/2_06_02_parameter_expansion/default_no_colon_empty_keeps_empty.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var-word} keeps empty value (no colon = unset only)
# EXPECT_OUTPUT: [empty]
# EXPECT_EXIT: 0
x=
echo "[${x-hello}empty]"
```

- [ ] **Step 5.3: Write assign / alternate / error forms (4 files)**

Create `e2e/posix_spec/2_06_02_parameter_expansion/assign_unset.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var:=word} assigns word to var when var is unset
# EXPECT_OUTPUT<<END
# hello
# hello
# END
# EXPECT_EXIT: 0
unset x
echo "${x:=hello}"
echo "$x"
```

Create `e2e/posix_spec/2_06_02_parameter_expansion/assign_no_colon_set_no_assign.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var=word} does not assign when var is set (even if empty)
# EXPECT_OUTPUT<<END
# 
# 
# END
# EXPECT_EXIT: 0
x=
echo "${x=hello}"
echo "$x"
```

Create `e2e/posix_spec/2_06_02_parameter_expansion/error_unset.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var:?msg} causes shell error when var is unset (non-interactive: exit)
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
# EXPECT_STDERR: missing
(unset x; echo "${x:?missing}")
```

Create `e2e/posix_spec/2_06_02_parameter_expansion/alternate_set.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var:+word} substitutes word when var is set and non-empty
# EXPECT_OUTPUT: alt
# EXPECT_EXIT: 0
x=value
echo "${x:+alt}"
```

- [ ] **Step 5.4: Write length and prefix/suffix-strip forms (6 files)**

Create `e2e/posix_spec/2_06_02_parameter_expansion/length.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${#var} expands to string length of var
# EXPECT_OUTPUT: 5
# EXPECT_EXIT: 0
x=hello
echo "${#x}"
```

Create `e2e/posix_spec/2_06_02_parameter_expansion/remove_suffix_shortest.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var%pattern} removes shortest matching suffix
# EXPECT_OUTPUT: foo
# EXPECT_EXIT: 0
x=foo.txt
echo "${x%.txt}"
```

Create `e2e/posix_spec/2_06_02_parameter_expansion/remove_suffix_longest.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var%%pattern} removes longest matching suffix
# EXPECT_OUTPUT: a
# EXPECT_EXIT: 0
x=a.b.c
echo "${x%%.*}"
```

Create `e2e/posix_spec/2_06_02_parameter_expansion/remove_prefix_shortest.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var#pattern} removes shortest matching prefix
# EXPECT_OUTPUT: to/file
# EXPECT_EXIT: 0
x=path/to/file
echo "${x#path/}"
```

Create `e2e/posix_spec/2_06_02_parameter_expansion/remove_prefix_longest.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var##pattern} removes longest matching prefix
# EXPECT_OUTPUT: c
# EXPECT_EXIT: 0
x=a.b.c
echo "${x##*.}"
```

Create `e2e/posix_spec/2_06_02_parameter_expansion/nested_default.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Nested default expansion uses inner fallback when both outer and inner are unset
# EXPECT_OUTPUT: fallback
# EXPECT_EXIT: 0
unset x
y=fallback
echo "${x:-${y}}"
```

- [ ] **Step 5.5: Set permissions and run**

```sh
chmod 644 e2e/posix_spec/2_06_02_parameter_expansion/*.sh
./e2e/run_tests.sh --filter=2_06_02_parameter_expansion
```

Expected: All PASS. If any fail because yosh doesn't implement a specific form, add `# XFAIL: not yet implemented (TODO: implement ${form} parameter expansion)` and re-run.

- [ ] **Step 5.6: Commit**

```sh
git add e2e/posix_spec/2_06_02_parameter_expansion
git commit -m "$(cat <<'EOF'
test(e2e): add §2.6.2 parameter expansion tests (P2.2)

Sixteen normative-clause tests covering braces, default (:-, -),
assign (:=, =), error (:?), alternate (:+), length (${#var}),
suffix/prefix removal (%, %%, #, ##), and nested expansion.

Spec: docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch2-deepening-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6 (P2.3): 2.6.3 Command Substitution

**Files:**
- Create dir: `e2e/posix_spec/2_06_03_command_substitution/`
- Create 6 test files

- [ ] **Step 6.1: Create directory**

```sh
mkdir -p e2e/posix_spec/2_06_03_command_substitution
```

- [ ] **Step 6.2: Write 6 tests**

Create `e2e/posix_spec/2_06_03_command_substitution/dollar_paren_basic.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: $(...) substitutes the standard output of the enclosed command
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
echo $(echo hi)
```

Create `e2e/posix_spec/2_06_03_command_substitution/backtick_basic.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Backtick form is equivalent to $(...) for simple cases
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
echo `echo hi`
```

Create `e2e/posix_spec/2_06_03_command_substitution/nested_dollar_paren.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: $(...) supports nesting
# EXPECT_OUTPUT: inner
# EXPECT_EXIT: 0
echo $(echo $(echo inner))
```

Create `e2e/posix_spec/2_06_03_command_substitution/inside_dquote_preserves_spaces.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Command substitution inside double-quotes does not field-split
# EXPECT_OUTPUT: a b c
# EXPECT_EXIT: 0
echo "$(echo a b c)"
```

Create `e2e/posix_spec/2_06_03_command_substitution/trailing_newlines_stripped.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Trailing newlines are removed from command substitution output
# EXPECT_OUTPUT: [foo]
# EXPECT_EXIT: 0
x=$(printf 'foo\n\n\n')
echo "[$x]"
```

Create `e2e/posix_spec/2_06_03_command_substitution/exit_status_propagates_to_parent.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: $? after standalone $(...) reflects the substituted command's status
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
$(false)
echo $?
```

- [ ] **Step 6.3: Set permissions, run, commit**

```sh
chmod 644 e2e/posix_spec/2_06_03_command_substitution/*.sh
./e2e/run_tests.sh --filter=2_06_03_command_substitution
git add e2e/posix_spec/2_06_03_command_substitution
git commit -m "$(cat <<'EOF'
test(e2e): add §2.6.3 command substitution tests (P2.3)

Six normative-clause tests: $(...) form, backtick form, nesting,
double-quote context, trailing newline stripping, and exit
status propagation.

Spec: docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch2-deepening-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7 (P2.4): 2.6.4 Arithmetic Expansion

**Files:**
- Create dir: `e2e/posix_spec/2_06_04_arithmetic_expansion/`
- Create 6 test files

- [ ] **Step 7.1: Create directory and write tests**

```sh
mkdir -p e2e/posix_spec/2_06_04_arithmetic_expansion
```

Create `e2e/posix_spec/2_06_04_arithmetic_expansion/basic_addition.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: $((expr)) evaluates arithmetic with addition
# EXPECT_OUTPUT: 2
# EXPECT_EXIT: 0
echo $((1+1))
```

Create `e2e/posix_spec/2_06_04_arithmetic_expansion/parentheses_precedence.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Parentheses override operator precedence
# EXPECT_OUTPUT: 14
# EXPECT_EXIT: 0
echo $((2*(3+4)))
```

Create `e2e/posix_spec/2_06_04_arithmetic_expansion/variable_reference.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Bare variable name inside $(()) is evaluated as its numeric value
# EXPECT_OUTPUT: 6
# EXPECT_EXIT: 0
x=5
echo $((x+1))
```

Create `e2e/posix_spec/2_06_04_arithmetic_expansion/unset_variable_is_zero.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Unset variable is treated as zero in arithmetic context
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
unset x
echo $((x+1))
```

Create `e2e/posix_spec/2_06_04_arithmetic_expansion/integer_division_truncates.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Integer division truncates toward zero
# EXPECT_OUTPUT: 3
# EXPECT_EXIT: 0
echo $((10/3))
```

Create `e2e/posix_spec/2_06_04_arithmetic_expansion/modulo.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Modulo operator returns remainder
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
echo $((10%3))
```

- [ ] **Step 7.2: Permissions, run, commit**

```sh
chmod 644 e2e/posix_spec/2_06_04_arithmetic_expansion/*.sh
./e2e/run_tests.sh --filter=2_06_04_arithmetic_expansion
git add e2e/posix_spec/2_06_04_arithmetic_expansion
git commit -m "$(cat <<'EOF'
test(e2e): add §2.6.4 arithmetic expansion tests (P2.4)

Six normative-clause tests: addition, parentheses precedence,
variable reference, unset-as-zero, integer division truncation,
and modulo.

Spec: docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch2-deepening-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8 (P2.5): 2.6.5 Field Splitting

**Files:**
- Create dir: `e2e/posix_spec/2_06_05_field_splitting/`
- Create 8 test files

- [ ] **Step 8.1: Create directory and write tests**

```sh
mkdir -p e2e/posix_spec/2_06_05_field_splitting
```

Create `e2e/posix_spec/2_06_05_field_splitting/default_ifs_splits.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Unquoted expansion is split on default IFS (space, tab, newline)
# EXPECT_OUTPUT: 3
# EXPECT_EXIT: 0
x="a b c"
set -- $x
echo $#
```

Create `e2e/posix_spec/2_06_05_field_splitting/dquote_inhibits_split.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Double-quoted expansion is not split
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
x="a b c"
set -- "$x"
echo $#
```

Create `e2e/posix_spec/2_06_05_field_splitting/unset_ifs_uses_default.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Unset IFS behaves as if IFS=<space><tab><newline>
# EXPECT_OUTPUT: 3
# EXPECT_EXIT: 0
unset IFS
x="a b c"
set -- $x
echo $#
```

Create `e2e/posix_spec/2_06_05_field_splitting/empty_ifs_inhibits_split.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Empty IFS inhibits all field splitting
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
IFS=
set -- $(echo "a b c")
echo $#
```

Create `e2e/posix_spec/2_06_05_field_splitting/custom_separator.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Custom IFS character splits fields
# EXPECT_OUTPUT: 3
# EXPECT_EXIT: 0
IFS=:
x=a:b:c
set -- $x
echo $#
```

Create `e2e/posix_spec/2_06_05_field_splitting/whitespace_ifs_collapses.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Adjacent whitespace IFS characters collapse to one delimiter
# EXPECT_OUTPUT: 2
# EXPECT_EXIT: 0
x="a    b"
set -- $x
echo $#
```

Create `e2e/posix_spec/2_06_05_field_splitting/nonwhitespace_ifs_makes_empty_field.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Adjacent non-whitespace IFS characters yield empty fields
# EXPECT_OUTPUT: 3
# EXPECT_EXIT: 0
IFS=:
x=a::b
set -- $x
echo $#
```

Create `e2e/posix_spec/2_06_05_field_splitting/at_in_dquote_splits_per_positional.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: "$@" expands to separate fields per positional parameter
# EXPECT_OUTPUT<<END
# [a b]
# [c]
# END
# EXPECT_EXIT: 0
set -- "a b" c
for w in "$@"; do
    echo "[$w]"
done
```

- [ ] **Step 8.2: Permissions, run, commit**

```sh
chmod 644 e2e/posix_spec/2_06_05_field_splitting/*.sh
./e2e/run_tests.sh --filter=2_06_05_field_splitting
git add e2e/posix_spec/2_06_05_field_splitting
git commit -m "$(cat <<'EOF'
test(e2e): add §2.6.5 field splitting tests (P2.5)

Eight normative-clause tests: default-IFS split, dquote inhibits,
unset-IFS default, empty-IFS no-split, custom separator,
whitespace collapse, non-whitespace empty field, "$@" per-arg
splitting.

Spec: docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch2-deepening-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9 (P2.6): 2.6.6 Pathname Expansion

**Files:**
- Create dir: `e2e/posix_spec/2_06_06_pathname_expansion/`
- Create 6 test files (all use `$TEST_TMPDIR` for isolation)

- [ ] **Step 9.1: Create directory and write tests**

```sh
mkdir -p e2e/posix_spec/2_06_06_pathname_expansion
```

Create `e2e/posix_spec/2_06_06_pathname_expansion/star_matches_files.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: * matches any string in filenames (excluding leading .)
# EXPECT_OUTPUT: a.txt b.txt
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
: > a.txt
: > b.txt
echo *.txt
```

Create `e2e/posix_spec/2_06_06_pathname_expansion/question_matches_single_char.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: ? matches exactly one character in filenames
# EXPECT_OUTPUT: a b
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
: > a
: > b
: > ab
echo ?
```

Create `e2e/posix_spec/2_06_06_pathname_expansion/bracket_class.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: [abc] matches any one of the listed characters
# EXPECT_OUTPUT: a b
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
: > a
: > b
: > c
echo [ab]
```

Create `e2e/posix_spec/2_06_06_pathname_expansion/no_match_is_literal.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: Pattern with no match expands to the literal pattern
# EXPECT_OUTPUT: *.nomatch
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
echo *.nomatch
```

Create `e2e/posix_spec/2_06_06_pathname_expansion/quoted_pattern_no_glob.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: Quoted glob metacharacters are not expanded
# EXPECT_OUTPUT: *.txt
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
: > a.txt
echo "*.txt"
```

Create `e2e/posix_spec/2_06_06_pathname_expansion/leading_dot_not_matched_by_default.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: Files starting with . are not matched by unquoted * by default
# EXPECT_OUTPUT: visible
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
: > .hidden
: > visible
echo *
```

- [ ] **Step 9.2: Permissions, run, commit**

```sh
chmod 644 e2e/posix_spec/2_06_06_pathname_expansion/*.sh
./e2e/run_tests.sh --filter=2_06_06_pathname_expansion
git add e2e/posix_spec/2_06_06_pathname_expansion
git commit -m "$(cat <<'EOF'
test(e2e): add §2.6.6 pathname expansion tests (P2.6)

Six normative-clause tests: *, ?, [], no-match literal, quoted
suppression, leading-dot exclusion. All use \$TEST_TMPDIR for
isolation.

Spec: docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch2-deepening-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10 (P2.7): 2.6.7 Quote Removal

**Files:**
- Create dir: `e2e/posix_spec/2_06_07_quote_removal/`
- Create 3 test files

- [ ] **Step 10.1: Create directory and write tests**

```sh
mkdir -p e2e/posix_spec/2_06_07_quote_removal
```

Create `e2e/posix_spec/2_06_07_quote_removal/single_quote_removed.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.7 Quote Removal
# DESCRIPTION: Single-quote characters are removed from final word after expansion
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
echo 'hi'
```

Create `e2e/posix_spec/2_06_07_quote_removal/double_quote_removed.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.7 Quote Removal
# DESCRIPTION: Double-quote characters are removed from final word after expansion
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
echo "hi"
```

Create `e2e/posix_spec/2_06_07_quote_removal/backslash_removed.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.7 Quote Removal
# DESCRIPTION: Backslash escape character is removed from final word
# EXPECT_OUTPUT: $
# EXPECT_EXIT: 0
echo \$
```

- [ ] **Step 10.2: Permissions, run, commit**

```sh
chmod 644 e2e/posix_spec/2_06_07_quote_removal/*.sh
./e2e/run_tests.sh --filter=2_06_07_quote_removal
git add e2e/posix_spec/2_06_07_quote_removal
git commit -m "$(cat <<'EOF'
test(e2e): add §2.6.7 quote removal tests (P2.7)

Three normative-clause tests asserting unquoted output after
single-quote, double-quote, and backslash removal.

Spec: docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch2-deepening-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11 (P3.1): 2.7 Redirection rest + here-document

**Files:**
- Create dir: `e2e/posix_spec/2_07_04_heredoc/`
- Create 8 files in existing `e2e/posix_spec/2_07_redirection/` (redirection rest)
- Create 6 files in new `2_07_04_heredoc/` (here-document)
- Create 3 files in `2_07_redirection/` (open R/W and order interactions)

Total: 17 files.

- [ ] **Step 11.1: Create heredoc directory**

```sh
mkdir -p e2e/posix_spec/2_07_04_heredoc
```

- [ ] **Step 11.2: Write 2.7.1–2.7.4 redirection tests (8 files)**

Create `e2e/posix_spec/2_07_redirection/input_basic.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.1 Redirecting Input
# DESCRIPTION: < redirects stdin from a file
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
printf 'hi\n' > f
cat <f
```

Create `e2e/posix_spec/2_07_redirection/output_truncates_existing.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: > truncates the target file before writing
# EXPECT_OUTPUT: b
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
echo a >f
echo b >f
cat f
```

Create `e2e/posix_spec/2_07_redirection/append_creates.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.3 Appending Redirected Output
# DESCRIPTION: >> creates the file if it does not exist
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
echo hi >>f
cat f
```

Create `e2e/posix_spec/2_07_redirection/append_appends.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.3 Appending Redirected Output
# DESCRIPTION: >> appends to existing content
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
echo a >f
echo b >>f
cat f
```

Create `e2e/posix_spec/2_07_redirection/noclobber_blocks_overwrite.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: set -C prevents > from overwriting existing files
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
cd "$TEST_TMPDIR"
echo a >f
set -C
echo b >f 2>/dev/null
```

Create `e2e/posix_spec/2_07_redirection/noclobber_force_with_pipe.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: >| overrides noclobber to force overwrite
# EXPECT_OUTPUT: b
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
echo a >f
set -C
echo b >|f
cat f
```

Create `e2e/posix_spec/2_07_redirection/stderr_to_file.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: 2> form of output redirection sends stderr to a file
# EXPECT_OUTPUT: err
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
sh -c 'echo err >&2' 2>e
cat e
```

Create `e2e/posix_spec/2_07_redirection/stdout_and_stderr_to_file.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: cmd >f 2>&1 merges stderr into stdout target
# EXPECT_OUTPUT<<END
# out
# err
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
sh -c 'echo out; echo err >&2' >f 2>&1
cat f
```

- [ ] **Step 11.3: Write 2.7.6 here-document tests (6 files)**

Create `e2e/posix_spec/2_07_04_heredoc/basic.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: << reads input until matching delimiter on its own line
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
cat <<EOF
hi
EOF
```

Create `e2e/posix_spec/2_07_04_heredoc/unquoted_delim_expands.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Unquoted delimiter allows parameter expansion in body
# EXPECT_OUTPUT: /tmp/h
# EXPECT_EXIT: 0
HOME=/tmp/h
cat <<EOF
$HOME
EOF
```

Create `e2e/posix_spec/2_07_04_heredoc/quoted_delim_no_expansion.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Quoted delimiter suppresses parameter expansion in body
# EXPECT_OUTPUT: $HOME
# EXPECT_EXIT: 0
HOME=/tmp/h
cat <<'EOF'
$HOME
EOF
```

Create `e2e/posix_spec/2_07_04_heredoc/dash_strips_leading_tabs.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: <<- strips leading tab characters from each input line
# EXPECT_OUTPUT: hey
# EXPECT_EXIT: 0
cat <<-EOF
	hey
	EOF
```

Create `e2e/posix_spec/2_07_04_heredoc/dash_preserves_leading_spaces.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: <<- strips only tabs, not spaces
# EXPECT_OUTPUT:   hey
# EXPECT_EXIT: 0
cat <<-EOF
  hey
EOF
```

Create `e2e/posix_spec/2_07_04_heredoc/escape_dollar_in_unquoted.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Backslash escapes $ in unquoted-delimiter heredoc body
# EXPECT_OUTPUT: $x
# EXPECT_EXIT: 0
x=value
cat <<EOF
\$x
EOF
```

- [ ] **Step 11.4: Write 2.7.7 / 2.7.8 supplement (3 files)**

Create `e2e/posix_spec/2_07_redirection/open_rw_basic.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.7 Open File Descriptors for Reading and Writing
# DESCRIPTION: <> opens a file for reading and writing
# EXPECT_OUTPUT: data
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
exec 3<>f
echo data >&3
exec 3<&-
cat f
```

Create `e2e/posix_spec/2_07_redirection/redir_order_left_to_right.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7 Redirection
# DESCRIPTION: Redirections are processed left-to-right; 2>&1 before >f sends stderr to current stdout
# EXPECT_OUTPUT: out
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
sh -c 'echo out; echo err >&2' 2>&1 >f
cat f
```

Create `e2e/posix_spec/2_07_redirection/multiple_input_last_wins.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.7.1 Redirecting Input
# DESCRIPTION: Multiple input redirections — the last one is effective
# EXPECT_OUTPUT: B
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
printf 'A\n' > a
printf 'B\n' > b
cat <a <b
```

- [ ] **Step 11.5: Permissions, run, commit**

```sh
chmod 644 e2e/posix_spec/2_07_redirection/*.sh e2e/posix_spec/2_07_04_heredoc/*.sh
./e2e/run_tests.sh --filter=2_07_redirection
./e2e/run_tests.sh --filter=2_07_04_heredoc
git add e2e/posix_spec/2_07_redirection e2e/posix_spec/2_07_04_heredoc
git commit -m "$(cat <<'EOF'
test(e2e): add §2.7 redirection rest + here-doc tests (P3.1)

Seventeen normative-clause tests covering input/output redirect,
truncate vs append, noclobber + force, stderr redirect, fd
duplication, here-document (basic, expand, quoted, <<- strip,
escape), open R/W, redirection order, fd close.

Spec: docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch2-deepening-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 12 (P3.2): 2.8 Errors supplement

**Files:**
- Create 5 files in existing `e2e/posix_spec/2_08_01_consequences_of_shell_errors/`

- [ ] **Step 12.1: Write 5 tests**

Create `e2e/posix_spec/2_08_01_consequences_of_shell_errors/special_builtin_redir_error_exits.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: Redirection error on special builtin causes non-interactive shell to exit
# EXPECT_OUTPUT:
# EXPECT_EXIT: 0
# Run in subshell so the parent stays alive. The subshell exits non-zero.
(: < /nonexistent/path 2>/dev/null; echo not-reached) ; :
```

Create `e2e/posix_spec/2_08_01_consequences_of_shell_errors/regular_builtin_redir_error_continues.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: Redirection error on regular builtin does not exit non-interactive shell
# EXPECT_OUTPUT: continued
# EXPECT_EXIT: 0
true < /nonexistent/path 2>/dev/null
echo continued
```

Create `e2e/posix_spec/2_08_01_consequences_of_shell_errors/cmd_not_found_exits_127.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: Unknown command yields exit status 127
# EXPECT_OUTPUT: 127
# EXPECT_EXIT: 0
nonexistent_cmd_xyz 2>/dev/null
echo $?
```

Create `e2e/posix_spec/2_08_01_consequences_of_shell_errors/cmd_not_executable_exits_126.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: Command exists but is not executable yields exit status 126
# EXPECT_OUTPUT: 126
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
: > notexec
chmod 644 notexec
./notexec 2>/dev/null
echo $?
```

Create `e2e/posix_spec/2_08_01_consequences_of_shell_errors/assignment_only_succeeds.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: Assignment-only command has exit status 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
x=value
echo $?
```

- [ ] **Step 12.2: Permissions, run, commit**

```sh
chmod 644 e2e/posix_spec/2_08_01_consequences_of_shell_errors/special_builtin_redir_error_exits.sh e2e/posix_spec/2_08_01_consequences_of_shell_errors/regular_builtin_redir_error_continues.sh e2e/posix_spec/2_08_01_consequences_of_shell_errors/cmd_not_found_exits_127.sh e2e/posix_spec/2_08_01_consequences_of_shell_errors/cmd_not_executable_exits_126.sh e2e/posix_spec/2_08_01_consequences_of_shell_errors/assignment_only_succeeds.sh
./e2e/run_tests.sh --filter=2_08_01
git add e2e/posix_spec/2_08_01_consequences_of_shell_errors
git commit -m "$(cat <<'EOF'
test(e2e): add §2.8 shell error consequences supplement (P3.2)

Five normative-clause tests: special-builtin vs regular-builtin
redirect-error semantics, 127 (not found), 126 (not executable),
and assignment-only success.

Spec: docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch2-deepening-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 13 (P3.3): 2.9 Shell Commands

**Files:**
- Create 5 directories: `2_09_01_simple_commands/`, `2_09_02_pipelines/`, `2_09_03_lists/`, `2_09_04_compound_commands/`, `2_09_05_function_definition/`
- Create 22 test files total

- [ ] **Step 13.1: Create all five directories**

```sh
mkdir -p e2e/posix_spec/2_09_01_simple_commands e2e/posix_spec/2_09_02_pipelines e2e/posix_spec/2_09_03_lists e2e/posix_spec/2_09_04_compound_commands e2e/posix_spec/2_09_05_function_definition
```

- [ ] **Step 13.2: Write 2.9.1 Simple Commands (4 files)**

Create `e2e/posix_spec/2_09_01_simple_commands/assignment_scoped_to_command.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Assignment preceding a command is scoped to that command's environment
# EXPECT_OUTPUT<<END
# scoped
# 
# END
# EXPECT_EXIT: 0
x=scoped sh -c 'echo "$x"'
echo "$x"
```

Create `e2e/posix_spec/2_09_01_simple_commands/redirection_only_creates_file.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: A command consisting only of redirections still applies the redirections
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
>f
test -f f && echo ok
```

Create `e2e/posix_spec/2_09_01_simple_commands/assignment_only_sets_var.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Assignment without a command name sets the variable in the current shell
# EXPECT_OUTPUT: value
# EXPECT_EXIT: 0
x=value
echo "$x"
```

Create `e2e/posix_spec/2_09_01_simple_commands/expansion_then_execute_order.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Word expansion occurs before the command is looked up and executed
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
cmd=echo
$cmd hi
```

- [ ] **Step 13.3: Write 2.9.2 Pipelines (4 files)**

Create `e2e/posix_spec/2_09_02_pipelines/basic.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: | pipes stdout of left to stdin of right
# EXPECT_OUTPUT: a
# EXPECT_EXIT: 0
echo a | cat
```

Create `e2e/posix_spec/2_09_02_pipelines/negation_flips_status.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: ! reserved word negates the exit status of a pipeline
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
! false
echo $?
```

Create `e2e/posix_spec/2_09_02_pipelines/status_is_last_command.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Pipeline exit status is that of the last command
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
false | true
echo $?
```

Create `e2e/posix_spec/2_09_02_pipelines/multi_stage.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Multi-stage pipeline passes data through each stage in order
# EXPECT_OUTPUT: c
# EXPECT_EXIT: 0
echo a | tr a b | tr b c
```

- [ ] **Step 13.4: Write 2.9.3 Lists (4 files)**

Create `e2e/posix_spec/2_09_03_lists/semicolon_sequences.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: ; sequences commands left-to-right
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
echo a; echo b
```

Create `e2e/posix_spec/2_09_03_lists/and_runs_when_left_succeeds.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: && executes right side only when left side succeeds
# EXPECT_OUTPUT: yes
# EXPECT_EXIT: 0
true && echo yes
```

Create `e2e/posix_spec/2_09_03_lists/or_runs_when_left_fails.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: || executes right side only when left side fails
# EXPECT_OUTPUT: no
# EXPECT_EXIT: 0
false || echo no
```

Create `e2e/posix_spec/2_09_03_lists/and_or_left_to_right.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: && and || have equal precedence and associate left-to-right
# EXPECT_OUTPUT: x
# EXPECT_EXIT: 0
true || false && echo x
```

- [ ] **Step 13.5: Write 2.9.4 Compound Commands (6 files)**

Create `e2e/posix_spec/2_09_04_compound_commands/subshell_isolates_assignments.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.4 Compound Commands
# DESCRIPTION: (...) runs commands in a subshell; assignments do not affect the parent
# EXPECT_OUTPUT: unset
# EXPECT_EXIT: 0
(x=value)
echo "${x:-unset}"
```

Create `e2e/posix_spec/2_09_04_compound_commands/brace_group_shares_environment.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.4 Compound Commands
# DESCRIPTION: { ...; } runs in the current shell environment
# EXPECT_OUTPUT: value
# EXPECT_EXIT: 0
{ x=value; }
echo "$x"
```

Create `e2e/posix_spec/2_09_04_compound_commands/for_iterates_list.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.4 Compound Commands
# DESCRIPTION: for iterates over a word list
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
for i in a b; do
    echo "$i"
done
```

Create `e2e/posix_spec/2_09_04_compound_commands/for_without_in_uses_positional.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.4 Compound Commands
# DESCRIPTION: for without "in" iterates over positional parameters
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
set -- a b
for i; do
    echo "$i"
done
```

Create `e2e/posix_spec/2_09_04_compound_commands/case_matches_pattern.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.4 Compound Commands
# DESCRIPTION: case selects the first matching pattern branch
# EXPECT_OUTPUT: matched
# EXPECT_EXIT: 0
case foo in
    bar) echo no ;;
    foo) echo matched ;;
esac
```

Create `e2e/posix_spec/2_09_04_compound_commands/while_loops_until_condition_false.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.4 Compound Commands
# DESCRIPTION: while loops while condition is true
# EXPECT_OUTPUT<<END
# 0
# 1
# 2
# END
# EXPECT_EXIT: 0
i=0
while [ $i -lt 3 ]; do
    echo $i
    i=$((i+1))
done
```

- [ ] **Step 13.6: Write 2.9.5 Function Definition (4 files)**

Create `e2e/posix_spec/2_09_05_function_definition/define_and_call.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Function defined with name() { body; } is callable as a simple command
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
greet() { echo hi; }
greet
```

Create `e2e/posix_spec/2_09_05_function_definition/return_propagates_status.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: return sets the function's exit status
# EXPECT_OUTPUT: 7
# EXPECT_EXIT: 0
f() { return 7; }
f
echo $?
```

Create `e2e/posix_spec/2_09_05_function_definition/modifies_caller_scope.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Function assignments affect the calling shell (no local scope by default)
# EXPECT_OUTPUT: inside
# EXPECT_EXIT: 0
f() { x=inside; }
f
echo "$x"
```

Create `e2e/posix_spec/2_09_05_function_definition/sees_positional_parameters.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Function call passes arguments as positional parameters $1..
# EXPECT_OUTPUT: arg
# EXPECT_EXIT: 0
f() { echo "$1"; }
f arg
```

- [ ] **Step 13.7: Permissions, run, commit**

```sh
chmod 644 e2e/posix_spec/2_09_01_simple_commands/*.sh e2e/posix_spec/2_09_02_pipelines/*.sh e2e/posix_spec/2_09_03_lists/*.sh e2e/posix_spec/2_09_04_compound_commands/*.sh e2e/posix_spec/2_09_05_function_definition/*.sh
./e2e/run_tests.sh --filter=2_09_
git add e2e/posix_spec/2_09_01_simple_commands e2e/posix_spec/2_09_02_pipelines e2e/posix_spec/2_09_03_lists e2e/posix_spec/2_09_04_compound_commands e2e/posix_spec/2_09_05_function_definition
git commit -m "$(cat <<'EOF'
test(e2e): add §2.9 shell commands tests (P3.3)

Twenty-two normative-clause tests covering simple commands (4),
pipelines (4), lists (4), compound commands (6), and function
definition (4).

Spec: docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch2-deepening-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 14 (P3.4): 2.11 Signals supplement

**Files:**
- Create 3 files in existing `e2e/posix_spec/2_11_signals_and_error_handling/`

- [ ] **Step 14.1: Write 3 tests**

Create `e2e/posix_spec/2_11_signals_and_error_handling/trap_list_shows_handlers.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: trap with no arguments lists currently-set handlers
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
trap 'echo bye' INT
out=$(trap)
case "$out" in
    *INT*) echo ok ;;
    *) echo "missing: $out" ;;
esac
```

Create `e2e/posix_spec/2_11_signals_and_error_handling/trap_zero_runs_on_exit.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: trap on signal 0 / EXIT runs at shell exit
# EXPECT_OUTPUT: bye
# EXPECT_EXIT: 0
(trap 'echo bye' 0; :)
```

Create `e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: Subshell starts with traps reset to default for signals not caught in parent
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
trap 'echo parent' USR1
out=$( (trap) )
case "$out" in
    *USR1*) echo unexpected ;;
    *) echo ok ;;
esac
```

- [ ] **Step 14.2: Permissions, run, commit**

```sh
chmod 644 e2e/posix_spec/2_11_signals_and_error_handling/trap_list_shows_handlers.sh e2e/posix_spec/2_11_signals_and_error_handling/trap_zero_runs_on_exit.sh e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh
./e2e/run_tests.sh --filter=2_11_signals
git add e2e/posix_spec/2_11_signals_and_error_handling
git commit -m "$(cat <<'EOF'
test(e2e): add §2.11 signals supplement (P3.4)

Three normative-clause tests: bare trap listing, trap 0 / EXIT
behavior, subshell trap reset.

Spec: docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch2-deepening-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 15 (P3.5): 2.12 Shell Execution Environment

**Files:**
- Create dir: `e2e/posix_spec/2_12_shell_exec_env/`
- Create 4 test files

- [ ] **Step 15.1: Create directory and write tests**

```sh
mkdir -p e2e/posix_spec/2_12_shell_exec_env
```

Create `e2e/posix_spec/2_12_shell_exec_env/export_propagates_to_child.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Exported variables are visible in child processes
# EXPECT_OUTPUT: value
# EXPECT_EXIT: 0
export x=value
sh -c 'echo "$x"'
```

Create `e2e/posix_spec/2_12_shell_exec_env/unexported_not_in_child.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Unexported variables are not visible in child processes
# EXPECT_OUTPUT: unset
# EXPECT_EXIT: 0
x=value
sh -c 'echo "${x:-unset}"'
```

Create `e2e/posix_spec/2_12_shell_exec_env/subshell_inherits_unexported.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: A subshell (parenthesized list) inherits the parent's variables
# EXPECT_OUTPUT: value
# EXPECT_EXIT: 0
x=value
(echo "$x")
```

Create `e2e/posix_spec/2_12_shell_exec_env/subshell_changes_isolated.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Variable changes in a subshell do not affect the parent
# EXPECT_OUTPUT: original
# EXPECT_EXIT: 0
x=original
(x=changed)
echo "$x"
```

- [ ] **Step 15.2: Permissions, run, commit**

```sh
chmod 644 e2e/posix_spec/2_12_shell_exec_env/*.sh
./e2e/run_tests.sh --filter=2_12_shell_exec_env
git add e2e/posix_spec/2_12_shell_exec_env
git commit -m "$(cat <<'EOF'
test(e2e): add §2.12 shell execution environment tests (P3.5)

Four normative-clause tests: export propagates, unexported
hidden, subshell inherits, subshell-change isolated.

Spec: docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch2-deepening-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 16 (P4.1): Existing § supplement (2.3 / 2.4 / 2.10 / 2.13)

**Files:**
- Create 4 files in `2_03_token_recognition/`
- Create 3 files in `2_04_reserved_words/`
- Create 4 files in `2_10_shell_grammar/`
- Create 4 files in `2_13_pattern_matching/`

Total: 15 files.

- [ ] **Step 16.1: Write 2.3 Token Recognition supplement (4 files)**

Create `e2e/posix_spec/2_03_token_recognition/operator_inside_dquote_is_literal.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.3 Token Recognition
# DESCRIPTION: Operator characters inside double-quotes are literal
# EXPECT_OUTPUT: |
# EXPECT_EXIT: 0
echo "|"
```

Create `e2e/posix_spec/2_03_token_recognition/operator_inside_squote_is_literal.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.3 Token Recognition
# DESCRIPTION: Operator characters inside single-quotes are literal
# EXPECT_OUTPUT: &
# EXPECT_EXIT: 0
echo '&'
```

Create `e2e/posix_spec/2_03_token_recognition/escape_breaks_operator_recognition.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.3 Token Recognition
# DESCRIPTION: Backslash-escaped operator character is treated as a word character
# EXPECT_OUTPUT: &
# EXPECT_EXIT: 0
echo \&
```

Create `e2e/posix_spec/2_03_token_recognition/blank_separates_words.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.3 Token Recognition
# DESCRIPTION: Unquoted blank characters separate tokens
# EXPECT_OUTPUT: 2
# EXPECT_EXIT: 0
set -- a b
echo $#
```

- [ ] **Step 16.2: Write 2.4 Reserved Words supplement (3 files)**

Create `e2e/posix_spec/2_04_reserved_words/reserved_after_assignment_recognized.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: Reserved word in command position is recognized after assignment prefix
# EXPECT_OUTPUT: y
# EXPECT_EXIT: 0
x=1 if true; then echo y; fi
```

Create `e2e/posix_spec/2_04_reserved_words/reserved_inside_dquote_is_word.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: Reserved word inside double-quotes is a literal word, not a keyword
# EXPECT_OUTPUT: if
# EXPECT_EXIT: 0
echo "if"
```

Create `e2e/posix_spec/2_04_reserved_words/reserved_after_pipe_recognized.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: Reserved word after | is recognized as command-position start
# EXPECT_OUTPUT: looped
# EXPECT_EXIT: 0
echo a | while read x; do echo looped; done
```

- [ ] **Step 16.3: Write 2.10 Shell Grammar supplement (4 files)**

Create `e2e/posix_spec/2_10_shell_grammar/newline_terminates_command.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar - Newline as Terminator
# DESCRIPTION: Newline terminates a command equivalent to semicolon
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
echo a
echo b
```

Create `e2e/posix_spec/2_10_shell_grammar/pipe_continuation_across_newline.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar - Pipe Continuation
# DESCRIPTION: Newline after | continues the pipeline implicitly
# EXPECT_OUTPUT: A
# EXPECT_EXIT: 0
echo a |
    tr a A
```

Create `e2e/posix_spec/2_10_shell_grammar/ampersand_terminates_command.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar - Background Operator
# DESCRIPTION: & terminates a command and runs it in the background
# EXPECT_OUTPUT: done
# EXPECT_EXIT: 0
sleep 0 &
echo done
wait
```

Create `e2e/posix_spec/2_10_shell_grammar/compound_assignment_before_function_call.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar - Assignment Prefix
# DESCRIPTION: Assignment prefix on a function call sets the variable for that call's environment
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
f() { echo "$x"; }
x=1 f
```

- [ ] **Step 16.4: Write 2.13 Pattern Matching supplement (4 files)**

Create `e2e/posix_spec/2_13_pattern_matching/bracket_range.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: [a-c] matches any character in the range a..c
# EXPECT_OUTPUT: in
# EXPECT_EXIT: 0
case b in
    [a-c]) echo in ;;
    *) echo out ;;
esac
```

Create `e2e/posix_spec/2_13_pattern_matching/star_matches_empty.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: * matches the empty string
# EXPECT_OUTPUT: matched
# EXPECT_EXIT: 0
case "" in
    *) echo matched ;;
esac
```

Create `e2e/posix_spec/2_13_pattern_matching/question_no_match_for_empty.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: ? does not match an empty string
# EXPECT_OUTPUT: none
# EXPECT_EXIT: 0
case "" in
    ?) echo one ;;
    *) echo none ;;
esac
```

Create `e2e/posix_spec/2_13_pattern_matching/escape_meta_is_literal.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: Escaped pattern metacharacter is matched literally
# EXPECT_OUTPUT: lit
# EXPECT_EXIT: 0
case "*" in
    \*) echo lit ;;
    *) echo other ;;
esac
```

- [ ] **Step 16.5: Permissions, run, commit**

```sh
chmod 644 e2e/posix_spec/2_03_token_recognition/operator_inside_dquote_is_literal.sh e2e/posix_spec/2_03_token_recognition/operator_inside_squote_is_literal.sh e2e/posix_spec/2_03_token_recognition/escape_breaks_operator_recognition.sh e2e/posix_spec/2_03_token_recognition/blank_separates_words.sh
chmod 644 e2e/posix_spec/2_04_reserved_words/reserved_after_assignment_recognized.sh e2e/posix_spec/2_04_reserved_words/reserved_inside_dquote_is_word.sh e2e/posix_spec/2_04_reserved_words/reserved_after_pipe_recognized.sh
chmod 644 e2e/posix_spec/2_10_shell_grammar/newline_terminates_command.sh e2e/posix_spec/2_10_shell_grammar/pipe_continuation_across_newline.sh e2e/posix_spec/2_10_shell_grammar/ampersand_terminates_command.sh e2e/posix_spec/2_10_shell_grammar/compound_assignment_before_function_call.sh
chmod 644 e2e/posix_spec/2_13_pattern_matching/bracket_range.sh e2e/posix_spec/2_13_pattern_matching/star_matches_empty.sh e2e/posix_spec/2_13_pattern_matching/question_no_match_for_empty.sh e2e/posix_spec/2_13_pattern_matching/escape_meta_is_literal.sh

./e2e/run_tests.sh --filter=2_03_token_recognition
./e2e/run_tests.sh --filter=2_04_reserved_words
./e2e/run_tests.sh --filter=2_10_shell_grammar
./e2e/run_tests.sh --filter=2_13_pattern_matching

git add e2e/posix_spec/2_03_token_recognition e2e/posix_spec/2_04_reserved_words e2e/posix_spec/2_10_shell_grammar e2e/posix_spec/2_13_pattern_matching
git commit -m "$(cat <<'EOF'
test(e2e): add §2.3 / §2.4 / §2.10 / §2.13 supplements (P4.1)

Fifteen normative-clause tests filling gaps in existing
directories: token recognition (4), reserved words (3), shell
grammar (4), pattern matching (4).

Spec: docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch2-deepening-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 17: Final verification, TODO.md cleanup, and XFAIL backlog

**Files:**
- Modify: `TODO.md` — delete the `Future: E2E Test Expansion` section; if any new XFAILs surfaced during P1–P4, append them to `Future: POSIX Conformance Bugs`.

- [ ] **Step 17.1: Run the full E2E suite**

```sh
./e2e/run_tests.sh 2>&1 | tail -3 | tee /tmp/e2e-ch2-final.txt
```

Compare `/tmp/e2e-ch2-final.txt` against `/tmp/e2e-ch2-baseline.txt` (recorded in Pre-flight). Expected:

- `Total` increases by ~142.
- `Failed` is unchanged (no regression on existing).
- `XPass` is 0.
- `XFail` may have grown by up to ~50 due to new XFAIL entries added during P1–P4.

If `Failed` increased: stop and triage. Failures must be either fixed in the test body or re-classified as XFAIL.
If `XPass` increased: list the XPASS tests (`./e2e/run_tests.sh --verbose 2>&1 | grep XPASS`) and remove the `# XFAIL:` line from each in a follow-up commit.

- [ ] **Step 17.2: Run cargo test for regression check**

```sh
cargo test
```

Expected: all PASS (test additions are E2E only; no Rust code changed).

- [ ] **Step 17.3: Run fmt and clippy gates**

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: both clean. If either fails, the failure must be from pre-existing drift (not from this branch), since no Rust code was modified.

- [ ] **Step 17.4: Delete the `Future: E2E Test Expansion` section from TODO.md**

In `TODO.md`, locate this block:

```
## Future: E2E Test Expansion

- [ ] Deepen Chapter 2 POSIX coverage to normative-requirement granularity — after the hybrid (representative + thin-section) coverage lands, enumerate every shall/must/should clause in XCU Chapter 2 and add one E2E test per normative requirement (est. +100–200 tests). Use `XFAIL` liberally to register gaps; the goal is to make each normative clause individually traceable to a test ID.
```

Delete the entire `## Future: E2E Test Expansion` section (heading + bullet + trailing blank line). The TODO.md convention (CLAUDE.md) is to delete completed items, not mark them `[x]`.

- [ ] **Step 17.5: Append XFAIL discoveries to `Future: POSIX Conformance Bugs`**

If any tests in P1–P4 were committed with `# XFAIL: not yet implemented (TODO: ...)` or `# XFAIL: non-POSIX deviation (...)`, append a one-line bullet for each new finding to TODO.md under `## Future: POSIX Conformance Bugs`. Format mirrors the existing 2026-05-13 entries:

```
- [ ] <one-line description of the deviation>. XFAIL test:
      `e2e/posix_spec/<dir>/<file>.sh`.
```

Skip this step if no new XFAILs were added during P1–P4.

- [ ] **Step 17.6: Verify TODO.md and commit cleanup**

Run: `git diff TODO.md`
Expected: shows the deleted section and any appended `Future: POSIX Conformance Bugs` bullets.

```sh
git add TODO.md
git commit -m "$(cat <<'EOF'
docs(todo): complete Ch2 normative-clause E2E deepening

Removes the Future: E2E Test Expansion section; the +142 new
clause-granularity tests across §2.1–§2.13 land in posix_spec/2_*/
under the 2026-05-13 Ch2 deepening plan. Newly-discovered
XFAILs (if any) are appended to Future: POSIX Conformance Bugs
as actionable backlog items.

Plan: docs/superpowers/plans/2026-05-13-e2e-test-expansion-ch2-deepening.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 17.7: Final cross-phase smoke**

```sh
./e2e/run_tests.sh 2>&1 | tail -3
cargo test 2>&1 | tail -5
cargo fmt --all -- --check && echo "fmt clean"
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
git log --oneline -20
```

Expected:
- E2E summary shows `Failed: 0`, `XPass: 0`, with `Total` ≈ baseline + 142.
- `cargo test` summary shows all PASS.
- `fmt clean` printed.
- clippy reports no warnings.
- `git log --oneline -20` shows ~17 commits from Tasks 1–17.

Plan complete.
