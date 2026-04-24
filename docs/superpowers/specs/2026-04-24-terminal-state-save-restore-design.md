# Design: Terminal State Save/Restore for Job Control

**Date:** 2026-04-24
**Status:** Design approved, awaiting implementation
**Target TODO entry:** Job Control: Known Limitations (line 7) — "Terminal state save/restore (tcgetattr/tcsetattr) — jobs that modify terminal settings may leave terminal in bad state"

## Problem

When a foreground job that modifies terminal settings (e.g. `vim` running in raw mode, `stty raw; sleep 10`, any full-screen TUI) is suspended with Ctrl-Z and later resumed with `fg`, yosh does not restore the terminal to a sensible state on either side of the transition:

1. **Suspend side**: on `WaitStatus::Stopped` detection in `wait_for_foreground_job` (`src/exec/mod.rs:821-838`), yosh takes the terminal back with `tcsetpgrp` but does not call `tcsetattr`. The terminal stays in whatever mode the stopped child left it (often raw mode), so the shell's notification line and the subsequent prompt render incorrectly (no echo, no line-editing).
2. **Resume side**: on `fg` (`src/exec/mod.rs:621-698`), yosh hands the terminal back to the job's process group with `tcsetpgrp` but does not restore the termios state the job was running under. The child resumes in whatever mode the shell left behind — typically cooked mode — instead of the raw mode the child expected.

The standard fix, documented in the GNU libc manual's "Implementing a Job Control Shell," is to snapshot termios per-job at suspend time and replay it on resume, plus snapshot the shell's own termios once at interactive startup and replay it whenever control returns to the shell.

yosh has none of this. `src/env/jobs.rs` has no termios fields, and no file in `src/` calls `tcgetattr`/`tcsetattr` (only PTY tests read termios for assertion purposes).

## Scope

**In scope:**
- Per-job termios save on `WaitStatus::Stopped` detection and restore on `fg`.
- Shell termios snapshot at interactive startup; restore whenever a foreground job stops or exits.
- `bg` → `fg` promotion path (job stopped in bg then moved to fg uses the same saved termios).

**Out of scope:**
- Edge cases where a pipeline has some members stopped and others running (POSIX semantics for per-process termios are under-specified; yosh currently tracks a single `JobStatus` per pipeline).
- Handling of shell itself modifying termios (e.g. user running `stty -echo` at the yosh prompt) — `shell_tmodes` is captured once at startup and not refreshed.
- Adding a macOS CI job to exercise this on darwin — tracked separately in TODO.md line 80.

## Approach

Follow the classic glibc manual pattern:

1. `ShellEnv.process.jobs.shell_tmodes: Option<Termios>` — captured once at interactive REPL entry. `None` in non-interactive / non-monitor mode.
2. `Job.saved_tmodes: Option<Termios>` — `None` at job creation. Set on suspend (via `capture_tty_termios`). Used as the restore target on `fg`; falls back to `shell_tmodes` when `None`.
3. All tcgetattr / tcsetattr calls live in a new module `src/exec/terminal_state.rs` with two pure functions. Callers decide *when* to save/restore; the helpers only know *how*.

Line editor interaction is already safe: raw mode is enabled/disabled per `read_line` call (`src/interactive/line_editor.rs:447,449,937,949`), and the tcgetattr/tcsetattr window around fork/stop sits entirely in the cooked-mode interval between `read_line` exit and the next `read_line` entry (confirmed in exploration — see §5).

## Architecture

### New module: `src/exec/terminal_state.rs`

```rust
use nix::sys::termios::{tcgetattr, tcsetattr, SetArg, Termios};
use std::os::fd::BorrowedFd;

/// Capture the controlling terminal's current termios.
/// Returns `Ok(None)` when stdin is not a TTY (pipes, CI, redirected).
pub fn capture_tty_termios() -> nix::Result<Option<Termios>>;

/// Apply a saved Termios to the controlling terminal.
/// No-op when stdin is not a TTY.
pub fn apply_tty_termios(tmodes: &Termios) -> nix::Result<()>;
```

- Both functions internally check `isatty(0)` before touching fd 0.
- Callers gate with `env.mode.is_interactive && env.mode.options.monitor` — the helpers themselves are unconditional.

### `src/env/jobs.rs` additions

```rust
// Field addition on Job
pub struct Job {
    // ... existing fields ...
    pub saved_tmodes: Option<Termios>,
}

// Field addition on JobTable
pub struct JobTable {
    // ... existing fields ...
    pub shell_tmodes: Option<Termios>,
}

impl JobTable {
    pub fn init_shell_tmodes(&mut self, t: Termios) { self.shell_tmodes = Some(t); }
}
```

`Job::new` and `JobTable::new` initialize both fields to `None`.

### Call-site integrations

**1. REPL startup** — `src/interactive/mod.rs` (after `take_terminal(shell_pgid)` around line 48):
```rust
if env.mode.is_interactive && env.mode.options.monitor {
    if let Ok(Some(t)) = capture_tty_termios() {
        env.process.jobs.init_shell_tmodes(t);
    }
}
```

