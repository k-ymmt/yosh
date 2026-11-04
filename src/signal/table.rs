//! Signal tables: name/number conversion, the handled set, and the
//! default action of each signal.

/// Full signal table for name/number conversion: every signal the
/// platform defines (`kill -l`, `kill -s NAME`, `trap … NAME`), in
/// ascending signal-number order. Numbers come from libc, so the table
/// is correct per platform; only the set and order are per-OS.
///
/// Every consumer that walks the table (ignored-on-entry capture, child
/// disposition reset) must skip SIGKILL/SIGSTOP, which cannot be caught
/// or ignored — see [`catchable_signals`].
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub const SIGNAL_TABLE: &[(i32, &str)] = &[
    (libc::SIGHUP, "HUP"),
    (libc::SIGINT, "INT"),
    (libc::SIGQUIT, "QUIT"),
    (libc::SIGILL, "ILL"),
    (libc::SIGTRAP, "TRAP"),
    (libc::SIGABRT, "ABRT"),
    (libc::SIGEMT, "EMT"),
    (libc::SIGFPE, "FPE"),
    (libc::SIGKILL, "KILL"),
    (libc::SIGBUS, "BUS"),
    (libc::SIGSEGV, "SEGV"),
    (libc::SIGSYS, "SYS"),
    (libc::SIGPIPE, "PIPE"),
    (libc::SIGALRM, "ALRM"),
    (libc::SIGTERM, "TERM"),
    (libc::SIGURG, "URG"),
    (libc::SIGSTOP, "STOP"),
    (libc::SIGTSTP, "TSTP"),
    (libc::SIGCONT, "CONT"),
    (libc::SIGCHLD, "CHLD"),
    (libc::SIGTTIN, "TTIN"),
    (libc::SIGTTOU, "TTOU"),
    (libc::SIGIO, "IO"),
    (libc::SIGXCPU, "XCPU"),
    (libc::SIGXFSZ, "XFSZ"),
    (libc::SIGVTALRM, "VTALRM"),
    (libc::SIGPROF, "PROF"),
    (libc::SIGWINCH, "WINCH"),
    (libc::SIGINFO, "INFO"),
    (libc::SIGUSR1, "USR1"),
    (libc::SIGUSR2, "USR2"),
];

/// See the macOS variant. Linux numbering (x86/ARM; libc supplies the
/// right numbers on other architectures).
#[cfg(any(target_os = "linux", target_os = "android"))]
pub const SIGNAL_TABLE: &[(i32, &str)] = &[
    (libc::SIGHUP, "HUP"),
    (libc::SIGINT, "INT"),
    (libc::SIGQUIT, "QUIT"),
    (libc::SIGILL, "ILL"),
    (libc::SIGTRAP, "TRAP"),
    (libc::SIGABRT, "ABRT"),
    (libc::SIGBUS, "BUS"),
    (libc::SIGFPE, "FPE"),
    (libc::SIGKILL, "KILL"),
    (libc::SIGUSR1, "USR1"),
    (libc::SIGSEGV, "SEGV"),
    (libc::SIGUSR2, "USR2"),
    (libc::SIGPIPE, "PIPE"),
    (libc::SIGALRM, "ALRM"),
    (libc::SIGTERM, "TERM"),
    (libc::SIGSTKFLT, "STKFLT"),
    (libc::SIGCHLD, "CHLD"),
    (libc::SIGCONT, "CONT"),
    (libc::SIGSTOP, "STOP"),
    (libc::SIGTSTP, "TSTP"),
    (libc::SIGTTIN, "TTIN"),
    (libc::SIGTTOU, "TTOU"),
    (libc::SIGURG, "URG"),
    (libc::SIGXCPU, "XCPU"),
    (libc::SIGXFSZ, "XFSZ"),
    (libc::SIGVTALRM, "VTALRM"),
    (libc::SIGPROF, "PROF"),
    (libc::SIGWINCH, "WINCH"),
    (libc::SIGIO, "IO"),
    (libc::SIGPWR, "PWR"),
    (libc::SIGSYS, "SYS"),
];

