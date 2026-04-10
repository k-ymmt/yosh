# TODO

## Phase 3: Known Limitations

- [ ] Unquoted `$@` should produce separate fields per positional param, currently joins with space (`src/expand/mod.rs`)
- [ ] Deeply nested command substitution edge cases untested

## Phase 5: Known Limitations

- [ ] `$N` (positional params) inside `$((...))` arithmetic not supported — use temp variable workaround: `x=$1; echo $((x - 1))` (`src/expand/arith.rs`)
- [ ] Function-scoped assignments with prefix syntax (`VAR=val func`) not implemented — assignments only apply to external commands

## Phase 6: Known Limitations

- [ ] `-m` (monitor) flag is settable but job control is not implemented — deferred to future phase
- [ ] `-b` (notify) flag is settable but has no effect — depends on `-m`
- [ ] `ignoreeof` is settable but has no effect — interactive mode feature

## Phase 7: Known Limitations

- [ ] `wait` signal interruption — if multiple signals arrive simultaneously during `wait`, only the first is used for the return status
- [ ] `kill 0` in pipeline subshell sends to pipeline's process group, not the shell's
- [ ] `test_kill_dash_s` is flaky — intermittently returns 137 (SIGKILL) instead of expected 130 (SIGINT), likely a timing issue (`tests/signals.rs`)
- [ ] `-m` (monitor) flag is settable but job control is not implemented — deferred to future phase
- [ ] `-b` (notify) flag is settable but has no effect — depends on `-m`
- [ ] `ignoreeof` is settable but has no effect — interactive mode feature

## Phase 8: Known Limitations

- [ ] `umask` builtin not implemented — `test_umask_inheritance` is ignored; umask inheritance cannot be verified (`tests/subshell.rs`)
- [ ] `exec N>file` fd persistence not implemented — `exec` builtin restores redirects, so `test_fd_inheritance` is ignored (`tests/subshell.rs`, `src/builtin/special.rs`)
- [ ] `test_umask_isolation` may pass incidentally due to fork isolation, not because umask is correctly set/read (`tests/subshell.rs`)
- [ ] `return` outside function in subshell error test not implemented — POSIX requires error, untested (`tests/subshell.rs`)

## Future: Code Quality Improvements

- [ ] `cd` overwrites OLDPWD before `set_current_dir` — if chdir fails, OLDPWD is still modified (`src/builtin/mod.rs`)
- [ ] `exec_function_call` lacks panic safety — `push_scope` without Drop guard means `pop_scope` is skipped on panic (`src/exec/mod.rs`)
- [ ] `VarStore::vars_iter()` rebuilds HashMap on every call — consider returning `Vec` or caching for performance (`src/env/vars.rs`)

## Future: E2E Test Expansion

- [ ] Builtin test POSIX_REF values could use more specific section numbers (e.g., `2.14.3` instead of `2.14 Special Built-In Utilities`)
- [ ] `fd_close.sh` test only checks exit code, not actual fd close effect
- [ ] `redirection/heredoc_pipeline.sh` has stale XFAIL marker — test now passes, remove XFAIL to eliminate XPASS in summary
- [ ] `e2e/command_execution/echo_simple.sh` has `755` permissions — should be `644` to match project convention
