use std::collections::VecDeque;
use std::io;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kish::interactive::terminal::Terminal;

/// A mock terminal that replays a queue of events and records output.
pub struct MockTerminal {
    events: VecDeque<Event>,
    size: (u16, u16),
    output: Vec<String>,
    /// Tracks vertical cursor movement. Each `\n` in write_str increments,
    /// each move_up(n) decrements by n.  Starts at 0.
    cursor_row: i32,
    dim: bool,
}

impl MockTerminal {
    pub fn new(events: Vec<Event>) -> Self {
        Self {
            events: VecDeque::from(events),
            size: (80, 24),
            output: Vec::new(),
            cursor_row: 0,
            dim: false,
        }
    }

    #[allow(dead_code)]
    pub fn output(&self) -> &[String] {
        &self.output
    }

    #[allow(dead_code)]
    pub fn set_size(&mut self, width: u16, height: u16) {
        self.size = (width, height);
    }

    /// Return the cumulative vertical cursor offset from the start position.
    /// A value of 0 means the cursor is back where it started.
    #[allow(dead_code)]
    pub fn cursor_row(&self) -> i32 {
        self.cursor_row
    }

    #[allow(dead_code)]
    pub fn dim(&self) -> bool {
        self.dim
    }
}

impl Terminal for MockTerminal {
    fn read_event(&mut self) -> io::Result<Event> {
        self.events.pop_front().ok_or_else(|| {
            io::Error::new(io::ErrorKind::UnexpectedEof, "no more events in MockTerminal")
        })
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        Ok(self.size)
    }

    fn enable_raw_mode(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn disable_raw_mode(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn move_to_column(&mut self, _col: u16) -> io::Result<()> {
        Ok(())
    }

    fn move_up(&mut self, n: u16) -> io::Result<()> {
        self.cursor_row -= n as i32;
        Ok(())
    }

    fn clear_current_line(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn clear_until_newline(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn write_str(&mut self, s: &str) -> io::Result<()> {
        self.cursor_row += s.chars().filter(|&c| c == '\n').count() as i32;
        self.output.push(s.to_string());
        Ok(())
    }

    fn set_reverse(&mut self, _on: bool) -> io::Result<()> {
        Ok(())
    }

    fn set_dim(&mut self, on: bool) -> io::Result<()> {
        self.dim = on;
        if on {
            self.output.push("[DIM]".to_string());
        } else {
            self.output.push("[/DIM]".to_string());
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// ── Event construction helpers ──────────────────────────────────────────

/// Create a plain key event (no modifiers).
pub fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::empty()))
}

/// Create a Ctrl+char key event.
pub fn ctrl(ch: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL))
}

/// Create an Alt+char key event.
pub fn alt(ch: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::ALT))
}

/// Convert a string into a sequence of plain character key events.
pub fn chars(s: &str) -> Vec<Event> {
    s.chars().map(|c| key(KeyCode::Char(c))).collect()
}
