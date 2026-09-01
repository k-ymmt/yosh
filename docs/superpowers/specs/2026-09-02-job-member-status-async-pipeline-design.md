# Per-Member Job Status Tracking + Async Pipeline Direct Fork Design

**Date:** 2026-09-02
**Goal:** Track each member process's status individually in the job
table, and fork async pipeline members (`a | b &`) directly from the
shell (no wrapper subshell), extending the 2026-09-01 async
exec-in-place fix to the pipeline shape. Also: WCONTINUED handling in
the reaper, and operandless `wait` no longer skipping Stopped jobs.
Resolves four TODO.md items (reaped-status aggregate recording,
WCONTINUED, bare-wait-with-stopped-jobs, `update_status` per-process
tracking) and narrows the async-wrapper-residuals item.

## Problems

1. **Aggregate-only status** — `Job.status` is a single field
   overwritten by each member's wait event. For `a | b`, whichever
   member is reaped *last* defines the job status (`cat big | false` →
   `Done(0)`), and `cleanup_notified` records that one aggregate for
   *every* member pid, so `wait <member-pid>` after cleanup reports the
   wrong status.
2. **Async pipelines keep the wrapper** — `a | b &` forks a wrapper
   subshell that forks members; the job table tracks only the wrapper
   pid. Member stops are invisible to `jobs`/`bg`, and `$!` is the
   wrapper (bash: `$!` is the LAST member pid — verified empirically
   2026-09-02, bash 3.2: `[1] <last-pid>` notice, `$!` = last pid,
   `jobs -p` = pgid leader = first pid).
3. **No WCONTINUED** — a background job stopped and resumed by an
   external `kill -CONT` stays displayed Stopped in `jobs`.
4. **Bare `wait` skips Stopped jobs** — returns 0 immediately and
   clears the reaped-status map; dash/POSIX wait for termination.

## Design

### 1. Per-member statuses (`src/env/jobs/model.rs`, `mod.rs`)

`Job` gains `member_statuses: Vec<JobStatus>`, parallel to `pids`.
`Job.status` stays as the cached **aggregate**, recomputed on every
member update:

- any member `Running` → `Running` (bash: a job is stopped only when
  every process is stopped or dead)
- else any member `Stopped` → `Stopped(sig)` (sig of the last stopped
  member in pipeline order)
- else all terminal → the **last member's** terminal status (POSIX: a
  pipeline's status is its last element's)

New `Job` methods:
- `member_status(pid) -> Option<JobStatus>`
- `set_member_status(pid, st) -> bool` (internal)
- `aggregate_status() -> JobStatus` (internal)
- `resume_stopped_members()` — Stopped→Running + recompute (fg/bg)
- `stop_live_members(sig)` — Running→Stopped(sig) + recompute
  (foreground stop recording: the whole pgrp received the stop signal)

`JobTable::update_status(pid, st)` sets the member status and
recomputes; `notified` resets only when the aggregate changed (for
single-pid jobs this preserves the current always-reset behavior).

