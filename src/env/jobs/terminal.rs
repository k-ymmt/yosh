//! Terminal control for job-control mode.
//!
//! Wraps `tcsetpgrp(2)` for transferring terminal ownership between the
//! shell and foreground job process groups, and stores the shell's own
//! termios snapshot on `JobTable` so it can be restored after each
//! foreground wait completion.

use nix::unistd::Pid;
use std::os::fd::BorrowedFd;
use std::os::unix::io::RawFd;

const TERMINAL_FD: RawFd = 0;

/// Give the terminal to the specified process group.
pub fn give_terminal(pgid: Pid) -> Result<(), nix::Error> {
    // SAFETY: TERMINAL_FD (0) is stdin, which lives for the process lifetime.
    let fd = unsafe { BorrowedFd::borrow_raw(TERMINAL_FD) };
    nix::unistd::tcsetpgrp(fd, pgid)
}

/// Reclaim the terminal for the shell process group.
pub fn take_terminal(shell_pgid: Pid) -> Result<(), nix::Error> {
    // SAFETY: TERMINAL_FD (0) is stdin, which lives for the process lifetime.
    let fd = unsafe { BorrowedFd::borrow_raw(TERMINAL_FD) };
    nix::unistd::tcsetpgrp(fd, shell_pgid)
}

impl super::JobTable {
    /// Store the shell's termios snapshot. The interactive REPL calls
    /// this once at startup after `take_terminal`. Calling again
    /// overwrites the previous value; callers must not rely on this
    /// for re-initialization after fork.
    pub fn set_shell_tmodes(&mut self, t: nix::sys::termios::Termios) {
        self.shell_tmodes = Some(t);
    }

    /// Return the shell's snapshot of its termios, if one was captured.
    pub fn shell_tmodes(&self) -> Option<&nix::sys::termios::Termios> {
        self.shell_tmodes.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::jobs::JobTable;

    #[test]
    fn test_terminal_functions_compile() {
        // This test verifies the functions exist and have the correct
        // signatures.  We cannot actually call tcsetpgrp in a unit test
        // (no controlling terminal), so we just take function pointers.
        let _: fn(Pid) -> Result<(), nix::Error> = give_terminal;
        let _: fn(Pid) -> Result<(), nix::Error> = take_terminal;
    }

    #[test]
    fn test_job_table_shell_tmodes_defaults_none() {
        let table = JobTable::default();
        assert!(
            table.shell_tmodes().is_none(),
            "shell_tmodes should default to None on new JobTable"
        );
    }

    #[test]
    fn test_set_shell_tmodes_stores_value() {
        let mut table = JobTable::default();
        let zeroed: libc::termios = unsafe { std::mem::zeroed() };
        let t: nix::sys::termios::Termios = zeroed.into();
        table.set_shell_tmodes(t);
        assert!(
            table.shell_tmodes().is_some(),
            "shell_tmodes should hold the value after set_shell_tmodes"
        );
    }
}
