# Interactive Mode Design

## Overview

Implement interactive mode for kish — the POSIX-compliant REPL loop with line editing via crossterm 0.29.

## Requirements

- **POSIX compliant** interactive shell behavior
- **crossterm 0.29** for terminal I/O (raw mode, key events)
- **Minimal line editing**: cursor movement, backspace/delete, home/end
- **PS1/PS2 prompt** with parameter expansion
- **No Bash extensions** — pure POSIX interactive shell
- Future enhancements (history, tab completion, Emacs keybindings, job control) deferred to later phases

## Module Structure

```
src/interactive/
  mod.rs          — Repl struct, REPL loop
  line_editor.rs  — LineEditor struct, crossterm raw mode + line editing
  prompt.rs       — PS1/PS2 expansion and display
```

## 1. Entry Point (`main.rs`)

```rust
if args.len() == 1 {
    if nix::unistd::isatty(0) {
        let mut repl = Repl::new();
        std::process::exit(repl.run());
    } else {
        // stdin is a pipe — read as script
        run_stdin();
    }
}
```

- Use `nix::unistd::isatty()` for TTY detection (no new dependency)
- Non-TTY stdin (`echo "ls" | kish`) treated as script execution

## 2. REPL Loop (`interactive/mod.rs`)

```
Repl::run()
  ├── Initialize ShellEnv
  ├── Initialize Executor
  ├── signal_init()
  └── loop {
        ├── Determine prompt var (PS1 if buffer empty, PS2 if continuing)
        ├── prompt::expand_prompt(&env, prompt_var)
        ├── Print prompt to stderr
        ├── line = LineEditor::read_line(prompt_width)?
        ├── Handle EOF (Ctrl+D): ignoreeof check
        ├── Append line to input_buffer
        ├── Try parse input_buffer
        │   ├── Complete → exec_complete_command(), clear buffer
        │   ├── Incomplete → continue (PS2 on next iteration)
        │   └── Error → eprintln, clear buffer, continue
        ├── drain_pending_signals() + execute traps
        └── Continue loop
      }
```

### Parse result classification

The parser must distinguish between:
- **Complete**: A valid complete command was parsed
- **Incomplete**: Input consumed but syntax not closed (e.g., `if` without `fi`)
- **Error**: Actual syntax error (e.g., `if ; then`)

Implementation detail: verify at implementation time whether the existing parser's error reporting can distinguish incomplete from erroneous input.

## 3. LineEditor (`interactive/line_editor.rs`)

### Struct

```rust
pub struct LineEditor {
    buffer: Vec<char>,  // Input buffer (character-level for UTF-8)
    cursor: usize,      // Cursor position within buffer
}
```

### Public API

```rust
impl LineEditor {
    pub fn new() -> Self;
    pub fn read_line(&mut self, prompt_width: usize) -> Result<Option<String>, io::Error>;
    // Ok(Some(line)) — input received
    // Ok(None)       — EOF (Ctrl+D on empty buffer)
    // Err(_)         — I/O error
}
```

### Raw mode scope

- `enable_raw_mode()` at the start of `read_line()`
- `disable_raw_mode()` guaranteed on exit via Drop guard pattern
- Commands execute in cooked mode (raw mode disabled before exec, re-enabled after)

### Key bindings

| Key | Action |
|-----|--------|
| Printable char | Insert at cursor, shift right |
| ← / Ctrl+B | Move cursor left |
| → / Ctrl+F | Move cursor right |
| Home / Ctrl+A | Move cursor to start |
| End / Ctrl+E | Move cursor to end |
| Backspace | Delete char before cursor |
| Delete | Delete char at cursor |
| Enter | Submit line |
| Ctrl+D | EOF if buffer empty, else Delete |
| Ctrl+C | Discard input, new prompt (SIGINT) |

### Screen rendering

- On insert/delete, redraw from cursor position to end of line
- Use `crossterm::cursor::MoveToColumn` with prompt width offset for cursor positioning
- Internal editing methods (`insert_char`, `delete_char`, `move_cursor`) extracted as testable units

## 4. Prompt (`interactive/prompt.rs`)

### API

```rust
pub fn expand_prompt(env: &ShellEnv, var_name: &str) -> String;
```

### Behavior

- Retrieves PS1 or PS2 from `ShellEnv`
- Default values:
  - PS1: `"$ "` (or `"# "` if UID == 0)
  - PS2: `"> "`
- Runs existing `expand::expand_word()` for parameter expansion and command substitution
- Outputs to stderr (stdout reserved for command output)

### Prompt width

- Calculated as `expanded_string.chars().count()`
- Accurate width for control characters / escape sequences deferred to future phase

## 5. POSIX Interactive-Specific Behavior

### 5.1 Syntax errors do not exit the shell

Non-interactive mode exits on syntax errors. Interactive mode prints the error and returns to the prompt.

### 5.2 ignoreeof

When `ignoreeof` is set (via `set -o ignoreeof` or as a variable), Ctrl+D on an empty line prints a message instead of exiting:

```
kish: Use "exit" to leave the shell.
```

### 5.3 SIGINT handling

- **During command execution**: Existing signal handling forwards SIGINT to child process
- **During line input**: LineEditor detects Ctrl+C as key event → discard current input → print newline → display new prompt

### 5.4 PS2 continuation

When the parser determines input is incomplete:
1. Display PS2 prompt
2. Read additional line
3. Append to input buffer with newline
4. Re-parse the accumulated buffer
5. Repeat until complete or error

## 6. Test Strategy

### Unit tests

- **LineEditor internals**: Test `insert_char`, `delete_char`, `move_cursor` methods against buffer/cursor state assertions. These methods are crossterm-independent.
- **Prompt expansion**: Set PS1/PS2 variables in ShellEnv → call `expand_prompt()` → verify output string. Test default values and UID=0 case.

### E2E tests

- Variable expansion: `kish -c 'echo "$PS1"'` to verify variable behavior
- ignoreeof: `echo exit | kish` exits normally
- Syntax error recovery: Verify interactive mode doesn't exit on parse errors

### Constraints

- Interactive E2E tests requiring a real TTY may need to be skipped in CI
- `is_interactive` flag in `ShellEnv` enables unit testing of behavior branches without TTY

## 7. Dependencies

Add to `Cargo.toml`:

```toml
crossterm = "0.29"
```

No other new dependencies. TTY detection uses existing `nix` crate.
