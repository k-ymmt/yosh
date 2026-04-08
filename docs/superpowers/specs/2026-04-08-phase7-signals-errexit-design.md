# Phase 7: Signals + Errexit — Design Specification

## Overview

Phase 7 adds full POSIX signal handling, errexit (`set -e`) with strict exception rules, `kill` and `wait` builtins, and process group management. This completes the shell's signal infrastructure that was deferred from Phase 6 (trap registration only).

### Scope

- **In scope:** Signal handling via self-pipe trick, errexit with all POSIX exception contexts (closure-based suppression), `kill` builtin (extended forms), `wait` builtin (signal interruption), process group management (`setpgid`), background job PID tracking, subshell trap reset, async command SIGINT/SIGQUIT ignore
- **Out of scope:** Job control / monitor mode (`-m`, `-b`), interactive features (ignoreeof, line editing), ERR trap (bash extension)

### Design Decisions

- **Signal handling:** Self-pipe trick for safe async signal delivery. Signals are deferred during foreground command execution and processed after `waitpid` returns.
- **Errexit suppression:** Closure-based API (`with_errexit_suppressed`) instead of manual increment/decrement or RAII guards. Provides compile-time scope safety without borrow checker conflicts.
- **Process groups:** `setpgid` double-call pattern (parent + child) to avoid race conditions. No terminal process group management (deferred to job control phase).
- **SIGCHLD:** Not handled via self-pipe. Detected via `poll` EINTR return to avoid interference with `waitpid`.

---

## 1. signal.rs Module — Self-Pipe + Signal Handlers

### New file: `src/signal.rs`

```rust
use std::sync::OnceLock;
use std::os::fd::RawFd;

/// Global self-pipe fd pair
/// Signal handlers write here; main loop reads
static SELF_PIPE: OnceLock<(RawFd, RawFd)> = OnceLock::new();
// (read_fd, write_fd)

/// Signals handled by the shell
pub const HANDLED_SIGNALS: &[(i32, &str)] = &[
    (1, "HUP"), (2, "INT"), (3, "QUIT"), (14, "ALRM"), (15, "TERM"),
    (10, "USR1"), (12, "USR2"),
];

/// Full signal table for kill -l and name/number conversion
pub const SIGNAL_TABLE: &[(i32, &str)] = &[
    (1, "HUP"), (2, "INT"), (3, "QUIT"), (6, "ABRT"),
    (9, "KILL"), (10, "USR1"), (12, "USR2"), (13, "PIPE"),
    (14, "ALRM"), (15, "TERM"),
    // Platform-dependent signals use cfg attributes
];
```

### Functions

- `init_signal_handling()`: Create self-pipe with `O_NONBLOCK | O_CLOEXEC`, register `sigaction` handlers for all `HANDLED_SIGNALS`. Handler writes signal number as 1 byte to write_fd (async-signal-safe).
- `drain_pending_signals() -> Vec<i32>`: Non-blocking read from read_fd, returns list of pending signal numbers.
- `ignore_signal(sig: i32)`: Set signal disposition to `SIG_IGN`.
- `default_signal(sig: i32)`: Set signal disposition to `SIG_DFL`.
- `reset_child_signals()`: Close self-pipe fds, reset all `HANDLED_SIGNALS` to default. Called in child processes after `fork`.
- `self_pipe_read_fd() -> RawFd`: Returns read fd for `poll` in `wait` builtin.
- `signal_name_to_number(name: &str) -> Result<i32, String>`: Name-to-number lookup via `SIGNAL_TABLE`.
- `signal_number_to_name(num: i32) -> Option<&'static str>`: Number-to-name lookup.

### Signal Handler

```rust
extern "C" fn signal_handler(sig: libc::c_int) {
    // async-signal-safe: write only
    let byte = sig as u8;
    let fd = SELF_PIPE.get().unwrap().1; // write_fd
    unsafe { libc::write(fd, &byte as *const u8 as *const libc::c_void, 1); }
}
```

---

## 2. Errexit — Closure-Based Suppression

### Executor Extension

