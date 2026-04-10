# Interactive Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a POSIX-compliant interactive REPL for kish with minimal line editing via crossterm 0.29.

**Architecture:** Three new modules under `src/interactive/` — `LineEditor` (crossterm raw mode + editing), `prompt` (PS1/PS2 expansion), and `Repl` (REPL loop). Entry point branches on TTY detection. Parser errors are classified as incomplete vs. erroneous for PS2 continuation.

**Tech Stack:** Rust (edition 2024), crossterm 0.29, nix 0.31 (existing)

---

## File Structure

| File | Responsibility |
|------|---------------|
| Create: `src/interactive/mod.rs` | `Repl` struct + REPL loop |
| Create: `src/interactive/line_editor.rs` | `LineEditor` struct + raw mode line editing |
| Create: `src/interactive/prompt.rs` | PS1/PS2 expansion via existing expander |
| Modify: `src/main.rs` | Add `mod interactive`, TTY branch, `run_stdin()` |
| Modify: `src/env/mod.rs` | Add `is_interactive` flag to `ShellEnv` |
| Modify: `src/error.rs` | (no changes — existing `ShellErrorKind` variants suffice for incomplete detection) |
| Modify: `Cargo.toml` | Add `crossterm = "0.29"` |
| Create: `tests/interactive.rs` | Unit tests for line editor internals and prompt expansion |

---

