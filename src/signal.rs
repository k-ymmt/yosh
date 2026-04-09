use std::os::unix::io::RawFd;
use std::sync::OnceLock;

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};

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
/// write end of the self-pipe.
extern "C" fn signal_handler(sig: libc::c_int) {
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

        // Set O_NONBLOCK | O_CLOEXEC on both ends.
        for &fd in &fds {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            unsafe {
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
            }
        }

        let read_fd = fds[0];
        let write_fd = fds[1];

        // Register sigaction handlers for all HANDLED_SIGNALS.
        let sa = SigAction::new(
            SigHandler::Handler(signal_handler),
            SaFlags::SA_RESTART,
            SigSet::empty(),
        );

        for &(num, _) in HANDLED_SIGNALS {
            let sig = Signal::try_from(num).expect("invalid signal number in HANDLED_SIGNALS");
            unsafe {
                sigaction(sig, &sa).expect("sigaction failed");
            }
        }

        (read_fd, write_fd)
    });
}

/// Non-blocking read of all pending signal bytes from the self-pipe.
///
/// Returns a (possibly empty) vector of signal numbers.
pub fn drain_pending_signals() -> Vec<i32> {
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
}
