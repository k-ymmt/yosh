# TODO

## E2E XFAIL Roadmap Follow-ups

Roadmap closed 2026-05-17. Non-blocking follow-ups from SP1–SP6
retained below for tracking.

### SP1 follow-ups (non-blocking)

- [ ] `exec_function_call` does not clear `env.exec.loop_depth` on entry, so `break`/`continue` inside a function called from a loop affects the caller's loop. Matches dash; bash treats it as out-of-loop. Decide intent and either save/restore `loop_depth` on function entry or document the deviation (`src/exec/function.rs`).
- [ ] `loop_depth` bump/restore in `exec_for` / `exec_loop` is not panic-safe — `_inner` panics would skip the decrement. Currently fine because yosh uses `Result`, but a small `LoopDepthGuard` RAII drop guard would harden it (`src/exec/compound.rs`).
- [ ] `exit_child` doc comment (`src/exec/mod.rs:24`) says "Use ONLY after fork() in the child branch, never in the shell parent", but SP1 G5b added a top-level non-interactive call site in `src/exec/simple.rs` (BuiltinKind::Special redirect-error). Either update the doc to permit non-interactive shell exit, or introduce a dedicated `exit_shell(status)` helper.
- [ ] `builtin_exec` absolute-path branch (`cmd.contains('/')`) has no dedicated unit/e2e test. `exec_keeps_env.sh` covers the PATH-walk branch only. Add a focused test like `export m=v; exec /bin/sh -c 'echo $m'` (`src/builtin/special.rs::builtin_exec`).
- [ ] `export -p foo=v` silently drops `foo=v` operand (the `-p` branch prints and returns). Pre-existing, made more visible by SP1 G2's stricter validation. Either accept operands after `-p` or document the limitation (`src/builtin/special.rs::builtin_export`).
- [ ] `export -- foo=v` and `readonly -- foo=v` now report `--` as not a valid identifier (visible regression after SP1 G2's strict gate). Should consume `--` as POSIX end-of-options before validation (`src/builtin/special.rs::builtin_export`, `::builtin_readonly`).
- [ ] `e2e/posix_spec/8_env_vars/PATH_search.sh` and `e2e/builtin/job_spec_prefix.sh` intermittently TIMEOUT under full-suite load (pass standalone). Observed twice during SP1 closure runs. Likely fork/wait timing under contention; investigate or bump per-test timeout (`e2e/run_tests.sh`).

### SP2 follow-ups (non-blocking)

- [ ] Migrate remaining variable-setting call sites to `env.assign_var` so
      PATH cache invalidation is total. Pending paths:
      `${var:=value}` in `src/expand/param.rs:75`; arithmetic assignment in
      `src/expand/arith.rs:591,603`; `for` loop variable in
      `src/exec/compound.rs:229`; plugin-set variables in
      `src/plugin/host/variables.rs:19,35`. Each path could in principle
      set `PATH` but no current XFAIL test exercises it.
- [ ] `hash` listing format omits `hits=N` count. POSIX leaves the
      format implementation-defined; bash includes hit counts. Track
      hit counts on the cache entries if a tooling consumer asks
      (`src/builtin/hash.rs`).
- [ ] `command -V` (`src/builtin/command.rs::render_verbose`) does not
      escape single quotes in alias values while native `type`
      (`src/builtin/type.rs::format_type_line`) does. Pre-existing
      escape gap surfaced during SP2 G2 code review. Either align
      `render_verbose` with the escaping in `type`, or extract a shared
      `format_alias_value` helper for both call sites.
- [ ] `ShellEnv.utility_hash` field visibility is `pub` (`src/env/mod.rs`);
      tighten to `pub(crate)` so only `assign_var`/`unset_var`/`hash`
      builtin and `find_in_path` reach into it.
- [ ] `lookup_in_path` checks `cmd.contains('/')` twice (once at the
      cache-hit guard, once before auto-insert in the PATH walk loop)
      while `find_in_path` checks once at the top. Symmetrize for
      readability (`src/exec/command.rs`).
- [ ] The `BuiltinKind::NotBuiltin` arm in `format_type_line`
      (`src/builtin/type.rs`) and `render_verbose`
      (`src/builtin/command.rs`) is dead code with a `// Cannot happen`
      comment. Replace with `unreachable!("…")` so the contract is
      compile-checked.

### SP3 follow-ups (non-blocking)

- [ ] Word-splitting applied to literal command argv tokens — uncovered
      during SP3 Task 5 manual smoke. `IFS=:; echo a::b` produces three
      args (`a`, ``, `b`) joined by `echo` into `a  b`, instead of the
      single POSIX-literal `a::b`. POSIX XCU §2.6.5 restricts field
      splitting to results of parameter/command/arithmetic expansion,
      not to literal text. The `printf "a::b\n" | { read ... }` variant
      works correctly because the IFS is consumed by `read`, not by the
      expander on the literal. Likely fix in `src/expand/field_split.rs`
      or in the simple-command expansion path before `field_split::split`
      is called. Verified `read` itself is unaffected.
- [ ] `split_fields` terminator-consume branches are mildly redundant
      (`src/builtin/read.rs:200-213`). The `is_sep` and `else` (ws-only)
      branches both end with the same `while is_ws(...)` consume loop;
      flattening to "consume one sep byte if present, then consume any
      ws-IFS run" makes the POSIX rule clearer. Pure refactor.
- [ ] `split_fields` test coverage gaps surfaced by Task 4 code review
      (`src/builtin/read.rs`):
      - N=1 with sep-only IFS (leading `:` not trimmed)
      - N=3 with collapsed multi-space run (`a   b   c`)
      - Escaped leading sep-IFS byte (`\<:>` stays in field 1)
      Each is a 3–5 line test; not blocking because spec coverage is
      adequate via the existing 11 tests.
- [ ] `assert!(n_vars >= 1)` at `src/builtin/read.rs:140` could drop to
      `debug_assert!` since the only caller (`builtin_read`) guarantees
      `var_names.len() >= 1` via `ArgError::NoVarName`. Saves a tiny
      runtime check; matches the `debug_assert_eq!(result.len(), n_vars)`
      at the function tail.
- [ ] Partial-variable-assignment state on readonly error
      (`src/builtin/read.rs:43-49`): if the Kth variable in
      `read x y z` is readonly, `x` and `y` are already assigned before
      the error fires for `z`. POSIX leaves this implementation-defined;
      bash exhibits the same behaviour. A short comment at the loop
      head acknowledging the contract would aid future readers, or
      pre-check all names against `env.vars.is_readonly` before any
      assignment lands.
- [ ] `StdinByteReader::read_byte` SAFETY comment imprecise
      (`src/builtin/read.rs:67-70`). Says "STDIN_FILENO is always open at
      process start" but does not address user-driven closure (e.g.,
      `exec 0>&-`). Process path is fail-safe in that case (`libc::read`
      returns -1/EBADF → `Err` → `yosh: read: <strerror>` and exit 1), but
      the comment overstates the precondition. Either tighten to "fd 0
      is a valid file descriptor at the time of the syscall — if it has
      been explicitly closed by the user, `libc::read` returns EBADF
      which we propagate as `Err`", or note the fail-safe behaviour
      alongside the existing text. Surfaced during SP3 final review.

