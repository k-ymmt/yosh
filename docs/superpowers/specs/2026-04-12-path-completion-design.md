# Path Completion for Interactive Mode

**Date**: 2026-04-12
**Status**: Approved

## Overview

Add Tab-based file path completion to kish's interactive mode. Tab press once completes the longest common prefix; Tab twice opens an interactive fuzzy-filter UI for candidate selection (similar to the Ctrl+R history search UI).

## Scope

- File path completion only (no command name, variable, or builtin completion)
- Tab key trigger only (no inline autosuggestion for paths)

## Architecture

### New Module: `src/interactive/completion.rs`

Responsible for:

- Extracting the completion word from the buffer at cursor position
- Splitting the completion word into directory and prefix parts
- Scanning the filesystem for candidates
- Filtering and sorting candidates
- Computing the longest common prefix
- Providing the interactive completion UI (`CompletionUI`)

### Integration Points

- `LineEditor`: Tab key handling, `tab_count` state tracking
- `Repl`: Passing completion context (CWD, environment variables)

## Detailed Design

### 1. Completion Word Extraction

Scan leftward from the cursor position until hitting a shell word delimiter:
- Space, `|`, `;`, `&`, `<`, `>`, `(`, `)`

Special handling:
- Quoted strings: spaces inside quotes are not treated as delimiters
- Example: `ls src/int|` (cursor at `|`) -> completion word is `src/int`
- Example: `cat foo | grep b|` -> completion word is `b`

### 2. Path Decomposition

Split the completion word at the last `/`:

| Input | Directory | Prefix |
|-------|-----------|--------|
| `src/int` | `src/` | `int` |
| `foo` | (CWD) | `foo` |
| `/usr/lo` | `/usr/` | `lo` |
| `~/Doc` | `$HOME/` | `Doc` |

`~` is expanded to the value of `$HOME` before directory resolution.

### 3. Candidate Generation

- Scan the resolved directory using `std::fs::read_dir`
- Filter entries by prefix (case-sensitive prefix match)
- If directory doesn't exist or is unreadable, return empty list (silent failure)

**Hidden file handling:**
- If prefix starts with `.`: include dotfiles
- If `KISH_SHOW_DOTFILES=1`: always include dotfiles
- Otherwise: exclude entries starting with `.`

**Candidate decoration:**
- Directories: append `/` to the candidate name
- Regular files: no suffix

**Sorting:** Alphabetical order (byte-wise).

### 4. Longest Common Prefix

Compute the longest common prefix of all candidate names, character by character. Completion is applied only when this prefix is longer than the current prefix.

### 5. Tab Key Behavior

**State:** `tab_count: u8` field in `LineEditor`, reset to 0 on any non-Tab key.

**Tab 1st press (`tab_count == 1`):**

1. Extract completion word from buffer at cursor position
2. Generate candidates via `completion` module
3. If 0 candidates: do nothing
4. If 1 candidate: replace completion word in buffer with the candidate, move cursor. If directory, append `/`
5. If multiple candidates: insert longest common prefix into buffer, move cursor. If common prefix equals current prefix, nothing changes (signals to press Tab again)

**Tab 2nd press (`tab_count >= 2`):**

1. If candidates >= 2: open `CompletionUI` (interactive fuzzy selection)
2. On selection (Enter/Tab): replace completion word in buffer with selected candidate. If directory, append `/`
3. On cancel (Esc/Ctrl+G): no change to buffer

**Continuous completion:**
After a directory is completed (e.g., `src/`), subsequent Tab presses start fresh (`tab_count` resets because the buffer content changes), generating candidates for the new directory.

### 6. Interactive Completion UI (`CompletionUI`)

A separate struct in `completion.rs`, following the same rendering pattern as `FuzzySearchUI`.

**Layout:**
```
  file_c.txt
  file_b.rs
> file_a.rs        <- selected (reverse video)
  ────────────────
  3/5 > fi         <- count + fuzzy filter query
```

**Key bindings:**

| Key | Action |
|-----|--------|
| Characters | Fuzzy filter candidates |
| Backspace | Delete filter character |
| Up / Ctrl+P | Move selection up |
| Down / Ctrl+N | Move selection down |
| Enter | Confirm selection |
| Tab | Confirm selection (same as Enter) |
| Esc / Ctrl+G | Cancel |

**Display limit:** 40% of terminal height (minimum 3 lines), same as `FuzzySearchUI`.

### 7. KeyAction and LineEditor Changes

New `KeyAction` variant:
```rust
enum KeyAction {
    Continue,
    Submit,
    Eof,
    Interrupt,
    FuzzySearch,
    TabComplete,  // New
}
```

`handle_key` changes:
- Add `(KeyCode::Tab, _) => { self.tab_count += 1; KeyAction::TabComplete }`
- All other key branches reset `self.tab_count = 0`

### 8. Completion Context

```rust
pub struct CompletionContext {
    pub cwd: String,
    pub show_dotfiles: bool,
}
```

Passed from `Repl` to `read_line`. `Repl` constructs it from:
- CWD: `std::env::current_dir()` or `$PWD`
- `show_dotfiles`: `KISH_SHOW_DOTFILES == "1"`

### 9. `read_line_loop` Integration

`TabComplete` handling follows the same pattern as `FuzzySearch`:

```
TabComplete received:
  1. Call completion module with buffer, cursor pos, and CompletionContext
  2. If tab_count == 1: apply common prefix completion to buffer
  3. If tab_count >= 2 and candidates >= 2:
     - Disable raw mode
     - Run CompletionUI::run()
     - Apply result to buffer
     - Re-enable raw mode
     - Redraw prompt + buffer
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `KISH_SHOW_DOTFILES` | unset | When set to `1`, include hidden files in completion candidates regardless of prefix |

## Testing Strategy

### Unit Tests (in `completion.rs`)

- `extract_completion_word`: space, pipe, semicolon, quote edge cases
- `split_path`: relative, absolute, `~`, no-directory cases
- `longest_common_prefix`: various candidate sets
- `generate_candidates`: using `tempdir`
  - Hidden file filtering (with/without `KISH_SHOW_DOTFILES`)
  - Directory `/` suffix
  - Sort order
  - Non-existent directory returns empty

### Integration Tests (`tests/interactive.rs`)

- MockTerminal Tab key injection
- Single Tab common prefix completion
- Single candidate immediate completion

### PTY E2E Tests (`tests/pty_interactive.rs`)

- Tab completion of file paths in a real shell session
- Directory completion with `/` and continuous Tab for subdirectory
