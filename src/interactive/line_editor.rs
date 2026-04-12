use std::io::{self, Write, Stdout, stdout};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{self, ClearType},
    ExecutableCommand,
};

use super::fuzzy_search::FuzzySearchUI;
use super::history::History;

/// A minimal line-editing buffer used by the interactive REPL.
///
/// The buffer stores characters as a `Vec<char>` so that cursor
/// movement and insertion work correctly with multi-byte UTF-8
/// characters.
#[derive(Debug, Default)]
pub struct LineEditor {
    buf: Vec<char>,
    pos: usize,
}

impl LineEditor {
    /// Create an empty line editor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the current buffer contents as a `String`.
    pub fn buffer(&self) -> String {
        self.buf.iter().collect()
    }

    /// Return the current cursor position (0-based character index).
    #[allow(dead_code)] // public API for interactive mode enhancements
    pub fn cursor(&self) -> usize {
        self.pos
    }

    /// Return `true` if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Clear the buffer and reset the cursor to 0.
    pub fn clear(&mut self) {
        self.buf.clear();
        self.pos = 0;
    }

    /// Insert a character at the current cursor position and advance
    /// the cursor by one.
    pub fn insert_char(&mut self, ch: char) {
        self.buf.insert(self.pos, ch);
        self.pos += 1;
    }

    /// Delete the character immediately before the cursor (like the
    /// Backspace key).  Does nothing when the cursor is at position 0.
    pub fn backspace(&mut self) {
        if self.pos > 0 {
            self.pos -= 1;
            self.buf.remove(self.pos);
        }
    }

    /// Delete the character at the current cursor position (like the
    /// Delete key).  Does nothing when the cursor is at the end of
    /// the buffer.
    pub fn delete(&mut self) {
        if self.pos < self.buf.len() {
            self.buf.remove(self.pos);
        }
    }

    /// Move the cursor one position to the left.  Does nothing when
    /// the cursor is already at position 0.
    pub fn move_cursor_left(&mut self) {
        if self.pos > 0 {
            self.pos -= 1;
        }
    }

    /// Move the cursor one position to the right.  Does nothing when
    /// the cursor is already at the end of the buffer.
    pub fn move_cursor_right(&mut self) {
        if self.pos < self.buf.len() {
            self.pos += 1;
        }
    }

    /// Move the cursor to the beginning of the buffer (position 0).
    pub fn move_to_start(&mut self) {
        self.pos = 0;
    }

    /// Move the cursor to the end of the buffer.
    pub fn move_to_end(&mut self) {
        self.pos = self.buf.len();
    }
}

impl std::fmt::Display for LineEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.buffer())
    }
}

// ---------------------------------------------------------------------------
// Terminal I/O support (crossterm)
// ---------------------------------------------------------------------------

/// RAII guard that enables raw mode on creation and disables it on drop.
struct RawModeGuard;

impl RawModeGuard {
    fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

/// Result of processing a single key event.
enum KeyAction {
    Continue,
    Submit,
    Eof,
    Interrupt,
    FuzzySearch,
}

impl LineEditor {
    /// Read a line of input from the terminal, handling cursor movement and
    /// editing keys.  Returns `Ok(Some(line))` on Enter, `Ok(None)` on
    /// Ctrl-D with an empty buffer (EOF), or `Ok(Some(""))` on Ctrl-C.
    pub fn read_line(&mut self, prompt_width: usize, history: &mut History) -> io::Result<Option<String>> {
        self.clear();
        let mut _guard = RawModeGuard::new()?;
        let mut stdout = stdout();

        loop {
            stdout.flush()?;
            if let Event::Key(key_event) = event::read()? {
                match self.handle_key(key_event, history) {
                    KeyAction::Submit => {
                        history.reset_cursor();
                        stdout.execute(cursor::MoveToColumn(0))?;
                        write!(stdout, "\r\n")?;
                        stdout.flush()?;
                        return Ok(Some(self.buffer()));
                    }
                    KeyAction::Eof => {
                        return Ok(None);
                    }
                    KeyAction::Interrupt => {
                        history.reset_cursor();
                        stdout.execute(cursor::MoveToColumn(0))?;
                        write!(stdout, "\r\n")?;
                        stdout.flush()?;
                        self.clear();
                        return Ok(Some(String::new()));
                    }
                    KeyAction::FuzzySearch => {
                        drop(_guard);
                        match FuzzySearchUI::run(history) {
                            Ok(Some(line)) => {
                                self.buf = line.chars().collect();
                                self.pos = self.buf.len();
                            }
                            _ => {}
                        }
                        _guard = RawModeGuard::new()?;
                        self.redraw(&mut stdout, prompt_width)?;
                    }
                    KeyAction::Continue => {}
                }
                self.redraw(&mut stdout, prompt_width)?;
            }
        }
    }

