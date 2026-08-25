//! Terminal control for job-control mode.
//!
//! Wraps `tcsetpgrp(2)` for transferring terminal ownership between the
//! shell and foreground job process groups, and stores the shell's own
//! termios snapshot on `JobTable` so it can be restored after each
//! foreground wait completion.

use nix::unistd::Pid;
use std::os::fd::BorrowedFd;
use std::os::unix::io::RawFd;
use std::sync::OnceLock;

/// Fd of the controlling terminal, resolved once per process.
///
/// `/dev/tty` names the controlling terminal regardless of stdio
/// redirections, so terminal handoffs and the monitor-mode ownership
/// gate target the same terminal even when stdin is `</dev/null`
/// (historically this was a hardcoded fd 0, which made a foreground
/// `yosh -m script </dev/null` hand the terminal to nobody and let the
/// child be stopped by SIGTTIN on its first tty read). Falls back to
/// fd 0 when `/dev/tty` cannot be opened — with no controlling
/// terminal, tcsetpgrp/tcgetpgrp fail exactly as before and every
/// call site already handles the error. O_CLOEXEC keeps the fd out of
/// exec'd programs; forked children inherit it for their pre-exec
/// `give_terminal` call.
pub(crate) fn terminal_fd() -> RawFd {
    static FD: OnceLock<RawFd> = OnceLock::new();
    *FD.get_or_init(|| {
        use std::os::fd::IntoRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_CLOEXEC)
            .open("/dev/tty")
        {
            Ok(f) => {
                // into_raw_fd: deliberately leaked — the controlling
                // terminal fd lives for the process lifetime. Move it
                // out of the fd range scripts realistically address: a
                // low fd would be clobbered by a script's
                // `exec 3>/dev/null`, silently breaking every later
                // terminal handoff. yosh accepts multi-digit
                // IO_NUMBERs (like bash), so no floor is unreachable —
                // 100 clears the single-digit POSIX range and the
                // self-pipe's 10/11 with margin (bash parks its own
                // internal fds high for the same reason).
                // F_DUPFD_CLOEXEC keeps the dup out of exec'd programs.
                let raw = f.into_raw_fd();
                // SAFETY: raw is a freshly opened, owned fd.
                let high = unsafe { libc::fcntl(raw, libc::F_DUPFD_CLOEXEC, 100) };
                if high >= 100 {
                    // SAFETY: raw is owned and no longer used after the dup.
                    unsafe { libc::close(raw) };
                    high
                } else {
                    // dup failed (fd table exhausted): keep the
                    // original, which still carries O_CLOEXEC.
                    raw
                }
            }
            Err(_) => 0,
        }
    })
}

/// Give the terminal to the specified process group.
pub fn give_terminal(pgid: Pid) -> Result<(), nix::Error> {
    // SAFETY: terminal_fd() is either the leaked /dev/tty fd or fd 0;
    // both live for the process lifetime.
    let fd = unsafe { BorrowedFd::borrow_raw(terminal_fd()) };
    nix::unistd::tcsetpgrp(fd, pgid)
}

/// Reclaim the terminal for the shell process group.
pub fn take_terminal(shell_pgid: Pid) -> Result<(), nix::Error> {
    // SAFETY: terminal_fd() is either the leaked /dev/tty fd or fd 0;
    // both live for the process lifetime.
    let fd = unsafe { BorrowedFd::borrow_raw(terminal_fd()) };
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
