use std::collections::HashSet;
use std::os::unix::io::RawFd;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};

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
/// [`reset_child_signals`] before this flag could matter to them.
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

/// Full signal table for name/number conversion.
pub const SIGNAL_TABLE: &[(i32, &str)] = &[
    (libc::SIGHUP, "HUP"),
    (libc::SIGINT, "INT"),
    (libc::SIGQUIT, "QUIT"),
    (libc::SIGABRT, "ABRT"),
    (libc::SIGKILL, "KILL"),
    (libc::SIGUSR1, "USR1"),
    (libc::SIGUSR2, "USR2"),
    (libc::SIGPIPE, "PIPE"),
    (libc::SIGALRM, "ALRM"),
    (libc::SIGTERM, "TERM"),
    (libc::SIGCHLD, "CHLD"),
    (libc::SIGCONT, "CONT"),
    (libc::SIGSTOP, "STOP"),
    (libc::SIGTSTP, "TSTP"),
    (libc::SIGTTIN, "TTIN"),
    (libc::SIGTTOU, "TTOU"),
];

/// Signals for which the shell registers handlers.
pub const HANDLED_SIGNALS: &[(i32, &str)] = &[
    (libc::SIGHUP, "HUP"),
    (libc::SIGINT, "INT"),
    (libc::SIGQUIT, "QUIT"),
    (libc::SIGALRM, "ALRM"),
    (libc::SIGTERM, "TERM"),
    (libc::SIGUSR1, "USR1"),
    (libc::SIGUSR2, "USR2"),
];

/// Look up a signal number by name (case-insensitive, strips optional "SIG" prefix).
pub fn signal_name_to_number(name: &str) -> Result<i32, String> {
    let upper = name.to_ascii_uppercase();
    let stripped = upper.strip_prefix("SIG").unwrap_or(&upper);

    for &(num, table_name) in SIGNAL_TABLE {
        if table_name == stripped {
            return Ok(num);
        }
    }

    Err(format!("unknown signal: {name}"))
}

/// Look up a signal name by number.
pub fn signal_number_to_name(num: i32) -> Option<&'static str> {
    for &(table_num, name) in SIGNAL_TABLE {
        if table_num == num {
            return Some(name);
        }
    }
    None
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

/// Signals inherited with SIG_IGN disposition at shell entry.
/// Per POSIX §2.12, these signals cannot be trapped or reset by the shell.
/// Captured once at startup before any yosh handler is installed; never mutated
/// afterward, so a stale `get()` from a fork/exec child reflects the correct
/// entry state (because the global is inherited as a copy of the parent's set).
static IGNORED_ON_ENTRY: OnceLock<HashSet<i32>> = OnceLock::new();

/// Query each trappable POSIX signal's current disposition via `sigaction(_, NULL, &mut old)`
/// and return the set of signals currently set to SIG_IGN.
/// Must be called before any yosh handler is installed to correctly observe
/// what was inherited from the parent process.
fn capture_ignored_on_entry() -> HashSet<i32> {
    let mut set = HashSet::new();
    for &(num, _) in SIGNAL_TABLE {
        if num == libc::SIGKILL || num == libc::SIGSTOP {
            // SIGKILL/SIGSTOP cannot be caught or ignored; skip them.
            continue;
        }
        if num == libc::SIGPIPE {
            // The Rust std runtime sets SIGPIPE to SIG_IGN before main(),
            // so an observed SIG_IGN here is (almost always) the runtime's
            // doing, not an inherited disposition from the invoking
            // process. Treating it as ignored-on-entry made `trap ... PIPE`
            // a silent no-op, listed a phantom `trap -- '' SIGPIPE`, and
            // kept SIGPIPE ignored in every child (`find | head` broke).
            // The runtime's clobber makes the true inherited state
            // unrecoverable; assume the common case (not ignored).
            // init_signal_handling restores SIG_DFL right after this.
            continue;
        }
        let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::sigaction(num, std::ptr::null(), &mut old) };
        if rc != 0 {
            continue;
        }
        if old.sa_sigaction == libc::SIG_IGN {
            set.insert(num);
        }
    }
    set
}

/// Returns `true` if `sig` was inherited with SIG_IGN disposition at shell startup.
/// Returns `false` if [`init_signal_handling`] has not been called yet.
pub fn is_ignored_on_entry(sig: i32) -> bool {
    IGNORED_ON_ENTRY.get().is_some_and(|set| set.contains(&sig))
}

