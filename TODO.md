# TODO

## Phase 1: Known Limitations

- [ ] Nested command substitution edge cases: `$(echo $(echo ')'))` may fail due to balanced-paren approach in lexer (`src/lexer/mod.rs` `read_balanced_parens`)
- [ ] `Lexer.pending_heredocs` is `pub` — consider accessor methods for better encapsulation

## Phase 2: Known Limitations

- [ ] `export` output format missing quotes — should be `export FOO="bar"` not `export FOO=bar` (`src/builtin/mod.rs`)
- [ ] `echo -n` flag not handled — POSIX strict doesn't require it but practical shells need it (`src/builtin/mod.rs`)
- [ ] `cd -` (change to OLDPWD) not implemented (`src/builtin/mod.rs`)
- [ ] `VarStore` has no scope mechanism — needed for function execution in Phase 5 (`src/env/vars.rs`)
- [ ] `TempDir` ID uses nanosecond timestamp — risk of collision under heavy parallel testing (`tests/helpers/mod.rs`)

## Phase 3: Known Limitations

- [ ] Unquoted `$@` should produce separate fields per positional param, currently joins with space (`src/expand/mod.rs`)
- [ ] Arithmetic compound assignment operators (`+=`, `-=`, `*=`, etc.) not implemented (`src/expand/arith.rs`)
- [ ] `${parameter:?word}` should exit non-interactive shell, currently only prints error (`src/expand/param.rs`)
- [ ] Deeply nested command substitution edge cases untested

## Phase 4: Known Limitations

- [ ] Heredoc + pipeline not working — `cat <<EOF | tr a-z A-Z` produces empty output due to redirect timing in child process (`src/exec/pipeline.rs`)

## Phase 5: Known Limitations

- [ ] `$N` (positional params) inside `$((...))` arithmetic not supported — use temp variable workaround: `x=$1; echo $((x - 1))` (`src/expand/arith.rs`)
- [ ] Function-scoped assignments with prefix syntax (`VAR=val func`) not implemented — assignments only apply to external commands

## Phase 6: Known Limitations

- [ ] `-m` (monitor) flag is settable but job control is not implemented — deferred to future phase
- [ ] `-b` (notify) flag is settable but has no effect — depends on `-m`
- [ ] `ignoreeof` is settable but has no effect — interactive mode feature
- [ ] Alias expansion in non-interactive mode requires incremental parsing — complex scripts with nested structures may have edge cases

## Phase 7: Known Limitations

- [ ] `wait` signal interruption — if multiple signals arrive simultaneously during `wait`, only the first is used for the return status
- [ ] `kill 0` in pipeline subshell sends to pipeline's process group, not the shell's
- [ ] `-m` (monitor) flag is settable but job control is not implemented — deferred to future phase
- [ ] `-b` (notify) flag is settable but has no effect — depends on `-m`
- [ ] `ignoreeof` is settable but has no effect — interactive mode feature

## Phase 8: Known Limitations

- [ ] `umask` builtin not implemented — `test_umask_inheritance` is ignored; umask inheritance cannot be verified (`tests/subshell.rs`)
- [ ] `exec N>file` fd persistence not implemented — `exec` builtin restores redirects, so `test_fd_inheritance` is ignored (`tests/subshell.rs`, `src/builtin/special.rs`)
- [ ] `test_umask_isolation` may pass incidentally due to fork isolation, not because umask is correctly set/read (`tests/subshell.rs`)
- [ ] `return` outside function in subshell error test not implemented — POSIX requires error, untested (`tests/subshell.rs`)

## Discovered via E2E Tests

- [ ] Pattern matcher panics on negated character class `[!0-9]` — slice index out of bounds at `src/expand/pattern.rs:37`
- [ ] `$(trap)` does not capture trap output in command substitution — trap writes to original stdout, not captured
- [ ] `set -- a "" c` drops empty string arguments — `$#` returns 2 instead of 3
- [ ] `${parameter:?word}` with spaces in word causes parse error instead of printing custom error message

## E2E Test Runner Improvements

- [ ] Timeout handler should use `exec` to avoid orphan child processes when kish forks (`e2e/run_tests.sh`)
- [ ] `normalize_trailing()` is a no-op — `$()` already strips trailing newlines; update comment or remove (`e2e/run_tests.sh`)
- [ ] Add warning when `EXPECT_OUTPUT<<DELIM` heredoc is never closed by matching delimiter (`e2e/run_tests.sh`)
- [ ] Add `timedout` counter to summary output, separate from `failed` count (`e2e/run_tests.sh`)

## Future: E2E Test Expansion

- [ ] Deep edge-case tests for each feature (e.g., nested expansions, unusual quoting combinations)
- [ ] POSIX corner cases (e.g., special IFS values, empty IFS, IFS with whitespace/non-whitespace mix)
