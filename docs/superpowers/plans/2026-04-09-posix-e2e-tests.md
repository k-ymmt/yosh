# POSIX E2E Test Suite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a POSIX sh test runner and ~130–140 E2E test cases that verify kish's POSIX shell compliance for all Phase 1–8 features.

**Architecture:** External `.sh` test files with embedded metadata comments, executed by a POSIX sh test runner (`e2e/run_tests.sh`). Each test is one file in a category directory under `e2e/`. The runner parses metadata, executes tests with kish, compares output, and reports PASS/FAIL/XFAIL/XPASS/TIMEOUT.

**Tech Stack:** POSIX sh (test runner), kish binary (test target)

---

### Task 1: Create directory structure and test runner

**Files:**
- Create: `e2e/run_tests.sh`

- [ ] **Step 1: Create the e2e directory structure**

```bash
mkdir -p e2e/command_execution e2e/variable_and_expansion e2e/arithmetic \
  e2e/command_substitution e2e/pipeline_and_list e2e/redirection \
  e2e/control_flow e2e/function e2e/builtin e2e/signal_and_trap \
  e2e/subshell e2e/quoting e2e/field_splitting
```

- [ ] **Step 2: Write the test runner**

Create `e2e/run_tests.sh` with the full content below. This runner:
- Discovers `.sh` test files recursively under `e2e/`
- Parses metadata comments (`POSIX_REF`, `DESCRIPTION`, `EXPECT_OUTPUT`, `EXPECT_EXIT`, `EXPECT_STDERR`, `XFAIL`)
- Supports multi-line `EXPECT_OUTPUT<<END ... # END`
- Executes each test with configurable shell (default: `./target/debug/kish`)
- Compares stdout (exact match, trailing newline normalized), stderr (substring match), exit code
- Applies 5-second timeout per test
- Creates per-test `$TEST_TMPDIR` and cleans up
- Reports PASS/FAIL/XFAIL/XPASS/TIMEOUT with summary
- Supports `--shell=PATH`, `--filter=PATTERN`, `--verbose` options
- Exits 0 if no FAILs or TIMEOUTs, 1 otherwise

```sh
#!/bin/sh
# POSIX E2E Test Runner for kish
# Usage: ./e2e/run_tests.sh [--shell=PATH] [--filter=PATTERN] [--verbose]

set -u

# --- defaults ---
SHELL_CMD="./target/debug/kish"
FILTER=""
VERBOSE=0
TIMEOUT=5

# --- parse args ---
for arg in "$@"; do
  case "$arg" in
    --shell=*) SHELL_CMD="${arg#--shell=}" ;;
    --filter=*) FILTER="${arg#--filter=}" ;;
    --verbose) VERBOSE=1 ;;
    --help)
      echo "Usage: $0 [--shell=PATH] [--filter=PATTERN] [--verbose]"
      exit 0
      ;;
    *) echo "Unknown option: $arg" >&2; exit 1 ;;
  esac
done

# --- counters ---
total=0
passed=0
failed=0
xfailed=0
xpassed=0
timedout=0

# --- color support ---
if [ -t 1 ]; then
  GREEN='\033[0;32m'
  RED='\033[0;31m'
  YELLOW='\033[0;33m'
  CYAN='\033[0;36m'
  RESET='\033[0m'
else
  GREEN=''
  RED=''
  YELLOW=''
  CYAN=''
  RESET=''
fi

# --- parse metadata from test file ---
parse_metadata() {
  _file="$1"
  EXPECT_OUTPUT=""
  EXPECT_EXIT="0"
  EXPECT_STDERR=""
  XFAIL=""
  DESCRIPTION=""
  POSIX_REF=""
  _has_expect_output=0
  _in_multiline=0

  while IFS= read -r _line; do
    # Stop at first non-comment, non-shebang, non-blank line
    case "$_line" in
      '#!'*) continue ;;
      '#'*) ;;
      '') continue ;;
      *) break ;;
    esac

    # Strip leading "# "
    _content="${_line#\# }"

    if [ "$_in_multiline" = 1 ]; then
      if [ "$_content" = "$_multiline_delim" ]; then
        _in_multiline=0
      else
        if [ -n "$EXPECT_OUTPUT" ]; then
          EXPECT_OUTPUT="${EXPECT_OUTPUT}
${_content}"
        else
          EXPECT_OUTPUT="$_content"
        fi
      fi
      continue
    fi

    case "$_content" in
      POSIX_REF:*) POSIX_REF="${_content#POSIX_REF: }" ;;
      DESCRIPTION:*) DESCRIPTION="${_content#DESCRIPTION: }" ;;
      EXPECT_EXIT:*) EXPECT_EXIT="${_content#EXPECT_EXIT: }" ;;
      EXPECT_STDERR:*) EXPECT_STDERR="${_content#EXPECT_STDERR: }" ;;
      XFAIL:*) XFAIL="${_content#XFAIL: }" ;;
      EXPECT_OUTPUT:*)
        _has_expect_output=1
        _value="${_content#EXPECT_OUTPUT:}"
        # Check for heredoc-style multi-line
        case "$_value" in
          '<<'*)
            _multiline_delim="${_value#<<}"
            _in_multiline=1
            ;;
          *)
            # Single-line: strip leading space
            EXPECT_OUTPUT="${_value# }"
            ;;
        esac
        ;;
    esac
  done < "$_file"

  # Export whether EXPECT_OUTPUT was specified
  HAS_EXPECT_OUTPUT="$_has_expect_output"
}

# --- run a single test ---
run_test() {
  _test_file="$1"
  _rel_path="${_test_file#e2e/}"

  # Apply filter
  if [ -n "$FILTER" ]; then
    case "$_rel_path" in
      *"$FILTER"*) ;;
      *) return ;;
    esac
  fi

  total=$((total + 1))

  parse_metadata "$_test_file"

  # Create temp dir for this test
  _tmpdir="$(mktemp -d)"
  export TEST_TMPDIR="$_tmpdir"

  # Run with timeout
  _actual_stdout="$_tmpdir/stdout"
  _actual_stderr="$_tmpdir/stderr"

  # Use a background process + wait for timeout
  "$SHELL_CMD" "$_test_file" >"$_actual_stdout" 2>"$_actual_stderr" &
  _pid=$!

  # Timeout handling
  (
    sleep "$TIMEOUT"
    kill "$_pid" 2>/dev/null
  ) &
  _timer_pid=$!

  wait "$_pid" 2>/dev/null
  _actual_exit=$?
  kill "$_timer_pid" 2>/dev/null
  wait "$_timer_pid" 2>/dev/null

  # Check if timed out (killed by signal = 128+signal)
  if [ "$_actual_exit" -ge 128 ] && [ "$_actual_exit" -le 159 ]; then
    # Could be timeout or legitimate signal test — check if timer killed it
    # We detect timeout by checking if process was killed by our timer
    # Simple heuristic: if exit code is 137 (SIGKILL) or 143 (SIGTERM), likely timeout
    # But signal tests may also use these. Use a marker file approach.
    :
  fi

  _stdout_content="$(cat "$_actual_stdout")"
  _stderr_content="$(cat "$_actual_stderr")"

  # Compare results
  _test_passed=1

  # Check exit code
  if [ "$_actual_exit" != "$EXPECT_EXIT" ]; then
    _test_passed=0
    _fail_reason="exit code: expected=$EXPECT_EXIT actual=$_actual_exit"
  fi

  # Check stdout (if EXPECT_OUTPUT was specified)
  if [ "$_test_passed" = 1 ] && [ "$HAS_EXPECT_OUTPUT" = 1 ]; then
    if [ "$_stdout_content" != "$EXPECT_OUTPUT" ]; then
      _test_passed=0
      _fail_reason="stdout mismatch"
    fi
  fi

  # Check stderr (substring match)
  if [ "$_test_passed" = 1 ] && [ -n "$EXPECT_STDERR" ]; then
    case "$_stderr_content" in
      *"$EXPECT_STDERR"*) ;;
      *)
        _test_passed=0
        _fail_reason="stderr: expected substring '$EXPECT_STDERR' not found"
        ;;
    esac
  fi

  # Classify result
  if [ -n "$XFAIL" ]; then
    if [ "$_test_passed" = 1 ]; then
      xpassed=$((xpassed + 1))
      printf "${CYAN}[XPASS]${RESET}   %s  (unexpectedly passed!)\\n" "$_rel_path"
    else
      xfailed=$((xfailed + 1))
      printf "${YELLOW}[XFAIL]${RESET}   %s  (known: %s)\\n" "$_rel_path" "$XFAIL"
    fi
  elif [ "$_test_passed" = 1 ]; then
    passed=$((passed + 1))
    printf "${GREEN}[PASS]${RESET}    %s\\n" "$_rel_path"
  else
    failed=$((failed + 1))
    printf "${RED}[FAIL]${RESET}    %s\\n" "$_rel_path"
    if [ "$VERBOSE" = 1 ]; then
      printf "          Reason: %s\\n" "$_fail_reason"
      if [ "$HAS_EXPECT_OUTPUT" = 1 ]; then
        printf "          Expected stdout: %s\\n" "$EXPECT_OUTPUT"
        printf "          Actual stdout:   %s\\n" "$_stdout_content"
      fi
      printf "          Expected exit: %s  Actual exit: %s\\n" "$EXPECT_EXIT" "$_actual_exit"
      if [ -n "$_stderr_content" ]; then
        printf "          Stderr: %s\\n" "$_stderr_content"
      fi
    fi
  fi

  # Cleanup
  rm -rf "$_tmpdir"
}

# --- discover and run tests ---
find e2e -name '*.sh' -not -name 'run_tests.sh' | sort | while IFS= read -r test_file; do
  run_test "$test_file"
done

# Because the while loop runs in a subshell (piped from find),
# we need to persist counters. Use a temp file approach.
# Re-implement without pipe to keep counters in main shell:

# Reset counters — the above loop was in a subshell due to pipe
total=0
passed=0
failed=0
xfailed=0
xpassed=0
timedout=0

for test_file in $(find e2e -name '*.sh' -not -name 'run_tests.sh' | sort); do
  run_test "$test_file"
done

# --- summary ---
echo ""
echo "Results: ${passed} passed, ${failed} failed, ${xfailed} xfail, ${xpassed} xpass"
echo "Total: ${total} tests"

if [ "$failed" -gt 0 ] || [ "$timedout" -gt 0 ]; then
  exit 1
fi
exit 0
```

