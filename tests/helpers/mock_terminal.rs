use std::collections::VecDeque;
use std::io;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kish::interactive::terminal::Terminal;

/// A mock terminal that replays a queue of events and records output.
pub struct MockTerminal {
    events: VecDeque<Event>,
    size: (u16, u16),
    output: Vec<String>,
}

impl MockTerminal {
    pub fn new(events: Vec<Event>) -> Self {
        Self {
            events: VecDeque::from(events),
            size: (80, 24),
            output: Vec::new(),
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

    fn move_up(&mut self, _n: u16) -> io::Result<()> {
        Ok(())
    }

    fn clear_current_line(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn clear_until_newline(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn write_str(&mut self, s: &str) -> io::Result<()> {
        self.output.push(s.to_string());
        Ok(())
    }

    fn set_reverse(&mut self, _on: bool) -> io::Result<()> {
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

/// Convert a string into a sequence of plain character key events.
pub fn chars(s: &str) -> Vec<Event> {
    s.chars().map(|c| key(KeyCode::Char(c))).collect()
}
