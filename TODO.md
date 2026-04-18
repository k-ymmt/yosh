# TODO

## Job Control: Known Limitations

- [ ] `disown` builtin — not implemented (non-POSIX extension)
- [ ] `suspend` builtin — not implemented
- [ ] Terminal state save/restore (tcgetattr/tcsetattr) — jobs that modify terminal settings may leave terminal in bad state
- [ ] Pipeline command display in `jobs` output uses placeholder format — improve to reconstruct shell syntax

## History: Known Limitations

- [ ] `HISTCONTROL` colon-separated values — bash supports `ignoredups:ignorespace` but current implementation only accepts single values like `ignoreboth` (`src/interactive/history.rs`)
- [ ] `history.save()` silently ignores write errors — disk-full or permission errors are swallowed (`src/interactive/history.rs`)
- [ ] `suggest()` linear scan performance — iterates all history entries on each keystroke; acceptable for HISTSIZE ≤ 500, may need caching or indexing for larger histories (`src/interactive/history.rs`)

## Future: Interactive Mode Enhancements

- [ ] `ENV` tilde expansion PTY test — `ENV=~/foo` tilde expansion is only exercised on interactive startup; add PTY test to verify `~` and `~user` cases (`tests/pty_interactive.rs`)
- [ ] Multiline editing — visual multiline editing with cursor movement across lines
- [ ] `set -o interactive` flag management
- [ ] Interactive-specific trap behavior — SIGTERM/SIGQUIT ignored by default
- [ ] `CLICOLOR=0` support in `should_colorize()` — disable colors even on TTY when `CLICOLOR=0` is set; many CLI tools support this alongside `NO_COLOR` (`src/main.rs`)
- [ ] Bash-style prompt escapes — `\w` (working directory), `\u` (username), `\h` (hostname), etc.
- [ ] History expansion — `!!` (last command), `!n` (by number)
- [ ] Right-aligned prompt (`PS1_RIGHT`) — starship-style right-side prompt display based on terminal width (`src/interactive/line_editor.rs`)
- [ ] Pre-prompt hook timeout — protect against slow `pre_prompt` plugins blocking prompt display; consider timeout or async approach (`src/plugin/mod.rs`)
- [ ] Prompt segment API — structured segment registration for multiple plugins to contribute prompt sections without PS1 conflicts (`src/plugin/`, `crates/yosh-plugin-sdk/`)
- [ ] Ctrl+C / empty-Enter type distinction — both return `Ok(Some(""))` from `read_line`; introduce a dedicated variant for clearer intent (`src/interactive/line_editor.rs`, `src/interactive/mod.rs`)
- [ ] Parse status edge-case tests — `||` continuation, `for...do` incomplete, nested structures, unterminated here-document (`tests/interactive.rs`)
- [ ] Tab completion: `CompletionUI`/`FuzzySearchUI` filtered/total display — both UIs show `N/N` instead of `filtered/total` because original count is not tracked (`src/interactive/completion.rs`, `src/interactive/fuzzy_search.rs`)
- [ ] Tab completion: unify `read_line` and `read_line_with_completion` — `read_line` is now only used by tests; consider merging into a single method (`src/interactive/line_editor.rs`)
- [ ] Syntax highlighting: color palette customization — allow users to override colors via environment variables like `YOSH_COLOR_KEYWORD=blue` (`src/interactive/highlight.rs`)
- [ ] Syntax highlighting: double-quote `$` expansion uses inline scanning — deeply nested cases like `"$(foo "$(bar)")"` may highlight incorrectly; consider mode-stack approach (`src/interactive/highlight.rs`)
- [ ] Syntax highlighting: `redraw()` ANSI optimization — currently calls `reset_style()` on every style change; could reduce escape sequences with diff-based rendering (`src/interactive/line_editor.rs`)
- [ ] Emacs keybindings: `~/.inputrc` config file — Keymap struct is separated for future configurability but no config file reading is implemented (`src/interactive/keymap.rs`)
- [ ] Emacs keybindings: undo group boundary on space — spec says space triggers undo group boundary but implementation defers boundary to next non-space char; undo granularity is slightly coarser than readline (`src/interactive/line_editor.rs`)
- [ ] Emacs keybindings: PTY E2E tests — kill/yank round-trip, undo, word movement, numeric arg scenarios not covered by PTY tests (`tests/pty_interactive.rs`)
- [ ] PTY tests: remaining `thread::sleep` after send — autosuggest/tab completion/syntax highlight/`set -m` tests still rely on 50–200ms fixed waits for UI render or child startup (not raw-mode races); if CI flakiness appears on those paths, migrate them to condition-based waits similar to `wait_for_raw_mode` (`tests/pty_interactive.rs`)

