# History-Based Inline Autosuggest Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add fish-style inline autosuggestion that shows dim ghost text from command history as the user types, accepted via Right/Ctrl+F (full) or Alt+F (word-by-word).

**Architecture:** `History` gets a `suggest(prefix)` method for prefix-match lookup (newest first). `LineEditor` gains a `suggestion: Option<String>` field for display state, with key handlers for acceptance. The `Terminal` trait gets `set_dim()` for rendering.

**Tech Stack:** Rust, crossterm (terminal I/O, `Attribute::Dim`), expectrl (PTY tests)

---

## File Structure

| File | Role | Action |
|------|------|--------|
| `src/interactive/history.rs` | Prefix-match suggestion lookup | Modify: add `suggest()` method |
| `src/interactive/terminal.rs` | Terminal trait + crossterm impl | Modify: add `set_dim()` to trait and impl |
| `tests/helpers/mock_terminal.rs` | Mock terminal for tests | Modify: add `set_dim()` impl and tracking |
| `src/interactive/line_editor.rs` | Line editing + key handling + redraw | Modify: add suggestion state, key handlers, redraw logic |
| `tests/interactive.rs` | MockTerminal-based tests | Modify: add autosuggest tests |
| `tests/pty_interactive.rs` | PTY E2E tests | Modify: add autosuggest E2E test |

---

### Task 1: History::suggest() — Test and Implementation

**Files:**
- Modify: `src/interactive/history.rs:16-133` (add method + unit tests in `mod tests`)

- [ ] **Step 1: Write failing tests for `suggest()`**

Add these tests inside the existing `#[cfg(test)] mod tests` block at the end of `src/interactive/history.rs`:

```rust
#[test]
fn test_suggest_prefix_match() {
    let mut h = History::new();
    h.add("git commit -m 'fix'", 500, "");
    h.add("git push origin main", 500, "");
    // "git c" matches "git commit -m 'fix'" — returns the suffix
    assert_eq!(h.suggest("git c"), Some("ommit -m 'fix'".to_string()));
}

#[test]
fn test_suggest_most_recent_wins() {
    let mut h = History::new();
    h.add("echo first", 500, "");
    h.add("echo second", 500, "");
    // Both match "echo ", but most recent ("echo second") wins
    assert_eq!(h.suggest("echo "), Some("second".to_string()));
}

#[test]
fn test_suggest_exact_match_excluded() {
    let mut h = History::new();
    h.add("ls -la", 500, "");
    // Exact match returns None (nothing to suggest)
    assert_eq!(h.suggest("ls -la"), None);
}

#[test]
fn test_suggest_empty_prefix_returns_none() {
    let mut h = History::new();
    h.add("some command", 500, "");
    assert_eq!(h.suggest(""), None);
}

#[test]
fn test_suggest_no_match_returns_none() {
    let mut h = History::new();
    h.add("git commit", 500, "");
    assert_eq!(h.suggest("cargo"), None);
}

#[test]
fn test_suggest_empty_history_returns_none() {
    let h = History::new();
    assert_eq!(h.suggest("git"), None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib interactive::history::tests::test_suggest -- --nocapture`
Expected: Compilation error — `suggest` method does not exist.

- [ ] **Step 3: Implement `suggest()` method**

Add this method to the `impl History` block in `src/interactive/history.rs`, after the `is_empty()` method (around line 35):

```rust
/// Return the suffix of the most recent history entry that starts with `prefix`.
/// Returns `None` if `prefix` is empty, no entry matches, or only exact matches exist.
pub fn suggest(&self, prefix: &str) -> Option<String> {
    if prefix.is_empty() {
        return None;
    }
    self.entries
        .iter()
        .rev()
        .find(|entry| entry.starts_with(prefix) && entry.as_str() != prefix)
        .map(|entry| entry[prefix.len()..].to_string())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib interactive::history::tests::test_suggest`
