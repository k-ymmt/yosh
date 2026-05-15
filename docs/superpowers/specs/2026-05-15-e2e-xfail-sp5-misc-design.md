# SP5 — Miscellaneous POSIX Features (8 tests)

**Date:** 2026-05-15
**Status:** Design (pending plan + implementation)
**Roadmap:** [`2026-05-13-e2e-xfail-roadmap-design.md`](2026-05-13-e2e-xfail-roadmap-design.md) §3 SP5
**Predecessor SPs:** SP1 / SP2 / SP3 / SP4 (all complete)

## 1. Background

The E2E XFAIL roadmap §3 partitions 55 XFAIL tests into 7 sub-projects.
SP1+SP2+SP3+SP4 (34 tests) are complete; 21 XFails remain across
SP5–SP7. SP5 is the "miscellaneous small features" bucket: 8 tests, each
touching a distinct subsystem (parser, expander, redirect layer, trap
machinery, env startup, xtrace formatting). They are bundled because
none warrants its own spec.

## 2. Scope

In scope:

- Remove the `# XFAIL: …` header from each of the 8 listed test files so
  they execute as normal expectations under `./e2e/run_tests.sh`.
- Implement the corresponding behavior in `src/` (one sub-task per test).
- Add focused unit tests in the touched modules.
- Record non-blocking polish items in TODO.md as `### SP5 follow-ups`.

Out of scope:

- PTY harness migration (SP6).
- Deferred / known-deviation tests (SP7).
- Refactors beyond what the targeted fix requires.
- Generalizing PS4 to perform `$LINENO` / `$`-expansion (literal-only is
  sufficient for the test; expansion is a follow-up).
- Per-process EXIT-trap fire in pipeline children (only subshell child
  is required by the SP5 test).

## 3. Target Tests

The 8 tests, grouped by subsystem affinity (see §5 Grouping):

| # | Test file | XFAIL reason snippet | Group |
|---|-----------|---------------------|-------|
| 1 | `e2e/posix_spec/2_04_reserved_words/reserved_after_assignment_recognized.sh` | reserved word not recognized after assignment prefix | G1 |
| 2 | `e2e/posix_spec/2_06_03_command_substitution/exit_status_propagates_to_parent.sh` | `$?` after standalone `$(...)` is 0, not the substituted command's status | G2 |
| 3 | `e2e/posix_spec/2_07_redirection/redir_order_left_to_right.sh` | `2>&1 >f` does not dup stderr to the original stdout | G2 |
| 4 | `e2e/posix_spec/2_09_01_simple_commands/redirection_only_creates_file.sh` | redirect-only command (`>f`) does not apply the redirect | G2 |
| 5 | `e2e/posix_spec/2_11_signals_and_error_handling/trap_zero_runs_on_exit.sh` | `trap 0/EXIT` not fired on subshell exit | G3 |
| 6 | `e2e/posix_spec/4_special_builtin/trap_int_handler.sh` | SIGINT trap deferred to end-of-script; should run async | G3 |
| 7 | `e2e/posix_spec/8_env_vars/PPID_is_set.sh` | `$PPID` empty at startup | G4 |
| 8 | `e2e/posix_spec/8_env_vars/PS4_assigned.sh` | `set -x` trace prefix hardcoded to `"+ "`; PS4 ignored | G4 |

## 4. Architecture / Components

SP5 is not a single architectural change; it is eight targeted patches.
The relevant subsystems are:

- **Parser** (`src/parser/simple.rs`) — `parse_simple_command` decides
  when assignment-prefix parsing ends and command-position parsing
  begins. G1 extends this boundary so reserved words remain
  command-position-eligible after a leading assignment.
- **Simple-command executor** (`src/exec/simple.rs`) — its
  assignment-only / empty-expansion path is where G2-1 (`$?`
  propagation) and G2-2 (redirect-only) live. The xtrace `eprintln!("+
  ...")` site is also here (G4-2 PS4).
- **Redirect layer** (`src/exec/redirect.rs`) — `RedirectState::apply`
  already iterates redirects in source order. G2-3 verifies the
  external-command path actually applies them L-to-R and fixes any
  diverging code paths.