## Future: Plugin System Enhancements

- [ ] Runtime plugin load/unload — builtin commands `plugin load <path>` / `plugin unload <name>` for dynamic management
- [ ] SemVer API version management — replace single `YOSH_PLUGIN_API_VERSION` check with semver range compatibility (`crates/yosh-plugin-api/`)
- [ ] SDK `export!` macro `unsafe` lint — `#[allow(unsafe_attr_outside_unsafe)]` workaround in generated code; clean up when macro hygiene improves (`crates/yosh-plugin-sdk/src/lib.rs`)
- [ ] Sandbox: warn on unknown capability strings in `plugins.toml` — currently `capabilities_from_strs` silently ignores typos like `"typo:read"`; should log warning in `load_from_config` (`src/plugin/config.rs`, `src/plugin/mod.rs`)
- [ ] Sandbox: `CAP_ALL` manual sync risk — when adding new capabilities, `CAP_ALL` must be manually updated; consider deriving it from a list or using a test to verify completeness (`crates/yosh-plugin-api/src/lib.rs`)
- [ ] `yosh-plugin sync`/`install`: suggest `YOSH_GITHUB_TOKEN` when GitHub API rate limit (60 req/hour) is hit without auth (`crates/yosh-plugin-manager/src/github.rs`, `crates/yosh-plugin-manager/src/install.rs`)
- [ ] `yosh-plugin install`: tilde expansion for local paths — `~/my-plugin.dylib` not supported because `canonicalize()` doesn't expand `~`; consider reusing `config::expand_tilde_path` before canonicalization (`crates/yosh-plugin-manager/src/install.rs`)
- [ ] `yosh-plugin sync --prune`: remove empty plugin directories after deleting binaries (`crates/yosh-plugin-manager/src/sync.rs`)
- [ ] Workspace default package: `cargo test` without `-p` or `--workspace` may not find yosh tests — document in CLAUDE.md or set `default-members` in workspace config (`Cargo.toml`)
- [ ] `yosh-plugin update`: version replacement uses naive `String::replacen` which may target wrong plugin if two share the same version — consider using `toml_edit` for TOML-preserving edits (`crates/yosh-plugin-manager/src/main.rs`)
- [ ] `yosh-plugin update` help: add `#[arg(value_name = "PLUGIN")]` to show `[PLUGIN]` instead of `[NAME]` in help output (`crates/yosh-plugin-manager/src/main.rs`)
- [ ] `verify.rs` reads entire file into memory for SHA-256 — use streaming `Digest::update()` for large binaries (`crates/yosh-plugin-manager/src/verify.rs`)
- [ ] `GitHubClient` public API error type — `find_asset_url`, `latest_version`, `download` still return `Result<_, String>`; promote internal `GitHubApiError` to a public error type so callers can match on structured variants instead of string messages (`crates/yosh-plugin-manager/src/github.rs`)
- [ ] Integration tests: add checksum mismatch re-download test and partial failure (404) test per spec (`crates/yosh-plugin-manager/tests/`)

## Future: Code Quality Improvements

- [ ] `JobTable::update_status` per-process status tracking — currently overwrites the overall `job.status` on each child exit; if per-process status tracking (e.g., `$PIPESTATUS` array) is needed in the future, the `Job` struct will need a `Vec<(Pid, JobStatus)>` field instead of a single `status` (`src/env/jobs.rs`)
- [ ] `skip_balanced_*` unterminated input tests — `skip_balanced_parens`, `skip_balanced_braces`, `skip_balanced_double_parens` all return `bytes.len()` on unterminated input but none have tests for this behavior (`src/expand/mod.rs`)
- [ ] `find_in_path` vs `lookup_in_path` — `find_in_path` returns `Option<PathBuf>` (exec-only); `lookup_in_path` returns 3-state `PathLookup` for 126/127 distinction. Consider making `find_in_path` a thin wrapper over `lookup_in_path` to remove the near-duplicate directory walk (`src/exec/command.rs`)
- [ ] `exec_regular_builtin` "internal error" guards for `wait` / `fg`/`bg`/`jobs` / `command` are growing — consider factoring "Executor-requiring builtins" into an explicit classification or dispatch table instead of per-name guards (`src/builtin/mod.rs`)
- [ ] `render_verbose` Function arm has no unit test — `command -V <function>` branch exercised only through E2E; add a focused unit test in `src/builtin/command.rs` tests module
- [ ] `preview_command` has no direct unit tests — only exercised via E2E; add focused tests for compound-command / unexpandable-word fallback and pipeline first-command extraction (`src/exec/mod.rs`)
- [ ] `JobSpecError::Ambiguous` fully qualified at 3 call sites in `src/exec/mod.rs` (builtin_wait/fg/bg) — add a module-level `use crate::env::jobs::JobSpecError;` for readability
- [ ] `highlight_scanner.rs` `KEYWORDS` duplicates POSIX §2.4 list — `src/interactive/highlight_scanner.rs:66-69` defines its own copy of the 16 reserved words, separate from the canonical `crate::lexer::reserved::RESERVED_WORDS`. Consolidate once the contextual subsets (`COMMAND_POSITION_KEYWORDS` includes `"time"`, command-position restoration logic) are re-expressed in terms of the canonical list (`src/interactive/highlight_scanner.rs`)

