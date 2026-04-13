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
- [ ] `~/.kishrc` startup file — ENV variable support for interactive initialization
- [ ] `set -o interactive` flag management
- [ ] Interactive-specific trap behavior — SIGTERM/SIGQUIT ignored by default
- [ ] Bash-style prompt escapes — `\w` (working directory), `\u` (username), `\h` (hostname), etc.
- [ ] History expansion — `!!` (last command), `!n` (by number)
- [ ] Right-aligned prompt (`PS1_RIGHT`) — starship-style right-side prompt display based on terminal width (`src/interactive/line_editor.rs`)
- [ ] Pre-prompt hook timeout — protect against slow `pre_prompt` plugins blocking prompt display; consider timeout or async approach (`src/plugin/mod.rs`)
- [ ] Prompt segment API — structured segment registration for multiple plugins to contribute prompt sections without PS1 conflicts (`src/plugin/`, `crates/kish-plugin-sdk/`)
- [ ] Ctrl+C / empty-Enter type distinction — both return `Ok(Some(""))` from `read_line`; introduce a dedicated variant for clearer intent (`src/interactive/line_editor.rs`, `src/interactive/mod.rs`)
- [ ] Parse status edge-case tests — `||` continuation, `for...do` incomplete, nested structures, unterminated here-document (`tests/interactive.rs`)
- [ ] Tab completion: command name completion — complete executable names from PATH in command position (`src/interactive/completion.rs`)
- [ ] Tab completion: `CompletionUI`/`FuzzySearchUI` filtered/total display — both UIs show `N/N` instead of `filtered/total` because original count is not tracked (`src/interactive/completion.rs`, `src/interactive/fuzzy_search.rs`)
- [ ] Tab completion: unify `read_line` and `read_line_with_completion` — `read_line` is now only used by tests; consider merging into a single method (`src/interactive/line_editor.rs`)
- [ ] Syntax highlighting: color palette customization — allow users to override colors via environment variables like `KISH_COLOR_KEYWORD=blue` (`src/interactive/highlight.rs`)
- [ ] Syntax highlighting: double-quote `$` expansion uses inline scanning — deeply nested cases like `"$(foo "$(bar)")"` may highlight incorrectly; consider mode-stack approach (`src/interactive/highlight.rs`)
- [ ] Syntax highlighting: `redraw()` ANSI optimization — currently calls `reset_style()` on every style change; could reduce escape sequences with diff-based rendering (`src/interactive/line_editor.rs`)
- [ ] Emacs keybindings: `~/.inputrc` config file — Keymap struct is separated for future configurability but no config file reading is implemented (`src/interactive/keymap.rs`)
- [ ] Emacs keybindings: undo group boundary on space — spec says space triggers undo group boundary but implementation defers boundary to next non-space char; undo granularity is slightly coarser than readline (`src/interactive/line_editor.rs`)
- [ ] Emacs keybindings: PTY E2E tests — kill/yank round-trip, undo, word movement, numeric arg scenarios not covered by PTY tests (`tests/pty_interactive.rs`)

## Future: Plugin System Enhancements

- [ ] Runtime plugin load/unload — builtin commands `plugin load <path>` / `plugin unload <name>` for dynamic management
- [ ] SemVer API version management — replace single `KISH_PLUGIN_API_VERSION` check with semver range compatibility (`crates/kish-plugin-api/`)
- [ ] `~/.kishrc` plugin loading — load plugins configured in `~/.kishrc` once startup file support is implemented
- [ ] SDK `export!` macro `unsafe` lint — `#[allow(unsafe_attr_outside_unsafe)]` workaround in generated code; clean up when macro hygiene improves (`crates/kish-plugin-sdk/src/lib.rs`)
- [ ] Sandbox: warn on unknown capability strings in `plugins.toml` — currently `capabilities_from_strs` silently ignores typos like `"typo:read"`; should log warning in `load_from_config` (`src/plugin/config.rs`, `src/plugin/mod.rs`)
- [ ] Sandbox: `CAP_ALL` manual sync risk — when adding new capabilities, `CAP_ALL` must be manually updated; consider deriving it from a list or using a test to verify completeness (`crates/kish-plugin-api/src/lib.rs`)

## Future: Arithmetic Expansion Edge Cases

- [ ] `$(cmd)` inside `$((...))` does not handle quoted `)` — depth counter in `expand_vars` ignores quote context, so `$(echo "3)")` breaks (`src/expand/arith.rs`)

## Future: Code Quality Improvements

- [ ] Runtime error migration — replace ~90 `eprintln!("kish: ...")` call sites in exec/builtin with `Result<i32, ShellError>` using `RuntimeErrorKind` variants (type definitions ready in `src/error.rs`)

## Future: E2E Test Expansion

- [ ] Builtin test POSIX_REF values could use more specific section numbers (e.g., `2.14.3` instead of `2.14 Special Built-In Utilities`)
- [ ] `fd_close.sh` test only checks exit code, not actual fd close effect
- [ ] `e2e/command_execution/echo_simple.sh` has `755` permissions — should be `644` to match project convention
