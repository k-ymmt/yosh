//! Redirections: expanding their words ([`expand`]), applying them to
//! the fd table ([`apply`], [`open`], [`heredoc`]) and undoing them
//! again ([`RedirectState`] over the [`saved_fds`] stack).

mod apply;
mod expand;
mod heredoc;
mod open;
mod saved_fds;

use std::os::fd::RawFd;

pub use expand::{ExpandedRedirect, expand_redirects};
pub(crate) use open::{errno_text, io_error_text};
pub(crate) use saved_fds::{forget_inherited_saved_redirections, restore_all_saved_redirections};

use saved_fds::{WAS_CLOSED, push_saved_fd, saved_fds_epoch, saved_fds_len, unwind_saved_fds};

/// Tracks saved file descriptors so they can be restored after a builtin
/// or compound command runs. The saved fds live on the per-process
/// `SAVED_FDS` stack; once this state saves its first fd it owns the
/// entries above `base` while the stack is still in the `epoch` it was
/// in at that point.
///
/// The ownership is claimed lazily, on the first save: most commands
/// have no redirections at all, and a state that never saved touches
/// neither thread-local on creation, restore or drop (the eager claim
/// cost ~3% of a function-call loop). Scopes nest strictly, so the stack
/// length at the first save equals the length at creation.
pub struct RedirectState {
    /// `(base, epoch)` once this state has saved an fd.
    owned: Option<(usize, u64)>,
}

impl Default for RedirectState {
    fn default() -> Self {
        Self::new()
    }
}

impl RedirectState {
    pub fn new() -> Self {
        Self { owned: None }
    }

    /// Claim the stack suffix above its current top, before the first
    /// push (see the type docs).
    fn claim(&mut self) {
        if self.owned.is_none() {
            self.owned = Some((saved_fds_len(), saved_fds_epoch()));
        }
    }

    /// Restore this state's saved fds, unless an out-of-band reset
    /// already did (see `SAVED_FDS_EPOCH`).
    fn unwind(&self) {
        if let Some((base, epoch)) = self.owned
            && epoch == saved_fds_epoch()
        {
            unwind_saved_fds(base);
        }
    }

    /// Number of saved fds this state currently holds.
    #[cfg(test)]
    fn saved_len(&self) -> usize {
        match self.owned {
            Some((base, epoch)) if epoch == saved_fds_epoch() => {
                saved_fds_len().saturating_sub(base)
            }
            _ => 0,
        }
    }

    /// dup() the fd before it gets overwritten, storing the saved copy.
    /// Uses F_DUPFD_CLOEXEC so the saved fd is automatically closed in child processes.
    /// A closed fd (EBADF) is recorded as [`WAS_CLOSED`] so `restore`
    /// closes it again — redirecting a closed fd is not an error
    /// (`{ echo x >&5; } 5>&1` works in every shell).
    fn save_fd(&mut self, fd: RawFd) -> Result<(), String> {
        self.claim();
        let saved = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 10) };
        if saved == -1 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EBADF) {
                push_saved_fd(fd, WAS_CLOSED, false);
                return Ok(());
            }
            return Err(format!("dup: {}", err));
        }
        // dup2 on restore clears FD_CLOEXEC; remember it so an internal
        // close-on-exec fd (the signal self-pipe, the terminal fd) that a
        // user redirection temporarily overwrote does not leak into
        // children afterwards (`: 10>&1; external` with a posix_spawn
        // child would otherwise inherit the self-pipe).
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        let cloexec = flags != -1 && flags & libc::FD_CLOEXEC != 0;
        push_saved_fd(fd, saved, cloexec);
        Ok(())
    }

    /// Restore all saved fds in reverse order (LIFO).
    pub fn restore(&mut self) {
        self.unwind();
    }
}

