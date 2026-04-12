# Interactive Mode Testing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable comprehensive testing of kish's interactive mode by introducing a Terminal trait abstraction, MockTerminal for unit tests, and PTY-based E2E tests.

**Architecture:** Two-layer testing strategy. Layer 1: `Terminal` trait with `MockTerminal` implementation for fast, deterministic tests of key sequences and buffer state. Layer 2: PTY-based E2E tests using `expectrl` to verify real terminal interaction, prompt rendering, and REPL lifecycle. The `LineEditor` and `FuzzySearchUI` are refactored to be generic over `<T: Terminal>`.

**Tech Stack:** Rust, crossterm 0.29 (existing), expectrl 0.8 (new dev-dependency)

---

## File Structure

```
src/interactive/
├── terminal.rs         # NEW: Terminal trait + CrosstermTerminal impl
├── line_editor.rs      # MODIFY: read_line/redraw generic over <T: Terminal>
├── fuzzy_search.rs     # MODIFY: run/draw/clear_ui generic over <T: Terminal>
├── mod.rs              # MODIFY: add terminal module, Repl holds CrosstermTerminal
├── history.rs          # unchanged
├── parse_status.rs     # unchanged
└── prompt.rs           # unchanged

tests/
├── helpers/
│   ├── mod.rs              # MODIFY: add mock_terminal module
│   └── mock_terminal.rs    # NEW: MockTerminal + event helper functions
├── interactive.rs          # MODIFY: add MockTerminal-based tests
├── pty_interactive.rs      # NEW: PTY E2E tests
└── ...                     # existing tests unchanged

Cargo.toml                  # MODIFY: add expectrl + crossterm dev-dependencies
```

---

### Task 1: Create Terminal Trait and CrosstermTerminal

**Files:**
- Create: `src/interactive/terminal.rs`
- Modify: `src/interactive/mod.rs:1`

- [ ] **Step 1: Create `src/interactive/terminal.rs`**

```rust
use std::io::{self, Stdout, Write, stdout};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
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

    /// Flush output.
    fn flush(&mut self) -> io::Result<()>;
}

/// Production terminal implementation backed by crossterm.
pub struct CrosstermTerminal {
    stdout: Stdout,
}

impl CrosstermTerminal {
    pub fn new() -> Self {
        Self { stdout: stdout() }
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

    fn flush(&mut self) -> io::Result<()> {
        self.stdout.flush()
    }
}
```

- [ ] **Step 2: Add module declaration to `src/interactive/mod.rs`**

Add `pub mod terminal;` after the existing module declarations. Insert at line 1:

```rust
pub mod terminal;
```

So the top of `mod.rs` becomes:

```rust
pub mod fuzzy_search;
pub mod history;
pub mod line_editor;
pub mod parse_status;
pub mod prompt;
pub mod terminal;
```

- [ ] **Step 3: Verify existing tests pass**

Run: `cargo test --test interactive 2>&1 | tail -5`

Expected: `test result: ok. 33 passed; 0 failed`

- [ ] **Step 4: Commit**

```bash
git add src/interactive/terminal.rs src/interactive/mod.rs
git commit -m "feat(interactive): add Terminal trait and CrosstermTerminal implementation

Introduce terminal I/O abstraction for testability. The Terminal trait
defines all operations used by LineEditor and FuzzySearchUI. CrosstermTerminal
provides the production implementation wrapping crossterm calls."
```

---

### Task 2: Refactor Interactive Module to Use Terminal Trait

**Files:**
- Modify: `src/interactive/fuzzy_search.rs:88-305`
- Modify: `src/interactive/line_editor.rs:1-303`
- Modify: `src/interactive/mod.rs:7-67`

- [ ] **Step 1: Refactor `src/interactive/fuzzy_search.rs`**

Replace the crossterm I/O imports and `FuzzySearchUI` methods (lines 88-305). Keep lines 1-87 (pure matching logic and tests) unchanged.

Replace lines 88-99 with:

```rust
use std::io;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use super::history::History;
use super::terminal::Terminal;
```

Replace the `FuzzySearchUI` impl block (lines 111-299) with:

