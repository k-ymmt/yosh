# Command History: Design Specification

## Overview

Add command history support to kish's interactive mode: up/down arrow navigation, `~/.kish_history` persistence, Ctrl+R fzf-style fuzzy search, and the POSIX `fc` built-in command.

## Requirements

1. **Up/Down arrow key navigation** through past commands
2. **`~/.kish_history` persistence** with bash-compatible shell variable control (HISTFILE, HISTSIZE, HISTFILESIZE)
3. **HISTCONTROL=ignoreboth default** — skip consecutive duplicates and commands starting with a space
4. **Ctrl+R fzf-style fuzzy search UI** — terminal height 40%, fuzzy matching with scoring, bottom-up candidate list
5. **`fc` built-in command** — POSIX-compliant history listing, editor-based editing, and substitution re-execution

## Architecture: Module Separation (Approach B)

```
src/interactive/
├── mod.rs              — Repl (existing, modified)
├── line_editor.rs      — LineEditor (existing, modified for ↑/↓/Ctrl+R)
├── history.rs          — NEW: History struct (in-memory list + file persistence)
├── fuzzy_search.rs     — NEW: FuzzySearchUI (Ctrl+R fzf-style UI + fuzzy match algorithm)
├── prompt.rs           — existing, unchanged
└── parse_status.rs     — existing, unchanged
src/builtin/special.rs  — fc command added (POSIX Special Built-In)
```

## Module 1: History (`src/interactive/history.rs`)

### Struct

```rust
pub struct History {
    entries: Vec<String>,    // History entries (oldest first)
    cursor: Option<usize>,   // Current position for ↑/↓ navigation (None = at newest)
    saved_line: String,      // Stashed in-progress input when navigation starts
}
```

### Public API

| Method | Description |
|--------|-------------|
| `new()` | Create empty history |
| `load(path: &Path)` | Load from file. No-op if file doesn't exist |
| `save(path: &Path, histfilesize: usize)` | Write to file, truncating to HISTFILESIZE |
| `add(line: &str, histsize: usize, histcontrol: &str)` | Add entry with ignoreboth logic and HISTSIZE truncation |
| `navigate_up(current_line: &str) -> Option<&str>` | Return previous entry. Stash current_line on first call |
| `navigate_down() -> Option<&str>` | Return next entry. Return saved_line past the end |
| `reset_cursor()` | Reset navigation state (called on Enter/Ctrl+C) |
| `entries() -> &[String]` | Read access for fuzzy search and fc |

### Shell Variable Integration

| Variable | Default | Used by |
|----------|---------|---------|
| `HISTFILE` | `~/.kish_history` | `load()`, `save()` |
| `HISTSIZE` | `500` | `add()` — max entries in memory |
| `HISTFILESIZE` | `500` | `save()` — max entries in file |
| `HISTCONTROL` | `ignoreboth` | `add()` — duplicate/space filtering |

`History` does not read shell variables directly. The caller (Repl) reads variables from ShellEnv and passes values as arguments. This keeps History decoupled from shell state.

### Ignoreboth Logic in `add()`

- **ignorespace**: Skip if `line.starts_with(' ')`
- **ignoredups**: Skip if `line == entries.last()`
- **ignoreboth**: Apply both checks (default)

## Module 2: LineEditor Integration (`src/interactive/line_editor.rs`)

### Signature Change

```rust
// Before
pub fn read_line(&mut self, prompt_width: usize) -> Result<Option<String>>

// After
pub fn read_line(&mut self, prompt_width: usize, history: &mut History) -> Result<Option<String>>
```

### Key Additions in `handle_key()`

| Key | Action |
|-----|--------|
| `KeyCode::Up` | Call `history.navigate_up(current_line)`, replace buffer, cursor to end |
| `KeyCode::Down` | Call `history.navigate_down()`, replace buffer, cursor to end |
| `Ctrl+R` | Launch `FuzzySearchUI::run(history)`, set result to buffer |

### Lifecycle

1. User presses ↑ → `navigate_up()` stashes current input on first call, moves cursor back
2. Further ↑ → cursor continues back. Stays at oldest entry at boundary
3. ↓ → cursor moves forward. Past newest entry, restores saved_line
4. Enter / Ctrl+C → `history.reset_cursor()` resets navigation state

## Module 3: Fuzzy Search UI (`src/interactive/fuzzy_search.rs`)

### Struct

```rust
pub struct FuzzySearchUI {
    query: Vec<char>,        // Search query
    cursor: usize,           // Cursor position within query
    selected: usize,         // Selected index in candidate list
    scroll_offset: usize,    // Scroll offset for display
}
```

### Entry Point

```rust
/// Start search UI, return selected command string. None on cancel.
pub fn run(history: &History) -> Result<Option<String>>
```

### Fuzzy Match Algorithm

Order-preserving scoring:

1. Each query character must appear **in order** in the target string
2. Scoring bonuses:
   - **Consecutive match**: characters matching in sequence score higher
   - **Word boundary**: match at start of a word (after space, `/`, `-`, `_`) scores higher
   - **Exact case**: case-sensitive match scores higher than case-insensitive