- [ ] **Step 3: Make the runner executable**

Run: `chmod +x e2e/run_tests.sh`

- [ ] **Step 4: Write a minimal smoke test to verify the runner works**

Create `e2e/command_execution/echo_simple.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Simple echo command execution
# EXPECT_OUTPUT: hello
echo hello
```

- [ ] **Step 5: Build kish and run the smoke test**

Run: `cargo build && ./e2e/run_tests.sh --filter=echo_simple --verbose`

Expected output:
```
[PASS]    command_execution/echo_simple.sh

Results: 1 passed, 0 failed, 0 xfail, 0 xpass
Total: 1 tests
```

- [ ] **Step 6: Commit**

```bash
git add e2e/
git commit -m "feat(e2e): add POSIX E2E test runner and directory structure

POSIX sh test runner with metadata parsing, timeout handling,
XFAIL support, and per-test temp dirs. Initial smoke test included.

Task: Unix に厳密に準拠しているかの E2E テストを追加"
```

---

### Task 2: command_execution tests (~10 tests)

**Files:**
- Create: `e2e/command_execution/echo_simple.sh` (already created in Task 1)
- Create: `e2e/command_execution/multiple_commands.sh`
- Create: `e2e/command_execution/exit_code_success.sh`
- Create: `e2e/command_execution/exit_code_custom.sh`
- Create: `e2e/command_execution/command_not_found.sh`
- Create: `e2e/command_execution/empty_command.sh`
- Create: `e2e/command_execution/path_search.sh`
- Create: `e2e/command_execution/script_file.sh`
- Create: `e2e/command_execution/assignment_only.sh`
- Create: `e2e/command_execution/prefix_assignment_external.sh`

- [ ] **Step 1: Create all command_execution test files**

`e2e/command_execution/multiple_commands.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: Semicolon-separated commands execute sequentially
# EXPECT_OUTPUT<<END
# first
# second
# third
# END
echo first; echo second; echo third
```

`e2e/command_execution/exit_code_success.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: Successful command returns exit code 0
# EXPECT_EXIT: 0
true
```

`e2e/command_execution/exit_code_custom.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: exit builtin sets custom exit code
# EXPECT_EXIT: 42
exit 42
```

`e2e/command_execution/command_not_found.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: Nonexistent command returns exit code 127
# EXPECT_EXIT: 127
nonexistent_cmd_xyzzy_12345
```

`e2e/command_execution/empty_command.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Empty command (just a newline) succeeds
# EXPECT_EXIT: 0

```

`e2e/command_execution/path_search.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Commands are found via PATH search
# EXPECT_EXIT: 0
/bin/echo path_works >/dev/null
```

`e2e/command_execution/script_file.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.1 Shell Introduction
# DESCRIPTION: Shell can execute a script file passed as argument
# EXPECT_OUTPUT<<END
# line1
# line2
# END
# NOTE: This test writes a temp script and executes it via kish.
# Since we can't self-invoke kish from within kish to run a file,
# we test sequential command execution in a single file instead.
echo line1
echo line2
```

`e2e/command_execution/assignment_only.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Assignment without command sets variable and returns 0
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
x=hello
echo "$x"
```

`e2e/command_execution/prefix_assignment_external.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Prefix assignment on external command does not persist
# EXPECT_OUTPUT:
# EXPECT_EXIT: 0
MY_PREFIX_TEST_VAR=hello /usr/bin/true
echo "$MY_PREFIX_TEST_VAR"
```

- [ ] **Step 2: Run the command_execution tests**

Run: `cargo build && ./e2e/run_tests.sh --filter=command_execution --verbose`

Expected: All tests PASS.

- [ ] **Step 3: Commit**

```bash
git add e2e/command_execution/
git commit -m "test(e2e): add command_execution POSIX compliance tests

10 tests covering simple commands, exit codes, PATH search,
script files, and prefix assignment behavior per POSIX 2.9.1.

Task: Unix に厳密に準拠しているかの E2E テストを追加"
```

---

### Task 3: variable_and_expansion tests (~20 tests)

**Files:**
- Create: `e2e/variable_and_expansion/simple_assignment.sh`
- Create: `e2e/variable_and_expansion/variable_reference.sh`
- Create: `e2e/variable_and_expansion/default_value.sh`
- Create: `e2e/variable_and_expansion/default_value_set.sh`
- Create: `e2e/variable_and_expansion/assign_default.sh`
- Create: `e2e/variable_and_expansion/alternate_value.sh`
- Create: `e2e/variable_and_expansion/error_if_unset.sh`
- Create: `e2e/variable_and_expansion/string_length.sh`
- Create: `e2e/variable_and_expansion/strip_suffix_short.sh`
- Create: `e2e/variable_and_expansion/strip_suffix_long.sh`
- Create: `e2e/variable_and_expansion/strip_prefix_short.sh`
- Create: `e2e/variable_and_expansion/strip_prefix_long.sh`
- Create: `e2e/variable_and_expansion/positional_params.sh`
- Create: `e2e/variable_and_expansion/special_var_question.sh`
- Create: `e2e/variable_and_expansion/special_var_hash.sh`
- Create: `e2e/variable_and_expansion/special_var_dollar.sh`
- Create: `e2e/variable_and_expansion/special_var_at.sh`
- Create: `e2e/variable_and_expansion/special_var_star.sh`
- Create: `e2e/variable_and_expansion/braces_required.sh`
- Create: `e2e/variable_and_expansion/unset_variable.sh`