```rust
impl FuzzySearchUI {
    pub fn run<T: Terminal>(history: &History, term: &mut T) -> io::Result<Option<String>> {
        let entries = history.entries();
        if entries.is_empty() {
            return Ok(None);
        }

        let (_, term_height) = term.size()?;
        let max_visible = ((term_height as f32) * 0.4).max(3.0) as usize;

        let mut ui = FuzzySearchUI {
            query: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            candidates: entries.iter().cloned().map(|e| (0, e)).collect(),
            max_visible,
        };
        ui.candidates.reverse(); // newest first

        let draw_lines = ui.max_visible + 2; // candidates + separator + query
        for _ in 0..draw_lines {
            term.write_str("\r\n")?;
        }
        term.move_up(draw_lines as u16)?;
        ui.draw(term)?;

        loop {
            term.flush()?;
            if let Event::Key(key_event) = term.read_event()? {
                match ui.handle_key(key_event, entries) {
                    SearchAction::Continue => {}
                    SearchAction::Select(line) => {
                        ui.clear_ui(term, draw_lines)?;
                        return Ok(Some(line));
                    }
                    SearchAction::Cancel => {
                        ui.clear_ui(term, draw_lines)?;
                        return Ok(None);
                    }
                }
                ui.draw(term)?;
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent, entries: &[String]) -> SearchAction {
        match (key.code, key.modifiers) {
            (KeyCode::Enter, _) => {
                if let Some((_score, line)) = self.candidates.get(self.selected) {
                    SearchAction::Select(line.clone())
                } else {
                    SearchAction::Cancel
                }
            }
            (KeyCode::Esc, _) => SearchAction::Cancel,
            (KeyCode::Char('g'), m) if m.contains(KeyModifiers::CONTROL) => SearchAction::Cancel,
            (KeyCode::Up, _) => {
                if self.selected + 1 < self.candidates.len() {
                    self.selected += 1;
                    self.adjust_scroll();
                }
                SearchAction::Continue
            }
            (KeyCode::Char('p'), m) if m.contains(KeyModifiers::CONTROL) => {
                if self.selected + 1 < self.candidates.len() {
                    self.selected += 1;
                    self.adjust_scroll();
                }
                SearchAction::Continue
            }
            (KeyCode::Char('r'), m) if m.contains(KeyModifiers::CONTROL) => {
                if self.selected + 1 < self.candidates.len() {
                    self.selected += 1;
                    self.adjust_scroll();
                }
                SearchAction::Continue
            }
            (KeyCode::Down, _) => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.adjust_scroll();
                }
                SearchAction::Continue
            }
            (KeyCode::Char('n'), m) if m.contains(KeyModifiers::CONTROL) => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.adjust_scroll();
                }
                SearchAction::Continue
            }
            (KeyCode::Backspace, _) => {
                if !self.query.is_empty() {
                    self.query.pop();
                    self.update_candidates(entries);
                }
                SearchAction::Continue
            }
            (KeyCode::Char(ch), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.query.push(ch);
                self.update_candidates(entries);
                SearchAction::Continue
            }
            _ => SearchAction::Continue,
        }
    }

    fn update_candidates(&mut self, entries: &[String]) {
        let query: String = self.query.iter().collect();
        if query.is_empty() {
            self.candidates = entries.iter().cloned().map(|e| (0, e)).collect();
            self.candidates.reverse();
        } else {
            self.candidates = filter_and_sort(&query, entries);
        }
        self.selected = 0;
        self.scroll_offset = 0;
    }

    fn adjust_scroll(&mut self) {
        if self.selected >= self.scroll_offset + self.max_visible {
            self.scroll_offset = self.selected - self.max_visible + 1;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
    }

    fn draw<T: Terminal>(&self, term: &mut T) -> io::Result<()> {
        let (term_width, _) = term.size()?;
        let width = term_width as usize;

        term.move_to_column(0)?;

        let visible_end = (self.scroll_offset + self.max_visible).min(self.candidates.len());
        let visible_range = self.scroll_offset..visible_end;
        let visible_count = visible_range.len();

        // Fill empty lines if fewer candidates than max_visible
        for _ in 0..(self.max_visible - visible_count) {
            term.clear_current_line()?;
            term.write_str("\r\n")?;
        }

        // Draw candidates in reverse order (highest index = top of UI)
        for i in (visible_range).rev() {
            term.clear_current_line()?;
            let (_score, ref line) = self.candidates[i];
            let display: String = line.chars().take(width.saturating_sub(2)).collect();
            if i == self.selected {
                term.set_reverse(true)?;
                term.write_str(&format!("> {}", display))?;
                term.set_reverse(false)?;
            } else {
                term.write_str(&format!("  {}", display))?;
            }
            term.write_str("\r\n")?;
        }

        // Separator
        term.clear_current_line()?;
        let sep: String = "\u{2500}".repeat(width.min(40));
        term.write_str(&format!("  {}\r\n", sep))?;

        // Query line
        term.clear_current_line()?;
        let query_str: String = self.query.iter().collect();
        let total = self.candidates.len();
        term.write_str(&format!("  {}/{} > {}", total, total, query_str))?;

        // Move back to top
        let total_lines = self.max_visible + 2;
        term.move_up(total_lines as u16)?;
        term.flush()?;
        Ok(())
    }

    fn clear_ui<T: Terminal>(&self, term: &mut T, draw_lines: usize) -> io::Result<()> {
        term.move_to_column(0)?;
        for _ in 0..draw_lines {
            term.clear_current_line()?;
            term.write_str("\r\n")?;
        }
        term.move_up(draw_lines as u16)?;
        term.flush()?;
        Ok(())
    }
}
```

