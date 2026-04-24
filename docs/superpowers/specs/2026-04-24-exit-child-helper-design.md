# Design: `exit_child` Helper for Post-Fork Safe Exit

**Date:** 2026-04-24
**Status:** Design approved, awaiting implementation
**Target TODO entries:** Known Bug (line 5), Code Quality Improvements (line 89)

## Problem

`cargo test --workspace` can deadlock on `exec_compound_subshell_sets_lineno_on_entry` — a rare race in `src/exec/mod.rs:1235` whose root cause is that every post-fork child in yosh calls `std::process::exit(status)`. `std::process::exit` runs the full Rust runtime cleanup path:

```
std::process::exit
 → std::rt::cleanup
   → stack_overflow::cleanup
     → drop_handler → delete_current_info
       → LOCK.lock()   // static Mutex<()> in std::sys::pal::unix::stack_overflow::thread_info
```

In a multithreaded parent (e.g. the test harness), another worker thread may hold that `LOCK` at the moment `fork()` is called. The child inherits the locked state, but the lock-holder thread does not exist in the child. The child blocks forever in `__psynch_mutexwait`, and the parent's `wait4()` also hangs. Observed 2026-04-24 via a 6-hour test hang; the grandchild was sampled and confirmed at `__psynch_mutexwait`.

The same class of hazard exists for any std-internal mutex (including the `ENV_LOCK` already documented inline at `src/exec/simple.rs:477-483`), so a narrow fix to one specific `LOCK` is insufficient.

Production interactive shells do not manifest this bug because the shell parent is single-threaded at fork time. The test harness is the exception (rayon/test workers hold locks concurrently with the forking thread).

## Approach

POSIX guarantees async-signal-safety only for a small set of functions between `fork()` and `exec()` (or `_exit`). `libc::_exit(2)` is in that set and bypasses Rust runtime cleanup entirely. Every post-fork exit in yosh must therefore route through `libc::_exit`, never `std::process::exit`.