## Future: E2E Test Expansion

- [ ] Builtin test POSIX_REF values could use more specific section numbers (e.g., `2.14.3` instead of `2.14 Special Built-In Utilities`)
- [ ] `fd_close.sh` test only checks exit code, not actual fd close effect
- [ ] `e2e/command_execution/echo_simple.sh` has `755` permissions — should be `644` to match project convention
- [ ] Extend chapter-by-chapter POSIX coverage beyond XCU Chapter 2 — once the Chapter 2 coverage matrix stabilizes, add systematic E2E coverage for Chapter 4 Utilities (all shell-relevant builtins: special + regular, with option/edge-case matrices) and Chapter 8 Environment Variables. Reuse the `POSIX_REF`/`XFAIL` harness established for Chapter 2.
- [ ] Deepen Chapter 2 POSIX coverage to normative-requirement granularity — after the hybrid (representative + thin-section) coverage lands, enumerate every shall/must/should clause in XCU Chapter 2 and add one E2E test per normative requirement (est. +100–200 tests). Use `XFAIL` liberally to register gaps; the goal is to make each normative clause individually traceable to a test ID.

## Future: POSIX Conformance Gaps (Chapter 2)

- [ ] §2.6.1 Tilde expansion on assignment RHS — `x=~/bin` does not expand `~` to `$HOME` (see `e2e/posix_spec/2_06_01_tilde_expansion/tilde_assignment_rhs.sh` XFAIL)
- [ ] §2.10 Shell Grammar — parser accepts an empty `compound_list` inside `if ... then fi` (exit 0) instead of rejecting it as a syntax error; POSIX BNF `term : term separator and_or | and_or` requires at least one `and_or` (see `e2e/posix_spec/2_10_shell_grammar/empty_compound_list_in_if_is_error.sh` XFAIL)
- [ ] §2.5.3 LINENO — `$LINENO` expands to an empty string; POSIX requires it to be set to the current script/function line number before each command (see `e2e/posix_spec/2_05_03_shell_variables/lineno_in_script.sh` XFAIL)
- [ ] §2.5.3 PWD logical path — `cd` resolves PWD to the physical path (e.g., `/tmp` -> `/private/tmp` on macOS); POSIX `cd` without `-P` shall preserve the logical path unless dot-dot resolution occurs (see `e2e/posix_spec/2_05_03_shell_variables/pwd_after_cd.sh` XFAIL)
- [ ] §2.11 ignored-on-entry signal inheritance — no in-harness test yet (nested `sh -c` escapes yosh); revisit after a yosh-aware subshell helper lands

## Future: Release Skill Enhancements

- [ ] `phase_push` remote tag upsert — currently only checks local tag existence; if the same tag already exists on origin, `git push origin <tag>` rejects. Add `git ls-remote --exit-code --tags origin <tag>` check before pushing (`.claude/skills/release/scripts/release.sh`)
- [ ] `test_plugin/Cargo.toml` version lag risk — `tests/plugins/test_plugin` is a workspace member but not in the `phase_bump` manifests list (not publishable). Currently safe because it depends on workspace crates only via `path =`; breaks if it ever adds `version = "..."` pins (`.claude/skills/release/scripts/release.sh`)
- [ ] `CRATES` array comment — `yosh-plugin-manager` has no dependency on `yosh-plugin-api`/`yosh-plugin-sdk`, so its position in `api → sdk → manager → yosh` is convention, not dependency-ordered. Add a comment clarifying this (`.claude/skills/release/scripts/release.sh`)
- [ ] `phase_publish` root-crate branch — the `if [[ "$crate" == "yosh" ]]` special case (bare `cargo publish` for root vs `cargo publish -p` for members) can be simplified to uniform `cmd=(cargo publish -p "$crate")` since cargo accepts `-p` on root crates too (`.claude/skills/release/scripts/release.sh`)