- [ ] **Step 2: Refactor `src/interactive/line_editor.rs`**

Replace the entire file. Buffer manipulation methods (lines 17-107) stay the same. Terminal I/O section changes completely.

Replace lines 1-11 (imports) with:

```rust
use std::io;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use super::fuzzy_search::FuzzySearchUI;
use super::history::History;
use super::terminal::Terminal;
```

Remove the `RawModeGuard` struct and its impls (lines 113-127).

Replace the terminal I/O `impl LineEditor` block (lines 138-303) with:

```rust
impl LineEditor {
    /// Read a line of input using the terminal, handling cursor movement and
    /// editing keys. Returns `Ok(Some(line))` on Enter, `Ok(None)` on
    /// Ctrl-D with an empty buffer (EOF), or `Ok(Some(""))` on Ctrl-C.
    pub fn read_line<T: Terminal>(
        &mut self,
        prompt_width: usize,
        history: &mut History,
        term: &mut T,
    ) -> io::Result<Option<String>> {
        self.clear();
        term.enable_raw_mode()?;
        let result = self.read_line_loop(prompt_width, history, term);
        let _ = term.disable_raw_mode();
        result
    }

    fn read_line_loop<T: Terminal>(
        &mut self,
        prompt_width: usize,
        history: &mut History,
        term: &mut T,
    ) -> io::Result<Option<String>> {
        loop {
            term.flush()?;
            if let Event::Key(key_event) = term.read_event()? {
                match self.handle_key(key_event, history) {
                    KeyAction::Submit => {
                        history.reset_cursor();
                        term.move_to_column(0)?;
                        term.write_str("\r\n")?;
                        term.flush()?;
                        return Ok(Some(self.buffer()));
                    }
                    KeyAction::Eof => {
                        return Ok(None);
                    }
                    KeyAction::Interrupt => {
                        history.reset_cursor();
                        term.move_to_column(0)?;
                        term.write_str("\r\n")?;
                        term.flush()?;
                        self.clear();
                        return Ok(Some(String::new()));
                    }
                    KeyAction::FuzzySearch => {
                        term.disable_raw_mode()?;
                        match FuzzySearchUI::run(history, term) {
                            Ok(Some(line)) => {
                                self.buf = line.chars().collect();
                                self.pos = self.buf.len();
                            }
                            _ => {}
                        }
                        term.enable_raw_mode()?;
                        self.redraw(term, prompt_width)?;
                    }
                    KeyAction::Continue => {}
                }
                self.redraw(term, prompt_width)?;
            }
        }
    }

    /// Redraw the current buffer on screen, positioning the cursor correctly.
    fn redraw<T: Terminal>(&self, term: &mut T, prompt_width: usize) -> io::Result<()> {
        let col = |n: usize| -> u16 { n.min(u16::MAX as usize) as u16 };
        term.move_to_column(col(prompt_width))?;
        term.clear_until_newline()?;
        term.write_str(&self.buffer())?;
        term.move_to_column(col(prompt_width + self.pos))?;
        term.flush()?;
        Ok(())
    }

    /// Map a single key event to a [`KeyAction`], mutating the buffer as needed.
    fn handle_key(&mut self, key: KeyEvent, history: &mut History) -> KeyAction {
        match (key.code, key.modifiers) {
            // Ctrl+D -- EOF when empty, otherwise delete char at cursor
            (KeyCode::Char('d'), m) if m.contains(KeyModifiers::CONTROL) => {
                if self.is_empty() {
                    KeyAction::Eof
                } else {
                    self.delete();
                    KeyAction::Continue
                }
            }

            // Ctrl+C -- interrupt
            (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => KeyAction::Interrupt,

            // Ctrl+B / Left -- move cursor left
            (KeyCode::Char('b'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.move_cursor_left();
                KeyAction::Continue
            }
            (KeyCode::Left, _) => {
                self.move_cursor_left();
                KeyAction::Continue
            }

            // Ctrl+F / Right -- move cursor right
            (KeyCode::Char('f'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.move_cursor_right();
                KeyAction::Continue
            }
            (KeyCode::Right, _) => {
                self.move_cursor_right();
                KeyAction::Continue
            }

            // Ctrl+A / Home -- move to start
            (KeyCode::Char('a'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.move_to_start();
                KeyAction::Continue
            }
            (KeyCode::Home, _) => {
                self.move_to_start();
                KeyAction::Continue
            }

            // Ctrl+E / End -- move to end
            (KeyCode::Char('e'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.move_to_end();
                KeyAction::Continue
            }
            (KeyCode::End, _) => {
                self.move_to_end();
                KeyAction::Continue
            }

            // Enter -- submit
            (KeyCode::Enter, _) => KeyAction::Submit,

            // Backspace -- delete char before cursor
            (KeyCode::Backspace, _) => {
                self.backspace();
                KeyAction::Continue
            }

            // Delete -- delete char at cursor
            (KeyCode::Delete, _) => {
                self.delete();
                KeyAction::Continue
            }

            // Printable character (without Ctrl modifier)
            (KeyCode::Char(ch), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.insert_char(ch);
                KeyAction::Continue
            }

            // Ctrl+R -- fuzzy history search
            (KeyCode::Char('r'), m) if m.contains(KeyModifiers::CONTROL) => {
                KeyAction::FuzzySearch
            }

            // Up -- navigate history backward
            (KeyCode::Up, _) => {
                if let Some(line) = history.navigate_up(&self.buffer()) {
                    self.buf = line.chars().collect();
                    self.pos = self.buf.len();
                }
                KeyAction::Continue
            }

            // Down -- navigate history forward
            (KeyCode::Down, _) => {
                if let Some(line) = history.navigate_down() {
                    self.buf = line.chars().collect();
                    self.pos = self.buf.len();
                }
                KeyAction::Continue
            }

            // Everything else -- ignore
            _ => KeyAction::Continue,
        }
    }
}
```

