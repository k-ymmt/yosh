use std::os::unix::io::RawFd;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};

/// Set to `true` by the signal handler when SIGHUP or SIGTERM is received.
/// Checked by the terminal read loop to interrupt blocking reads gracefully.
static PENDING_EXIT_SIGNAL: AtomicBool = AtomicBool::new(false);

/// Returns `true` if a SIGHUP or SIGTERM has been received since the last
/// call to [`drain_pending_signals`].
///
/// This is safe to call from any thread or async context.
pub fn has_pending_exit_signal() -> bool {
    PENDING_EXIT_SIGNAL.load(Ordering::Acquire)
}

/// Full signal table for name/number conversion.
pub const SIGNAL_TABLE: &[(i32, &str)] = &[
    (1, "HUP"),
    (2, "INT"),
    (3, "QUIT"),
    (6, "ABRT"),
    (9, "KILL"),
    (10, "USR1"),
    (12, "USR2"),
    (13, "PIPE"),
    (14, "ALRM"),
    (15, "TERM"),
    (17, "CHLD"),
    (18, "CONT"),
    (19, "STOP"),
    (20, "TSTP"),
    (21, "TTIN"),
    (22, "TTOU"),
];

/// Signals for which the shell registers handlers.
pub const HANDLED_SIGNALS: &[(i32, &str)] = &[
    (1, "HUP"),
    (2, "INT"),
    (3, "QUIT"),
    (14, "ALRM"),
    (15, "TERM"),
    (10, "USR1"),
    (12, "USR2"),
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

/// Async-signal-safe handler: writes the signal number as a single byte to the
/// write end of the self-pipe, and sets the PENDING_EXIT_SIGNAL flag for
/// SIGHUP and SIGTERM so that the terminal read loop can notice quickly.
extern "C" fn signal_handler(sig: libc::c_int) {
    // AtomicBool::store is async-signal-safe.
    if sig == libc::SIGHUP || sig == libc::SIGTERM {
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
}

/// Create the self-pipe (O_NONBLOCK | O_CLOEXEC) and register sigaction
/// handlers for every signal in [`HANDLED_SIGNALS`].
///
/// This function is idempotent — calling it more than once is a no-op.
pub fn init_signal_handling() {
    SELF_PIPE.get_or_init(|| {
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
    // Clear the exit-signal flag before draining so that the terminal poll
    // loop does not spuriously re-trigger after the signal has been handled.
    PENDING_EXIT_SIGNAL.store(false, Ordering::Release);

    let Some(&(read_fd, _)) = SELF_PIPE.get() else {
        return Vec::new();
    };

    let mut signals = Vec::new();
    let mut buf = [0u8; 128];

    loop {
        let n = unsafe {
            libc::read(
                read_fd,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
            )
        };
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
pub fn reset_child_signals(ignored: &[i32]) {
    for &(num, _) in HANDLED_SIGNALS {
        if ignored.contains(&num) {
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
    if !ignored.contains(&libc::SIGTSTP) { default_signal(libc::SIGTSTP); }
    if !ignored.contains(&libc::SIGTTIN) { default_signal(libc::SIGTTIN); }
    if !ignored.contains(&libc::SIGTTOU) { default_signal(libc::SIGTTOU); }
}

/// Set up signals for a background child process.
/// Ignores SIGTTIN to prevent background reads from stopping.
pub fn setup_background_child_signals(ignored: &[i32]) {
    reset_child_signals(ignored);
    ignore_signal(libc::SIGTTIN);
    if !ignored.contains(&libc::SIGTSTP) { default_signal(libc::SIGTSTP); }
    if !ignored.contains(&libc::SIGTTOU) { default_signal(libc::SIGTTOU); }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Task 1: Signal table tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_signal_name_to_number_int() {
        assert_eq!(signal_name_to_number("INT").unwrap(), 2);
    }

    #[test]
    fn test_signal_name_to_number_sigint() {
        assert_eq!(signal_name_to_number("SIGINT").unwrap(), 2);
    }

    #[test]
    fn test_signal_name_to_number_case_insensitive() {
        assert_eq!(signal_name_to_number("hup").unwrap(), 1);
    }

    #[test]
    fn test_signal_name_to_number_term() {
        assert_eq!(signal_name_to_number("TERM").unwrap(), 15);
    }

    #[test]
    fn test_signal_name_to_number_kill() {
        assert_eq!(signal_name_to_number("KILL").unwrap(), 9);
    }

    #[test]
    fn test_signal_name_to_number_invalid() {
        assert!(signal_name_to_number("INVALID").is_err());
    }

    #[test]
    fn test_signal_number_to_name_2() {
        assert_eq!(signal_number_to_name(2), Some("INT"));
    }

    #[test]
    fn test_signal_number_to_name_15() {
        assert_eq!(signal_number_to_name(15), Some("TERM"));
    }

    #[test]
    fn test_signal_number_to_name_9() {
        assert_eq!(signal_number_to_name(9), Some("KILL"));
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
        assert_eq!(signal_name_to_number("CHLD").unwrap(), 17);
        assert_eq!(signal_name_to_number("CONT").unwrap(), 18);
        assert_eq!(signal_name_to_number("STOP").unwrap(), 19);
        assert_eq!(signal_name_to_number("TSTP").unwrap(), 20);
        assert_eq!(signal_name_to_number("TTIN").unwrap(), 21);
        assert_eq!(signal_name_to_number("TTOU").unwrap(), 22);
    }

    #[test]
    fn test_signal_number_to_name_job_control() {
        assert_eq!(signal_number_to_name(17), Some("CHLD"));
        assert_eq!(signal_number_to_name(20), Some("TSTP"));
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
}
