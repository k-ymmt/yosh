# Job Control (Phase 6 Known Limitations) — Design Specification

## Overview

Implement full POSIX job control to resolve the Phase 6 known limitations: `-m` (monitor) flag has no behavioral effect and `-b` (notify) flag has no effect. This adds terminal control, job suspend/resume, `fg`/`bg`/`jobs` builtins, and asynchronous job notification.

### Scope

- **In scope:** JobTable data structure, terminal control (tcsetpgrp), SIGTSTP/SIGCONT/SIGTTIN/SIGTTOU/SIGCHLD signal handling, `fg`/`bg`/`jobs` builtins, `-m` flag behavior (auto-enable in interactive), `-b` flag behavior (immediate notification), job notification at prompt, WUNTRACED waitpid for stopped job detection, `%n`/`%%`/`%+`/`%-` job specifiers
- **Out of scope:** `%string`/`%?string` job specifiers (future), `disown` builtin (future), `suspend` builtin (future)

### Design Decisions

- **Approach A (New JobTable module):** Create `src/env/jobs.rs` with dedicated `JobTable` replacing `BgJob` + `Vec<BgJob>`
- **POSIX compliance:** `-m` auto-enabled in interactive mode, disabled in scripts; can be toggled with `set -m`/`set +m`
- **Notification format:** POSIX standard format (`[n]+ Status  command`)
- **Terminal control:** `tcsetpgrp` for foreground/background group switching via stdin fd

---

## 1. JobTable Data Structure (`src/env/jobs.rs` — new file)

### Types

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Stopped(Signal),       // SIGTSTP, SIGSTOP, etc.
    Done(i32),             // exit code
    Terminated(Signal),    // killed by signal -> 128 + signum
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: u32,           // [1], [2], ...
    pub pgid: Pid,         // process group ID (= first process PID)
    pub pids: Vec<Pid>,    // all PIDs in pipeline
    pub command: String,   // display command string
    pub status: JobStatus,
    pub notified: bool,    // whether Done/Terminated has been reported
    pub foreground: bool,  // foreground job flag
}

#[derive(Debug, Clone, Default)]
pub struct JobTable {
    jobs: HashMap<u32, Job>,
    next_id: u32,
    current: Option<u32>,    // %+ / %% (most recent job)
    previous: Option<u32>,   // %-
}
```

### JobTable Methods

- `add_job(pgid: Pid, pids: Vec<Pid>, command: String, foreground: bool) -> u32` — add job, assign ID, update current/previous
- `remove_job(id: u32)` — remove completed job, update current/previous
- `get(id: u32) -> Option<&Job>` / `get_mut(id: u32) -> Option<&mut Job>` — job lookup
- `current_job() -> Option<&Job>` / `previous_job() -> Option<&Job>` — `%+` / `%-` references
- `update_status(pid: Pid, status: JobStatus)` — update job status from waitpid result
- `find_by_pgid(pgid: Pid) -> Option<&mut Job>` — lookup by PGID
- `pending_notifications() -> Vec<&Job>` — unnotified Done/Terminated jobs
- `format_job(job: &Job, current_id: Option<u32>, previous_id: Option<u32>) -> String` — POSIX display format
- `last_bg_pid() -> Option<Pid>` — for `$!` expansion
- `resolve_job_spec(spec: &str) -> Option<u32>` — parse `%n`, `%%`, `%+`, `%-`
- `all_jobs() -> impl Iterator<Item = &Job>` — sorted by ID for display

### Current/Previous Job Update Rules

- When a new job is added: new job becomes current, old current becomes previous
- When current job is removed: previous becomes current, next most recent becomes previous
- When a job is brought to foreground: it becomes current

### Terminal Control Functions (in `src/env/jobs.rs`)

```rust
const TERMINAL_FD: RawFd = 0;  // stdin

pub fn give_terminal(pgid: Pid) -> Result<(), nix::Error> {
    nix::unistd::tcsetpgrp(TERMINAL_FD, pgid)
}

