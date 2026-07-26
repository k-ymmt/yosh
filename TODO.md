# TODO

## Code-Review Follow-ups (non-blocking)

### 2026-05-21 locale-support follow-ups (non-blocking)

- [ ] `LocaleCategory::env_var_name` is `fn` (private). When a
      future caller needs the variable name string (e.g., diagnostic
      messages referring to "LC_CTYPE"), promote to `pub(crate)`.
      Code-review follow-up from 2026-05-21 locale-support branch
      (`src/env/locale.rs`).
- [ ] `locale::resolve()` API has no live callers in v1 (intentional
      `#![allow(dead_code)]` scaffolding). When non-C locale support
      is added, wire `resolve()` into the pattern range / POSIX
      character class / `test` string-comparison call sites. Spec
      `docs/superpowers/specs/2026-05-21-locale-support-design.md`
      §2.3 documents this as the intended branch point.

### 2026-05-23 literal-argv word-splitting follow-ups (non-blocking)

- [ ] `set_mask_range` (`src/expand/mod.rs:130-143`) has a per-bit
      inner loop with no word-aligned fast path. For long literal /
      quoted runs (common in real shell input) the `/64 + %64 +
      shift` per byte is wasted. Switch to a word-aligned middle
      block plus head/tail edges — measurably faster if/when
      `cargo bench` shows expand pressure. Code-review follow-up
      from Task 1 (Important).

### 2026-07-03 selector-UI follow-ups

- [ ] Selector UI `colors_enabled()` reads NO_COLOR / CLICOLOR_FORCE / CLICOLOR
      from the process environment via `std::env::var_os` at startup only
      (`src/interactive/selector.rs`). yosh never calls `std::env::set_var`
      (thread-safety), so exported shell variables live only in ShellEnv and
      are passed to child processes explicitly. Consequence: running
      `export NO_COLOR=1` inside yosh does NOT disable selector colors —
      only setting NO_COLOR in the parent environment before launching yosh
      works. Future follow-up: plumb ShellEnv's NO_COLOR value (or other
      color-control exports) through the line_editor into `SelectorOptions.colors`
      so runtime `export NO_COLOR=1` takes effect immediately
      (`src/interactive/line_editor.rs`, `src/interactive/selector.rs::colors_enabled`).

## Job Control: Known Limitations

- [ ] `disown` builtin — not implemented (non-POSIX extension)
- [ ] `suspend` builtin — not implemented
- [ ] Pipeline command display in `jobs` output uses placeholder format — improve to reconstruct shell syntax
- [ ] Task 7 (`fg` job-termios replay) has no direct PTY assertion — Task 9/10 verify end-state only (Task 6 shell-restore). On macOS/BSD, `/bin/cat`'s `read()` inherits `SIG_DFL` for SIGCONT and BSD does not auto-restart `read()` without `SA_RESTART`, so cat exits with EINTR immediately after `fg`. Linux auto-restarts `read()` on terminals for `SIG_DFL` signals, masking this asymmetry. Revisit by using a sleep/read-loop helper that retries on EINTR, or by reading `tcgetattr` directly via the PTY master between `fg\r` and cat's exit (the diagnosis details currently live in the `DEVIATION` comment of `test_pty_termios_preserved_across_suspend_fg` in `tests/pty_interactive.rs`).
- [ ] `JobTable.shell_tmodes` is a one-time startup snapshot — `stty` invoked at the interactive prompt modifies the real terminal but not the cached snapshot, so the post-foreground shell-restore overwrites user-applied `stty` changes (`src/interactive/mod.rs` + `src/env/jobs/mod.rs`). Matches glibc manual behavior; revisit if user reports surface.

## History: Known Limitations

- [ ] `suggest()` linear scan performance — iterates all history entries on each keystroke; acceptable for HISTSIZE ≤ 500, may need caching or indexing for larger histories (`src/interactive/history.rs`)

## Future: Interactive Mode Enhancements

