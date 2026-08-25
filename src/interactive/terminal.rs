use crossterm::{
    ExecutableCommand, cursor,
    event::{self, Event},
    style::{Attribute, Color, SetAttribute, SetBackgroundColor, SetForegroundColor},
    terminal::{self, ClearType},
};
use std::io::{self, Stdout, Write, stdout};

/// Text-cursor shape, used by the vi editing mode to distinguish
/// insert mode (bar) from command mode (block). Emitted as DECSCUSR
/// escapes; terminals without DECSCUSR support ignore them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorStyle {
    /// Terminal / user default (`ESC [ 0 SP q`).
    Default,
    /// Steady block (`ESC [ 2 SP q`) — vi command mode.
    Block,
    /// Steady bar (`ESC [ 6 SP q`) — vi insert mode.
    Bar,
}

/// Abstraction over terminal I/O for testability.
pub trait Terminal {
    /// Read one terminal event (blocking).
    fn read_event(&mut self) -> io::Result<Event>;

    /// Return terminal size (width, height).
    fn size(&self) -> io::Result<(u16, u16)>;

    /// Enable raw mode.
    fn enable_raw_mode(&mut self) -> io::Result<()>;

    /// Disable raw mode.
    fn disable_raw_mode(&mut self) -> io::Result<()>;

    /// Move cursor to the specified column.
    fn move_to_column(&mut self, col: u16) -> io::Result<()>;

    /// Move cursor up by N lines.
    fn move_up(&mut self, n: u16) -> io::Result<()>;

    /// Move cursor down by N lines.
    fn move_down(&mut self, n: u16) -> io::Result<()>;

    /// Clear the current line.
    fn clear_current_line(&mut self) -> io::Result<()>;

    /// Clear from cursor to end of line.
    fn clear_until_newline(&mut self) -> io::Result<()>;

    /// Clear the entire screen and move cursor to top-left.
    fn clear_all(&mut self) -> io::Result<()>;

    /// Write a string to the terminal.
    fn write_str(&mut self, s: &str) -> io::Result<()>;

    /// Set reverse video on/off.
    fn set_reverse(&mut self, on: bool) -> io::Result<()>;

    /// Set dim (faint) text attribute on/off.
    fn set_dim(&mut self, on: bool) -> io::Result<()>;

    /// Set foreground color.
    fn set_fg_color(&mut self, color: Color) -> io::Result<()>;

    /// Set background color.
    fn set_bg_color(&mut self, color: Color) -> io::Result<()>;

    /// Reset all text styling (color, bold, dim, underline, reverse).
    fn reset_style(&mut self) -> io::Result<()>;

    /// Set bold text attribute on/off.
    fn set_bold(&mut self, on: bool) -> io::Result<()>;

    /// Set underline text attribute on/off.
    fn set_underline(&mut self, on: bool) -> io::Result<()>;

    /// Write a single character to the terminal.
    fn write_char(&mut self, ch: char) -> io::Result<()>;

    /// Hide the text cursor.
    fn hide_cursor(&mut self) -> io::Result<()>;

    /// Show the text cursor.
    fn show_cursor(&mut self) -> io::Result<()>;

    /// Flush output.
    fn flush(&mut self) -> io::Result<()>;

    /// Set the text-cursor shape (DECSCUSR). Default implementation is
    /// a no-op so test/mock terminals need not care.
    fn set_cursor_style(&mut self, _style: CursorStyle) -> io::Result<()> {
        Ok(())
    }
}

/// Production terminal implementation backed by crossterm.
pub struct CrosstermTerminal {
    stdout: Stdout,
    /// Whether stdin was a tty at construction. When it is, crossterm
    /// reads events from fd 0 and [`read_event`](Terminal::read_event)
    /// can watch that fd directly for terminal hangup.
    stdin_is_tty: bool,
}

