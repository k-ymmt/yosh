//! Terminal state save/restore helpers for job control.
//!
//! Thin wrappers around `tcgetattr` / `tcsetattr` that no-op when stdin is
//! not a TTY. Callers gate with `is_interactive && monitor` before invoking
//! these helpers — the helpers themselves are unconditional on mode.

use nix::sys::termios::{SetArg, Termios, tcgetattr, tcsetattr};
use nix::unistd::isatty;
use std::os::fd::BorrowedFd;

const TTY_FD: std::os::unix::io::RawFd = 0;

/// Capture the controlling terminal's current termios.
///
/// Returns `Ok(None)` when stdin is not a TTY (pipes, redirected input,
/// CI environments). Returns `Err` only for unexpected I/O failures on a
/// real TTY.
pub fn capture_tty_termios() -> nix::Result<Option<Termios>> {
    // SAFETY: fd 0 lives for the process lifetime; borrowing is always valid.
    let fd = unsafe { BorrowedFd::borrow_raw(TTY_FD) };
    if !isatty(fd)? {
        return Ok(None);
    }
    tcgetattr(fd).map(Some)
}

/// Apply a saved termios to the controlling terminal.
///
/// No-op when stdin is not a TTY.
pub fn apply_tty_termios(tmodes: &Termios) -> nix::Result<()> {
    // SAFETY: fd 0 lives for the process lifetime; borrowing is always valid.
    let fd = unsafe { BorrowedFd::borrow_raw(TTY_FD) };
    if !isatty(fd)? {
        return Ok(());
    }
    tcsetattr(fd, SetArg::TCSANOW, tmodes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_tty_termios_returns_none_when_stdin_redirected() {
        let result = capture_tty_termios();
        assert!(matches!(result, Ok(None)),
            "expected Ok(None) when stdin is not a TTY, got {:?}", result);
    }

    #[test]
    fn apply_tty_termios_noop_when_non_tty() {
        let zeroed: libc::termios = unsafe { std::mem::zeroed() };
        let tmodes: nix::sys::termios::Termios = zeroed.into();
        let result = apply_tty_termios(&tmodes);
        assert!(result.is_ok(),
            "expected Ok(()) when stdin is not a TTY, got {:?}", result);
    }
}