/// Like [`ignored_on_entry_set`] but returns `None` if the capture has not
/// happened yet (useful for callers that must not panic, e.g. `display_all`).
pub fn ignored_on_entry_set_opt() -> Option<&'static HashSet<i32>> {
    IGNORED_ON_ENTRY.get()
}

/// Returns a reference to the set of ignored-on-entry signals.
///
/// # Panics
///
/// Panics if [`init_signal_handling`] has not been called.
#[allow(dead_code)]
pub fn ignored_on_entry_set() -> &'static HashSet<i32> {
    IGNORED_ON_ENTRY
        .get()
        .expect("init_signal_handling() must be called first")
}

/// Async-signal-safe handler: writes the signal number as a single byte to the
/// write end of the self-pipe, and sets the PENDING_EXIT_SIGNAL flag for
/// SIGHUP and SIGTERM so that the terminal read loop can notice quickly.
extern "C" fn signal_handler(sig: libc::c_int) {
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

/// Create the self-pipe (O_NONBLOCK | O_CLOEXEC) and register sigaction
/// handlers for every signal in [`HANDLED_SIGNALS`].
///
/// This function is idempotent — calling it more than once is a no-op.
pub fn init_signal_handling() {
    SELF_PIPE.get_or_init(|| {
        // POSIX §2.12: capture the set of signals inherited as SIG_IGN before we
        // install any yosh handler. Skip registration for those signals so they
        // remain ignored for the shell's lifetime.
        let entry_ignored = IGNORED_ON_ENTRY.get_or_init(capture_ignored_on_entry);

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
        let sa_restart = SigAction::new(
            SigHandler::Handler(signal_handler),
            SaFlags::SA_RESTART,
            SigSet::empty(),
        );
        let sa_no_restart = SigAction::new(
            SigHandler::Handler(signal_handler),
            SaFlags::empty(),
            SigSet::empty(),
        );

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
/// Returns a (possibly empty) vector of signal numbers.
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
            signals.push(b as i32);
        }
    }

    signals
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

/// Reset signals after fork for child processes.
/// `ignored` signals retain SIG_IGN; all others reset to SIG_DFL.
/// Signals inherited as SIG_IGN at shell entry (§2.12) are also kept ignored.
///
/// Walks the full SIGNAL_TABLE (not just HANDLED_SIGNALS): the trap
/// builtin can install handlers/SIG_IGN for any trappable signal (e.g.
/// `trap 'cmd' ABRT`, `trap '' PIPE`), and the shell itself ignores the
/// job-control signals in monitor mode — all of those must come back to
/// SIG_DFL in children unless the user trap-ignored them. Callers that
/// need job-control exceptions (`setup_*_child_signals`) adjust after.
pub fn reset_child_signals(ignored: &[i32]) {
    let entry_set = IGNORED_ON_ENTRY.get();
    for &(num, _) in SIGNAL_TABLE {
        if num == libc::SIGKILL || num == libc::SIGSTOP {
            // Cannot be caught or ignored; sigaction would fail.
            continue;
        }
        let keep_ignored = ignored.contains(&num) || entry_set.is_some_and(|s| s.contains(&num));
        if keep_ignored {
            ignore_signal(num);
        } else {
            default_signal(num);
        }
    }

    // Close self-pipe fds if they exist.
    if let Some(&(read_fd, write_fd)) = SELF_PIPE.get() {
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
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
    let self_pipe_handler = |restart: bool| {
        SigAction::new(
            SigHandler::Handler(signal_handler),
            if restart {
                SaFlags::SA_RESTART
            } else {
                SaFlags::empty()
            },
            SigSet::empty(),
        )
    };
    // HUP/TERM deliberately omit SA_RESTART so a blocking terminal read
    // returns EINTR and the exit is handled (see init_signal_handling).
    let restart = sig != libc::SIGHUP && sig != libc::SIGTERM;
    let sa = match disposition {
        TrapDisposition::Command => self_pipe_handler(restart),
        TrapDisposition::Ignore => SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty()),
        TrapDisposition::Default => {
            if HANDLED_SIGNALS.iter().any(|&(n, _)| n == sig) {
                // The shell always keeps its own handler on these.
                self_pipe_handler(restart)
            } else if monitor && matches!(sig, libc::SIGTSTP | libc::SIGTTIN | libc::SIGTTOU) {
                // Monitor mode: the shell itself must not be stopped.
                SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty())
            } else if monitor && sig == libc::SIGCHLD {
                // Monitor mode registers SIGCHLD on the self-pipe.
                self_pipe_handler(true)
            } else {
                SigAction::new(SigHandler::SigDfl, SaFlags::empty(), SigSet::empty())
            }
        }
    };
    // SAFETY: installing an async-signal-safe handler or plain
    // disposition; failures (e.g. SIGKILL) are intentionally ignored.
    let _ = unsafe { sigaction(signal, &sa) };
}

/// Set up job control signals for the shell process itself.
/// Ignores SIGTSTP, SIGTTIN, SIGTTOU so the shell is not stopped.
/// Adds SIGCHLD to the self-pipe handler.
pub fn init_job_control_signals() {
    ignore_signal(libc::SIGTSTP);
    ignore_signal(libc::SIGTTIN);
    ignore_signal(libc::SIGTTOU);

    // Register SIGCHLD handler via self-pipe
    let sa = SigAction::new(
        SigHandler::Handler(signal_handler),
        SaFlags::SA_RESTART,
        SigSet::empty(),
    );
    let sig = Signal::try_from(libc::SIGCHLD).expect("SIGCHLD is valid");
    unsafe {
        sigaction(sig, &sa).expect("sigaction(SIGCHLD) failed");
    }
}

/// Whether this shell owns its controlling terminal: the terminal's
/// foreground process group is the shell's own. Probes the same fd
/// the terminal handoffs use (`jobs::terminal_fd()`, `/dev/tty` with
/// an fd-0 fallback) rather than stdin, so a foreground
/// `yosh -m script <input` with redirected stdin still detects
/// ownership, and the gate can never authorize a terminal that
/// `give_terminal`/`take_terminal` would not actually target.
fn owns_controlling_terminal() -> bool {
    // SAFETY: terminal_fd() is either the leaked /dev/tty fd or fd 0;
    // both live for the process lifetime.
    let fd = unsafe { std::os::fd::BorrowedFd::borrow_raw(crate::env::jobs::terminal_fd()) };
    nix::unistd::tcgetpgrp(fd).ok() == Some(nix::unistd::getpgrp())
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
    // SAFETY: terminal_fd() is either the leaked /dev/tty fd or fd 0;
    // both live for the process lifetime.
    let fd = unsafe { std::os::fd::BorrowedFd::borrow_raw(crate::env::jobs::terminal_fd()) };
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

/// Set up signals for a foreground child process.
/// Restores SIGTSTP, SIGTTIN, SIGTTOU to SIG_DFL so the child can be stopped.
pub fn setup_foreground_child_signals(ignored: &[i32]) {
    reset_child_signals(ignored);
    if !ignored.contains(&libc::SIGTSTP) {
        default_signal(libc::SIGTSTP);
    }
    if !ignored.contains(&libc::SIGTTIN) {
        default_signal(libc::SIGTTIN);
    }
    if !ignored.contains(&libc::SIGTTOU) {
        default_signal(libc::SIGTTOU);
    }
}

/// Set up signals for a background child process.
/// Ignores SIGTTIN to prevent background reads from stopping.
pub fn setup_background_child_signals(ignored: &[i32]) {
    reset_child_signals(ignored);
    ignore_signal(libc::SIGTTIN);
    if !ignored.contains(&libc::SIGTSTP) {
        default_signal(libc::SIGTSTP);
    }
    if !ignored.contains(&libc::SIGTTOU) {
        default_signal(libc::SIGTTOU);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Task 1: Signal table tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_signal_name_to_number_int() {
        assert_eq!(signal_name_to_number("INT").unwrap(), libc::SIGINT);
    }

    #[test]
    fn test_signal_name_to_number_sigint() {
        assert_eq!(signal_name_to_number("SIGINT").unwrap(), libc::SIGINT);
    }

    #[test]
    fn test_signal_name_to_number_case_insensitive() {
        assert_eq!(signal_name_to_number("hup").unwrap(), libc::SIGHUP);
    }

    #[test]
    fn test_signal_name_to_number_term() {
        assert_eq!(signal_name_to_number("TERM").unwrap(), libc::SIGTERM);
    }

    #[test]
    fn test_signal_name_to_number_kill() {
        assert_eq!(signal_name_to_number("KILL").unwrap(), libc::SIGKILL);
    }

    #[test]
    fn test_signal_name_to_number_invalid() {
        assert!(signal_name_to_number("INVALID").is_err());
    }

    #[test]
    fn test_signal_number_to_name_2() {
        assert_eq!(signal_number_to_name(libc::SIGINT), Some("INT"));
    }

    #[test]
    fn test_signal_number_to_name_15() {
        assert_eq!(signal_number_to_name(libc::SIGTERM), Some("TERM"));
    }

    #[test]
    fn test_signal_number_to_name_9() {
        assert_eq!(signal_number_to_name(libc::SIGKILL), Some("KILL"));
    }

    #[test]
    fn test_signal_number_to_name_999() {
        assert_eq!(signal_number_to_name(999), None);
    }

    #[test]
    fn test_handled_signals_are_in_signal_table() {
        // Every signal in HANDLED_SIGNALS must exist in SIGNAL_TABLE.
        for &(num, name) in HANDLED_SIGNALS {
            let found = SIGNAL_TABLE.iter().any(|&(n, nm)| n == num && nm == name);
            assert!(
                found,
                "HANDLED_SIGNALS entry ({num}, {name}) not found in SIGNAL_TABLE"
            );
        }
    }

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

    #[test]
    fn test_signal_table_has_job_control_signals() {
        assert_eq!(signal_name_to_number("CHLD").unwrap(), libc::SIGCHLD);
        assert_eq!(signal_name_to_number("CONT").unwrap(), libc::SIGCONT);
        assert_eq!(signal_name_to_number("STOP").unwrap(), libc::SIGSTOP);
        assert_eq!(signal_name_to_number("TSTP").unwrap(), libc::SIGTSTP);
        assert_eq!(signal_name_to_number("TTIN").unwrap(), libc::SIGTTIN);
        assert_eq!(signal_name_to_number("TTOU").unwrap(), libc::SIGTTOU);
    }

    #[test]
    fn test_signal_number_to_name_job_control() {
        assert_eq!(signal_number_to_name(libc::SIGCHLD), Some("CHLD"));
        assert_eq!(signal_number_to_name(libc::SIGTSTP), Some("TSTP"));
    }

    #[test]
    fn test_job_control_signal_functions_exist() {
        let _ = init_job_control_signals as fn();
        let _ = reset_job_control_signals as fn();
        let _ = setup_foreground_child_signals as fn(&[i32]);
        let _ = setup_background_child_signals as fn(&[i32]);
    }

    #[test]
    fn test_reset_job_control_signals_after_init() {
        init_signal_handling();
        init_job_control_signals();
        reset_job_control_signals();
        // No panic = success
    }

    // -----------------------------------------------------------------------
    // Sub-project 5 — Task 1: Ignored-on-entry capture tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_ignored_on_entry_false_for_unlikely_signal() {
        // After init (possibly already called by other tests), a benign signal
        // that is extremely unlikely to be inherited as SIG_IGN in a `cargo test`
        // run should report `false`. SIGALRM is a safe choice — its number (14)
        // is identical on Linux and macOS AND is present in SIGNAL_TABLE, so
        // the assertion actually exercises the capture path on both platforms.
        init_signal_handling();
        assert!(
            !is_ignored_on_entry(libc::SIGALRM),
            "SIGALRM should not be ignored-on-entry in a normal test environment"
        );
    }

    #[test]
    fn test_capture_ignored_on_entry_detects_sig_ign() {
        // IMPORTANT: Initialize IGNORED_ON_ENTRY with a clean signal state
        // BEFORE we mutate SIGALRM. This ensures that parallel tests running
        // is_ignored_on_entry(...) or init_signal_handling() do not observe
        // this test's mid-flight SIG_IGN as part of the "inherited at entry"
        // set. OnceLock::get_or_init guarantees atomic one-shot init.
        init_signal_handling();

        // It exercises `capture_ignored_on_entry` directly to verify the
        // sigaction query logic. We use SIGALRM (14) which is in SIGNAL_TABLE
        // on both Linux (num 14) and macOS (num 14). On macOS, SIGUSR2=31
        // is not in SIGNAL_TABLE, so SIGALRM is used instead. We restore the
        // original disposition afterward to avoid polluting sibling tests.
        let sig_num = libc::SIGALRM;

        // Save the current disposition.
        let mut original: libc::sigaction = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::sigaction(sig_num, std::ptr::null(), &mut original) };
        assert_eq!(rc, 0);

        // Install SIG_IGN.
        let ign_sa = SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty());
        let sig = Signal::try_from(sig_num).unwrap();
        unsafe {
            sigaction(sig, &ign_sa).unwrap();
        }

        // Run the capture helper and assert SIGALRM is in the set.
        let captured = capture_ignored_on_entry();
        assert!(
            captured.contains(&sig_num),
            "capture_ignored_on_entry should detect SIGALRM SIG_IGN, got {:?}",
            captured
        );

        // Restore original disposition.
        let rc = unsafe { libc::sigaction(sig_num, &original, std::ptr::null_mut()) };
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_capture_ignored_on_entry_excludes_default() {
        // IMPORTANT: Initialize IGNORED_ON_ENTRY with a clean signal state
        // BEFORE we mutate SIGPIPE. This ensures that parallel tests running
        // is_ignored_on_entry(...) or init_signal_handling() do not observe
        // this test's mid-flight SIG_DFL mutation as part of the captured set.
        // OnceLock::get_or_init guarantees atomic one-shot init.
        init_signal_handling();

        // SIGPIPE (13) at SIG_DFL should NOT appear in the captured set.
        // SIGPIPE is in SIGNAL_TABLE on both Linux and macOS with number 13.
        let sig_num = libc::SIGPIPE;

        let mut original: libc::sigaction = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::sigaction(sig_num, std::ptr::null(), &mut original) };
        assert_eq!(rc, 0);

        let dfl_sa = SigAction::new(SigHandler::SigDfl, SaFlags::empty(), SigSet::empty());
        let sig = Signal::try_from(sig_num).unwrap();
        unsafe {
            sigaction(sig, &dfl_sa).unwrap();
        }

        let captured = capture_ignored_on_entry();
        assert!(
            !captured.contains(&sig_num),
            "capture_ignored_on_entry should not include SIG_DFL signals, got {:?}",
            captured
        );

        // Restore.
        let rc = unsafe { libc::sigaction(sig_num, &original, std::ptr::null_mut()) };
        assert_eq!(rc, 0);
    }

    /// Map a POSIX signal short-name (e.g. `"HUP"`) to its `libc::SIG*`
    /// constant. Adding a new entry to `SIGNAL_TABLE` only requires
    /// updating this single table, not both libc-constant tests below.
    fn name_to_libc(name: &str) -> Option<i32> {
        Some(match name {
            "HUP" => libc::SIGHUP,
            "INT" => libc::SIGINT,
            "QUIT" => libc::SIGQUIT,
            "ABRT" => libc::SIGABRT,
            "KILL" => libc::SIGKILL,
            "USR1" => libc::SIGUSR1,
            "USR2" => libc::SIGUSR2,
            "PIPE" => libc::SIGPIPE,
            "ALRM" => libc::SIGALRM,
            "TERM" => libc::SIGTERM,
            "CHLD" => libc::SIGCHLD,
            "CONT" => libc::SIGCONT,
            "STOP" => libc::SIGSTOP,
            "TSTP" => libc::SIGTSTP,
            "TTIN" => libc::SIGTTIN,
            "TTOU" => libc::SIGTTOU,
            _ => return None,
        })
    }

    #[test]
    fn test_signal_table_matches_libc_constants() {
        // Portable check: the table must agree with libc on every entry.
        // Pre-fix this would have failed on macOS for USR1/USR2/CHLD/CONT/STOP/TSTP
        // because the table hard-coded Linux signal numbers.
        for &(num, name) in SIGNAL_TABLE {
            let expected = name_to_libc(name)
                .unwrap_or_else(|| panic!("unexpected signal name in SIGNAL_TABLE: {name}"));
            assert_eq!(
                num, expected,
                "SIGNAL_TABLE entry for {name} has {num}, libc says {expected}"
            );
        }
    }

    #[test]
    fn test_handled_signals_match_libc_constants() {
        for &(num, name) in HANDLED_SIGNALS {
            let expected = name_to_libc(name)
                .unwrap_or_else(|| panic!("unexpected signal name in HANDLED_SIGNALS: {name}"));
            assert_eq!(
                num, expected,
                "HANDLED_SIGNALS entry for {name} has {num}, libc says {expected}"
            );
        }
    }
}