```rust
pub struct Executor {
    pub env: ShellEnv,
    errexit_suppressed_depth: usize,  // NEW
}
```

### Core Methods

```rust
impl Executor {
    /// Execute closure within errexit-suppressed context
    fn with_errexit_suppressed<F, R>(&mut self, f: F) -> R
    where F: FnOnce(&mut Self) -> R {
        self.errexit_suppressed_depth += 1;
        let result = f(self);
        self.errexit_suppressed_depth -= 1;
        result
    }

    /// Check if errexit is active and not suppressed
    fn should_errexit(&self) -> bool {
        self.env.options.errexit
            && self.errexit_suppressed_depth == 0
    }

    /// Errexit check after command execution
    /// Non-zero status + should_errexit() -> execute EXIT trap and exit
    fn check_errexit(&mut self, status: i32) -> i32 {
        if status != 0 && self.should_errexit() {
            self.execute_exit_trap();
            std::process::exit(status);
        }
        status
    }
}
```

### POSIX Suppression Contexts (5 locations)

| Context | Call site | Change |
|---------|-----------|--------|
| `if`/`elif` condition | `exec_if` | `self.with_errexit_suppressed(\|e\| e.exec_body(condition))` |
| `while`/`until` condition | `exec_loop` | `self.with_errexit_suppressed(\|e\| e.exec_body(condition))` |
| `!` pipeline | `exec_and_or` | Suppress when executing negated pipeline |
| AND-OR non-final commands | `exec_and_or` | Suppress all except the last command |
| `trap` action | `process_pending_signals` | Suppress during trap command evaluation |

### exec_body with Errexit Check

```rust
fn exec_body(&mut self, body: &[CompleteCommand]) -> i32 {
    let mut status = 0;
    for cmd in body {
        status = self.exec_complete_command(cmd);
        if self.env.flow_control.is_some() {
            break;
        }
        self.check_errexit(status);
        self.process_pending_signals();
    }
    status
}
```

### exec_and_or Changes

```rust
fn exec_and_or(&mut self, list: &AndOrList) -> i32 {
    let has_rest = !list.rest.is_empty();

    let mut status = if list.first.negated {
        self.with_errexit_suppressed(|e| e.exec_pipeline(&list.first))
    } else if has_rest {
        self.with_errexit_suppressed(|e| e.exec_pipeline(&list.first))
    } else {
        self.exec_pipeline(&list.first)
    };

    for (i, (op, pipeline)) in list.rest.iter().enumerate() {
        let is_last = i == list.rest.len() - 1;
        let should_run = match op {
            AndOrOp::And => status == 0,
            AndOrOp::Or => status != 0,
        };
        if !should_run { continue; }

        status = if pipeline.negated || !is_last {
            self.with_errexit_suppressed(|e| e.exec_pipeline(pipeline))
        } else {
            self.exec_pipeline(pipeline)
        };
    }
    status
}
```

---

## 3. Signal Processing Flow

### Check Insertion Points

Signals are processed at three natural boundaries:

1. `exec_body`: After each `CompleteCommand` execution
2. `exec_loop`: After each loop iteration
3. `main.rs`: After top-level command loop

### process_pending_signals

```rust
fn process_pending_signals(&mut self) {
    let signals = signal::drain_pending_signals();
    for sig in signals {
        match self.env.traps.get_signal_trap(sig) {
            Some(TrapAction::Command(cmd)) => {
                let cmd = cmd.clone();
                self.with_errexit_suppressed(|exec| {
                    exec.eval_string(&cmd);
                });
            }
            Some(TrapAction::Ignore) => { /* ignored */ }
            Some(TrapAction::Default) | None => {
                self.handle_default_signal(sig);
            }
        }
    }
}
```

### Default Signal Behavior

```rust
fn handle_default_signal(&mut self, sig: i32) {
    // All default-handled signals terminate the shell
    self.execute_exit_trap();
    std::process::exit(128 + sig);
}
```

### Foreground Command Signal Deferral

Signals accumulate in the self-pipe during `waitpid`. After `waitpid` returns, `process_pending_signals` processes them. The OS handles signal delivery to child processes via process groups naturally.

