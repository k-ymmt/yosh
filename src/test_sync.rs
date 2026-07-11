//! Cross-module synchronization for unit tests.
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
//! history in `builtin::regular`'s `resolve_cdpath_empty_entry_is_dot`).

use std::sync::{Mutex, MutexGuard};

static CWD_LOCK: Mutex<()> = Mutex::new(());

/// Serialize a test that mutates or depends on the process cwd.
/// Poison-tolerant: a panicking test must not cascade into every other
/// cwd test.
pub(crate) fn lock_cwd() -> MutexGuard<'static, ()> {
    CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
