# TODO

## E2E XFAIL Roadmap

Decomposition of 55 XFAIL tests into 7 sub-projects. See
`docs/superpowers/specs/2026-05-13-e2e-xfail-roadmap-design.md`.

- [ ] SP2 — Required-builtin diagnostics + native `type`/`hash` (5 tests)
- [ ] SP3 — `read` builtin implementation (9 tests; includes `exec_close_fd` and `exec_redir_input`)
- [ ] SP4 — `getopts` builtin implementation (9 tests)
- [ ] SP5 — Miscellaneous small POSIX features (8 tests)
- [ ] SP6 — PTY harness migration (10 tests)
- [ ] SP7 — Deferred / recorded as known deviation (3 tests)

## Job Control: Known Limitations

- [ ] `disown` builtin — not implemented (non-POSIX extension)
- [ ] `suspend` builtin — not implemented
- [ ] Pipeline command display in `jobs` output uses placeholder format — improve to reconstruct shell syntax
- [ ] Task 7 (`fg` job-termios replay) has no direct PTY assertion — Task 9/10 verify end-state only (Task 6 shell-restore). On macOS/BSD, `/bin/cat`'s `read()` inherits `SIG_DFL` for SIGCONT and BSD does not auto-restart `read()` without `SA_RESTART`, so cat exits with EINTR immediately after `fg`. Linux auto-restarts `read()` on terminals for `SIG_DFL` signals, masking this asymmetry. Revisit by using a sleep/read-loop helper that retries on EINTR, or by reading `tcgetattr` directly via the PTY master between `fg\r` and cat's exit (the diagnosis details currently live in the `DEVIATION` comment of `test_pty_termios_preserved_across_suspend_fg` in `tests/pty_interactive.rs`).
- [ ] `JobTable.shell_tmodes` is a one-time startup snapshot — `stty` invoked at the interactive prompt modifies the real terminal but not the cached snapshot, so the post-foreground shell-restore overwrites user-applied `stty` changes (`src/interactive/mod.rs` + `src/env/jobs/mod.rs`). Matches glibc manual behavior; revisit if user reports surface.

## Code Format Drift

- [ ] Add `cargo fmt --all -- --check` step to a GitHub Actions workflow so the workspace stays drift-free after the 2026-05-03 sweep. Workspace is currently fmt-clean but no CI enforcement exists; new contributions can re-introduce drift silently. Pair with `cargo clippy --all-targets -- -D warnings` if a lint gate is also wanted (`.github/workflows/`).

## History: Known Limitations

- [ ] `suggest()` linear scan performance — iterates all history entries on each keystroke; acceptable for HISTSIZE ≤ 500, may need caching or indexing for larger histories (`src/interactive/history.rs`)

## Future: Interactive Mode Enhancements

- [ ] `ENV` tilde expansion PTY test — `ENV=~/foo` tilde expansion is only exercised on interactive startup; add PTY test to verify `~` and `~user` cases (`tests/pty_interactive.rs`)
- [ ] Multiline editing — visual multiline editing with cursor movement across lines
- [ ] `set -o interactive` flag management
- [ ] Interactive-specific trap behavior — SIGTERM/SIGQUIT ignored by default
- [ ] `set -x` PS4 prefix — `set -x` trace output always uses the hardcoded `+ ` prefix; the `PS4` variable is not consulted. POSIX requires trace lines to be prefixed with the value of `PS4` (default `+ `). XFAIL test: `e2e/posix_spec/8_env_vars/PS4_assigned.sh` (`src/exec/simple.rs`)
- [ ] Bash-style prompt escapes — `\w` (working directory), `\u` (username), `\h` (hostname), etc.
- [ ] History expansion — `!!` (last command), `!n` (by number)
- [ ] Right-aligned prompt (`PS1_RIGHT`) — starship-style right-side prompt display based on terminal width (`src/interactive/line_editor.rs`)
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

