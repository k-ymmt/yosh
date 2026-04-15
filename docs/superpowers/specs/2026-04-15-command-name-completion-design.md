# Command Name Completion Design

## Summary

Add command name tab-completion in interactive mode. When the cursor is in command position (first word of a simple command), pressing Tab completes against PATH executables, builtin commands, and aliases instead of file paths.

## Motivation

Currently, kish only supports file/directory path completion. Command name completion is a fundamental shell UX feature supported by bash, zsh, and fish. Without it, users must type full command names manually.

## Design Decisions

- **Candidates:** PATH executables + builtins + aliases
- **Caching:** Lazy session cache with PATH change invalidation (fish-style)
- **Path fallback:** If the input word contains `/` in command position, fall back to path completion (e.g., `./script`, `/usr/bin/foo`)

## Architecture

### Command Position Detection

A new function `is_command_position(buf, word_start)` in `completion.rs` determines whether the cursor is at command position by scanning backward from `word_start`:

- **Command position:** line start (empty before word), or last non-whitespace char before word is `|`, `;`, `&`, `(`, `!`
- **Argument position:** everything else

Fallback rule:
```
is_command_position && !word_contains_slash → command name completion
is_command_position && word_contains_slash  → path completion (existing)
!is_command_position                        → path completion (existing)
```

### `CommandCompleter` (new file: `src/interactive/command_completion.rs`)

Owns the PATH executable cache and generates command name candidates.

```rust
pub struct CommandCompleter {
    /// Sorted list of executable names from PATH
    cached_executables: Vec<String>,
    /// PATH value when cache was built (for invalidation)
    cached_path: String,
}
```

**Cache lifecycle:**
- Held in `InteractiveShell` for the session lifetime
- Built lazily on first command-position Tab press
- Invalidated and rebuilt when current `PATH` differs from `cached_path`

**`rebuild_cache(path)`:**
- Split PATH by `:`
- Scan each directory for executable files (is_file + execute permission on Unix)
- Earlier PATH directories take priority (dedup by name)
- Sort and store

**`complete(prefix, path, builtins, aliases)` -> `Vec<String>`:**
1. Check if cache needs rebuild (PATH changed)
2. Collect prefix-matching candidates from aliases, builtins, and cached executables
3. Deduplicate and sort

### `CompletionContext` Extension

```rust
pub struct CompletionContext<'a> {
    pub cwd: String,
    pub home: String,
    pub show_dotfiles: bool,
    // New fields
    pub command_completer: &'a mut CommandCompleter,
    pub path: &'a str,
    pub builtins: &'a [&'a str],
    pub aliases: &'a HashMap<String, String>,
}
```

### Integration in `handle_tab_complete`

```rust
fn handle_tab_complete(...) -> io::Result<()> {
    let (word_start, word) = extract_completion_word(&self.buffer(), self.pos);

    if is_command_position(&self.buffer(), word_start) && !word.contains('/') {
        // Command name completion
        let candidates = ctx.command_completer.complete(
            word, ctx.path, ctx.builtins, ctx.aliases,
        );
        // ... same candidate display logic as existing path completion
    } else {
        // Path completion (existing logic)
        let result = completion::complete(&self.buffer(), self.pos, ctx);
        // ...
    }
}
```

### `InteractiveShell` Ownership

```rust
pub struct InteractiveShell {
    // ... existing fields
    command_completer: CommandCompleter,
}
```

Initialized in `InteractiveShell::new()`. Passed as `&mut` into `CompletionContext` each loop iteration.

### Builtin Names

A function or constant slice providing the list of all builtin command names from `src/builtin/special.rs` and `src/builtin/regular.rs`.

## Files Changed

| File | Change |
|------|--------|
| `src/interactive/command_completion.rs` | New — `CommandCompleter` struct |
| `src/interactive/completion.rs` | Add `is_command_position()` |
| `src/interactive/mod.rs` | Hold `CommandCompleter`, extend `CompletionContext` |
| `src/interactive/line_editor.rs` | Branch logic in `handle_tab_complete()` |
| `src/builtin/mod.rs` (or similar) | Expose builtin name list |

## Testing

### Unit Tests (`command_completion.rs`)

- `complete()` returns prefix-matched aliases, builtins, and PATH executables
- Deduplication: same name in alias and PATH returns only once
- `rebuild_cache()` with temp directory containing executable files
- Cache invalidation: changing PATH string triggers rebuild
- Empty prefix returns all candidates

### Unit Tests (`completion.rs`)

- `is_command_position()` cases:
  - Line start (`""`) → true
  - After pipe (`"ls | "`) → true
  - After semicolon (`"echo a; "`) → true
  - After `&&` / `||` → true
  - After `(` → true
  - After `!` → true
  - Argument position (`"ls "`) → false

### Integration Tests

- Command position Tab yields command name candidates
- `./` prefix falls back to path completion
- Argument position still uses path completion (regression)

### PTY Tests (`tests/pty_interactive.rs`)

- Type partial command name, press Tab, verify completion appears
- Verify command completion after pipe (`|`)
- Verify path completion in argument position still works