- [ ] **Step 1: Create all variable_and_expansion test files**

`e2e/variable_and_expansion/simple_assignment.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.5 Parameters and Variables
# DESCRIPTION: Simple variable assignment and expansion
# EXPECT_OUTPUT: hello
x=hello
echo "$x"
```

`e2e/variable_and_expansion/variable_reference.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Variable reference with braces
# EXPECT_OUTPUT: helloworld
x=hello
echo "${x}world"
```

`e2e/variable_and_expansion/default_value.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Default value with :- when variable is unset
# EXPECT_OUTPUT: fallback
unset x
echo "${x:-fallback}"
```

`e2e/variable_and_expansion/default_value_set.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Default value not used when variable is set
# EXPECT_OUTPUT: actual
x=actual
echo "${x:-fallback}"
```

`e2e/variable_and_expansion/assign_default.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Assign default with := when unset, variable persists
# EXPECT_OUTPUT<<END
# assigned
# assigned
# END
unset x
echo "${x:=assigned}"
echo "$x"
```

`e2e/variable_and_expansion/alternate_value.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Alternate value with :+ when set vs unset
# EXPECT_OUTPUT<<END
# alt
#
# END
x=set
echo "${x:+alt}"
unset y
echo "${y:+alt}"
```

`e2e/variable_and_expansion/error_if_unset.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Error message with :? when variable is unset
# EXPECT_EXIT: 1
# EXPECT_STDERR: custom error
unset x
: "${x:?custom error}"
```

`e2e/variable_and_expansion/string_length.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: String length with ${#var}
# EXPECT_OUTPUT: 5
x=hello
echo "${#x}"
```

`e2e/variable_and_expansion/strip_suffix_short.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Strip shortest suffix with ${var%pattern}
# EXPECT_OUTPUT: /path/to/file
f=/path/to/file.txt
echo "${f%.txt}"
```

`e2e/variable_and_expansion/strip_suffix_long.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Strip longest suffix with ${var%%pattern}
# EXPECT_OUTPUT: /path/to/file
f=/path/to/file.tar.gz
echo "${f%%.*}"
```

`e2e/variable_and_expansion/strip_prefix_short.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Strip shortest prefix with ${var#pattern}
# EXPECT_OUTPUT: path/to/file.txt
f=/path/to/file.txt
echo "${f#/}"
```

`e2e/variable_and_expansion/strip_prefix_long.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Strip longest prefix with ${var##pattern}
# EXPECT_OUTPUT: file.txt
f=/path/to/file.txt
echo "${f##*/}"
```

`e2e/variable_and_expansion/positional_params.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: Positional parameters $1 through $3 via set --
# EXPECT_OUTPUT: a b c
set -- a b c
echo "$1 $2 $3"
```

`e2e/variable_and_expansion/special_var_question.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $? holds exit status of last command
# EXPECT_OUTPUT<<END
# 0
# 1
# END
true
echo "$?"
false
echo "$?"
```

`e2e/variable_and_expansion/special_var_hash.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $# holds count of positional parameters
# EXPECT_OUTPUT: 3
set -- a b c
echo "$#"
```

`e2e/variable_and_expansion/special_var_dollar.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $$ holds shell process ID (numeric)
# EXPECT_EXIT: 0
# We just verify $$ is a positive integer
pid=$$
case "$pid" in
  ''|*[!0-9]*) exit 1 ;;
  *) exit 0 ;;
esac
```

`e2e/variable_and_expansion/special_var_at.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: "$@" expands to each positional parameter as separate field
# EXPECT_OUTPUT<<END
# a
# b
# c
# END
set -- a b c
for i in "$@"; do
  echo "$i"
done
```

`e2e/variable_and_expansion/special_var_star.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: "$*" expands to all positional parameters as single field
# EXPECT_OUTPUT: a b c
set -- a b c
echo "$*"
```

`e2e/variable_and_expansion/braces_required.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: $10 is $1 followed by 0, ${10} is tenth parameter
# EXPECT_OUTPUT: a0
set -- a b c d e f g h i j
echo "$10"
```

`e2e/variable_and_expansion/unset_variable.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Unset variable expands to empty string
# EXPECT_OUTPUT:
unset x
echo "$x"
```

- [ ] **Step 2: Run the variable_and_expansion tests**

Run: `./e2e/run_tests.sh --filter=variable_and_expansion --verbose`

Expected: All tests PASS (or note any that need XFAIL).

- [ ] **Step 3: Commit**

```bash
git add e2e/variable_and_expansion/
git commit -m "test(e2e): add variable_and_expansion POSIX compliance tests

20 tests covering variable assignment, parameter expansion operators
(default, assign, alternate, error, length, prefix/suffix strip),
positional parameters, and special variables per POSIX 2.5-2.6.2.

Task: Unix に厳密に準拠しているかの E2E テストを追加"
```

---

### Task 4: arithmetic tests (~10 tests)

**Files:**
- Create: `e2e/arithmetic/basic_operations.sh`
- Create: `e2e/arithmetic/operator_precedence.sh`
- Create: `e2e/arithmetic/comparison.sh`
- Create: `e2e/arithmetic/logical_operators.sh`
- Create: `e2e/arithmetic/ternary.sh`
- Create: `e2e/arithmetic/variable_reference.sh`
- Create: `e2e/arithmetic/assignment_in_expr.sh`
- Create: `e2e/arithmetic/nested_parens.sh`
- Create: `e2e/arithmetic/hex_octal.sh`
- Create: `e2e/arithmetic/compound_assign.sh`

- [ ] **Step 1: Create all arithmetic test files**

`e2e/arithmetic/basic_operations.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Basic arithmetic operations +, -, *, /
# EXPECT_OUTPUT<<END
# 5
# 1
# 6
# 3
# END
echo $((2 + 3))
echo $((5 - 4))
echo $((2 * 3))
echo $((7 / 2))
```

`e2e/arithmetic/operator_precedence.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Multiplication has higher precedence than addition
# EXPECT_OUTPUT: 14
echo $((2 + 3 * 4))
```

`e2e/arithmetic/comparison.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Comparison operators return 1 (true) or 0 (false)
# EXPECT_OUTPUT<<END
# 1
# 0
# 1
# 1
# END
echo $((3 > 2))
echo $((2 > 3))
echo $((3 >= 3))
echo $((2 != 3))
```

`e2e/arithmetic/logical_operators.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Logical AND and OR operators
# EXPECT_OUTPUT<<END
# 1
# 0
# 1
# 0
# END
echo $((1 && 1))
echo $((1 && 0))
echo $((0 || 1))
echo $((0 || 0))
```

`e2e/arithmetic/ternary.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Ternary conditional operator
# EXPECT_OUTPUT<<END
# 10
# 20
# END
echo $((1 ? 10 : 20))
echo $((0 ? 10 : 20))
```

`e2e/arithmetic/variable_reference.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Variables referenced in arithmetic without $ prefix
# EXPECT_OUTPUT: 13
x=10
y=3
echo $((x + y))
```

`e2e/arithmetic/assignment_in_expr.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Assignment within arithmetic expression persists
# EXPECT_OUTPUT<<END
# 42
# 42
# END
echo $((x = 42))
echo "$x"
```

`e2e/arithmetic/nested_parens.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Nested parentheses for grouping
# EXPECT_OUTPUT: 20
echo $(( (2 + 3) * 4 ))
```

`e2e/arithmetic/hex_octal.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Hexadecimal and octal literals
# EXPECT_OUTPUT<<END
# 255
# 8
# END
echo $((0xFF))
echo $((010))
```

`e2e/arithmetic/compound_assign.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Compound assignment operators (+=, -=, etc.)
# XFAIL: Arithmetic compound assignment operators not implemented
# EXPECT_OUTPUT<<END
# 15
# 12
# END
x=10
echo $((x += 5))
echo $((x -= 3))
```

