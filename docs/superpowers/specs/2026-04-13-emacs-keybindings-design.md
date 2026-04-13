# Emacs Keybindings for Interactive Mode

**Date:** 2026-04-13
**Status:** Approved

## Overview

Add readline-compatible Emacs keybindings to kish's interactive mode, including a full kill ring, undo support, numeric arguments, and word/case manipulation commands. The internal architecture separates keymap resolution from action execution, providing a foundation for future configurable keybindings (`~/.inputrc`).

## Scope

- 20 new keybindings (see full list below)
- Kill ring with Ctrl+Y / Alt+Y rotation
- Undo with Ctrl+_ (grouped by input type)
- Numeric arguments with Alt+0..9
- Internal keymap separation for future configurability
- No config file reading in this phase

## Module Structure

```
src/interactive/
├── line_editor.rs          # LineEditor (buffer ops + event loop)
├── edit_action.rs          # EditAction enum
├── keymap.rs               # KeyEvent → EditAction conversion (Emacs default)
├── kill_ring.rs            # KillRing struct
└── undo.rs                 # UndoManager struct
```

### edit_action.rs

All editing operations expressed as an enum. Serves as the contract between keymap and editor.

```rust
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EditAction {
    // Character input
    InsertChar(char),

    // Cursor movement
    MoveBackward,           // Ctrl+B, Left
    MoveForward,            // Ctrl+F, Right
    MoveToStart,            // Ctrl+A, Home
    MoveToEnd,              // Ctrl+E, End
    MoveBackwardWord,       // Alt+B
    MoveForwardWord,        // Alt+F (when no suggestion displayed)

    // Delete (does NOT enter kill ring)
    DeleteBackward,         // Backspace
    DeleteForward,          // Delete, Ctrl+D (non-empty buffer)

    // Kill (enters kill ring)
    KillToEnd,              // Ctrl+K
    KillToStart,            // Ctrl+U
    KillBackwardWord,       // Alt+Backspace, Ctrl+W
    KillForwardWord,        // Alt+D

    // Yank
    Yank,                   // Ctrl+Y
    YankPop,                // Alt+Y (only valid after Yank/YankPop)

    // Editing
    TransposeChars,         // Ctrl+T
    TransposeWords,         // Alt+T
    UpcaseWord,             // Alt+U
    DowncaseWord,           // Alt+L
    CapitalizeWord,         // Alt+C

    // Undo
    Undo,                   // Ctrl+_

    // Other
    ClearScreen,            // Ctrl+L
    Cancel,                 // Ctrl+G (reset numeric arg)
    AcceptSuggestion,       // Right (cursor at end + suggestion present)
    AcceptWordSuggestion,   // Alt+F (suggestion present)
    SetNumericArg(u8),      // Alt+0..9

    // Control (maps to KeyAction for REPL loop)
    Submit,                 // Enter
    Eof,                    // Ctrl+D (empty buffer)
    Interrupt,              // Ctrl+C
    FuzzySearch,            // Ctrl+R
    TabComplete,            // Tab
    HistoryPrev,            // Up
    HistoryNext,            // Down
    Noop,                   // Unmapped keys
}
```

### keymap.rs

Converts `KeyEvent` to `(EditAction, repeat_count)`.

```rust
pub struct Keymap {
    numeric_arg: Option<u32>,
}

/// Minimal buffer state needed for key resolution.
pub struct BufferState {
    pub is_empty: bool,
    pub at_end: bool,
    pub has_suggestion: bool,
    pub last_action: EditAction,
}

impl Keymap {
    pub fn new() -> Self;
    pub fn resolve(&mut self, key: KeyEvent, state: &BufferState) -> (EditAction, u32);
    pub fn pending_numeric_arg(&self) -> Option<u32>;
}
```

Resolution rules:
1. `Alt+0..9` → accumulate in `numeric_arg`, return `(Noop, 1)`
2. `Ctrl+G` → reset `numeric_arg`, return `(Cancel, 1)`
3. All other keys → convert to `EditAction`, consume `numeric_arg` as `repeat_count` (default 1)
4. `Ctrl+D` → `Eof` if `is_empty`, else `DeleteForward`
5. `Right` / `Ctrl+F` → `AcceptSuggestion` if `at_end && has_suggestion`, else `MoveForward`
6. `Alt+F` → `AcceptWordSuggestion` if `has_suggestion`, else `MoveForwardWord`
7. `Alt+Y` → `YankPop` if `last_action` is `Yank` or `YankPop`, else `Noop`

### kill_ring.rs

Circular buffer of killed text.

```rust
pub struct KillRing {
    ring: VecDeque<String>,
    max_size: usize,          // Default 60 (same as readline)
    yank_index: usize,
}

impl KillRing {
    pub fn new(max_size: usize) -> Self;
    pub fn kill(&mut self, text: &str, append: bool);
    pub fn yank(&mut self) -> Option<&str>;
    pub fn yank_pop(&mut self) -> Option<&str>;
}
```

Consecutive kill merging:
- Consecutive kills of the same direction call `kill(text, append: true)` to concatenate
- `KillToEnd` / `KillForwardWord` append to the end
- `KillToStart` / `KillBackwardWord` prepend to the front
- Any non-kill action between kills starts a new entry (`append: false`)
- Determination of `append` is done by `LineEditor` checking `last_action`

Alt+Y (yank_pop) behavior:
1. `Ctrl+Y` yanks → `yank_index = 0` (newest entry)
2. `Alt+Y` → remove yanked text from buffer, replace with `yank_index = 1`
3. Further `Alt+Y` → increment `yank_index`, wrap around at ring end
4. Any non-`Alt+Y` key confirms the replacement