- **Subshell executor** (`src/exec/compound.rs::exec_subshell`) — child
  branch currently calls `exit_child(status)` directly. G3-1 inserts an
  `execute_exit_trap()` call so the child's EXIT trap fires before
  `_exit`.
- **Per-command signal-drain hook** (`src/exec/control.rs::exec_complete_command`
  + `src/exec/mod.rs::process_pending_signals`) — G3-2 calls
  `process_pending_signals` at the end of each complete command so
  asynchronous signals (SIGINT in particular) are handled near their
  delivery point rather than only at script end.
- **Shell env startup** (`src/env/mod.rs::ShellEnv::new`) — G4-1 sets
  `$PPID` once at construction, mirroring the existing `OPTIND` init.

## 5. Grouping (4 groups, reverse-risk order)

Execution order is **G4 → G1 → G2 → G3**, smallest/safest first,
trap-machinery work last.

### G4 — env + xtrace (2 tests, 1 commit)

**Files touched:** `src/env/mod.rs`, `src/exec/simple.rs`

**G4-1 PPID at shell startup** (`PPID_is_set.sh`)

In `ShellEnv::new` (`src/env/mod.rs:58`), right after the existing
`OPTIND=1` init:

```rust
let _ = vars.set("OPTIND", "1");
// POSIX §2.5.3: $PPID is the parent PID of the invoking shell,
// captured at startup. Subshells inherit the value — they do not
// recompute it.
let _ = vars.set("PPID", nix::unistd::getppid().to_string());
```

PPID is not made readonly (matches bash and dash).

**G4-2 PS4 trace prefix** (`PS4_assigned.sh`)

In `exec_simple_command` (`src/exec/simple.rs:174`), replace the
hardcoded `"+ "` with a `PS4` lookup:

```rust
if self.env.mode.options.xtrace && !expanded.is_empty() {
    let ps4 = self.env.vars.get("PS4").unwrap_or("+ ");
    eprintln!("{}{}", ps4, expanded.join(" "));
}
```

The first-character-repeat rule (POSIX: first byte of PS4 is repeated
for nesting depth) is **out of scope**; literal PS4 emission is
sufficient for the test and tracked as a follow-up.

Variable / arithmetic / command-sub expansion of PS4 is also **out of
scope** (literal-only is sufficient); recorded as a follow-up.

**Unit tests:**
- `ShellEnv::new` sets `$PPID` to a positive integer matching `getppid()`.
- `eprintln` capture: xtrace with `PS4='> '` emits `> echo 0`.
- Default xtrace (PS4 unset) emits `+ echo 0`.

**Commit shape (1 commit):**
- `feat(env,exec/simple): set $PPID at startup; honour PS4 in set -x`

### G1 — parser (1 test, 1 commit)

**Files touched:** `src/parser/simple.rs`, possibly `src/parser/word.rs`
or wherever reserved-word recognition lives.

**G1-1 reserved word after assignment** (`reserved_after_assignment_recognized.sh`)

POSIX §2.4 requires reserved-word recognition to happen at
**command-word position**. An assignment prefix is allowed before the
command word per §2.10.2 (Shell Grammar Rules) Rule 7, and reserved
words are recognized at this position. Therefore `x=1 if true; then
echo y; fi` must parse `if` as the `If` reserved word starting a
compound command, with the `x=1` assignment forming an in-scope
temporary binding for the compound's duration.

**Implementation direction (final mechanism decided in the plan):**

Two viable approaches, both touching only `src/parser/simple.rs` and
its surrounding token-classification helpers:

1. **Hand-off:** when `parse_simple_command` has consumed one or more
   assignments and the next token is a Word that matches a reserved
   word, abort simple-command parsing and re-dispatch to the
   compound-command parser, attaching the consumed assignments to the
   resulting compound node.
2. **Pre-classify:** at the start of every `parse_simple_command`
   iteration after consuming an assignment, re-check the next Word
   token against `RESERVED_WORDS` and convert it to the matching
   reserved-token variant before the simple-command loop sees it.

Approach 1 is cleaner architecturally (the AST node for `if`/`while`/
`for` ends up under a compound branch with attached prefix
assignments). Approach 2 is mechanical and may require less AST
plumbing.

**The plan task is empowered to pick either approach** based on what
the AST currently supports for "compound command with prefix
assignments". If the AST has no slot for prefix assignments on
compound commands, Approach 2 followed by a small AST extension is the
likely choice.

