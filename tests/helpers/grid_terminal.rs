//! A screen-grid-emulating [`Terminal`] mock with xterm semantics.
//!
//! Unlike [`super::mock_terminal::MockTerminal`] — which records the output
//! stream but models no geometry — this mock maintains an actual cell grid
//! with auto-wrap, deferred wrap at the last column, cursor-movement
//! clamping at the screen edges, and scrolling at the bottom row. Tests can
//! therefore assert on the *final screen contents* a real terminal would
//! display, which catches the class of renderer bugs where the internal row
//! model disagrees with physical terminal behavior (wrapped prompts, scroll
//! misalignment, viewport drift) that stream-level assertions cannot see.
//!
//! Edge clamping is also *recorded*: a renderer that keeps every relative
//! movement inside the viewport never triggers it, so
//! [`GridTerminal::edge_violations`] doubles as a corruption detector.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::io;

use crossterm::event::Event;
use unicode_width::UnicodeWidthChar;
use yosh::interactive::terminal::Terminal;

pub struct GridTerminal {
    events: VecDeque<Event>,
    width: usize,
    height: usize,
    /// `height` rows × `width` cols. `'\0'` marks the continuation cell of
    /// a width-2 char.
    cells: Vec<Vec<char>>,
    row: usize,
    col: usize,
    /// Deferred-wrap state: a char was written into the last column; the
    /// next graphic char wraps first (xterm DECAWM), while CR / explicit
    /// cursor positioning clears the pending wrap.
    wrap_pending: bool,
    scrolls: usize,
    /// move_up hitting the top edge + move_down hitting the bottom edge.
    /// A viewport-correct renderer never clamps.
    edge_violations: usize,
    /// move_to_column targeting a column past the last one.
    col_overflows: usize,
}

impl GridTerminal {
    pub fn new(events: Vec<Event>, width: u16, height: u16) -> Self {
        let (w, h) = (width.max(1) as usize, height.max(1) as usize);
        Self {
            events: VecDeque::from(events),
            width: w,
            height: h,
            cells: vec![vec![' '; w]; h],
            row: 0,
            col: 0,
            wrap_pending: false,
            scrolls: 0,
            edge_violations: 0,
            col_overflows: 0,
        }
    }

    /// Screen contents as one string per row, trailing blanks trimmed.
    pub fn screen(&self) -> Vec<String> {
        self.cells
            .iter()
            .map(|row| {
                row.iter()
                    .filter(|&&c| c != '\0')
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    /// Cursor position as (row, col), with a pending wrap shown at the
    /// last column (where the terminal displays the cursor).
    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col.min(self.width - 1))
    }

    pub fn scrolls(&self) -> usize {
        self.scrolls
    }

    /// Times a cursor movement clamped at a screen edge. Non-zero means
    /// the renderer addressed rows outside the screen — the corruption
    /// trigger viewport clamping exists to prevent.
    pub fn edge_violations(&self) -> usize {
        self.edge_violations
    }

    /// Times move_to_column targeted a column past the last one.
    pub fn col_overflows(&self) -> usize {
        self.col_overflows
    }

    /// Advance one row, scrolling the grid when at the bottom.
    fn line_feed(&mut self) {
        if self.row + 1 == self.height {
            self.cells.remove(0);
            self.cells.push(vec![' '; self.width]);
            self.scrolls += 1;
        } else {
            self.row += 1;
        }
    }

    fn put(&mut self, ch: char) {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w == 0 {
            return; // zero-width: no grid effect modeled
        }
        if self.wrap_pending || self.col + w > self.width {
            self.col = 0;
            self.wrap_pending = false;
            self.line_feed();
        }
        self.cells[self.row][self.col] = ch;
        if w == 2 && self.col + 1 < self.width {
            self.cells[self.row][self.col + 1] = '\0';
        }
        self.col += w;
        if self.col >= self.width {
            self.col = self.width;
            self.wrap_pending = true;
        }
    }
}

impl Terminal for GridTerminal {
    fn read_event(&mut self) -> io::Result<Event> {
        self.events.pop_front().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "no more events in GridTerminal",
            )
        })
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        Ok((self.width as u16, self.height as u16))
    }

    fn enable_raw_mode(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn disable_raw_mode(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn move_to_column(&mut self, c: u16) -> io::Result<()> {
        let c = c as usize;
        if c >= self.width {
            self.col_overflows += 1;
        }
        self.col = c.min(self.width - 1);
        self.wrap_pending = false;
        Ok(())
    }

    fn move_up(&mut self, n: u16) -> io::Result<()> {
        let n = n as usize;
        if n > self.row {
            self.edge_violations += 1;
            self.row = 0;
        } else {
            self.row -= n;
        }
        self.wrap_pending = false;
        Ok(())
    }

    fn move_down(&mut self, n: u16) -> io::Result<()> {
        let n = n as usize;
        // CUD clamps at the bottom row; it never scrolls.
        if self.row + n >= self.height {
            if n > 0 {
                self.edge_violations += 1;
            }
            self.row = self.height - 1;
        } else {
            self.row += n;
        }
        self.wrap_pending = false;
        Ok(())
    }

    fn clear_current_line(&mut self) -> io::Result<()> {
        self.cells[self.row] = vec![' '; self.width];
        Ok(())
    }

    fn clear_until_newline(&mut self) -> io::Result<()> {
        let col = self.col.min(self.width);
        for c in self.cells[self.row][col..].iter_mut() {
            *c = ' ';
        }
        Ok(())
    }

    fn clear_all(&mut self) -> io::Result<()> {
        self.cells = vec![vec![' '; self.width]; self.height];
        self.row = 0;
        self.col = 0;
        self.wrap_pending = false;
        Ok(())
    }

    fn write_str(&mut self, s: &str) -> io::Result<()> {
        let mut chars = s.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\r' => {
                    self.col = 0;
                    self.wrap_pending = false;
                }
                '\n' => {
                    // Raw mode: LF is a pure line feed (no implicit CR).
                    self.wrap_pending = false;
                    self.line_feed();
                }
                // Skip ANSI escape sequences (styling in prompts); the
                // grid tracks characters only.
                '\x1b' => {
                    if chars.peek() == Some(&'[') {
                        chars.next();
                        for c in chars.by_ref() {
                            if (0x40..=0x7E).contains(&(c as u32)) {
                                break;
                            }
                        }
                    }
                }
                _ => self.put(ch),
            }
        }
        Ok(())
    }

    fn write_char(&mut self, ch: char) -> io::Result<()> {
        match ch {
            '\r' | '\n' | '\x1b' => self.write_str(&ch.to_string()),
            _ => {
                self.put(ch);
                Ok(())
            }
        }
    }

    fn set_reverse(&mut self, _on: bool) -> io::Result<()> {
        Ok(())
    }

    fn set_dim(&mut self, _on: bool) -> io::Result<()> {
        Ok(())
    }

    fn set_fg_color(&mut self, _color: crossterm::style::Color) -> io::Result<()> {
        Ok(())
    }

    fn set_bg_color(&mut self, _color: crossterm::style::Color) -> io::Result<()> {
        Ok(())
    }

    fn reset_style(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn set_bold(&mut self, _on: bool) -> io::Result<()> {
        Ok(())
    }

    fn set_underline(&mut self, _on: bool) -> io::Result<()> {
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