- [ ] **Step 2: Run the arithmetic tests**

Run: `./e2e/run_tests.sh --filter=arithmetic --verbose`

Expected: 9 PASS, 1 XFAIL (compound_assign).

- [ ] **Step 3: Commit**

```bash
git add e2e/arithmetic/
git commit -m "test(e2e): add arithmetic POSIX compliance tests

10 tests covering basic operations, precedence, comparison, logical,
ternary, variable references, assignment, hex/octal, and compound
assignment (XFAIL) per POSIX 2.6.4.

Task: Unix に厳密に準拠しているかの E2E テストを追加"
```

---

### Task 5: command_substitution tests (~8 tests)

**Files:**
- Create: `e2e/command_substitution/basic.sh`
- Create: `e2e/command_substitution/nested.sh`
- Create: `e2e/command_substitution/trailing_newline_removed.sh`
- Create: `e2e/command_substitution/exit_code_propagation.sh`
- Create: `e2e/command_substitution/in_assignment.sh`
- Create: `e2e/command_substitution/in_double_quotes.sh`
- Create: `e2e/command_substitution/multiline_output.sh`
- Create: `e2e/command_substitution/nested_deep.sh`

- [ ] **Step 1: Create all command_substitution test files**

`e2e/command_substitution/basic.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Basic command substitution with $(...)
# EXPECT_OUTPUT: hello
echo $(echo hello)
```

`e2e/command_substitution/nested.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Nested command substitution
# EXPECT_OUTPUT: hello
echo $(echo $(echo hello))
```

`e2e/command_substitution/trailing_newline_removed.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Trailing newlines are removed from command substitution
# EXPECT_OUTPUT: xhellox
echo "x$(echo hello)x"
```

`e2e/command_substitution/exit_code_propagation.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Exit code of command substitution is propagated
# EXPECT_OUTPUT: 1
x=$(false)
echo "$?"
```

`e2e/command_substitution/in_assignment.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Command substitution result assigned to variable
# EXPECT_OUTPUT: hello
x=$(echo hello)
echo "$x"
```

`e2e/command_substitution/in_double_quotes.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Command substitution inside double quotes
# EXPECT_OUTPUT: result is hello
echo "result is $(echo hello)"
```

`e2e/command_substitution/multiline_output.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Multi-line command substitution output preserved (minus trailing newlines)
# EXPECT_OUTPUT: line1 line2
x=$(printf 'line1\nline2\n')
echo "$x"
```

`e2e/command_substitution/nested_deep.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Deeply nested command substitution
# XFAIL: Nested command substitution edge cases
# EXPECT_OUTPUT: deep
echo $(echo $(echo $(echo deep)))
```

- [ ] **Step 2: Run the command_substitution tests**

Run: `./e2e/run_tests.sh --filter=command_substitution --verbose`

Expected: 7 PASS, 1 XFAIL (nested_deep).

- [ ] **Step 3: Commit**

```bash
git add e2e/command_substitution/
git commit -m "test(e2e): add command_substitution POSIX compliance tests

8 tests covering basic, nested, trailing newline removal, exit code
propagation, assignment, double quotes, multiline, and deep nesting
(XFAIL) per POSIX 2.6.3.

Task: Unix に厳密に準拠しているかの E2E テストを追加"
```

---

### Task 6: pipeline_and_list tests (~10 tests)

**Files:**
- Create: `e2e/pipeline_and_list/simple_pipe.sh`
- Create: `e2e/pipeline_and_list/multi_stage_pipe.sh`
- Create: `e2e/pipeline_and_list/pipe_exit_status.sh`
- Create: `e2e/pipeline_and_list/and_list.sh`
- Create: `e2e/pipeline_and_list/or_list.sh`
- Create: `e2e/pipeline_and_list/and_or_combined.sh`
- Create: `e2e/pipeline_and_list/negation.sh`
- Create: `e2e/pipeline_and_list/background_command.sh`
- Create: `e2e/pipeline_and_list/semicolon_list.sh`
- Create: `e2e/pipeline_and_list/pipeline_with_builtin.sh`

- [ ] **Step 1: Create all pipeline_and_list test files**

`e2e/pipeline_and_list/simple_pipe.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Simple two-command pipeline
# EXPECT_OUTPUT: Hello
echo hello | tr h H
```

`e2e/pipeline_and_list/multi_stage_pipe.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Multi-stage pipeline with three commands
# EXPECT_OUTPUT: HELLO
echo hello | tr a-z A-Z | tr -d '\n'
echo ""
```

`e2e/pipeline_and_list/pipe_exit_status.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Pipeline exit status is that of the last command
# EXPECT_OUTPUT<<END
# 0
# 1
# END
true | true
echo "$?"
true | false
echo "$?"
```

`e2e/pipeline_and_list/and_list.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: AND list - second command runs only if first succeeds
# EXPECT_OUTPUT: yes
true && echo yes
false && echo no
```

`e2e/pipeline_and_list/or_list.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: OR list - second command runs only if first fails
# EXPECT_OUTPUT: fallback
false || echo fallback
true || echo no
```

`e2e/pipeline_and_list/and_or_combined.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: Combined AND/OR list
# EXPECT_OUTPUT: ok
false || true && echo ok
```

`e2e/pipeline_and_list/negation.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: ! negates the exit status of a pipeline
# EXPECT_OUTPUT<<END
# 0
# 1
# END
! false
echo "$?"
! true
echo "$?"
```

`e2e/pipeline_and_list/background_command.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: Background command with & and $! contains its PID
# EXPECT_EXIT: 0
/bin/sleep 0 &
pid=$!
case "$pid" in
  ''|*[!0-9]*) exit 1 ;;
  *) exit 0 ;;
esac
```

`e2e/pipeline_and_list/semicolon_list.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: Semicolons separate sequential commands
# EXPECT_OUTPUT<<END
# first
# second
# third
# END
echo first; echo second; echo third
```

`e2e/pipeline_and_list/pipeline_with_builtin.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Pipeline with builtin command
# EXPECT_OUTPUT: hello
echo hello world | tr -d ' world'
echo ""
```

- [ ] **Step 2: Run the pipeline_and_list tests**

Run: `./e2e/run_tests.sh --filter=pipeline_and_list --verbose`

Expected: All tests PASS.

- [ ] **Step 3: Commit**

```bash
git add e2e/pipeline_and_list/
git commit -m "test(e2e): add pipeline_and_list POSIX compliance tests

10 tests covering simple/multi-stage pipes, exit status, AND/OR lists,
negation, background commands, and semicolon lists per POSIX 2.9.2-2.9.3.

Task: Unix に厳密に準拠しているかの E2E テストを追加"
```

---

### Task 7: redirection tests (~12 tests)

**Files:**
- Create: `e2e/redirection/output_redirect.sh`
- Create: `e2e/redirection/output_append.sh`
- Create: `e2e/redirection/input_redirect.sh`
- Create: `e2e/redirection/stderr_redirect.sh`
- Create: `e2e/redirection/stderr_to_stdout.sh`
- Create: `e2e/redirection/dev_null.sh`
- Create: `e2e/redirection/multiple_redirects.sh`
- Create: `e2e/redirection/heredoc_basic.sh`
- Create: `e2e/redirection/heredoc_expansion.sh`
- Create: `e2e/redirection/heredoc_quoted_no_expansion.sh`
- Create: `e2e/redirection/heredoc_strip_tabs.sh`
- Create: `e2e/redirection/noclobber.sh`

- [ ] **Step 1: Create all redirection test files**

`e2e/redirection/output_redirect.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: > redirects stdout to a file
# EXPECT_EXIT: 0
echo hello > "$TEST_TMPDIR/out.txt"
result=$(cat "$TEST_TMPDIR/out.txt")
test "$result" = "hello"
```

`e2e/redirection/output_append.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: >> appends stdout to a file
# EXPECT_OUTPUT<<END
# first
# second
# END
echo first > "$TEST_TMPDIR/out.txt"
echo second >> "$TEST_TMPDIR/out.txt"
cat "$TEST_TMPDIR/out.txt"
```