- [ ] **Step 3: Update `src/interactive/mod.rs`**

Add import and field for `CrosstermTerminal`. Replace line 9 `use crate::signal;` area to add the import:

```rust
use terminal::CrosstermTerminal;
```

Add `terminal` field to the `Repl` struct:

```rust
pub struct Repl {
    executor: Executor,
    line_editor: LineEditor,
    terminal: CrosstermTerminal,
}
```

Update `Repl::new` return value to include the terminal:

```rust
        Self {
            executor,
            line_editor: LineEditor::new(),
            terminal: CrosstermTerminal::new(),
        }
```

Update the `read_line` call in `Repl::run` (around line 67) to pass the terminal:

```rust
            let line = match self.line_editor.read_line(prompt_width, &mut self.executor.env.history, &mut self.terminal) {
```

- [ ] **Step 4: Verify existing tests pass**

Run: `cargo test --test interactive 2>&1 | tail -5`

Expected: `test result: ok. 33 passed; 0 failed`

Run: `cargo test 2>&1 | tail -5`

Expected: all tests pass (the refactoring changes no behavior)

- [ ] **Step 5: Commit**

```bash
git add src/interactive/line_editor.rs src/interactive/fuzzy_search.rs src/interactive/mod.rs
git commit -m "refactor(interactive): use Terminal trait in LineEditor and FuzzySearchUI

Refactor read_line, redraw, FuzzySearchUI::run, draw, and clear_ui to be
generic over <T: Terminal>. Remove RawModeGuard in favor of explicit
enable/disable_raw_mode calls through the trait. Repl now holds a
CrosstermTerminal instance. No behavioral changes."
```

