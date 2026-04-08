# POSIX E2E Test Suite Design

## Overview

Add an end-to-end test suite that verifies kish's POSIX shell compliance by testing all implemented features (Phase 1–8) against the IEEE Std 1003.1 (POSIX Shell Command Language) specification.

## Goals

- Verify that kish correctly implements POSIX shell behavior for all Phase 1–8 features
- Provide clear traceability from each test case to the relevant POSIX specification section
- Track known limitations with XFAIL markers so fixes are automatically detected
- Enable future comparison with reference shells (dash, bash, etc.)

## Non-Goals

- Interactive mode testing (not yet implemented)
- Performance benchmarking
- Testing non-POSIX extensions

## Test Case Format

Each test case is a single `.sh` file with metadata in header comments:

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
| `POSIX_REF` | Yes | POSIX spec section number and name |
| `DESCRIPTION` | Yes | One-line test description |
| `EXPECT_OUTPUT` | No | Expected stdout (exact match, trailing newline normalized). Omit to skip output check |
| `EXPECT_EXIT` | No | Expected exit code (default: `0`) |
| `EXPECT_STDERR` | No | Expected stderr substring (partial match) |
| `XFAIL` | No | Reason for expected failure (known limitation) |

### Multi-line Expected Output

```sh
# EXPECT_OUTPUT<<END
# line1
# line2
# END
```

## Directory Structure

```
e2e/
  run_tests.sh              # Test runner (POSIX sh)
  README.md                 # How to write and run tests
  command_execution/         # Simple commands, PATH search, execution
  variable_and_expansion/    # Parameter expansion, variable assignment
  arithmetic/                # Arithmetic expansion
  command_substitution/      # Command substitution
  pipeline_and_list/         # Pipelines, AND/OR lists
  redirection/               # Redirection operators
  control_flow/              # if, for, while, until, case
  function/                  # Function definition and invocation
  builtin/                   # Special and regular builtins
  signal_and_trap/           # Signal handling, trap
  subshell/                  # Subshell environment isolation
  quoting/                   # Quoting rules
  field_splitting/           # Field splitting, pathname expansion
```

File naming convention: snake_case describing the test target (e.g., `default_value.sh`, `simple_pipe.sh`).

## Test Runner (`run_tests.sh`)

### Implementation Language

POSIX sh — zero external dependencies, natural fit for a POSIX shell project.

### Basic Operation

1. Recursively discover all `.sh` files under `e2e/` (excluding `run_tests.sh` itself)
2. Parse metadata comments from each file
3. Execute with kish, capture stdout, stderr, and exit code
4. Compare against expected values
5. Display summary

### Command-Line Options

```sh
./e2e/run_tests.sh                         # Run all tests
./e2e/run_tests.sh --shell=/path/to/dash   # Run with a different shell
./e2e/run_tests.sh --filter=variable       # Run tests matching path pattern
./e2e/run_tests.sh --verbose               # Show diff details on failure
```

### Result Classification

| Status | Meaning |
|---|---|
| `PASS` | Test succeeded as expected |
| `FAIL` | Unexpected failure |
| `XFAIL` | Expected failure (XFAIL marker present, test failed) |
| `XPASS` | Unexpected pass (XFAIL marker present, but test succeeded — possible fix) |
| `TIMEOUT` | Test exceeded timeout (treated as FAIL) |

### Output Format

```
[PASS]    variable_and_expansion/simple_assignment.sh
[FAIL]    redirection/heredoc_pipeline.sh
[XFAIL]   command_substitution/nested_deep.sh  (known: nested substitution)
[XPASS]   arithmetic/compound_assign.sh  (unexpectedly passed!)

Results: 150 passed, 2 failed, 5 xfail, 1 xpass, 0 skipped
```

### Exit Code

- `0`: Zero FAIL or TIMEOUT results (XFAIL is acceptable)
- `1`: One or more FAIL or TIMEOUT results

## Error Handling and Robustness

### Timeout

- 5-second timeout per test to prevent hangs from infinite loops or input waits
- Timed-out tests reported as `[TIMEOUT]` and counted as failures

### Temporary Files

- Runner creates a temporary directory per test, passed via `$TEST_TMPDIR` environment variable
- Automatic cleanup after each test

### Output Comparison

- `EXPECT_OUTPUT`: Exact match (trailing newline normalized)
- `EXPECT_STDERR`: Substring match (error message format is implementation-dependent)
- `--verbose` shows diff on comparison failure

## Test Categories and Coverage

### command_execution (~10 tests)

Simple command execution, PATH search, nonexistent command, empty command, semicolon-separated commands.

### variable_and_expansion (~20 tests)

Simple assignment, reference, `${var:-default}`, `${var:=assign}`, `${var:+alternate}`, `${var:?error}`, `${#var}`, `${var%pat}`, `${var#pat}`, `${var%%pat}`, `${var##pat}`, positional parameters `$1`–`$9`, special variables `$?`, `$#`, `$$`, `$!`, `$@`, `$*`.

### arithmetic (~10 tests)

Basic operations, comparison, logical operators, ternary operator, variable references, nesting.

### command_substitution (~8 tests)

Basic `$(...)`, nesting, trailing newline removal, exit code propagation.

### pipeline_and_list (~10 tests)

Simple pipe, multi-stage pipe, `&&`, `||`, `!` negation, combinations.

### redirection (~12 tests)

`>`, `>>`, `<`, `2>`, `2>&1`, `>&`, multiple redirections, heredoc, `/dev/null`.

### control_flow (~15 tests)

`if/elif/else`, `for`, `while`, `until`, `case`, `break`, `continue`, nesting.

### function (~8 tests)

Definition, arguments, return, recursion, variable behavior in functions.

### builtin (~15 tests)

`cd`, `echo`, `export`, `unset`, `readonly`, `eval`, `exec`, `.` (source), `shift`, `set`, `true`/`false`, `:`, `exit`, `alias`/`unalias`.

### signal_and_trap (~8 tests)

Trap set/unset, EXIT trap, signal delivery and capture.

### subshell (~8 tests)

Variable isolation, function isolation, trap reset, option isolation.

### quoting (~10 tests)

Single quotes, double quotes, backslash escape, expansion inside quotes.

### field_splitting (~8 tests)

IFS-based field splitting, pathname expansion (globbing), `noglob`.

**Total: ~130–140 test cases**, with XFAIL markers for known limitations documented in TODO.md.