impl Drop for RedirectState {
    fn drop(&mut self) {
        // A state dropped without `restore` (an early return) must not
        // leave its redirections in place for the commands after it, nor
        // leave stale entries on the shared stack.
        self.unwind();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;
    use crate::parser::ast::{Redirect, RedirectKind, Word};

    fn make_env() -> ShellEnv {
        ShellEnv::new("yosh", vec![])
    }

    // Tests that manipulate process-wide file descriptors (fd 0, fd 1,
    // fd 2) hold `test_sync::lock_stdio_fds` for their entire duration.

    #[test]
    fn test_redirect_output_and_restore() {
        let _guard = crate::test_sync::lock_stdio_fds();
        let mut env = make_env();
        let tmp = std::env::temp_dir().join("yosh_redirect_test_output.txt");
        let path_str = tmp.to_str().unwrap().to_string();

        let redirects = vec![Redirect {
            fd: Some(1),
            kind: RedirectKind::Output(Word::literal(&path_str)),
        }];

        let mut state = RedirectState::new();
        state
            .apply(&redirects, &mut env, true)
            .expect("apply should succeed");

        // Write to stdout (fd 1), which is now the file
        use std::io::Write;
        use std::os::unix::io::FromRawFd;
        let mut stdout = unsafe { std::fs::File::from_raw_fd(1) };
        write!(stdout, "hello redirect").unwrap();
        // Don't close fd 1 — just flush and forget the File wrapper
        std::mem::forget(stdout);

        state.restore();

        let contents = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert!(
            contents.contains("hello redirect"),
            "file should contain written text"
        );
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_redirect_input() {
        let _guard = crate::test_sync::lock_stdio_fds();
        use std::io::Read;
        use std::os::unix::io::FromRawFd;

        let mut env = make_env();
        let tmp = std::env::temp_dir().join("yosh_redirect_test_input.txt");
        std::fs::write(&tmp, "test input\n").unwrap();
        let path_str = tmp.to_str().unwrap().to_string();

        let redirects = vec![Redirect {
            fd: Some(0),
            kind: RedirectKind::Input(Word::literal(&path_str)),
        }];

        let mut state = RedirectState::new();
        state
            .apply(&redirects, &mut env, true)
            .expect("apply should succeed");

        let mut buf = String::new();
        let mut stdin = unsafe { std::fs::File::from_raw_fd(0) };
        stdin.read_to_string(&mut buf).ok();
        std::mem::forget(stdin);

        state.restore();

        assert!(buf.contains("test input"));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_apply_rolls_back_on_second_redirect_failure() {
        let _guard = crate::test_sync::lock_stdio_fds();
        // Two redirects: first targets a valid tmp file (fd 1), second targets
        // a path whose parent directory does not exist (fd 2) so open() fails.
        // Pre-fix: saved_fds is non-empty after Err and fd 1 remains dup2'd over
        // the tmp file, so a subsequent libc::write(1, ...) leaks into the tmp file.
        // Post-fix: apply() calls self.restore() internally, saved_fds is empty,
        // and fd 1 points back at the pre-apply target.

        let mut env = make_env();
        let tmp_ok = std::env::temp_dir().join("yosh_apply_rollback_ok.txt");
        // Remove any stale file from a prior test run.
        let _ = std::fs::remove_file(&tmp_ok);
        let bad_path = "/no/such/dir/should-not-exist-yosh-test/file.txt";

        let redirects = vec![
            Redirect {
                fd: Some(1),
                kind: RedirectKind::Output(Word::literal(tmp_ok.to_str().unwrap())),
            },
            Redirect {
                fd: Some(2),
                kind: RedirectKind::Output(Word::literal(bad_path)),
            },
        ];

        // Save original fd 1 outside RedirectState so we can restore it at the end
        // (cargo test captures stdout; we must not leave fd 1 corrupted for sibling tests).
        let orig_stdout = unsafe { libc::dup(1) };
        assert!(orig_stdout >= 0, "dup(1) failed");

        let mut state = RedirectState::new();
        let result = state.apply(&redirects, &mut env, true);
        assert!(result.is_err(), "expected apply to fail on the bad path");

        // Post-condition 1: rollback emptied saved_fds.
        assert!(
            state.saved_len() == 0,
            "saved_fds should be empty after rollback, got {} entries",
            state.saved_len()
        );

        // Post-condition 2: writes to fd 1 should not land in tmp_ok.
        let marker = b"post-rollback-marker\n";
        unsafe {
            libc::write(1, marker.as_ptr() as *const _, marker.len());
        }

        let written = std::fs::read_to_string(&tmp_ok).unwrap_or_default();

        // Cleanup BEFORE assertion so a failure still cleans up.
        unsafe {
            libc::dup2(orig_stdout, 1);
            libc::close(orig_stdout);
        }
        let _ = std::fs::remove_file(&tmp_ok);

        assert!(
            !written.contains("post-rollback-marker"),
            "fd 1 should not still point at tmp_ok after rollback; tmp_ok contained: {written:?}"
        );
    }

    /// Redirecting an fd that is closed records it as closed and closes it
    /// again on restore — even when `open` hands back the target number
    /// itself (the file must not leak past the command).
    #[test]
    fn closed_target_fd_is_closed_again_on_restore() {
        let _guard = crate::test_sync::lock_stdio_fds();
        let mut env = make_env();
        // The highest fd the process may open: parallel tests allocate
        // fds lowest-first, so none of them owns this number (a fixed
        // mid-range number such as 77 could be a parallel test's pipe,
        // which closing here would break).
        let mut lim: libc::rlimit = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) }, 0);
        let target: RawFd = (lim.rlim_cur.min(1024) - 1) as RawFd;
        assert_eq!(
            unsafe { libc::fcntl(target, libc::F_GETFD) },
            -1,
            "fd {target} unexpectedly open"
        );
        let tmp = std::env::temp_dir().join("yosh_redirect_closed_target.txt");
        let redirects = vec![Redirect {
            fd: Some(target),
            kind: RedirectKind::Output(Word::literal(tmp.to_str().unwrap())),
        }];
        let mut state = RedirectState::new();
        state
            .apply(&redirects, &mut env, true)
            .expect("redirecting a closed fd must succeed");
        assert_ne!(unsafe { libc::fcntl(target, libc::F_GETFD) }, -1);
        state.restore();
        assert_eq!(
            unsafe { libc::fcntl(target, libc::F_GETFD) },
            -1,
            "fd must be closed again after restore"
        );
        let _ = std::fs::remove_file(&tmp);
    }