---

### Task 3: Create MockTerminal Test Helper

**Files:**
- Create: `tests/helpers/mock_terminal.rs`
- Modify: `tests/helpers/mod.rs:1`

- [ ] **Step 1: Create `tests/helpers/mock_terminal.rs`**

```rust
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
```

- [ ] **Step 2: Add module to `tests/helpers/mod.rs`**

Add at the top of the file:

```rust
pub mod mock_terminal;
```

- [ ] **Step 3: Verify build**

Run: `cargo test --test interactive --no-run 2>&1 | tail -3`

Expected: compiles without error

- [ ] **Step 4: Commit**

```bash
git add tests/helpers/mock_terminal.rs tests/helpers/mod.rs
git commit -m "test: add MockTerminal and event helpers for interactive testing

MockTerminal replays a queue of crossterm Events and records write_str
output. Helper functions key(), ctrl(), and chars() simplify event
construction in tests."
```

---

### Task 4: Layer 1 Tests - LineEditor Key Sequences

**Files:**
- Modify: `tests/interactive.rs:1-311`

- [ ] **Step 1: Add MockTerminal-based tests to `tests/interactive.rs`**

Add these imports at the top of the file (after existing imports):

```rust
use crossterm::event::KeyCode;
use kish::interactive::history::History;

mod helpers;
use helpers::mock_terminal::{MockTerminal, key, ctrl, chars};
```

Add the following tests at the end of the file:

```rust
// ── MockTerminal-based LineEditor tests ─────────────────────────────────

#[test]
fn test_mock_basic_input() {
    let mut events = chars("hello");
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("hello".to_string()));
}

#[test]
fn test_mock_ctrl_c_returns_empty() {
    let events = vec![
        key(KeyCode::Char('a')),
        key(KeyCode::Char('b')),
        ctrl('c'),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some(String::new()));
}

#[test]
fn test_mock_ctrl_d_empty_returns_none() {
    let events = vec![ctrl('d')];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, None);
}

#[test]
fn test_mock_ctrl_d_nonempty_deletes_char() {
    // Type "ab", move left, Ctrl+D deletes 'b', Enter submits "a"
    let events = vec![
        key(KeyCode::Char('a')),
        key(KeyCode::Char('b')),
        key(KeyCode::Left),
        ctrl('d'),
        key(KeyCode::Enter),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("a".to_string()));
}

#[test]
fn test_mock_ctrl_a_and_ctrl_e() {
    // Type "abc", Ctrl+A (start), type "x", Ctrl+E (end), type "y"
    let events = vec![
        key(KeyCode::Char('a')),
        key(KeyCode::Char('b')),
        key(KeyCode::Char('c')),
        ctrl('a'),
        key(KeyCode::Char('x')),
        ctrl('e'),
        key(KeyCode::Char('y')),
        key(KeyCode::Enter),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("xabcy".to_string()));
}

#[test]
fn test_mock_ctrl_b_and_ctrl_f() {
    // Type "abc", Ctrl+B twice (back to pos 1), type "x", Ctrl+F (forward), type "y"
    let events = vec![
        key(KeyCode::Char('a')),
        key(KeyCode::Char('b')),
        key(KeyCode::Char('c')),
        ctrl('b'),
        ctrl('b'),
        key(KeyCode::Char('x')),
        ctrl('f'),
        key(KeyCode::Char('y')),
        key(KeyCode::Enter),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("axbyc".to_string()));
}

#[test]
fn test_mock_home_end_keys() {
    // Type "abc", Home, type "x", End, type "y"
    let events = vec![
        key(KeyCode::Char('a')),
        key(KeyCode::Char('b')),
        key(KeyCode::Char('c')),
        key(KeyCode::Home),
        key(KeyCode::Char('x')),
        key(KeyCode::End),
        key(KeyCode::Char('y')),
        key(KeyCode::Enter),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("xabcy".to_string()));
}

#[test]
fn test_mock_backspace() {
    // Type "abc", Backspace twice, Enter
    let events = vec![
        key(KeyCode::Char('a')),
        key(KeyCode::Char('b')),
        key(KeyCode::Char('c')),
        key(KeyCode::Backspace),
        key(KeyCode::Backspace),
        key(KeyCode::Enter),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("a".to_string()));
}

#[test]
fn test_mock_delete_key() {
    // Type "abc", Home, Delete, Enter -> "bc"
    let events = vec![
        key(KeyCode::Char('a')),
        key(KeyCode::Char('b')),
        key(KeyCode::Char('c')),
        key(KeyCode::Home),
        key(KeyCode::Delete),
        key(KeyCode::Enter),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("bc".to_string()));
}

#[test]
fn test_mock_history_up_down() {
    let mut history = History::new();
    history.add("first", 500, "");
    history.add("second", 500, "");

    // Up (second), Up (first), Down (second), Enter
    let events = vec![
        key(KeyCode::Up),
        key(KeyCode::Up),
        key(KeyCode::Down),
        key(KeyCode::Enter),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("second".to_string()));
}

#[test]
fn test_mock_history_up_and_edit() {
    let mut history = History::new();
    history.add("echo old", 500, "");

    // Up (recall "echo old"), Backspace x3 (remove "old"), type "new", Enter
    let mut events = vec![key(KeyCode::Up)];
    events.extend(vec![key(KeyCode::Backspace); 3]);
    events.extend(chars("new"));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("echo new".to_string()));
}

#[test]
fn test_mock_history_preserves_typed_text() {
    let mut history = History::new();
    history.add("old", 500, "");

    // Type "partial", Up (recall "old"), Down (back to "partial"), Enter
    let mut events = chars("partial");
    events.push(key(KeyCode::Up));
    events.push(key(KeyCode::Down));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("partial".to_string()));
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --test interactive 2>&1 | tail -5`

