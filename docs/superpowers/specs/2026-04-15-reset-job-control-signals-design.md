# Reset Job Control Signals on `set +m` / `set -m`

## Date

2026-04-15

## Problem

When `set +m` disables monitor mode at runtime, `reset_job_control_signals()` is not called. The signal dispositions (SIGTSTP, SIGTTIN, SIGTTOU ignored; SIGCHLD handled via self-pipe) remain active even though the shell is no longer performing job control. Conversely, `set -m` re-enables the monitor flag but does not call `init_job_control_signals()`.

This means:

- **`set +m`**: SIGTSTP/SIGTTIN/SIGTTOU stay SIG_IGN and SIGCHLD stays handled — the shell continues to behave as if job control is active at the signal level.
- **`set -m`**: If monitor mode was previously disabled, the job control signals are not re-established.

## POSIX Reference

POSIX 2.11 (Job Control):
> "If job control is enabled, ... the shell shall ignore the SIGTSTP, SIGTTIN, and SIGTTOU signals."

Disabling monitor mode should restore default signal dispositions for these signals.

## Design

### Approach: Executor-level before/after check (Approach C)

The `exec_special_builtin` function in `src/builtin/special.rs` already receives `&mut Executor` and dispatches `"set"` to `builtin_set`. We add a before/after comparison of the `monitor` flag around the `builtin_set` call, invoking the appropriate signal function when the value changes.

### Changes

#### `src/builtin/special.rs` — `exec_special_builtin`

Replace the `"set"` arm:

```rust
"set" => {
    let was_monitor = executor.env.mode.options.monitor;
    let ret = builtin_set(args, &mut executor.env);
    let is_monitor = executor.env.mode.options.monitor;
    if was_monitor && !is_monitor {
        crate::signal::reset_job_control_signals();
    } else if !was_monitor && is_monitor {
        crate::signal::init_job_control_signals();
    }
    ret
}
```

#### `src/signal.rs` — Remove dead_code annotation

Remove `#[allow(dead_code)]` and the TODO comment from `reset_job_control_signals()`.

### Testing

1. **Unit test in `src/signal.rs`**: Verify `reset_job_control_signals()` can be called without panicking after `init_job_control_signals()`.
2. **Integration test**: Run `set +m` in a script and verify job control commands (`bg`, `fg`) correctly report "no job control".
3. **Integration test**: Verify `set -m` re-enables monitor mode (signal-level behavior verified indirectly through job control commands working again in an interactive context).
4. **E2E test**: Script-level test for `set +m` / `set -m` toggle behavior.

### Scope

- `src/builtin/special.rs`: ~8 lines changed in `exec_special_builtin`
- `src/signal.rs`: ~2 lines changed (remove annotation)
- New E2E tests: 1-2 test scripts
- New integration/unit tests: 1-2 tests

### Out of Scope

- Terminal state save/restore (separate TODO item)
- `suspend` / `disown` builtins
- Any changes to `ShellOptions` or `set_by_char` / `set_by_name` signatures