- [ ] Consolidate `HostContext`, `MetadataCtx`, and `TestCtx` onto a shared `HostBackend` trait so the three host implementations no longer have to mirror WIT changes by hand. Mirrors the existing TODO about deriving metadata-extract deny stubs from the bindgen `Host` traits (`src/plugin/host/`, `crates/yosh-plugin-manager/src/test_host/`, `crates/yosh-plugin-manager/src/metadata_extract.rs`).
- [ ] `yosh plugin run --watch` mode to re-run on wasm file change. Out of scope for the initial run/test landing per spec §11.
- [ ] Scenario format: consider a multi-plugin variant for cooperating plugin tests. Currently one scenario = one plugin. Defer until a real use case appears.
- [ ] `runner::load_plugin` watchdog uses a one-shot detached thread (matches `metadata_extract`). Under a CPU-bound guest busy-loop the elapsed wall-clock to trip the trap is 3–8s on macOS, well over the spec §10 case-5 budget of ~2s. The `tests/runner.rs::case_5_timeout_on_slow_plugin_pre_prompt` test was relaxed to a 15s ceiling. Either move the manager runner onto a continuous tick thread (production host parity) or amend the spec to record the one-shot model.
- [ ] Harness-level error paths in `yosh plugin run` (`load`/`engine`/`metadata`/runner) currently print stderr-only human text and bypass `--format json`. Spec §4.1 says JSON callers should receive `{"error":{"kind":..., "message":...}}` even on failure. Re-route all `eprintln!` paths in `cmd_run` through the formatter when `OutputFormat::Json` is selected (`crates/yosh-plugin-manager/src/lib.rs::cmd_run`).
- [ ] `--cap` empty fallback in `yosh plugin run` re-reads the wasm + builds a fresh engine + runs `metadata_extract`, then `load_plugin` re-reads + recompiles. ~2× wasm I/O + compile per invocation. Threading the bytes / engine through would halve startup time on cold runs (`crates/yosh-plugin-manager/src/lib.rs::cmd_run`).
- [ ] `yosh plugin test --format json` summary lines omit spec §4.2 fields `step` / `expected` / `got`. Currently the failure is conflated into the freeform `reason` string (`step N: vars_set: want {...}, got {...}`). CI consumers can extract via regex but the structured fields would be more reliable (`crates/yosh-plugin-manager/src/scenario.rs::format_summary_json`).
- [ ] Spec §6 last paragraph promised `log` crate wiring (`RUST_LOG=yosh_plugin_manager::runner=debug` traces host import calls). No `log::` calls were added in the initial implementation; runner/scenario/test_host are silent on the trace channel. Wire `log` once a debug story is needed.
- [ ] Spec §6 troubleshooting hint strings not implemented in `cmd_run`: "metadata called a host import" / "commands:exec denied for `<argv>` — re-run with `--allow-exec '<pattern>'` or `--cap commands:exec`" / "files:read denied — add `files:read` to `env.caps`". Generic `Err(Denied)` propagates without guidance. Surfacing these via a small `hint(error_kind, context)` helper in runner.rs would close the most-cited dev-UX gap (`crates/yosh-plugin-manager/src/lib.rs::cmd_run`, `crates/yosh-plugin-manager/src/runner.rs`).
- [ ] `Expect::files_write = { path = "bytes-string" }` only checks byte *length*, not content, because `RunOutcome.write_log` stores `(PathBuf, usize)` not `(PathBuf, Vec<u8>)`. A scenario expecting `files_write = { "/out" = "hello" }` passes for any 5-byte write to `/out`. Either widen `write_log` to capture bytes or document the length-only semantics in `docs/yosh/plugin.md` §Testing Locally.
- [ ] `tests/runner.rs` covers virtual-FS scenarios only. Sandbox-mode (`sandbox_root = Some(path)`) is unit-tested in `test_host/files.rs` but no end-to-end scenario exercises a real-FS plugin write. Adding `tests/scenarios/sandbox_write_pass.toml` and a fixture plugin variant would close the gap.
- [ ] `RunnerError::{Trap, Timeout}` variants are dead code — only `Load` is ever constructed; trap/timeout classification happens via `classify_trap` returning a `&'static str` to `RunOutcome.error_kind`. Either collapse the enum to a single variant or wire Trap/Timeout through `LoadedPlugin` return paths (`crates/yosh-plugin-manager/src/runner.rs:21-26`).
- [ ] CLI-only types in `lib.rs` (`RunAction`, `HookKind`, `OutputFormat`, `parse_kv`) are `pub` despite being clap-derive helpers used only by `Cli` / `cmd_run`. Tightening to `pub(crate)` shrinks the library surface without affecting the binary (`crates/yosh-plugin-manager/src/lib.rs:57,68,76,81`).
- [ ] `host_commands_exec` 1000 ms timeout path (SIGTERM → 100 ms grace → SIGKILL) has no dedicated test in the manager crate. Production-side `src/plugin/host/commands.rs` has `host_commands_exec_timeout_after_1000ms` and `host_commands_exec_kills_child_on_timeout`; mirror those in `test_host/commands.rs` to lock down the duplicated spawn helper's behaviour (`crates/yosh-plugin-manager/src/test_host/commands.rs`).
- [ ] `set_cwd` empty-path error-code drift: TestCtx returns `InvalidArgument`, production host returns `IoFailed` (different error mapping). Either align TestCtx with production or document the deliberate divergence in the `TestState` doc comment so plugin authors don't write error-mapping tests that pass in the harness and fail in production (`crates/yosh-plugin-manager/src/test_host/filesystem.rs::host_set_cwd`).
- [ ] `Expect::denied: bool` scenario key (spec §5) — observing capability-denied errors from the harness needs plumbing a counter through every host import (each `Err(Denied)` increments `TestState.denied_count`). Deferred from the initial landing because authors can detect denial via `stdout_regex` on guest-side error handling or via specific `exit` codes the guest returns on `Err(ErrorCode::Denied)` (`crates/yosh-plugin-manager/src/scenario.rs::Expect`).
- [ ] WASI surface lockdown deviation from spec §6 — both `src/plugin/linker.rs` and `crates/yosh-plugin-manager/src/metadata_extract.rs` register the full `wasmtime_wasi::add_to_linker_sync` surface rather than the spec-prescribed `clocks` + `random` subset, because cargo-component's wasip2 adapter pulls in `wasi:io`, `wasi:cli/*`, `wasi:filesystem`, and `wasi:sockets` transitively for any Rust component (even plugins that touch only the `yosh:plugin/*` host imports). The metadata-extract path was widened in response to issue #3 — a narrow subset broke `instantiate_pre` for any real cargo-component plugin and silently dropped it from `plugins.lock`. Privacy is still enforced by the empty `WasiCtx` (no preopens, no stdio, no env, no args), but the linker surface is wider than the spec implied. Revisit if a future cargo-component release stops emitting unused WASI imports, or if a hand-built core-wasm pipeline becomes practical.
- [ ] Spec §8.4 "metadata cannot reach host APIs" — covered at the host-internal level via `src/plugin/host.rs::tests::metadata_contract_*` (every real host import returns `Err(Denied)` when `HostContext.env` is null). A contrived plugin whose `metadata()` calls `cwd()` would test the same invariant but requires SDK plumbing to override the trait's default `metadata` body, which Task 6 deferred. If the SDK gains an `override_metadata` hook in the future, add the integration-level companion.
- [ ] Derive metadata-extract deny stubs from the bindgen `Host` traits — `crates/yosh-plugin-manager/src/metadata_extract.rs::register_all_deny_imports` lists every `yosh:plugin/*` function by hand. When a new interface or function is added to `wit/yosh-plugin.wit` it must be mirrored here (and in `src/plugin/linker.rs`), or `instantiate_pre` will fail at sync time exactly like issue #3. Implementing `wasmtime::component::bindgen!`'s generated `Host` traits on `MetadataCtx` (each returning `Err(Denied)`) and calling the generated `add_to_linker` would make the surface compile-checked. Defer until the next plugin-world expansion since the current set is small and stable.
- [ ] Spec §8.10 "WASI surface lockdown" integration test — currently covered indirectly by `src/plugin/linker.rs::tests::linker_construction_smoke` and the empty-`WasiCtx` isolation property. A hand-crafted wasm component that imports `wasi:cli/stdout` and asserts an unsatisfied-import error at instantiate would be a stronger negative test, but requires fixture authoring (raw wasm) outside the cargo-component pipeline. Defer until a fixture pattern is established.
- [ ] Plugin runtime limits (fuel / memory caps / pre-prompt timeout) — out of scope for v0.2.0 per spec §10; add wasmtime fuel metering and per-call memory caps when ready.
- [ ] Spec §8.6–§8.8 cwasm field-mutation tests at integration level — `tests/plugin.rs` covers `t06` (cwasm missing) and `t09` (wasm SHA mismatch) end-to-end, but per-field mutation of `wasmtime_version` / `target_triple` / `engine_config_hash` is currently only unit-tested in `src/plugin/cache.rs::tests`. Adding integration smokes would require a fixture-cwasm builder helper.
- [ ] Plugin perf §4.2 linker_cache concurrency story — `PluginManager.linker_cache: HashMap<u32, Linker<HostContext>>` (added 2026-05-09 in commit `0f49eb8` for fix#2) is plain `HashMap` because `load_one` takes `&mut self` and current loads are sequential. If `load_one` ever becomes concurrent (parallel plugin loads, runtime `plugin load` builtin), the field must migrate to `RwLock<HashMap<u32, Arc<Linker<HostContext>>>>` or equivalent — see `docs/superpowers/specs/2026-05-09-plugin-real-linker-cache-design.md` §7 + §11 for the migration path (`src/plugin/mod.rs`).
- [ ] Plugin perf Appendix D delta note — `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` Appendix D records the §4.2 fix#2 verification (698 blocks, −50% same-mask), but does not call out that the design spec's prediction of `≈ 467 blocks` was 33% optimistic because non-`build_linker` allocations (`instantiate_pre`, component init) also share the `LinkerInstance::insert` dhat frame. Adding a one-paragraph note would protect future planning sessions from over-trusting per-call dhat extrapolations when frames are shared. Final-review follow-up from 2026-05-09 plugin real-linker-cache branch.
- [ ] `commands::exec` argv borrow design-spec drift — `docs/superpowers/specs/2026-05-09-plugin-commands-exec-argv-borrow-design.md` has two stale sections after implementation: (a) §3.1's closure sketch shows `args.iter(&store)` (immutable) which does not compile in wasmtime 27 because `WasmList::iter` requires `impl Into<StoreContextMut<'a, U>>`; the actual implementation uses a two-pass collect with `&mut store`, recorded in `Appendix F: Plan deviation` of the perf report; (b) §3 Scope (in) lists `src/bin/yosh-dhat.rs` "add `noop_commands_exec_borrow` smoke" but no changes were made to that file (the `--exec-loop` dispatcher is generic and looks up commands by name; the smoke command was added to `tests/plugins/perf_plugin/src/lib.rs`). Either annotate §3.1 / §3 inline as historical with forward-references to Appendix F, or rewrite the affected sections to match the implementation. Final-review follow-up from 2026-05-09 plugin commands::exec argv borrow rollout.
- [ ] Runtime plugin load/unload — builtin commands `plugin load <path>` / `plugin unload <name>` for dynamic management
- [ ] Workspace default package: `cargo test` without `-p` or `--workspace` may not find yosh tests — document in CLAUDE.md or set `default-members` in workspace config (`Cargo.toml`)
- [ ] `yosh-plugin update` help: add `#[arg(value_name = "PLUGIN")]` to show `[PLUGIN]` instead of `[NAME]` in help output (`crates/yosh-plugin-manager/src/main.rs`)
- [ ] `verify.rs` reads entire file into memory for SHA-256 — use streaming `Digest::update()` for large binaries (`crates/yosh-plugin-manager/src/verify.rs`)
- [ ] `GitHubClient` public API error type — `find_asset_url`, `latest_version`, `download` still return `Result<_, String>`; promote internal `GitHubApiError` to a public error type so callers can match on structured variants instead of string messages (`crates/yosh-plugin-manager/src/github.rs`)
- [ ] Integration tests: add checksum mismatch re-download test and partial failure (404) test per spec (`crates/yosh-plugin-manager/tests/`)
- [ ] `files:write` host ops (`write-file`/`append-file`/`create-dir`) collapse `io::ErrorKind::NotFound` into `IoFailed` rather than mapping to `ErrorCode::NotFound` like the read ops do. Spec §4 error-mapping table is written as if uniform across all 8 functions; either add a footnote acknowledging the write-side asymmetry or actually map NotFound on the write side (e.g., parent-dir-missing on `write-file`/`create-dir`). Pre-decided as acceptable during implementation but worth revisiting if a plugin author wants the parent-not-found distinction (`src/plugin/host.rs`, `docs/superpowers/specs/2026-04-29-plugin-files-rw-capability-design.md` §4). Code-review follow-up from 2026-04-29 plugin files-rw branch.
- [ ] `FileStat::is_symlink` is effectively always `false` because `host_files_metadata` uses `std::fs::metadata` (which follows symlinks). Document in the SDK doc comment that the field is currently always `false` for symlinks-on-disk and that detecting them requires the future `symlink_metadata` host import (Spec §10 Open Questions already lists adding it) (`crates/yosh-plugin-sdk/src/lib.rs`, `crates/yosh-plugin-api/wit/yosh-plugin.wit`). Code-review follow-up from 2026-04-29 plugin files-rw branch.
- [ ] Restore WIT inline doc-comments stripped during implementation — spec §2 includes design-intent comments on `interface files` (e.g. `// Lightweight stat. Extended in the future by adding new functions, never by changing this record's shape.`, `// basename only, not full path`, `// Read group — gated by CAP_FILES_READ`) that were dropped from `crates/yosh-plugin-api/wit/yosh-plugin.wit`. These are the only WIT-level documentation telling future authors why each record/group is shaped the way it is. Cosmetic but high-value for future maintainers. Code-review follow-up from 2026-04-29 plugin files-rw branch.
- [ ] `yosh-plugin-sdk` could grow an `exec_to_string` helper that wraps `exec()` and returns `Result<(String, i32), ErrorCode>` (mirrors `read_to_string` vs `read_file`). Plugin authors invoking `exec()` for line-counting use cases will commonly write `String::from_utf8_lossy(&out.stdout)`. Code-review follow-up from 2026-04-29 plugin commands:exec branch (`crates/yosh-plugin-sdk/src/lib.rs`).
- [ ] `yosh-plugin-sdk::exec()` doc style drift — its `# Errors` block predates the 2026-05-03 doc sweep and uses a different format (`Err(ErrorCode::Denied) — …`) than the newer helpers (`[ErrorCode::Denied] — …`). Normalize to the newer style next time `lib.rs` is touched. Cosmetic but jarring side-by-side (`crates/yosh-plugin-sdk/src/lib.rs`). Follow-up from 2026-05-03 plugin-sdk doc-comment sweep.
- [ ] `files:write` sandbox cross-reference is prose, not a clickable link — per-helper docs in `crates/yosh-plugin-sdk/src/lib.rs` say `"files:write sandbox" note` (plain text) instead of an intra-doc link to the crate-level `# files:write sandbox` heading. Convert to `[crate#fileswrite-sandbox]` (rustdoc slug form) so `cargo doc` renders it as a hyperlink. Discoverability nit. Follow-up from 2026-05-03 plugin-sdk doc-comment sweep.
- [ ] `test_helpers::load_plugin_with_caps` no-allowlist ergonomics — all 15 callers in `tests/plugin.rs` pass `&[]` for the new `allowed_commands` parameter introduced in T6. Consider a no-allowlist convenience method or a `Default` impl so common test setups are less verbose. Code-review follow-up from 2026-04-29 plugin commands:exec branch (`src/plugin/mod.rs`).
- [ ] `resolve_cdpath_empty_entry_is_dot` test in `src/builtin/regular.rs:783` calls `std::env::set_current_dir(tempdir.path())` then lets the tempdir drop, leaving the lib-test process cwd pointing at a deleted directory. Any subsequent test that spawns a subshell (e.g., `host_commands_exec_captures_stderr_separately`) sees a "shell-init: error retrieving current directory" warning leak into stderr. Mitigated at the consumer side via `ends_with` in commit `715ffd6`; root-cause fix is to capture the original cwd at test entry and restore on exit (or use `set_current_dir(tempdir)` only after replacing the tempdir's drop semantics). Code-review follow-up from 2026-04-29 plugin commands:exec branch.
- [ ] Hook timeout: extend epoch-deadline enforcement from `pre_prompt` to `pre_exec` / `post_exec` / `on_cd`. The infrastructure (tick thread, `WithEnvError::Trapped { is_interrupt }`, `set_epoch_deadline`, `STORE_BASELINE_DEADLINE_TICKS` reset) is already in place from the 2026-05-04 pre_prompt-timeout work; only `call_pre_exec` / `call_post_exec` / `call_on_cd` need to gain a deadline + hook-specific message. Defer until a slow non-pre_prompt hook is reported in practice (`src/plugin/mod.rs`).
- [ ] Pre-prompt timeout regression test — direct verification of the post-call deadline-restore in `call_pre_prompt` requires a "fast pre_prompt + pre_exec" plugin fixture (slow_plugin's busy-loop cannot model a successful return). The fix (commit `154e96e`) is exercised indirectly today via the existing 23-test corpus on plugins without `pre_prompt`. Add a dedicated fixture and integration test once a use case forces the issue (`tests/plugin.rs`, `tests/plugins/`).
- [ ] Auto-regenerated `tests/plugins/{test_plugin,trap_plugin,slow_plugin}/src/bindings.rs` — `cargo component build` regenerates these files on every invocation, dirtying the git tree. Either add them to `.gitignore` (and have `cargo component build` run as part of any test that needs them, which already happens via `ensure_built`) or pin a stable regeneration mode in the build pipeline. Operational nit observed during 2026-05-04 pre_prompt-timeout work.

## Future: Code Quality Improvements

- [ ] `JobTable::update_status` per-process status tracking — currently overwrites the overall `job.status` on each child exit; if per-process status tracking (e.g., `$PIPESTATUS` array) is needed in the future, the `Job` struct will need a `Vec<(Pid, JobStatus)>` field instead of a single `status` (`src/env/jobs/mod.rs`)
- [ ] `find_in_path` vs `lookup_in_path` — `find_in_path` returns `Option<PathBuf>` (exec-only); `lookup_in_path` returns 3-state `PathLookup` for 126/127 distinction. Consider making `find_in_path` a thin wrapper over `lookup_in_path` to remove the near-duplicate directory walk (`src/exec/command.rs`)
- [ ] `exec_regular_builtin` "internal error" guards for `wait` / `fg`/`bg`/`jobs` / `command` are growing — consider factoring "Executor-requiring builtins" into an explicit classification or dispatch table instead of per-name guards (`src/builtin/mod.rs`)
- [ ] `render_verbose` Function arm has no unit test — `command -V <function>` branch exercised only through E2E; add a focused unit test in `src/builtin/command.rs` tests module
- [ ] `preview_command` has no direct unit tests — only exercised via E2E; add focused tests for compound-command / unexpandable-word fallback and pipeline first-command extraction (`src/exec/mod.rs`)
- [ ] `highlight_scanner` `KEYWORDS` duplicates POSIX §2.4 list — `src/interactive/highlight_scanner/helpers.rs` defines its own copy of the 16 reserved words, separate from the canonical `crate::lexer::reserved::RESERVED_WORDS`. Consolidate once the contextual subsets (`COMMAND_POSITION_KEYWORDS` includes `"time"`, command-position restoration logic) are re-expressed in terms of the canonical list (`src/interactive/highlight_scanner/helpers.rs`)
- [ ] `cargo fmt --check -- <path>` misreads edition — rustfmt 1.8.0 / Rust 1.94.1 fails to parse let-chain syntax as edition 2024 when invoked with explicit file paths despite `Cargo.toml` specifying `edition = "2024"`, producing spurious fmt errors. Workaround: invoke `rustfmt --edition 2024 --check <path>` directly. Revisit when upstream rustfmt catches up.
- [ ] `parse_compound_list` non-empty regression tests are incomplete — only `nonempty_if_parses_ok` exists in `src/parser/compound.rs`. Add parallel `nonempty_while_parses_ok` / `nonempty_until_parses_ok` / `nonempty_for_parses_ok` / `nonempty_brace_group_parses_ok` / `nonempty_subshell_parses_ok` so future refactors cannot accidentally over-reject any individual context.
- [ ] LINENO update allocates a `String` per command — `exec_simple_command` / `exec_compound_command` call `cmd.line.to_string()` and go through `VarStore::set`. For tight loops this is ~500μs per 10k commands. If benchmarks ever show pressure, add `ShellEnv.exec.current_lineno: usize` and intercept `$LINENO` in `expand::param` to read that field directly, bypassing the alloc + HashMap write (`src/exec/simple.rs`, `src/exec/compound.rs`, `src/expand/param.rs`).
- [ ] Extract `try_parse_assignment` value-construction walker into a private helper — the ~25-line match loop plus its 21-line doc comment dominates `try_parse_assignment` and will be swapped wholesale when sub-project 4 replaces `prev_was_literal` with escape metadata. A helper like `fn build_assignment_value_parts(after_eq: &str, remaining_parts: &[WordPart]) -> Vec<WordPart>` would make the doc comment a rustdoc `///`, keep `try_parse_assignment` focused on name/value splitting, and localize sub-project 4's diff (`src/parser/simple.rs`).
- [ ] `try_parse_assignment` `other.clone()` deep-copies CommandSub — the non-Literal branch clones each remaining `WordPart`, which for `$(...)` substitutions clones the embedded `Program`. Same inefficiency as the prior `extend_from_slice`, so not a regression, but consider consuming `Word` (take ownership) or draining `word.parts` to avoid the copy (`src/parser/simple.rs`).
- [ ] `expand_assignment_builtin_args` string round-trip — helper builds `"NAME=value"` strings that the builtin re-parses with `find('=')`. Lossless today, but couples the helper shape to the legacy builtin API. When a future refactor touches `builtin_export`/`builtin_readonly` signatures, consider passing `Vec<(String, Option<String>)>` directly to skip the round-trip (`src/exec/simple.rs`, `src/builtin/special.rs`).
- [ ] macOS CI job — Task 1 (SIGNAL_TABLE libc-const fix) corrects a bug that only manifests on macOS. Current CI only runs on Linux, so the regression test for the fix is not actually exercising the bug pre-fix. Add a GitHub Actions macOS runner to `cargo test` on every push so future signal-numbering regressions are caught. Spec cross-cutting concern from 2026-04-20 signal-table design.
- [ ] `exec_function_call` residual 2.1× overhead vs arithmetic loop (§4.2) — ~50 µs/call vs ~24 µs/iter at HEAD. Sub-benches are the prerequisite per `performance.md` §4.2 candidate #1: split into `exec_function_call_nopanic_guard` (replace `catch_unwind` with a Drop-guard scope popper), `exec_function_call_cached_environ`, `exec_function_call_smallvec_scope` to isolate which of the four suspected causes dominates. Then act on whatever the sub-benches reveal (`src/exec/function.rs:9-45`).
- [ ] Multi-byte IFS support in UTF-8 locale (bash-extension parity) — `field_split::split` currently matches IFS as an ASCII byte-set. `IFS="日"; set -- $"a日b"` yields `[a] [b]` under bash in UTF-8 locale (character-level match) but is silently ignored (post-fix A) or produces garbled bytes (pre-fix A) in yosh. POSIX leaves this locale-dependent; bash uses character-level matching when locale is multi-byte. Plan: introduce a `char`-level IFS match path (`char_indices` in `split_field`, char-mode `ifs` set) gated by locale detection. Deferred from the 2026-04-21 `append_byte` UTF-8 panic fix to keep scope minimal. See the brainstorming log for that fix; reference bash 3.2 behavior under `LC_ALL=en_US.UTF-8` as the target semantics.
- [ ] `fork + run-Rust-shell-code-in-child` is fundamentally POSIX-UB in MT contexts — even with `exit_child` helper, `exec_subshell` runs `self.exec_body(body)` in the child, which touches arbitrary Rust std (mutexes, allocators, env) and is technically only legal between `fork()` and `exec()` if all calls are async-signal-safe. Currently safe in practice because interactive shell parent is single-threaded; test harness is the exception. Long-term architectural consideration: reevaluate whether subshells should use `fork+exec` (separate yosh invocation with serialized state) instead of `fork+in-process interpreter`. Out of scope for the immediate fix; record to avoid forgetting the latent hazard.
- [ ] `Parser::current_token` API shape — `interactive/parse_status.rs:61` compares the result against `&Token::Newline` literally, which forces every caller to construct a borrowed `Token` value just for equality. Consider a predicate `fn is_token(&self, t: &Token) -> bool` (or an enum-tag helper) that hides the borrow. Discovered during the 2026-05-05 visibility-tightening spec follow-up (`docs/superpowers/specs/2026-05-05-parser-visibility-tightening-design.md` §4.2-1, `src/parser/mod.rs`).
- [ ] `Parser::try_parse_assignment` should be a free function — it takes no `self` and is called only from `src/exec/simple.rs:33`. Moving it to a module-level `pub fn try_parse_assignment(word: &Word) -> Option<Assignment>` in `src/parser/simple.rs` would drop one of the two surviving `pub fn`s on the `Parser` impl and clarify that the function is a pure utility. Discovered during the 2026-05-05 visibility-tightening spec follow-up (§4.2-2, `src/parser/simple.rs:84`).
- [ ] Bench API surface — `Parser::new` and `parse_program` are the only two `Parser` items required to stay `pub`; their sole external consumers are `benches/parser_bench.rs` and `benches/exec_bench.rs`. Wrapping them in a bench-only helper module (e.g. an internal `pub(crate) fn parse_for_bench(s: &str) -> Program` reachable through a `#[cfg(any(test, feature = "internal_api"))]` shim) would let both `Parser::new` and `parse_program` drop to `pub(crate)`, shrinking the public Parser surface from 10 to 8. Requires bench-side refactor. Discovered during the 2026-05-05 visibility-tightening spec follow-up (§4.2-3, `benches/parser_bench.rs`, `benches/exec_bench.rs`).
- [ ] `Executor` API visibility tightening (post-split follow-up) — five `pub` methods on `Executor` are candidates for `pub(crate)` since their callers are all in-crate: `Executor::exec_command` (only `pipeline.rs` + tests), `exec_and_or` (internal-only), `exec_program` (used by `expand/command_sub.rs`, `bin/yosh-dhat.rs`, `builtin/special.rs`), `exec_complete_command` (used by `compound.rs`, `interactive/mod.rs`, `main.rs`), and `display_job_notifications` (only `interactive/mod.rs` + `control.rs::exec_complete_command`). Mirrors the 2026-05-05 parser-visibility-tightening pattern. Surfaced during the 2026-05-05 exec/mod.rs split final review (`src/exec/control.rs`, `src/exec/job_control.rs`).
- [ ] `assignment_rhs_backslash_tilde_after_colon_stays_literal` (`src/parser/simple.rs:311`) still uses the loose `!any(matches!(p, Tilde(_)))` form — sibling test to `assignment_rhs_param_then_escaped_tilde_stays_literal` (line 321) which was tightened on 2026-05-10 to a structural `assert_eq!`. Apply the same treatment so a `/bin` segment drop or shape regression is caught at unit-test level. Code-review follow-up from 2026-05-10 POSIX TODO cleanup branch.

## Future: POSIX Required Builtin Implementation

The following XCU §1.4 required builtins are not implemented as native
yosh builtins. yosh currently falls through to the system's
`/usr/bin/<name>` POSIX shell wrappers, which works for external
commands and basic option parsing but cannot see yosh's session state
(aliases, functions, in-shell variables). The XFAIL tests added in
2026-05-13 (`e2e/posix_spec/4_required_builtin/`) serve as the
behavioral acceptance spec for each native implementation. When a
native builtin is implemented, the corresponding XFAIL tests should
become PASS; remove the `# XFAIL:` line at that point.

- [ ] `getopts optstring var [args]` — option-parsing helper, used in
      portable shell scripts. Currently uses `/usr/bin/getopts`. XFAIL
      tests: `e2e/posix_spec/4_required_builtin/getopts_*.sh` (6 of 8 tests
      pass via fallback; 6 remain XFAIL pending native impl)
- [ ] `hash [-r] [cmd]` — utility-location cache. Currently uses
      `/usr/bin/hash`. XFAIL tests:
      `e2e/posix_spec/4_required_builtin/hash_*.sh` (1 of 4 remains XFAIL —
      exit-status mismatch for unknown command)
- [ ] `read [-r] var...` — read one line from stdin into variables.
      Currently uses `/usr/bin/read`. XFAIL tests:
      `e2e/posix_spec/4_required_builtin/read_*.sh` (6 of 7 remain XFAIL —
      most cases require in-process state)
- [ ] `type name...` — identify command kind (function / builtin / alias
      / external path). Currently uses `/usr/bin/type`. XFAIL tests:
      `e2e/posix_spec/4_required_builtin/type_*.sh` (2 of 5 remain XFAIL —
      session-local aliases and functions not visible to external wrapper)
- [ ] `ulimit [-f] [num]` — resource-limit query/set. Currently uses
      `/usr/bin/ulimit`. XFAIL tests:
      `e2e/posix_spec/4_required_builtin/ulimit_*.sh` (1 of 3 remains XFAIL
      — unknown-option case)

## Future: POSIX Conformance Bugs

The following yosh behaviors diverge from POSIX shall/must requirements
and were surfaced as `XFAIL: non-POSIX deviation (...)` during the
2026-05-13 Ch4+Ch8 E2E expansion. Each entry points to the XFAIL test
that documents the expected POSIX behavior; when the fix lands, the
test becomes PASS and the `# XFAIL:` line should be removed.

- [ ] `break` / `continue` outside any enclosing loop — yosh exits 0
      silently. POSIX requires nonzero exit and a diagnostic. XFAIL
      tests: `e2e/posix_spec/4_special_builtin/break_outside_loop.sh`,
      `continue_outside_loop.sh`.
- [ ] `continue N` when N exceeds loop nesting — yosh treats it as
      `break` (only first iteration runs). POSIX requires continuing
      the outermost loop. XFAIL test:
      `e2e/posix_spec/4_special_builtin/continue_n_exceeds_depth.sh`.
- [ ] `export` / `readonly` / `unset` accept invalid identifiers (e.g.,
      `export 1foo=v`). POSIX requires an error. XFAIL tests:
      `e2e/posix_spec/4_special_builtin/export_invalid_name.sh`,
      `readonly_invalid_name.sh`, `unset_invalid_name.sh`.
- [ ] `readonly -p` produces no output (bare `readonly` works). POSIX
      requires re-input form listing. XFAIL test:
      `e2e/posix_spec/4_special_builtin/readonly_p_listing.sh`.
- [ ] `unset -f` removes the variable instead of the function. POSIX
      requires `-f` to act on functions only. XFAIL tests:
      `e2e/posix_spec/4_special_builtin/unset_f_function.sh`,
      `unset_f_keeps_variable.sh`.
- [ ] `exec CMD` does not pass exported variables to the replaced
      process. POSIX requires the environment to be preserved across
      exec. XFAIL test:
      `e2e/posix_spec/4_special_builtin/exec_keeps_env.sh`.
- [ ] `exec <FILE` does not redirect the shell's stdin for subsequent
      commands (e.g., a following `read` does not see the file
      contents). XFAIL test:
      `e2e/posix_spec/4_special_builtin/exec_redir_input.sh`.
- [ ] Standalone `$(...)` exit status not propagated to `$?` — yosh
      sets `$?` to 0 after a bare `$(cmd)` regardless of `cmd`'s exit
      status. POSIX §2.6.3 requires the substituted command's exit
      status to be reflected in `$?` when the substitution is the
      command itself. XFAIL test:
      `e2e/posix_spec/2_06_03_command_substitution/exit_status_propagates_to_parent.sh`.
- [ ] Redirection left-to-right ordering is not honoured — `cmd 2>&1 >f`
      should dup fd 2 to the current stdout (terminal) before redirecting
      stdout to `f`, so only stdout ends up in the file; yosh processes
      both redirections against the post-update state, causing stderr to
      also land in `f`. XFAIL test:
      `e2e/posix_spec/2_07_redirection/redir_order_left_to_right.sh`.
- [ ] `trap` INT handler is deferred to end-of-script instead of
      running asynchronously when the signal is delivered. POSIX
      requires the trap action to run as soon as the shell is ready
      to accept it. XFAIL test:
      `e2e/posix_spec/4_special_builtin/trap_int_handler.sh`.
- [ ] `trap 0` / `trap EXIT` not fired on subshell exit — POSIX §2.11
      requires the EXIT pseudo-signal handler to run when the shell
      exits, including subshells. XFAIL test:
      `e2e/posix_spec/2_11_signals_and_error_handling/trap_zero_runs_on_exit.sh`.
- [ ] `jobs` returns exit 0 for an unknown job spec or unknown option.
      POSIX requires exit 1 with a diagnostic. XFAIL tests:
      `e2e/posix_spec/4_required_builtin/jobs_unknown_spec.sh`,
      `jobs_invalid_option.sh`.
- [ ] `$PPID` special parameter returns empty — POSIX requires it to
      hold the parent process ID at shell startup. XFAIL test:
      `e2e/posix_spec/8_env_vars/PPID_is_set.sh`.
- [ ] Locale support not implemented — `LANG` / `LC_*` / `NLSPATH` are
      accepted as variables but do not affect collation, character
      classification, message localization, or message catalogs.
      XFAIL test:
      `e2e/posix_spec/8_env_vars/LANG_default_collate.sh` (other
      `LC_*` tests currently pass via default-C-locale semantics).
- [ ] Redirection error on a special builtin does not exit the (sub)shell — yosh
      continues to execute subsequent commands. POSIX §2.8.1 requires the
      non-interactive shell to exit on such an error. XFAIL test:
      `e2e/posix_spec/2_08_01_consequences_of_shell_errors/special_builtin_redir_error_exits.sh`.
- [ ] Redirect-only simple command (no command word) does not apply the redirect — yosh
      exits 0 without creating or truncating the target file. POSIX §2.9.1 requires
      that the redirections are performed even when no command is present. XFAIL test:
      `e2e/posix_spec/2_09_01_simple_commands/redirection_only_creates_file.sh`.
- [ ] Reserved word not recognized after an assignment prefix — `x=1 if true; then echo y; fi`
      triggers exit 127 ("if: not found") instead of treating `if` as the command-position
      reserved word. POSIX §2.4 requires reserved-word recognition regardless of leading
      assignment prefixes. XFAIL test:
      `e2e/posix_spec/2_04_reserved_words/reserved_after_assignment_recognized.sh`.

## Future: Release Skill Enhancements

- [ ] `phase_push` remote tag upsert — currently only checks local tag existence; if the same tag already exists on origin, `git push origin <tag>` rejects. Add `git ls-remote --exit-code --tags origin <tag>` check before pushing (`.claude/skills/release/scripts/release.sh`)
- [ ] `test_plugin/Cargo.toml` version lag risk — `tests/plugins/test_plugin` is a workspace member but not in the `phase_bump` manifests list (not publishable). Currently safe because it depends on workspace crates only via `path =`; breaks if it ever adds `version = "..."` pins (`.claude/skills/release/scripts/release.sh`)
- [ ] `phase_publish` root-crate branch — the `if [[ "$crate" == "yosh" ]]` special case (bare `cargo publish` for root vs `cargo publish -p` for members) can be simplified to uniform `cmd=(cargo publish -p "$crate")` since cargo accepts `-p` on root crates too (`.claude/skills/release/scripts/release.sh`)
- [ ] `release.sh test` wall-time variance observation — after per-test-binary parallelization (2026-04-23), 3 back-to-back runs measured 95 s / 162 s / 178 s (±22 %, exceeds nominal ±20 % stability threshold). Root cause: `cargo test --no-run --workspace` incremental-check time varies with filesystem cache state (run 1 benefits from peak warmth). Not a correctness issue. If CI-based benchmarking is added, introduce a warm-up run before timed measurements to reduce first-run bias (`.claude/skills/release/scripts/release.sh`).