Expected: all tests pass (33 existing + 12 new = 45 total)

- [ ] **Step 3: Commit**

```bash
git add tests/interactive.rs
git commit -m "test(interactive): add MockTerminal-based LineEditor tests

Test key sequences through read_line with MockTerminal: basic input,
Ctrl+C/D, cursor movement (Ctrl+A/B/E/F, Home/End, arrows), backspace,
delete, history navigation, and history editing."
```

---

### Task 5: Layer 1 Tests - Ctrl+R Fuzzy Search

**Files:**
- Modify: `tests/interactive.rs`

- [ ] **Step 1: Add Ctrl+R tests to `tests/interactive.rs`**

Add these imports if not already present (check existing imports at top of file):

```rust
use kish::interactive::fuzzy_search::FuzzySearchUI;
```

Add the following tests at the end of the file:

```rust
// ── Ctrl+R fuzzy search tests ───────────────────────────────────────────

#[test]
fn test_mock_ctrl_r_selects_matching_entry() {
    let mut history = History::new();
    history.add("ls -la", 500, "");
    history.add("git commit -m 'fix'", 500, "");
    history.add("cargo test", 500, "");

    // Ctrl+R -> type "git" -> Enter (select) -> Enter (submit)
    let mut events = vec![ctrl('r')];
    events.extend(chars("git"));
    events.push(key(KeyCode::Enter)); // select from search
    events.push(key(KeyCode::Enter)); // submit in line editor

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("git commit -m 'fix'".to_string()));
}

#[test]
fn test_mock_ctrl_r_cancel_with_esc() {
    let mut history = History::new();
    history.add("ls -la", 500, "");
    history.add("git commit", 500, "");

    // Type "hello", Ctrl+R -> type "git" -> Esc (cancel) -> Enter (submit "hello")
    let mut events = chars("hello");
    events.push(ctrl('r'));
    events.extend(chars("git"));
    events.push(key(KeyCode::Esc)); // cancel search
    events.push(key(KeyCode::Enter)); // submit whatever is in buffer

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    // After Esc, buffer should retain pre-search content "hello"
    assert_eq!(result, Some("hello".to_string()));
}

#[test]
fn test_mock_ctrl_r_navigate_up() {
    let mut history = History::new();
    history.add("echo first", 500, "");
    history.add("echo second", 500, "");
    history.add("echo third", 500, "");

    // Ctrl+R (no query, all entries shown, newest first: third=0, second=1, first=2)
    // Up moves selection from index 0 to 1 (second)
    // Enter selects "echo second"
    let events = vec![
        ctrl('r'),
        key(KeyCode::Up),     // select "echo second" (index 1)
        key(KeyCode::Enter),  // select from search
        key(KeyCode::Enter),  // submit
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("echo second".to_string()));
}

#[test]
fn test_mock_ctrl_r_backspace_updates_candidates() {
    let mut history = History::new();
    history.add("git log", 500, "");
    history.add("cargo test", 500, "");

    // Ctrl+R -> type "gi" -> Backspace -> type "ca" -> Enter (selects "cargo test")
    let events = vec![
        ctrl('r'),
        key(KeyCode::Char('g')),
        key(KeyCode::Char('i')),
        key(KeyCode::Backspace),
        key(KeyCode::Char('c')),
        key(KeyCode::Char('a')),
        key(KeyCode::Enter),  // select from search
        key(KeyCode::Enter),  // submit
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("cargo test".to_string()));
}

#[test]
fn test_mock_fuzzy_search_direct_select() {
    // Test FuzzySearchUI::run directly (not through LineEditor)
    let mut history = History::new();
    history.add("ls -la", 500, "");
    history.add("git status", 500, "");
    history.add("cargo build", 500, "");

    // Type "sta" -> Enter (selects "git status" as best match)
    let mut events = chars("sta");
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let result = FuzzySearchUI::run(&history, &mut term).unwrap();
    assert_eq!(result, Some("git status".to_string()));
}

#[test]
fn test_mock_fuzzy_search_direct_cancel() {
    let mut history = History::new();
    history.add("ls -la", 500, "");

    let events = vec![key(KeyCode::Esc)];

    let mut term = MockTerminal::new(events);
    let result = FuzzySearchUI::run(&history, &mut term).unwrap();
    assert_eq!(result, None);
}

#[test]
fn test_mock_fuzzy_search_empty_history() {
    let history = History::new();
    let mut term = MockTerminal::new(vec![]);
    let result = FuzzySearchUI::run(&history, &mut term).unwrap();
    assert_eq!(result, None);
}

#[test]
fn test_mock_ctrl_r_with_ctrl_g_cancel() {
    let mut history = History::new();
    history.add("some command", 500, "");

    // Ctrl+R -> Ctrl+G (cancel) -> Enter (submit empty)
    let events = vec![
        ctrl('r'),
        ctrl('g'),            // cancel search
        key(KeyCode::Enter),  // submit
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    // Buffer is empty since Ctrl+R was triggered from empty state and cancelled
    assert_eq!(result, Some(String::new()));
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --test interactive 2>&1 | tail -5`