### Subshell Trap Reset

```rust
fn exec_subshell(&mut self, body: &[CompleteCommand]) -> i32 {
    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            // POSIX: reset non-ignored traps to default
            self.env.traps.reset_non_ignored();
            signal::reset_child_signals();
            let status = self.exec_body(body);
            std::process::exit(status);
        }
        // ...
    }
}
```

`TrapStore::reset_non_ignored()`: Resets `Command(_)` to `Default`. `Ignore` is preserved (POSIX requirement).

---

## 4. `wait` Builtin — Self-Pipe Integration

### Background Job Tracking (ShellEnv Extension)

```rust
// src/env/mod.rs
pub struct BgJob {
    pub pid: Pid,
    pub status: Option<i32>,  // None = running, Some = exited
}

pub struct ShellEnv {
    // ... existing fields
    pub bg_jobs: Vec<BgJob>,  // NEW
}
```

Jobs are added to `bg_jobs` when `&` async commands are launched. `$!` reads from `bg_jobs.last()`.

### Syntax

```
wait            -> Wait for all background jobs
wait pid...     -> Wait for specified PIDs
```

### Implementation (self-pipe + poll)

```rust
fn builtin_wait(args: &[String], executor: &mut Executor) -> i32 {
    let target_pids = if args.is_empty() {
        executor.env.bg_jobs.iter()
            .filter(|j| j.status.is_none())
            .map(|j| j.pid)
            .collect()
    } else {
        parse_pids(args)
    };

    for pid in &target_pids {
        loop {
            match waitpid(*pid, Some(WaitPidFlag::WNOHANG)) {
                Ok(WaitStatus::Exited(_, code)) => {
                    update_bg_job(&mut executor.env, *pid, code);
                    break;
                }
                Ok(WaitStatus::Signaled(_, sig, _)) => {
                    let code = 128 + sig as i32;
                    update_bg_job(&mut executor.env, *pid, code);
                    break;
                }
                Ok(WaitStatus::StillAlive) => {
                    let result = poll_for_event(signal::self_pipe_read_fd());
                    if result == PollResult::Signal {
                        let signals = signal::drain_pending_signals();
                        executor.process_pending_signals();
                        return 128 + signals[0];
                    }
                    // PollResult::ChildReady -> re-check waitpid
                }
                Err(_) | _ => break,
            }
        }
    }
    last_exit_status(&executor.env, &target_pids)
}
```

### poll_for_event

```rust
fn poll_for_event(self_pipe_fd: RawFd) -> PollResult {
    let mut fds = [
        PollFd::new(self_pipe_fd, PollFlags::POLLIN),
    ];
    match poll(&mut fds, -1) {
        Ok(_) if fds[0].revents().contains(PollFlags::POLLIN) => {
            PollResult::Signal
        }
        Err(Errno::EINTR) => PollResult::ChildReady,
        _ => PollResult::ChildReady,
    }
}

enum PollResult {
    Signal,
    ChildReady,
}
```

### SIGCHLD Handling

SIGCHLD is NOT routed through the self-pipe. It is detected via `poll` returning `EINTR`. This avoids interference with `waitpid` semantics.

---

## 5. Process Group Management

### Pipeline Process Groups

```rust
// src/exec/pipeline.rs
fn exec_pipeline_commands(&mut self, commands: &[Command]) -> i32 {
    let mut pgid: Pid = Pid::from_raw(0);
    let mut children: Vec<Pid> = Vec::new();

    for (i, cmd) in commands.iter().enumerate() {
        match unsafe { fork() } {
            Ok(ForkResult::Child) => {
                let my_pid = nix::unistd::getpid();
                if i == 0 {
                    setpgid(my_pid, my_pid).ok(); // leader
                } else {
                    setpgid(my_pid, pgid).ok();
                }
                signal::reset_child_signals();
                // pipe setup + exec
            }
            Ok(ForkResult::Parent { child }) => {
                if i == 0 {
                    pgid = child;
                }
                // Double setpgid call to avoid race
                setpgid(child, pgid).ok();
                children.push(child);
            }
            Err(e) => { /* error handling */ }
        }
    }

    wait_pipeline_children(&children)
}
```