3. Candidates sorted by score descending

Example: query `gco` matches `git checkout` (g→c→o in order).

### UI Layout

```
  git checkout main           <- Candidate 3 (by score)
  git commit -m "fix"         <- Candidate 2
> git checkout develop        <- Candidate 1 (selected, highlighted)
  ──────────────────────
  3/150 > gco█                <- Query input (match_count/total > input)
```

- Uses ~40% of terminal height (`crossterm::terminal::size()`)
- Candidates displayed bottom-to-top (fzf style — most relevant near input line)
- Selected line uses reverse video (`SetAttribute(Attribute::Reverse)`)

### Key Bindings

| Key | Action |
|-----|--------|
| Character input | Append to query, real-time filtering |
| Backspace | Delete last query character |
| ↑ / Ctrl+P | Move selection up |
| ↓ / Ctrl+N | Move selection down |
| Enter | Insert selected command into prompt (does not execute) |
| Esc / Ctrl+G | Cancel, return to original prompt |
| Ctrl+R | Move selection up (same as ↑) |

### Rendering and Cleanup

- **Draw**: Save cursor position → reserve blank lines for draw area → render candidates and query line
- **Update**: Clear draw area each keypress, re-render
- **Exit**: Clear entire draw area, restore cursor to original position

## Module 4: `fc` Built-in (POSIX-compliant)

### Synopsis

```
fc [-r] [-e editor] [first [last]]    # Edit and execute
fc -l [-nr] [first [last]]            # List
fc -s [old=new] [first]               # Substitute and re-execute
```

### Supported Operations

**List mode (`fc -l`)**
- `fc -l` — display last 16 entries with numbers
- `fc -l first last` — range specified by number (positive: absolute, negative: relative from end) or string (prefix match)
- `fc -ln` — display without numbers
- `fc -lr` — display in reverse order

**Edit and execute (`fc [-e editor]`)**
- `fc` — open last command in `$FCEDIT` (fallback: `$EDITOR`, then `/bin/ed`)
- Write command(s) to temp file (`/tmp/kish_fc_XXXXXX`) → launch editor → read back → execute → delete temp file
- `fc first last` — open range of commands in editor
- `fc -r` — reverse order in editor

**Substitute and re-execute (`fc -s`)**
- `fc -s` — re-execute last command
- `fc -s old=new` — replace `old` with `new` in last command, then execute
- `fc -s old=new first` — same but for command identified by `first`

### Implementation Location

Add to `src/builtin/special.rs` since `fc` is a POSIX Special Built-In Utility.

### History Access

`fc` accesses history via `env.history` (ShellEnv owns History).

## Module 5: Repl Integration and Lifecycle

### Ownership

```
Repl
 ├── executor: Executor
 │    └── env: ShellEnv
 │         └── history: History   <- Ownership here
 └── line_editor: LineEditor      <- Borrows &mut History
```

### Initialization (`Repl::new()`)

1. Create `History::new()`
2. Set shell variable defaults: `HISTFILE=~/.kish_history`, `HISTSIZE=500`, `HISTFILESIZE=500`, `HISTCONTROL=ignoreboth`
3. Call `history.load(histfile_path)`

### Main Loop Changes

```
loop {
    // ...existing prompt display...
    let line = line_editor.read_line(prompt_width, &mut env.history)?;
    // ...existing parse logic...
    if parsed successfully {
        env.history.add(&complete_command, histsize, histcontrol);
        executor.exec_complete_command(...);
    }
}
```

The command passed to `history.add()` is the complete command string including multi-line continuations (the full `input_buffer`).

### Shutdown

Call `history.save(histfile_path, histfilesize)` before `Repl::run()` returns.

### Signal Handling

Call `history.save()` on SIGHUP to prevent history loss.

## Testing Strategy

### Unit Tests

**`history.rs`**
- `add()`: normal add, ignoredups, ignorespace, ignoreboth
- `add()`: HISTSIZE truncation
- `navigate_up()` / `navigate_down()`: cursor movement, boundary behavior, saved_line stash/restore
- `reset_cursor()`: navigation state reset
- `load()` / `save()`: file I/O, missing file, HISTFILESIZE truncation, empty file

**`fuzzy_search.rs`**
- Fuzzy match algorithm: exact match, substring, order-preserving match, no match, empty query
- Scoring: consecutive bonus, word boundary bonus, ranking verification

**`fc` built-in**
- Argument parsing: `-l`, `-ln`, `-lr`, `-s`, `-e editor`, `first`, `last`
- Range resolution: positive numbers, negative numbers, string prefix match
- `fc -s old=new` string substitution

### E2E Tests

- History file read/write: start kish → execute commands → exit → verify `~/.kish_history` contents
- `fc -l`: list display after command execution
- `fc -s`: substitution re-execution

### Manual Testing

Up/down navigation and Ctrl+R fuzzy search UI require terminal interaction and will be verified through manual testing. Unit tests cover the underlying logic layers.