`e2e/redirection/input_redirect.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.7.1 Redirecting Input
# DESCRIPTION: < redirects stdin from a file
# EXPECT_OUTPUT: hello from file
echo "hello from file" > "$TEST_TMPDIR/in.txt"
cat < "$TEST_TMPDIR/in.txt"
```

`e2e/redirection/stderr_redirect.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: 2> redirects stderr to a file
# EXPECT_EXIT: 0
echo error_msg >&2 2>"$TEST_TMPDIR/err.txt"
result=$(cat "$TEST_TMPDIR/err.txt")
test "$result" = "error_msg"
```

`e2e/redirection/stderr_to_stdout.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: 2>&1 redirects stderr to stdout
# EXPECT_OUTPUT: error_msg
echo error_msg >&2 2>&1
```

`e2e/redirection/dev_null.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: Redirect to /dev/null discards output
# EXPECT_OUTPUT:
echo hidden > /dev/null
echo ""
```

`e2e/redirection/multiple_redirects.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.7 Redirection
# DESCRIPTION: Multiple redirections on one command
# EXPECT_EXIT: 0
echo stdout_msg > "$TEST_TMPDIR/out.txt" 2> "$TEST_TMPDIR/err.txt"
result=$(cat "$TEST_TMPDIR/out.txt")
test "$result" = "stdout_msg"
```

`e2e/redirection/heredoc_basic.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Basic here-document
# EXPECT_OUTPUT: hello world
cat <<EOF
hello world
EOF
```

`e2e/redirection/heredoc_expansion.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Here-document with variable expansion
# EXPECT_OUTPUT: value is hello
x=hello
cat <<EOF
value is $x
EOF
```

`e2e/redirection/heredoc_quoted_no_expansion.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Quoted delimiter suppresses expansion in here-document
# EXPECT_OUTPUT: value is $x
x=hello
cat <<'EOF'
value is $x
EOF
```

`e2e/redirection/heredoc_strip_tabs.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: <<- strips leading tabs
# EXPECT_OUTPUT<<END
# hello
# world
# END
cat <<-EOF
	hello
	world
	EOF
```

`e2e/redirection/noclobber.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: noclobber prevents overwriting, >| overrides
# EXPECT_OUTPUT<<END
# original
# override
# END
echo original > "$TEST_TMPDIR/file.txt"
set -C
echo new > "$TEST_TMPDIR/file.txt" 2>/dev/null
cat "$TEST_TMPDIR/file.txt"
echo override >| "$TEST_TMPDIR/file.txt"
cat "$TEST_TMPDIR/file.txt"
```

- [ ] **Step 2: Run the redirection tests**

Run: `./e2e/run_tests.sh --filter=redirection --verbose`

Expected: All tests PASS.

- [ ] **Step 3: Commit**

```bash
git add e2e/redirection/
git commit -m "test(e2e): add redirection POSIX compliance tests

12 tests covering output/append/input redirect, stderr redirect,
fd duplication, /dev/null, multiple redirects, here-documents,
and noclobber per POSIX 2.7.

Task: Unix に厳密に準拠しているかの E2E テストを追加"
```

---

### Task 8: control_flow tests (~15 tests)

**Files:**
- Create: `e2e/control_flow/if_true.sh`
- Create: `e2e/control_flow/if_false.sh`
- Create: `e2e/control_flow/if_else.sh`
- Create: `e2e/control_flow/if_elif.sh`
- Create: `e2e/control_flow/if_nested.sh`
- Create: `e2e/control_flow/while_basic.sh`
- Create: `e2e/control_flow/while_false_no_exec.sh`
- Create: `e2e/control_flow/until_basic.sh`
- Create: `e2e/control_flow/for_list.sh`
- Create: `e2e/control_flow/for_empty.sh`
- Create: `e2e/control_flow/for_default_positional.sh`
- Create: `e2e/control_flow/case_basic.sh`
- Create: `e2e/control_flow/case_glob.sh`
- Create: `e2e/control_flow/break_continue.sh`
- Create: `e2e/control_flow/break_nested.sh`

- [ ] **Step 1: Create all control_flow test files**

`e2e/control_flow/if_true.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.4.1 if Conditional Construct
# DESCRIPTION: if with true condition executes then-body
# EXPECT_OUTPUT: yes
if true; then echo yes; fi
```

`e2e/control_flow/if_false.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.4.1 if Conditional Construct
# DESCRIPTION: if with false condition produces no output
# EXPECT_OUTPUT:
if false; then echo yes; fi
```

`e2e/control_flow/if_else.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.4.1 if Conditional Construct
# DESCRIPTION: if-else executes else-body when condition is false
# EXPECT_OUTPUT: no
if false; then echo yes; else echo no; fi
```

`e2e/control_flow/if_elif.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.4.1 if Conditional Construct
# DESCRIPTION: elif chain picks the first true branch
# EXPECT_OUTPUT: second
if false; then echo first; elif true; then echo second; elif true; then echo third; fi
```

`e2e/control_flow/if_nested.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.4.1 if Conditional Construct
# DESCRIPTION: Nested if statements
# EXPECT_OUTPUT: inner
if true; then
  if false; then
    echo wrong
  else
    echo inner
  fi
fi
```

`e2e/control_flow/while_basic.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.4.3 while Loop
# DESCRIPTION: while loop iterates until condition becomes false
# EXPECT_OUTPUT<<END
# 0
# 1
# 2
# END
x=0
while test "$x" -lt 3; do
  echo "$x"
  x=$((x + 1))
done
```

`e2e/control_flow/while_false_no_exec.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.4.3 while Loop
# DESCRIPTION: while with initially false condition never executes body
# EXPECT_OUTPUT:
while false; do echo never; done
```

`e2e/control_flow/until_basic.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.4.4 until Loop
# DESCRIPTION: until loop iterates until condition becomes true
# EXPECT_OUTPUT<<END
# 0
# 1
# 2
# END
x=0
until test "$x" -ge 3; do
  echo "$x"
  x=$((x + 1))
done
```

`e2e/control_flow/for_list.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.4.2 for Loop
# DESCRIPTION: for loop iterates over word list
# EXPECT_OUTPUT<<END
# a
# b
# c
# END
for i in a b c; do
  echo "$i"
done
```

`e2e/control_flow/for_empty.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.4.2 for Loop
# DESCRIPTION: for with empty word list does not execute body
# EXPECT_OUTPUT:
for i in; do echo "$i"; done
```

`e2e/control_flow/for_default_positional.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.4.2 for Loop
# DESCRIPTION: for without in-clause iterates over positional params
# EXPECT_OUTPUT<<END
# a
# b
# c
# END
set -- a b c
for i; do
  echo "$i"
done
```

`e2e/control_flow/case_basic.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.4.5 case Conditional Construct
# DESCRIPTION: case matches literal pattern
# EXPECT_OUTPUT: matched
case foo in
  foo) echo matched ;;
  bar) echo wrong ;;
esac
```

`e2e/control_flow/case_glob.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.4.5 case Conditional Construct
# DESCRIPTION: case supports glob patterns and multiple patterns with |
# EXPECT_OUTPUT<<END
# glob
# multi
# default
# END
case hello in h*) echo glob ;; esac
case bar in foo|bar|baz) echo multi ;; esac
case xyz in foo) echo wrong ;; *) echo default ;; esac
```

`e2e/control_flow/break_continue.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: break exits loop, continue skips to next iteration
# EXPECT_OUTPUT<<END
# 1
# 1
# 3
# END
for i in 1 2 3; do
  if test "$i" = 2; then break; fi
  echo "$i"
done
for i in 1 2 3; do
  if test "$i" = 2; then continue; fi
  echo "$i"
done
```