Double `setpgid` call: Execution order between parent and child is nondeterministic. Calling in both avoids the race where a child starts pipe operations before the parent has set its process group.

### Async Command Process Groups

```rust
fn exec_async(&mut self, cmd: &CompleteCommand) -> i32 {
    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            let pid = nix::unistd::getpid();
            setpgid(pid, pid).ok(); // own group
            // POSIX: async commands in non-interactive shell ignore SIGINT/SIGQUIT
            signal::ignore_signal(libc::SIGINT);
            signal::ignore_signal(libc::SIGQUIT);
            signal::reset_child_signals();
            // redirect stdin to /dev/null if no explicit redirect
            let status = self.exec_command(cmd);
            std::process::exit(status);
        }
        Ok(ForkResult::Parent { child }) => {
            setpgid(child, child).ok();
            self.env.bg_jobs.push(BgJob { pid: child, status: None });
            self.env.last_bg_pid = Some(child.as_raw());
            0
        }
        Err(_) => 1,
    }
}
```

### reset_child_signals (signal.rs)

```rust
pub fn reset_child_signals() {
    // Close self-pipe fds (not needed in child)
    if let Some(&(read_fd, write_fd)) = SELF_PIPE.get() {
        nix::unistd::close(read_fd).ok();
        nix::unistd::close(write_fd).ok();
    }
    // Reset handled signals to default
    for &(sig, _) in HANDLED_SIGNALS {
        default_signal(sig);
    }
}
```

---

## 6. `kill` Builtin

Regular builtin added to `src/builtin/mod.rs`.

### Supported Syntax

```
kill [-s signal_name] pid...     # Signal by name
kill [-signal_name] pid...       # -INT, -HUP, etc.
kill [-signal_number] pid...     # -9, -15, etc.
kill -l [exit_status]            # List signals / status-to-name
kill -- -pgid                    # Send to process group
kill 0                           # Send to own process group
```

Default signal is `SIGTERM` when not specified.

### Implementation

```rust
pub fn builtin_kill(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        eprintln!("kish: kill: usage: kill [-s sigspec | -signum] pid...");
        return 2;
    }

    if args[0] == "-l" {
        return kill_list(&args[1..]);
    }

    let (signal, pids) = parse_kill_args(args);

    let mut status = 0;
    for pid_str in pids {
        let pid = parse_pid(pid_str); // negative = process group
        if let Err(e) = nix::sys::signal::kill(
            Pid::from_raw(pid),
            Some(Signal::try_from(signal).unwrap())
        ) {
            eprintln!("kish: kill: ({}) - {}", pid_str, e);
            status = 1;
        }
    }
    status
}
```

### parse_kill_args

```rust
fn parse_kill_args(args: &[String]) -> (i32, &[String]) {
    if args[0] == "-s" {
        let sig = signal_name_to_number(&args[1]);
        (sig, &args[2..])
    } else if args[0] == "--" {
        (libc::SIGTERM, &args[1..])
    } else if args[0].starts_with('-') {
        let spec = &args[0][1..];
        if let Ok(num) = spec.parse::<i32>() {
            (num, &args[1..])
        } else {
            let sig = signal_name_to_number(spec);
            (sig, &args[1..])
        }
    } else {
        (libc::SIGTERM, &args[..])
    }
}
```

### kill -l Behavior

```
kill -l          -> HUP INT QUIT ... TERM ... (all signal names)
kill -l 130      -> INT (subtract 128, then number-to-name)
kill -l INT      -> 2 (name-to-number)
```

---

## 7. File Change Summary

### New Files

| File | Content |
|------|---------|
| `src/signal.rs` | Self-pipe, signal handlers, drain/check, signal table, name/number conversion |

### Modified Files