**Acceptance for either approach:**
- `x=1 if true; then echo y; fi` produces `y` and exits 0.
- The temporary assignment `x=1` must not leak past the compound (POSIX
  §2.9.1 simple-command semantics extended to compound when prefixed).

**Unit tests:**
- Parse `x=1 if true; then echo y; fi` → AST root is the `If` compound,
  with prefix assignments `[x=1]` attached.
- Parse `x=1 while …; do …; done` → same shape with `While`.
- Parse `x=1 echo y` (regular simple-command path) — regression: still
  works.
- Parse `if true; then x=1 if ...; fi; fi` — nested, no leakage.

**Commit shape (1 commit):**
- `feat(parser): recognize reserved words after assignment prefix`

### G2 — exec/simple + redirect (3 tests, 3 commits)

**Files touched:** `src/exec/simple.rs`, `src/exec/redirect.rs` (if
G2-3 turns up a real ordering bug there).

#### G2-1 `$?` propagation from standalone command substitution

(`exit_status_propagates_to_parent.sh`)

`$(false)` alone on a line: yosh parses it as `SimpleCommand { words:
[Word(CommandSub(false))], assignments: [], redirects: [] }`. Expansion
runs the substitution, sets `env.exec.last_exit_status = 1`, and
returns an empty field list. The assignment-only path then re-assigns
`last_exit_status = last_cmd_sub_status.unwrap_or(0)`, overwriting the
1 with 0 because the assignment loop never runs and
`last_cmd_sub_status` stays `None`.

**Fix:** detect command substitution in any of `cmd.words` (not just in
`cmd.assignments`), and feed it into `last_cmd_sub_status` for the
same reduction.

```rust
let words_had_cmd_sub = cmd.words.iter().any(word_has_command_sub);
// existing block reading assignment-side substitutions...
let final_status = match (last_cmd_sub_status, words_had_cmd_sub) {
    (Some(s), _) => s,                                  // assignment-side sub wins (last)
    (None, true) => self.env.exec.last_exit_status,     // word-side sub last seen
    (None, false) => 0,
};
```

`word_has_command_sub` is the helper used at `simple.rs:138`.

**Unit tests:**
- `$(false)` alone: returns 1, `last_exit_status` == 1 afterwards.
- `$(false); echo $?` → outputs `1`.
- `x=$(false)` (existing case) still works: returns 1.
- `: $(false)` (substitution on argv, not assignment-only) — covered by
  builtin path, no regression.

**Commit shape:**
- `fix(exec/simple): propagate $? from standalone command substitution`

#### G2-2 Redirect-only command applies redirects

(`redirection_only_creates_file.sh`)

`>f` parses as `SimpleCommand { words: [], assignments: [], redirects:
[Output("f")] }`. The assignment-only path returns before reaching the
external/builtin redirect-apply path. POSIX §2.9.1 says redirections
are still performed.

**Fix:** in the assignment-only branch (after expansion yields empty),
apply `cmd.redirects` to the shell process (`save=true` so the change
is rolled back after the command).

```rust
if expanded.is_empty() {
    let mut redirect_state = RedirectState::new();
    if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
        self.env.exec.last_exit_status = 1;
        return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
    }
    // ...existing assignment loop + last_cmd_sub_status…
    redirect_state.restore();
    return Ok(final_status);
}
```

**Note on semantics:** POSIX §2.9.1 specifies "the redirections shall
be performed in the current shell execution environment" for a
redirect-only command. The fd state must persist past the redirect-
only "command" for subsequent commands on the same logical line, BUT
the test `cd $TEST_TMPDIR; >f; test -f f && echo ok` only needs the
file to be created — restoring fds after `>f` does not affect the
file's existence. Restore (save=true) matches bash and dash behavior:
`>f` truncates `f`, the redirection state does not persist.

**Unit tests:**
- `>f` creates `f` empty, returns 0, stdout unaffected afterwards.
- `>f; echo hi` outputs `hi` to terminal (not to `f`).
- `2>f` creates `f`, stderr unaffected afterwards.
- Invalid path `>/nonexistent/dir/f` returns nonzero with diagnostic.