Expected: All 6 `test_suggest_*` tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/interactive/history.rs
git commit -m "feat(history): add suggest() method for prefix-match autosuggestion"
```

---

### Task 2: Terminal Trait — Add `set_dim()` Method

**Files:**
- Modify: `src/interactive/terminal.rs:11-49` (trait definition)
- Modify: `src/interactive/terminal.rs:63-127` (CrosstermTerminal impl)
- Modify: `tests/helpers/mock_terminal.rs:8-15` (MockTerminal struct)
- Modify: `tests/helpers/mock_terminal.rs:45-101` (MockTerminal impl)

- [ ] **Step 1: Add `set_dim()` to the `Terminal` trait**

In `src/interactive/terminal.rs`, add a new method to the `Terminal` trait after `set_reverse` (line 40):

```rust
/// Set dim (faint) text attribute on/off.
fn set_dim(&mut self, on: bool) -> io::Result<()>;
```

- [ ] **Step 2: Implement `set_dim()` for `CrosstermTerminal`**

In `src/interactive/terminal.rs`, add this method to `impl Terminal for CrosstermTerminal` after `set_reverse` (line 112):

```rust
fn set_dim(&mut self, on: bool) -> io::Result<()> {
    if on {
        self.stdout.execute(SetAttribute(Attribute::Dim))?;
    } else {
        self.stdout.execute(SetAttribute(Attribute::Reset))?;
    }
    Ok(())
}
```

- [ ] **Step 3: Add dim tracking to `MockTerminal` and implement `set_dim()`**

In `tests/helpers/mock_terminal.rs`, add a `dim` field to the `MockTerminal` struct:

```rust
pub struct MockTerminal {
    events: VecDeque<Event>,
    size: (u16, u16),
    output: Vec<String>,
    cursor_row: i32,
    dim: bool,
}
```

Update `MockTerminal::new()` to initialize it:

```rust
pub fn new(events: Vec<Event>) -> Self {
    Self {
        events: VecDeque::from(events),
        size: (80, 24),
        output: Vec::new(),
        cursor_row: 0,
        dim: false,
    }
}
```

Add a getter method after `cursor_row()`:

```rust
#[allow(dead_code)]
pub fn dim(&self) -> bool {
    self.dim
}
```

Add the `set_dim()` implementation in `impl Terminal for MockTerminal`, after `set_reverse`:

```rust
fn set_dim(&mut self, on: bool) -> io::Result<()> {
    self.dim = on;
    if on {
        self.output.push("[DIM]".to_string());
    } else {
        self.output.push("[/DIM]".to_string());
    }
    Ok(())
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo test --no-run`
Expected: Compiles successfully (no test failures — just checking compilation).

- [ ] **Step 5: Commit**

```bash
git add src/interactive/terminal.rs tests/helpers/mock_terminal.rs
git commit -m "feat(terminal): add set_dim() method to Terminal trait for dim text rendering"
```

---

### Task 3: LineEditor — Suggestion State and Update Logic

**Files:**
- Modify: `src/interactive/line_editor.rs:13-17` (struct fields)
- Modify: `src/interactive/line_editor.rs:19-97` (impl methods)
- Modify: `src/interactive/line_editor.rs:118-175` (read_line_loop, pass history to update)
- Test: `tests/interactive.rs`

- [ ] **Step 1: Write failing test for suggestion state**

Add this test to `tests/interactive.rs`:

```rust
#[test]
fn test_suggest_appears_on_typing() {
    let mut history = History::new();
    history.add("git commit -m 'fix'", 500, "");

    // Type "git c" then Enter
    let mut events = chars("git c");
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line("$ ", &mut history, &mut term).unwrap();
    assert_eq!(result, Some("git c".to_string()));

    // Check that dim suggestion text was rendered
    let output = term.output().join("");
    assert!(
        output.contains("[DIM]"),
        "suggestion should trigger dim rendering"
    );
    assert!(
        output.contains("ommit -m 'fix'"),
        "suggestion text should appear in output"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_suggest_appears_on_typing -- --nocapture`
Expected: FAIL — no `[DIM]` in output because suggestion logic doesn't exist yet.

- [ ] **Step 3: Add `suggestion` field and `update_suggestion()` to `LineEditor`**

In `src/interactive/line_editor.rs`, modify the struct (line 13-17):

```rust
#[derive(Debug, Default)]
pub struct LineEditor {
    buf: Vec<char>,
    pos: usize,
    suggestion: Option<String>,
}
```

Add these methods to the first `impl LineEditor` block (after `move_to_end`, around line 96):

```rust
/// Return the current suggestion text, if any.
#[allow(dead_code)]
pub fn suggestion(&self) -> Option<&str> {
    self.suggestion.as_deref()
}

/// Update the autosuggestion based on the current buffer state.
/// Only suggests when the cursor is at the end of a non-empty buffer.
fn update_suggestion(&mut self, history: &History) {
    if self.pos == self.buf.len() && !self.buf.is_empty() {
        self.suggestion = history.suggest(&self.buffer());
    } else {
        self.suggestion = None;
    }
}
```

Also update `clear()` to reset suggestion:

```rust
pub fn clear(&mut self) {
    self.buf.clear();
    self.pos = 0;
    self.suggestion = None;
}
```

- [ ] **Step 4: Call `update_suggestion()` in `read_line_loop` after each key action**

In `src/interactive/line_editor.rs`, modify `read_line_loop` (around line 130-175). Add an `update_suggestion` call after `handle_key` for the `Continue` case. The loop body becomes:

```rust
fn read_line_loop<T: Terminal>(&mut self, prompt: &str, history: &mut History, term: &mut T) -> io::Result<Option<String>> {
    let prompt_width = prompt.chars().count();
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
                    self.suggestion = None;
                    term.disable_raw_mode()?;
                    match FuzzySearchUI::run(history, term) {
                        Ok(Some(line)) => {
                            self.buf = line.chars().collect();
                            self.pos = self.buf.len();
                        }
                        _ => {}
                    }
                    term.enable_raw_mode()?;
                    term.move_to_column(0)?;
                    term.clear_current_line()?;
                    term.write_str(prompt)?;
                    self.redraw(term, prompt_width)?;
                }
                KeyAction::Continue => {}
            }
            self.update_suggestion(history);
            self.redraw(term, prompt_width)?;
        }
    }
}
```

- [ ] **Step 5: Add suggestion rendering to `redraw()`**

Modify `redraw()` in `src/interactive/line_editor.rs`:

```rust
fn redraw<T: Terminal>(&self, term: &mut T, prompt_width: usize) -> io::Result<()> {
    let col = |n: usize| -> u16 { n.min(u16::MAX as usize) as u16 };
    term.move_to_column(col(prompt_width))?;
    term.clear_until_newline()?;
    term.write_str(&self.buffer())?;
    // Draw suggestion in dim text when cursor is at end of buffer
    if let Some(ref suggestion) = self.suggestion {
        if self.pos == self.buf.len() {
            term.set_dim(true)?;
            term.write_str(suggestion)?;
            term.set_dim(false)?;
        }
    }
    term.move_to_column(col(prompt_width + self.pos))?;
    term.flush()?;
    Ok(())
}
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test test_suggest_appears_on_typing -- --nocapture`
Expected: PASS

- [ ] **Step 7: Write test that suggestion disappears when cursor not at end**

Add to `tests/interactive.rs`:

```rust
#[test]
fn test_suggest_hidden_when_cursor_not_at_end() {
    let mut history = History::new();
    history.add("echo hello world", 500, "");

    // Type "echo h", Left (cursor no longer at end), then Enter
    let mut events = chars("echo h");
    events.push(key(KeyCode::Left));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let _ = editor.read_line("$ ", &mut history, &mut term).unwrap();

    // After moving cursor left, the last redraw should NOT have dim text.
    // Check that the last [DIM] is followed by output, then [/DIM], and
    // after the cursor-left there's no further [DIM].
    let output_parts = term.output();
    // Find the index of the last Left key event's redraw — it should not contain [DIM]
    // Simplest check: the last few outputs before Enter should not have [DIM]
    let last_outputs = output_parts.iter().rev().take(10).collect::<Vec<_>>();
    // The last redraw cycle (after Left) should not contain [DIM]
    let last_chunk: String = last_outputs.iter().rev().map(|s| s.as_str()).collect();
    // After the Left arrow, there should be no new [DIM] before the Enter
    let last_dim_pos = last_chunk.rfind("[DIM]");
    let last_nodim_pos = last_chunk.rfind("[/DIM]");
    // Either no [DIM] in the last chunk, or the last [DIM] is before the last [/DIM]
    match (last_dim_pos, last_nodim_pos) {
        (Some(d), Some(nd)) => assert!(d < nd, "suggestion should not be active after cursor moved left"),
        (None, _) => {} // No dim at all in the tail — correct
        (Some(_), None) => panic!("unclosed [DIM] in output"),
    }
}
```

- [ ] **Step 8: Run test to verify it passes**

Run: `cargo test test_suggest_hidden_when_cursor_not_at_end -- --nocapture`
Expected: PASS

- [ ] **Step 9: Write test that Up/Down clears suggestion**

Add to `tests/interactive.rs`:

```rust
#[test]
fn test_suggest_cleared_on_history_navigation() {
    let mut history = History::new();
    history.add("echo hello", 500, "");
    history.add("echo world", 500, "");

    // Type "echo " (suggestion active), then Up (history nav clears suggestion), Enter
    let mut events = chars("echo ");
    events.push(key(KeyCode::Up));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line("$ ", &mut history, &mut term).unwrap();
    // Up replaces buffer with "echo world" (most recent)
    assert_eq!(result, Some("echo world".to_string()));
}
```

- [ ] **Step 10: Clear suggestion on Up/Down in `handle_key()`**

In `src/interactive/line_editor.rs`, modify the Up and Down arms in `handle_key()`:

```rust
// Up — navigate history backward
(KeyCode::Up, _) => {
    if let Some(line) = history.navigate_up(&self.buffer()) {
        self.buf = line.chars().collect();
        self.pos = self.buf.len();
    }
    self.suggestion = None;
    KeyAction::Continue
}