| File | Changes |
|------|---------|
| `src/exec/mod.rs` | `errexit_suppressed_depth` field, `with_errexit_suppressed`, `should_errexit`, `check_errexit`, `process_pending_signals`, `handle_default_signal`, `exec_async`, errexit checks in `exec_if`/`exec_loop`/`exec_and_or`/`exec_body` |
| `src/exec/pipeline.rs` | `setpgid` process group management (double-call), child signal reset |
| `src/exec/command.rs` | `signal::reset_child_signals()` in forked external commands |
| `src/env/mod.rs` | `BgJob` struct, `bg_jobs: Vec<BgJob>` in ShellEnv, `TrapStore::reset_non_ignored()`, `TrapStore::get_signal_trap()` |
| `src/builtin/mod.rs` | `builtin_kill`, `builtin_wait`, `classify_builtin` updates for "kill"/"wait" |
| `src/builtin/special.rs` | `builtin_exit` calls `process_pending_signals` before exit |
| `src/main.rs` | `signal::init_signal_handling()` at startup, `process_pending_signals` at shutdown |

---

## 8. Testing Strategy

### Unit Tests

| Module | Tests |
|--------|-------|
| `signal.rs` | Self-pipe init, `signal_name_to_number`/`signal_number_to_name` conversion, `SIGNAL_TABLE` completeness |
| `exec/mod.rs` — errexit | `should_errexit` state checks, `with_errexit_suppressed` nest count |
| `env/mod.rs` — TrapStore | `reset_non_ignored`: Command->Default, Ignore preserved |
| `env/mod.rs` — BgJob | Job add, status update |
| `builtin/mod.rs` — kill | `parse_kill_args` all syntax patterns (`-s NAME`, `-9`, `-NAME`, `--`, default), `kill -l` output |

### Integration Tests

Tests are added to `tests/errexit.rs` (new) and `tests/signals.rs` (new or extend existing).

#### Errexit Tests

| Test | Description |
|------|-------------|
| Basic | `set -e; false; echo unreachable` -> no output, exit 1 |
| if condition suppression | `set -e; if false; then echo no; fi; echo reached` -> "reached" |
| while condition suppression | `set -e; while false; do :; done; echo reached` -> "reached" |
| until condition suppression | `set -e; until true; do :; done; echo reached` -> "reached" |
| `!` pipeline suppression | `set -e; ! false; echo reached` -> "reached" |
| AND-OR suppression | `set -e; false \|\| true; echo reached` -> "reached" |
| AND-OR final exit | `set -e; true && false; echo unreachable` -> no output |
| Nested suppression | `set -e; if ! false; then echo ok; fi; echo reached` -> "ok\nreached" |
| trap action suppression | `set -e; trap 'false; echo trap' EXIT; exit 0` -> "trap" |
| Subshell | `set -e; (false); echo unreachable` -> no output |
| Function | `set -e; f() { false; }; f; echo unreachable` -> no output |

#### Signal Tests

| Test | Description |
|------|-------------|
| trap INT execution | `trap 'echo caught' INT; kill -INT $$; echo after` -> "caught\nafter" |
| trap reset | `trap 'echo x' INT; trap - INT` -> default restored |
| Subshell trap reset | `trap 'echo x' INT; (trap)` -> no traps |
| Ignore preserved in subshell | `trap '' INT; (trap -p INT)` -> `trap -- '' INT` |
| wait signal interruption | `sleep 100 & wait` + SIGINT -> exit 130 |
| Async SIGINT ignore | `trap 'echo caught' INT; : & kill -INT $!` -> ignored |

#### kill Tests

| Test | Description |
|------|-------------|
| kill default | `sleep 100 & kill $!; wait $!` -> status 143 (128+15) |
| kill -s | `sleep 100 & kill -s INT $!; wait $!` -> status 130 |
| kill -9 | `sleep 100 & kill -9 $!; wait $!` -> status 137 |
| kill -l | `kill -l` -> signal names, `kill -l 130` -> "INT" |
| kill 0 | Sends to own process group |

#### wait Tests

| Test | Description |
|------|-------------|
| wait basic | `sleep 0.1 & wait` -> exit 0 |
| wait PID | `sleep 0.1 & pid=$!; wait $pid` -> status 0 |
| wait invalid PID | `wait 99999` -> error, status 127 |
