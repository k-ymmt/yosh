# TODO

## Audit 2026-07-02: Security / Correctness / Performance

Findings from a three-lens codebase audit (security, POSIX correctness,
performance). Behavioral items were verified against the debug binary on
2026-07-02; code-pinned items cite the exact offending line. Items already
tracked elsewhere (break/continue `loop_depth` across functions — see the
SP1 follow-up above; plugin `is_symlink` — see the plugin follow-up below)
are intentionally omitted here. The Security / Robustness findings from
this audit were all resolved on 2026-07-03.

### Correctness (POSIX deviations)

- [ ] Unquoted `$*` joins positional parameters with a hardcoded space instead
      of the first char of IFS, so `IFS=:; set a b c; for x in $*` yields the
      single field `a b c` instead of three fields (verified) (`src/expand/param.rs:171`).
- [ ] The word argument of `${x:-w}` / `${x:=w}` / `${x:?w}` / `${x:+w}` is
      expanded via `expand_word_to_string`, discarding quote metadata; an
      unquoted `${x:-"a b"}` then field-splits into `a`,`b` instead of the
      single field POSIX requires (verified: `set -- ${x:-"a b"}` gives `$#`=2)
      (`src/expand/param.rs:54`).
- [ ] `set -u` (nounset) is only enforced for `ParamExpr::Simple`; expanding an
      unset positional (`$1`) or special parameter yields empty with no
      error/exit (verified: `set -u; echo "[$1]"` → `[]` exit 0), whereas POSIX
      treats it as an expansion error (`src/expand/param.rs:22`).
- [ ] Unquoted here-document bodies only do a plain `${name}` lookup;
      conditional (`${x:-def}`), length (`${#x}`), and strip forms are not
      applied, so `${x:-default}` in a heredoc expands to empty
      (`src/expand/heredoc.rs:62`).
- [ ] Under `set -e`, a `!`-negated pipeline still triggers errexit exit
      (verified: `set -e; ! true` terminates before the next command), but POSIX
      exempts pipelines beginning with `!` (`src/exec/control.rs:183`).
- [ ] Under `set -e`, a nonzero result from a non-final component of an AND-OR
      list triggers errexit exit (verified: `set -e; false && true` terminates),
      but only the last command of the list is subject to `-e`
      (`src/exec/control.rs:183`).
- [ ] In monitor mode (`set -m`) a multi-command pipeline's exit status comes
      from the last process *reaped* (completion order) rather than the last
      command in the pipeline, so `set -m; sleep 0.3 | false` reports `$?`=0
      instead of 1 (verified) (`src/exec/pipeline.rs:151`).
- [ ] `test`/`[` 3-argument parsing checks `!` negation (and `( )` grouping)
      before checking whether `$2` is a binary primary, so `[ ! = x ]` errors
      with "unknown operator" (exit 2) instead of comparing the strings `!` and
      `x` (exit 1) per POSIX §2.14 (verified) (`src/builtin/test.rs:63`).
- [ ] `exit`/`return` with a non-numeric argument returns an error without
      terminating, so `exit foo; echo after` prints the diagnostic and then runs
      `after` (verified); POSIX requires termination with status 2
      (`src/builtin/special.rs:67`).
- [ ] Reserved words (`fi`, `done`, `then`, `else`, `elif`, `do`, `esac`, `}`)
      are accepted as ordinary command names in command position, so bare `done`
      runs "command not found" instead of the syntax error POSIX §2.4 requires
      (verified) (`src/parser/simple.rs:22`).
- [ ] A trailing pipe or logical operator with no following command (`echo hi |`,
      `true &&`) is accepted and builds a pipeline with a phantom empty command
      instead of a syntax error (verified: `echo hi |` exits 0)
      (`src/parser/simple.rs:63`).
- [ ] The `for … in` word list is terminated on `do` even without a preceding
      `;`/newline, so `for i in a b do echo x; done` is misparsed (loops over
      `a b`, verified prints `x x`) instead of raising a syntax error
      (`src/parser/compound.rs:167`).

#### Byte-semantics instances (concrete cases for the POSIX Byte Semantics work below)

- [ ] Here-document body expansion emits ordinary bytes via `bytes[i] as char`,
      decoding each UTF-8 byte as Latin-1; a heredoc containing `日本語` is
      corrupted to mojibake (`src/expand/heredoc.rs:170`).