    /// Redraw the current buffer on screen, positioning the cursor correctly.
    fn redraw(&self, stdout: &mut Stdout, prompt_width: usize) -> io::Result<()> {
        let col = |n: usize| -> u16 { n.min(u16::MAX as usize) as u16 };
        stdout.execute(cursor::MoveToColumn(col(prompt_width)))?;
        stdout.execute(terminal::Clear(ClearType::UntilNewLine))?;
        write!(stdout, "{}", self.buffer())?;
        stdout.execute(cursor::MoveToColumn(col(prompt_width + self.pos)))?;
        stdout.flush()?;
        Ok(())
    }

    /// Map a single key event to a [`KeyAction`], mutating the buffer as needed.
    fn handle_key(&mut self, key: KeyEvent, history: &mut History) -> KeyAction {
        match (key.code, key.modifiers) {
            // Ctrl+D — EOF when empty, otherwise delete char at cursor
            (KeyCode::Char('d'), m) if m.contains(KeyModifiers::CONTROL) => {
                if self.is_empty() {
                    KeyAction::Eof
                } else {
                    self.delete();
                    KeyAction::Continue
                }
            }

            // Ctrl+C — interrupt
            (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => KeyAction::Interrupt,

            // Ctrl+B / Left — move cursor left
            (KeyCode::Char('b'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.move_cursor_left();
                KeyAction::Continue
            }
            (KeyCode::Left, _) => {
                self.move_cursor_left();
                KeyAction::Continue
            }

            // Ctrl+F / Right — move cursor right
            (KeyCode::Char('f'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.move_cursor_right();
                KeyAction::Continue
            }
            (KeyCode::Right, _) => {
                self.move_cursor_right();
                KeyAction::Continue
            }

            // Ctrl+A / Home — move to start
            (KeyCode::Char('a'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.move_to_start();
                KeyAction::Continue
            }
            (KeyCode::Home, _) => {
                self.move_to_start();
                KeyAction::Continue
            }

            // Ctrl+E / End — move to end
            (KeyCode::Char('e'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.move_to_end();
                KeyAction::Continue
            }
            (KeyCode::End, _) => {
                self.move_to_end();
                KeyAction::Continue
            }

            // Enter — submit
            (KeyCode::Enter, _) => KeyAction::Submit,

            // Backspace — delete char before cursor
            (KeyCode::Backspace, _) => {
                self.backspace();
                KeyAction::Continue
            }

            // Delete — delete char at cursor
            (KeyCode::Delete, _) => {
                self.delete();
                KeyAction::Continue
            }

            // Printable character (without Ctrl modifier)
            (KeyCode::Char(ch), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.insert_char(ch);
                KeyAction::Continue
            }

            // Ctrl+R — fuzzy history search
            (KeyCode::Char('r'), m) if m.contains(KeyModifiers::CONTROL) => {
                KeyAction::FuzzySearch
            }

            // Up — navigate history backward
            (KeyCode::Up, _) => {
                if let Some(line) = history.navigate_up(&self.buffer()) {
                    self.buf = line.chars().collect();
                    self.pos = self.buf.len();
                }
                KeyAction::Continue
            }

            // Down — navigate history forward
            (KeyCode::Down, _) => {
                if let Some(line) = history.navigate_down() {
                    self.buf = line.chars().collect();
                    self.pos = self.buf.len();
                }
                KeyAction::Continue
            }

            // Everything else — ignore
            _ => KeyAction::Continue,
        }
    }
}
