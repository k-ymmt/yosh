# SIGHUP History Save Design

## Problem

When the shell receives SIGHUP (or other termination signals), `handle_default_signal()` calls `std::process::exit(128 + sig)` directly, bypassing the history save code in `Repl::run()` (lines 192-198). This causes history loss.

The same issue exists in `builtin_exit()` — it calls `std::process::exit()` directly, also bypassing history save.

## Approach: Flag-Based Exit

Instead of calling `std::process::exit()` in interactive mode, set a flag on `Executor` and let the `Repl::run()` loop break through the normal cleanup path. This centralizes all cleanup logic in one place, making it easy to add future cleanup steps.

Non-interactive mode retains `std::process::exit()` since there is no history to save.

## Changes

### 1. `Executor` struct (`src/exec/mod.rs`)

Add `exit_requested: Option<i32>` field. `None` means no exit requested; `Some(code)` means exit with the given status code.

Initialize to `None` in both `new()` and `from_env()`.

### 2. `handle_default_signal()` (`src/exec/mod.rs`)

```rust
fn handle_default_signal(&mut self, sig: i32) {
    self.execute_exit_trap();
    if self.env.mode.is_interactive {
        self.exit_requested = Some(128 + sig);
    } else {
        std::process::exit(128 + sig);
    }
}
```

### 3. `builtin_exit()` (`src/builtin/special.rs`)

```rust
fn builtin_exit(args: &[String], executor: &mut Executor) -> i32 {
    let code = /* existing parse logic */;
    executor.process_pending_signals();
    executor.execute_exit_trap();
    if executor.env.mode.is_interactive {
        executor.exit_requested = Some(code);
        code
    } else {
        std::process::exit(code);
    }
}
```

### 4. `Repl::run()` (`src/interactive/mod.rs`)

Add `exit_requested` checks at two points:

**After command execution (inside the `Complete` branch):**

```rust
for cmd in &commands {
    let status = self.executor.exec_complete_command(cmd);
    self.executor.env.exec.last_exit_status = status;
    if self.executor.exit_requested.is_some() {
        break;
    }
}
```

**After `process_pending_signals()` in the loop (line 186):**

```rust
self.executor.process_pending_signals();
if let Some(code) = self.executor.exit_requested {
    self.executor.env.exec.last_exit_status = code;
    break;
}
```

The existing cleanup code after the loop (lines 189-198) runs unchanged, saving history through the normal path.

**After `process_pending_signals()` post-loop (line 189):** Skip the redundant `execute_exit_trap()` call since `handle_default_signal()` already called it:

```rust
self.executor.process_pending_signals();
if self.executor.exit_requested.is_none() {
    self.executor.execute_exit_trap();
}
```

### 5. Exit code propagation

When `exit_requested` is set, `Repl::run()` uses that code as the final exit status. The existing `last_exit_status` return at line 200 naturally picks this up.

## Testing

### Unit test (`src/exec/mod.rs`)

- Verify `handle_default_signal()` sets `exit_requested` when `is_interactive = true`
- Verify `handle_default_signal()` does not set `exit_requested` when `is_interactive = false` (uses `std::process::exit`, tested indirectly)

### E2E test (`e2e/`)

- Start kish in interactive mode, send SIGHUP, verify history file is written

## Files Changed

| File | Change |
|------|--------|
| `src/exec/mod.rs` | Add `exit_requested` field, modify `handle_default_signal()` |
| `src/interactive/mod.rs` | Add `exit_requested` checks in loop and post-loop |
| `src/builtin/special.rs` | Modify `builtin_exit()` to use flag in interactive mode |