- [ ] `read` builds field/remainder strings with `b.value as char`, decoding raw
      input bytes as Latin-1; non-ASCII input like `café` is stored corrupted
      (`src/builtin/read.rs:265` and lines 247, 282, 308).
- [ ] `$'\xHH'` / `$'\NNN'` octal build a `char` from the numeric value, so
      `$'\xe9'` produces the two UTF-8 bytes of U+00E9 rather than the single
      byte `0xe9` that POSIX/other shells emit (`src/lexer/word.rs:582`).
- [ ] Command substitution reads child output with `read_to_string`, so output
      containing an invalid-UTF-8 byte is discarded entirely with a
      "stream did not contain valid UTF-8" error instead of captured as bytes
      (`src/expand/command_sub.rs:85`).
- [ ] Pathname expansion lists directory entries via `to_string_lossy`, so files
      with non-UTF-8 names cannot be matched or are returned corrupted
      (`src/expand/pathname.rs:173`).

### Performance

- [ ] PERF: `redraw`'s diff-based partial repaint (added to replace the
      always-full clear+repaint) only engages when both the previous and
      the new render fit on a single terminal row; any input that wraps to
      multiple rows still falls back to a full clear+repaint every
      keystroke. Extending the partial-repaint path to the multi-row case
      would need wrapped-row-aware cursor positioning
      (`src/interactive/line_editor.rs` `redraw`).

## Future: POSIX Byte Semantics

- [ ] Complete full non-UTF-8 shell input, argv, paths, and environment value
      support. Stage 1 established byte-oriented expansion-field APIs and
      regression tests around byte-index split/glob protection, plus a
      centralized UTF-8 `CString` boundary for external exec. Remaining work:
      migrate shell source input away from `read_to_string`; move AST word
      storage, variables, positional parameters, aliases, traps, and functions
      toward byte buffers plus quote/protection metadata; carry `OsString` or
      raw bytes through paths and process boundaries; and decide plugin API byte
      semantics. Keep this open until invalid UTF-8 data is preserved end to end.

## E2E XFAIL Roadmap Follow-ups

Roadmap closed 2026-05-17. Non-blocking follow-ups from SP1–SP6
retained below for tracking.

### SP1 follow-ups (non-blocking)

- [ ] `tests/cli_help.rs` CLICOLOR_FORCE tests fail standalone:
      `help_color_forced_with_clicolor_force` and
      `help_clicolor_force_overrides_clicolor_zero` do not observe ANSI
      escapes even with `CLICOLOR_FORCE=1`. Discovered during 2026-06-03
      POSIX byte semantics stage-1 verification; unrelated to the byte
      semantics change set.
- [ ] `exit_child` doc comment (`src/exec/mod.rs:24`) says "Use ONLY after fork() in the child branch, never in the shell parent", but SP1 G5b added a top-level non-interactive call site in `src/exec/simple.rs` (BuiltinKind::Special redirect-error). Either update the doc to permit non-interactive shell exit, or introduce a dedicated `exit_shell(status)` helper.
- [ ] `builtin_exec` absolute-path branch (`cmd.contains('/')`) has no dedicated unit/e2e test. `exec_keeps_env.sh` covers the PATH-walk branch only. Add a focused test like `export m=v; exec /bin/sh -c 'echo $m'` (`src/builtin/special.rs::builtin_exec`).
- [ ] `export -p foo=v` silently drops `foo=v` operand (the `-p` branch prints and returns). Pre-existing, made more visible by SP1 G2's stricter validation. Either accept operands after `-p` or document the limitation (`src/builtin/special.rs::builtin_export`).
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
- [ ] `command -p` shares `utility_hash` while searching the *default*
      PATH (`src/exec/simple.rs:932`), so a `command -p foo` hit can be
      reused by a later plain `foo` lookup under a different `$PATH` —
      the same cache-key mismatch fixed for `PATH=dir cmd` prefix
      overrides in edb5254 (which resolves uncached). Route `command -p`
      through `lookup_in_path_uncached`, or key cache entries on the
      PATH value used. Surfaced by the 2026-07-04 perf-branch final
      review; pre-existing at base 31ef66b.

### SP3 follow-ups (non-blocking)

