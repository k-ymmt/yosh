//! Self-pipe signal delivery: the async-signal-safe handler, the pipe
//! it writes to, the pending-signal flags, and draining.

use std::os::unix::io::RawFd;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};

use super::default_signal;
use super::entry::capture_ignored_on_entry_once;
use super::table::HANDLED_SIGNALS;

/// Set to `true` by the signal handler when SIGHUP or SIGTERM is received.
/// Checked by the terminal read loop to interrupt blocking reads gracefully.
static PENDING_EXIT_SIGNAL: AtomicBool = AtomicBool::new(false);

/// `true` once the shell has entered interactive mode. Read by the
/// async-signal handler (atomic load is async-signal-safe) to decide
/// whether SIGTERM should interrupt the terminal read loop: POSIX sh
/// requires interactive shells to ignore untrapped SIGTERM, so setting
/// [`PENDING_EXIT_SIGNAL`] for it would wrongly abort the read loop and
/// exit the shell. SIGHUP (terminal hangup) still interrupts either way.
static INTERACTIVE_SHELL: AtomicBool = AtomicBool::new(false);

/// Mark the shell as interactive (called once from `Repl::new`).
/// Forked children reset their signal dispositions via
/// [`reset_child_signals`](super::reset_child_signals) before this flag
/// could matter to them.
pub fn set_interactive_shell(on: bool) {
    INTERACTIVE_SHELL.store(on, Ordering::Release);
}

/// Returns `true` if a SIGHUP or SIGTERM has been received since the last
/// call to [`drain_pending_signals`].
///
/// This is safe to call from any thread or async context.
pub fn has_pending_exit_signal() -> bool {
    PENDING_EXIT_SIGNAL.load(Ordering::Acquire)
}

// ---------------------------------------------------------------------------
// Self-pipe and signal handlers (Task 2)
// ---------------------------------------------------------------------------

/// Global self-pipe file descriptor pair (read_fd, write_fd).
static SELF_PIPE: OnceLock<(RawFd, RawFd)> = OnceLock::new();

/// Fast-path gate for [`drain_pending_signals`]: set by the signal
/// handler after it writes the self-pipe byte, cleared (swapped) by the
/// drain. When clear, the drain performs zero syscalls. The
/// write-byte-then-set-flag / swap-flag-then-read-pipe ordering
/// guarantees a byte can never be stranded with the flag clear: at
/// worst a signal arriving exactly during a drain is picked up by the
/// next drain point.
static SIGNAL_PENDING: AtomicBool = AtomicBool::new(false);

/// Async-signal-safe handler: writes the signal number as a single byte to the
/// write end of the self-pipe, and sets the PENDING_EXIT_SIGNAL flag for
/// SIGHUP and SIGTERM so that the terminal read loop can notice quickly.
pub(super) extern "C" fn signal_handler(sig: libc::c_int) {
    // AtomicBool::store/load are async-signal-safe. SIGTERM only counts
    // as an exit signal for non-interactive shells — interactive shells
    // ignore it (POSIX sh) and must keep the read loop running.
    if sig == libc::SIGHUP || (sig == libc::SIGTERM && !INTERACTIVE_SHELL.load(Ordering::Acquire)) {
        PENDING_EXIT_SIGNAL.store(true, Ordering::Release);
    }
    let Some(&(_, write_fd)) = SELF_PIPE.get() else {
        return;
    };
    let byte = sig as u8;
    // write(2) is async-signal-safe; we intentionally ignore errors (pipe full
    // just means the signal is already pending).
    unsafe {
        libc::write(write_fd, &byte as *const u8 as *const libc::c_void, 1);
    }
    // AtomicBool::store is async-signal-safe. Set AFTER the byte is in
    // the pipe so a concurrent drain that observes the flag always finds
    // the byte (see SIGNAL_PENDING).
    SIGNAL_PENDING.store(true, Ordering::Release);
}

/// A `SigAction` routing a signal through [`signal_handler`], with
/// `SA_RESTART` when `restart` is set.
pub(super) fn self_pipe_action(restart: bool) -> SigAction {
    SigAction::new(
        SigHandler::Handler(signal_handler),
        if restart {
            SaFlags::SA_RESTART
        } else {
            SaFlags::empty()
        },
        SigSet::empty(),
    )
}