### SP4 follow-ups (non-blocking)

- [ ] User-reset semantics for `OPTIND` — POSIX `getopts` allows
      restarting argument parsing by setting `OPTIND=1` mid-iteration
      (XCU `getopts` rationale). Current native impl does NOT detect
      the user writing `OPTIND="1"` when the previous call left
      OPTIND already at "1" (e.g. mid-stacked `-ab`); state therefore
      continues stacking. Workaround: scripts can write `OPTIND=2`
      followed by `OPTIND=1` to force a detected change, but this is
      not POSIX. Proper fix requires either (a) hooking
      `VarStore::set("OPTIND", _)` to reset `getopts_subindex` (rejected
      during SP4 for layering reasons), or (b) tracking
      `last_optind_written` on the scope. None of the 9 SP4 XFAIL
      targets exercised this; closed test
      `builtin_user_resets_optind_to_one` dropped from `src/builtin/getopts.rs`
      tests as out-of-scope. Pre-decided during SP4 final review.
- [ ] `Scope.saved_optind` restore path (`src/env/vars.rs::pop_scope`)
      silently discards `set()` errors with a stale rationale comment
      ("Readonly OPTIND is not supported and the assignment cannot fail
      in practice"). User scripts CAN make OPTIND readonly via
      `readonly OPTIND=5`, in which case `set` returns Err on pop.
      Behavior is still correct (the readonly value IS the saved one,
      so the discard is a no-op), but the comment overstates the
      precondition. Tighten to acknowledge the readonly-OPTIND fail-safe.
      Code-review follow-up from SP4 Task 1.
- [ ] Edge-case test coverage: nested function-call OPTIND save/restore
      (push → push → set OPTIND → pop sees inner saved → pop sees outer
      saved), and readonly-OPTIND push/pop round-trip. Both are real
      script patterns. Code-review follow-up from SP4 Task 1.
- [ ] Inherited-environment OPTIND override test: if the parent process
      exports `OPTIND=5`, `ShellEnv::new` correctly overwrites to `"1"`
      per POSIX, but no test covers this path. Add a focused unit test
      in `src/env/mod.rs`. Code-review follow-up from SP4 Task 2.
- [ ] `OPTARG` is unconditionally overwritten to empty string in
      `builtin_getopts` even on end-of-options (exit 1) and on known
      options with no argument. POSIX says OPTARG is unspecified at
      end-of-options; bash and dash leave the previous value in place,
      which is friendlier for scripts that inspect OPTARG after the
      loop. Guard the OPTARG write on `step.optarg.is_some()` (or split
      the end-of-options branch to skip the write entirely)
      (`src/builtin/getopts.rs:74-75`). Final-review follow-up from
      SP4 Task 6.
- [ ] `let _ = env.assign_var(parsed.var_name, …)` and the two
      following `assign_var` calls in `builtin_getopts` silently
      discard readonly errors. If the user has `readonly opt`, calling
      `getopts a opt` advances OPTIND without assigning `opt`, leaving
      state inconsistent. POSIX does not mandate the behavior; bash
      emits `getopts: opt: readonly variable` and returns non-zero.
      Either capture the Err and surface a diagnostic + rc=2, or
      pre-check `is_readonly` before any side effects
      (`src/builtin/getopts.rs:73-76`). Final-review follow-up from
      SP4 Task 6.
- [ ] `step_getopts` casts a stack byte to `char` via `bytes[cursor]
      as char`, which silently misinterprets non-ASCII UTF-8 bytes
      (e.g. `-é` yields the byte `0xC3` as a char). POSIX option chars
      are ASCII letters/digits so practical inputs hit the unknown-
      option branch and exit safely, but the cast obscures the
      intent. Switch to `char::from_u32(bytes[cursor] as u32)` (with
      a fallback) or add a `// ASCII spec only` doc-comment at the
      cast site (`src/builtin/getopts.rs:139-140`). Final-review
      follow-up from SP4 Task 6.

### SP5 follow-ups (non-blocking)

- [ ] PS4 variable / arithmetic / command-sub expansion not implemented.
      `set -x` currently emits the raw PS4 value as a literal prefix,
      so `PS4='+ $LINENO> '` shows `$LINENO` verbatim instead of the
      line number. POSIX leaves PS4 expansion implementation-defined;
      bash performs full parameter expansion before printing. Add a
      param/arith expansion pass on PS4 in `exec_simple_command`'s
      xtrace branch (`src/exec/simple.rs`). Code-review follow-up
      from SP5 T1.
- [ ] PS4 first-character-repeat rule for nesting depth — POSIX-
      compatible shells repeat the first byte of PS4 N times where N
      is the depth of nested function / source invocations. Not
      implemented; SP5 T1 emits PS4 verbatim. Add nesting-depth
      tracking on `ShellEnv` and repeat the first char in
      `xtrace_prefix`. Final-review follow-up from SP5 T1.
- [ ] `word_has_command_sub` returns true for `WordPart::ArithSub` even
      though arithmetic expansion does not update `last_exit_status`.
      For an assignment-only command consisting only of `$((expr))`,
      yosh's new T3 logic now seeds `last_cmd_sub_status` from the
      previous command's `$?`, which is a behavioural regression from
      "0 on entry to arithmetic-only". POSIX leaves it implementation-
      defined; bash returns 0 in this case. Either split
      `word_has_command_sub` into a CmdSub-only predicate, or document
      the divergence (`src/exec/simple.rs:819`). Code-review follow-up
      from SP5 T3.
- [ ] Pipeline-child EXIT trap firing — `exec_subshell`'s child branch
      now fires `execute_exit_trap` (SP5 T6) but `exec/pipeline.rs`'s
      pipeline-member child branches still call `exit_child` directly
      without firing the trap. POSIX permits either interpretation;
      bash fires the trap on every pipeline member's exit while dash
      fires only on the rightmost. Pick a stance and apply uniformly,
      or document the asymmetry. Final-review follow-up from SP5 T6.
- [ ] `process_pending_signals` is now called at the tail of
      `exec_complete_command` (top level) but NOT inside `exec_body`
      iteration tails or `exec_function_call` returns. Async traps
      installed inside a long-running function or loop body therefore
      fire only after the function / loop completes (rather than
      between iterations / between statements inside the body). Add
      drain calls inside `exec_body`'s loop and `exec_function_call`'s
      tail if a use case surfaces, weighing the per-iteration cost.
      Code-review follow-up from SP5 T7.
- [ ] `x=1 myfunc() { :; }` (assignment prefix before function
      definition) silently drops the assignment instead of emitting
      `ParseErrorKind::UnexpectedToken`. yosh still errors via a
      downstream parser path ("empty compound list in subshell") so
      the user sees a syntax error, but the message is misleading.
      The clean fix is to detect this case in `parse_command` after
      restoring lexer state and emit an explicit syntax-error
      diagnostic (`src/parser/mod.rs`). Code-review follow-up from
      SP5 T2.
- [ ] E2E runner perl wrapper introduces a perl dependency. macOS /
      most Linux distros ship perl by default but the requirement is
      now part of the test infrastructure contract. Either add an
      explicit check at runner entry (`command -v perl >/dev/null ||
      exit 1`), document the dependency in CLAUDE.md, or replace the
      perl one-liner with a tiny C helper / Rust binary. Follow-up
      from SP5 T7.

### SP6 follow-ups (non-blocking)

- [ ] `fc` builtin with no operand (`fc`, `fc -e <editor>`) infinite-recurses
      and stack-overflows yosh. Root cause: `Repl::run` adds the running
      command to history via `executor.env.history.add(...)` BEFORE
      `exec_complete_command` (`src/interactive/mod.rs:268-272`), so bare
      `fc` resolves "previous command" to the fc command itself and
      `eval_string`s back into fc. POSIX explicitly says fc must not
      enter itself in history. Affects `tests/pty_posix.rs::fc::editor_dash_e`
      and `tests/pty_posix.rs::fc::no_args_uses_editor`, which currently
      work around it by passing an `echo` prefix operand. Fix: hoist the
      `history.add` call after `exec_complete_command`, or have `fc`
      itself filter its own command from the history slice it operates on.
- [ ] `tests/helpers/pty.rs::read_until_prompt` regex `\$ ` mis-matches
      yosh's syntax-highlight repaint output, which emits a transient
      `$ <partial>` after every keystroke. `capture_until_sentinel`
      (added in SP6 T4, promoted in T5) is the workaround. Long-term, a
      raw-mode-aware capture primitive that recognizes the line-editor's
      repaint pattern would let `read_until_prompt` work everywhere
      (`tests/helpers/pty.rs`).
- [ ] `exec >file` followed by `capture_until_sentinel` hangs because the
      sentinel's `echo __YOSH_DONE__` lands in the file rather than
      back at the PTY. SP6 T6 (`tests/pty_posix.rs::exec_redirect::no_cmd_redirects`)
      works around this by fusing the entire redirect sequence into one
      command line so stdout is restored before the sentinel fires. If
      more tests need step-wise interaction across a `exec >file` boundary,
      add a `capture_until_sentinel_via_stderr` variant that uses
      `>&2 echo __YOSH_DONE__` so the sentinel travels on fd 2 even when
      fd 1 is redirected (`tests/helpers/pty.rs`).

### 2026-05-19 trap-reset follow-ups (non-blocking)

- [ ] `tests/subshell.rs` の trap-reset 統合テスト 3 件
      (`test_nested_subshell_inside_cmdsub_shows_reset_traps`,
      `test_pipeline_child_clears_saved_traps`,
      `test_background_async_clears_saved_traps`) がファイル末尾にあり、
      意味的に近い `test_cmdsub_trap_isolation`
      (`tests/subshell.rs:237` 周辺) から離れている。次回 subshell.rs を
      触る時にコマンドサブセクションへ寄せる。
      Code-review follow-up from f703a26.
- [ ] `src/env/traps.rs::tests` の `.unwrap()` と `.expect("...")` が
      不統一 — `test_reset_for_subshell_*` 系は `.unwrap()`、
      `test_set_trap_with_*` 系も `.unwrap()` のメッセージなし。
      失敗時のデバッグ性を上げるため `.expect(...)` で揃える。
      Code-review follow-up from f703a26.
- [ ] `reset_for_subshell` が `Command` 種 `exit_trap` をクリアする
      ことを直接検証するユニットテストがない。`reset_non_ignored`
      の `exit_trap` クリア挙動は `test_trap_store_reset_non_ignored`
      が間接的にカバーするのみ。`reset_for_subshell` 経由の同等カバレッジ
      を追加すると安心。Code-review follow-up from f703a26.

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

- [ ] `ulimit [-f] [num]` — resource-limit query/set. Currently uses
      `/usr/bin/ulimit`. XFAIL tests:
      `e2e/posix_spec/4_required_builtin/ulimit_*.sh` (1 of 3 remains XFAIL
      — unknown-option case)

## Future: Release Skill Enhancements

- [ ] `phase_push` remote tag upsert — currently only checks local tag existence; if the same tag already exists on origin, `git push origin <tag>` rejects. Add `git ls-remote --exit-code --tags origin <tag>` check before pushing (`.claude/skills/release/scripts/release.sh`)
- [ ] `test_plugin/Cargo.toml` version lag risk — `tests/plugins/test_plugin` is a workspace member but not in the `phase_bump` manifests list (not publishable). Currently safe because it depends on workspace crates only via `path =`; breaks if it ever adds `version = "..."` pins (`.claude/skills/release/scripts/release.sh`)
- [ ] `phase_publish` root-crate branch — the `if [[ "$crate" == "yosh" ]]` special case (bare `cargo publish` for root vs `cargo publish -p` for members) can be simplified to uniform `cmd=(cargo publish -p "$crate")` since cargo accepts `-p` on root crates too (`.claude/skills/release/scripts/release.sh`)
- [ ] `release.sh test` wall-time variance observation — after per-test-binary parallelization (2026-04-23), 3 back-to-back runs measured 95 s / 162 s / 178 s (±22 %, exceeds nominal ±20 % stability threshold). Root cause: `cargo test --no-run --workspace` incremental-check time varies with filesystem cache state (run 1 benefits from peak warmth). Not a correctness issue. If CI-based benchmarking is added, introduce a warm-up run before timed measurements to reduce first-run bias (`.claude/skills/release/scripts/release.sh`).