pub fn take_terminal(shell_pgid: Pid) -> Result<(), nix::Error> {
    nix::unistd::tcsetpgrp(TERMINAL_FD, shell_pgid)
}
```

---

## 2. ShellEnv Changes (`src/env/mod.rs`)

### Removed

- `pub struct BgJob` — replaced by `Job` in `jobs.rs`
- `bg_jobs: Vec<BgJob>` — replaced by `jobs: JobTable`
- `last_bg_pid: Option<i32>` — moved to `JobTable::last_bg_pid()`

### Added

- `pub jobs: JobTable` — full job table
- `pub shell_pgid: Pid` — shell's own process group ID (set at startup)

### Modified

All code referencing `env.bg_jobs` and `env.last_bg_pid` updated to use `env.jobs`.

---

## 3. Signal Changes (`src/signal.rs`)

### SIGNAL_TABLE Additions

```rust
(17, "CHLD"), (18, "CONT"), (19, "STOP"), (20, "TSTP"),
(21, "TTIN"), (22, "TTOU")
```

### New Functions

```rust
/// Set up job control signals for the shell process.
/// Called when monitor mode is enabled.
/// Ignores SIGTSTP, SIGTTIN, SIGTTOU so the shell itself is not stopped.
/// Adds SIGCHLD to self-pipe handlers.
pub fn init_job_control_signals();

/// Restore job control signals to defaults.
/// Called when monitor mode is disabled.
pub fn reset_job_control_signals();

/// Set up signals for a foreground child process.
/// Restores SIGTSTP, SIGTTIN, SIGTTOU to SIG_DFL.
pub fn setup_foreground_child_signals(ignored: &[i32]);