**2. Foreground job stop detection (save only)** — `src/exec/mod.rs::wait_for_foreground_job`, inside the `WaitStatus::Stopped(pid, sig)` arm (around line 821-838), before returning:
```rust
if env.mode.is_interactive && env.mode.options.monitor {
    if let Ok(Some(t)) = capture_tty_termios() {
        job.saved_tmodes = Some(t);
    }
}
```

**3. Restore shell termios after every foreground wait** — immediately after `take_terminal(shell_pgid)` at each caller. Three places:

- `src/exec/simple.rs:532` (normal foreground exec path) — after `take_terminal`:
  ```rust
  self.restore_shell_termios_if_interactive();
  ```
- `src/exec/mod.rs::builtin_fg` (around line 715) — after `take_terminal`:
  ```rust
  self.restore_shell_termios_if_interactive();
  ```
- `src/exec/pipeline.rs` (pipeline foreground wait path) — after `take_terminal`:
  ```rust
  self.restore_shell_termios_if_interactive();
  ```

Restoring on **both** `Stopped` and `Exited/Signaled` paths is intentional: a terminated child may have left the terminal in raw mode (e.g. crashed TUI). This block restores sanity regardless of how the job ended.

**4. `fg` resume (pre-give_terminal)** — `src/exec/mod.rs::builtin_fg`, just before `give_terminal(pgid)` (around line 688):
```rust
if env.mode.is_interactive && env.mode.options.monitor {
    let target = job.saved_tmodes.as_ref()
        .or(env.process.jobs.shell_tmodes.as_ref());
    if let Some(t) = target {
        let _ = apply_tty_termios(t);
    }
}
```

The post-wait path in `builtin_fg` then reuses integrations (2) and (3): stopped-again jobs re-save via (2); exit triggers shell restore via (3).

**5. `bg`** — `src/exec/mod.rs::builtin_bg` — no termios operations. `bg` does not take the terminal; the SIGCONT path does not require tcsetattr. When the user later promotes with `fg`, path (4) fires.

### Dependency change

`Cargo.toml`: add `"term"` to the `nix` crate's feature list to expose `nix::sys::termios`.

## Data Flow

### Scenario 1: `vim` → Ctrl-Z → `fg`

```
read_line() exits → raw mode OFF (cooked)
exec simple command:
  fork + setpgid(child)
  give_terminal(child_pgid)
  wait_for_foreground_job():
    vim enables its raw mode, user hits Ctrl-Z
    WaitStatus::Stopped detected
    ★ capture_tty_termios() → job.saved_tmodes = Some(vim_raw)
    print notification
    return (stopped)
  take_terminal(shell_pgid)
  ★ apply_tty_termios(shell_tmodes) → terminal back to cooked
read_line() re-entry → raw mode ON

user types `fg`:
  builtin_fg:
    ★ apply_tty_termios(job.saved_tmodes = vim_raw)
    killpg(pgid, SIGCONT)
    give_terminal(pgid)
    wait_for_foreground_job() → vim exits (Exited) — no save needed
    take_terminal(shell_pgid)
    ★ apply_tty_termios(shell_tmodes) → cooked for next prompt
```

### Scenario 2: Ctrl-Z → `bg` → `fg`

```
Stop detected (same as scenario 1)
  job.saved_tmodes = Some(vim_raw), terminal at shell_tmodes

builtin_bg:
  killpg(pgid, SIGCONT); no termios operations
  job may stop immediately again on SIGTTOU/SIGTTIN — out of scope

builtin_fg: same path as scenario 1 fg
  ★ apply_tty_termios(job.saved_tmodes = vim_raw)
  give_terminal → wait → terminal handoff back to shell
  stopped or exited as in scenario 1
```

### Scenario 3: Non-interactive / non-monitor execution

```
shell_tmodes stays None (init_shell_tmodes never called)
All ★ branches are guarded by is_interactive && monitor → skipped entirely
capture_tty_termios() would return Ok(None) even if called (isatty check)
```

### Key invariants

- `shell_tmodes` is set at most once per ShellEnv lifetime (at REPL entry).
- `job.saved_tmodes` updates only on `WaitStatus::Stopped`; never on `Exited`/`Signaled`.
- `shell_tmodes` is applied after **every** foreground wait completion (stopped or exited). Stopped-side save and shell-side restore are done in separate steps (§Call-site integrations 2 vs 3) so the restore is symmetric across outcomes.
- All termios operations are best-effort: failures do not propagate.

## Error Handling

