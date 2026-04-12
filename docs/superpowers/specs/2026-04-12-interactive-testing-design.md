# Interactive Mode Testing Design

**Date:** 2026-04-12
**Goal:** Enable comprehensive testing of kish's interactive mode, including line editing, history navigation, Ctrl+R fuzzy search, and REPL lifecycle.

## Background

Currently, interactive mode testing covers only pure logic (buffer manipulation, history data structure, fuzzy matching algorithm) via unit tests. The actual terminal I/O flow — `read_line()` blocking on `crossterm::event::read()`, `FuzzySearchUI::run()` rendering to stdout — is untestable because crossterm is used directly without abstraction.

### Current Test Gaps

- `LineEditor::read_line()` — blocks on real terminal events, no way to inject key sequences
- `FuzzySearchUI::run()` — requires real terminal for size queries and event loop
- `LineEditor::redraw()` — writes directly to stdout
- REPL lifecycle (prompt → input → execute → prompt) — no integration test
- Key binding integration (e.g., Ctrl+R invokes fuzzy search, result populates buffer) — untested

## Approach

Two-layer testing strategy:

- **Layer 1 (Unit/Integration):** Introduce a `Terminal` trait, mock terminal I/O, test key sequence → buffer state without real terminal
- **Layer 2 (E2E):** Use PTY via `expectrl` crate to drive the real kish process, verify end-to-end behavior including rendering

## Design

### 1. Terminal Trait

New file: `src/interactive/terminal.rs`

```rust
use std::io;
use crossterm::event::Event;

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
```

**Design decisions:**
- Uses `crossterm::event::Event` directly — crossterm is already pervasive in the codebase; defining custom key event types would add unnecessary indirection.
- `set_reverse(bool)` abstracts `Attribute::Reverse` / `Attribute::Reset` — these are the only style attributes currently used.
- `enable_raw_mode` / `disable_raw_mode` replace the `RawModeGuard` RAII pattern. The `CrosstermTerminal` implementation can handle cleanup internally if needed.

### 2. CrosstermTerminal (Production Implementation)

Defined in `src/interactive/terminal.rs`:

```rust
pub struct CrosstermTerminal {
    stdout: Stdout,
}
```

Wraps existing crossterm calls. Each trait method maps 1:1 to the current crossterm usage in `LineEditor` and `FuzzySearchUI`.

### 3. Refactoring LineEditor and FuzzySearchUI

**LineEditor changes:**
- `read_line(&mut self, prompt_width: usize, history: &mut History)` becomes `read_line<T: Terminal>(&mut self, prompt_width: usize, history: &mut History, term: &mut T)`
- `redraw(&self, stdout: &mut Stdout, prompt_width: usize)` becomes `redraw<T: Terminal>(&self, term: &mut T, prompt_width: usize)`
- `handle_key(&mut self, key: KeyEvent, history: &mut History) -> KeyAction` — unchanged (no Terminal dependency)
- `RawModeGuard` is removed; replaced by explicit `term.enable_raw_mode()` / `term.disable_raw_mode()` calls

**FuzzySearchUI changes:**
- `run(history: &History)` becomes `run<T: Terminal>(history: &History, term: &mut T)`
- `draw(&self, stdout: &mut Stdout)` becomes `draw<T: Terminal>(&self, term: &mut T)`
- `clear_ui(&self, stdout: &mut Stdout, draw_lines: usize)` becomes `clear_ui<T: Terminal>(&self, term: &mut T, draw_lines: usize)`
- `handle_key(&mut self, key: KeyEvent, entries: &[String]) -> SearchAction` — unchanged

**Repl changes:**
- Holds `CrosstermTerminal` as a field
- Passes `&mut self.terminal` to `read_line()` and (indirectly) to `FuzzySearchUI::run()`

### 4. MockTerminal (Test Implementation)

```rust
#[cfg(any(test, feature = "test-support"))]
pub mod mock {
    pub struct MockTerminal {
        events: VecDeque<Event>,
        size: (u16, u16),
        output: Vec<String>,
        raw_mode: bool,
    }
}
```

- **Event queue:** Events are consumed from front. `UnexpectedEof` error when exhausted — immediately catches unintended extra reads in tests.
- **Output recording:** `write_str()` appends to `output: Vec<String>` for optional verification.
- **Terminal size:** Configurable via `set_size()` — affects `FuzzySearchUI::max_visible` calculation.
- **No-op operations:** `move_to_column`, `move_up`, `clear_*`, `set_reverse`, `flush` return `Ok(())`.

**Helper functions:**

