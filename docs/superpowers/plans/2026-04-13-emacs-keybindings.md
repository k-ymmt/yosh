# Emacs Keybindings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add readline-compatible Emacs keybindings to kish's interactive mode with kill ring, undo, numeric arguments, and word/case manipulation.

**Architecture:** Extract key dispatch from `LineEditor::handle_key()` into a `Keymap` that maps `KeyEvent` → `EditAction` enum. Add `KillRing` and `UndoManager` as independent modules. `LineEditor` owns all three and orchestrates the resolve → snapshot → execute flow.

**Tech Stack:** Rust (edition 2024), crossterm 0.29 for terminal events, existing `MockTerminal` + `helpers` in `tests/` for testing.

---

### Task 1: EditAction enum

**Files:**
- Create: `src/interactive/edit_action.rs`
- Modify: `src/interactive/mod.rs:1-8` (add module declaration)

- [ ] **Step 1: Create `edit_action.rs` with the enum**

```rust
// src/interactive/edit_action.rs

/// All editing operations that the line editor can perform.
/// Serves as the contract between Keymap (key → action) and LineEditor (action → mutation).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EditAction {
    // Character input
    InsertChar(char),

    // Cursor movement
    MoveBackward,
    MoveForward,
    MoveToStart,
    MoveToEnd,
    MoveBackwardWord,
    MoveForwardWord,

    // Delete (does NOT enter kill ring)
    DeleteBackward,
    DeleteForward,

    // Kill (enters kill ring)
    KillToEnd,
    KillToStart,
    KillBackwardWord,
    KillForwardWord,

    // Yank
    Yank,
    YankPop,

    // Editing
    TransposeChars,
    TransposeWords,
    UpcaseWord,
    DowncaseWord,
    CapitalizeWord,

    // Undo
    Undo,

    // Other
    ClearScreen,
    Cancel,
    AcceptSuggestion,
    AcceptWordSuggestion,
    SetNumericArg(u8),

    // Control (maps to KeyAction for REPL loop)
    Submit,
    Eof,
    Interrupt,
    FuzzySearch,
    TabComplete,
    HistoryPrev,
    HistoryNext,
    Noop,
}

impl EditAction {
    /// Returns true if this action is a kill operation (text goes to kill ring).
    pub fn is_kill(&self) -> bool {
        matches!(self, Self::KillToEnd | Self::KillToStart | Self::KillBackwardWord | Self::KillForwardWord)
    }
}
```

- [ ] **Step 2: Register module in `mod.rs`**

Add `pub mod edit_action;` to `src/interactive/mod.rs` after the existing module declarations.

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | head -20`
Expected: successful build (warnings about unused are fine)

- [ ] **Step 4: Commit**

```bash
git add src/interactive/edit_action.rs src/interactive/mod.rs
git commit -m "feat(interactive): add EditAction enum for keybinding dispatch"
```

---

### Task 2: KillRing

**Files:**
- Create: `src/interactive/kill_ring.rs`
- Modify: `src/interactive/mod.rs` (add module declaration)
- Test: `tests/interactive.rs` (add kill ring unit tests)

- [ ] **Step 1: Write failing tests for KillRing**

Add to `tests/interactive.rs`:

```rust
// ── Kill ring tests ───────────────────────────────────────────────────

#[test]
fn test_kill_ring_kill_and_yank() {
    use kish::interactive::kill_ring::KillRing;
    let mut kr = KillRing::new(60);
    kr.kill("hello", false);
    assert_eq!(kr.yank(), Some("hello"));
}

#[test]
fn test_kill_ring_multiple_kills() {
    use kish::interactive::kill_ring::KillRing;
    let mut kr = KillRing::new(60);
    kr.kill("first", false);
    kr.kill("second", false);
    assert_eq!(kr.yank(), Some("second"));
}

#[test]
fn test_kill_ring_append_forward() {
    use kish::interactive::kill_ring::KillRing;
    let mut kr = KillRing::new(60);
    kr.kill("hello", false);
    kr.kill(" world", true);
    assert_eq!(kr.yank(), Some("hello world"));
}

#[test]
fn test_kill_ring_yank_pop_cycles() {
    use kish::interactive::kill_ring::KillRing;
    let mut kr = KillRing::new(60);
    kr.kill("first", false);
    kr.kill("second", false);
    kr.kill("third", false);
    assert_eq!(kr.yank(), Some("third"));
    assert_eq!(kr.yank_pop(), Some("second"));
    assert_eq!(kr.yank_pop(), Some("first"));
    // Wraps around
    assert_eq!(kr.yank_pop(), Some("third"));
}

#[test]
fn test_kill_ring_yank_empty() {
    use kish::interactive::kill_ring::KillRing;
    let mut kr = KillRing::new(60);
    assert_eq!(kr.yank(), None);
}

#[test]
fn test_kill_ring_yank_pop_empty() {
    use kish::interactive::kill_ring::KillRing;
    let mut kr = KillRing::new(60);
    assert_eq!(kr.yank_pop(), None);
}

#[test]
fn test_kill_ring_max_size() {
    use kish::interactive::kill_ring::KillRing;
    let mut kr = KillRing::new(3);
    kr.kill("a", false);
    kr.kill("b", false);
    kr.kill("c", false);
    kr.kill("d", false);
    // "a" should have been evicted
    assert_eq!(kr.yank(), Some("d"));
    assert_eq!(kr.yank_pop(), Some("c"));
    assert_eq!(kr.yank_pop(), Some("b"));
    // Wraps: back to "d" (only 3 entries)
    assert_eq!(kr.yank_pop(), Some("d"));
}

