# TODO

## Job Control: Known Limitations

- [ ] `%string` / `%?string` job specifiers — prefix/substring matching not implemented
- [ ] `disown` builtin — not implemented (non-POSIX extension)
- [ ] `suspend` builtin — not implemented
- [ ] Terminal state save/restore (tcgetattr/tcsetattr) — jobs that modify terminal settings may leave terminal in bad state
- [ ] Pipeline command display in `jobs` output uses placeholder format — improve to reconstruct shell syntax
- [ ] `reset_job_control_signals` is unused — should be called when `set +m` disables monitor mode at runtime (`src/signal.rs`)

## History: Known Limitations

- [ ] `HISTCONTROL` colon-separated values — bash supports `ignoredups:ignorespace` but current implementation only accepts single values like `ignoreboth` (`src/interactive/history.rs`)
- [ ] SIGHUP history save — verify history is saved before exit on SIGHUP; if `handle_default_signal` calls `std::process::exit()` directly, history may be lost (`src/exec/mod.rs`, `src/interactive/mod.rs`)
- [ ] `history.save()` silently ignores write errors — disk-full or permission errors are swallowed (`src/interactive/history.rs`)
- [ ] `suggest()` linear scan performance — iterates all history entries on each keystroke; acceptable for HISTSIZE ≤ 500, may need caching or indexing for larger histories (`src/interactive/history.rs`)

## Future: Interactive Mode Enhancements

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
- [ ] Tab completion: command name completion — complete executable names from PATH in command position (`src/interactive/completion.rs`)
- [ ] Tab completion: `CompletionUI`/`FuzzySearchUI` filtered/total display — both UIs show `N/N` instead of `filtered/total` because original count is not tracked (`src/interactive/completion.rs`, `src/interactive/fuzzy_search.rs`)
- [ ] Tab completion: unify `read_line` and `read_line_with_completion` — `read_line` is now only used by tests; consider merging into a single method (`src/interactive/line_editor.rs`)
- [ ] Syntax highlighting: color palette customization — allow users to override colors via environment variables like `KISH_COLOR_KEYWORD=blue` (`src/interactive/highlight.rs`)
- [ ] Syntax highlighting: double-quote `$` expansion uses inline scanning — deeply nested cases like `"$(foo "$(bar)")"` may highlight incorrectly; consider mode-stack approach (`src/interactive/highlight.rs`)
- [ ] Syntax highlighting: `redraw()` ANSI optimization — currently calls `reset_style()` on every style change; could reduce escape sequences with diff-based rendering (`src/interactive/line_editor.rs`)

## Future: Arithmetic Expansion Edge Cases

- [ ] `$(cmd)` inside `$((...))` does not handle quoted `)` — depth counter in `expand_vars` ignores quote context, so `$(echo "3)")` breaks (`src/expand/arith.rs`)

## Future: Code Quality Improvements

- [ ] Runtime error migration — replace ~90 `eprintln!("kish: ...")` call sites in exec/builtin with `Result<i32, ShellError>` using `RuntimeErrorKind` variants (type definitions ready in `src/error.rs`)

## Future: E2E Test Expansion

- [ ] Builtin test POSIX_REF values could use more specific section numbers (e.g., `2.14.3` instead of `2.14 Special Built-In Utilities`)
- [ ] `fd_close.sh` test only checks exit code, not actual fd close effect
- [ ] `e2e/command_execution/echo_simple.sh` has `755` permissions — should be `644` to match project convention