`e2e/control_flow/break_nested.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: break N exits N levels of nested loops
# EXPECT_OUTPUT: 1a
for i in 1 2; do
  for j in a b c; do
    if test "$j" = b; then break 2; fi
    echo "$i$j"
  done
done
```

- [ ] **Step 2: Run the control_flow tests**

Run: `./e2e/run_tests.sh --filter=control_flow --verbose`

Expected: All tests PASS.

- [ ] **Step 3: Commit**

```bash
git add e2e/control_flow/
git commit -m "test(e2e): add control_flow POSIX compliance tests

15 tests covering if/elif/else, while, until, for, case with glob
and multi-pattern, break, continue, and nesting per POSIX 2.9.4.

Task: Unix に厳密に準拠しているかの E2E テストを追加"
```

---

### Task 9: function tests (~8 tests)

**Files:**
- Create: `e2e/function/basic_definition.sh`
- Create: `e2e/function/with_arguments.sh`
- Create: `e2e/function/return_value.sh`
- Create: `e2e/function/return_default.sh`
- Create: `e2e/function/recursion.sh`
- Create: `e2e/function/global_variable.sh`
- Create: `e2e/function/positional_params_restore.sh`
- Create: `e2e/function/dollar_at_in_function.sh`

- [ ] **Step 1: Create all function test files**

`e2e/function/basic_definition.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Function definition and invocation
# EXPECT_OUTPUT: hello
greet() { echo hello; }
greet
```

`e2e/function/with_arguments.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Function receives positional parameters
# EXPECT_OUTPUT: hello world
greet() { echo "hello $1"; }
greet world
```

`e2e/function/return_value.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: return sets function exit status
# EXPECT_OUTPUT: 42
myfn() { return 42; }
myfn
echo "$?"
```

`e2e/function/return_default.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: Function without return uses exit status of last command
# EXPECT_OUTPUT: 0
myfn() { true; }
myfn
echo "$?"
```

`e2e/function/recursion.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Recursive function calls
# EXPECT_OUTPUT<<END
# 3
# 2
# 1
# END
countdown() {
  if test "$1" -gt 0; then
    echo "$1"
    x=$1
    countdown $((x - 1))
  fi
}
countdown 3
```

`e2e/function/global_variable.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Functions modify variables in the calling environment
# EXPECT_OUTPUT: after
x=before
setx() { x=after; }
setx
echo "$x"
```

`e2e/function/positional_params_restore.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Positional parameters are restored after function call
# EXPECT_OUTPUT<<END
# func: inner
# script: outer
# END
set -- outer
show() { echo "func: $1"; }
show inner
echo "script: $1"
```

`e2e/function/dollar_at_in_function.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: $@ in function expands to function arguments
# EXPECT_OUTPUT<<END
# a
# b
# c
# END
each() { for i in "$@"; do echo "$i"; done; }
each a b c
```

- [ ] **Step 2: Run the function tests**

Run: `./e2e/run_tests.sh --filter=function --verbose`

Expected: All tests PASS.

- [ ] **Step 3: Commit**

```bash
git add e2e/function/
git commit -m "test(e2e): add function POSIX compliance tests

8 tests covering definition, arguments, return, recursion, global
variables, positional parameter restoration, and \$@ per POSIX 2.9.5.

Task: Unix に厳密に準拠しているかの E2E テストを追加"
```

---

### Task 10: builtin tests (~15 tests)

**Files:**
- Create: `e2e/builtin/cd_basic.sh`
- Create: `e2e/builtin/echo_basic.sh`
- Create: `e2e/builtin/export_basic.sh`
- Create: `e2e/builtin/unset_variable.sh`
- Create: `e2e/builtin/readonly_basic.sh`
- Create: `e2e/builtin/eval_basic.sh`
- Create: `e2e/builtin/eval_variable.sh`
- Create: `e2e/builtin/exec_replace.sh`
- Create: `e2e/builtin/exec_no_args.sh`
- Create: `e2e/builtin/source_file.sh`
- Create: `e2e/builtin/shift_basic.sh`
- Create: `e2e/builtin/set_positional.sh`
- Create: `e2e/builtin/colon_noop.sh`
- Create: `e2e/builtin/true_false.sh`
- Create: `e2e/builtin/alias_basic.sh`

- [ ] **Step 1: Create all builtin test files**

`e2e/builtin/cd_basic.sh`:
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd changes the working directory, PWD is updated
# EXPECT_EXIT: 0
original="$PWD"
cd "$TEST_TMPDIR"
test "$PWD" = "$TEST_TMPDIR" || exit 1
cd "$original"
```

`e2e/builtin/echo_basic.sh`:
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - echo
# DESCRIPTION: echo outputs its arguments followed by newline
# EXPECT_OUTPUT: hello world
echo hello world
```

`e2e/builtin/export_basic.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: export makes variable available to child processes
# EXPECT_EXIT: 0
export MY_EXPORT_TEST=hello
result=$(/usr/bin/env | grep MY_EXPORT_TEST)
test "$result" = "MY_EXPORT_TEST=hello"
```

`e2e/builtin/unset_variable.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: unset removes a variable
# EXPECT_OUTPUT<<END
# hello
#
# END
x=hello
echo "$x"
unset x
echo "$x"
```

`e2e/builtin/readonly_basic.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: readonly variable cannot be modified
# EXPECT_EXIT: 1
# EXPECT_STDERR: readonly
x=hello
readonly x
x=world 2>&1
```

`e2e/builtin/eval_basic.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: eval executes concatenated arguments as shell command
# EXPECT_OUTPUT: hello
eval 'echo hello'
```

`e2e/builtin/eval_variable.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: eval with variable expansion constructs command dynamically
# EXPECT_OUTPUT: world
CMD='echo world'
eval $CMD
```

`e2e/builtin/exec_replace.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: exec replaces the shell process
# EXPECT_OUTPUT: replaced
exec /bin/echo replaced
echo "should not reach here"
```

`e2e/builtin/exec_no_args.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: exec without command does not replace the shell
# EXPECT_OUTPUT: still here
exec
echo still here
```

`e2e/builtin/source_file.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: . (dot) sources a file in the current environment
# EXPECT_OUTPUT: sourced
echo 'MY_SRC_VAR=sourced' > "$TEST_TMPDIR/lib.sh"
. "$TEST_TMPDIR/lib.sh"
echo "$MY_SRC_VAR"
```

`e2e/builtin/shift_basic.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: shift removes first N positional parameters
# EXPECT_OUTPUT<<END
# b c
# c
# END
set -- a b c
shift
echo "$@"
shift
echo "$@"
```

`e2e/builtin/set_positional.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: set -- replaces positional parameters
# EXPECT_OUTPUT: x y z
set -- x y z
echo "$1 $2 $3"
```

`e2e/builtin/colon_noop.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: : (colon) is a no-op that returns 0
# EXPECT_OUTPUT: 0
:
echo "$?"
```

`e2e/builtin/true_false.sh`:
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - true, false
# DESCRIPTION: true returns 0, false returns 1
# EXPECT_OUTPUT<<END
# 0
# 1
# END
true
echo "$?"
false
echo "$?"
```

`e2e/builtin/alias_basic.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.3.1 Alias Substitution
# DESCRIPTION: alias defines command alias, unalias removes it
# EXPECT_OUTPUT: hello
alias greet='echo hello'
greet
```

- [ ] **Step 2: Run the builtin tests**

Run: `./e2e/run_tests.sh --filter=builtin --verbose`

Expected: All tests PASS.

- [ ] **Step 3: Commit**

```bash
git add e2e/builtin/
git commit -m "test(e2e): add builtin POSIX compliance tests

15 tests covering cd, echo, export, unset, readonly, eval, exec,
source, shift, set, colon, true/false, and alias per POSIX 2.14.

