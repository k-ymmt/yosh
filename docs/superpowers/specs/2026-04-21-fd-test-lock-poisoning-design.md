# FD_TEST_LOCK Poisoning Cascade Fix

## Problem

`src/exec/redirect.rs` defines a module-scoped `FD_TEST_LOCK: Mutex<()>` so
that unit tests which mutate process-wide file descriptors (fd 0, 1, 2) do
not race under `cargo test`'s parallel runner.

Each fd test acquires the lock with `FD_TEST_LOCK.lock().unwrap()`. Rust's
`Mutex` becomes *poisoned* when a thread panics while holding the guard, and
all subsequent `lock()` calls return `Err(PoisonError)`. `.unwrap()` then
panics the subsequent test with a `PoisonError` message — so a single test
panic cascades into failures in unrelated fd tests that were, on their own,
healthy.

This is a latent bug: it only manifests when an fd test panics under the
lock. In practice no current test triggers it, but every future test failure
inside the lock region risks producing a confusing multi-failure signature
that obscures the real root cause.

Source locations:

- `src/exec/redirect.rs:262` — `test_redirect_output_and_restore`
- `src/exec/redirect.rs:297` — `test_redirect_input`
- `src/exec/redirect.rs:329` — `test_apply_rolls_back_on_second_redirect_failure`

## Fix

Replace each `FD_TEST_LOCK.lock().unwrap()` with
`FD_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())`.

`PoisonError::into_inner()` returns the inner `MutexGuard` without the
poison check, recovering mutual exclusion for the subsequent test.

### Why this is safe

The `FD_TEST_LOCK` guards *temporal* exclusion only — it prevents two tests
from simultaneously manipulating the same process-wide fd table. It does
**not** protect any invariants on shared data (the inner type is `()`).

Each fd test is self-contained:

- Builds its own `ShellEnv` via `make_env()`.
- Saves the current fd via `apply(..., save = true)` before mutating.
- Restores the original fd via `restore()` (or via `apply()`'s internal
  rollback on the second-redirect-failure test) before returning.

If one test panics mid-body, the remaining tests still set up and tear down
their own fd state independently. There is no persistent shared state whose
corruption would make later tests invalid.

## Testing

- `cargo test --lib exec::redirect` must continue to pass (all 3 fd tests).
- No new tests are added: poison-recovery is a test-infrastructure
  improvement, not an observable production behavior.

## Scope — Out

- `tests/signals.rs` parallel-load flakes (TODO.md entry) share a related
  "parallel test interaction" theme but a different root cause (pgid /
  signal-delivery races between spawned yosh subprocesses). Not touched
  here.
- Logging or warning when poison is encountered. Poison should be silently
  recovered; if a test panics, its own failure is already reported by the
  test harness, and the downstream tests should proceed unaffected.
- Broader audit of other `Mutex::lock().unwrap()` call sites across the
  codebase. Only the three call sites named above are addressed.

## Risk

Minimal. The change is three lines in test-only code (`#[cfg(test)] mod
tests`). Production behavior is unchanged. Failure mode of the new pattern
(`unwrap_or_else(|e| e.into_inner())`) is strictly a superset of the old
pattern: every case that previously unwrapped successfully still does, and
poison cases that previously panicked now recover.
