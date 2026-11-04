//! Job-control signal setup for the shell process itself: monitor-mode
//! enable/disable and the interactive-startup foreground wait.

use std::os::fd::BorrowedFd;
use std::sync::atomic::{AtomicBool, Ordering};

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};

use super::disposition::{default_signal, ignore_signal, note_disposition_change};
use super::self_pipe::self_pipe_action;

/// Set up job control signals for the shell process itself.
/// Ignores SIGTSTP, SIGTTIN, SIGTTOU so the shell is not stopped.
/// Adds SIGCHLD to the self-pipe handler.
pub fn init_job_control_signals() {
    ignore_signal(libc::SIGTSTP);
    ignore_signal(libc::SIGTTIN);
    ignore_signal(libc::SIGTTOU);

    // Register SIGCHLD handler via self-pipe
    let sa = self_pipe_action(true);
    note_disposition_change(libc::SIGCHLD);
    let sig = Signal::try_from(libc::SIGCHLD).expect("SIGCHLD is valid");
    unsafe {
        sigaction(sig, &sa).expect("sigaction(SIGCHLD) failed");
    }
}

/// The fd the terminal handoffs use (`jobs::terminal_fd()`, `/dev/tty`
/// with an fd-0 fallback). Probing it rather than stdin means the
/// terminal checks here can never pass for a terminal that
/// `give_terminal`/`take_terminal` would not actually target.
fn terminal_fd() -> BorrowedFd<'static> {
    // SAFETY: terminal_fd() is either the leaked /dev/tty fd or fd 0;
    // both live for the process lifetime.
    unsafe { BorrowedFd::borrow_raw(crate::env::jobs::terminal_fd()) }
}

/// Whether this shell owns its controlling terminal: the terminal's
/// foreground process group is the shell's own. Probes the same fd
/// the terminal handoffs use (`jobs::terminal_fd()`, `/dev/tty` with
/// an fd-0 fallback) rather than stdin, so a foreground
/// `yosh -m script <input` with redirected stdin still detects
/// ownership, and the gate can never authorize a terminal that
/// `give_terminal`/`take_terminal` would not actually target.
fn owns_controlling_terminal() -> bool {
    nix::unistd::tcgetpgrp(terminal_fd()).ok() == Some(nix::unistd::getpgrp())
}

/// Shared monitor-mode enable transition for invocation `-m`
/// (`run_string`) and the runtime `set -m` builtin: job-control signal
/// setup runs only when the shell actually owns its controlling
/// terminal. Otherwise a background shell would either be stopped by
/// SIGTTOU on the terminal handoffs (with SIG_DFL) or steal the
/// terminal from the invoking shell (with SIG_IGN). bash likewise
/// disables job control — dropping `m` from `$-` — when it cannot get
/// the terminal. Returns whether monitor mode was actually enabled;
/// on `false` the caller must clear its `monitor` flag.
pub fn try_enable_monitor_mode() -> bool {
    if owns_controlling_terminal() {
        init_job_control_signals();
        true
    } else {
        false
    }
}

/// Set by [`cont_flag_handler`] while [`wait_until_foreground`] runs:
/// distinguishes a genuine stop/continue cycle (the parent resumed us
/// with SIGCONT) from a *discarded* self-stop — POSIX discards stop
/// signals sent to an orphaned process group, and no SIGCONT ever
/// follows a discarded stop.
static WAIT_FG_CONT_SEEN: AtomicBool = AtomicBool::new(false);

extern "C" fn cont_flag_handler(_sig: libc::c_int) {
    // AtomicBool::store is async-signal-safe.
    WAIT_FG_CONT_SEEN.store(true, Ordering::Release);
}

/// Interactive-startup foreground wait (the glibc-manual "Initializing
/// the Shell" loop): a REPL launched in the background of a
/// job-controlling parent (`yosh &`) must not run its startup
/// `take_terminal` — that would steal the terminal from the parent.
/// Instead the shell stops itself with SIGTTIN until the user
/// foregrounds it (`fg` hands the terminal over and delivers SIGCONT),
/// then returns `true` so the caller can finish job-control init as
/// the real foreground process group. Returns `false` when the shell
/// has no controlling terminal at all (tcgetpgrp fails), or when the
/// self-stop is being discarded (orphaned process group): the caller
/// must run without monitor mode, mirroring [`try_enable_monitor_mode`].
pub fn wait_until_foreground() -> bool {
    // The SIGCONT flag handler is installed only for the duration of
    // the wait loop; the entry disposition is restored on every exit.
    let flag_cont = SigAction::new(
        SigHandler::Handler(cont_flag_handler),
        SaFlags::SA_RESTART,
        SigSet::empty(),
    );
    note_disposition_change(libc::SIGCONT);
    // SAFETY: installing an async-signal-safe flag handler (atomic
    // store only); the previous disposition is restored below.
    let old_cont =
        unsafe { sigaction(Signal::SIGCONT, &flag_cont) }.expect("sigaction(SIGCONT) failed");
    let result = wait_until_foreground_loop();
    // SAFETY: restoring the disposition captured above.
    unsafe { sigaction(Signal::SIGCONT, &old_cont) }.expect("sigaction(SIGCONT, restore) failed");
    result
}