- [ ] `ENV` tilde expansion PTY test — `ENV=~/foo` tilde expansion is only exercised on interactive startup; add PTY test to verify `~` and `~user` cases (`tests/pty_interactive.rs`)
- [ ] Multiline editing follow-ups (core landed 2026-07-26: in-buffer
      continuation on incomplete Enter, Alt+Enter forced newline, up/down
      cursor movement, line-local C-a/C-e/C-k/C-u, PS2-prefixed rendering;
      2026-07-26 adversarial-review fixes landed: history-cursor reset on
      continuation, navigate_down guard, lazy PS2 expansion, newline-aware
      completion, composable closing-keyword probes, comment-aware
      trailing-operator check, multiline-safe history file format v2,
      heredoc highlight mode; 2026-07-27 landed: viewport-clamped
      terminal-height-aware rendering with explicit row packing,
      preferred-column stickiness on up/down, multiline autosuggestions
      rendered with continuation prompts):
      (a) multiline buffers always take the full clear+repaint path — extend
      the diff-based partial repaint to multiline layouts if large pasted
      blocks make per-keystroke repaints visibly slow
      (`src/interactive/line_editor.rs`).
- [ ] `set -o interactive` flag management
- [ ] Interactive-specific trap behavior — SIGTERM/SIGQUIT ignored by default
- [ ] `set -x` does not emit bash-style structural headers for `for` / `case` (yosh matches dash here; POSIX leaves the header format implementation-defined). Empirical survey 2026-05-28 confirmed compound bodies and pipeline members are already traced via `exec_simple_command`; the assignment-only gap was closed in the 2026-05-28 assignment-trace work. Adding bash parity for the headers requires Word→source rendering plus an xtrace argument-quoting algorithm; the latter also affects existing simple-command trace output (`echo "a b" c` traces as `+ echo a b c` not `+ echo 'a b' c`). Tracked together because both want the same quoting helper. See `docs/superpowers/specs/2026-05-28-set-x-assignment-trace-design.md` §5 for the closed assignment portion (`src/exec/compound.rs`, `src/exec/simple.rs`).
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
- [ ] Spec completion: `command_words` treats unquoted `&` in `2>&1` as a segment separator and counts assignment prefixes (`FOO=1 cmd`) / redirection words as command/positional words — spec lookup misses, degrading gracefully to path completion (`src/interactive/spec_completion.rs`)
- [ ] Spec completion: parse-error warning `eprintln!` fires while the terminal is in raw mode (staircase output, prompt not redrawn; once per session per bad file) (`src/interactive/spec_completion.rs`)
- [ ] Spec completion: quoted flag values (`--flag="path`) keep the quote inside `keep_prefix`, diverging from `completion::complete`'s quote-stripping convention (`src/interactive/spec_completion.rs`)

## Future: Plugin System Enhancements

- [ ] Consolidate `HostContext`, `MetadataCtx`, and `TestCtx` onto a shared `HostBackend` trait so the three host implementations no longer have to mirror WIT changes by hand. Mirrors the existing TODO about deriving metadata-extract deny stubs from the bindgen `Host` traits (`src/plugin/host/`, `crates/yosh-plugin-manager/src/test_host/`, `crates/yosh-plugin-manager/src/metadata_extract.rs`).
- [ ] Scenario format: consider a multi-plugin variant for cooperating plugin tests. Currently one scenario = one plugin. Defer until a real use case appears.
- [ ] `host_commands_exec` 1000 ms timeout path (SIGTERM → 100 ms grace → SIGKILL) has no dedicated test in the manager crate. Production-side `src/plugin/host/commands.rs` has `host_commands_exec_timeout_after_1000ms` and `host_commands_exec_kills_child_on_timeout`; mirror those in `test_host/commands.rs` to lock down the duplicated spawn helper's behaviour (`crates/yosh-plugin-manager/src/test_host/commands.rs`).
- [ ] WASI surface lockdown deviation from spec §6 — both `src/plugin/linker.rs` and `crates/yosh-plugin-manager/src/metadata_extract.rs` register the full `wasmtime_wasi::add_to_linker_sync` surface rather than the spec-prescribed `clocks` + `random` subset, because cargo-component's wasip2 adapter pulls in `wasi:io`, `wasi:cli/*`, `wasi:filesystem`, and `wasi:sockets` transitively for any Rust component (even plugins that touch only the `yosh:plugin/*` host imports). The metadata-extract path was widened in response to issue #3 — a narrow subset broke `instantiate_pre` for any real cargo-component plugin and silently dropped it from `plugins.lock`. Privacy is still enforced by the empty `WasiCtx` (no preopens, no stdio, no env, no args), but the linker surface is wider than the spec implied. Revisit if a future cargo-component release stops emitting unused WASI imports, or if a hand-built core-wasm pipeline becomes practical.
- [ ] Spec §8.4 "metadata cannot reach host APIs" — covered at the host-internal level via `src/plugin/host.rs::tests::metadata_contract_*` (every real host import returns `Err(Denied)` when `HostContext.env` is null). A contrived plugin whose `metadata()` calls `cwd()` would test the same invariant but requires SDK plumbing to override the trait's default `metadata` body, which Task 6 deferred. If the SDK gains an `override_metadata` hook in the future, add the integration-level companion.
- [ ] Derive metadata-extract deny stubs from the bindgen `Host` traits — `crates/yosh-plugin-manager/src/metadata_extract.rs::register_all_deny_imports` lists every `yosh:plugin/*` function by hand. When a new interface or function is added to `wit/yosh-plugin.wit` it must be mirrored here (and in `src/plugin/linker.rs`), or `instantiate_pre` will fail at sync time exactly like issue #3. Implementing `wasmtime::component::bindgen!`'s generated `Host` traits on `MetadataCtx` (each returning `Err(Denied)`) and calling the generated `add_to_linker` would make the surface compile-checked. Defer until the next plugin-world expansion since the current set is small and stable.
- [ ] Spec §8.10 "WASI surface lockdown" integration test — currently covered indirectly by `src/plugin/linker.rs::tests::linker_construction_smoke` and the empty-`WasiCtx` isolation property. A hand-crafted wasm component that imports `wasi:cli/stdout` and asserts an unsatisfied-import error at instantiate would be a stronger negative test, but requires fixture authoring (raw wasm) outside the cargo-component pipeline. Defer until a fixture pattern is established.
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
- [ ] Auto-regenerated `tests/plugins/{test_plugin,trap_plugin,slow_plugin}/src/bindings.rs` — `cargo component build` regenerates these files on every invocation, dirtying the git tree. Either add them to `.gitignore` (and have `cargo component build` run as part of any test that needs them, which already happens via `ensure_built`) or pin a stable regeneration mode in the build pipeline. Operational nit observed during 2026-05-04 pre_prompt-timeout work.