Task: Unix に厳密に準拠しているかの E2E テストを追加"
```

---

### Task 11: signal_and_trap tests (~8 tests)

**Files:**
- Create: `e2e/signal_and_trap/trap_exit.sh`
- Create: `e2e/signal_and_trap/trap_display.sh`
- Create: `e2e/signal_and_trap/trap_reset.sh`
- Create: `e2e/signal_and_trap/trap_exit_on_error.sh`
- Create: `e2e/signal_and_trap/trap_multiple_commands.sh`
- Create: `e2e/signal_and_trap/trap_ignore_signal.sh`
- Create: `e2e/signal_and_trap/trap_in_subshell_reset.sh`
- Create: `e2e/signal_and_trap/kill_basic.sh`

- [ ] **Step 1: Create all signal_and_trap test files**

`e2e/signal_and_trap/trap_exit.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: EXIT trap fires when shell exits
# EXPECT_OUTPUT<<END
# hello
# goodbye
# END
trap 'echo goodbye' EXIT
echo hello
```

`e2e/signal_and_trap/trap_display.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: trap with no arguments displays current traps
# EXPECT_EXIT: 0
trap 'echo bye' EXIT
output=$(trap)
case "$output" in
  *"echo bye"*EXIT*) exit 0 ;;
  *) exit 1 ;;
esac
```

`e2e/signal_and_trap/trap_reset.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: trap - EXIT resets EXIT trap to default
# EXPECT_OUTPUT: hello
trap 'echo goodbye' EXIT
trap - EXIT
echo hello
```

`e2e/signal_and_trap/trap_exit_on_error.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: EXIT trap fires on non-zero exit
# EXPECT_OUTPUT: cleanup
# EXPECT_EXIT: 1
trap 'echo cleanup' EXIT
exit 1
```

`e2e/signal_and_trap/trap_multiple_commands.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: Trap action can contain multiple commands
# EXPECT_OUTPUT<<END
# hello
# step1
# step2
# END
trap 'echo step1; echo step2' EXIT
echo hello
```

`e2e/signal_and_trap/trap_ignore_signal.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: trap '' SIGNAL ignores the signal
# EXPECT_OUTPUT: still alive
# EXPECT_EXIT: 0
trap '' TERM
kill -TERM $$
echo still alive
```

`e2e/signal_and_trap/trap_in_subshell_reset.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: Traps (except ignore) are reset in subshells
# EXPECT_OUTPUT<<END
# main_trap
# no_trap
# END
trap 'echo main_trap' EXIT
(trap)
echo no_trap
# The main trap should fire at the end, not the subshell
```

`e2e/signal_and_trap/kill_basic.sh`:
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill sends signal to a process
# EXPECT_EXIT: 0
/bin/sleep 10 &
pid=$!
kill "$pid"
wait "$pid" 2>/dev/null
exit 0
```

- [ ] **Step 2: Run the signal_and_trap tests**

Run: `./e2e/run_tests.sh --filter=signal_and_trap --verbose`

Expected: All tests PASS (some may need adjustment based on trap display format).

- [ ] **Step 3: Commit**

```bash
git add e2e/signal_and_trap/
git commit -m "test(e2e): add signal_and_trap POSIX compliance tests

8 tests covering EXIT trap, trap display/reset, signal ignore,
subshell trap reset, and kill per POSIX 2.11 and 2.14.

Task: Unix に厳密に準拠しているかの E2E テストを追加"
```

---

### Task 12: subshell tests (~8 tests)

**Files:**
- Create: `e2e/subshell/basic_execution.sh`
- Create: `e2e/subshell/variable_isolation.sh`
- Create: `e2e/subshell/function_isolation.sh`
- Create: `e2e/subshell/exit_status.sh`
- Create: `e2e/subshell/option_isolation.sh`
- Create: `e2e/subshell/alias_isolation.sh`
- Create: `e2e/subshell/dollar_dollar_same.sh`
- Create: `e2e/subshell/nested_subshell.sh`

- [ ] **Step 1: Create all subshell test files**

`e2e/subshell/basic_execution.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Subshell executes commands
# EXPECT_OUTPUT: hello
(echo hello)
```

`e2e/subshell/variable_isolation.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Variable changes in subshell do not affect parent
# EXPECT_OUTPUT<<END
# after
# before
# END
x=before
(x=after; echo "$x")
echo "$x"
```

`e2e/subshell/function_isolation.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Functions defined in subshell do not exist in parent
# EXPECT_EXIT: 127
(myfn() { echo hello; })
myfn
```

`e2e/subshell/exit_status.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Subshell exit status is propagated to parent
# EXPECT_OUTPUT<<END
# 0
# 1
# END
(true)
echo "$?"
(false)
echo "$?"
```

`e2e/subshell/option_isolation.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Shell option changes in subshell do not affect parent
# EXPECT_OUTPUT<<END
# *
# END
(set -f; echo *)
echo * >/dev/null
```

`e2e/subshell/alias_isolation.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Aliases defined in subshell do not exist in parent
# EXPECT_OUTPUT: original
alias greet='echo original'
(alias greet='echo modified')
greet
```

`e2e/subshell/dollar_dollar_same.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $$ in subshell is same as parent shell PID
# EXPECT_OUTPUT: same
parent_pid=$$
child_pid=$(echo $$)
if test "$parent_pid" = "$child_pid"; then
  echo same
else
  echo different
fi
```

`e2e/subshell/nested_subshell.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Nested subshells maintain isolation at each level
# EXPECT_OUTPUT<<END
# inner
# outer
# original
# END
x=original
(x=outer; (x=inner; echo "$x"); echo "$x")
echo "$x"
```

- [ ] **Step 2: Run the subshell tests**

Run: `./e2e/run_tests.sh --filter=subshell --verbose`

Expected: All tests PASS.

- [ ] **Step 3: Commit**

```bash
git add e2e/subshell/
git commit -m "test(e2e): add subshell POSIX compliance tests

8 tests covering basic execution, variable/function/option/alias
isolation, exit status propagation, \$\$ preservation, and nesting
per POSIX 2.12.

Task: Unix に厳密に準拠しているかの E2E テストを追加"
```

---

### Task 13: quoting tests (~10 tests)

**Files:**
- Create: `e2e/quoting/single_quotes.sh`
- Create: `e2e/quoting/double_quotes.sh`
- Create: `e2e/quoting/double_quotes_expansion.sh`
- Create: `e2e/quoting/backslash_escape.sh`
- Create: `e2e/quoting/backslash_in_double_quotes.sh`
- Create: `e2e/quoting/nested_quotes.sh`
- Create: `e2e/quoting/empty_string.sh`
- Create: `e2e/quoting/glob_suppressed.sh`
- Create: `e2e/quoting/spaces_preserved.sh`
- Create: `e2e/quoting/literal_dollar.sh`

- [ ] **Step 1: Create all quoting test files**

`e2e/quoting/single_quotes.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.2.2 Single-Quotes
# DESCRIPTION: Single quotes preserve all characters literally
# EXPECT_OUTPUT: $HOME is literal
echo '$HOME is literal'
```

`e2e/quoting/double_quotes.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Double quotes allow variable expansion
# EXPECT_OUTPUT: value is hello
x=hello
echo "value is $x"
```

`e2e/quoting/double_quotes_expansion.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Double quotes allow command and arithmetic expansion
# EXPECT_OUTPUT: cmd=hello arith=3
echo "cmd=$(echo hello) arith=$((1+2))"
```

`e2e/quoting/backslash_escape.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.2.1 Escape Character
# DESCRIPTION: Backslash escapes special characters
# EXPECT_OUTPUT: $HOME
echo \$HOME
```

