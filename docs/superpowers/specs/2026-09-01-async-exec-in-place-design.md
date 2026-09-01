# Async Exec-in-Place Optimization Design

**Date:** 2026-09-01
**Goal:** Eliminate the async double fork for the common `cmd &` case by
exec'ing the command directly in the async child when the payload is a
single external simple command — bash's "nofork" optimization. This makes
the job table track the real command pid, fixing stop-visibility and
`$PPID` skew (TODO.md Job Control section, 2026-08-25/26 findings).

## Problem

`cmd &` currently forks an async wrapper subshell (`exec_async`,
`src/exec/control.rs`), which then forks + execs the external command
(`spawn_external_at_path`, `src/exec/simple.rs`). The job table tracks
the wrapper pid, so:

1. **Stops are invisible** — when the grandchild stops (e.g. background
   `cat` hit by SIGTTIN, or a backgrounded `yosh &` REPL self-stopping),
   `jobs` still reports Running and `bg %1` fails with "job not stopped".
   `fg` only works because it operates on the whole pgrp.
2. **`$PPID` skew** — the exec'd command sees the intermediate wrapper as
   its parent; `sh -c 'kill -USR1 $PPID' &` signals the wrapper (which
   dies at SIG_DFL) instead of the forking shell. bash/dash exec in place
   and report the real shell.
3. One wasted fork per background external.

## Approach

Keep the single `exec_async` fork (it is needed anyway — the parent must
get the child pid back immediately for `$!` and the job table). Change
what the *child* does: when the async payload is shaped like a single
simple command, set a one-shot flag; if that command then dispatches to
the external-utility path, the child execs **in place** instead of
forking a grandchild. The job-table entry (the async child pid) then IS
the command.

Everything before the exec is unchanged: the async child still does its
setpgid, trap/job-table subshell reset, monitor-mode handling, stdin
`/dev/null` (non-monitor), and SIGINT/SIGQUIT ignores (non-monitor) —
expansion (which may run command substitutions), redirect application,
and environ merge all happen in the async child exactly where the
wrapper used to do them.

### Candidate shape (checked in `exec_async`, child side)

```
and_or.rest.is_empty()
  && !and_or.first.negated
  && and_or.first.commands.len() == 1
  && matches!(and_or.first.commands[0], Command::Simple(_))
```

If it matches, set `env.exec.async_exec_in_place = true` before
`exec_and_or`. Anything else (`a && b &`, pipelines, `! cmd &`,
compounds) keeps the wrapper-subshell path unchanged.

### One-shot flag consumption (`exec_simple_command`)

`exec_simple_command` **takes** the flag (read + clear) at entry, before
any expansion runs. This guarantees:

- Command substitutions fired during expansion fork children whose
  cloned `ShellEnv` has the flag already cleared — no nested consumer
  can steal it.
- Non-external dispatches (assignment-only, functions, builtins,
  `wait`/`fg`/`bg`/`jobs`/`command`, plugin commands) simply ignore the
  taken flag and behave as today (wrapper subshell semantics).

The taken flag is honored only in the `BuiltinKind::NotBuiltin` external
branch.

**DEVIATION (decided during implementation):** plugins' `post_exec` hook
does not fire for an exec-in-place background external — the process is
replaced by the exec. An earlier draft gated the optimization on
`!plugins.has_exec_hooks()`, but that turns it off for any user with an
exec-hook plugin loaded (e.g. rich-prompt-plugin implements both hooks),
defeating the job-control fix exactly where it matters (the interactive
shell). Pre-optimization, `post_exec` for async externals ran inside the
forked wrapper whose plugin state was discarded on exit, so only
external side effects (logging, file writes) are lost. `pre_exec` still
fires in the async child before the exec, exactly as it did in the
wrapper.

### In-place exec sequence

`exec_external_with_redirects` gains an `in_place: bool` parameter. In
the `ResolvedExec::Executable` arm with `in_place`:

1. `signal::reset_child_signals(&ignored)` — identical dispositions to
   what the old grandchild got (`ignored` = trap-ignored signals, which
   in the non-monitor async child already includes the SIGINT/SIGQUIT
   entries inserted by `exec_async`): everything else to SIG_DFL, and
   the inherited self-pipe fds closed. Exec resets caught handlers to
   SIG_DFL anyway; the explicit reset matters for SIG_IGN dispositions
   (which persist across exec) such as the background-child SIGTTIN
   ignore — the exec'd command must be able to receive SIGTTIN-stop,
   which is exactly what makes the stop *visible* now that the job
   table tracks this pid.
2. Apply redirects, merge environ + prefix-assignment overrides, and
   `execv` with the ENOEXEC `/bin/sh` fallback — this is the existing
   child-side body of `spawn_external_at_path`, extracted into a shared
   `exec_child_prepared(...) -> !` helper so the two paths cannot
   drift. Failures exit the async child with the same codes the
   grandchild used (redirect failure 1, ENOENT 127, EACCES/ENOEXEC 126).

No setpgid is needed at exec time (the async child already put itself
in its own pgrp) and monitor is already forced off inside async
children, so the helper split is: callers do fork-specific setup
(setpgid + signal flavor), `exec_child_prepared` does redirects + env +
execv + exit.

`ResolvedExec::NotFound` / `NotExecutable` return 127/126 without exec;
the async child then exits with that status through the normal wrapper
path — same observable as today.

## Observable changes (intended)

- `jobs` reports Stopped when a background external stops; `bg %1`
  works on it.
- `$PPID` in a background external equals the forking shell's pid.
- One fewer fork per `cmd &` with an external simple command.

Not changed: `$!`, job ids, `[n] pid` notice, exit/signal statuses,
async builtin/function/compound/list behavior, `wait` semantics,
non-monitor SIGINT/SIGQUIT ignores, stdin `/dev/null`.

## Tests

- Integration (`tests/`): `$PPID` equals `$$` for a background
  `/bin/sh -c 'echo $PPID'`; exit-status and TERM-signal propagation
  through `wait $!`; redirect-failure status 1; async builtins
  unaffected.
- PTY (`tests/pty_interactive.rs`): background `/bin/cat` stops via
  SIGTTIN and `jobs` shows Stopped; `bg`/`fg` interaction with the now
  correctly tracked pid.
- Existing suites are the regression net; the backgrounded-REPL PTY
  tests exercise the exact stop-visibility path this changes and their
  assertions are updated where the new (correct) behavior differs.

## Out of scope

- Exec-in-place for `command cmd &`, pipelines-of-one wrapped in
  compounds, or the last command of an async list (`a; b &` semantics
  don't exist — `&` binds to the list item) — bash optimizes some of
  these too; revisit if profiling says it matters.
- pgrp-wide WUNTRACED probing (the alternative fix from TODO.md) —
  superseded by this approach for the single-external case; multi-pid
  async payloads (pipelines) keep the wrapper and its limitations.
