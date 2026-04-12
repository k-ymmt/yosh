use std::io::{self, Stdout, Write, stdout};
use crossterm::{
    cursor,
    event::{self, Event},
    style::{Attribute, SetAttribute},
    terminal::{self, ClearType},
    ExecutableCommand,
};

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

    /// Clear the current line.
    fn clear_current_line(&mut self) -> io::Result<()>;

    /// Clear from cursor to end of line.
    fn clear_until_newline(&mut self) -> io::Result<()>;

    /// Write a string to the terminal.
    fn write_str(&mut self, s: &str) -> io::Result<()>;

    /// Set reverse video on/off.
    fn set_reverse(&mut self, on: bool) -> io::Result<()>;

    /// Set dim (faint) text attribute on/off.
    fn set_dim(&mut self, on: bool) -> io::Result<()>;

    /// Hide the text cursor.
    fn hide_cursor(&mut self) -> io::Result<()>;

    /// Show the text cursor.
    fn show_cursor(&mut self) -> io::Result<()>;

    /// Flush output.
    fn flush(&mut self) -> io::Result<()>;
}

/// Production terminal implementation backed by crossterm.
pub struct CrosstermTerminal {
    stdout: Stdout,
}

impl Default for CrosstermTerminal {
    fn default() -> Self {
        Self { stdout: stdout() }
    }
}

impl CrosstermTerminal {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Terminal for CrosstermTerminal {
    fn read_event(&mut self) -> io::Result<Event> {
        event::read()
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
        self.stdout.execute(cursor::MoveUp(n))?;
        Ok(())
    }

    fn clear_current_line(&mut self) -> io::Result<()> {
        self.stdout.execute(terminal::Clear(ClearType::CurrentLine))?;
        Ok(())
    }

    fn clear_until_newline(&mut self) -> io::Result<()> {
        self.stdout.execute(terminal::Clear(ClearType::UntilNewLine))?;
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
            self.stdout.execute(SetAttribute(Attribute::Reset))?;
        }
        Ok(())
    }

    fn set_dim(&mut self, on: bool) -> io::Result<()> {
        if on {
            self.stdout.execute(SetAttribute(Attribute::Dim))?;
        } else {
            self.stdout.execute(SetAttribute(Attribute::Reset))?;
        }
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
}