**Commit shape:**
- `fix(exec/simple): apply redirections on redirect-only commands`

#### G2-3 Redirect L-to-R ordering for external commands

(`redir_order_left_to_right.sh`)

Test: `sh -c 'echo out; echo err >&2' 2>&1 >f`. Expected: `f` contains
only `out`. POSIX §2.7: redirections are processed left-to-right. With
the current stdout pointing at the terminal:

- Step 1 `2>&1`: dup current stdout (terminal) onto fd 2 → fd 2 points
  to terminal, fd 1 still points to terminal.
- Step 2 `>f`: open `f`, dup onto fd 1 → fd 1 points to `f`, fd 2
  still points to terminal.

Result: `echo out` writes to fd 1 (f); `echo err >&2` writes to fd 2
(terminal). `cat f` outputs `out`.

`RedirectState::apply` iterates `redirects` in vector order, which is
source order, and `apply_one` for `DupOutput` reads the current
`src_fd` value via `dup2`. So the L-to-R ordering *should* already
work for the assignment-aware shell-process path.

**Diagnostic step (plan task):** run the failing test with `cargo test
--test e2e` against a `RUST_LOG=trace` instrumented build (or with
ad-hoc `eprintln!` in `apply_one`) to identify whether:

- (a) the redirects are stored in reverse order in the AST,
- (b) one of `apply_one`'s `dup2` calls is using a stale fd value
  because of an `into_raw_fd` ownership bug,
- (c) the external-command fork path applies redirects in a different
  order than the shell-process path (e.g., a separate code path that
  iterates in reverse), or
- (d) some other root cause not yet identified.

**Fix** in whichever path the diagnostic points at. Spec does not
prescribe the location because the bug has not been root-caused yet.

**Acceptance:** the test passes; the existing redirect E2E tests do
not regress.

**Unit tests:**
- `2>&1 >f` applied in a controlled `RedirectState`: fd 1 ends at `f`,
  fd 2 ends at the saved original stdout.
- `>f 2>&1` (reversed order, both to f) — regression check: fd 1 and fd
  2 both at `f`.
- `>f >g` (two output redirects) — last wins per POSIX.

**Commit shape:**
- `fix(exec/redirect): apply redirections left-to-right for external commands`
  (commit message names the actual location based on diagnostic outcome.)

### G3 — trap machinery (2 tests, 2 commits)

**Files touched:** `src/exec/compound.rs`, `src/exec/control.rs`,
`src/exec/mod.rs`.

#### G3-1 Subshell EXIT trap fires before exit_child

(`trap_zero_runs_on_exit.sh`)

`(trap 'echo bye' 0; :)` — subshell child sets `exit_trap = Command("echo
bye")` then runs `:`. Currently `exec_subshell` (`src/exec/compound.rs:86-100`)
forks, in the child runs `reset_non_ignored()` + `exec_body`, then
`exit_child(status)` directly. POSIX §2.11: the EXIT trap shall run on
shell exit, including subshell exit.

**Fix:** insert `self.execute_exit_trap()` immediately before
`super::exit_child(status)` in the child branch:

```rust
ForkResult::Child => {
    self.env.traps.reset_non_ignored();
    // ... LINENO bump, etc. ...
    let status = match self.exec_body(body) {
        Ok(s) => s,
        Err(_) => 1,
    };
    self.execute_exit_trap();   // NEW
    super::exit_child(status);
}
```

`execute_exit_trap` already exists at `src/exec/mod.rs:153` and uses
`with_errexit_suppressed` so trap-side errors do not propagate out.

**Scope decision:** pipeline child branches (`src/exec/pipeline.rs:106`)
also call `exit_child` without firing EXIT trap. The SP5 test does not
require pipeline-child EXIT trap (it uses subshell parentheses, not a
pipeline). Apply the fix to subshell only; record pipeline-child EXIT
trap firing as a TODO.md follow-up.

**Unit / E2E coverage:**
- E2E test pass.
- E2E supplemental: `(trap 'echo bye' 0; exit 5)` outputs `bye` and
  parent's `$?` is `5`.
- E2E supplemental: `(trap '' 0; :)` (Ignore trap) — no `bye`, no
  crash.
- E2E supplemental: nested `(trap 'echo outer' 0; (trap 'echo inner' 0; :))`
  outputs `inner` then `outer`.