#[test]
fn test_kill_ring_prepend() {
    use kish::interactive::kill_ring::KillRing;
    let mut kr = KillRing::new(60);
    kr.kill("world", false);
    kr.prepend("hello ", true);
    assert_eq!(kr.yank(), Some("hello world"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_kill_ring 2>&1 | tail -20`
Expected: compilation error — `kill_ring` module doesn't exist

- [ ] **Step 3: Create `kill_ring.rs`**

```rust
// src/interactive/kill_ring.rs

use std::collections::VecDeque;

/// Circular buffer of killed (cut) text, supporting yank and yank-pop.
pub struct KillRing {
    ring: VecDeque<String>,
    max_size: usize,
    yank_index: usize,
}

impl KillRing {
    pub fn new(max_size: usize) -> Self {
        Self {
            ring: VecDeque::new(),
            max_size,
            yank_index: 0,
        }
    }

    /// Add text to the kill ring.
    /// If `append` is true, concatenate to the most recent entry (for consecutive forward kills).
    pub fn kill(&mut self, text: &str, append: bool) {
        if text.is_empty() {
            return;
        }
        if append && !self.ring.is_empty() {
            let front = self.ring.front_mut().unwrap();
            front.push_str(text);
        } else {
            self.ring.push_front(text.to_string());
            if self.ring.len() > self.max_size {
                self.ring.pop_back();
            }
        }
        self.yank_index = 0;
    }

    /// Prepend text to the most recent entry (for consecutive backward kills).
    /// If `append` is false or ring is empty, behaves like `kill()`.
    pub fn prepend(&mut self, text: &str, append: bool) {
        if text.is_empty() {
            return;
        }
        if append && !self.ring.is_empty() {
            let front = self.ring.front_mut().unwrap();
            front.insert_str(0, text);
        } else {
            self.ring.push_front(text.to_string());
            if self.ring.len() > self.max_size {
                self.ring.pop_back();
            }
        }
        self.yank_index = 0;
    }

    /// Return the most recent kill (for Ctrl+Y). Resets yank_index to 0.
    pub fn yank(&mut self) -> Option<&str> {
        if self.ring.is_empty() {
            return None;
        }
        self.yank_index = 0;
        Some(self.ring[0].as_str())
    }

    /// Cycle to the next older entry in the kill ring (for Alt+Y).
    /// Returns None if the ring is empty.
    pub fn yank_pop(&mut self) -> Option<&str> {
        if self.ring.is_empty() {
            return None;
        }
        self.yank_index = (self.yank_index + 1) % self.ring.len();
        Some(self.ring[self.yank_index].as_str())
    }
}
```

- [ ] **Step 4: Register module in `mod.rs`**

Add `pub mod kill_ring;` to `src/interactive/mod.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test test_kill_ring 2>&1 | tail -20`
Expected: all 8 tests pass

- [ ] **Step 6: Commit**

```bash
git add src/interactive/kill_ring.rs src/interactive/mod.rs tests/interactive.rs
git commit -m "feat(interactive): add KillRing with append/prepend and yank-pop cycling"
```

---

### Task 3: UndoManager

**Files:**
- Create: `src/interactive/undo.rs`
- Modify: `src/interactive/mod.rs` (add module declaration)
- Test: `tests/interactive.rs` (add undo tests)

- [ ] **Step 1: Write failing tests for UndoManager**

Add to `tests/interactive.rs`:

```rust
// ── Undo manager tests ────────────────────────────────────────────────

#[test]
fn test_undo_save_and_restore() {
    use kish::interactive::undo::UndoManager;
    let mut um = UndoManager::new(256);
    um.save(&['a', 'b', 'c'], 3);
    um.save(&['a', 'b', 'c', 'd'], 4);
    let (buf, pos) = um.undo().unwrap();
    assert_eq!(buf, vec!['a', 'b', 'c']);
    assert_eq!(pos, 3);
}

#[test]
fn test_undo_multiple() {
    use kish::interactive::undo::UndoManager;
    let mut um = UndoManager::new(256);
    um.save(&[], 0);
    um.save(&['a'], 1);
    um.save(&['a', 'b'], 2);
    let (buf, pos) = um.undo().unwrap();
    assert_eq!(buf, vec!['a']);
    assert_eq!(pos, 1);
    let (buf, pos) = um.undo().unwrap();
    assert_eq!(buf, vec![]);
    assert_eq!(pos, 0);
}

#[test]
fn test_undo_empty_returns_none() {
    use kish::interactive::undo::UndoManager;
    let mut um = UndoManager::new(256);
    assert!(um.undo().is_none());
}

#[test]
fn test_undo_clear() {
    use kish::interactive::undo::UndoManager;
    let mut um = UndoManager::new(256);
    um.save(&['a'], 1);
    um.clear();
    assert!(um.undo().is_none());
}

#[test]
fn test_undo_max_size() {
    use kish::interactive::undo::UndoManager;
    let mut um = UndoManager::new(3);
    um.save(&['a'], 1);
    um.save(&['b'], 1);
    um.save(&['c'], 1);
    um.save(&['d'], 1);
    // Only 3 entries: d, c, b (a was evicted)
    assert!(um.undo().is_some()); // c
    assert!(um.undo().is_some()); // b
    assert!(um.undo().is_some()); // a — wait, let's think again.
    // After save x4 with max 3: stack has [a, b, c, d] but capped at 3 → [b, c, d]
    // undo() pops d → returns c state; pops c → returns b state; pops b → None? No.
    // Actually stack = [b, c, d], undo pops d → (c,1), pops c → (b,1), pops b → None? 
    // No: undo pops the top and returns it. Let me re-check.
    // Stack after 4 saves with max 3: oldest (a) evicted → [b, c, d]
    // undo() → pop d, but we return the state TO RESTORE, which is the entry.
    // Hmm, the semantics: save() saves the state BEFORE a change.
    // So stack = [state_before_b, state_before_c, state_before_d]
    // undo() pops state_before_d = (['c'], 1)
    // undo() pops state_before_c = (['b'], 1) 
    // undo() pops state_before_b = (['a'], 1)
    // undo() → None
}
```

Actually, let me simplify the max size test to be clearer:

```rust
#[test]
fn test_undo_max_size() {
    use kish::interactive::undo::UndoManager;
    let mut um = UndoManager::new(2);
    um.save(&[], 0);       // state 0
    um.save(&['a'], 1);    // state 1
    um.save(&['a', 'b'], 2); // state 2 — evicts state 0
    // Stack: [state 1, state 2]
    let (buf, _) = um.undo().unwrap(); // restores state 1
    assert_eq!(buf, vec!['a']);
    let (buf, _) = um.undo().unwrap(); // restores state 0 — wait, state 0 was evicted
    // Actually with max_size=2, we keep the 2 most recent: state 1 and state 2
    // undo() pops state 2 → (['a'], 1)
    // undo() pops state 1 → ([], 0)
    // undo() → None
}
```

Let me rewrite the tests cleanly now.

- [ ] **Step 1 (revised): Write failing tests for UndoManager**

Add to `tests/interactive.rs`:

```rust
// ── Undo manager tests ────────────────────────────────────────────────

#[test]
fn test_undo_save_and_restore() {
    use kish::interactive::undo::UndoManager;
    let mut um = UndoManager::new(256);
    um.save(&['h', 'e', 'l', 'l', 'o'], 5);
    // After some edit, undo restores saved state
    let (buf, pos) = um.undo().unwrap();
    assert_eq!(buf, vec!['h', 'e', 'l', 'l', 'o']);
    assert_eq!(pos, 5);
}

#[test]
fn test_undo_multiple_states() {
    use kish::interactive::undo::UndoManager;
    let mut um = UndoManager::new(256);
    um.save(&[], 0);
    um.save(&['a'], 1);
    let (buf, pos) = um.undo().unwrap();
    assert_eq!(buf, vec!['a']);
    assert_eq!(pos, 1);
    let (buf, pos) = um.undo().unwrap();
    assert_eq!(buf, vec![]);
    assert_eq!(pos, 0);
    assert!(um.undo().is_none());
}

#[test]
fn test_undo_empty_returns_none() {
    use kish::interactive::undo::UndoManager;
    let mut um = UndoManager::new(256);
    assert!(um.undo().is_none());
}

#[test]
fn test_undo_clear_resets_stack() {
    use kish::interactive::undo::UndoManager;
    let mut um = UndoManager::new(256);
    um.save(&['a'], 1);
    um.save(&['a', 'b'], 2);
    um.clear();
    assert!(um.undo().is_none());
}

#[test]
fn test_undo_respects_max_size() {
    use kish::interactive::undo::UndoManager;
    let mut um = UndoManager::new(2);
    um.save(&[], 0);
    um.save(&['a'], 1);
    um.save(&['a', 'b'], 2); // evicts ([], 0)
    let (buf, _) = um.undo().unwrap();
    assert_eq!(buf, vec!['a', 'b']);
    let (buf, _) = um.undo().unwrap();
    assert_eq!(buf, vec!['a']);
    assert!(um.undo().is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_undo 2>&1 | tail -20`
Expected: compilation error — `undo` module doesn't exist

- [ ] **Step 3: Create `undo.rs`**

```rust
// src/interactive/undo.rs

/// A snapshot of the line buffer state at a point in time.
struct UndoEntry {
    buf: Vec<char>,
    pos: usize,
}

/// Manages undo history as a stack of buffer snapshots.
pub struct UndoManager {
    stack: Vec<UndoEntry>,
    max_size: usize,
}

impl UndoManager {
    pub fn new(max_size: usize) -> Self {
        Self {
            stack: Vec::new(),
            max_size,
        }
    }

    /// Save the current buffer state before a modification.
    pub fn save(&mut self, buf: &[char], pos: usize) {
        if self.stack.len() >= self.max_size {
            self.stack.remove(0);
        }
        self.stack.push(UndoEntry {
            buf: buf.to_vec(),
            pos,
        });
    }

    /// Restore the most recently saved state. Returns `None` if the stack is empty.
    pub fn undo(&mut self) -> Option<(Vec<char>, usize)> {
        self.stack.pop().map(|entry| (entry.buf, entry.pos))
    }

    /// Clear all undo history (called on Submit).
    pub fn clear(&mut self) {
        self.stack.clear();
    }
}
```

- [ ] **Step 4: Register module in `mod.rs`**

Add `pub mod undo;` to `src/interactive/mod.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test test_undo 2>&1 | tail -20`
Expected: all 5 tests pass

- [ ] **Step 6: Commit**

```bash
git add src/interactive/undo.rs src/interactive/mod.rs tests/interactive.rs
git commit -m "feat(interactive): add UndoManager with snapshot-based undo stack"
```

---

### Task 4: Keymap

**Files:**
- Create: `src/interactive/keymap.rs`
- Modify: `src/interactive/mod.rs` (add module declaration)
- Test: `tests/interactive.rs` (add keymap tests)

- [ ] **Step 1: Write failing tests for Keymap**

Add to `tests/interactive.rs`:

```rust
// ── Keymap tests ──────────────────────────────────────────────────────

use kish::interactive::edit_action::EditAction;
use kish::interactive::keymap::{BufferState, Keymap};

fn default_state() -> BufferState {
    BufferState {
        is_empty: false,
        at_end: false,
        has_suggestion: false,
        last_action: EditAction::Noop,
    }
}

fn key_event(code: KeyCode, modifiers: crossterm::event::KeyModifiers) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, modifiers)
}

#[test]
fn test_keymap_ctrl_k() {
    let mut km = Keymap::new();
    let (action, count) = km.resolve(
        key_event(KeyCode::Char('k'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::KillToEnd);
    assert_eq!(count, 1);
}

#[test]
fn test_keymap_ctrl_u() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('u'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::KillToStart);
}

#[test]
fn test_keymap_ctrl_w() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('w'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::KillBackwardWord);
}

#[test]
fn test_keymap_ctrl_y() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('y'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::Yank);
}

#[test]
fn test_keymap_alt_y_after_yank() {
    let mut km = Keymap::new();
    let state = BufferState {
        last_action: EditAction::Yank,
        ..default_state()
    };
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('y'), crossterm::event::KeyModifiers::ALT),
        &state,
    );
    assert_eq!(action, EditAction::YankPop);
}