```rust
fn key(code: KeyCode) -> Event { ... }
fn ctrl(ch: char) -> Event { ... }
fn chars(s: &str) -> Vec<Event> { ... }
```

**Visibility:** Exposed via `test-support` feature flag so integration tests in `tests/` can access it:

```toml
[features]
test-support = []

[dev-dependencies]
kish = { path = ".", features = ["test-support"] }
```

### 5. PTY E2E Tests

**Library:** `expectrl` (added to dev-dependencies)

**Helper module:** `tests/pty/mod.rs`

```rust
fn spawn_kish() -> Session { ... }
fn wait_for_prompt(session: &mut Session) { ... }
fn send_and_expect(session: &mut Session, input: &str, expected: &str) { ... }
```

**CI stability measures:**
- 5-second timeout on all `expect` calls
- `TERM=dumb` environment variable to minimize escape sequences
- `HISTFILE=/dev/null` to prevent interference from existing history
- Each test uses an independent PTY session (parallelizable)
- Focus on verifying command output and behavior results, not exact screen rendering

### 6. File Structure

```
src/interactive/
├── mod.rs              # Changed: Repl holds CrosstermTerminal
├── terminal.rs         # New: Terminal trait + CrosstermTerminal + mock module
├── line_editor.rs      # Changed: read_line/redraw generic over <T: Terminal>
├── fuzzy_search.rs     # Changed: run/draw/clear_ui generic over <T: Terminal>
├── history.rs          # Unchanged
├── parse_status.rs     # Unchanged
└── prompt.rs           # Unchanged

tests/
├── interactive.rs      # Changed: existing tests preserved + MockTerminal tests added
├── pty/
│   ├── mod.rs          # New: PTY helpers
│   └── interactive.rs  # New: PTY E2E tests
└── ...                 # Existing tests unchanged

Cargo.toml              # Changed: add expectrl, test-support feature
```

### 7. Test Coverage Matrix

| Test Target | Layer 1 (MockTerminal) | Layer 2 (PTY E2E) |
|---|---|---|
| All key bindings | **Primary** — exhaustive | Representative only |
| Ctrl+R flow | **Primary** — event sequence | 1-2 cases with real UI |
| History navigation | **Primary** — Up/Down combos | 1 basic case |
| REPL lifecycle | — | **Primary** — prompt → exec → output |
| Prompt display (PS1/PS2) | — | **Primary** — correct rendering |
| Signal handling (Ctrl+C) | — | **Primary** — interrupt behavior |

### 8. Test Cases

**Layer 1 (MockTerminal):**

| Category | Test Case |
|---|---|
| Basic input | chars → Enter → buffer returned |
| Ctrl+C | returns empty string |
| Ctrl+D (empty) | returns None (EOF) |
| Ctrl+D (non-empty) | deletes char at cursor |
| Cursor movement | Ctrl+A/E/B/F, Home/End, Left/Right |
| Backspace/Delete | deletion at various positions |
| History Up/Down | Up→Up→Down→Enter result |
| History + edit | Up to recall → edit chars → Enter |
| Ctrl+R basic | Ctrl+R → query → Enter → buffer populated |
| Ctrl+R cancel | Ctrl+R → Esc → original buffer preserved |
| Ctrl+R navigation | Ctrl+R → query → Up/Down candidates → Enter |
| Ctrl+R backspace | Ctrl+R → type → Backspace → candidates update |

**Layer 2 (PTY E2E):**

| Category | Test Case |
|---|---|
| REPL cycle | prompt → echo → output → prompt |
| PS2 prompt | incomplete input (bare `if`) → PS2 → complete → execute |
| Ctrl+C | interrupt during input → new prompt |
| Ctrl+D | EOF on empty prompt → shell exits |
| Ctrl+R E2E | register commands → Ctrl+R → search → execute |
| History Up | execute command → Up → Enter → re-execute |
| Line editing | input → Backspace → correction → execute |

### 9. Implementation Order

1. Define `Terminal` trait + `CrosstermTerminal` in `terminal.rs`
2. Refactor `LineEditor` to `<T: Terminal>`
3. Refactor `FuzzySearchUI` to `<T: Terminal>`
4. Update `Repl` to hold and pass `CrosstermTerminal`
5. Implement `MockTerminal` + helper functions
6. Add Layer 1 tests (MockTerminal-based)
7. Add PTY helpers + Layer 2 tests (E2E)

Steps 1-4 must pass existing tests at each step. Steps 6-7 are independent of each other.

### 10. Dependencies Added

```toml
[dev-dependencies]
expectrl = "0.7"
```

No new production dependencies required.