**Commit shape:**
- `fix(exec/compound): fire EXIT trap on subshell child exit`

#### G3-2 Async signal handling at command boundary

(`trap_int_handler.sh`)

Test:
```sh
trap 'echo caught' INT
kill -INT $$ 2>/dev/null
sleep 0.05 2>/dev/null
echo after
```

Expected output:
```
caught
after
```

Currently `process_pending_signals` is called only at script end
(`main.rs:272/279`), so the `Command("echo caught")` trap fires *after*
`echo after`. POSIX §2.11: trap actions for non-EXIT signals run "as
soon as the shell is ready to accept" them — at minimum, between
commands.

**Fix:** call `self.process_pending_signals()` at the end of
`exec_complete_command` in `src/exec/control.rs`:

```rust
pub fn exec_complete_command(&mut self, cmd: &CompleteCommand) -> i32 {
    let status = /* ... existing logic ... */;
    self.process_pending_signals();   // NEW
    status
}
```

This runs the drain once per top-level command (`foo; bar; baz` runs it
3 times; `if true; then a; b; fi` runs it once for the if, plus
whatever inner exec_complete_command's may apply). Trap actions for
SIGINT / SIGHUP / SIGTERM / SIGUSR1 / SIGUSR2 / SIGQUIT / SIGALRM all
become responsive to delivery points between commands.

**Risk: $? interaction.** `process_pending_signals` evaluates the trap
command via `eval_string`, which runs `exec_program`, which updates
`env.exec.last_exit_status`. If the user wrote `false; echo $?` and a
SIGINT arrives between them, the trap action's exit status would
overwrite the `false`'s status before `echo $?` reads it. POSIX permits
this in the general async-signal context (the script is responsible
for save/restore in the trap action), and bash exhibits the same
behavior. **No save/restore is added.**

**Risk: nested signal during trap.** A second SIGINT arriving while
the first trap action is mid-execution writes to the self-pipe; the
next drain (at the next command boundary after the trap returns)
handles it. This is the documented sloppy-but-POSIX behavior — yosh
matches bash.

**Risk: drain inside `exec_program`'s recursive call.** Trap actions
run via `eval_string → exec_program → exec_complete_command`, which
re-enters `process_pending_signals`. Re-entry is safe because the
self-pipe drain is idempotent (the bytes have already been read), but
deep recursion is possible if a trap action installs another trap on
the same signal. Pre-decided as acceptable behavior; record as
follow-up if observed.

**Acceptance:**
- `trap_int_handler.sh` passes.
- `cargo test` green (no signal-test regression in `tests/interactive.rs`
  or `tests/pty_interactive.rs`).
- `cargo test --features test-helpers` plugin tests green.
- Interactive mode is unaffected (interactive loop has its own
  `process_pending_signals` call after each line, this change adds an
  extra drain inside per-command exec which is idempotent).

**Unit tests:**
- Mock-fed self-pipe: `process_pending_signals` runs Command trap on
  SIGINT, leaves Default and Ignore alone, does not call `_exit` on
  trapped signals.

**Commit shape:**
- `fix(exec): drain pending signals at command boundary for async trap`

## 6. Data Flow

```
G4-1 PPID:
  process_start → ShellEnv::new → vars.set("PPID", getppid())
  → $PPID visible in subsequent expansions

G4-2 PS4:
  exec_simple_command(xtrace=true)
  → vars.get("PS4").unwrap_or("+ ")
  → eprintln!("{}{}", ps4, joined)

G1 reserved-after-assign:
  parse_simple_command consumes assignments
  → next token == reserved → hand-off to compound parse
  (or pre-classify, plan decides)

G2-1 $? from $(...):
  expand_words runs CommandSub → last_exit_status set
  → expanded empty
  → assignment-only path detects word-side cmd-sub via word_has_command_sub
  → last_cmd_sub_status seeded from current last_exit_status
  → final_status preserves it

G2-2 redirect-only:
  exec_simple_command sees empty expansion
  → RedirectState::apply(cmd.redirects, save=true)
  → process the redirect (e.g., open f, dup to fd 1)
  → restore fds
  → return 0

G2-3 L-to-R order:
  RedirectState::apply iterates redirects in source order
  → apply_one captures the CURRENT fd before dup2
  → identified buggy path fixed to match

G3-1 subshell EXIT:
  fork child → reset_non_ignored → exec_body → execute_exit_trap → exit_child

G3-2 INT async:
  SIGINT delivered → handler writes self-pipe
  → exec_complete_command tail runs process_pending_signals
  → drain → run TrapAction::Command via eval_string
  → next command sees the trap output already on stderr
```

## 7. Error Handling

- G1 parser: invalid reserved-word usage falls through to the existing
  parser error path (no new error variants).
- G2-1: no new errors; existing CommandSub failure already produces
  diagnostics via `expand_words`.
- G2-2: redirect failure on a redirect-only command returns
  `ShellError::runtime(RedirectFailed, ...)` with exit 1, same as
  redirect failure on a command-with-name.
- G2-3: no new error paths; the fix is purely correctness of fd
  ordering.
- G3-1: `execute_exit_trap` is wrapped in `with_errexit_suppressed`;
  trap-action errors do not propagate to the child's exit status.
- G3-2: drained signals with `TrapAction::Default | None` route through
  `handle_default_signal` (existing path); the script terminates at
  `128+sig` as before. Drained signals with `TrapAction::Command` run
  the action; any internal error from `eval_string` is swallowed
  (existing behavior).

## 8. Testing

### 8.1 E2E

Each of the 8 target tests has its `# XFAIL: …` line removed; the
existing `EXPECT_OUTPUT` / `EXPECT_EXIT` / `EXPECT_STDERR` headers stay.
Acceptance criterion is `XFail: 13` in the post-SP5 baseline (21 - 8 =
13).

Pre-merge command:
```sh
cargo build && ./e2e/run_tests.sh
```

Expected line at tail: `Total: <same> Passed: <prev+8> Failed: 0 Timedout: 0 XFail: 13 XPass: 0`.

### 8.2 Unit

Each sub-task adds 2–5 focused unit tests in the touched module's
`mod tests` block. See per-sub-task lists in §5.

### 8.3 Format / clippy

`cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D
warnings` both clean before each commit.

### 8.4 Existing test stability

`cargo test` green; `cargo test --features test-helpers` green for
plugin paths (G3-2 in particular changes per-command execution flow
and must not regress plugin hook timing).

## 9. Subagent Dispatch Model

Mirrors SP4's structure:

- **Per group**: implementer (sonnet) → spec-reviewer (sonnet) →
  code-quality-reviewer (sonnet).
- **Order**: G4 → G1 → G2 → G3 (reverse-risk). Each group runs to
  completion before the next group's implementer dispatches.
- **G2 internal**: G2-1 / G2-2 / G2-3 are sequential within G2 because
  all three edit `src/exec/simple.rs`; reviewer dispatch per sub-task.
- **Final pass**: branch-level reviewer (sonnet) for cross-cutting
  consistency; haiku task for TODO.md / memory updates / XFAIL count
  verification.

## 10. Acceptance Criterion

SP5 is complete when:

1. All 8 listed E2E tests pass with no `# XFAIL` line.
2. `./e2e/run_tests.sh` reports `XFail: 13` (21 − 8).
3. `cargo test`, `cargo test --features test-helpers`, `cargo fmt --check`,
   `cargo clippy --all-targets -- -D warnings` all green.
4. TODO.md `## E2E XFAIL Roadmap` entry for SP5 is removed (per project
   convention); `### SP5 follow-ups (non-blocking)` section captures any
   polish items discovered during implementation.
5. Memory `project_e2e_xfail_roadmap.md` updated with SP5 completion
   line.

## 11. Open Items (resolved in plan, not here)

- G1 implementation mechanism: hand-off vs. pre-classify (§5 G1).
- G2-3 root-cause location for the L-to-R ordering bug (§5 G2-3).
- Whether subshell EXIT trap fix needs to mirror in pipeline-child path
  (§5 G3-1 — current decision: subshell only, pipeline as follow-up).
- Whether `process_pending_signals` should also be called in other
  hot-path command boundaries (compound `for`/`while` iteration tails,
  function call returns). Current decision: top-level `exec_complete_command`
  only; revisit if interactive scripts surface a missed-signal scenario.