/// Fallback for other Unix targets: the POSIX-required signals plus the
/// widely available XSI ones.
#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
)))]
pub const SIGNAL_TABLE: &[(i32, &str)] = &[
    (libc::SIGHUP, "HUP"),
    (libc::SIGINT, "INT"),
    (libc::SIGQUIT, "QUIT"),
    (libc::SIGILL, "ILL"),
    (libc::SIGTRAP, "TRAP"),
    (libc::SIGABRT, "ABRT"),
    (libc::SIGBUS, "BUS"),
    (libc::SIGFPE, "FPE"),
    (libc::SIGKILL, "KILL"),
    (libc::SIGUSR1, "USR1"),
    (libc::SIGSEGV, "SEGV"),
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
    (libc::SIGURG, "URG"),
    (libc::SIGXCPU, "XCPU"),
    (libc::SIGXFSZ, "XFSZ"),
    (libc::SIGVTALRM, "VTALRM"),
    (libc::SIGPROF, "PROF"),
    (libc::SIGWINCH, "WINCH"),
    (libc::SIGSYS, "SYS"),
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

/// The action `SIG_DFL` takes for a signal, which the shell has to
/// emulate when a delivery it queued on the self-pipe is processed after
/// the trap was reset (`trap 'trap - WINCH' WINCH; kill -WINCH $$ $$`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultAction {
    Terminate,
    /// Discarded: CHLD, URG, WINCH, CONT (and INFO on the BSDs/macOS).
    Ignore,
    /// Stops the process: TSTP, TTIN, TTOU (STOP cannot be caught).
    Stop,
}

/// See [`DefaultAction`].
pub fn default_action(sig: i32) -> DefaultAction {
    match sig {
        libc::SIGCHLD | libc::SIGURG | libc::SIGWINCH | libc::SIGCONT => DefaultAction::Ignore,
        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "dragonfly"
        ))]
        libc::SIGINFO => DefaultAction::Ignore,
        libc::SIGTSTP | libc::SIGTTIN | libc::SIGTTOU | libc::SIGSTOP => DefaultAction::Stop,
        _ => DefaultAction::Terminate,
    }
}

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

/// Every [`SIGNAL_TABLE`] signal whose disposition can be queried or
/// changed, in table order: SIGKILL/SIGSTOP are skipped since they
/// cannot be caught or ignored (sigaction would fail).
pub(super) fn catchable_signals() -> impl Iterator<Item = i32> {
    SIGNAL_TABLE
        .iter()
        .map(|&(num, _)| num)
        .filter(|&num| num != libc::SIGKILL && num != libc::SIGSTOP)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use nix::sys::signal::Signal;

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

    /// Every table entry must be a real signal that nix (used for every
    /// `sigaction` call, some of which `expect` success) can represent,
    /// with nix's canonical name matching ours — so a wrong libc constant
    /// or a typo'd name fails here instead of panicking in a fork child.
    #[test]
    fn test_signal_table_matches_nix_signals() {
        for &(num, name) in SIGNAL_TABLE {
            let sig = Signal::try_from(num)
                .unwrap_or_else(|_| panic!("SIGNAL_TABLE entry {name}={num} unknown to nix"));
            assert_eq!(sig.as_str(), format!("SIG{name}"), "entry {name}={num}");
        }
    }

    #[test]
    fn test_signal_table_names_and_numbers_unique_and_sorted() {
        let mut nums: Vec<i32> = SIGNAL_TABLE.iter().map(|&(n, _)| n).collect();
        let sorted = {
            let mut v = nums.clone();
            v.sort_unstable();
            v
        };
        assert_eq!(nums, sorted, "SIGNAL_TABLE must be in signal-number order");
        nums.dedup();
        assert_eq!(nums.len(), SIGNAL_TABLE.len(), "duplicate signal numbers");
        let names: HashSet<&str> = SIGNAL_TABLE.iter().map(|&(_, n)| n).collect();
        assert_eq!(names.len(), SIGNAL_TABLE.len(), "duplicate signal names");
    }

    #[test]
    fn test_signal_table_has_fault_and_xsi_signals() {
        for name in [
            "SEGV", "BUS", "FPE", "ILL", "TRAP", "SYS", "URG", "XCPU", "XFSZ", "VTALRM", "PROF",
            "WINCH",
        ] {
            assert!(
                signal_name_to_number(name).is_ok(),
                "{name} missing from SIGNAL_TABLE"
            );
        }
        assert_eq!(signal_number_to_name(libc::SIGSEGV), Some("SEGV"));
    }

    #[test]
    fn test_handled_signals_match_table() {
        for &(num, name) in HANDLED_SIGNALS {
            assert_eq!(signal_number_to_name(num), Some(name));
        }
    }
}