/// Create the self-pipe (O_NONBLOCK | O_CLOEXEC) and register sigaction
/// handlers for every signal in [`HANDLED_SIGNALS`].
///
/// This function is idempotent — calling it more than once is a no-op.
pub fn init_signal_handling() {
    SELF_PIPE.get_or_init(|| {
        // POSIX §2.12: capture the set of signals inherited as SIG_IGN before we
        // install any yosh handler. Skip registration for those signals so they
        // remain ignored for the shell's lifetime.
        let entry_ignored = capture_ignored_on_entry_once();

        // Undo the Rust runtime's pre-main SIG_IGN on SIGPIPE (see
        // capture_ignored_on_entry): the shell itself dies on SIGPIPE like
        // sh/bash (`yosh -c 'echo hi' | head -0` exits 141), and children
        // inherit SIG_DFL so pipeline writers get killed by EPIPE instead
        // of erroring. A user `trap '' PIPE` re-ignores it via the trap
        // builtin's disposition install.
        default_signal(libc::SIGPIPE);

        let mut fds: [libc::c_int; 2] = [0; 2];

        // Create the pipe.
        let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
        assert_eq!(ret, 0, "pipe() failed");

        // Move pipe fds to high numbers (>= 10) so they don't collide with
        // user-visible fds (0–9).  F_DUPFD_CLOEXEC atomically dups to >= 10
        // and sets CLOEXEC.
        let read_fd = unsafe { libc::fcntl(fds[0], libc::F_DUPFD_CLOEXEC, 10) };
        assert!(read_fd >= 10, "F_DUPFD_CLOEXEC failed for read end");
        unsafe { libc::close(fds[0]) };

        let write_fd = unsafe { libc::fcntl(fds[1], libc::F_DUPFD_CLOEXEC, 10) };
        assert!(write_fd >= 10, "F_DUPFD_CLOEXEC failed for write end");
        unsafe { libc::close(fds[1]) };

        // Set O_NONBLOCK on both ends (CLOEXEC already set by F_DUPFD_CLOEXEC).
        for &fd in &[read_fd, write_fd] {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            unsafe {
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }

        // Register sigaction handlers for all HANDLED_SIGNALS.
        // Use SA_RESTART for most signals so that slow system calls are
        // automatically restarted.  SIGHUP and SIGTERM are termination
        // signals; we deliberately omit SA_RESTART so that a blocking
        // read() (e.g. inside read_event()) returns EINTR, which causes
        // the shell to break out of its read loop and call
        // process_pending_signals() where the exit is handled.
        let sa_restart = self_pipe_action(true);
        let sa_no_restart = self_pipe_action(false);

        for &(num, _) in HANDLED_SIGNALS {
            // POSIX §2.12: leave inherited SIG_IGN in place.
            if entry_ignored.contains(&num) {
                continue;
            }

            let sig = Signal::try_from(num).expect("invalid signal number in HANDLED_SIGNALS");
            let sa = if num == libc::SIGHUP || num == libc::SIGTERM {
                &sa_no_restart
            } else {
                &sa_restart
            };
            unsafe {
                sigaction(sig, sa).expect("sigaction failed");
            }
        }

        (read_fd, write_fd)
    });
}

/// Non-blocking read of all pending signal bytes from the self-pipe.
///
/// Returns a (possibly empty) vector of signal numbers in first-delivery
/// order, each at most once: like the kernel's pending set for standard
/// signals (and dash/bash), several deliveries of one signal received
/// before the shell gets to them coalesce into one. Otherwise a trap that
/// resets itself (`trap 'trap - TERM' TERM; kill -TERM $$ $$`) would see
/// its second, already-caught delivery processed at the default action
/// and kill the shell.
/// Also clears the [`PENDING_EXIT_SIGNAL`] flag.
pub fn drain_pending_signals() -> Vec<i32> {
    // Fast path: no handler has signalled since the last drain — skip the
    // read(2) entirely (this runs after every command). The swap also
    // claims responsibility for whatever bytes are in the pipe.
    if !SIGNAL_PENDING.swap(false, Ordering::AcqRel) {
        return Vec::new();
    }

    // Clear the exit-signal flag before draining so that the terminal poll
    // loop does not spuriously re-trigger after the signal has been handled.
    PENDING_EXIT_SIGNAL.store(false, Ordering::Release);

    let Some(&(read_fd, _)) = SELF_PIPE.get() else {
        return Vec::new();
    };

    let mut signals = Vec::new();
    let mut buf = [0u8; 128];

    loop {
        let n = unsafe { libc::read(read_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n <= 0 {
            break;
        }
        for &b in &buf[..n as usize] {
            let sig = b as i32;
            if !signals.contains(&sig) {
                signals.push(sig);
            }
        }
    }

    signals
}

/// Hand already-drained signals back to the self-pipe, as if they had
/// just been delivered, so the next drain point handles them. For a
/// command that drains signals while it runs but must leave their trap
/// actions until it has completed (`read`, POSIX §2.11).
pub fn requeue_signals(signals: &[i32]) {
    for &sig in signals {
        signal_handler(sig);
    }
}

/// The self-pipe's read end, or `None` before `init_signal_handling`.
pub fn try_self_pipe_read_fd() -> Option<RawFd> {
    SELF_PIPE.get().map(|&(read_fd, _)| read_fd)
}

/// Return the read end of the self-pipe (for use with poll/select).
///
/// # Panics
///
/// Panics if [`init_signal_handling`] has not been called.
pub fn self_pipe_read_fd() -> RawFd {
    SELF_PIPE
        .get()
        .expect("init_signal_handling() must be called first")
        .0
}

/// Close both self-pipe fds, if the pipe exists. For fork children that
/// will `exec`: they have no use for the pipe (the fds are CLOEXEC
/// anyway — closing here also covers exec failure).
pub(super) fn close_self_pipe() {
    if let Some(&(read_fd, write_fd)) = SELF_PIPE.get() {
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
    }
}

/// Replace the inherited self-pipe with a fresh pipe on the SAME fd
/// numbers (the `SELF_PIPE` OnceLock cannot be re-set), and clear the
/// pending-signal flags inherited from the parent. Called only in fork
/// children that keep running shell code; a no-op if the pipe was never
/// created. The old fds are still open when `pipe(2)` runs, so the new
/// temporary fds can never collide with the targets and `dup2` replaces
/// them atomically.
pub(super) fn reinit_self_pipe_after_fork() {
    if let Some(&(read_fd, write_fd)) = SELF_PIPE.get() {
        // SAFETY: plain fd syscalls in a freshly forked, single-threaded
        // child; failures leave the (harmless) inherited pipe in place.
        unsafe {
            let mut fds: [libc::c_int; 2] = [0; 2];
            if libc::pipe(fds.as_mut_ptr()) == 0 {
                libc::dup2(fds[0], read_fd);
                libc::dup2(fds[1], write_fd);
                if fds[0] != read_fd && fds[0] != write_fd {
                    libc::close(fds[0]);
                }
                if fds[1] != read_fd && fds[1] != write_fd {
                    libc::close(fds[1]);
                }
                // dup2 clears O_CLOEXEC and copies file-status flags from
                // the fresh pipe (no O_NONBLOCK) — restore both.
                for &fd in &[read_fd, write_fd] {
                    let fl = libc::fcntl(fd, libc::F_GETFL);
                    libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
                    libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
                }
            }
        }
    }
    // Stale parent state must not leak into the child: a byte the parent
    // handler wrote pre-fork now sits in a pipe the child no longer
    // reads, and these flags belong with it.
    SIGNAL_PENDING.store(false, Ordering::Release);
    PENDING_EXIT_SIGNAL.store(false, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Task 2: Self-pipe tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_init_signal_handling() {
        // init_signal_handling is idempotent — calling it twice must not panic.
        init_signal_handling();
        init_signal_handling();

        let fd = self_pipe_read_fd();
        assert!(fd >= 0, "self_pipe_read_fd() should return a valid fd");
    }

    #[test]
    fn test_drain_pending_signals_empty() {
        init_signal_handling();

        // With no signals sent, drain should return an empty vec.
        let signals = drain_pending_signals();
        assert!(
            signals.is_empty(),
            "expected no pending signals, got: {signals:?}"
        );
    }
}
