# SP6 — PTY Harness Migration (10 tests)

**Date:** 2026-05-16
**Status:** Design (pending plan + implementation)
**Roadmap:** [`2026-05-13-e2e-xfail-roadmap-design.md`](2026-05-13-e2e-xfail-roadmap-design.md) §3 SP6
**Predecessor SPs:** SP1 / SP2 / SP3 / SP4 / SP5 (all complete)

## 1. Background

The E2E XFAIL roadmap §3 partitions 55 XFAIL tests into 7 sub-projects.
SP1+SP2+SP3+SP4+SP5 (42 tests) are complete; 13 XFails remain across
SP6–SP7. SP6 is the "PTY harness migration" bucket: 10 tests that
fundamentally cannot run under the non-interactive `e2e/run_tests.sh`
harness because they rely on interactive history, an editor process,
the default `PS1`, or `/dev/tty`.

The 10 tests fall into four categories:

- **`fc` builtin (6):** all require non-empty command history, which
  only the interactive REPL populates (`src/interactive/mod.rs::Repl::run`).
- **`FCEDIT` selection (2):** require launching an editor (`cat`,
  `/bin/ed`) as `fc`'s child process and verifying its exit status.
- **`PS1` default value (1):** requires the `PS1` variable to be set
  at interactive startup so `${PS1+x}` returns "x".
- **`exec` with redirects (1):** uses `/dev/tty`, which is only
  available when the shell runs under a pseudo-terminal.

`tests/pty_interactive.rs` (762 lines) already provides a robust
`expectrl`-based PTY harness for interactive tests (raw-mode detection,
prompt synchronization, temp-dir lifecycle). SP6 reuses this
infrastructure rather than building PTY support into the
non-interactive e2e runner.

## 2. Scope

In scope:

- Migrate 10 e2e shell tests to a new Rust PTY test file
  `tests/pty_posix.rs` using `expectrl`.
- Extend `e2e/run_tests.sh` to recognize a new `# MIGRATED_TO:`
  metadata directive that marks a shell test as superseded by a Rust
  test, displays it as `[MIGRATED]`, and accounts for it in the summary.
- Replace `# XFAIL: …` lines in the 10 e2e shell files with
  `# MIGRATED_TO: tests/pty_posix.rs::<test_name>` pointers, keeping
  POSIX_REF / DESCRIPTION / EXPECT_OUTPUT / EXPECT_EXIT metadata as
  reference (the shell file becomes a metadata stub).
- Extract reusable PTY helpers (`spawn_yosh`, `wait_for_prompt`,
  `TempDir`, etc.) from `tests/pty_interactive.rs` into
  `tests/helpers/pty.rs` so both files share one definition.
- Initialize `PS1` to its POSIX default (`"$ "` unprivileged / `"# "`
  privileged) at interactive shell startup when not inherited from the
  environment.
- Record non-blocking polish items in TODO.md as
  `### SP6 follow-ups (non-blocking)`.

Out of scope:

- Building PTY support into the non-interactive e2e runner (rejected
  during brainstorming because cost — new `expect` runtime dependency,
  bash-side prompt-synchronization re-implementation, ANSI/echo
  stripping, new metadata directives — is several times the Rust-side
  alternative for the same 10 tests).
- Deferred / known-deviation tests (SP7).
- New `fc` features beyond what the existing implementation already
  supports (`fc -lnrs`, `-e EDITOR`, default editor fallback).
- Refactoring `tests/pty_interactive.rs` beyond the helper extraction
  needed by SP6.

## 3. Target Tests

The 10 tests, grouped by category:

| # | Test file | Category | Rust test name |
|---|-----------|----------|----------------|
| 1 | `e2e/posix_spec/4_required_builtin/fc_l_lists_recent.sh` | fc | `fc::list_recent` |
| 2 | `e2e/posix_spec/4_required_builtin/fc_l_n_no_numbers.sh` | fc | `fc::list_no_numbers` |
| 3 | `e2e/posix_spec/4_required_builtin/fc_r_reverse.sh` | fc | `fc::list_reverse` |
| 4 | `e2e/posix_spec/4_required_builtin/fc_s_substitute.sh` | fc | `fc::substitute` |
| 5 | `e2e/posix_spec/4_required_builtin/fc_e_editor.sh` | fc | `fc::editor_dash_e` |
| 6 | `e2e/posix_spec/4_required_builtin/fc_no_command.sh` | fc | `fc::no_args_uses_editor` |
| 7 | `e2e/posix_spec/8_env_vars/FCEDIT_used_by_fc.sh` | FCEDIT | `fcedit::used_by_fc` |
| 8 | `e2e/posix_spec/8_env_vars/FCEDIT_default_ed.sh` | FCEDIT | `fcedit::default_ed` |
| 9 | `e2e/posix_spec/8_env_vars/PS1_default_value.sh` | PS1 | `ps1::default_value_set` |
| 10 | `e2e/posix_spec/4_special_builtin/exec_no_cmd_redirects.sh` | exec-redir | `exec_redirect::no_cmd_redirects` |

