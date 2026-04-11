# Phase 7 Known Limitations — Design Spec

## Overview

Fix two known limitations from Phase 7:

1. `wait` signal interruption uses only the first signal for return status
2. `kill 0` in pipeline subshell sends to pipeline's process group instead of shell's

## Fix 1: `wait` Signal Interruption

### Problem

In `src/exec/mod.rs` line 897, `builtin_wait()` uses `signals[0]` when multiple signals arrive simultaneously during a `wait` call. All trap handlers execute correctly, but the return status reflects only the first signal in the drain buffer.

### Solution

Change `signals[0]` to `*signals.last().unwrap()` to use the last-received signal for the return status. This matches bash behavior (last-writer-wins). POSIX leaves this unspecified, so bash compatibility is the pragmatic choice.

### Changes

- `src/exec/mod.rs`: In `builtin_wait()`, change `128 + signals[0]` to `128 + *signals.last().unwrap()`

### Testing

- Verify existing `wait` tests still pass
- The simultaneous multi-signal scenario is inherently timing-dependent and difficult to test reliably; no new test for this specific edge case

## Fix 2: `kill 0` in Pipeline Subshell

### Problem

`builtin_kill()` in `src/builtin/mod.rs` passes PID 0 directly to `nix::sys::signal::kill()`. The kernel interprets PID 0 as "caller's process group." In a pipeline subshell, the caller's process group is the pipeline's group (set via `setpgid()`), not the shell's original group. POSIX expects `kill 0` to target the shell's process group.

### Solution

Add a `shell_pgid` parameter to `builtin_kill()`. When PID is 0, substitute `-shell_pgid.as_raw()` (negative PID = send to entire process group) so the signal targets the shell's process group regardless of the caller's current group membership.

### Changes

1. `src/builtin/mod.rs`:
   - Change signature: `fn builtin_kill(args: &[String], shell_pgid: Pid) -> i32`
   - In the PID loop: when `pid == 0`, use `Pid::from_raw(-shell_pgid.as_raw())` instead of `Pid::from_raw(0)`
   - Update `exec_regular_builtin()` to pass `env.shell_pgid` to `builtin_kill()`

2. `src/exec/mod.rs`: No changes needed (kill is dispatched via `exec_regular_builtin`)

### Testing

- Add test in `tests/signals.rs` to verify `kill 0` sends to the shell's process group
