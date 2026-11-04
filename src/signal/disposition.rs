//! Signal disposition primitives and the trap builtin's OS-level
//! disposition install.

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};

use super::self_pipe::self_pipe_action;
use super::table::HANDLED_SIGNALS;

/// Query the current disposition of `sig` without changing it
/// (`sigaction(sig, NULL, &mut old)`). `None` when the query fails.
pub(super) fn current_disposition(sig: i32) -> Option<libc::sigaction> {
    let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
    // SAFETY: query-only sigaction (null new action).
    let rc = unsafe { libc::sigaction(sig, std::ptr::null(), &mut old) };
    (rc == 0).then_some(old)
}

/// Set the disposition of `sig` to SIG_IGN.
pub fn ignore_signal(sig: i32) {
    let signal = Signal::try_from(sig).expect("invalid signal number");
    let sa = SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty());
    unsafe {
        sigaction(signal, &sa).expect("sigaction(SIG_IGN) failed");
    }
}

/// Set the disposition of `sig` to SIG_DFL.
pub fn default_signal(sig: i32) {
    let signal = Signal::try_from(sig).expect("invalid signal number");
    let sa = SigAction::new(SigHandler::SigDfl, SaFlags::empty(), SigSet::empty());
    unsafe {
        sigaction(signal, &sa).expect("sigaction(SIG_DFL) failed");
    }
}

/// OS-level disposition matching a trap-store change, for
/// [`apply_trap_disposition`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapDisposition {
    /// `trap 'cmd' SIG` — route the signal through the self-pipe handler.
    Command,
    /// `trap '' SIG` — SIG_IGN (inherited across fork AND exec, per POSIX).
    Ignore,
    /// `trap - SIG` — restore what the shell would have without a trap.
    Default,
}

/// Install the OS signal disposition for a trap-store change made by the
/// `trap` builtin. Without this, traps on signals outside
/// [`HANDLED_SIGNALS`] never take effect (`trap 'echo x' ABRT; kill
/// -ABRT $$` killed the shell at SIG_DFL).
///
/// `monitor` selects the restore target for removed traps on the
/// job-control signals the shell keeps non-default in monitor mode.
/// Errors are ignored (SIGKILL/SIGSTOP cannot be caught; the store-level
/// entry is harmless). Callers must skip signals that were ignored on
/// entry (POSIX §2.12) — the trap store already no-ops those.
pub fn apply_trap_disposition(sig: i32, disposition: TrapDisposition, monitor: bool) {
    let Ok(signal) = Signal::try_from(sig) else {
        return;
    };
    // HUP/TERM deliberately omit SA_RESTART so a blocking terminal read
    // returns EINTR and the exit is handled (see init_signal_handling).
    let restart = sig != libc::SIGHUP && sig != libc::SIGTERM;
    let sa = match disposition {
        TrapDisposition::Command => self_pipe_action(restart),
        TrapDisposition::Ignore => {
            SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty())
        }
        TrapDisposition::Default => {
            if HANDLED_SIGNALS.iter().any(|&(n, _)| n == sig) {
                // The shell always keeps its own handler on these.
                self_pipe_action(restart)
            } else if monitor && matches!(sig, libc::SIGTSTP | libc::SIGTTIN | libc::SIGTTOU) {
                // Monitor mode: the shell itself must not be stopped.
                SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty())
            } else if monitor && sig == libc::SIGCHLD {
                // Monitor mode registers SIGCHLD on the self-pipe.
                self_pipe_action(true)
            } else {
                SigAction::new(SigHandler::SigDfl, SaFlags::empty(), SigSet::empty())
            }
        }
    };
    // SAFETY: installing an async-signal-safe handler or plain
    // disposition; failures (e.g. SIGKILL) are intentionally ignored.
    let _ = unsafe { sigaction(signal, &sa) };
}