| Failure | Handling |
|---|---|
| stdin not a TTY | `capture_tty_termios` returns `Ok(None)`; caller skips update. Silent. |
| `tcgetattr` EIO/EBADF | `Err` returned; caller does `let _ = ...`. Silent in release, `eprintln!("yosh: tcgetattr failed: {e}")` under `#[cfg(debug_assertions)]`. |
| `tcsetattr` failure | Same as tcgetattr: silent in release, debug eprintln. Job continues. |
| `shell_tmodes` never initialized | `Option` chain in `builtin_fg` falls through both `or()` arms; apply skipped. No visible effect. |
| Non-interactive / non-monitor | Caller guard prevents capture/apply. Runs through without termios interaction. |

### Guards

Two-layer defense:

1. **Caller guard** (required at every call site): `if env.mode.is_interactive && env.mode.options.monitor { ... }`.
2. **Helper-internal guard**: `capture_tty_termios` checks `isatty(0)` first and returns `Ok(None)` on non-TTY.

Both are needed because `is_interactive` is static per ShellEnv, but TTY state can change at runtime (e.g. `exec 0</dev/null` redirects stdin mid-session).

### Panic boundary

- `Termios` is `Clone` from `nix`. No `Mutex`/`RwLock`.
- Helpers return `nix::Result`; no `unwrap()`.

## Testing

### Unit tests

**`src/exec/terminal_state.rs`** (new `#[cfg(test)] mod tests`):
- `capture_tty_termios_returns_none_when_stdin_redirected` — during `cargo test`, stdin is not a TTY; assert `Ok(None)`.
- `apply_tty_termios_noop_when_non_tty` — build a dummy `Termios` via `mem::zeroed()` (nix exposes this as `std::mem::zeroed::<libc::termios>()` wrapped); assert no `Err`.

**`src/env/jobs.rs`** (existing test module):
- `job_saved_tmodes_defaults_none`.
- `job_table_shell_tmodes_defaults_none`.
- `init_shell_tmodes_stores_value`.

### Integration / PTY tests

Added to `tests/pty_interactive.rs`. All tests use `expectrl` with generous timeouts to match existing PTY test conventions (CLAUDE.md flags PTY flakiness).

1. **`shell_termios_restored_after_stopped_job`**
   - Launch yosh in PTY.
   - Run `stty raw; sleep 30 &` — no, that's background. Use `stty raw; sleep 30` in foreground.
   - Send Ctrl-Z.
   - After prompt returns, send `stty -a\n` (yosh must be in cooked mode to echo this correctly).
   - Assert `stty -a` output contains `icanon` (or platform-equivalent "cooked" indicator).

2. **`termios_preserved_across_suspend_fg`**
   - Launch yosh in PTY.
   - Run a short helper that puts terminal in raw mode and writes a sentinel on SIGCONT resume — OR simpler: run `stty -echo; cat` in foreground, Ctrl-Z, `fg`, then type characters and assert they are NOT echoed (because `-echo` must have been restored for the `cat` resume).

3. **`bg_then_fg_preserves_job_termios`**
   - Scenario 2 variant: `stty -echo; cat`, Ctrl-Z, `bg`, `fg`, assert echo still off.

**Platform caveats**: `stty -a` flag names differ between Linux and macOS. Tests parse for common substrings (`-echo`, `echo`) rather than full flag-line equality.

### Test risk acknowledgment

PTY tests in yosh are known-flaky under CI (documented in CLAUDE.md). Tests 2 and 3 rely on subtle timing around SIGTSTP/SIGCONT delivery. Use `wait_for_raw_mode`-style polling instead of `thread::sleep`. If CI flakes appear, reduce to test 1 only in CI and keep 2/3 as local-run coverage.

### Out of scope for tests

- `tcgetattr`/`tcsetattr` themselves (nix crate responsibility).
- `isatty` behavior (nix/libc responsibility).
- Crossterm raw-mode switching (unchanged existing code).

## Risks and Open Questions

1. **SIGTTOU when `bg` resumes a termios-modifying job**: a bg'd job that tries to read/write to the controlling terminal will hit SIGTTOU and stop again. This is correct POSIX behavior but may surprise users who `bg` a full-screen app. Out of scope — yosh cannot prevent this without violating POSIX.
2. **`shell_tmodes` captured post-REPL-init**: if a user changes terminal state *before* the first command (e.g. via startup rc file), the captured `shell_tmodes` reflects that. Probably what users want; document in the spec if behavior is surprising.
3. **Fallback to `shell_tmodes` on `fg`**: if a job was never stopped (somehow reaches `fg` without `saved_tmodes`), we apply `shell_tmodes`. This may be wrong for a job that started in a non-standard mode (e.g. launched via `stty raw; cmd &`). Accepted for v1; revisit if reports surface.

## Alternatives Considered

- **Inline save/restore at each call site, no new module**: rejected because the same 4-line block would appear 3 times and the `is_interactive && monitor` + `isatty` layering is easier to reason about as a single helper module.
- **TermiosStack (push/pop semantics)**: over-engineered for a 2-level model (shell + current job).
- **Refresh `shell_tmodes` before every foreground exec**: would accommodate interactive `stty` changes but complicates invariants. Deferred — the glibc manual pattern sets shell_tmodes once.