## Future: Code Quality Improvements

- [ ] Self-pipe fork race in remaining fork sites — a forked child inherits
      the parent's self-pipe handler and the shared pipe until
      `reset_child_signals` runs, so a signal delivered to the child in that
      window is written into the pipe and later misread by the parent as its
      own (parent then exits via `handle_default_signal`). `exec_async`
      (`src/exec/control.rs`) now blocks all signals across the fork and
      restores the mask on both sides after dispositions are set; the same
      latent race exists in `exec_subshell` (`src/exec/compound.rs`) and the
      pipeline child forks (`src/exec/pipeline.rs`). Apply the same
      block/restore pattern there if a repro surfaces (2026-07-14: observed
      only for async lists via `kill -TERM $!` racing the child's reset).

- [ ] `JobTable::update_status` per-process status tracking — currently overwrites the overall `job.status` on each child exit; if per-process status tracking (e.g., `$PIPESTATUS` array) is needed in the future, the `Job` struct will need a `Vec<(Pid, JobStatus)>` field instead of a single `status` (`src/env/jobs/mod.rs`)
- [ ] `exec_regular_builtin` "internal error" guards for `wait` / `fg`/`bg`/`jobs` / `command` are growing — consider factoring "Executor-requiring builtins" into an explicit classification or dispatch table instead of per-name guards (`src/builtin/mod.rs`)
- [ ] `highlight_scanner` `KEYWORDS` duplicates POSIX §2.4 list — `src/interactive/highlight_scanner/helpers.rs` defines its own copy of the 16 reserved words, separate from the canonical `crate::lexer::reserved::RESERVED_WORDS`. Consolidate once the contextual subsets (`COMMAND_POSITION_KEYWORDS` includes `"time"`, command-position restoration logic) are re-expressed in terms of the canonical list (`src/interactive/highlight_scanner/helpers.rs`)
- [ ] `cargo fmt --check -- <path>` misreads edition — rustfmt 1.8.0 / Rust 1.94.1 fails to parse let-chain syntax as edition 2024 when invoked with explicit file paths despite `Cargo.toml` specifying `edition = "2024"`, producing spurious fmt errors. Workaround: invoke `rustfmt --edition 2024 --check <path>` directly. Revisit when upstream rustfmt catches up.
- [ ] LINENO update allocates a `String` per command — `exec_simple_command` / `exec_compound_command` call `cmd.line.to_string()` and go through `VarStore::set`. For tight loops this is ~500μs per 10k commands. If benchmarks ever show pressure, add `ShellEnv.exec.current_lineno: usize` and intercept `$LINENO` in `expand::param` to read that field directly, bypassing the alloc + HashMap write (`src/exec/simple.rs`, `src/exec/compound.rs`, `src/expand/param.rs`).
- [ ] `strip_prefix` / `strip_suffix` re-parse `pat` on every candidate cut point — `pattern::matches` re-walks the whole pattern (rebuilding a `Vec<BracketItem>` per bracket) once per char boundary, so a value with N boundaries does O(N) full pattern parses and the cut-point scan is O(n²) char comparisons. The 2026-05-27 Layer-2 rewrite removed the per-cut `String` / `Vec<char>` *allocations* but not this CPU cost. Options: compile `pat` to an AST once and match it many times, or replace the brute-force cut-point loop with a left/right-anchored single-pass matcher. Recorded out-of-scope in `docs/superpowers/specs/2026-05-27-strip-prefix-suffix-zero-alloc-design.md` §7 (`src/expand/param.rs`, `src/expand/pattern.rs`).
- [ ] `try_parse_assignment` `other.clone()` deep-copies CommandSub — the non-Literal branch clones each remaining `WordPart`, which for `$(...)` substitutions clones the embedded `Program`. Same inefficiency as the prior `extend_from_slice`, so not a regression, but consider consuming `Word` (take ownership) or draining `word.parts` to avoid the copy (`src/parser/simple.rs`).
- [ ] `expand_assignment_builtin_args` string round-trip — helper builds `"NAME=value"` strings that the builtin re-parses with `find('=')`. Lossless today, but couples the helper shape to the legacy builtin API. When a future refactor touches `builtin_export`/`builtin_readonly` signatures, consider passing `Vec<(String, Option<String>)>` directly to skip the round-trip (`src/exec/simple.rs`, `src/builtin/special.rs`).
- [ ] macOS CI job — Task 1 (SIGNAL_TABLE libc-const fix) corrects a bug that only manifests on macOS. Current CI only runs on Linux, so the regression test for the fix is not actually exercising the bug pre-fix. Add a GitHub Actions macOS runner to `cargo test` on every push so future signal-numbering regressions are caught. Spec cross-cutting concern from 2026-04-20 signal-table design.
- [ ] `exec_function_call` residual 2.1× overhead vs arithmetic loop (§4.2) — ~50 µs/call vs ~24 µs/iter at HEAD. Sub-benches are the prerequisite per `performance.md` §4.2 candidate #1: split into `exec_function_call_nopanic_guard` (replace `catch_unwind` with a Drop-guard scope popper), `exec_function_call_cached_environ`, `exec_function_call_smallvec_scope` to isolate which of the four suspected causes dominates. Then act on whatever the sub-benches reveal (`src/exec/function.rs:9-45`).
- [ ] Multi-byte IFS support in UTF-8 locale (bash-extension parity) — `field_split::split` currently matches IFS as an ASCII byte-set. `IFS="日"; set -- $"a日b"` yields `[a] [b]` under bash in UTF-8 locale (character-level match) but is silently ignored (post-fix A) or produces garbled bytes (pre-fix A) in yosh. POSIX leaves this locale-dependent; bash uses character-level matching when locale is multi-byte. Plan: introduce a `char`-level IFS match path (`char_indices` in `split_field`, char-mode `ifs` set) gated by locale detection. Deferred from the 2026-04-21 `append_byte` UTF-8 panic fix to keep scope minimal. See the brainstorming log for that fix; reference bash 3.2 behavior under `LC_ALL=en_US.UTF-8` as the target semantics.
- [ ] `fork + run-Rust-shell-code-in-child` is fundamentally POSIX-UB in MT contexts — even with `exit_child` helper, `exec_subshell` runs `self.exec_body(body)` in the child, which touches arbitrary Rust std (mutexes, allocators, env) and is technically only legal between `fork()` and `exec()` if all calls are async-signal-safe. Currently safe in practice because interactive shell parent is single-threaded; test harness is the exception. Long-term architectural consideration: reevaluate whether subshells should use `fork+exec` (separate yosh invocation with serialized state) instead of `fork+in-process interpreter`. Out of scope for the immediate fix; record to avoid forgetting the latent hazard.
- [ ] `Parser::current_token` API shape — `interactive/parse_status.rs:61` compares the result against `&Token::Newline` literally, which forces every caller to construct a borrowed `Token` value just for equality. Consider a predicate `fn is_token(&self, t: &Token) -> bool` (or an enum-tag helper) that hides the borrow. Discovered during the 2026-05-05 visibility-tightening spec follow-up (`docs/superpowers/specs/2026-05-05-parser-visibility-tightening-design.md` §4.2-1, `src/parser/mod.rs`).
- [ ] Bench API surface — `Parser::new` and `parse_program` are the only two `Parser` items required to stay `pub`; their sole external consumers are `benches/parser_bench.rs` and `benches/exec_bench.rs`. Wrapping them in a bench-only helper module (e.g. an internal `pub(crate) fn parse_for_bench(s: &str) -> Program` reachable through a `#[cfg(any(test, feature = "internal_api"))]` shim) would let both `Parser::new` and `parse_program` drop to `pub(crate)`, shrinking the public Parser surface from 10 to 8. Requires bench-side refactor. Discovered during the 2026-05-05 visibility-tightening spec follow-up (§4.2-3, `benches/parser_bench.rs`, `benches/exec_bench.rs`).
- [ ] `Executor` API visibility tightening (post-split follow-up) — five `pub` methods on `Executor` are candidates for `pub(crate)` since their callers are all in-crate: `Executor::exec_command` (only `pipeline.rs` + tests), `exec_and_or` (internal-only), `exec_program` (used by `expand/command_sub.rs`, `bin/yosh-dhat.rs`, `builtin/special.rs`), `exec_complete_command` (used by `compound.rs`, `interactive/mod.rs`, `main.rs`), and `display_job_notifications` (only `interactive/mod.rs` + `control.rs::exec_complete_command`). Mirrors the 2026-05-05 parser-visibility-tightening pattern. Surfaced during the 2026-05-05 exec/mod.rs split final review (`src/exec/control.rs`, `src/exec/job_control.rs`).
- [ ] `ulimit` `-f` block arithmetic assumes `libc::rlim_t == u64` — `BLOCK_SIZE: libc::rlim_t = 512` and `SetBlocks(n: u64).saturating_mul(BLOCK_SIZE)` (`src/builtin/regular.rs`) compile only where `rlim_t` is `u64` (macOS, 64-bit Linux). On 32-bit Linux `rlim_t` is `u32`, making the `u64 * u32` a type error. Not a runtime risk and the project has no Linux CI / targets macOS, but if a 32-bit target is ever added, type `BLOCK_SIZE` as `u64` and cast at the `set_fsize` / `format_fsize_limit` call sites. Final-review follow-up from 2026-05-26 ulimit native -f branch.

## Future: Release Skill Enhancements

- [ ] `phase_push` remote tag upsert — currently only checks local tag existence; if the same tag already exists on origin, `git push origin <tag>` rejects. Add `git ls-remote --exit-code --tags origin <tag>` check before pushing (`.claude/skills/release/scripts/release.sh`)
- [ ] `test_plugin/Cargo.toml` version lag risk — `tests/plugins/test_plugin` is a workspace member but not in the `phase_bump` manifests list (not publishable). Currently safe because it depends on workspace crates only via `path =`; breaks if it ever adds `version = "..."` pins (`.claude/skills/release/scripts/release.sh`)
- [ ] `phase_publish` root-crate branch — the `if [[ "$crate" == "yosh" ]]` special case (bare `cargo publish` for root vs `cargo publish -p` for members) can be simplified to uniform `cmd=(cargo publish -p "$crate")` since cargo accepts `-p` on root crates too (`.claude/skills/release/scripts/release.sh`)
- [ ] `release.sh test` wall-time variance observation — after per-test-binary parallelization (2026-04-23), 3 back-to-back runs measured 95 s / 162 s / 178 s (±22 %, exceeds nominal ±20 % stability threshold). Root cause: `cargo test --no-run --workspace` incremental-check time varies with filesystem cache state (run 1 benefits from peak warmth). Not a correctness issue. If CI-based benchmarking is added, introduce a warm-up run before timed measurements to reduce first-run bias (`.claude/skills/release/scripts/release.sh`).
- [ ] `release.sh test` critical path is e2e (~86 s of a ~90 s warm-run parallel window) — the 2026-05-22 "~40 min" slowness was fixed by the 2026-07-11 per-job precompile change; measured 2026-07-19 with the new per-job instrumentation: 207 s semi-warm / 90 s fully warm. Remaining (low-priority) optimization: shard `e2e/run_tests.sh` (~700 sequential tests, each spawns perl+yosh) into N parallel buckets to shrink the window toward the next-longest job (`pty_posix`, ~58 s incl. lock wait). Only worth doing if the suite grows enough that the per-job table (printed at end of every run) shows e2e drifting past ~2-3 min (`.claude/skills/release/scripts/release.sh`, `e2e/run_tests.sh`).