### Task 1: Add crossterm dependency and `is_interactive` flag

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/env/mod.rs:288-327`

- [ ] **Step 1: Add crossterm to Cargo.toml**

In `Cargo.toml`, add crossterm under `[dependencies]`:

```toml
[dependencies]
nix = { version = "0.31", features = ["signal", "process", "fs", "poll"] }
libc = "0.2"
crossterm = "0.29"
```

- [ ] **Step 2: Add `is_interactive` field to `ShellEnv`**

In `src/env/mod.rs`, add `is_interactive: bool` to the `ShellEnv` struct after the `expansion_error` field:

```rust
pub struct ShellEnv {
    // ... existing fields ...
    pub expansion_error: bool,
    /// True when running as an interactive shell (stdin is a TTY).
    pub is_interactive: bool,
}
```

In `ShellEnv::new()`, initialize it to `false`:

```rust
impl ShellEnv {
    pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
        // ... existing code ...
        ShellEnv {
            // ... existing fields ...
            expansion_error: false,
            is_interactive: false,
        }
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: Build succeeds (warnings OK).

- [ ] **Step 4: Run existing tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All existing tests pass.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/env/mod.rs
git commit -m "feat(interactive): add crossterm dependency and is_interactive flag"
```

---

### Task 2: Implement `LineEditor` core — buffer operations and struct

**Files:**
- Create: `src/interactive/line_editor.rs`
- Create: `src/interactive/mod.rs` (stub)
- Modify: `src/main.rs:1` (add `mod interactive`)
- Create: `tests/interactive.rs`

- [ ] **Step 1: Write failing tests for LineEditor buffer operations**

Create `tests/interactive.rs`:

```rust
use kish::interactive::line_editor::LineEditor;

#[test]
fn test_insert_char_at_start() {
    let mut editor = LineEditor::new();
    editor.insert_char('a');
    assert_eq!(editor.buffer(), "a");
    assert_eq!(editor.cursor(), 1);
}

#[test]
fn test_insert_char_multiple() {
    let mut editor = LineEditor::new();
    editor.insert_char('a');
    editor.insert_char('b');
    editor.insert_char('c');
    assert_eq!(editor.buffer(), "abc");
    assert_eq!(editor.cursor(), 3);
}

#[test]
fn test_insert_char_at_middle() {
    let mut editor = LineEditor::new();
    editor.insert_char('a');
    editor.insert_char('c');
    editor.move_cursor_left();
    editor.insert_char('b');
    assert_eq!(editor.buffer(), "abc");
    assert_eq!(editor.cursor(), 2);
}

#[test]
fn test_delete_char_backspace() {
    let mut editor = LineEditor::new();
    editor.insert_char('a');
    editor.insert_char('b');
    editor.backspace();
    assert_eq!(editor.buffer(), "a");
    assert_eq!(editor.cursor(), 1);
}

#[test]
fn test_backspace_at_start_does_nothing() {
    let mut editor = LineEditor::new();
    editor.backspace();
    assert_eq!(editor.buffer(), "");
    assert_eq!(editor.cursor(), 0);
}

#[test]
fn test_delete_at_cursor() {
    let mut editor = LineEditor::new();
    editor.insert_char('a');
    editor.insert_char('b');
    editor.insert_char('c');
    editor.move_cursor_left(); // cursor at 2, before 'c'
    editor.delete();
    assert_eq!(editor.buffer(), "ab");
    assert_eq!(editor.cursor(), 2);
}

#[test]
fn test_delete_at_end_does_nothing() {
    let mut editor = LineEditor::new();
    editor.insert_char('a');
    editor.delete();
    assert_eq!(editor.buffer(), "a");
    assert_eq!(editor.cursor(), 1);
}

#[test]
fn test_move_cursor_left() {
    let mut editor = LineEditor::new();
    editor.insert_char('a');
    editor.insert_char('b');
    editor.move_cursor_left();
    assert_eq!(editor.cursor(), 1);
}

#[test]
fn test_move_cursor_left_at_start_does_nothing() {
    let mut editor = LineEditor::new();
    editor.move_cursor_left();
    assert_eq!(editor.cursor(), 0);
}

#[test]
fn test_move_cursor_right() {
    let mut editor = LineEditor::new();
    editor.insert_char('a');
    editor.insert_char('b');
    editor.move_cursor_left();
    editor.move_cursor_left();
    editor.move_cursor_right();
    assert_eq!(editor.cursor(), 1);
}

#[test]
fn test_move_cursor_right_at_end_does_nothing() {
    let mut editor = LineEditor::new();
    editor.insert_char('a');
    editor.move_cursor_right();
    assert_eq!(editor.cursor(), 1);
}

#[test]
fn test_move_to_start() {
    let mut editor = LineEditor::new();
    editor.insert_char('a');
    editor.insert_char('b');
    editor.insert_char('c');
    editor.move_to_start();
    assert_eq!(editor.cursor(), 0);
}

#[test]
fn test_move_to_end() {
    let mut editor = LineEditor::new();
    editor.insert_char('a');
    editor.insert_char('b');
    editor.insert_char('c');
    editor.move_to_start();
    editor.move_to_end();
    assert_eq!(editor.cursor(), 3);
}

#[test]
fn test_clear_buffer() {
    let mut editor = LineEditor::new();
    editor.insert_char('a');
    editor.insert_char('b');
    editor.clear();
    assert_eq!(editor.buffer(), "");
    assert_eq!(editor.cursor(), 0);
}

#[test]
fn test_is_empty() {
    let mut editor = LineEditor::new();
    assert!(editor.is_empty());
    editor.insert_char('a');
    assert!(!editor.is_empty());
}

#[test]
fn test_to_string() {
    let mut editor = LineEditor::new();
    editor.insert_char('h');
    editor.insert_char('i');
    assert_eq!(editor.to_string(), "hi");
}

#[test]
fn test_backspace_in_middle() {
    let mut editor = LineEditor::new();
    editor.insert_char('a');
    editor.insert_char('b');
    editor.insert_char('c');
    editor.move_cursor_left(); // cursor at 2
    editor.backspace(); // delete 'b'
    assert_eq!(editor.buffer(), "ac");
    assert_eq!(editor.cursor(), 1);
}
```

- [ ] **Step 2: Create module stubs to make it compile**

Create `src/interactive/mod.rs`:

```rust
pub mod line_editor;
pub mod prompt;
```

Create `src/interactive/line_editor.rs`:

```rust
/// Minimal line editor with cursor-based buffer operations.
/// Terminal I/O (crossterm) is handled in `read_line()`;
/// the buffer methods below are pure logic, testable without a terminal.
pub struct LineEditor {
    buf: Vec<char>,
    pos: usize,
}

impl LineEditor {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            pos: 0,
        }
    }

    /// Current buffer contents as a string.
    pub fn buffer(&self) -> String {
        self.buf.iter().collect()
    }

    /// Current cursor position (character index).
    pub fn cursor(&self) -> usize {
        self.pos
    }

    /// True if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Return buffer contents as a String.
    pub fn to_string(&self) -> String {
        self.buf.iter().collect()
    }

    /// Clear the buffer and reset cursor.
    pub fn clear(&mut self) {
        self.buf.clear();
        self.pos = 0;
    }

    /// Insert a character at the current cursor position.
    pub fn insert_char(&mut self, ch: char) {
        self.buf.insert(self.pos, ch);
        self.pos += 1;
    }

    /// Delete the character before the cursor (backspace).
    pub fn backspace(&mut self) {
        if self.pos > 0 {
            self.pos -= 1;
            self.buf.remove(self.pos);
        }
    }

    /// Delete the character at the cursor position.
    pub fn delete(&mut self) {
        if self.pos < self.buf.len() {
            self.buf.remove(self.pos);
        }
    }

    /// Move cursor one position to the left.
    pub fn move_cursor_left(&mut self) {
        if self.pos > 0 {
            self.pos -= 1;
        }
    }

    /// Move cursor one position to the right.
    pub fn move_cursor_right(&mut self) {
        if self.pos < self.buf.len() {
            self.pos += 1;
        }
    }

    /// Move cursor to the start of the line.
    pub fn move_to_start(&mut self) {
        self.pos = 0;
    }

    /// Move cursor to the end of the line.
    pub fn move_to_end(&mut self) {
        self.pos = self.buf.len();
    }
}
```

Create `src/interactive/prompt.rs` (stub):

```rust
// Prompt expansion — implemented in Task 4.
```

Add `mod interactive;` to `src/main.rs` after the existing module declarations.

- [ ] **Step 3: Run tests to verify they fail (then pass after implementation)**

Run: `cargo test --test interactive 2>&1 | tail -20`
Expected: All 16 tests pass (implementation is in the same step).

- [ ] **Step 4: Commit**

```bash
git add src/interactive/ tests/interactive.rs src/main.rs
git commit -m "feat(interactive): implement LineEditor buffer operations with tests"
```

---

### Task 3: Implement `LineEditor::read_line()` with crossterm

**Files:**
- Modify: `src/interactive/line_editor.rs`

- [ ] **Step 1: Add `read_line()` method with raw mode and key event handling**

Add the following imports at the top of `src/interactive/line_editor.rs`:

```rust
use std::io::{self, Write, Stdout, stdout};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{self, ClearType},
    ExecutableCommand,
};
```

Add a `RawModeGuard` struct for RAII cleanup:

```rust
/// RAII guard that disables raw mode on drop.
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
```

Add `read_line()` to the `impl LineEditor` block:

```rust
    /// Read a line of input from the terminal with editing support.
    ///
    /// Returns:
    /// - `Ok(Some(line))` — user pressed Enter
    /// - `Ok(None)` — EOF (Ctrl+D on empty buffer)
    /// - `Err(_)` — I/O error
    ///
    /// `prompt_width` is the number of character columns the prompt occupies,
    /// used to position the cursor correctly.
    pub fn read_line(&mut self, prompt_width: usize) -> io::Result<Option<String>> {
        self.clear();
        let _guard = RawModeGuard::new()?;
        let mut stdout = stdout();

        loop {
            match event::read()? {
                Event::Key(key_event) => {
                    match self.handle_key(key_event) {
                        KeyAction::Continue => {}
                        KeyAction::Submit => {
                            // Print newline after the entered line
                            stdout.execute(cursor::MoveToColumn(0))?;
                            write!(stdout, "\r\n")?;
                            stdout.flush()?;
                            let line = self.to_string();
                            return Ok(Some(line));
                        }
                        KeyAction::Eof => {
                            return Ok(None);
                        }
                        KeyAction::Interrupt => {
                            // Ctrl+C: discard input, signal new line
                            stdout.execute(cursor::MoveToColumn(0))?;
                            write!(stdout, "\r\n")?;
                            stdout.flush()?;
                            self.clear();
                            return Ok(Some(String::new()));
                        }
                    }
                    self.redraw(&mut stdout, prompt_width)?;
                }
                _ => {} // Ignore mouse, resize, etc.
            }
        }
    }

    /// Redraw the current line from the prompt onward.
    fn redraw(&self, stdout: &mut Stdout, prompt_width: usize) -> io::Result<()> {
        // Move to prompt end, clear to end of line, print buffer, reposition cursor
        stdout.execute(cursor::MoveToColumn(prompt_width as u16))?;
        stdout.execute(terminal::Clear(ClearType::UntilEndOfLine))?;
        write!(stdout, "{}", self.buffer())?;
        stdout.execute(cursor::MoveToColumn((prompt_width + self.pos) as u16))?;
        stdout.flush()?;
        Ok(())
    }

    /// Process a key event and return what action the REPL should take.
    fn handle_key(&mut self, key: KeyEvent) -> KeyAction {
        let KeyEvent { code, modifiers, .. } = key;
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);

        match (code, ctrl) {
            // Ctrl+D: EOF if empty, delete if not
            (KeyCode::Char('d'), true) => {
                if self.is_empty() {
                    KeyAction::Eof
                } else {
                    self.delete();
                    KeyAction::Continue
                }
            }
            // Ctrl+C: interrupt
            (KeyCode::Char('c'), true) => KeyAction::Interrupt,
            // Ctrl+B: left
            (KeyCode::Char('b'), true) => {
                self.move_cursor_left();
                KeyAction::Continue
            }
            // Ctrl+F: right
            (KeyCode::Char('f'), true) => {
                self.move_cursor_right();
                KeyAction::Continue
            }
            // Ctrl+A: home
            (KeyCode::Char('a'), true) => {
                self.move_to_start();
                KeyAction::Continue
            }
            // Ctrl+E: end
            (KeyCode::Char('e'), true) => {
                self.move_to_end();
                KeyAction::Continue
            }
            // Enter: submit
            (KeyCode::Enter, _) => KeyAction::Submit,
            // Backspace
            (KeyCode::Backspace, _) => {
                self.backspace();
                KeyAction::Continue
            }
            // Delete
            (KeyCode::Delete, _) => {
                self.delete();
                KeyAction::Continue
            }
            // Arrow keys
            (KeyCode::Left, _) => {
                self.move_cursor_left();
                KeyAction::Continue
            }
            (KeyCode::Right, _) => {
                self.move_cursor_right();
                KeyAction::Continue
            }
            // Home / End
            (KeyCode::Home, _) => {
                self.move_to_start();
                KeyAction::Continue
            }
            (KeyCode::End, _) => {
                self.move_to_end();
                KeyAction::Continue
            }
            // Printable characters (without Ctrl modifier)
            (KeyCode::Char(ch), false) => {
                self.insert_char(ch);
                KeyAction::Continue
            }
            // Everything else: ignore
            _ => KeyAction::Continue,
        }
    }