#[test]
fn test_keymap_alt_y_without_yank() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('y'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::Noop);
}

#[test]
fn test_keymap_alt_b() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('b'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::MoveBackwardWord);
}

#[test]
fn test_keymap_alt_d() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('d'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::KillForwardWord);
}

#[test]
fn test_keymap_ctrl_t() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('t'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::TransposeChars);
}

#[test]
fn test_keymap_alt_t() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('t'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::TransposeWords);
}

#[test]
fn test_keymap_alt_u() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('u'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::UpcaseWord);
}

#[test]
fn test_keymap_alt_l() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('l'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::DowncaseWord);
}

#[test]
fn test_keymap_alt_c() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('c'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::CapitalizeWord);
}

#[test]
fn test_keymap_ctrl_underscore() {
    let mut km = Keymap::new();
    // Ctrl+_ is typically reported as KeyCode::Char('_') with CONTROL
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('_'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::Undo);
}

#[test]
fn test_keymap_ctrl_l() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('l'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::ClearScreen);
}

#[test]
fn test_keymap_ctrl_g_cancel() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('g'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::Cancel);
}

#[test]
fn test_keymap_ctrl_d_empty_is_eof() {
    let mut km = Keymap::new();
    let state = BufferState { is_empty: true, ..default_state() };
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('d'), crossterm::event::KeyModifiers::CONTROL),
        &state,
    );
    assert_eq!(action, EditAction::Eof);
}

#[test]
fn test_keymap_ctrl_d_nonempty_is_delete() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('d'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::DeleteForward);
}

#[test]
fn test_keymap_right_with_suggestion_accepts() {
    let mut km = Keymap::new();
    let state = BufferState {
        at_end: true,
        has_suggestion: true,
        ..default_state()
    };
    let (action, _) = km.resolve(
        key_event(KeyCode::Right, crossterm::event::KeyModifiers::empty()),
        &state,
    );
    assert_eq!(action, EditAction::AcceptSuggestion);
}

#[test]
fn test_keymap_right_without_suggestion_moves() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Right, crossterm::event::KeyModifiers::empty()),
        &default_state(),
    );
    assert_eq!(action, EditAction::MoveForward);
}

#[test]
fn test_keymap_alt_f_with_suggestion_accepts_word() {
    let mut km = Keymap::new();
    let state = BufferState {
        has_suggestion: true,
        ..default_state()
    };
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('f'), crossterm::event::KeyModifiers::ALT),
        &state,
    );
    assert_eq!(action, EditAction::AcceptWordSuggestion);
}

#[test]
fn test_keymap_alt_f_without_suggestion_moves_word() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('f'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::MoveForwardWord);
}

