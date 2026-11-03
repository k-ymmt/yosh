//! Cross-module synchronization for unit tests.
//!
//! Two process-global resources are shared by the parallel libtest
//! threads: the cwd ([`lock_cwd`]) and the standard fds 0/1/2
//! ([`lock_stdio_fds`]). When a test needs both, take `lock_cwd` first.
//!
//! The process cwd is process-global while `cargo test` runs tests on
//! parallel threads. Any test that mutates the cwd (`set_current_dir`)
//! or resolves relative paths against it must hold [`CWD_LOCK`] for its
//! whole body, or a concurrent test observes the wrong directory.
//! Mutating tests must additionally chdir only to directories that
//! outlive the test process (e.g. `std::env::temp_dir()`): a subprocess
//! spawned by a parallel test inherits the cwd at spawn time without
//! taking this lock, and inheriting a soon-deleted tempdir leaves it
//! with a dangling cwd (`/bin/sh` stalls trying to recover — see the
//! history in `builtin::cd`'s `resolve_cdpath_empty_entry_is_dot`).

use std::sync::{Mutex, MutexGuard};

static CWD_LOCK: Mutex<()> = Mutex::new(());

/// Serialize a test that mutates or depends on the process cwd.
/// Poison-tolerant: a panicking test must not cascade into every other
/// cwd test.
pub(crate) fn lock_cwd() -> MutexGuard<'static, ()> {
    CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

static STDIO_FDS_LOCK: Mutex<()> = Mutex::new(());

/// Serialize a test that redirects, closes, or inspects the process's
/// own standard file descriptors 0, 1 and 2 (`RedirectState::apply`
/// with `save = true`, `exec_program` of a redirect-only or
/// redirected special-builtin command, `isatty(0)` / `tcgetattr(0)`).
///
/// Every such test must take THIS lock — one lock for all of them. A
/// redirect swaps an fd through a `dup` / `dup2` / `close` sequence, so
/// a second test touching the same fd mid-swap sees it closed
/// (`dup: Bad file descriptor`, `isatty: EBADF`) or pointing at the
/// other test's file. Separate per-module locks (the former
/// `STDIO_LOCK` and `FD_TEST_LOCK`) did not exclude each other and
/// failed about one `cargo test --lib` run in three on a loaded machine.
///
/// Lock order: take this lock BEFORE anything that locks the std
/// `stdout` / `stderr` handles ([`crate::exec::process::fork_shell`]
/// takes both while forking, and a test holding this lock may well run
/// a command that forks). Never acquire it while holding a std stdio
/// lock. libtest's own threads take the std handles but never this
/// lock, so the order cannot cycle. A forked child inherits this mutex
/// in whatever state it had, but children never run test code that
/// takes it.
///
/// Poison-tolerant: a panicking test restored (or failed to restore)
/// the fds itself; poisoning must not cascade into every fd test.
pub(crate) fn lock_stdio_fds() -> MutexGuard<'static, ()> {
    STDIO_FDS_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