// Down — navigate history forward
(KeyCode::Down, _) => {
    if let Some(line) = history.navigate_down() {
        self.buf = line.chars().collect();
        self.pos = self.buf.len();
    }
    self.suggestion = None;
    KeyAction::Continue
}
```

- [ ] **Step 11: Run all tests so far**

Run: `cargo test test_suggest`
Expected: All `test_suggest_*` tests pass.

- [ ] **Step 12: Commit**

```bash
git add src/interactive/line_editor.rs tests/interactive.rs
git commit -m "feat(line_editor): add autosuggestion state, update logic, and dim rendering"
```

---

### Task 4: LineEditor — Right Arrow / Ctrl+F Full Acceptance

**Files:**
- Modify: `src/interactive/line_editor.rs:189-291` (handle_key)
- Test: `tests/interactive.rs`

- [ ] **Step 1: Write failing test for Right Arrow acceptance**

Add to `tests/interactive.rs`:

```rust
#[test]
fn test_suggest_accept_full_with_right_arrow() {
    let mut history = History::new();
    history.add("git commit -m 'fix'", 500, "");

    // Type "git c", then Right (accept suggestion), then Enter
    let mut events = chars("git c");
    events.push(key(KeyCode::Right));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line("$ ", &mut history, &mut term).unwrap();
    assert_eq!(result, Some("git commit -m 'fix'".to_string()));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_suggest_accept_full_with_right_arrow -- --nocapture`
Expected: FAIL — result is `"git c"` because Right just moves cursor (no-op at end).

- [ ] **Step 3: Add `accept_full_suggestion()` method**

Add this method to the first `impl LineEditor` block in `src/interactive/line_editor.rs` (after `update_suggestion`):

```rust
/// Accept the full autosuggestion, appending it to the buffer.
fn accept_full_suggestion(&mut self) {
    if let Some(suggestion) = self.suggestion.take() {
        self.buf.extend(suggestion.chars());
        self.pos = self.buf.len();
    }
}
```

- [ ] **Step 4: Modify Right Arrow and Ctrl+F handlers in `handle_key()`**

In `src/interactive/line_editor.rs`, replace the Ctrl+F and Right handlers:

```rust
// Ctrl+F / Right — move cursor right, or accept suggestion at end
(KeyCode::Char('f'), m) if m.contains(KeyModifiers::CONTROL) => {
    if self.pos == self.buf.len() && self.suggestion.is_some() {
        self.accept_full_suggestion();
    } else {
        self.move_cursor_right();
    }
    KeyAction::Continue
}
(KeyCode::Right, _) => {
    if self.pos == self.buf.len() && self.suggestion.is_some() {
        self.accept_full_suggestion();
    } else {
        self.move_cursor_right();
    }
    KeyAction::Continue
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test test_suggest_accept_full_with_right_arrow -- --nocapture`
Expected: PASS

- [ ] **Step 6: Write test for Ctrl+F acceptance**

Add to `tests/interactive.rs`:

```rust
#[test]
fn test_suggest_accept_full_with_ctrl_f() {
    let mut history = History::new();
    history.add("cargo test --release", 500, "");

    // Type "cargo t", then Ctrl+F (accept suggestion), then Enter
    let mut events = chars("cargo t");
    events.push(ctrl('f'));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line("$ ", &mut history, &mut term).unwrap();
    assert_eq!(result, Some("cargo test --release".to_string()));
}
```

- [ ] **Step 7: Run test to verify it passes**

Run: `cargo test test_suggest_accept_full_with_ctrl_f -- --nocapture`
Expected: PASS (already implemented in Step 4).

- [ ] **Step 8: Write test that Right Arrow still works normally mid-line**

Add to `tests/interactive.rs`:

```rust
#[test]
fn test_right_arrow_normal_when_no_suggestion() {
    let mut history = History::new();
    history.add("git commit", 500, "");

    // Type "abc", Left, Right (normal cursor move), Enter
    let mut events = chars("abc");
    events.push(key(KeyCode::Left));
    events.push(key(KeyCode::Right));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line("$ ", &mut history, &mut term).unwrap();
    assert_eq!(result, Some("abc".to_string()));
}
```

- [ ] **Step 9: Run all suggestion tests**

Run: `cargo test test_suggest`
Expected: All tests pass.

- [ ] **Step 10: Commit**

```bash
git add src/interactive/line_editor.rs tests/interactive.rs
git commit -m "feat(line_editor): accept full autosuggestion with Right Arrow / Ctrl+F"
```

---

### Task 5: LineEditor — Alt+F Word-by-Word Acceptance

**Files:**
- Modify: `src/interactive/line_editor.rs:189-291` (handle_key)
- Test: `tests/interactive.rs`

- [ ] **Step 1: Write failing test for Alt+F single word acceptance**

Add to `tests/interactive.rs`:

First, add a helper to create Alt+char events after the existing helpers in `tests/helpers/mock_terminal.rs`:

```rust
/// Create an Alt+char key event.
pub fn alt(ch: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::ALT))
}
```

Then add the test to `tests/interactive.rs`:

```rust
use helpers::mock_terminal::{MockTerminal, alt, chars, ctrl, key};