impl Default for CrosstermTerminal {
    fn default() -> Self {
        Self {
            stdout: stdout(),
            // SAFETY: isatty is always safe to call on a constant fd.
            stdin_is_tty: unsafe { libc::isatty(libc::STDIN_FILENO) } == 1,
        }
    }
}

/// Result of waiting for the stdin tty to become readable.
enum TtyWait {
    /// Input bytes are buffered and ready to read.
    Ready,
    /// Nothing happened within the timeout (or the wait was interrupted
    /// by a signal — the caller rechecks the pending-signal flag).
    Timeout,
    /// The terminal is gone: explicit `POLLHUP`/`POLLERR`, or the fd
    /// reports readable with zero buffered bytes (EOF after the pty
    /// master closed).
    Hangup,
}

/// Wait up to `timeout_ms` for stdin to become readable, distinguishing
/// real input from terminal hangup.
///
/// crossterm's own `event::poll` cannot make that distinction: after the
/// controlling pty's master side closes, the slave fd selects as ready
/// forever while `read` returns 0 bytes, so crossterm burns the whole
/// poll window in a hard `select`/`read` spin and reports "no event" —
/// the shell would busy-loop at 100% CPU instead of exiting.
fn wait_tty_readable(timeout_ms: libc::c_int) -> io::Result<TtyWait> {
    let mut pfd = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: pfd is a valid pollfd for the duration of the call.
    let rc = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
    if rc < 0 {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::Interrupted {
            return Ok(TtyWait::Timeout);
        }
        return Err(err);
    }
    if rc == 0 {
        return Ok(TtyWait::Timeout);
    }
    if pfd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
        return Ok(TtyWait::Hangup);
    }
    if pfd.revents & libc::POLLIN != 0 {
        let mut avail: libc::c_int = 0;
        // SAFETY: FIONREAD writes an int byte count for a valid fd.
        let rc = unsafe { libc::ioctl(libc::STDIN_FILENO, libc::FIONREAD, &mut avail) };
        if rc == 0 && avail == 0 {
            // "Readable" with nothing buffered is how macOS reports a
            // pty whose master is gone (no POLLHUP).
            return Ok(TtyWait::Hangup);
        }
    }
    Ok(TtyWait::Ready)
}

