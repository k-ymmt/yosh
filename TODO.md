# TODO

## Phase 6: Known Limitations

- [ ] `-m` (monitor) flag is settable but job control is not implemented — deferred to future phase
- [ ] `-b` (notify) flag is settable but has no effect — depends on `-m`

## Phase 7: Known Limitations

- [ ] `wait` signal interruption — if multiple signals arrive simultaneously during `wait`, only the first is used for the return status
- [ ] `kill 0` in pipeline subshell sends to pipeline's process group, not the shell's
- [ ] `-m` (monitor) flag is settable but job control is not implemented — deferred to future phase
- [ ] `-b` (notify) flag is settable but has no effect — depends on `-m`

## Phase 8: Known Limitations

- [ ] `umask` builtin not implemented — `test_umask_inheritance` is ignored; umask inheritance cannot be verified (`tests/subshell.rs`)
- [ ] `exec N>file` fd persistence not implemented — `exec` builtin restores redirects, so `test_fd_inheritance` is ignored (`tests/subshell.rs`, `src/builtin/special.rs`)
- [ ] `test_umask_isolation` may pass incidentally due to fork isolation, not because umask is correctly set/read (`tests/subshell.rs`)
- [ ] `return` outside function in subshell error test not implemented — POSIX requires error, untested (`tests/subshell.rs`)

## Future: Interactive Mode Enhancements

- [ ] History — ↑/↓ for history navigation, `~/.kish_history` persistence, Ctrl+R reverse search
- [ ] Tab completion — file path and command name completion
- [ ] Multiline editing — visual multiline editing with cursor movement across lines
- [ ] Emacs keybindings — Ctrl+K (kill to end), Ctrl+U (kill to start), Ctrl+W (kill word), Ctrl+Y (yank)
- [ ] `~/.kishrc` startup file — ENV variable support for interactive initialization
- [ ] `set -o interactive` flag management
- [ ] Interactive-specific trap behavior — SIGTERM/SIGQUIT ignored by default
- [ ] Job control — `-m` flag, fg/bg/jobs builtins, process group management, SIGTSTP/SIGCONT
- [ ] Prompt width — accurate column width calculation for control characters and escape sequences
- [ ] Bash-style prompt escapes — `\w` (working directory), `\u` (username), `\h` (hostname), etc.
- [ ] History expansion — `!!` (last command), `!n` (by number)

## Future: Arithmetic Expansion Edge Cases

- [ ] `$(cmd)` inside `$((...))` does not handle quoted `)` — depth counter in `expand_vars` ignores quote context, so `$(echo "3)")` breaks (`src/expand/arith.rs`)

## Future: Code Quality Improvements

- [ ] `cd` overwrites OLDPWD before `set_current_dir` — if chdir fails, OLDPWD is still modified (`src/builtin/mod.rs`)
- [ ] `exec_function_call` lacks panic safety — `push_scope` without Drop guard means `pop_scope` is skipped on panic (`src/exec/mod.rs`)
- [ ] `VarStore::vars_iter()` rebuilds HashMap on every call — consider returning `Vec` or caching for performance (`src/env/vars.rs`)

## Future: E2E Test Expansion

- [ ] Builtin test POSIX_REF values could use more specific section numbers (e.g., `2.14.3` instead of `2.14 Special Built-In Utilities`)
- [ ] `fd_close.sh` test only checks exit code, not actual fd close effect
- [ ] `redirection/heredoc_pipeline.sh` has stale XFAIL marker — test now passes, remove XFAIL to eliminate XPASS in summary
- [ ] `e2e/command_execution/echo_simple.sh` has `755` permissions — should be `644` to match project convention