## 4. Architecture

### 4.1 Migration mechanism (`# MIGRATED_TO:`)

The non-interactive e2e runner gains a new metadata directive that
short-circuits execution and accounts for the test as "migrated":

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc -l lists recent history entries
# MIGRATED_TO: tests/pty_posix.rs::fc::list_recent
# EXPECT_EXIT: 0
fc -l >/dev/null
```

The shell body (`fc -l >/dev/null`) is preserved as historical reference
but never executed. `EXPECT_OUTPUT` / `EXPECT_EXIT` lines are likewise
retained as informal documentation of the expected behavior; they have
no runtime effect.

**Runner behavior:**

- Detects `# MIGRATED_TO: ` prefix during the metadata-parse loop
  (alongside the existing `# XFAIL: ` handling near
  `e2e/run_tests.sh:234`).
- If present, prints `[MIGRATED] <relpath> (<target>)` and increments
  a new `migrated` counter; does not execute the script.
- Final summary line appends `Migrated: N` between `XFail: N` and
  `Total: N` (`e2e/run_tests.sh:435` region).
- If both `# MIGRATED_TO:` and `# XFAIL:` are present, the runner
  emits a single `[WARN]` line per file (helps catch stale XFAIL
  comments that survived the migration commit) and proceeds with
  `MIGRATED_TO` semantics.

The mechanism is generic; future PTY migrations or movement to other
harnesses can reuse it without further runner changes.

### 4.2 Rust PTY tests (`tests/pty_posix.rs`)

A new test file at `tests/pty_posix.rs` collects the 10 Rust tests,
organized into four sub-modules matching the SP6 categories:

```
tests/pty_posix.rs
├── mod helpers (use crate::helpers::pty::*)
├── mod fc
│   ├── list_recent
│   ├── list_no_numbers
│   ├── list_reverse
│   ├── substitute
│   ├── editor_dash_e
│   └── no_args_uses_editor
├── mod fcedit
│   ├── used_by_fc
│   └── default_ed
├── mod ps1
│   └── default_value_set
└── mod exec_redirect
    └── no_cmd_redirects
```

Each test follows the same skeleton (borrowed from
`tests/pty_interactive.rs`):

```rust
#[test]
fn list_recent() {
    let (mut session, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut session);

    // Seed history.
    session.send_line("echo a").unwrap();
    wait_for_prompt(&mut session);
    session.send_line("echo b").unwrap();
    wait_for_prompt(&mut session);

    // Drive the test command.
    session.send_line("fc -l").unwrap();
    let out = read_until_prompt(&mut session);

    assert!(out.contains("echo a"));
    assert!(out.contains("echo b"));

    session.send_line("exit").unwrap();
    session.expect(Eof).unwrap();
}
```

### 4.3 Shared PTY helpers (`tests/helpers/pty.rs`)

The new helper module exposes the primitives that both
`tests/pty_interactive.rs` and `tests/pty_posix.rs` need. The contents
are moved verbatim out of `pty_interactive.rs`:

- `TIMEOUT`, `RAW_MODE_WAIT_TIMEOUT` constants.
- `TempDir` struct (unique-per-test temp directory, dropped on test end).
- `spawn_yosh() -> (OsSession, TempDir)`.
- `wait_for_prompt(&mut OsSession)`.
- `wait_for_ps2(&mut OsSession)`.
- `wait_for_raw_mode(&mut OsSession)` — termios polling via `tcgetattr`
  on the master fd.
- (new) `read_until_prompt(&mut OsSession) -> String` — captures
  output between the previously-sent command and the next `$ ` prompt,
  with prompt + command-echo + ANSI sequences stripped.