/// The check/self-stop loop of [`wait_until_foreground`]; assumes the
/// caller has the SIGCONT flag handler installed.
fn wait_until_foreground_loop() -> bool {
    // Probe the same fd the terminal handoffs use (/dev/tty with an
    // fd-0 fallback) so the wait can never pass for a terminal that
    // `take_terminal` would not actually target.
    let fd = terminal_fd();
    // Bounded spin detector, not `loop`: SIGTTIN cannot stop an
    // orphaned process group (POSIX discards stop signals for them),
    // so a backgrounded shell whose parent died would spin here
    // forever; running without job control beats burning a core. Only
    // consecutive SIGCONT-less iterations count toward the bound — a
    // genuine stop always ends with the parent's SIGCONT, so any
    // number of legitimate `bg` re-stop cycles resets the counter
    // (bash loops here unboundedly; the bound only cuts the discarded
    // case loose).
    let mut spins = 0;
    while spins < 64 {
        match nix::unistd::tcgetpgrp(fd) {
            Err(_) => return false,
            Ok(pg) if pg == nix::unistd::getpgrp() => return true,
            Ok(_) => {
                WAIT_FG_CONT_SEEN.store(false, Ordering::Release);
                // SIGTTIN is forced to SIG_DFL around the self-stop
                // and restored afterwards (bash does the same): a
                // background child inherits SIG_IGN for it from a
                // job-controlling parent, which would otherwise turn
                // this stop into a busy spin.
                let dfl = SigAction::new(SigHandler::SigDfl, SaFlags::empty(), SigSet::empty());
                note_disposition_change(libc::SIGTTIN);
                // SAFETY: installing/restoring plain dispositions for
                // SIGTTIN; no handler code is involved.
                let old = unsafe { sigaction(Signal::SIGTTIN, &dfl) }
                    .expect("sigaction(SIGTTIN, SIG_DFL) failed");
                // Stops the whole process group until continued; on
                // SIGCONT the loop re-checks foreground ownership (a
                // `bg` leaves us background and we stop again). If
                // SIGTTIN is blocked it only becomes pending here.
                let _ = nix::sys::signal::killpg(nix::unistd::getpgrp(), Signal::SIGTTIN);
                // The signal mask survives exec, so a parent may have
                // handed us SIGTTIN blocked — killpg above then left
                // it pending instead of stopping us. Unblock it now
                // (delivering the stop) and restore the mask after;
                // pending standard signals collapse, so at most one
                // stop is delivered per iteration either way.
                let mut ttin = SigSet::empty();
                ttin.add(Signal::SIGTTIN);
                let mut prev_mask = SigSet::empty();
                if nix::sys::signal::sigprocmask(
                    nix::sys::signal::SigmaskHow::SIG_UNBLOCK,
                    Some(&ttin),
                    Some(&mut prev_mask),
                )
                .is_ok()
                {
                    let _ = nix::sys::signal::sigprocmask(
                        nix::sys::signal::SigmaskHow::SIG_SETMASK,
                        Some(&prev_mask),
                        None,
                    );
                }
                // SAFETY: restoring the disposition captured above.
                unsafe { sigaction(Signal::SIGTTIN, &old) }
                    .expect("sigaction(SIGTTIN, restore) failed");
                // A delivered stop always ends with the parent's
                // SIGCONT (observed by the flag handler); a discarded
                // one never sees a SIGCONT. Only SIGCONT-less
                // iterations count toward the bound.
                if WAIT_FG_CONT_SEEN.load(Ordering::Acquire) {
                    spins = 0;
                } else {
                    spins += 1;
                }
            }
        }
    }
    false
}

/// Reset job control signals to defaults.
/// Called when `set +m` disables monitor mode at runtime.
pub fn reset_job_control_signals() {
    default_signal(libc::SIGTSTP);
    default_signal(libc::SIGTTIN);
    default_signal(libc::SIGTTOU);
    default_signal(libc::SIGCHLD);
}
