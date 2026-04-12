# History-Based Inline Autosuggestion

## Overview

Add fish-style inline autosuggestion to the interactive mode. As the user types, a suggestion from command history appears in dim text to the right of the cursor. The user can accept the suggestion fully or word-by-word.

## Requirements

- **Source**: Command history only (no file path or PATH completion)
- **Matching**: Prefix match — the entire current buffer must match the beginning of a history entry
- **Priority**: Most recent matching entry wins
- **Display**: Dim/gray text to the right of the cursor, only when cursor is at end of line
- **Acceptance**:
  - Right Arrow / Ctrl+F — accept entire suggestion
  - Alt+F — accept next word from suggestion

## Architecture

### Approach: Hybrid (History search + LineEditor state)

- `History` gets a `suggest(prefix)` method for candidate search (data near logic)
- `LineEditor` manages suggestion display state and acceptance logic

### Data Flow

```
User input
    │
    ▼
LineEditor::handle_key()
    │
    ├─ Character input / Backspace / Delete / cursor movement
    │   → Update buffer
    │   → If cursor at end: call History::suggest(buffer) → update suggestion
    │   → If cursor not at end: clear suggestion
    │
    ├─ Right / Ctrl+F (cursor at end & suggestion exists)
    │   → Append entire suggestion to buffer, clear suggestion
    │
    ├─ Alt+F (suggestion exists)
    │   → Append next word from suggestion to buffer
    │   → Update or clear remaining suggestion
    │
    └─ Other keys → existing behavior
    │
    ▼
LineEditor::redraw()
    │
    ├─ Draw buffer text (existing)
    └─ If suggestion exists && cursor at end:
        → set_dim(true), write suggestion, set_dim(false)
        → Move cursor back to buffer end position
```

## Detailed Design

### History::suggest()

```rust
impl History {
    /// Return the suffix of the most recent history entry that starts with `prefix`.
    /// Returns None if prefix is empty, or if only exact matches exist.
    pub fn suggest(&self, prefix: &str) -> Option<String> {
        if prefix.is_empty() {
            return None;
        }
        // Iterate entries in reverse (newest first)
        // Find first entry that starts_with(prefix) && entry != prefix
        // Return entry[prefix.len()..]
    }
}
```

### LineEditor Changes

**New field:**

```rust
struct LineEditor {
    buf: Vec<char>,
    pos: usize,
    suggestion: Option<String>,  // NEW: suggestion text (the suffix portion)
}
```

**Suggestion update logic:**

After any buffer/cursor mutation in `handle_key()`:

```rust
fn update_suggestion(&mut self, history: &History) {
    if self.pos == self.buf.len() && !self.buf.is_empty() {
        self.suggestion = history.suggest(&self.buffer());
    } else {
        self.suggestion = None;
    }
}
```

**Suggestion clear triggers:**

- Buffer becomes empty
- Up/Down arrow (history navigation)
- Ctrl+R (fuzzy search)

### Key Handling

**Right Arrow / Ctrl+F (modified behavior):**

```
if cursor == buf.len() && suggestion.is_some():
    accept_full_suggestion()  // append suggestion to buf, clear suggestion
else:
    move_cursor_right()       // existing behavior
```

**Alt+F (new binding):**

```
if suggestion.is_some():
    accept_word_suggestion()
else:
    no-op (reserved for future Emacs-style forward-word)
```

**Word boundary definition:** Space-delimited. Leading spaces in the suggestion are included with the next word. For example, suggestion ` commit -m "fix"`:
- First Alt+F: accept ` commit` (up to next space boundary)
- Second Alt+F: accept ` -m` 
- Third Alt+F: accept ` "fix"` (remainder)

### Terminal Trait Extension

```rust
trait Terminal {
    // ... existing methods ...
    fn set_dim(&mut self, enabled: bool);  // NEW
}
```

- `CrosstermTerminal`: uses `crossterm::style::Attribute::Dim`
- `MockTerminal`: records flag for test assertions

### Redraw

After existing buffer drawing:

```rust
if let Some(ref suggestion) = self.suggestion {
    if self.pos == self.buf.len() {
        term.set_dim(true);
        term.write_str(suggestion);
        term.set_dim(false);
        // Move cursor back to buffer end position
        term.move_to_column((prompt_width + self.buf.len()) as u16);
    }
}
```

## Test Plan

### 1. History::suggest() Unit Tests (tests/history.rs)

- Prefix match returns suffix of most recent entry
- Exact match is excluded (returns None)
- Empty prefix returns None
- Multiple matches → most recent wins
- No match → None

### 2. LineEditor Key Handling Tests (tests/interactive.rs, MockTerminal)

- Character input triggers suggestion display
- Right Arrow accepts full suggestion → buffer becomes complete command
- Ctrl+F accepts full suggestion (same behavior as Right Arrow)
- Alt+F accepts one word → remaining suggestion continues
- Alt+F repeated → stepwise acceptance → suggestion becomes None
- Cursor not at end → no suggestion displayed
- Backspace updates suggestion
- Up/Down clears suggestion

### 3. Rendering Tests (MockTerminal output verification)

- set_dim(true) → suggestion text → set_dim(false) sequence
- Cursor returns to buffer end after suggestion drawing

### 4. PTY E2E Tests (tests/pty_interactive.rs)

- Add command to history → type prefix → press Right → verify completed command
- Focus on buffer content verification (dim rendering hard to verify via PTY)

## Files to Modify

| File | Changes |
|------|---------|
| `src/interactive/history.rs` | Add `suggest()` method |
| `src/interactive/line_editor.rs` | Add `suggestion` field, update key handling, modify redraw |
| `src/interactive/terminal.rs` | Add `set_dim()` to `Terminal` trait and implementations |
| `tests/helpers/mock_terminal.rs` | Implement `set_dim()` |
| `tests/history.rs` | Add suggest() unit tests |
| `tests/interactive.rs` | Add autosuggest key handling and rendering tests |
| `tests/pty_interactive.rs` | Add E2E autosuggest test |

## Out of Scope

- File path completion
- PATH/command name completion
- Frecency-based ranking (frequency + recency)
- Multiple suggestion candidates / cycling
- Configuration options (enable/disable, color customization)