impl CrosstermTerminal {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Terminal for CrosstermTerminal {
    fn read_event(&mut self) -> io::Result<Event> {
        use std::time::Duration;
        // Poll in short bursts so we can check the signal self-pipe between
        // calls.  This lets SIGHUP (and other termination signals) interrupt
        // the read loop even though crossterm itself retries on EINTR.
        //
        // Terminal hangup must be detected BEFORE handing control to
        // crossterm: its internal tty read loop does not handle the EOF
        // read (`Ok(0)`) that a pty slave returns once the master side
        // closes, so any `event::poll` after that point — even with a
        // zero timeout — spins forever inside crossterm at 100% CPU.
        // (A dying terminal that emits input bytes in the same instant
        // can still slip through this pre-check; that race is
        // unavoidable while crossterm owns the fd.)
        loop {
            if self.stdin_is_tty
                && let TtyWait::Hangup = wait_tty_readable(0)?
            {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "terminal closed",
                ));
            }
            // Deliver whatever crossterm has parsed or can parse from
            // buffered bytes (also picks up SIGWINCH resize events).
            if event::poll(Duration::from_millis(0))? {
                return event::read();
            }
            // Check whether a pending signal should abort the read.
            if crate::signal::has_pending_exit_signal() {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "signal pending"));
            }
            if self.stdin_is_tty {
                // Block on the tty fd ourselves; Ready and Timeout both
                // loop back to the hangup pre-check and the zero-timeout
                // poll above, which consumes any new bytes.
                if let TtyWait::Hangup = wait_tty_readable(50)? {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "terminal closed",
                    ));
                }
            } else if event::poll(Duration::from_millis(50))? {
                // Non-tty stdin (crossterm reads /dev/tty instead):
                // keep the historical burst-poll behavior.
                return event::read();
            }
        }
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        terminal::size()
    }

    fn enable_raw_mode(&mut self) -> io::Result<()> {
        terminal::enable_raw_mode()
    }

    fn disable_raw_mode(&mut self) -> io::Result<()> {
        terminal::disable_raw_mode()
    }

    fn move_to_column(&mut self, col: u16) -> io::Result<()> {
        self.stdout.execute(cursor::MoveToColumn(col))?;
        Ok(())
    }

    fn move_up(&mut self, n: u16) -> io::Result<()> {
        // Guard like move_down: crossterm emits `ESC[0A` for MoveUp(0),
        // which most terminals treat as CUU 1 — a real one-row move.
        if n > 0 {
            self.stdout.execute(cursor::MoveUp(n))?;
        }
        Ok(())
    }

    fn move_down(&mut self, n: u16) -> io::Result<()> {
        if n > 0 {
            self.stdout.execute(cursor::MoveDown(n))?;
        }
        Ok(())
    }

    fn clear_current_line(&mut self) -> io::Result<()> {
        self.stdout
            .execute(terminal::Clear(ClearType::CurrentLine))?;
        Ok(())
    }

    fn clear_until_newline(&mut self) -> io::Result<()> {
        self.stdout
            .execute(terminal::Clear(ClearType::UntilNewLine))?;
        Ok(())
    }

    fn clear_all(&mut self) -> io::Result<()> {
        self.stdout.execute(terminal::Clear(ClearType::All))?;
        self.stdout.execute(cursor::MoveTo(0, 0))?;
        Ok(())
    }

    fn write_str(&mut self, s: &str) -> io::Result<()> {
        write!(self.stdout, "{}", s)?;
        Ok(())
    }

    fn set_reverse(&mut self, on: bool) -> io::Result<()> {
        if on {
            self.stdout.execute(SetAttribute(Attribute::Reverse))?;
        } else {
            self.stdout.execute(SetAttribute(Attribute::NoReverse))?;
        }
        Ok(())
    }

    fn set_dim(&mut self, on: bool) -> io::Result<()> {
        if on {
            self.stdout.execute(SetAttribute(Attribute::Dim))?;
        } else {
            self.stdout
                .execute(SetAttribute(Attribute::NormalIntensity))?;
        }
        Ok(())
    }

    fn set_fg_color(&mut self, color: Color) -> io::Result<()> {
        self.stdout.execute(SetForegroundColor(color))?;
        Ok(())
    }

    fn set_bg_color(&mut self, color: Color) -> io::Result<()> {
        self.stdout.execute(SetBackgroundColor(color))?;
        Ok(())
    }

    fn reset_style(&mut self) -> io::Result<()> {
        self.stdout.execute(SetAttribute(Attribute::Reset))?;
        Ok(())
    }

    fn set_bold(&mut self, on: bool) -> io::Result<()> {
        if on {
            self.stdout.execute(SetAttribute(Attribute::Bold))?;
        } else {
            self.stdout
                .execute(SetAttribute(Attribute::NormalIntensity))?;
        }
        Ok(())
    }

    fn set_underline(&mut self, on: bool) -> io::Result<()> {
        if on {
            self.stdout.execute(SetAttribute(Attribute::Underlined))?;
        } else {
            self.stdout.execute(SetAttribute(Attribute::NoUnderline))?;
        }
        Ok(())
    }

    fn write_char(&mut self, ch: char) -> io::Result<()> {
        write!(self.stdout, "{}", ch)?;
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.stdout.execute(cursor::Hide)?;
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.stdout.execute(cursor::Show)?;
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stdout.flush()
    }

    fn set_cursor_style(&mut self, style: CursorStyle) -> io::Result<()> {
        let seq = match style {
            CursorStyle::Default => "\x1b[0 q",
            CursorStyle::Block => "\x1b[2 q",
            CursorStyle::Bar => "\x1b[6 q",
        };
        write!(self.stdout, "{}", seq)?;
        Ok(())
    }
}
