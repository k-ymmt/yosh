# TODO

## Job Control: Known Limitations

- [ ] `%string` / `%?string` job specifiers — prefix/substring matching not implemented
- [ ] `disown` builtin — not implemented (non-POSIX extension)
- [ ] `suspend` builtin — not implemented
- [ ] Terminal state save/restore (tcgetattr/tcsetattr) — jobs that modify terminal settings may leave terminal in bad state
- [ ] Pipeline command display in `jobs` output uses placeholder format — improve to reconstruct shell syntax
- [ ] `reset_job_control_signals` is unused — should be called when `set +m` disables monitor mode at runtime (`src/signal.rs`)

## Future: Interactive Mode Enhancements

- [ ] History — ↑/↓ for history navigation, `~/.kish_history` persistence, Ctrl+R reverse search
- [ ] Tab completion — file path and command name completion
- [ ] Multiline editing — visual multiline editing with cursor movement across lines
- [ ] Emacs keybindings — Ctrl+K (kill to end), Ctrl+U (kill to start), Ctrl+W (kill word), Ctrl+Y (yank)
- [ ] `~/.kishrc` startup file — ENV variable support for interactive initialization
- [ ] `set -o interactive` flag management
- [ ] Interactive-specific trap behavior — SIGTERM/SIGQUIT ignored by default
- [ ] Prompt width — accurate column width calculation for control characters and escape sequences
- [ ] Bash-style prompt escapes — `\w` (working directory), `\u` (username), `\h` (hostname), etc.
- [ ] History expansion — `!!` (last command), `!n` (by number)
- [ ] Terminal resize handling — `Event::Resize` not processed in `read_line`, prompt display may break after resize (`src/interactive/line_editor.rs`)
- [ ] Ctrl+C / empty-Enter type distinction — both return `Ok(Some(""))` from `read_line`; introduce a dedicated variant for clearer intent (`src/interactive/line_editor.rs`, `src/interactive/mod.rs`)
- [ ] Parse status edge-case tests — `||` continuation, `for...do` incomplete, nested structures, unterminated here-document (`tests/interactive.rs`)

## Future: Arithmetic Expansion Edge Cases

- [ ] `$(cmd)` inside `$((...))` does not handle quoted `)` — depth counter in `expand_vars` ignores quote context, so `$(echo "3)")` breaks (`src/expand/arith.rs`)

## Future: Code Quality Improvements

- [ ] `cd` overwrites OLDPWD before `set_current_dir` — if chdir fails, OLDPWD is still modified (`src/builtin/mod.rs`)
- [ ] `exec_function_call` lacks panic safety — `push_scope` without Drop guard means `pop_scope` is skipped on panic (`src/exec/mod.rs`)
- [ ] `VarStore::vars_iter()` rebuilds HashMap on every call — consider returning `Vec` or caching for performance (`src/env/vars.rs`)
- [ ] `DupOutput`/`DupInput` redirect kinds lack `fd == target_fd` guard — other redirect kinds have it to prevent closing a just-opened fd; not currently exploitable since dup variants don't call `close` (`src/exec/redirect.rs`)
- [ ] `builtin_source` `FlowControl::Return` consumption runs unconditionally — should only consume after `exec_program`, not on parse error path; currently unreachable but fragile (`src/builtin/special.rs`)

## Future: E2E Test Expansion

- [ ] Builtin test POSIX_REF values could use more specific section numbers (e.g., `2.14.3` instead of `2.14 Special Built-In Utilities`)
- [ ] `fd_close.sh` test only checks exit code, not actual fd close effect
- [ ] `e2e/command_execution/echo_simple.sh` has `755` permissions — should be `644` to match project convention
