# E2E Discovered Bugs Fix Design

Fix all 10 bugs listed under "Discovered via E2E Tests" in TODO.md. Organized into 3 categories: expand, builtin, and signal.

## Category 1: Expand Bugs (7 items)

### 1-1. `$-` Special Parameter (src/expand/param.rs, src/env/mod.rs, src/main.rs)

**Problem**: `SpecialParam::Dash` calls `env.options.to_flag_string()` which is implemented, but the `-c` invocation flag is not tracked in `ShellOptions`. POSIX requires `$-` to include flags from invocation (e.g., `c` when invoked with `-c`).

**Fix**: Add a field to `ShellOptions` to track the `-c` invocation mode. Set it in `main.rs` when `-c` is parsed. Include `c` in `to_flag_string()` output.

**Test**: `e2e/variable_and_expansion/special_var_hyphen.sh` (XFAIL) — `test -n "$-"` should succeed.

### 1-2. Double-Quote Backslash Duplication (src/lexer/mod.rs)

**Problem**: In `read_backslash_in_double_quote()`, the `_` (non-special character) arm returns `format!("\\{}", ch as char)` but does **not** call `self.advance()` to consume `ch`. The caller then processes `ch` again, producing `\aa` instead of `\a`.

**Fix**: Add `self.advance()` in the `_` arm before returning, so the non-special character is consumed.

**Test**: `e2e/quoting/backslash_non_special_in_dquotes.sh` (XFAIL) — `echo "\a"` should output `\a`.

### 1-3. IFS Consecutive Non-Whitespace Delimiters (src/expand/field_split.rs)

**Problem**: With `IFS=:`, splitting `"a::b"` produces 2 fields instead of 3. POSIX requires consecutive non-whitespace IFS delimiters to produce empty fields between them.

**Fix**: Debug the state machine in `field_split()`. The `AfterNws` state should emit an empty field when encountering another non-whitespace delimiter. Trace the actual state transitions for `a::b` to find where the empty field is lost.

**Test**: `e2e/field_splitting/ifs_non_whitespace_consecutive.sh` (XFAIL) — `$#` should be 3 after `set -- $x`.

### 1-4. Glob Dot Files — Test Fix Only (e2e/field_splitting/glob_dot_files.sh)

**Problem**: kish's glob correctly excludes dot files (verified manually). The E2E test fails because it `cd`s into `$TEST_TMPDIR` which also contains the test runner's temporary files (`_stdout`, `_stderr`, `_exit`), inflating the file count.

**Fix**: Modify the test to create and `cd` into a subdirectory of `$TEST_TMPDIR`. Remove the XFAIL marker.

**Test**: `e2e/field_splitting/glob_dot_files.sh` — should become a normal PASS.

### 1-5. Division by Zero Exit Code (src/expand/arith.rs)

**Problem**: `evaluate()` returns `"0"` on arithmetic error but does not set `env.last_exit_status = 1`. The exit code remains 0.

**Fix**: Change `evaluate()` to return an error indicator (e.g., `Result<String, String>` or a struct with value + success flag). The caller in `expand_arith()` sets `env.last_exit_status = 1` on error.

**Test**: `e2e/arithmetic/division_by_zero.sh` (XFAIL) — exit code should be 1.

### 1-6. Modulo by Zero Exit Code (src/expand/arith.rs)

**Problem**: Same root cause as 1-5.

**Fix**: Covered by the same `evaluate()` refactor in 1-5.

**Test**: `e2e/arithmetic/modulo_by_zero.sh` (XFAIL) — exit code should be 1.

### 1-7. Comma Operator in Arithmetic (src/expand/arith.rs)

**Problem**: The arithmetic parser has no comma operator. `$((a=1, b=2, a+b))` only evaluates the first expression.

**Fix**: Add a `comma()` method between `expr()` and `ternary()` in the operator precedence chain. Comma has the lowest precedence; it evaluates left-to-right and returns the value of the rightmost expression. `expr()` calls `comma()` instead of `ternary()`.

**Test**: `e2e/arithmetic/comma_operator.sh` (XFAIL) — `$((a=1, b=2, a+b))` should output `3`.

## Category 2: Builtin Bug (1 item)

### 2-1. `export -p` Output (src/builtin/special.rs)

**Problem**: `builtin_export()` only prints exported variables when `args.is_empty()`. When `-p` is passed, it's treated as a variable name rather than a flag.

**Fix**: Check if `args[0] == "-p"` and handle it the same as the empty-args case. Also quote the value in output: `export NAME="VALUE"` (POSIX format for re-input).

**Test**: `e2e/builtin/export_format.sh` (XFAIL) — output should contain `export MY_TEST_EXPORT_VAR`.

## Category 3: Signal Bugs (2 items)

### 3-1. `trap '' SIGNAL` Does Not Ignore Signals (src/signal.rs, src/builtin/special.rs)

**Problem**: `trap '' USR1` stores `TrapAction::Ignore` in `TrapStore` but does not change the OS-level signal disposition. If USR1 is not in `HANDLED_SIGNALS`, the default action (terminate) executes before the shell can check trap state.

**Fix**: When `TrapStore::set_trap()` sets `TrapAction::Ignore`, also call `signal::ignore_signal(sig)` to set `SIG_IGN` at the OS level. When `TrapAction::Command` is set, register the self-pipe handler via `signal::register_handler(sig)` (new function). When a trap is removed (reset to default), call `signal::default_signal(sig)`.

**Test**: `e2e/signal_and_trap/trap_ignore_empty.sh` (XFAIL) — process should survive USR1 and print "survived".

### 3-2. Subshell Does Not Inherit Ignored Signal Disposition (src/signal.rs, src/exec/mod.rs)

**Problem**: `reset_child_signals()` resets ALL handled signals to `SIG_DFL`, including those explicitly ignored by `trap ''`. POSIX requires ignored signals to remain ignored in subshells.

**Fix**: Change `reset_child_signals()` to accept a set of signal numbers that should remain ignored. In `exec_subshell()`, pass the set of `TrapAction::Ignore` signals from `TrapStore`. Those signals keep `SIG_IGN`; all others reset to `SIG_DFL`.

**Test**: `e2e/subshell/subshell_trap_inherit_ignore.sh` (XFAIL) — subshell should survive USR1 and print "survived".

## Implementation Order

1. **Expand category** (1-1 through 1-7): Independent of each other, can be done in any order within the category.
2. **Builtin category** (2-1): Independent of other categories.
3. **Signal category** (3-1, then 3-2): 3-2 depends on 3-1's signal infrastructure changes.

## Verification

After each category, run:
- `cargo test` — unit/integration tests
- `bash e2e/run_tests.sh --shell=./target/release/kish` — E2E tests, verify XFAIL becomes PASS

After all fixes, update TODO.md by removing completed items from "Discovered via E2E Tests".
