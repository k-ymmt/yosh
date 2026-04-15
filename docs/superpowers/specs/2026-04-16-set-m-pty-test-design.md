# `set -m` PTY Test Design

## Date

2026-04-16

## Problem

The `set +m` / `set -m` toggle was implemented in commit `3aba708`, which calls `reset_job_control_signals()` and `init_job_control_signals()` respectively. A unit test (`test_reset_job_control_signals_after_init`) and an integration test (`test_set_plus_m_disables_job_control`) exist, but neither runs in an interactive/PTY context with a controlling terminal. Signal-level behavior (SIGTSTP ignore, SIGCHLD handling, process group management) can only be fully verified in a PTY environment.

## POSIX Reference

POSIX 2.11 (Job Control):
> "If job control is enabled, ... the shell shall ignore the SIGTSTP, SIGTTIN, and SIGTTOU signals."

Disabling monitor mode (`set +m`) restores default dispositions; re-enabling (`set -m`) must re-establish the job control signal configuration.

## Design

### Approach: Two focused PTY tests (Approach A)

Add two tests to `tests/pty_interactive.rs` using the existing `expectrl`-based PTY framework.

### Test 1: `test_pty_set_plus_m_disables_job_control`

**Purpose**: Verify that `set +m` disables job control in an interactive shell (indirect signal verification).

**Steps**:
1. `spawn_kish()` — interactive shell starts with monitor=on
2. `set +m\r` — disables monitor mode, calls `reset_job_control_signals()`
3. `fg\r` — should fail with "no job control" error
4. `exit_shell()`

**Assertion**: Output contains "no job control".

### Test 2: `test_pty_set_minus_m_reenables_job_control`

**Purpose**: Verify that `init_job_control_signals()` is effective after `set +m; set -m` toggle (direct signal-level verification via Ctrl+Z suspend).

**Steps**:
1. `spawn_kish()` — interactive shell starts with monitor=on
2. `set +m\r` — disables monitor mode
3. `set -m\r` — re-enables monitor mode, calls `init_job_control_signals()`
4. `sleep 100\r` — starts a foreground job
5. Send Ctrl+Z (`\x1a`) — should suspend the job
6. Wait for prompt (shell regains control)
7. `jobs\r` — should show "Stopped" job
8. `kill %1\r` — cleanup
9. `exit_shell()`

**Assertion**: `jobs` output contains "Stopped".

**What this proves**: Ctrl+Z suspend succeeding after the toggle requires all of:
- SIGTSTP ignored by the shell (shell does not stop itself)
- SIGCHLD handled via self-pipe (child state change detected)
- Process group management active (foreground job has its own pgid)

### Changes

- `tests/pty_interactive.rs`: Add 2 test functions (~50 lines total)

### Out of Scope

- Terminal state save/restore (separate TODO item)
- `set +m` direct signal verification (e.g., verifying SIGTSTP is SIG_DFL for the shell itself) — difficult to observe from PTY without intrusive instrumentation
- Non-interactive `set -m` behavior (already covered by integration tests)