- [ ] `split_fields` test coverage gaps surfaced by Task 4 code review
      (`src/builtin/read.rs`):
      - N=1 with sep-only IFS (leading `:` not trimmed)
      - N=3 with collapsed multi-space run (`a   b   c`)
      - Escaped leading sep-IFS byte (`\<:>` stays in field 1)
      Each is a 3–5 line test; not blocking because spec coverage is
      adequate via the existing 11 tests.
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

- [ ] `getopts` OPTIND reset implementation verification is pending:
      `cargo test` / `cargo check` repeatedly hung while compiling
      `yosh-plugin-manager` (`rustc` sleeping at 0% CPU) during the
      2026-05-30 implementation session. No stuck cargo/rustc process
      was left running. Re-run `cargo test -p yosh --lib env::vars::tests`,
      `cargo test -p yosh --lib builtin::getopts::tests`, `cargo build`,
      and `./e2e/run_tests.sh --filter=getopts_optind_reset_stacked`
      once the build hang is resolved.
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

- [ ] POSIX-strict reading of "fc shall not be entered into the history list"
      not implemented. After the 2026-05-23 hoist of `history.add` past
      `exec_complete_command`, the fc command itself still ends up in
      history (just delayed). Up-arrow navigation surfaces the fc invocation,
      which matches user mental models but deviates from POSIX rationale.
      Future implementation: lightly parse `cmd_text` in `Repl::run` and
      skip `history.add` when the command starts with `fc ` (or is bare
      `fc`). Trade-off: up-arrow can no longer recall the fc invocation
      (`src/interactive/mod.rs`).
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
- [ ] `reset_for_subshell` が `Command` 種 `exit_trap` をクリアする
      ことを直接検証するユニットテストがない。`reset_non_ignored`
      の `exit_trap` クリア挙動は `test_trap_store_reset_non_ignored`
      が間接的にカバーするのみ。`reset_for_subshell` 経由の同等カバレッジ
      を追加すると安心。Code-review follow-up from f703a26.

### 2026-05-21 locale-support follow-ups (non-blocking)

- [ ] `LocaleCategory::env_var_name` is `fn` (private). When a
      future caller needs the variable name string (e.g., diagnostic
      messages referring to "LC_CTYPE"), promote to `pub(crate)`.
      Code-review follow-up from 2026-05-21 locale-support branch
      (`src/env/locale.rs`).
- [ ] `each_category_reads_its_own_var` test verifies only `value`,
      not `source`. A regression that swaps the LC_<category> and
      LANG branches in `resolve()` would still pass this test. Add
      `assert_eq!(r.source, LocaleSource::LcCategory)` to each of
      the six assertions. Code-review follow-up from 2026-05-21
      locale-support branch (`src/env/locale.rs`).
- [ ] `ResolvedLocale` derives only `Clone, Debug`. Add `PartialEq`
      (and possibly `Eq`) for symmetry with `LocaleCategory` and
      `LocaleSource`. Cheap, but YAGNI until a caller wants struct
      equality. Code-review follow-up from 2026-05-21 locale-support
      branch (`src/env/locale.rs`).
- [ ] `pattern.rs` test naming inconsistency — existing tests use
      `test_*` prefix (e.g. `test_bracket_set`), new POSIX-class
      tests use bare names (e.g. `class_alpha_matches_letter`).
      Unify in a future cleanup. Code-review follow-up from
      2026-05-21 locale-support branch (`src/expand/pattern.rs`).
- [ ] No unit test exercises multiple POSIX classes in one bracket
      (`[[:alpha:][:digit:]]`). Manually traced as correct, but a
      regression test would solidify the behaviour. Code-review
      follow-up from 2026-05-21 locale-support branch
      (`src/expand/pattern.rs`).
- [ ] `docs/yosh/posix-compliance.md` LC_COLLATE description says
      "Unicode codepoint ordering coincides with C-locale bytewise
      ordering". Strictly true only in the ASCII range; UTF-8 byte
      order and codepoint order diverge for non-ASCII characters
      (e.g. multi-byte sequences sort by leading-byte value, which
      matches codepoint order for U+0080-U+07FF but diverges in
      detail). yosh's `str::cmp` is bytewise (per `src/builtin/test.rs`
      comment), so the doc could tighten to "yosh uses bytewise
      comparison on UTF-8 encoding, which equals C-locale ordering
      on ASCII strings."
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
- [ ] Test gap: no direct assertion that the
      `append_char (true, false) → push_literal` arm preserves
      per-byte attributes through field splitting. Add a unit test
      like `f.push_literal("a*"); f.push_expanded(" b");` with
      `IFS=" "`, split, then assert `is_glob_protected(0)=false` on
      the resulting first field so a future regression that routed
      this byte through `push_expanded` (value would still match,
      but downstream pathname expansion would glob a literal byte)
      is caught (`src/expand/field_split.rs::tests`). Final-review
      follow-up from the branch (4366bc9..6d57c52).