#[test]
fn test_suggest_accept_word_with_alt_f() {
    let mut history = History::new();
    history.add("git commit -m 'fix'", 500, "");

    // Type "git", Alt+F (accept " commit"), then Enter
    let mut events = chars("git");
    events.push(alt('f'));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line("$ ", &mut history, &mut term).unwrap();
    assert_eq!(result, Some("git commit".to_string()));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_suggest_accept_word_with_alt_f -- --nocapture`
Expected: FAIL — result is `"git"` because Alt+F is not handled.

- [ ] **Step 3: Add `accept_word_suggestion()` method**

Add this method to the first `impl LineEditor` block in `src/interactive/line_editor.rs` (after `accept_full_suggestion`):

```rust
/// Accept the next word from the autosuggestion.
/// A "word" is defined as: any leading spaces + non-space characters up to the next space.
fn accept_word_suggestion(&mut self) {
    if let Some(suggestion) = self.suggestion.take() {
        let chars: Vec<char> = suggestion.chars().collect();
        let mut i = 0;
        // Skip leading spaces
        while i < chars.len() && chars[i] == ' ' {
            i += 1;
        }
        // Take non-space characters
        while i < chars.len() && chars[i] != ' ' {
            i += 1;
        }
        // Append the accepted portion to the buffer
        self.buf.extend(&chars[..i]);
        self.pos = self.buf.len();
        // Keep remaining suggestion, if any
        if i < chars.len() {
            self.suggestion = Some(chars[i..].iter().collect());
        }
    }
}
```

- [ ] **Step 4: Add Alt+F handler in `handle_key()`**

In `src/interactive/line_editor.rs`, add this arm in the `handle_key` match, before the existing printable character arm (the `(KeyCode::Char(ch), m) if !m.contains(KeyModifiers::CONTROL)` arm):

```rust
// Alt+F — accept next word from suggestion
(KeyCode::Char('f'), m) if m.contains(KeyModifiers::ALT) => {
    if self.suggestion.is_some() {
        self.accept_word_suggestion();
    }
    KeyAction::Continue
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test test_suggest_accept_word_with_alt_f -- --nocapture`
Expected: PASS

- [ ] **Step 6: Write test for Alt+F repeated stepwise acceptance**

Add to `tests/interactive.rs`:

```rust
#[test]
fn test_suggest_accept_word_stepwise() {
    let mut history = History::new();
    history.add("git commit -m 'fix'", 500, "");

    // Type "git", Alt+F three times (accept " commit", " -m", " 'fix'"), then Enter
    let mut events = chars("git");
    events.push(alt('f')); // accept " commit"
    events.push(alt('f')); // accept " -m"
    events.push(alt('f')); // accept " 'fix'"
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line("$ ", &mut history, &mut term).unwrap();
    assert_eq!(result, Some("git commit -m 'fix'".to_string()));
}
```

- [ ] **Step 7: Run test to verify it passes**

Run: `cargo test test_suggest_accept_word_stepwise -- --nocapture`
Expected: PASS

- [ ] **Step 8: Write test that Alt+F is no-op without suggestion**

Add to `tests/interactive.rs`:

```rust
#[test]
fn test_alt_f_noop_without_suggestion() {
    let history = History::new(); // empty history, no suggestions

    // Type "hello", Alt+F (no-op), Enter
    let mut events = chars("hello");
    events.push(alt('f'));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line("$ ", &mut history, &mut term).unwrap();
    assert_eq!(result, Some("hello".to_string()));
}
```

- [ ] **Step 9: Run all suggestion tests**

Run: `cargo test test_suggest`
Expected: All tests pass.

- [ ] **Step 10: Commit**

```bash
git add src/interactive/line_editor.rs tests/interactive.rs tests/helpers/mock_terminal.rs
git commit -m "feat(line_editor): accept autosuggestion word-by-word with Alt+F"
```

---

### Task 6: Backspace Updates Suggestion Test

**Files:**
- Test: `tests/interactive.rs`

- [ ] **Step 1: Write test that backspace updates suggestion**

Add to `tests/interactive.rs`:

```rust
#[test]
fn test_suggest_updates_on_backspace() {
    let mut history = History::new();
    history.add("echo hello", 500, "");
    history.add("echo world", 500, "");

    // Type "echo w" (suggests "orld"), Backspace (now "echo " suggests "world"), 
    // Right (accept "world"), Enter
    let mut events = chars("echo w");
    events.push(key(KeyCode::Backspace));
    events.push(key(KeyCode::Right)); // accept "world" (most recent match for "echo ")
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line("$ ", &mut history, &mut term).unwrap();
    assert_eq!(result, Some("echo world".to_string()));
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test test_suggest_updates_on_backspace -- --nocapture`
Expected: PASS (update_suggestion is already called after every key action).

- [ ] **Step 3: Commit**

```bash
git add tests/interactive.rs
git commit -m "test(interactive): add backspace suggestion update test"
```

---

### Task 7: Full Test Suite Verification

**Files:** (no changes — verification only)

- [ ] **Step 1: Run the entire test suite**

Run: `cargo test`
Expected: All tests pass, including all pre-existing tests. No regressions.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings or errors.

- [ ] **Step 3: Fix any issues found**

If clippy reports issues, fix them and run `cargo clippy -- -D warnings` again.

- [ ] **Step 4: Commit any fixes**

```bash
git add -A
git commit -m "fix: resolve clippy warnings from autosuggest implementation"
```

(Skip this step if no fixes were needed.)

---

### Task 8: PTY E2E Test

**Files:**
- Modify: `tests/pty_interactive.rs`

- [ ] **Step 1: Write PTY test for autosuggestion acceptance**

Add to `tests/pty_interactive.rs`:

```rust
#[test]
fn test_pty_autosuggest_accept_with_right_arrow() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    // Execute a command to populate history
    s.send("echo autosuggest_test_value\r").unwrap();
    expect_output(&mut s, "autosuggest_test_value", "initial echo failed");
    wait_for_prompt(&mut s);

    // Type prefix "echo auto" — suggestion should appear
    s.send("echo auto").unwrap();
    // Brief pause for suggestion to render
    std::thread::sleep(Duration::from_millis(50));

    // Press Right arrow to accept the suggestion
    s.send("\x1b[C").unwrap(); // Right arrow (ANSI escape)
    // Brief pause for acceptance
    std::thread::sleep(Duration::from_millis(50));

    // Press Enter to execute
    s.send("\r").unwrap();
    expect_output(&mut s, "autosuggest_test_value", "autosuggest acceptance failed");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}
```

- [ ] **Step 2: Run PTY test**

Run: `cargo test test_pty_autosuggest -- --nocapture`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add tests/pty_interactive.rs
git commit -m "test(pty): add E2E test for autosuggest acceptance with Right arrow"
```

---

### Task 9: Final Verification and Cleanup

**Files:** (no changes — verification only)

- [ ] **Step 1: Run the full test suite one final time**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 2: Verify the feature manually (optional)**

Run: `cargo run` to start kish interactively. Execute a few commands, then start typing a prefix of a previous command. Verify that:
- Dim ghost text appears after the cursor
- Right arrow accepts the full suggestion
- Alt+F accepts one word at a time
- The suggestion disappears when moving cursor left
- Up/Down arrow clears the suggestion