A naive `libc::_exit` loses buffered stdio output (e.g. `( echo -n hi ) | cat` regresses to empty output because `stdout()`'s `LineWriter` is not auto-flushed on `_exit`). So we wrap `_exit` in a helper that first flushes stdout and stderr:

```rust
pub(crate) fn exit_child(status: i32) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    unsafe { libc::_exit(status) }
}
```

Flush errors are silently ignored: the child has nothing to recover to, and any path that panics here returns to the exact Rust unwinding / drop hooks we are trying to avoid.

Shell-parent `std::process::exit` calls (errexit, signal handler default exit, `exit` builtin) are **not** changed — those paths correctly want the full cleanup.

## Architecture

**Helper location:** `src/exec/mod.rs`, `pub(crate) fn exit_child(status: i32) -> !`.

Rationale: keeping the helper alongside the `Executor` in `src/exec/mod.rs` matches the existing module structure and avoids creating a new module for a single function. TODO.md:90 flags a larger architectural concern (fork + run-Rust-code-in-child is POSIX-UB in MT contexts) which may later warrant a dedicated `src/exec/child.rs` module, but that redesign is out of scope here.

**Call-site access:**
- `src/exec/compound.rs` → `super::exit_child`
- `src/exec/simple.rs` → `super::exit_child`
- `src/exec/pipeline.rs` → `super::exit_child`
- `src/exec/mod.rs` internal bg-job path → `exit_child`
- `src/expand/command_sub.rs` → `crate::exec::exit_child`

## Changes

### Add: `exit_child` helper

`src/exec/mod.rs` (near the top of the module, after imports):

```rust
/// Exit a post-fork child process safely.
///
/// Uses `libc::_exit` to skip Rust runtime cleanup, which can deadlock
/// on std-internal mutexes inherited locked from a multithreaded parent
/// (e.g. `std::sys::pal::unix::stack_overflow::thread_info::LOCK`).
/// Flushes stdout/stderr first so buffered output is not lost.
///
/// Use ONLY after `fork()` in the child branch, never in the shell parent.
pub(crate) fn exit_child(status: i32) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    unsafe { libc::_exit(status) }
}
```

### Replace: post-fork exit sites

| # | File:line | Before | After | Context |
|---|---|---|---|---|
| 1 | `src/exec/compound.rs:99` | `std::process::exit(status);` | `super::exit_child(status);` | subshell child normal exit |
| 2 | `src/expand/command_sub.rs:72` | `std::process::exit(status);` | `crate::exec::exit_child(status);` | command substitution child |
| 3 | `src/exec/simple.rs:474` | `std::process::exit(1);` | `super::exit_child(1);` | external command redirect failure |
| 4 | `src/exec/simple.rs:508` | `std::process::exit(exit_code);` | `super::exit_child(exit_code);` | execvp failure |
| 5 | `src/exec/pipeline.rs:91` | `unsafe { libc::_exit(1) };` | `super::exit_child(1);` | pipeline dup2 (stdin) failure |
| 6 | `src/exec/pipeline.rs:99` | `unsafe { libc::_exit(1) };` | `super::exit_child(1);` | pipeline dup2 (stdout) failure |
| 7 | `src/exec/pipeline.rs:106` | `std::process::exit(status);` | `super::exit_child(status);` | pipeline member normal exit |
| 8 | `src/exec/mod.rs:361` | `std::process::exit(status);` | `exit_child(status);` | background job child |

### Do not change (shell parent, intentionally runs Rust cleanup)

- `src/exec/mod.rs:155` — errexit termination of non-interactive shell
- `src/exec/mod.rs:211` — shell parent signal handler
- `src/builtin/special.rs:82` — `exit` builtin (runs in the shell)
- `src/main.rs:258` — only a comment, no exit call
- `src/bin/yosh-plugin.rs:2`, `src/bin/yosh-dhat.rs:60` — `main()`, not post-fork

## Testing

### New regression test

Add to `tests/subshell.rs`:

```rust
#[test]
fn test_subshell_pipeline_preserves_unflushed_output() {
    // Regression test for the exit_child helper.
    //
    // Naive `libc::_exit(0)` without stdout flush would regress
    // `( echo -n hi ) | cat` to empty output, because `echo -n` does
    // not append a newline and stdout's LineWriter would not auto-flush
    // before _exit. Confirmed empirically 2026-04-24.
    //
    // This test exercises BOTH the subshell exit path (compound.rs)
    // AND the pipeline member exit path (pipeline.rs) via the pipe.
    let out = yosh_exec("( echo -n hi ) | cat");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hi");
}
```

Uses the existing `yosh_exec` helper defined at the top of `tests/subshell.rs`, which spawns `CARGO_BIN_EXE_yosh -c <input>` and returns `std::process::Output`.

### Verification commands

1. `cargo build` — compile check
2. `cargo test --test subshell` — new + existing subshell tests (fast, <30s)
3. `cargo test --workspace` — full workspace (~6-7 min, run in background per CLAUDE.md timeout guidance)
4. `./e2e/run_tests.sh` — POSIX E2E suite (background)

If the full workspace run passes multiple times without the deadlock recurring, the fix is confirmed (the original failure required a 6-hour hang to observe, so a few back-to-back green runs is strong evidence).

## Risks

1. **Loss of Rust `Drop` impls in the child** — temp files are not cleaned, FDs are not closed via `Drop`, etc.
   - **Mitigation:** yosh post-fork children (subshell, command sub, pipeline member, external cmd child, bg job) do not hold `mkstemp`-style resources. OS closes FDs on process exit. Verified by grepping the relevant paths — no `Drop` with user-visible side effects is present.

2. **The original deadlock was race-conditioned** — a few green test runs do not *prove* the fix.
   - **Mitigation:** `libc::_exit` is the canonical POSIX-sanctioned answer to this class of bug; by construction it cannot touch the std mutex that caused the hang. Empirical confirmation is a sanity check, not the proof.

3. **TODO.md:90 remains open** — `exec_body` in the child still invokes arbitrary Rust std, which is technically POSIX-UB in MT contexts.
   - **Out of scope:** This helper addresses the concrete deadlock. The architectural rework (subshell via `fork+exec` instead of `fork+in-process interpreter`) stays on TODO.md.

## TODO.md updates (post-implementation)

Per CLAUDE.md convention (delete completed items, do not use `[x]`):

- Delete lines 3-5 (the Known Bug entry for the deadlock)
- Delete line 89 (the `exit_child` helper systematic fix entry)
- **Keep** line 90 (fork+run-Rust-code-in-child architectural concern) — unresolved

## References

- `src/exec/simple.rs:477-483` — inline comment already documenting the same class of deadlock for `std::env::set_var` / `ENV_LOCK`. Validates the approach.
- POSIX.1-2017 `fork(2)` — "If a multi-threaded process calls fork(), the new process shall contain a replica of the calling thread and its entire address space… To avoid errors, the child process may only execute async-signal-safe operations until such time as one of the exec functions is called."
- POSIX.1-2017 `_exit(2)` — async-signal-safe.