After the extraction, `tests/pty_interactive.rs` keeps its 200+
test-specific functions but imports the primitives via
`mod helpers; use helpers::pty::*;` (matches the existing
`tests/helpers/mod.rs` + `mock_terminal.rs` pattern).

### 4.4 yosh source change: PS1 default initialization

`src/interactive/mod.rs::Repl::new()` gains one block, placed
immediately after the history-variable defaults (around L74-77):

```rust
// POSIX XCU §2.5.3: PS1 has a default value for interactive shells.
// Set it as a real variable so observers like `[ -n "${PS1+x}" ]`
// see it. Defer to inherited / rc-set value if already present.
if executor.env.vars.get("PS1").is_none() {
    let default = if unsafe { libc::getuid() } == 0 { "# " } else { "$ " };
    let _ = executor.env.vars.set("PS1", default);
}
```

Rationale:

- POSIX XCU §2.5.3 specifies PS1's default and lists it as a
  shell-set variable.
- Current `default_prompt()` (`src/interactive/prompt.rs:9`) is a
  render-time fallback only; the variable is never set, so any
  POSIX-conformant script that introspects PS1 (`${PS1+x}`,
  `${PS1-x}`, `set` listing) sees it as unset.
- The `is_none()` guard preserves the existing behavior of letting an
  inherited environment value or `~/.yoshrc` override the default.
- `~/.yoshrc` is sourced after this block (current
  `src/interactive/mod.rs:82-86`), so an rc that writes `PS1=...`
  takes precedence — matches POSIX intent.
- `libc::getuid()` matches the existing usage at
  `src/interactive/prompt.rs:13`.

No other source changes are required. `fc`, `/dev/tty`, and history
all work as-is once the PTY context is provided.

## 5. Per-test seed and verification

| # | Rust test | Pre-state setup | Verification |
|---|-----------|-----------------|--------------|
| 1 | `fc::list_recent` | send `echo a`, `echo b`, `echo c` (each wait_for_prompt) | `fc -l` output contains "echo a", "echo b", "echo c" with leading line numbers |
| 2 | `fc::list_no_numbers` | same | `fc -l -n` output contains the commands without leading numbers (assert via regex `^\techo a` style) |
| 3 | `fc::list_reverse` | same | `fc -l -r` output orders the commands `c, b, a` |
| 4 | `fc::substitute` | send `echo onevar` | `fc -s one=two echo` produces `twovar` on stdout |
| 5 | `fc::editor_dash_e` | seed 1 history line | `fc -e cat </dev/null >/dev/null 2>&1` exits 0 (cat reads tempfile, EOF, normal exit) |
| 6 | `fc::no_args_uses_editor` | seed 1 history line; `export FCEDIT=cat` | bare `fc` exits 0 (cat as editor) |
| 7 | `fcedit::used_by_fc` | seed 1 history line; `export FCEDIT=cat` | bare `fc` exits 0 |
| 8 | `fcedit::default_ed` | seed 1 history line; `unset FCEDIT; unset EDITOR` | bare `fc` invokes `/bin/ed`; exit status 0 — see §6 risk |
| 9 | `ps1::default_value_set` | start shell with `PS1` removed from inherited env (`Command::env_remove("PS1")`) | send `[ -n "${PS1+x}" ] && echo SET \|\| echo UNSET`; capture `SET` |
| 10 | `exec_redirect::no_cmd_redirects` | per-test tmpdir already provided by `spawn_yosh()`; send `export TEST_TMPDIR=<tmpdir.path()>` at the prompt | send `exec >$TEST_TMPDIR/out; echo persistent; exec >/dev/tty; cat $TEST_TMPDIR/out`; capture `persistent` from `cat` output |

**`PS1` env-removal:** `spawn_yosh()` currently inherits the parent's
PATH/HOME/etc. Test #9 needs a variant that strips `PS1` from the child
env before spawn. Implementation either via a `spawn_yosh_clean_ps1()`
sibling, or by adding an `env_overrides: &[(&str, Option<&str>)]`
argument to `spawn_yosh`. The variant lives in `tests/helpers/pty.rs`.

## 6. Risks and unknowns