#[test]
fn test_keymap_numeric_arg() {
    let mut km = Keymap::new();
    // Alt+3
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('3'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::SetNumericArg(3));
    assert_eq!(km.pending_numeric_arg(), Some(3));

    // Then Ctrl+F should have repeat_count = 3
    let (action, count) = km.resolve(
        key_event(KeyCode::Char('f'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::MoveForward);
    assert_eq!(count, 3);
    assert_eq!(km.pending_numeric_arg(), None);
}

#[test]
fn test_keymap_numeric_arg_multi_digit() {
    let mut km = Keymap::new();
    // Alt+1 then Alt+5 → numeric_arg = 15
    km.resolve(
        key_event(KeyCode::Char('1'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    km.resolve(
        key_event(KeyCode::Char('5'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(km.pending_numeric_arg(), Some(15));
}

#[test]
fn test_keymap_ctrl_g_resets_numeric_arg() {
    let mut km = Keymap::new();
    km.resolve(
        key_event(KeyCode::Char('5'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(km.pending_numeric_arg(), Some(5));
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('g'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::Cancel);
    assert_eq!(km.pending_numeric_arg(), None);
}

#[test]
fn test_keymap_existing_bindings_preserved() {
    let mut km = Keymap::new();
    // Ctrl+A → MoveToStart
    let (a, _) = km.resolve(
        key_event(KeyCode::Char('a'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(a, EditAction::MoveToStart);

    // Ctrl+E → MoveToEnd
    let (a, _) = km.resolve(
        key_event(KeyCode::Char('e'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(a, EditAction::MoveToEnd);

    // Ctrl+B → MoveBackward
    let (a, _) = km.resolve(
        key_event(KeyCode::Char('b'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(a, EditAction::MoveBackward);

    // Enter → Submit
    let (a, _) = km.resolve(
        key_event(KeyCode::Enter, crossterm::event::KeyModifiers::empty()),
        &default_state(),
    );
    assert_eq!(a, EditAction::Submit);

    // Ctrl+C → Interrupt
    let (a, _) = km.resolve(
        key_event(KeyCode::Char('c'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(a, EditAction::Interrupt);

    // Ctrl+R → FuzzySearch
    let (a, _) = km.resolve(
        key_event(KeyCode::Char('r'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(a, EditAction::FuzzySearch);
}

#[test]
fn test_keymap_alt_backspace() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Backspace, crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::KillBackwardWord);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_keymap 2>&1 | tail -20`
Expected: compilation error — `keymap` module doesn't exist

- [ ] **Step 3: Create `keymap.rs`**

```rust
// src/interactive/keymap.rs

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::edit_action::EditAction;

/// Minimal buffer state needed by the keymap to make context-dependent decisions.
pub struct BufferState {
    pub is_empty: bool,
    pub at_end: bool,
    pub has_suggestion: bool,
    pub last_action: EditAction,
}

/// Maps key events to edit actions. Manages numeric argument accumulation.
pub struct Keymap {
    numeric_arg: Option<u32>,
}

impl Keymap {
    pub fn new() -> Self {
        Self { numeric_arg: None }
    }

    /// Return the currently accumulated numeric argument, if any.
    pub fn pending_numeric_arg(&self) -> Option<u32> {
        self.numeric_arg
    }

    /// Resolve a key event into an edit action and repeat count.
    /// Consumes any pending numeric argument as the repeat count.
    pub fn resolve(&mut self, key: KeyEvent, state: &BufferState) -> (EditAction, u32) {
        let mods = key.modifiers;
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        let alt = mods.contains(KeyModifiers::ALT);

        // Alt+digit → accumulate numeric argument
        if alt && !ctrl {
            if let KeyCode::Char(ch) = key.code {
                if let Some(digit) = ch.to_digit(10) {
                    let current = self.numeric_arg.unwrap_or(0);
                    self.numeric_arg = Some(current * 10 + digit);
                    return (EditAction::SetNumericArg(digit as u8), 1);
                }
            }
        }

        // Ctrl+G → cancel (reset numeric arg)
        if ctrl && key.code == KeyCode::Char('g') {
            self.numeric_arg = None;
            return (EditAction::Cancel, 1);
        }

        // Consume numeric arg for repeat count
        let count = self.numeric_arg.take().unwrap_or(1);

        let action = match (key.code, ctrl, alt) {
            // --- Control keybindings ---
            (KeyCode::Char('a'), true, false) => EditAction::MoveToStart,
            (KeyCode::Char('b'), true, false) => EditAction::MoveBackward,
            (KeyCode::Char('c'), true, false) => EditAction::Interrupt,
            (KeyCode::Char('d'), true, false) => {
                if state.is_empty { EditAction::Eof } else { EditAction::DeleteForward }
            }
            (KeyCode::Char('e'), true, false) => EditAction::MoveToEnd,
            (KeyCode::Char('f'), true, false) => {
                if state.at_end && state.has_suggestion {
                    EditAction::AcceptSuggestion
                } else {
                    EditAction::MoveForward
                }
            }
            (KeyCode::Char('k'), true, false) => EditAction::KillToEnd,
            (KeyCode::Char('l'), true, false) => EditAction::ClearScreen,
            (KeyCode::Char('r'), true, false) => EditAction::FuzzySearch,
            (KeyCode::Char('t'), true, false) => EditAction::TransposeChars,
            (KeyCode::Char('u'), true, false) => EditAction::KillToStart,
            (KeyCode::Char('w'), true, false) => EditAction::KillBackwardWord,
            (KeyCode::Char('y'), true, false) => EditAction::Yank,
            (KeyCode::Char('_'), true, false) => EditAction::Undo,

            // --- Alt keybindings ---
            (KeyCode::Char('b'), false, true) => EditAction::MoveBackwardWord,
            (KeyCode::Char('c'), false, true) => EditAction::CapitalizeWord,
            (KeyCode::Char('d'), false, true) => EditAction::KillForwardWord,
            (KeyCode::Char('f'), false, true) => {
                if state.has_suggestion {
                    EditAction::AcceptWordSuggestion
                } else {
                    EditAction::MoveForwardWord
                }
            }
            (KeyCode::Char('l'), false, true) => EditAction::DowncaseWord,
            (KeyCode::Char('t'), false, true) => EditAction::TransposeWords,
            (KeyCode::Char('u'), false, true) => EditAction::UpcaseWord,
            (KeyCode::Char('y'), false, true) => {
                if state.last_action == EditAction::Yank || state.last_action == EditAction::YankPop {
                    EditAction::YankPop
                } else {
                    EditAction::Noop
                }
            }
            (KeyCode::Backspace, false, true) => EditAction::KillBackwardWord,

            // --- Plain keys ---
            (KeyCode::Enter, false, false) => EditAction::Submit,
            (KeyCode::Backspace, false, false) => EditAction::DeleteBackward,
            (KeyCode::Delete, false, false) => EditAction::DeleteForward,
            (KeyCode::Tab, false, false) => EditAction::TabComplete,
            (KeyCode::Home, false, false) => EditAction::MoveToStart,
            (KeyCode::End, false, false) => EditAction::MoveToEnd,
            (KeyCode::Left, _, _) => EditAction::MoveBackward,
            (KeyCode::Right, _, _) => {
                if state.at_end && state.has_suggestion {
                    EditAction::AcceptSuggestion
                } else {
                    EditAction::MoveForward
                }
            }
            (KeyCode::Up, _, _) => EditAction::HistoryPrev,
            (KeyCode::Down, _, _) => EditAction::HistoryNext,

            // --- Printable character (no Ctrl) ---
            (KeyCode::Char(ch), false, false) => EditAction::InsertChar(ch),

            // --- Everything else ---
            _ => EditAction::Noop,
        };

        (action, count)
    }
}
```

- [ ] **Step 4: Register module in `mod.rs`**

Add `pub mod keymap;` to `src/interactive/mod.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test test_keymap 2>&1 | tail -30`
Expected: all keymap tests pass

- [ ] **Step 6: Commit**

```bash
git add src/interactive/keymap.rs src/interactive/mod.rs tests/interactive.rs
git commit -m "feat(interactive): add Keymap for KeyEvent to EditAction resolution"
```

---

### Task 5: Word boundary helpers

**Files:**
- Modify: `src/interactive/line_editor.rs` (add word boundary methods)
- Test: `tests/interactive.rs` (add word movement/kill tests)

These helpers are used by word movement, word kill, word case, and word transpose operations. We add them to `LineEditor` now so subsequent tasks can use them.

- [ ] **Step 1: Write failing tests**

Add to `tests/interactive.rs`:

```rust
// ── Word boundary tests ───────────────────────────────────────────────

#[test]
fn test_move_backward_word() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() { ed.insert_char(ch); }
    // Cursor at end (pos=11)
    ed.move_backward_word();
    assert_eq!(ed.cursor(), 6); // before 'w'
    ed.move_backward_word();
    assert_eq!(ed.cursor(), 0); // before 'h'
    ed.move_backward_word();
    assert_eq!(ed.cursor(), 0); // stays at 0
}

#[test]
fn test_move_forward_word() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() { ed.insert_char(ch); }
    ed.move_to_start();
    ed.move_forward_word();
    assert_eq!(ed.cursor(), 5); // after 'o' in "hello"
    ed.move_forward_word();
    assert_eq!(ed.cursor(), 11); // after 'd' in "world"
    ed.move_forward_word();
    assert_eq!(ed.cursor(), 11); // stays at end
}

#[test]
fn test_move_backward_word_with_multiple_spaces() {
    let mut ed = LineEditor::new();
    for ch in "foo   bar".chars() { ed.insert_char(ch); }
    ed.move_backward_word();
    assert_eq!(ed.cursor(), 6); // before 'b'
}

#[test]
fn test_move_forward_word_with_symbols() {
    let mut ed = LineEditor::new();
    for ch in "foo--bar".chars() { ed.insert_char(ch); }
    ed.move_to_start();
    ed.move_forward_word();
    assert_eq!(ed.cursor(), 3); // after "foo"
    ed.move_forward_word();
    assert_eq!(ed.cursor(), 8); // after "bar"
}

#[test]
fn test_kill_to_end() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() { ed.insert_char(ch); }
    ed.move_to_start();
    for _ in 0..5 { ed.move_cursor_right(); } // pos = 5
    let killed = ed.kill_to_end();
    assert_eq!(ed.buffer(), "hello");
    assert_eq!(killed, " world");
}

#[test]
fn test_kill_to_start() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() { ed.insert_char(ch); }
    ed.move_to_start();
    for _ in 0..5 { ed.move_cursor_right(); } // pos = 5
    let killed = ed.kill_to_start();
    assert_eq!(ed.buffer(), " world");
    assert_eq!(ed.cursor(), 0);
    assert_eq!(killed, "hello");
}

#[test]
fn test_kill_backward_word() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() { ed.insert_char(ch); }
    let killed = ed.kill_backward_word();
    assert_eq!(ed.buffer(), "hello ");
    assert_eq!(killed, "world");
}

#[test]
fn test_kill_forward_word() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() { ed.insert_char(ch); }
    ed.move_to_start();
    let killed = ed.kill_forward_word();
    assert_eq!(ed.buffer(), " world");
    assert_eq!(killed, "hello");
}

#[test]
fn test_transpose_chars_middle() {
    let mut ed = LineEditor::new();
    for ch in "abc".chars() { ed.insert_char(ch); }
    ed.move_to_start();
    ed.move_cursor_right(); // pos = 1 (on 'b')
    ed.transpose_chars();
    assert_eq!(ed.buffer(), "bac");
    assert_eq!(ed.cursor(), 2);
}

#[test]
fn test_transpose_chars_at_end() {
    let mut ed = LineEditor::new();
    for ch in "abc".chars() { ed.insert_char(ch); }
    ed.transpose_chars();
    assert_eq!(ed.buffer(), "acb");
    assert_eq!(ed.cursor(), 3);
}

#[test]
fn test_transpose_chars_at_start_noop() {
    let mut ed = LineEditor::new();
    for ch in "abc".chars() { ed.insert_char(ch); }
    ed.move_to_start();
    ed.transpose_chars();
    assert_eq!(ed.buffer(), "abc");
    assert_eq!(ed.cursor(), 0);
}

#[test]
fn test_upcase_word() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() { ed.insert_char(ch); }
    ed.move_to_start();
    ed.upcase_word();
    assert_eq!(ed.buffer(), "HELLO world");
    assert_eq!(ed.cursor(), 5);
}

#[test]
fn test_downcase_word() {
    let mut ed = LineEditor::new();
    for ch in "HELLO WORLD".chars() { ed.insert_char(ch); }
    ed.move_to_start();
    ed.downcase_word();
    assert_eq!(ed.buffer(), "hello WORLD");
    assert_eq!(ed.cursor(), 5);
}

#[test]
fn test_capitalize_word() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() { ed.insert_char(ch); }
    ed.move_to_start();
    ed.capitalize_word();
    assert_eq!(ed.buffer(), "Hello world");
    assert_eq!(ed.cursor(), 5);
}

#[test]
fn test_transpose_words() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() { ed.insert_char(ch); }
    // Cursor at end — should swap the two words
    ed.transpose_words();
    assert_eq!(ed.buffer(), "world hello");
    assert_eq!(ed.cursor(), 11);
}

#[test]
fn test_transpose_words_cursor_in_middle() {
    let mut ed = LineEditor::new();
    for ch in "aaa bbb ccc".chars() { ed.insert_char(ch); }
    ed.move_to_start();
    for _ in 0..5 { ed.move_cursor_right(); } // pos=5, in "bbb"
    ed.transpose_words();
    assert_eq!(ed.buffer(), "bbb aaa ccc");
    assert_eq!(ed.cursor(), 7); // after "aaa"
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_move_backward_word test_move_forward_word test_kill_to_end test_kill_to_start test_kill_backward_word test_kill_forward_word test_transpose_chars test_upcase_word test_downcase_word test_capitalize_word test_transpose_words 2>&1 | tail -20`
Expected: compilation errors — methods don't exist on `LineEditor`

- [ ] **Step 3: Add word boundary helper and all word/kill/transpose/case methods to `LineEditor`**

Add to `src/interactive/line_editor.rs`, inside the `impl LineEditor` block (after `move_to_end`):

```rust
    /// Returns true if `ch` is a word character (alphanumeric or underscore).
    fn is_word_char(ch: char) -> bool {
        ch.is_alphanumeric() || ch == '_'
    }

    /// Move cursor backward to the start of the previous word.
    pub fn move_backward_word(&mut self) {
        // Skip non-word chars
        while self.pos > 0 && !Self::is_word_char(self.buf[self.pos - 1]) {
            self.pos -= 1;
        }
        // Skip word chars
        while self.pos > 0 && Self::is_word_char(self.buf[self.pos - 1]) {
            self.pos -= 1;
        }
    }

    /// Move cursor forward to the end of the next word.
    pub fn move_forward_word(&mut self) {
        let len = self.buf.len();
        // Skip word chars
        while self.pos < len && Self::is_word_char(self.buf[self.pos]) {
            self.pos += 1;
        }
        // Skip non-word chars
        while self.pos < len && !Self::is_word_char(self.buf[self.pos]) {
            self.pos += 1;
        }
    }

    /// Kill from cursor to end of line. Returns the killed text.
    pub fn kill_to_end(&mut self) -> String {
        let killed: String = self.buf[self.pos..].iter().collect();
        self.buf.truncate(self.pos);
        killed
    }

    /// Kill from start of line to cursor. Returns the killed text.
    pub fn kill_to_start(&mut self) -> String {
        let killed: String = self.buf[..self.pos].iter().collect();
        self.buf.drain(..self.pos);
        self.pos = 0;
        killed
    }

    /// Kill the word behind the cursor. Returns the killed text.
    pub fn kill_backward_word(&mut self) -> String {
        let old_pos = self.pos;
        self.move_backward_word();
        let killed: String = self.buf[self.pos..old_pos].iter().collect();
        self.buf.drain(self.pos..old_pos);
        killed
    }

    /// Kill from cursor to end of the next word. Returns the killed text.
    pub fn kill_forward_word(&mut self) -> String {
        let old_pos = self.pos;
        let len = self.buf.len();
        let mut end = self.pos;
        // Skip word chars
        while end < len && Self::is_word_char(self.buf[end]) {
            end += 1;
        }
        // Skip non-word chars
        while end < len && !Self::is_word_char(self.buf[end]) {
            end += 1;
        }
        let killed: String = self.buf[old_pos..end].iter().collect();
        self.buf.drain(old_pos..end);
        killed
    }

    /// Transpose the two characters around the cursor (Ctrl+T).
    pub fn transpose_chars(&mut self) {
        if self.buf.len() < 2 {
            return;
        }
        if self.pos == 0 {
            return;
        }
        if self.pos == self.buf.len() {
            // At end: swap the two chars before cursor
            self.buf.swap(self.pos - 2, self.pos - 1);
        } else {
            // In middle: swap char before cursor with char at cursor, advance
            self.buf.swap(self.pos - 1, self.pos);
            self.pos += 1;
        }
    }

    /// Transpose the two words around the cursor (Alt+T).
    pub fn transpose_words(&mut self) {
        // Find the end of the word the cursor is in (or before)
        let len = self.buf.len();
        if len == 0 { return; }

        // Find word2 (word at or before cursor)
        // First, if we're between words or at end, move back into a word
        let mut p = self.pos;
        if p == len || !Self::is_word_char(self.buf[p]) {
            // Move back to find end of previous word
            while p > 0 && !Self::is_word_char(self.buf[p - 1]) {
                p -= 1;
            }
        }
        if p == 0 { return; } // no word found before cursor

        // Now p is at or just past end of a word
        // Find end and start of word2
        let word2_end = p;
        // Find where word2 ends (scan forward if in word)
        let mut w2e = if self.pos < len && Self::is_word_char(self.buf[self.pos]) {
            let mut e = self.pos;
            while e < len && Self::is_word_char(self.buf[e]) { e += 1; }
            e
        } else {
            word2_end
        };

        // Find start of word2
        let mut w2s = w2e;
        while w2s > 0 && Self::is_word_char(self.buf[w2s - 1]) {
            w2s -= 1;
        }
        if w2s == 0 {
            // Only one word — need to find word1 after cursor
            // Actually let's try: find the previous word (word1)
            // If word2 starts at 0, there's no word before it
            return;
        }

        // Find start of word1 (word before word2)
        let mut w1e = w2s;
        while w1e > 0 && !Self::is_word_char(self.buf[w1e - 1]) {
            w1e -= 1;
        }
        if w1e == 0 { return; } // no word1
        let mut w1s = w1e;
        while w1s > 0 && Self::is_word_char(self.buf[w1s - 1]) {
            w1s -= 1;
        }

        // Extract words and separator
        let word1: Vec<char> = self.buf[w1s..w1e].to_vec();
        let sep: Vec<char> = self.buf[w1e..w2s].to_vec();
        let word2: Vec<char> = self.buf[w2s..w2e].to_vec();

        // Replace: word2 + sep + word1
        let mut replacement = Vec::new();
        replacement.extend_from_slice(&word2);
        replacement.extend_from_slice(&sep);
        replacement.extend_from_slice(&word1);

        self.buf.splice(w1s..w2e, replacement);
        self.pos = w1s + word2.len() + sep.len() + word1.len();
    }

    /// Convert the next word to uppercase (Alt+U).
    pub fn upcase_word(&mut self) {
        let len = self.buf.len();
        // Skip non-word chars
        while self.pos < len && !Self::is_word_char(self.buf[self.pos]) {
            self.pos += 1;
        }
        // Uppercase word chars
        while self.pos < len && Self::is_word_char(self.buf[self.pos]) {
            self.buf[self.pos] = self.buf[self.pos].to_uppercase().next().unwrap_or(self.buf[self.pos]);
            self.pos += 1;
        }
    }

    /// Convert the next word to lowercase (Alt+L).
    pub fn downcase_word(&mut self) {
        let len = self.buf.len();
        // Skip non-word chars
        while self.pos < len && !Self::is_word_char(self.buf[self.pos]) {
            self.pos += 1;
        }
        // Lowercase word chars
        while self.pos < len && Self::is_word_char(self.buf[self.pos]) {
            self.buf[self.pos] = self.buf[self.pos].to_lowercase().next().unwrap_or(self.buf[self.pos]);
            self.pos += 1;
        }
    }

    /// Capitalize the next word: first char uppercase, rest lowercase (Alt+C).
    pub fn capitalize_word(&mut self) {
        let len = self.buf.len();
        // Skip non-word chars
        while self.pos < len && !Self::is_word_char(self.buf[self.pos]) {
            self.pos += 1;
        }
        // Capitalize first char
        let mut first = true;
        while self.pos < len && Self::is_word_char(self.buf[self.pos]) {
            if first {
                self.buf[self.pos] = self.buf[self.pos].to_uppercase().next().unwrap_or(self.buf[self.pos]);
                first = false;
            } else {
                self.buf[self.pos] = self.buf[self.pos].to_lowercase().next().unwrap_or(self.buf[self.pos]);
            }
            self.pos += 1;
        }
    }

    /// Insert text at the current cursor position. Returns (start, len) for yank tracking.
    pub fn insert_str(&mut self, text: &str) -> (usize, usize) {
        let start = self.pos;
        let chars: Vec<char> = text.chars().collect();
        let len = chars.len();
        for (i, ch) in chars.into_iter().enumerate() {
            self.buf.insert(self.pos + i, ch);
        }
        self.pos += len;
        (start, len)
    }

    /// Remove `len` characters starting at `start`. Used by yank_pop to replace yanked text.
    pub fn remove_range(&mut self, start: usize, len: usize) {
        let end = (start + len).min(self.buf.len());
        self.buf.drain(start..end);
        if self.pos > start {
            self.pos = start;
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_move_backward_word test_move_forward_word test_kill_to_end test_kill_to_start test_kill_backward_word test_kill_forward_word test_transpose_chars test_upcase_word test_downcase_word test_capitalize_word test_transpose_words 2>&1 | tail -30`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add src/interactive/line_editor.rs tests/interactive.rs
git commit -m "feat(interactive): add word movement, kill, transpose, and case methods"
```

---

### Task 6: Integrate Keymap into LineEditor (refactor handle_key)

**Files:**
- Modify: `src/interactive/line_editor.rs` (replace `handle_key`, add new fields, add `execute_action`)
- Test: `tests/interactive.rs` (existing tests must still pass)

This is the core integration task. We replace the hardcoded `handle_key()` match with the Keymap dispatch flow, wire up KillRing and UndoManager, and implement the yank/yank_pop/undo execution logic.

- [ ] **Step 1: Write failing tests for the integrated behavior**

Add to `tests/interactive.rs`:

```rust
// ── Integration tests: kill ring via MockTerminal ─────────────────────

#[test]
fn test_mock_ctrl_k_kills_to_end() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("hello world"),
        // Move to position 5 (after "hello")
        vec![ctrl('a')],
        vec![key(KeyCode::Right); 5],
        // Kill to end
        vec![ctrl('k')],
        vec![key(KeyCode::Enter)],
    ].concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "hello");
}

#[test]
fn test_mock_ctrl_u_kills_to_start() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("hello world"),
        vec![ctrl('a')],
        vec![key(KeyCode::Right); 5],
        vec![ctrl('u')],
        vec![key(KeyCode::Enter)],
    ].concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), " world");
}

#[test]
fn test_mock_ctrl_w_kills_backward_word() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("hello world"),
        vec![ctrl('w')],
        vec![key(KeyCode::Enter)],
    ].concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "hello ");
}

#[test]
fn test_mock_ctrl_y_yanks() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("hello world"),
        vec![ctrl('w')],   // kills "world"
        vec![ctrl('a')],   // move to start
        vec![ctrl('y')],   // yank "world" at start
        vec![key(KeyCode::Enter)],
    ].concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "worldhello ");
}

#[test]
fn test_mock_ctrl_underscore_undo() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("hello"),
        vec![ctrl('k')],   // kill to end (but cursor at end, kills nothing visible)
        // Actually: let's type, then kill, then undo
        vec![ctrl('a')],   // move to start
        vec![ctrl('k')],   // kill "hello"
        vec![ctrl('_')],   // undo — should restore "hello"
        vec![key(KeyCode::Enter)],
    ].concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "hello");
}

#[test]
fn test_mock_alt_b_word_backward() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("hello world"),
        vec![alt('b')],    // move back to "world" start (pos 6)
        vec![ctrl('k')],   // kill "world"
        vec![key(KeyCode::Enter)],
    ].concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "hello ");
}

#[test]
fn test_mock_ctrl_l_clears_screen() {
    // Ctrl+L should not alter the buffer, just clear and redraw
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("test"),
        vec![ctrl('l')],
        vec![key(KeyCode::Enter)],
    ].concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "test");
}

#[test]
fn test_mock_ctrl_t_transpose() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("ab"),
        vec![ctrl('t')],   // at end: swap 'a' and 'b' → "ba"
        vec![key(KeyCode::Enter)],
    ].concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "ba");
}

#[test]
fn test_mock_alt_u_upcase() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("hello"),
        vec![ctrl('a')],
        vec![alt('u')],
        vec![key(KeyCode::Enter)],
    ].concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "HELLO");
}

#[test]
fn test_mock_numeric_arg_movement() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("abcdef"),
        vec![ctrl('a')],
        vec![alt('3')],         // numeric arg = 3
        vec![ctrl('f')],        // move forward 3
        vec![ctrl('k')],        // kill "def"
        vec![key(KeyCode::Enter)],
    ].concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "abc");
}
```

- [ ] **Step 2: Run tests to see them fail**

Run: `cargo test test_mock_ctrl_k test_mock_ctrl_u test_mock_ctrl_w test_mock_ctrl_y test_mock_ctrl_underscore test_mock_alt_b test_mock_ctrl_l test_mock_ctrl_t test_mock_alt_u test_mock_numeric_arg 2>&1 | tail -30`
Expected: failures (current `handle_key` doesn't handle these keys)

- [ ] **Step 3: Refactor `LineEditor` — add fields, replace `handle_key`, add `execute_action`**

Replace the `LineEditor` struct definition and `handle_key` method in `src/interactive/line_editor.rs`. The full changes:

**3a. Update imports at top of file:**

```rust
use std::io;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use super::completion::{self, CompletionContext, CompletionUI};
use super::edit_action::EditAction;
use super::fuzzy_search::FuzzySearchUI;
use super::highlight::{HighlightScanner, HighlightStyle, ColorSpan, CheckerEnv, apply_style};
use super::history::History;
use super::keymap::{BufferState, Keymap};
use super::kill_ring::KillRing;
use super::terminal::Terminal;
use super::undo::UndoManager;
```

**3b. Update struct definition:**

```rust
#[derive(Debug)]
pub struct LineEditor {
    buf: Vec<char>,
    pos: usize,
    suggestion: Option<String>,
    tab_count: u8,
    keymap: Keymap,
    kill_ring: KillRing,
    undo: UndoManager,
    yank_state: Option<YankState>,
    last_action: EditAction,
    last_was_insert: bool,
}

#[derive(Debug, Clone)]
struct YankState {
    start: usize,
    len: usize,
}
```

**3c. Update `new()` and `clear()`:**

```rust
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            pos: 0,
            suggestion: None,
            tab_count: 0,
            keymap: Keymap::new(),
            kill_ring: KillRing::new(60),
            undo: UndoManager::new(256),
            yank_state: None,
            last_action: EditAction::Noop,
            last_was_insert: false,
        }
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.pos = 0;
        self.suggestion = None;
        self.tab_count = 0;
        self.yank_state = None;
        self.last_action = EditAction::Noop;
        self.last_was_insert = false;
        self.undo.clear();
    }
```

Note: Remove `Default` derive from `LineEditor` since `Keymap` doesn't implement `Default`. Add a manual `Default` impl or just use `new()`.

**3d. Replace `handle_key` method:**

```rust
    fn handle_key(&mut self, key: KeyEvent, history: &mut History) -> KeyAction {
        let state = BufferState {
            is_empty: self.is_empty(),
            at_end: self.pos == self.buf.len(),
            has_suggestion: self.suggestion.is_some(),
            last_action: self.last_action,
        };

        let (action, count) = self.keymap.resolve(key, &state);

        if action != EditAction::Tab && action != EditAction::TabComplete {
            self.tab_count = 0;
        }

        // Undo snapshot management
        match action {
            EditAction::InsertChar(_) => {
                if !self.last_was_insert {
                    self.undo.save(&self.buf, self.pos);
                }
            }
            EditAction::KillToEnd | EditAction::KillToStart
            | EditAction::KillBackwardWord | EditAction::KillForwardWord
            | EditAction::DeleteBackward | EditAction::DeleteForward
            | EditAction::Yank | EditAction::YankPop
            | EditAction::TransposeChars | EditAction::TransposeWords
            | EditAction::UpcaseWord | EditAction::DowncaseWord | EditAction::CapitalizeWord => {
                if self.last_was_insert {
                    // Finalize the insert group — save current state so undo restores to end of typing
                    self.undo.save(&self.buf, self.pos);
                }
                self.undo.save(&self.buf, self.pos);
            }
            _ => {
                if self.last_was_insert {
                    self.undo.save(&self.buf, self.pos);
                }
            }
        }

        // Determine if consecutive kill for append
        let is_consecutive_kill = action.is_kill() && self.last_action.is_kill();

        // Execute action
        let key_action = self.execute_action(action, count, history, is_consecutive_kill);

        // Update tracking state
        self.last_was_insert = matches!(action, EditAction::InsertChar(ch) if ch != ' ');
        if !matches!(action, EditAction::Yank | EditAction::YankPop) {
            self.yank_state = None;
        }
        self.last_action = action;

        key_action
    }

    fn execute_action(
        &mut self,
        action: EditAction,
        count: u32,
        history: &mut History,
        consecutive_kill: bool,
    ) -> KeyAction {
        match action {
            EditAction::InsertChar(ch) => {
                for _ in 0..count {
                    self.insert_char(ch);
                }
                KeyAction::Continue
            }
            EditAction::MoveBackward => {
                for _ in 0..count { self.move_cursor_left(); }
                KeyAction::Continue
            }
            EditAction::MoveForward => {
                for _ in 0..count { self.move_cursor_right(); }
                KeyAction::Continue
            }
            EditAction::MoveToStart => {
                self.move_to_start();
                KeyAction::Continue
            }
            EditAction::MoveToEnd => {
                self.move_to_end();
                KeyAction::Continue
            }
            EditAction::MoveBackwardWord => {
                for _ in 0..count { self.move_backward_word(); }
                KeyAction::Continue
            }
            EditAction::MoveForwardWord => {
                for _ in 0..count { self.move_forward_word(); }
                KeyAction::Continue
            }
            EditAction::DeleteBackward => {
                for _ in 0..count { self.backspace(); }
                KeyAction::Continue
            }
            EditAction::DeleteForward => {
                for _ in 0..count { self.delete(); }
                KeyAction::Continue
            }
            EditAction::KillToEnd => {
                let killed = self.kill_to_end();
                self.kill_ring.kill(&killed, consecutive_kill);
                KeyAction::Continue
            }
            EditAction::KillToStart => {
                let killed = self.kill_to_start();
                self.kill_ring.prepend(&killed, consecutive_kill);
                KeyAction::Continue
            }
            EditAction::KillBackwardWord => {
                for _ in 0..count {
                    let killed = self.kill_backward_word();
                    self.kill_ring.prepend(&killed, consecutive_kill);
                }
                KeyAction::Continue
            }
            EditAction::KillForwardWord => {
                for _ in 0..count {
                    let killed = self.kill_forward_word();
                    self.kill_ring.kill(&killed, consecutive_kill);
                }
                KeyAction::Continue
            }
            EditAction::Yank => {
                if let Some(text) = self.kill_ring.yank().map(|s| s.to_string()) {
                    let (start, len) = self.insert_str(&text);
                    self.yank_state = Some(YankState { start, len });
                }
                KeyAction::Continue
            }
            EditAction::YankPop => {
                if let Some(ys) = self.yank_state.clone() {
                    self.remove_range(ys.start, ys.len);
                    if let Some(text) = self.kill_ring.yank_pop().map(|s| s.to_string()) {
                        let (start, len) = self.insert_str(&text);
                        self.yank_state = Some(YankState { start, len });
                    }
                }
                KeyAction::Continue
            }
            EditAction::TransposeChars => {
                for _ in 0..count { self.transpose_chars(); }
                KeyAction::Continue
            }
            EditAction::TransposeWords => {
                for _ in 0..count { self.transpose_words(); }
                KeyAction::Continue
            }
            EditAction::UpcaseWord => {
                for _ in 0..count { self.upcase_word(); }
                KeyAction::Continue
            }
            EditAction::DowncaseWord => {
                for _ in 0..count { self.downcase_word(); }
                KeyAction::Continue
            }
            EditAction::CapitalizeWord => {
                for _ in 0..count { self.capitalize_word(); }
                KeyAction::Continue
            }
            EditAction::Undo => {
                for _ in 0..count {
                    if let Some((buf, pos)) = self.undo.undo() {
                        self.buf = buf;
                        self.pos = pos;
                    }
                }
                KeyAction::Continue
            }
            EditAction::ClearScreen => {
                KeyAction::ClearScreen
            }
            EditAction::Cancel => {
                // Numeric arg already reset by keymap
                KeyAction::Continue
            }
            EditAction::AcceptSuggestion => {
                self.accept_full_suggestion();
                KeyAction::Continue
            }
            EditAction::AcceptWordSuggestion => {
                self.accept_word_suggestion();
                KeyAction::Continue
            }
            EditAction::SetNumericArg(_) => {
                // Numeric arg accumulated in keymap, nothing to do here
                KeyAction::Continue
            }
            EditAction::Submit => KeyAction::Submit,
            EditAction::Eof => KeyAction::Eof,
            EditAction::Interrupt => KeyAction::Interrupt,
            EditAction::FuzzySearch => KeyAction::FuzzySearch,
            EditAction::TabComplete => {
                self.tab_count += 1;
                KeyAction::TabComplete
            }
            EditAction::HistoryPrev => {
                for _ in 0..count {
                    if let Some(line) = history.navigate_up(&self.buffer()) {
                        self.buf = line.chars().collect();
                        self.pos = self.buf.len();
                    }
                }
                self.suggestion = None;
                KeyAction::Continue
            }
            EditAction::HistoryNext => {
                for _ in 0..count {
                    if let Some(line) = history.navigate_down() {
                        self.buf = line.chars().collect();
                        self.pos = self.buf.len();
                    }
                }
                self.suggestion = None;
                KeyAction::Continue
            }
            EditAction::Noop => KeyAction::Continue,
        }
    }
```

**3e. Add `ClearScreen` variant to `KeyAction` and handle it in both read loops:**

Add to the `KeyAction` enum:

```rust
enum KeyAction {
    Continue,
    Submit,
    Eof,
    Interrupt,
    FuzzySearch,
    TabComplete,
    ClearScreen,
}
```

In `read_line_loop`, add handling for `ClearScreen` alongside the other variants:

```rust
KeyAction::ClearScreen => {
    term.write_str("\x1b[2J\x1b[H")?;  // Clear screen + move cursor home
    term.move_to_column(0)?;
    term.write_str(prompt)?;
}
```

And in `read_line_loop_with_completion`, add the same:

```rust
KeyAction::ClearScreen => {
    term.write_str("\x1b[2J\x1b[H")?;
    term.move_to_column(0)?;
    term.write_str(prompt)?;
}
```

Update the match arms that use `KeyAction::TabComplete | KeyAction::Continue` to also include `ClearScreen`:
- In `read_line_loop`: `KeyAction::TabComplete | KeyAction::Continue | KeyAction::ClearScreen => {}`  
  Wait, no — `ClearScreen` needs its own handling. Let me restructure: move the `ClearScreen` arm BEFORE the catch-all. The pattern in `read_line_loop`:

```rust
KeyAction::ClearScreen => {
    term.write_str("\x1b[2J\x1b[H")?;
    term.move_to_column(0)?;
    term.write_str(prompt)?;
}
KeyAction::TabComplete | KeyAction::Continue => {}
```

- [ ] **Step 4: Run ALL existing tests to verify no regressions**

Run: `cargo test 2>&1 | tail -30`
Expected: all existing tests pass, plus the new integration tests pass

- [ ] **Step 5: Commit**

```bash
git add src/interactive/line_editor.rs tests/interactive.rs
git commit -m "feat(interactive): integrate Keymap, KillRing, UndoManager into LineEditor"
```

---

### Task 7: Add `clear_all` to Terminal trait for Ctrl+L

**Files:**
- Modify: `src/interactive/terminal.rs` (add method to trait + CrosstermTerminal impl)
- Modify: `src/interactive/line_editor.rs` (use trait method instead of raw escape)
- Modify: `tests/helpers/mock_terminal.rs` (add mock impl)

- [ ] **Step 1: Add `clear_all` method to `Terminal` trait**

Add to the trait in `src/interactive/terminal.rs`:

```rust
    /// Clear the entire screen and move cursor to top-left.
    fn clear_all(&mut self) -> io::Result<()>;
```

Add implementation in `CrosstermTerminal`:

```rust
    fn clear_all(&mut self) -> io::Result<()> {
        self.stdout.execute(terminal::Clear(ClearType::All))?;
        self.stdout.execute(cursor::MoveTo(0, 0))?;
        Ok(())
    }
```

- [ ] **Step 2: Add mock implementation**

Add to `MockTerminal` in `tests/helpers/mock_terminal.rs`:

```rust
    fn clear_all(&mut self) -> io::Result<()> {
        self.output.push("[CLEAR_ALL]".to_string());
        Ok(())
    }
```

- [ ] **Step 3: Update `ClearScreen` handling in `line_editor.rs`**

Replace the raw escape sequence with the trait method in both `read_line_loop` and `read_line_loop_with_completion`:

```rust
KeyAction::ClearScreen => {
    term.clear_all()?;
    term.write_str(prompt)?;
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add src/interactive/terminal.rs src/interactive/line_editor.rs tests/helpers/mock_terminal.rs
git commit -m "feat(interactive): add Terminal::clear_all for Ctrl+L screen clear"
```

---

### Task 8: Update TODO.md

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Remove the completed Emacs keybindings item from TODO.md**

Delete the line:
```
- [ ] Emacs keybindings — Ctrl+K (kill to end), Ctrl+U (kill to start), Ctrl+W (kill word), Ctrl+Y (yank)
```

- [ ] **Step 2: Commit**

```bash
git add TODO.md
git commit -m "docs(TODO): remove completed Emacs keybindings item"
```

---

### Task 9: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test 2>&1`
Expected: all tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy 2>&1 | tail -20`
Expected: no errors (warnings acceptable)

- [ ] **Step 3: Build in release mode**

Run: `cargo build --release 2>&1 | tail -5`
Expected: successful build