- [ ] Test gap: UTF-8 multi-byte content through `push_literal` is
      uncovered. `push_expanded` and `push_quoted` already have
      multi-byte UTF-8 tests (`test_utf8_*`, `test_utf8_quoted_not_split`);
      add `push_literal("日本")` + IFS-splittable expansion mix for
      symmetry. Cheap (`src/expand/field_split.rs::tests`). Final-review
      follow-up from the branch.
- [ ] `literal_glob_metachar_still_globs.sh` (`e2e/posix_spec/2_06_05_field_splitting/`)
      silently relies on `mktemp -d` succeeding. If it ever fails,
      `d` is empty, `cd ""` lands in `$HOME`, and `echo *.tmpext`
      prints the literal `*.tmpext` — the test still fails loudly
      via EXPECT_OUTPUT mismatch, but with a misleading reason. Add
      `d=$(mktemp -d) || exit 1` to surface the real failure. Code-review
      follow-up from Task 3.
- [ ] `e2e/run_tests.sh` heredoc EXPECT_OUTPUT parser (lines
      204-211) silently strips a leading `"# "` from each body line.
      If a future contributor writes a body line as bare `#` (no
      trailing space) the strip leaves the `#` and silently
      mismatches. Either add a defensive parser check ("body lines
      must start with `# `") or document the convention prominently
      at runner entry. Code-review follow-up from Task 3.
- [ ] `literal_glob_metachar_still_globs.sh` POSIX_REF cites only
      `2.6.5 Field Splitting`, but the test exercises the
      interaction with `2.6.6 Pathname Expansion`. Extend to
      `POSIX_REF: 2.6.5 Field Splitting (interaction with 2.6.6
      Pathname Expansion)` for discoverability — a contributor
      browsing by pathname-expansion reference would otherwise miss
      this test (`e2e/posix_spec/2_06_05_field_splitting/literal_glob_metachar_still_globs.sh`).
      Code-review follow-up from Task 3.

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
- [ ] Multiline editing — visual multiline editing with cursor movement across lines
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
- [ ] `render_verbose` Function arm has no unit test — `command -V <function>` branch exercised only through E2E; add a focused unit test in `src/builtin/command.rs` tests module
- [ ] `preview_command` has no direct unit tests — only exercised via E2E; add focused tests for compound-command / unexpandable-word fallback and pipeline first-command extraction (`src/exec/mod.rs`)
- [ ] `highlight_scanner` `KEYWORDS` duplicates POSIX §2.4 list — `src/interactive/highlight_scanner/helpers.rs` defines its own copy of the 16 reserved words, separate from the canonical `crate::lexer::reserved::RESERVED_WORDS`. Consolidate once the contextual subsets (`COMMAND_POSITION_KEYWORDS` includes `"time"`, command-position restoration logic) are re-expressed in terms of the canonical list (`src/interactive/highlight_scanner/helpers.rs`)
- [ ] `cargo fmt --check -- <path>` misreads edition — rustfmt 1.8.0 / Rust 1.94.1 fails to parse let-chain syntax as edition 2024 when invoked with explicit file paths despite `Cargo.toml` specifying `edition = "2024"`, producing spurious fmt errors. Workaround: invoke `rustfmt --edition 2024 --check <path>` directly. Revisit when upstream rustfmt catches up.
- [ ] `parse_compound_list` non-empty regression tests are incomplete — only `nonempty_if_parses_ok` exists in `src/parser/compound.rs`. Add parallel `nonempty_while_parses_ok` / `nonempty_until_parses_ok` / `nonempty_for_parses_ok` / `nonempty_brace_group_parses_ok` / `nonempty_subshell_parses_ok` so future refactors cannot accidentally over-reject any individual context.
- [ ] LINENO update allocates a `String` per command — `exec_simple_command` / `exec_compound_command` call `cmd.line.to_string()` and go through `VarStore::set`. For tight loops this is ~500μs per 10k commands. If benchmarks ever show pressure, add `ShellEnv.exec.current_lineno: usize` and intercept `$LINENO` in `expand::param` to read that field directly, bypassing the alloc + HashMap write (`src/exec/simple.rs`, `src/exec/compound.rs`, `src/expand/param.rs`).
- [ ] `pattern::matches` bracket set-member multibyte test gap — `src/expand/pattern.rs` tests cover a multibyte char as a bracket **range endpoint** (`matches("[あ-ん]", "か")`) but not as a plain **set member** (e.g. `matches("[あいう]", "い")`). No reachable bug — the set-member path reuses the same `BracketItem::Char(c0)` char decoding the range test exercises — but an explicit case would make `parse_bracket`'s loop intent clearer. Cosmetic test-coverage follow-up from the 2026-05-27 `&str` matcher rewrite final review.
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
- [ ] `indirection_level_balanced_after_dot_script*` tests use `tempfile::NamedTempFile` while the sibling `source_file_*` tests use `std::env::temp_dir()` + `std::fs::write`/`remove_file`. The `NamedTempFile` style is cleaner (auto-cleanup) — unify the `source_file_*` tests onto it in a future pass for consistency (`src/exec/mod.rs`). Code-review follow-up from 2026-05-28 PS4 full-support branch.

## Future: Release Skill Enhancements

- [ ] `phase_push` remote tag upsert — currently only checks local tag existence; if the same tag already exists on origin, `git push origin <tag>` rejects. Add `git ls-remote --exit-code --tags origin <tag>` check before pushing (`.claude/skills/release/scripts/release.sh`)
- [ ] `test_plugin/Cargo.toml` version lag risk — `tests/plugins/test_plugin` is a workspace member but not in the `phase_bump` manifests list (not publishable). Currently safe because it depends on workspace crates only via `path =`; breaks if it ever adds `version = "..."` pins (`.claude/skills/release/scripts/release.sh`)
- [ ] `phase_publish` root-crate branch — the `if [[ "$crate" == "yosh" ]]` special case (bare `cargo publish` for root vs `cargo publish -p` for members) can be simplified to uniform `cmd=(cargo publish -p "$crate")` since cargo accepts `-p` on root crates too (`.claude/skills/release/scripts/release.sh`)
- [ ] `release.sh test` wall-time variance observation — after per-test-binary parallelization (2026-04-23), 3 back-to-back runs measured 95 s / 162 s / 178 s (±22 %, exceeds nominal ±20 % stability threshold). Root cause: `cargo test --no-run --workspace` incremental-check time varies with filesystem cache state (run 1 benefits from peak warmth). Not a correctness issue. If CI-based benchmarking is added, introduce a warm-up run before timed measurements to reduce first-run bias (`.claude/skills/release/scripts/release.sh`).
- [ ] `release.sh test` is slow — measured **~40 min** wall on 2026-05-22 (two back-to-back runs: 41 min / 40 min on warmed cache), well past the script's own "15-30 min" warning. Breakdown: `cargo build` ~45 s + `cargo test --no-run --workspace` ~2 m 26 s + parallel test phase ~36 min. The throttle change committed 2026-05-22 (`RUST_TEST_THREADS` free=4 / pty=2) did NOT meaningfully change wall-clock; both global=2 and per-group 4/2 runs landed within 1 min of each other, so the cost is in actual test execution, not the throttle. Likely bottleneck candidates to investigate: (1) e2e (~700 tests, sequential, each spawns perl+yosh; under cargo CPU competition each test's wall stretches well past its standalone ~100 ms); (2) the PTY serial chain (`signals` + `pty_interactive` + `pty_posix`, now mutex'd via the mkdir lock, each with internal `thread::sleep` waits); (3) `cargo test --doc -p yosh` rustdoc compile-per-doctest cost. Easy first probes: instrument each parallel job with `/usr/bin/time` and dump per-job durations to the log dir on success, then decide between (a) splitting e2e out of the parallel batch, (b) sharding e2e, or (c) running the PTY chain in its own phase before/after the free batch (`.claude/skills/release/scripts/release.sh`).