```

Add the `KeyAction` enum before the `impl LineEditor` block:

```rust
/// Result of processing a key event.
enum KeyAction {
    /// Continue reading input.
    Continue,
    /// User pressed Enter — submit the line.
    Submit,
    /// User pressed Ctrl+D on empty buffer — end of file.
    Eof,
    /// User pressed Ctrl+C — discard input.
    Interrupt,
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: Build succeeds.

- [ ] **Step 3: Run existing tests to ensure nothing is broken**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/interactive/line_editor.rs
git commit -m "feat(interactive): implement LineEditor::read_line() with crossterm key handling"
```

---

### Task 4: Implement prompt expansion

**Files:**
- Modify: `src/interactive/prompt.rs`
- Modify: `tests/interactive.rs`

- [ ] **Step 1: Write failing tests for prompt expansion**

Add to `tests/interactive.rs`:

```rust
use kish::env::ShellEnv;
use kish::interactive::prompt::expand_prompt;

#[test]
fn test_prompt_default_ps1() {
    let mut env = ShellEnv::new("kish", vec![]);
    let prompt = expand_prompt(&mut env, "PS1");
    assert_eq!(prompt, "$ ");
}

#[test]
fn test_prompt_default_ps2() {
    let mut env = ShellEnv::new("kish", vec![]);
    let prompt = expand_prompt(&mut env, "PS2");
    assert_eq!(prompt, "> ");
}

#[test]
fn test_prompt_custom_ps1() {
    let mut env = ShellEnv::new("kish", vec![]);
    env.vars.set("PS1", "myshell> ", false);
    let prompt = expand_prompt(&mut env, "PS1");
    assert_eq!(prompt, "myshell> ");
}

#[test]
fn test_prompt_with_variable_expansion() {
    let mut env = ShellEnv::new("kish", vec![]);
    env.vars.set("MYVAR", "hello", false);
    env.vars.set("PS1", "${MYVAR}$ ", false);
    let prompt = expand_prompt(&mut env, "PS1");
    assert_eq!(prompt, "hello$ ");
}

#[test]
fn test_prompt_empty_string() {
    let mut env = ShellEnv::new("kish", vec![]);
    env.vars.set("PS1", "", false);
    let prompt = expand_prompt(&mut env, "PS1");
    assert_eq!(prompt, "");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test interactive test_prompt 2>&1 | tail -10`
Expected: FAIL — `expand_prompt` not found.

- [ ] **Step 3: Implement prompt expansion**

Replace `src/interactive/prompt.rs` with:

```rust
use crate::env::ShellEnv;
use crate::parser::ast::{Word, WordPart};
use crate::expand::expand_word_to_string;

/// Expand a prompt variable (PS1 or PS2) through the shell's word expander.
///
/// If the variable is not set, returns the POSIX default:
/// - PS1: `"$ "` (or `"# "` if UID == 0)
/// - PS2: `"> "`
pub fn expand_prompt(env: &mut ShellEnv, var_name: &str) -> String {
    let raw = match env.vars.get(var_name) {
        Some(val) => val.to_string(),
        None => default_prompt(var_name).to_string(),
    };

    if raw.is_empty() {
        return String::new();
    }

    // Parse the prompt string as a double-quoted word to enable
    // parameter expansion and command substitution.
    // We reuse the lexer/parser to handle `$VAR`, `${VAR}`, `$(cmd)` etc.
    let word = parse_prompt_word(&raw);
    expand_word_to_string(env, &word)
}

/// Returns the POSIX default prompt for the given variable.
fn default_prompt(var_name: &str) -> &'static str {
    match var_name {
        "PS1" => {
            if nix::unistd::getuid().is_root() {
                "# "
            } else {
                "$ "
            }
        }
        "PS2" => "> ",
        _ => "",
    }
}

/// Parse a prompt string into a Word as if it were inside double quotes.
/// This allows $VAR, ${VAR}, $(cmd), and `cmd` expansions.
fn parse_prompt_word(raw: &str) -> Word {
    // Use the shell's lexer to tokenize the prompt as a double-quoted string.
    // Wrap in double quotes so the parser treats it as a single word with expansions.
    let quoted = format!("\"{}\"", raw);
    let mut parser = crate::parser::Parser::new(&quoted);

    // Try to parse as a single word. If parsing fails (e.g., unmatched quotes
    // in the prompt value), fall back to a literal.
    match parser.parse_word() {
        Ok(word) => word,
        Err(_) => Word {
            parts: vec![WordPart::Literal(raw.to_string())],
        },
    }
}
```

- [ ] **Step 4: Verify `parse_word` is public on Parser**

Check `src/parser/mod.rs` for `parse_word`. If it is not `pub`, change its visibility to `pub`.

Run: `cargo build 2>&1 | tail -10`
Expected: Build succeeds. If `parse_word` is private, make it `pub` first.

- [ ] **Step 5: Run tests**

Run: `cargo test --test interactive test_prompt 2>&1 | tail -10`
Expected: All 5 prompt tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/interactive/prompt.rs tests/interactive.rs src/parser/mod.rs
git commit -m "feat(interactive): implement PS1/PS2 prompt expansion with tests"
```

---

### Task 5: Classify parser errors as incomplete vs. erroneous

**Files:**
- Create: `src/interactive/parse_status.rs`
- Modify: `src/interactive/mod.rs`

- [ ] **Step 1: Write failing tests for parse classification**

Add to `tests/interactive.rs`:

```rust
use kish::interactive::parse_status::classify_parse;
use kish::interactive::parse_status::ParseStatus;
use kish::env::aliases::AliasStore;

#[test]
fn test_classify_complete_command() {
    let aliases = AliasStore::default();
    match classify_parse("echo hello\n", &aliases) {
        ParseStatus::Complete(_) => {}
        other => panic!("expected Complete, got {:?}", other),
    }
}

#[test]
fn test_classify_empty_input() {
    let aliases = AliasStore::default();
    match classify_parse("\n", &aliases) {
        ParseStatus::Empty => {}
        other => panic!("expected Empty, got {:?}", other),
    }
}

#[test]
fn test_classify_incomplete_if() {
    let aliases = AliasStore::default();
    match classify_parse("if true; then\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_incomplete_while() {
    let aliases = AliasStore::default();
    match classify_parse("while true; do\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_incomplete_single_quote() {
    let aliases = AliasStore::default();
    match classify_parse("echo 'hello\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_incomplete_double_quote() {
    let aliases = AliasStore::default();
    match classify_parse("echo \"hello\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_incomplete_backslash_newline() {
    let aliases = AliasStore::default();
    match classify_parse("echo hello \\\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_incomplete_pipe() {
    let aliases = AliasStore::default();
    match classify_parse("echo hello |\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_incomplete_and_or() {
    let aliases = AliasStore::default();
    match classify_parse("true &&\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_error() {
    let aliases = AliasStore::default();
    match classify_parse("if ; then\n", &aliases) {
        ParseStatus::Error(_) => {}
        other => panic!("expected Error, got {:?}", other),
    }
}

#[test]
fn test_classify_multiple_commands() {
    let aliases = AliasStore::default();
    match classify_parse("echo a; echo b\n", &aliases) {
        ParseStatus::Complete(_) => {}
        other => panic!("expected Complete, got {:?}", other),
    }
}
```

- [ ] **Step 2: Create parse_status module**

Create `src/interactive/parse_status.rs`:

```rust
use crate::env::aliases::AliasStore;
use crate::error::ShellErrorKind;
use crate::parser::Parser;
use crate::parser::ast::CompleteCommand;

/// Result of attempting to parse accumulated input in interactive mode.
#[derive(Debug)]
pub enum ParseStatus {
    /// One or more complete commands were parsed.
    Complete(Vec<CompleteCommand>),
    /// Input is syntactically incomplete — needs more lines (PS2).
    Incomplete,
    /// Input is empty (only whitespace/newlines).
    Empty,
    /// Genuine syntax error.
    Error(String),
}

/// Classify the parse result of accumulated interactive input.
///
/// Distinguishes between:
/// - Complete: valid commands ready for execution
/// - Incomplete: input ends mid-construct (unterminated quote, open if/while/for, trailing operator)
/// - Empty: nothing to parse
/// - Error: real syntax error
pub fn classify_parse(input: &str, aliases: &AliasStore) -> ParseStatus {
    let trimmed = input.trim_start_matches([' ', '\t', '\n']);
    if trimmed.is_empty() {
        return ParseStatus::Empty;
    }

    // Check if input ends with a line continuation (backslash-newline)
    if input.ends_with("\\\n") {
        return ParseStatus::Incomplete;
    }

    // Check if input ends with a pipe or && or || (operator continuation)
    let trimmed_end = input.trim_end_matches([' ', '\t', '\n']);
    if trimmed_end.ends_with('|') || trimmed_end.ends_with("&&") || trimmed_end.ends_with("||") {
        return ParseStatus::Incomplete;
    }

    let mut parser = Parser::new_with_aliases(input, aliases);
    let mut commands = Vec::new();

    loop {
        if parser.is_at_end() {
            break;
        }

        match parser.parse_complete_command() {
            Ok(cmd) => {
                let consumed = parser.consumed_bytes();
                if consumed == 0 {
                    break;
                }
                commands.push(cmd);
            }
            Err(err) => {
                return if is_incomplete_error(&err.kind) {
                    ParseStatus::Incomplete
                } else {
                    ParseStatus::Error(err.message)
                };
            }
        }
    }

    if commands.is_empty() {
        ParseStatus::Empty
    } else {
        ParseStatus::Complete(commands)
    }
}

/// Determine if a parse error indicates incomplete input rather than a genuine error.
///
/// Unterminated quotes, command substitutions, and unexpected EOF all indicate
/// the user needs to type more, not that they made a mistake.
fn is_incomplete_error(kind: &ShellErrorKind) -> bool {
    matches!(
        kind,
        ShellErrorKind::UnterminatedSingleQuote
            | ShellErrorKind::UnterminatedDoubleQuote
            | ShellErrorKind::UnterminatedCommandSub
            | ShellErrorKind::UnterminatedArithSub
            | ShellErrorKind::UnterminatedParamExpansion
            | ShellErrorKind::UnterminatedBacktick
            | ShellErrorKind::UnterminatedDollarSingleQuote
            | ShellErrorKind::UnexpectedEof
    )
}
```

Update `src/interactive/mod.rs`:

```rust
pub mod line_editor;
pub mod parse_status;
pub mod prompt;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --test interactive test_classify 2>&1 | tail -20`
Expected: All 11 classify tests pass. Some may need adjustment based on how the parser actually reports errors — fix as needed.

- [ ] **Step 4: Commit**

```bash
git add src/interactive/parse_status.rs src/interactive/mod.rs tests/interactive.rs
git commit -m "feat(interactive): implement parse status classification for PS2 continuation"
```

---

### Task 6: Implement the REPL loop

**Files:**
- Modify: `src/interactive/mod.rs`

- [ ] **Step 1: Implement `Repl` struct and `run()` method**

Replace `src/interactive/mod.rs` with:

```rust
pub mod line_editor;
pub mod parse_status;
pub mod prompt;

use std::io::{self, Write};

use crate::exec::Executor;
use crate::signal;

use line_editor::LineEditor;
use parse_status::{ParseStatus, classify_parse};
use prompt::expand_prompt;

pub struct Repl {
    executor: Executor,
    line_editor: LineEditor,
}

impl Repl {
    pub fn new(shell_name: String) -> Self {
        signal::init_signal_handling();
        let mut executor = Executor::new(shell_name, vec![]);
        executor.env.is_interactive = true;
        Self {
            executor,
            line_editor: LineEditor::new(),
        }
    }

    /// Run the interactive REPL loop. Returns the exit status.
    pub fn run(&mut self) -> i32 {
        let mut input_buffer = String::new();

        loop {
            // Choose PS1 or PS2
            let prompt_var = if input_buffer.is_empty() { "PS1" } else { "PS2" };
            let prompt = expand_prompt(&mut self.executor.env, prompt_var);
            let prompt_width = prompt.chars().count();

            // Display prompt on stderr
            eprint!("{}", prompt);
            io::stderr().flush().ok();

            // Read a line
            let line = match self.line_editor.read_line(prompt_width) {
                Ok(Some(line)) => line,
                Ok(None) => {
                    // EOF (Ctrl+D)
                    if self.executor.env.options.ignoreeof {
                        eprintln!("\r\nkish: Use \"exit\" to leave the shell.");
                        input_buffer.clear();
                        continue;
                    }
                    // Exit the shell
                    eprintln!();
                    break;
                }
                Err(_) => {
                    break;
                }
            };

            // Ctrl+C returns empty string — reset buffer and re-prompt
            if line.is_empty() && !input_buffer.is_empty() {
                input_buffer.clear();
                continue;
            }

            // Skip empty lines at PS1
            if line.is_empty() && input_buffer.is_empty() {
                continue;
            }

            // Accumulate input
            input_buffer.push_str(&line);
            input_buffer.push('\n');

            // Verbose mode: print the input
            self.executor.verbose_print(&line);

            // Try to parse
            match classify_parse(&input_buffer, &self.executor.env.aliases) {
                ParseStatus::Complete(commands) => {
                    for cmd in &commands {
                        let status = self.executor.exec_complete_command(cmd);
                        self.executor.env.last_exit_status = status;
                        // In interactive mode, errexit does not exit the shell,
                        // but we still track the status.
                        if self.executor.env.options.errexit && status != 0 {
                            // POSIX: errexit in interactive shell does not exit,
                            // but the status is set.
                        }
                    }
                    input_buffer.clear();
                }
                ParseStatus::Incomplete => {
                    // Continue reading (PS2 will be shown next iteration)
                    continue;
                }
                ParseStatus::Empty => {
                    input_buffer.clear();
                }
                ParseStatus::Error(msg) => {
                    eprintln!("kish: {}", msg);
                    input_buffer.clear();
                }
            }

            // Process any pending signals
            self.executor.process_pending_signals();
        }

        self.executor.process_pending_signals();
        self.executor.execute_exit_trap();
        self.executor.env.last_exit_status
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: Build succeeds.

- [ ] **Step 3: Run all tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/interactive/mod.rs
git commit -m "feat(interactive): implement REPL loop with PS1/PS2 and parse continuation"
```

---

### Task 7: Wire up `main.rs` entry point

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Update main.rs to launch interactive mode**

Replace the `args.len() == 1` branch in `main()`:

```rust
1 => {
    if nix::unistd::isatty(nix::libc::STDIN_FILENO).unwrap_or(false) {
        let mut repl = interactive::Repl::new(shell_name);
        process::exit(repl.run());
    } else {
        // stdin is a pipe — read as script
        let mut input = String::new();
        io::stdin().read_to_string(&mut input).unwrap_or_else(|e| {
            eprintln!("kish: {}", e);
            process::exit(1);
        });
        let status = run_string(&input, shell_name, vec![], false);
        process::exit(status);
    }
}
```

Add `use nix::unistd::isatty;` is not needed since we use the full path. Make sure `use std::io::{self, Read};` is already present (it is).

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: Build succeeds.

- [ ] **Step 3: Run all tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests pass.

- [ ] **Step 4: Smoke test interactive mode manually**

Run: `echo 'echo hello' | cargo run`
Expected: Outputs `hello` (piped stdin goes through `run_string` path, not interactive).

Run: `echo 'echo hello' | cargo run -- -c 'echo world'`
Expected: Outputs `world` (the `-c` path is unchanged).

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat(interactive): wire up TTY detection and REPL entry point in main"
```

---

### Task 8: Update TODO.md — mark ignoreeof as resolved

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Remove the ignoreeof limitation entries**

In `TODO.md`, delete these two lines (they appear in both Phase 6 and Phase 7 sections):

```
- [ ] `ignoreeof` is settable but has no effect — interactive mode feature
```

They are now resolved since the REPL loop checks `ignoreeof` on Ctrl+D.

- [ ] **Step 2: Commit**

```bash
git add TODO.md
git commit -m "docs: remove ignoreeof limitation from TODO — resolved by interactive mode"
```

---

### Task 9: End-to-end integration verification

**Files:**
- No new files — verification only.

- [ ] **Step 1: Run the full test suite**

Run: `cargo test 2>&1 | tail -20`
Expected: All tests pass (unit + integration + e2e).

- [ ] **Step 2: Run the e2e test runner if it exists**

Run: `ls e2e/run_tests.sh 2>/dev/null && bash e2e/run_tests.sh 2>&1 | tail -30`
Expected: All e2e tests pass.

- [ ] **Step 3: Verify piped stdin works as non-interactive**

Run: `echo 'echo piped' | cargo run`
Expected: Outputs `piped`.

Run: `printf 'A=1\necho $A\n' | cargo run`
Expected: Outputs `1`.

- [ ] **Step 4: Verify -c mode is unchanged**

Run: `cargo run -- -c 'echo hello; echo world'`
Expected: Outputs `hello` then `world`.

- [ ] **Step 5: Verify file execution is unchanged**

Run: `echo 'echo from_file' > /tmp/kish_test.sh && cargo run -- /tmp/kish_test.sh && rm /tmp/kish_test.sh`
Expected: Outputs `from_file`.

- [ ] **Step 6: Commit (if any fixes were needed)**

Only commit if test failures required fixes. Otherwise, skip.