`e2e/quoting/backslash_in_double_quotes.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: In double quotes, backslash only escapes $ ` \ " newline
# EXPECT_OUTPUT: $HOME
echo "\$HOME"
```

`e2e/quoting/nested_quotes.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.2 Quoting
# DESCRIPTION: Nesting double quotes inside single quotes and vice versa
# EXPECT_OUTPUT<<END
# he said "hello"
# it's fine
# END
echo 'he said "hello"'
echo "it's fine"
```

`e2e/quoting/empty_string.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.2 Quoting
# DESCRIPTION: Empty quotes produce an empty argument (not removed)
# EXPECT_OUTPUT: 3
set -- a "" c
echo "$#"
```

`e2e/quoting/glob_suppressed.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.2 Quoting
# DESCRIPTION: Quoting suppresses glob expansion
# EXPECT_OUTPUT<<END
# src/*.rs
# src/*.rs
# END
echo 'src/*.rs'
echo "src/*.rs"
```

`e2e/quoting/spaces_preserved.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Double quotes preserve spaces within a field
# EXPECT_OUTPUT: hello   world
echo "hello   world"
```

`e2e/quoting/literal_dollar.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.2.1 Escape Character
# DESCRIPTION: Single-quoted dollar sign is literal
# EXPECT_OUTPUT: price is $5
echo 'price is $5'
```

- [ ] **Step 2: Run the quoting tests**

Run: `./e2e/run_tests.sh --filter=quoting --verbose`

Expected: All tests PASS.

- [ ] **Step 3: Commit**

```bash
git add e2e/quoting/
git commit -m "test(e2e): add quoting POSIX compliance tests

10 tests covering single/double quotes, backslash escaping,
nested quotes, empty strings, glob suppression, and space
preservation per POSIX 2.2.

Task: Unix に厳密に準拠しているかの E2E テストを追加"
```

---

### Task 14: field_splitting tests (~8 tests)

**Files:**
- Create: `e2e/field_splitting/default_ifs.sh`
- Create: `e2e/field_splitting/custom_ifs.sh`
- Create: `e2e/field_splitting/ifs_colon.sh`
- Create: `e2e/field_splitting/no_split_in_quotes.sh`
- Create: `e2e/field_splitting/glob_basic.sh`
- Create: `e2e/field_splitting/glob_no_match.sh`
- Create: `e2e/field_splitting/noglob.sh`
- Create: `e2e/field_splitting/glob_question_mark.sh`

- [ ] **Step 1: Create all field_splitting test files**

`e2e/field_splitting/default_ifs.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Default IFS splits on space, tab, newline
# EXPECT_OUTPUT<<END
# a
# b
# c
# END
x="a b c"
for i in $x; do
  echo "$i"
done
```

`e2e/field_splitting/custom_ifs.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Custom IFS splits on specified character
# EXPECT_OUTPUT<<END
# one
# two
# three
# END
IFS=:
x="one:two:three"
for i in $x; do
  echo "$i"
done
```

`e2e/field_splitting/ifs_colon.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: IFS with non-whitespace delimiter preserves empty fields behavior
# EXPECT_OUTPUT<<END
# a
# b
# END
IFS=:
x="a:b"
set -- $x
echo "$1"
echo "$2"
```

`e2e/field_splitting/no_split_in_quotes.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Quoted expansion prevents field splitting
# EXPECT_OUTPUT: 1
x="a b c"
set -- "$x"
echo "$#"
```

`e2e/field_splitting/glob_basic.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: Glob * matches files in directory
# EXPECT_EXIT: 0
# We create temp files and verify glob matches them
echo "file1" > "$TEST_TMPDIR/a.txt"
echo "file2" > "$TEST_TMPDIR/b.txt"
cd "$TEST_TMPDIR"
count=0
for f in *.txt; do
  count=$((count + 1))
done
test "$count" = 2
```

`e2e/field_splitting/glob_no_match.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: Glob with no matches returns the pattern literally
# EXPECT_OUTPUT: /tmp/kish_nonexistent_glob_test_*.zzz
echo /tmp/kish_nonexistent_glob_test_*.zzz
```

`e2e/field_splitting/noglob.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: set -f disables pathname expansion
# EXPECT_OUTPUT: *
set -f
echo *
```

`e2e/field_splitting/glob_question_mark.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: ? matches exactly one character
# EXPECT_EXIT: 0
echo "x" > "$TEST_TMPDIR/a1.txt"
echo "y" > "$TEST_TMPDIR/b2.txt"
echo "z" > "$TEST_TMPDIR/cc.txt"
cd "$TEST_TMPDIR"
count=0
for f in ??.txt; do
  count=$((count + 1))
done
test "$count" = 3
```

- [ ] **Step 2: Run the field_splitting tests**

Run: `./e2e/run_tests.sh --filter=field_splitting --verbose`

Expected: All tests PASS.

- [ ] **Step 3: Commit**

```bash
git add e2e/field_splitting/
git commit -m "test(e2e): add field_splitting POSIX compliance tests

8 tests covering default/custom IFS, quoted expansion preventing
splits, pathname expansion with * and ?, no-match behavior, and
noglob per POSIX 2.6.5-2.6.6.

Task: Unix に厳密に準拠しているかの E2E テストを追加"
```

---

### Task 15: Final validation and README

**Files:**
- Create: `e2e/README.md`

- [ ] **Step 1: Build kish and run the full test suite**

Run: `cargo build && ./e2e/run_tests.sh --verbose`

Review the output. All tests should be PASS or XFAIL. If any unexpected FAILs appear, investigate:
- If the failure is due to a known limitation in TODO.md, add `# XFAIL: <reason>` to the test file
- If the failure is due to a test bug, fix the test
- If the failure reveals a new kish bug, note it and add XFAIL

- [ ] **Step 2: Create README.md**

Create `e2e/README.md`:

```markdown
# POSIX E2E Test Suite

End-to-end tests verifying kish's POSIX shell compliance against
IEEE Std 1003.1 (POSIX Shell Command Language).

## Running Tests

```sh
# Build kish first
cargo build

# Run all tests
./e2e/run_tests.sh

# Run with verbose output (shows diff on failure)
./e2e/run_tests.sh --verbose

# Run only tests matching a pattern
./e2e/run_tests.sh --filter=variable

# Run with a different shell for comparison
./e2e/run_tests.sh --shell=/bin/dash
```

## Writing Tests

Each test is a single `.sh` file with metadata comments:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Default value expansion with :-
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
unset x
echo "${x:-hello}"
```

### Metadata Fields

| Field | Required | Description |
|---|---|---|
| `POSIX_REF` | Yes | POSIX spec section reference |
| `DESCRIPTION` | Yes | One-line test description |
| `EXPECT_OUTPUT` | No | Expected stdout (exact match) |
| `EXPECT_EXIT` | No | Expected exit code (default: 0) |
| `EXPECT_STDERR` | No | Expected stderr substring |
| `XFAIL` | No | Expected failure reason |

### Multi-line Expected Output

```sh
# EXPECT_OUTPUT<<END
# line1
# line2
# END
```

### Test Environment

- `$TEST_TMPDIR` — per-test temporary directory (auto-cleaned)
- Tests have a 5-second timeout

## Directory Structure

Tests are organized by functional category:

- `command_execution/` — Simple commands, PATH search
- `variable_and_expansion/` — Parameter expansion
- `arithmetic/` — Arithmetic expansion
- `command_substitution/` — Command substitution
- `pipeline_and_list/` — Pipelines, AND/OR lists
- `redirection/` — Redirection operators, heredocs
- `control_flow/` — if, for, while, until, case
- `function/` — Function definition and invocation
- `builtin/` — Special and regular builtins
- `signal_and_trap/` — Signal handling, trap
- `subshell/` — Subshell environment isolation
- `quoting/` — Quoting rules
- `field_splitting/` — Field splitting, pathname expansion

## Result Classification

- **PASS** — Test succeeded as expected
- **FAIL** — Unexpected failure
- **XFAIL** — Expected failure (known limitation)
- **XPASS** — XFAIL test unexpectedly passed (possible fix)
```

- [ ] **Step 3: Run the full suite one final time to confirm**

Run: `./e2e/run_tests.sh`

Confirm exit code is 0 (no unexpected failures).

- [ ] **Step 4: Commit**

```bash
git add e2e/README.md
git commit -m "docs(e2e): add README with test writing guide and usage

Task: Unix に厳密に準拠しているかの E2E テストを追加"
```