/// Set up signals for a background child process.
/// Ignores SIGTTIN (prevent TTY read).
pub fn setup_background_child_signals(ignored: &[i32]);
```

### SIGCHLD Handling

Add SIGCHLD to self-pipe mechanism when monitor mode is active. This allows the shell to detect child state changes asynchronously for `-b` (notify) support.

---

## 4. Executor Changes (`src/exec/mod.rs`)

### `exec_async()` Rewrite

1. Fork child process
2. Child: `setpgid(pid, pid)` + setup_background_child_signals + execute
3. Parent: `setpgid(child, child)` + `jobs.add_job(child, vec![child], command, false)`
4. Print `[n] PID` format
5. Return 0

### `reap_zombies()` Enhancement

- Use `WUNTRACED | WNOHANG` flags in waitpid to detect both exits and stops
- Map `WaitStatus::Stopped(pid, sig)` to `JobStatus::Stopped(sig)` in JobTable
- Map `WaitStatus::Exited/Signaled` to `JobStatus::Done/Terminated` in JobTable

### `builtin_wait()` Migration

- Replace `env.bg_jobs` references with `env.jobs` methods
- Support `wait %n` job spec syntax in addition to PID

### New Builtin Dispatch

Add `fg`, `bg`, `jobs` to regular builtin classification and dispatch in `exec_simple_command`.

### Foreground Job Wait

New method `wait_for_foreground_job(job_id: u32) -> i32`:
1. `waitpid(-pgid, WUNTRACED)` for all PIDs in job
2. On exit: update status to Done/Terminated, take_terminal, remove job
3. On stop: update status to Stopped, take_terminal, print notification
4. Return exit status or 128+signum

---

## 5. Pipeline Changes (`src/exec/pipeline.rs`)

### `exec_multi_pipeline()` Enhancement

When monitor mode is active:
1. After forking all children, register as foreground job in JobTable
2. `give_terminal(pgid)` to hand terminal to pipeline
3. Wait with `WUNTRACED` for all children
4. On any child stopped by SIGTSTP: mark job as Stopped, `take_terminal(shell_pgid)`
5. On all children exited: mark job as Done, `take_terminal(shell_pgid)`, remove job

When monitor mode is inactive (existing behavior):
- No change to current pipeline execution

### `wait_for_child()` Enhancement

Add WUNTRACED support, return a richer result type that distinguishes exit/signal/stop.

---

## 6. `fg` / `bg` / `jobs` Builtins

### `jobs` (Regular Builtin)

```
jobs             -> list all jobs (POSIX format)
jobs -l          -> list with PIDs
jobs -p          -> list PGIDs only
```

Output format:
```
[1]+ Running                 sleep 100 &
[2]- Stopped                 vim
```

- `+` = current job, `-` = previous job
- Done jobs displayed then marked as notified

### `fg` (Regular Builtin)

```
fg [%job_id]     -> bring job to foreground
fg               -> bring current job (%+) to foreground
```

Behavior:
1. Resolve job spec (default: current job)
2. Check monitor mode; error if disabled: `kish: fg: no job control`
3. Set `job.foreground = true`, `job.status = Running`
4. If stopped: `kill(-pgid, SIGCONT)`
5. `give_terminal(pgid)`
6. `waitpid(-pgid, WUNTRACED)` — wait for completion or stop
7. `take_terminal(shell_pgid)`
8. Return exit status or update as Stopped

### `bg` (Regular Builtin)

```
bg [%job_id]     -> resume stopped job in background
bg               -> resume current job (%+) in background
```

Behavior:
1. Resolve job spec (default: current job)
2. Check monitor mode; error if disabled: `kish: bg: no job control`
3. Check job is Stopped; error if Running
4. Set `job.status = Running`, `job.foreground = false`
5. `kill(-pgid, SIGCONT)`
6. Print `[n]+ command &`

### Job Specifier Parsing

- `%n` — job number n
- `%%` / `%+` — current job
- `%-` — previous job

---

## 7. `-m` (Monitor) Flag Behavior

### When Enabled

- Job control signals initialized (SIGTSTP/SIGTTIN/SIGTTOU ignored by shell, SIGCHLD handled)
- Terminal control active (tcsetpgrp used)
- `fg`/`bg` builtins functional
- Foreground jobs get terminal control
- Background jobs tracked with full job lifecycle

### When Disabled (Default for Scripts)

- Existing behavior preserved (setpgid-only isolation)
- `fg`/`bg` return error: `kish: fg: no job control`
- `jobs` still works (background job listing)
- No terminal control (no tcsetpgrp calls)
- No SIGTSTP/Ctrl+Z support

### Auto-Enable in Interactive Mode

In `Repl::new()`:
```rust
executor.env.options.monitor = true;
signal::init_job_control_signals();
executor.env.shell_pgid = nix::unistd::getpgrp();
```

---

## 8. `-b` (Notify) Flag Behavior

### When Enabled (`set -b`)

- Job state changes reported immediately after each command completes
- Implementation: in `exec_complete_command()`, after `reap_zombies()`, check and display notifications

### When Disabled (Default)

- Job state changes reported only before prompt display
- Implementation: in `Repl::run()` loop, before prompt display, check and display notifications

### Notification Format

```
[1]+  Done                    sleep 5
[2]-  Stopped                 vim file.txt
[1]+  Terminated(15)          sleep 100
```

---

## 9. New and Modified Files Summary

### New Files
- `src/env/jobs.rs` — JobTable, Job, JobStatus, terminal control functions

### Modified Files
- `src/env/mod.rs` — remove BgJob, add `jobs: JobTable` + `shell_pgid: Pid`, remove `last_bg_pid`
- `src/exec/mod.rs` — rewrite exec_async, enhance reap_zombies (WUNTRACED), migrate builtin_wait, add fg/bg/jobs dispatch, add wait_for_foreground_job
- `src/exec/pipeline.rs` — add foreground job registration + tcsetpgrp + WUNTRACED wait when monitor active
- `src/signal.rs` — add CHLD/CONT/STOP/TSTP/TTIN/TTOU to SIGNAL_TABLE, add job control signal functions
- `src/interactive/mod.rs` — auto-enable monitor mode, add job notification display before prompt
- `src/builtin/mod.rs` — add fg/bg/jobs as regular builtins
- `src/expand/param.rs` — change `$!` source from `env.last_bg_pid` to `env.jobs.last_bg_pid()`

---

## 10. Testing Strategy

### Unit Tests (`src/env/jobs.rs`)

- JobTable add/remove/update/get operations
- Current/previous job automatic tracking
- Job format output (POSIX format)
- Job specifier parsing (`%n`, `%%`, `%+`, `%-`)
- Terminal control function signatures (mock-friendly)

### Integration Tests (`tests/`)

- `jobs` builtin output format
- Background job `[n] PID` output
- `wait` builtin with JobTable integration
- `-m` disabled: `fg`/`bg` return error
- `set -b` notification timing (where testable without TTY)

### E2E Tests (`e2e/`)

- `sleep 1 &; jobs` — job listing display
- `set +m; fg` — error when monitor disabled
- Background job completion notification
- Multiple background jobs with correct numbering

### Manual Testing (TTY-dependent)

- `Ctrl+Z` to stop foreground job
- `fg` to resume stopped job
- `bg` to resume in background
- `fg %2` job specifier
- `-b` flag immediate notification
- Pipeline stop/resume (`cat | sort` then Ctrl+Z, fg)

---

## 11. Known Limitations (Deferred)

- `%string` / `%?string` job specifiers — prefix/substring matching deferred to future enhancement
- `disown` builtin — not part of POSIX, deferred
- `suspend` builtin — deferred
- Terminal state save/restore (tcgetattr/tcsetattr) — may be needed if jobs modify terminal settings; deferred unless issues arise during testing