`cleanup_notified` records each member's **own** wait-style status in
the reaped map (falling back to the aggregate if a member is somehow
non-terminal — unreachable through real update paths since a cleanable
job's aggregate being terminal implies all members terminal).

### 2. Member-aware `wait` (`src/exec/job_control.rs`)

- The already-done fast path and the ECHILD fallback consult
  `member_status(pid)` instead of the job aggregate, so
  `wait <member-pid>` reports that member's own status.
- Operandless `wait` targets all member pids of `Running` **and**
  `Stopped` jobs (POSIX: wait until all known process IDs terminate; a
  stopped job has not terminated — dash agrees, blocking until it is
  continued and exits). The premature `clear_reaped` disappears with
  the early return.
- `%jobspec` operands push **all** member pids of the job (bash waits
  for the whole job); pids are pushed in pipeline order so the final
  reported status is the last member's — `wait %1` on `a | b` returns
  b's status (bash parity, verified).

### 3. WCONTINUED (`src/exec/control.rs::reap_zombies`)

Add `WaitPidFlag::WCONTINUED`; on `WaitStatus::Continued(pid)`, update
the member to `Running`. Covers the TODO repro
(`sh -c 'kill -STOP $$; ...' &` + external `kill -CONT`).
`wait_for_foreground_job` keeps WUNTRACED-only: it returns at the
first stop, and `fg`/`bg` already set members Running before SIGCONT.

### 4. Async pipeline direct fork (`src/exec/pipeline.rs`)

`exec_async` dispatches payloads shaped
`rest.is_empty() && !negated && commands.len() >= 2` to a new
`exec_async_pipeline(&Pipeline)`. It forks the N members directly from
the shell — no wrapper:

- Child i mirrors the async-wrapper setup exactly: `setpgid` into the
  job's own pgrp (leader = member 0) monitor or not, trap/job-table
  subshell reset, monitor → `setup_background_shell_child_signals` +
  `monitor = false`, non-monitor → SIGINT/SIGQUIT trap-Ignore entries +
  `reset_shell_child_signals` + stdin `/dev/null` (member 0 only —
  members i>0 get the pipe), then pipe dup2s.
- A member whose command is `Command::Simple` sets
  `async_exec_in_place` so an external member execs **in place** —
  without this the member subshell blocks in waitpid over a grandchild
  and stops stay invisible one level down, defeating the point.
- Members run `exec_command` + `execute_exit_trap` + `exit_child`
  (bash fires member EXIT traps; mirrors the foreground member path).
- All signals are blocked across the fork loop and restored on both
  sides (same self-pipe race protection as `exec_async`; the TODO's
  fork-race item lists pipeline forks as a latent site — the new path
  starts protected).
- Fork failure mid-loop: SIGTERM+SIGKILL and reap the members forked
  so far (mirrors `exec_multi_pipeline`).
- Parent: `add_job(pgid, children, preview, false)`, then
  `set_last_bg_pid(last_member)` and `[n] <last-member-pid>` notice
  (bash parity — NOT the pgid leader).

Kept on the wrapper path (residuals): `a && b &`, `! p &`, compounds,
`command cmd &`, builtins/functions with nested externals, and
compound members *inside* an async pipeline (their nested externals
still run one fork down).

### 5. Pipeline command display (`src/exec/mod.rs`)

`preview_pipeline(&Pipeline) -> Option<String>` renders every member's
literal words joined with `" | "` (None if any member is non-simple or
non-literal). `preview_command` uses it (so `%str` job specs match the
full string, still prefix-compatible), and `exec_multi_pipeline`'s
foreground job uses it instead of the `"(pipeline)"` placeholder
(which remains the fallback).

## Observable changes (intended)

- `a | b &`: `$!` = last member pid; direct children (no wrapper);
  member stops visible to `jobs`/`bg`; `wait $!` = b's own status;
  `jobs` shows `a | b`.
- `wait <member-pid>` after notification cleanup reports that member's
  own status.
- `jobs` shows Running again after an externally continued job.
- Bare `wait` blocks on stopped jobs until they terminate.

Not changed: foreground pipeline status logic (pipefail/last-element
via `process_statuses`), single-command async behavior, job ids,
fg/bg/kill pgid semantics, `jobs -p` (pgid leader).

## Tests

- Unit (jobs): aggregate transitions (running/stopped/terminal,
  last-member terminal status), per-member cleanup recording,
  resume/stop member helpers; existing tests encoding aggregate-
  overwrite semantics updated.
- Unit (exec): member-aware wait fast path.
- Integration (`tests/signals.rs`): `$!` = last member + `wait $!`
  status for `a | b &`; `$PPID` of members = shell; stopped-then-
  continued job's exit status via `wait $!`; bare `wait` blocking on a
  stopped job until continued.
- PTY (`tests/pty_interactive.rs`): background pipeline member SIGTTIN
  stop shows Stopped once other members exit; `jobs` after external
  STOP/CONT shows Stopped then Running.
