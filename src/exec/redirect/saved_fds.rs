//! The per-process stack of fds saved by the active [`RedirectState`]s,
//! and the out-of-band resets of it (shell exit, fork).
//!
//! [`RedirectState`]: super::RedirectState

use std::os::fd::RawFd;

use super::open::raw_dup2;

/// Sentinel stored in `saved_fds` when the target fd was closed before
/// the redirection: restoring it means closing the fd again.
pub(super) const WAS_CLOSED: RawFd = -1;

/// One fd a [`RedirectState`] overwrote: `(original_fd, saved_copy_fd,
/// original_was_cloexec)`. `saved_copy_fd` is [`WAS_CLOSED`] when
/// `original_fd` was not open.
///
/// [`RedirectState`]: super::RedirectState
type SavedFd = (RawFd, RawFd, bool);

thread_local! {
    /// The saved fds of every [`RedirectState`] currently applied in this
    /// process, innermost last. Redirection scopes nest strictly (a
    /// compound command's redirections are restored after its body's),
    /// so each state owns the suffix of this stack that it pushed.
    ///
    /// Keeping them in one stack lets a shell that is about to terminate
    /// undo every active redirection before it runs the EXIT trap
    /// ([`restore_all_saved_redirections`]): `trap 'echo out' EXIT; {
    /// exit; } >f` must print `out` on the shell's own stdout, not into
    /// `f` (dash and bash unwind the redirection first).
    ///
    /// [`RedirectState`]: super::RedirectState
    static SAVED_FDS: std::cell::RefCell<Vec<SavedFd>> =
        const { std::cell::RefCell::new(Vec::new()) };

    /// Bumped whenever the whole stack is emptied out of band
    /// ([`restore_all_saved_redirections`], a fork): a [`RedirectState`]
    /// from an older epoch no longer owns any entry, and must not pop
    /// those of states created after the reset.
    ///
    /// [`RedirectState`]: super::RedirectState
    static SAVED_FDS_EPOCH: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

pub(super) fn saved_fds_epoch() -> u64 {
    SAVED_FDS_EPOCH.with(|e| e.get())
}

fn bump_saved_fds_epoch() {
    SAVED_FDS_EPOCH.with(|e| e.set(e.get().wrapping_add(1)));
}

pub(super) fn saved_fds_len() -> usize {
    SAVED_FDS.with(|s| s.borrow().len())
}

/// Push one saved fd onto the stack (see [`SavedFd`]).
pub(super) fn push_saved_fd(original: RawFd, saved: RawFd, cloexec: bool) {
    SAVED_FDS.with(|s| s.borrow_mut().push((original, saved, cloexec)));
}

/// Pop the saved fds above `base` (innermost first), restoring each one
/// to its original fd.
pub(super) fn unwind_saved_fds(base: usize) {
    while let Some((original, saved, cloexec)) = SAVED_FDS.with(|s| {
        let mut s = s.borrow_mut();
        if s.len() > base { s.pop() } else { None }
    }) {
        if saved == WAS_CLOSED {
            unsafe { libc::close(original) };
        } else {
            if raw_dup2(saved, original).is_ok() && cloexec {
                unsafe { libc::fcntl(original, libc::F_SETFD, libc::FD_CLOEXEC) };
            }
            unsafe { libc::close(saved) };
        }
    }
}

/// Undo every redirection currently applied with `save` in this
/// process, innermost first. Called when the shell (or a subshell) is
/// terminating, right before its EXIT trap runs, so the action writes to
/// the shell's own fds — the redirection scopes it is exiting from are
/// never re-entered. A [`RedirectState`] whose fds were undone this way
/// finds nothing left to restore.
///
/// [`RedirectState`]: super::RedirectState
pub(crate) fn restore_all_saved_redirections() {
    unwind_saved_fds(0);
    bump_saved_fds_epoch();
}

/// Forget (without restoring) the saved fds inherited from the parent
/// shell: called first thing in every forked child. The redirections the
/// parent applied before forking are part of the child's environment
/// (`( trap 'echo t' EXIT; exit ) >f` writes `t` into `f`), so the
/// child's exit must not undo them. The saved copies are close-on-exec
/// and stay open until the child exits.
pub(crate) fn forget_inherited_saved_redirections() {
    SAVED_FDS.with(|s| s.borrow_mut().clear());
    bump_saved_fds_epoch();
}