    /// A state with no redirections claims nothing on the saved-fd
    /// stack (it is created for every command), and dropping it inside
    /// an enclosing state that did save must not undo the outer
    /// redirection.
    #[test]
    fn empty_state_does_not_touch_the_saved_fd_stack() {
        let _guard = crate::test_sync::lock_stdio_fds();
        let mut env = make_env();
        // A high fd no parallel test owns (see the test above).
        let mut lim: libc::rlimit = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) }, 0);
        let target: RawFd = (lim.rlim_cur.min(1024) - 2) as RawFd;
        let tmp = std::env::temp_dir().join("yosh_redirect_empty_state.txt");
        let redirects = vec![Redirect {
            fd: Some(target),
            kind: RedirectKind::Output(Word::literal(tmp.to_str().unwrap())),
        }];
        let was_open = unsafe { libc::fcntl(target, libc::F_GETFD) } != -1;
        let mut outer = RedirectState::new();
        outer.apply(&redirects, &mut env, true).expect("apply");
        let depth = saved_fds_len();
        {
            let mut inner = RedirectState::new();
            inner.apply_expanded(&[], true).expect("empty apply");
            assert!(inner.owned.is_none());
            assert_eq!(inner.saved_len(), 0);
            inner.restore();
        }
        assert_eq!(saved_fds_len(), depth, "inner drop popped outer entries");
        assert_eq!(outer.saved_len(), 1);
        outer.restore();
        assert_eq!(
            unsafe { libc::fcntl(target, libc::F_GETFD) } != -1,
            was_open,
            "outer restore must put fd {target} back"
        );
        let _ = std::fs::remove_file(&tmp);
    }
}