**`/bin/ed </dev/null` exit status (test #8).** macOS `/bin/ed` may exit
with `?` (non-zero) when stdin is closed without any commands. If this
proves to be the case, options are:

1. Send a `q\n` line to ed via `session.send_line` after the `fc`
   command (requires expecting ed's editor prompt or absence thereof).
2. Have `fc_no_command` test set `FCEDIT=true` instead to short-circuit
   the editor invocation, and rewrite `FCEDIT_default_ed` to verify the
   command-line that `fc` would launch (intercepted via a stub script).
3. Demote `FCEDIT_default_ed` into SP7 per the roadmap §5.4 escape
   hatch and rewrite the e2e XFAIL line to
   `# XFAIL: deferred (/bin/ed batch-mode exit status varies across platforms)`.

Decision deferred to the implementation plan; first attempt is option 1.

**`fc -e cat` behavior with `< /dev/null` and `>/dev/null` (test #5).**
The current `fc_edit` (`src/builtin/special.rs:724`) writes a tempfile,
runs `Command::new(editor).arg(tmp).status()`, then re-runs the
edited tempfile contents as commands. With `cat` as the editor and
no edit step, the tempfile is unchanged and `fc` re-executes the
selected history line. With `< /dev/null` overriding stdin and
`>/dev/null` swallowing stdout, the re-execution succeeds silently.
Verifying the actual re-execution semantics is not in scope; SP6
mirrors the original test's exit-status-only contract.

**Flakiness budget.** PTY tests are inherently timing-sensitive. The
existing `wait_for_raw_mode` + `wait_for_prompt` synchronization in
`pty_interactive.rs` is already production-tested across the CI suite.
SP6 inherits this baseline; no new sleep-based waits are introduced.

**`fc::substitute` output stripping.** PTY captures include the
command echo line (`echo onevar`) before the actual output. The
`read_until_prompt` helper must strip this echo before assertion
matching, otherwise `out.contains("twovar")` could match the echoed
`one=two` operand text. The helper either filters by lines starting
with the prompt-stripped echo, or returns only the lines after the
command-send and before the next prompt — to be decided in the plan.

## 7. Acceptance criteria

1. **Test migration.** All 10 e2e shell files have their `# XFAIL: …`
   lines replaced with `# MIGRATED_TO: tests/pty_posix.rs::<name>`.
   Corresponding 10 Rust tests in `tests/pty_posix.rs` pass under
   `cargo test --test pty_posix`. Tests that prove unmigratable are
   demoted to SP7 per the roadmap escape hatch with a TODO.md entry.
2. **Runner update.** `e2e/run_tests.sh` recognizes `# MIGRATED_TO:`,
   prints `[MIGRATED]`, and reports `Migrated: N` in the summary.
   Both-directives case emits `[WARN]`.
3. **PS1 initialization.** `src/interactive/mod.rs::Repl::new()` sets
   `PS1` to the POSIX default when not inherited. `cargo test` overall
   remains green; no regressions in `tests/pty_interactive.rs`.
4. **E2E count.** `./e2e/run_tests.sh` reports `XFail: 3, Migrated: 10`
   (assuming no SP7 demotion). If demotion occurs, `XFail: 3+N,
   Migrated: 10-N` with each demotion documented.
5. **Documentation.** `TODO.md` SP6 entry is deleted (project
   convention "delete completed items"). Follow-ups recorded under
   `### SP6 follow-ups (non-blocking)`. The
   `project_e2e_xfail_roadmap` memory entry is updated to mark SP6
   complete.

## 8. Implementation plan reference

The implementation plan (separate document, written via
`superpowers:writing-plans`) decomposes SP6 into the following
commit-aligned groups:

- **G1** — `e2e/run_tests.sh` `# MIGRATED_TO:` support.
- **G2** — extract PTY helpers to `tests/helpers/pty.rs`; refactor
  `tests/pty_interactive.rs` to use them.
- **G3** — yosh PS1 default initialization.
- **G4** — fc tests 1-6 migration (Rust impl + e2e stubs).
- **G5** — FCEDIT tests 7-8 migration (Rust impl + e2e stubs, includes
  `/bin/ed` risk resolution).
- **G6** — PS1 + exec-redirect tests 9-10 migration.
- **G7** — closure: TODO.md cleanup, memory update, final XFail count
  verification.

Each group is a single commit; G2 and G3 are independent and can land
in either order. G4-G6 depend on G1+G2. G7 depends on all preceding.