### undo.rs

Snapshot-based undo with grouped character input.

```rust
struct UndoEntry {
    buf: Vec<char>,
    pos: usize,
}

pub struct UndoManager {
    stack: Vec<UndoEntry>,
    max_size: usize,          // Default 256
}

impl UndoManager {
    pub fn new(max_size: usize) -> Self;
    pub fn save(&mut self, buf: &[char], pos: usize);
    pub fn undo(&mut self) -> Option<(Vec<char>, usize)>;
    pub fn clear(&mut self);
}
```

Undo granularity:
- **Character input**: Grouped. Consecutive `InsertChar` calls share one snapshot. A space or any non-insert action triggers a group boundary (snapshot saved).
- **Kill/Delete/Yank/Transpose**: One snapshot per operation.
- **Submit**: `clear()` resets the stack.

Grouping is tracked by `last_was_insert: bool` in `LineEditor`.

### LineEditor Changes

Updated struct fields:

```rust
pub struct LineEditor {
    buf: Vec<char>,
    pos: usize,
    suggestion: Option<String>,
    tab_count: u8,

    // New
    keymap: Keymap,
    kill_ring: KillRing,
    undo: UndoManager,
    yank_state: Option<YankState>,
    last_action: EditAction,
    last_was_insert: bool,
}

struct YankState {
    start: usize,
    len: usize,
}
```

Action execution flow (replaces current `handle_key`):

1. Receive `KeyEvent`
2. `keymap.resolve(key, buffer_state)` → `(EditAction, repeat_count)`
3. Undo snapshot decision:
   - `InsertChar` with `last_was_insert == false` → `save()`
   - Non-`InsertChar` with `last_was_insert == true` → `save()` (finalize input group)
   - Kill/Delete/Yank/Transpose → `save()`
4. Execute `EditAction` × `repeat_count` via `execute_action()` match
5. Update `last_action`
6. Non-Yank/YankPop action → `yank_state = None`
7. Control actions (Submit/Eof/Interrupt/FuzzySearch/TabComplete) → return corresponding `KeyAction`

### TransposeChars Behavior

Follows readline convention:
- **Cursor at position 0**: No-op
- **Cursor at end of buffer**: Swap the two characters before cursor, cursor stays at end
- **Cursor in the middle**: Swap character before cursor with character at cursor, advance cursor by one

### TransposeWords Behavior

Follows readline convention:
- Find the word the cursor is in (or the previous word if between words)
- Swap it with the next word (or previous word if at end)
- Cursor moves to after the second word

### Word Boundary Definition

Shared by all word movement and kill operations (readline-compatible):
- **Word characters**: alphanumeric + underscore (`char::is_alphanumeric() || ch == '_'`)
- **Delimiters**: everything else
- `MoveForwardWord`: skip delimiters → advance through word characters to end
- `MoveBackwardWord`: skip delimiters → retreat through word characters to start
- `KillForwardWord` / `KillBackwardWord`: same boundaries, killed text goes to kill ring

## Full Keybinding Table

**Bold** = newly added.

### Cursor Movement
| Key | Action |
|-----|--------|
| Ctrl+B, Left | MoveBackward |
| Ctrl+F, Right | MoveForward |
| Ctrl+A, Home | MoveToStart |
| Ctrl+E, End | MoveToEnd |
| **Alt+B** | **MoveBackwardWord** |
| **Alt+F** | **MoveForwardWord** (no suggestion) |

### Delete (no kill ring)
| Key | Action |
|-----|--------|
| Backspace | DeleteBackward |
| Delete, Ctrl+D (non-empty) | DeleteForward |

### Kill (enters kill ring)
| Key | Action |
|-----|--------|
| **Ctrl+K** | **KillToEnd** |
| **Ctrl+U** | **KillToStart** |
| **Ctrl+W, Alt+Backspace** | **KillBackwardWord** |
| **Alt+D** | **KillForwardWord** |

### Yank
| Key | Action |
|-----|--------|
| **Ctrl+Y** | **Yank** |
| **Alt+Y** | **YankPop** |

### Editing
| Key | Action |
|-----|--------|
| **Ctrl+T** | **TransposeChars** |
| **Alt+T** | **TransposeWords** |
| **Alt+U** | **UpcaseWord** |
| **Alt+L** | **DowncaseWord** |
| **Alt+C** | **CapitalizeWord** |

### Undo / Cancel
| Key | Action |
|-----|--------|
| **Ctrl+_** | **Undo** |
| **Ctrl+G** | **Cancel** |

### Screen Control
| Key | Action |
|-----|--------|
| **Ctrl+L** | **ClearScreen** |

### Numeric Arguments
| Key | Action |
|-----|--------|
| **Alt+0..9** | **SetNumericArg** |

### Other (existing, maintained)
| Key | Action |
|-----|--------|
| Enter | Submit |
| Ctrl+D (empty) | Eof |
| Ctrl+C | Interrupt |
| Ctrl+R | FuzzySearch |
| Tab | TabComplete |
| Up | HistoryPrev |
| Down | HistoryNext |
| Alt+F (suggestion) | AcceptWordSuggestion |
| Right, Ctrl+F (suggestion) | AcceptSuggestion |

## Testing Strategy

- **Unit tests** for each new module (`kill_ring.rs`, `undo.rs`, `keymap.rs`, `edit_action.rs`)
- **Integration tests** for `LineEditor` exercising keybinding sequences
- **PTY E2E tests** for key scenarios: kill/yank round-trip, undo, word movement, numeric args