Expected: all tests pass (45 existing + 8 new = 53 total)

- [ ] **Step 3: Commit**

```bash
git add tests/interactive.rs
git commit -m "test(interactive): add Ctrl+R fuzzy search tests with MockTerminal

Test Ctrl+R integration through LineEditor and FuzzySearchUI directly:
selection, cancellation (Esc and Ctrl+G), navigation, backspace query
editing, empty history handling."
```

---

### Task 6: PTY E2E Tests

**Files:**
- Modify: `Cargo.toml:11`
- Create: `tests/pty_interactive.rs`

- [ ] **Step 1: Add `expectrl` to dev-dependencies in `Cargo.toml`**

Add after the existing dev-dependencies:

```toml
crossterm = "0.29"
expectrl = "0.8"
```

So `[dev-dependencies]` becomes:

```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
tempfile = "3"
crossterm = "0.29"
expectrl = "0.8"
```

`crossterm` in dev-dependencies makes crossterm types directly available in integration tests (`tests/` files).

- [ ] **Step 2: Verify dependency resolves**

Run: `cargo check --tests 2>&1 | tail -5`

Expected: compiles without error (may need to download expectrl)

- [ ] **Step 3: Create `tests/pty_interactive.rs`**

```rust
use std::process::Command;
use std::time::Duration;

use expectrl::Session;

mod helpers;

const TIMEOUT: Duration = Duration::from_secs(5);

// ── Helpers ─────────────────────────────────────────────────────────────

/// Returns (session, tmpdir). The tmpdir must be kept alive for the
/// duration of the test so that kish's HOME directory is not deleted.
fn spawn_kish() -> (Session, helpers::TempDir) {
    let bin = env!("CARGO_BIN_EXE_kish");
    let tmpdir = helpers::TempDir::new();

    let mut cmd = Command::new(bin);
    cmd.env("TERM", "dumb");
    cmd.env("HOME", tmpdir.path());

    let mut session = Session::spawn(cmd).expect("failed to spawn kish");
    session.set_expect_timeout(Some(TIMEOUT));
    (session, tmpdir)
}

fn wait_for_prompt(session: &mut Session) {
    session.expect("$ ").expect("prompt not found");
}

// ── Tests ───────────────────────────────────────────────────────────────

#[test]
fn test_pty_echo_command() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    s.send("echo hello\r").unwrap();
    s.expect("hello").expect("echo output not found");
    wait_for_prompt(&mut s);

    // Exit with Ctrl+D
    s.send("\x04").unwrap();
}

#[test]
fn test_pty_ctrl_d_exits() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    s.send("\x04").unwrap();
    s.expect(expectrl::Eof).expect("shell did not exit on Ctrl+D");
}

#[test]
fn test_pty_ctrl_c_interrupts_input() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    // Type something, then Ctrl+C
    s.send("partial input").unwrap();
    s.send("\x03").unwrap();

    // Should get a new prompt
    wait_for_prompt(&mut s);

    // Can still run commands
    s.send("echo ok\r").unwrap();
    s.expect("ok").expect("command after Ctrl+C failed");
    wait_for_prompt(&mut s);

    s.send("\x04").unwrap();
}

#[test]
fn test_pty_history_up_re_executes() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    s.send("echo first_cmd\r").unwrap();
    s.expect("first_cmd").expect("first command output not found");
    wait_for_prompt(&mut s);

    // Press Up then Enter to re-execute
    s.send("\x1b[A").unwrap(); // Up arrow (ANSI escape)
    s.send("\r").unwrap();
    s.expect("first_cmd").expect("history re-execution failed");
    wait_for_prompt(&mut s);

    s.send("\x04").unwrap();
}

#[test]
fn test_pty_backspace_editing() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    // Type "echoo", backspace, " works"
    s.send("echoo").unwrap();
    s.send("\x7f").unwrap(); // Backspace
    s.send(" works\r").unwrap();
    s.expect("works").expect("line editing with backspace failed");
    wait_for_prompt(&mut s);

    s.send("\x04").unwrap();
}

#[test]
fn test_pty_ps2_continuation() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    // Incomplete command: if true; then
    s.send("if true; then\r").unwrap();
    s.expect("> ").expect("PS2 prompt not shown");

    // Complete the command
    s.send("echo continued\r").unwrap();
    s.expect("> ").expect("PS2 prompt not shown after body");

    s.send("fi\r").unwrap();
    s.expect("continued").expect("if-then-fi output not found");
    wait_for_prompt(&mut s);

    s.send("\x04").unwrap();
}

#[test]
fn test_pty_ctrl_r_history_search() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    // Build up history
    s.send("echo alpha\r").unwrap();
    s.expect("alpha").unwrap();
    wait_for_prompt(&mut s);

    s.send("echo beta\r").unwrap();
    s.expect("beta").unwrap();
    wait_for_prompt(&mut s);

    // Ctrl+R to search, type "alp", Enter to select, Enter to execute
    s.send("\x12").unwrap(); // Ctrl+R
    s.send("alp").unwrap();
    s.send("\r").unwrap();   // Select from search
    s.send("\r").unwrap();   // Execute
    s.expect("alpha").expect("Ctrl+R history search failed");
    wait_for_prompt(&mut s);

    s.send("\x04").unwrap();
}
```

- [ ] **Step 4: Run PTY tests**

Run: `cargo test --test pty_interactive 2>&1 | tail -10`

Expected: all 7 tests pass

- [ ] **Step 5: Run full test suite**

Run: `cargo test 2>&1 | tail -10`

Expected: all tests pass (existing + Layer 1 + Layer 2)

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml tests/pty_interactive.rs
git commit -m "test(interactive): add PTY-based E2E tests using expectrl

E2E tests for REPL lifecycle: echo command, Ctrl+D exit, Ctrl+C interrupt,
history Up re-execution, backspace editing, PS2 continuation prompt, and
Ctrl+R history search. All tests use 5-second timeouts for CI stability."
```
