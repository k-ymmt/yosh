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
- [ ] `history.save()` silently ignores write errors — disk-full or permission errors are swallowed (`src/interactive/history.rs`)
- [ ] `suggest()` linear scan performance — iterates all history entries on each keystroke; acceptable for HISTSIZE ≤ 500, may need caching or indexing for larger histories (`src/interactive/history.rs`)

## Future: Interactive Mode Enhancements

- [ ] Multiline editing — visual multiline editing with cursor movement across lines
- [ ] `~/.kishrc` startup file — ENV variable support for interactive initialization
- [ ] `set -o interactive` flag management
- [ ] Interactive-specific trap behavior — SIGTERM/SIGQUIT ignored by default
- [ ] `CLICOLOR=0` support in `should_colorize()` — disable colors even on TTY when `CLICOLOR=0` is set; many CLI tools support this alongside `NO_COLOR` (`src/main.rs`)
- [ ] `print_help()` DRY refactor — color/no-color branches duplicate help text; extract into a data-driven approach to reduce update risk when adding new flags (`src/main.rs`)
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
- [ ] `kish-plugin sync`/`install`: suggest `KISH_GITHUB_TOKEN` when GitHub API rate limit (60 req/hour) is hit without auth (`crates/kish-plugin-manager/src/github.rs`, `crates/kish-plugin-manager/src/install.rs`)
- [ ] `kish-plugin install`: tilde expansion for local paths — `~/my-plugin.dylib` not supported because `canonicalize()` doesn't expand `~`; consider reusing `config::expand_tilde_path` before canonicalization (`crates/kish-plugin-manager/src/install.rs`)
- [ ] `kish-plugin sync --prune`: remove empty plugin directories after deleting binaries (`crates/kish-plugin-manager/src/sync.rs`)
- [ ] Workspace default package: `cargo test` without `-p` or `--workspace` may not find kish tests — document in CLAUDE.md or set `default-members` in workspace config (`Cargo.toml`)
- [ ] `kish-plugin update`: version replacement uses naive `String::replacen` which may target wrong plugin if two share the same version — consider using `toml_edit` for TOML-preserving edits (`crates/kish-plugin-manager/src/main.rs`)
- [ ] `kish-plugin update` help: add `#[arg(value_name = "PLUGIN")]` to show `[PLUGIN]` instead of `[NAME]` in help output (`crates/kish-plugin-manager/src/main.rs`)
- [ ] `verify.rs` reads entire file into memory for SHA-256 — use streaming `Digest::update()` for large binaries (`crates/kish-plugin-manager/src/verify.rs`)
- [ ] `GitHubClient` public API error type — `find_asset_url`, `latest_version`, `download` still return `Result<_, String>`; promote internal `GitHubApiError` to a public error type so callers can match on structured variants instead of string messages (`crates/kish-plugin-manager/src/github.rs`)
- [ ] Integration tests: add checksum mismatch re-download test and partial failure (404) test per spec (`crates/kish-plugin-manager/tests/`)

## Future: Expansion Edge Cases

- [ ] `expand_heredoc_string` `${...}` scanning is quote-unaware — depth counter ignores quote context; not currently reachable because only simple variable lookup is implemented (no conditional forms like `${var:-default}`), but will become a bug if conditional parameter expansion is added (`src/expand/mod.rs:183-197`)

## Future: Code Quality Improvements

- [ ] Extract quote-aware balanced-paren scanning into a shared helper — the same ~40-line scanning logic (single/double quote skip, backslash escape, depth counting) is duplicated in three places: `expand_heredoc_string` `$(...)` and `$((...))` branches (`src/expand/mod.rs`) and `expand_vars` in `src/expand/arith.rs`; consider a `skip_balanced_parens(bytes, start, terminator)` helper
- [ ] Runtime error migration — replace ~90 `eprintln!("kish: ...")` call sites in exec/builtin with `Result<i32, ShellError>` using `RuntimeErrorKind` variants (type definitions ready in `src/error.rs`)
- [ ] `exit_requested` check missing in `exec_and_or()` — `exit 0 || echo X` could execute the `echo` after `exit` sets `exit_requested`; rare in practice but technically incorrect (`src/exec/mod.rs`)
- [ ] `PENDING_EXIT_SIGNAL` uses `Ordering::Relaxed` — sufficient for 50ms polling but `Acquire/Release` pair would be more idiomatic for cross-thread flag signaling (`src/signal.rs`)

## Future: E2E Test Expansion

- [ ] Builtin test POSIX_REF values could use more specific section numbers (e.g., `2.14.3` instead of `2.14 Special Built-In Utilities`)
- [ ] `fd_close.sh` test only checks exit code, not actual fd close effect
- [ ] `e2e/command_execution/echo_simple.sh` has `755` permissions — should be `644` to match project convention
