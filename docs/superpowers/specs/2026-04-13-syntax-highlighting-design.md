# Syntax Highlighting for Interactive Mode

**Date:** 2026-04-13
**Status:** Approved

## Overview

Add real-time syntax highlighting to kish's interactive mode input, including fish-style dynamic command existence checking. A dedicated lightweight scanner (Approach B) is used instead of reusing the existing Lexer, for better performance, extensibility, and error tolerance.

## Requirements

- Highlight keywords, operators, redirections, strings, variables, command substitutions, arithmetic substitutions, comments, tilde, IO numbers, and assignments
- Dynamic command existence checking: valid commands in green, invalid commands in red (fish-style)
- Active error visualization: unclosed quotes, unterminated expansions shown in red with underline
- Incremental caching with checkpoint-based rescanning for performance
- Multi-line (PS2) context awareness: scan accumulated buffer for context, display highlights for current line only
- Fixed color palette initially; customization deferred to future work

## Architecture

```
Input buffer (Vec<char>)
      |
      v
HighlightScanner -- scan() --> Vec<ColorSpan>
      |                              |
      | (cache: state snapshots)     |
      |                              v
      |                      LineEditor::redraw()
      |                         per-span color switching
      |
      +-- ScannerState (mode + position)
      |     +-- incremental: rescan from change point
      |
      +-- CommandChecker
            +-- classify_builtin()  (existing)
            +-- AliasStore lookup   (existing)
            +-- PATH search + cache
```

### Data Flow

1. Key input mutates `buf`
2. Before `redraw()`, call `HighlightScanner::scan()`
3. Identify change point from previous cache, resume scanning from nearest checkpoint
4. Return `Vec<ColorSpan>` (each span = char range + style)
5. `redraw()` iterates spans, switching terminal colors per span

### Separation of Concerns

- `HighlightScanner` (`src/interactive/highlight.rs`): input text -> token classification (pure scanning)
- `CommandChecker` (`src/interactive/highlight.rs`): command name -> existence check (PATH I/O)
- `terminal.rs`: color output (Terminal trait extension)
- `line_editor.rs`: rendering with scan results (extended redraw)

## Token Classification and Color Palette

### HighlightStyle Enum

```rust
enum HighlightStyle {
    Default,        // Unclassified text -- terminal default
    Keyword,        // if, then, else, elif, fi, for, do, done, while, until,
                    // case, esac, in, !, {, }
    Operator,       // |, &&, ||, ;, &, ;;
    Redirect,       // <, >, >>, <<, <&, >&, <>, >|, <<-
    String,         // 'single quoted', $'dollar single quoted'
    DoubleString,   // "double quoted" (internal expansions override with own style)
    Variable,       // $var, ${var}, ${var:-default}, $1, $@, $? etc.
    CommandSub,     // $( ) and ` ` delimiters
    ArithSub,       // $(( )) delimiters
    Comment,        // # to end of line
    CommandValid,   // Existing command (builtin/alias/PATH)
    CommandInvalid, // Non-existing command
    IoNumber,       // Digits before redirection (2>, 1>&2)
    Assignment,     // VAR= portion of VAR=value
    Tilde,          // ~ in ~/path
    Error,          // Unclosed quotes, invalid syntax
}
```

### Fixed Color Palette (ANSI basic 16 colors)

| Style | Color | Rationale |
|---|---|---|
| Default | (reset) | Follow terminal default |
| Keyword | Bold + Magenta | Make control structures stand out (fish/zsh convention) |
| Operator | Cyan | Structural but not dominant |
| Redirect | Cyan | Same family as Operator |
| String | Yellow | Standard convention for quoted strings |
| DoubleString | Yellow | Same family as String (internal expansions override) |
| Variable | Bold + Green | Highlight expansion elements |
| CommandSub | Bold + Yellow | Emphasize $( ) delimiters |
| ArithSub | Bold + Yellow | Same family as CommandSub |
| Comment | Dim + Default | Subdued display |
| CommandValid | Bold + Green | Intuitively signals "executable" |
| CommandInvalid | Bold + Red | Immediate warning for "not found" |
| IoNumber | Blue | Redirection accessory element |
| Assignment | Blue | Distinguish variable definitions |
| Tilde | Green | Same family as Variable |
| Error | Red + Underline | Highlight error locations |

### ColorSpan

```rust
struct ColorSpan {
    start: usize,  // char index in buffer (inclusive)
    end: usize,    // char index in buffer (exclusive)
    style: HighlightStyle,
}
```

Char indices are used because `LineEditor.buf` is `Vec<char>` and rendering outputs chars one at a time.

## Highlight Scanner Design

### State Machine

The scanner processes input one character at a time, assigning styles based on the current mode.

```
            +-------------+
     ------>|   Normal    |<--------------------+
            +--+-+-+-+-+--+                     |
               | | | | |                       |
          '    " $  # $( `                  close
               | | | | |                       |
            +--+ ++ +--+ +---+ +-----+ +------+
            |SQ ||DQ||Var||Cmt||CmdSub||Btick |
            +---++--++---++---++------++------+
                  |
             $ ` $( internal expansions
                  |
             nesting (Variable, CmdSub inside DQ)
```

### Modes

| Mode | Entry | Exit | Style |
|---|---|---|---|
| Normal | default | -- | context-dependent |
| SingleQuote | `'` | `'` | String |
| DoubleQuote | `"` | `"` | DoubleString |
| DollarSingleQuote | `$'` | `'` | String |
| Parameter | `$` or `${` | name end or `}` | Variable |
| CommandSub | `$(` | `)` | delimiters=CommandSub, body=push Normal mode onto stack (full syntax scanning inside) |
| Backtick | `` ` `` | `` ` `` | delimiters=CommandSub, body=push Normal mode onto stack (full syntax scanning inside) |
| ArithSub | `$((` | `))` | ArithSub |
| Comment | `#` (at word start position) | end of line | Comment |

### Mode Stack

A stack manages nesting of modes (e.g., `$(...)` inside double quotes).

```rust
struct ScannerState {
    mode_stack: Vec<ScanMode>,
    pos: usize,                 // current char index
    word_start: bool,           // at word start (for # comment detection)
    command_position: bool,     // at command name position
    after_assignment: bool,     // after = in VAR=value
}

enum ScanMode {
    Normal,
    SingleQuote { start: usize },
    DoubleQuote { start: usize },
    DollarSingleQuote { start: usize },
    Parameter { start: usize, braced: bool },
    CommandSub { start: usize, depth: usize },
    Backtick { start: usize },
    ArithSub { start: usize },
    Comment { start: usize },
}
```

### Command Position Detection

Rules for identifying command name positions without the parser:

1. **Line start** (after leading whitespace) -> command position
2. **After `|`** -> command position
3. **After `&&` / `||`** -> command position
4. **After `;` / `&`** -> command position
5. **After `(`** -> command position (subshell)
6. **After keywords `then`, `else`, `do`, `!`** -> command position
7. **Skip `VAR=value` patterns** -> position after them is command position

When a Word is encountered at a command position: check for keyword first, then use `CommandChecker` for existence check.

### Accumulated Buffer Support (PS2 Multi-line)

```rust
impl HighlightScanner {
    /// accumulated: previous lines accumulated input (PS2 state)
    /// current: currently editing line
    /// checker_env: lightweight view into shell environment (PATH, aliases)
    /// returns: ColorSpans for current only (indices relative to current)
    fn scan(
        &mut self,
        accumulated: &str,
        current: &[char],
        checker_env: &CheckerEnv,
    ) -> Vec<ColorSpan>;
}
```

The scanner uses its internal `CommandChecker` (accessed via `&mut self`) with the provided `CheckerEnv` for command existence lookups. The accumulated buffer is scanned to determine the ending mode (e.g., inside double quote), and that mode becomes the initial state for scanning the current line. The accumulated buffer scan result is cached.

## Command Existence Checker

### Structure

```rust
struct CommandChecker {
    path_cache: HashMap<String, bool>,
    cached_path: String,
}
```

### Check Order

1. **Special builtin**: `classify_builtin(name) == Special` -> Valid
2. **Regular builtin**: `classify_builtin(name) == Regular` -> Valid
3. **Alias**: `AliasStore::get(name).is_some()` -> Valid
4. **Shell function**: function table lookup -> Valid (future)
5. **PATH search**: check executable file in PATH directories -> Valid / Invalid

### PATH Search Optimization

- **Cache**: store results in `path_cache` by command name
- **PATH change detection**: compare PATH value on each `scan()` call; clear cache if changed
- **Lazy search**: only search when a Word appears at command position
- **Direct path check**: paths containing `/` (e.g., `./script`, `/usr/bin/foo`) are checked directly without PATH search

### CheckerEnv

```rust
struct CheckerEnv<'a> {
    path: &'a str,
    aliases: &'a AliasStore,
}
```

Lightweight view into ShellEnv, constructed in `Repl` and passed to `scan()`. Avoids direct ShellEnv dependency for testability.

## Incremental Cache Strategy

### Cache Structure

```rust
struct HighlightCache {
    prev_input: Vec<char>,
    prev_spans: Vec<ColorSpan>,
    checkpoints: Vec<(usize, ScannerState)>,
    checkpoint_interval: usize,  // default: 32
}
```

### Incremental Scan Algorithm

1. Compare `prev_input` and `current_input`, find first differing position `diff_pos`
2. Find nearest checkpoint before `diff_pos` -> `checkpoint_pos`, `checkpoint_state`
3. Resume scanning from `checkpoint_state` to end of `current_input`
4. Retain `prev_spans` before `checkpoint_pos`, replace remainder with new results
5. Update checkpoints

### Common Case Optimization

**Append/delete at end (cursor at end):** `diff_pos` is near the end, so only a few characters are rescanned from the last checkpoint. With interval=32, worst case is 32 characters of rescanning.

**Mid-buffer edit:** Rewind to nearest checkpoint before `diff_pos`. For typical shell input (<100 chars), even a full rescan is fast, but checkpoints avoid unnecessary work.

### Accumulated Buffer Cache

```rust
struct HighlightScanner {
    cache: HighlightCache,
    accumulated_state: Option<(String, ScannerState)>,
    checker: CommandChecker,
}
```

`accumulated_state` is only recomputed when the accumulated buffer content changes (new line added on Enter).

### Cache Invalidation

| Event | Action |
|---|---|
| Char insert/delete | Incremental scan |
| History navigation (Up/Down) | Full rescan (entire buffer changes) |
| Suggestion accept | Incremental scan (append at end) |
| Tab completion | Incremental scan from diff_pos (word replacement) |
| Enter (PS2 transition) | Update accumulated_state, clear current line cache |
| PATH change | Clear CommandChecker path_cache, recheck command spans only |

## LineEditor and Terminal Integration

### Terminal Trait Extensions

```rust
// Added to Terminal trait
fn set_fg_color(&mut self, color: Color) -> io::Result<()>;
fn reset_style(&mut self) -> io::Result<()>;
fn set_bold(&mut self, on: bool) -> io::Result<()>;
fn set_underline(&mut self, on: bool) -> io::Result<()>;
fn write_char(&mut self, ch: char) -> io::Result<()>;
```

Uses `crossterm::style::Color` directly (no new color type).

### Fix Existing Attribute::Reset Issue

The existing `set_dim(false)` / `set_reverse(false)` use `Attribute::Reset` which resets ALL attributes (noted in TODO.md). This is fixed as part of this work:

- `set_dim(false)` -> `Attribute::NoDim`
- `set_reverse(false)` -> `Attribute::NoReverse`
- Only `reset_style()` performs full `Attribute::Reset`

### Style Application

```rust
fn apply_style<T: Terminal>(term: &mut T, style: HighlightStyle) -> io::Result<()> {
    term.reset_style()?;
    match style {
        Default => {},
        Keyword => { term.set_bold(true)?; term.set_fg_color(Color::Magenta)?; },
        Operator | Redirect => { term.set_fg_color(Color::Cyan)?; },
        String | DoubleString => { term.set_fg_color(Color::Yellow)?; },
        Variable | Tilde => { term.set_bold(true)?; term.set_fg_color(Color::Green)?; },
        CommandSub | ArithSub => { term.set_bold(true)?; term.set_fg_color(Color::Yellow)?; },
        Comment => { term.set_dim(true)?; },
        CommandValid => { term.set_bold(true)?; term.set_fg_color(Color::Green)?; },
        CommandInvalid => { term.set_bold(true)?; term.set_fg_color(Color::Red)?; },
        IoNumber | Assignment => { term.set_fg_color(Color::Blue)?; },
        Error => { term.set_fg_color(Color::Red)?; term.set_underline(true)?; },
    }
    Ok(())
}
```

### Modified redraw()

The current `redraw()` outputs the entire buffer in a single `write_str`. It changes to per-char output with span-based color switching.

### API Change

```rust
pub fn read_line_with_completion<T: Terminal>(
    &mut self,
    prompt: &str,
    history: &mut History,
    term: &mut T,
    ctx: &CompletionContext,
    scanner: &mut HighlightScanner,  // added
    accumulated: &str,               // added: PS2 accumulated buffer
) -> io::Result<Option<String>>;
```

`HighlightScanner` is owned by `Repl` (not `LineEditor`) to keep `LineEditor` free of shell environment dependencies.

## Error Handling

### Scanner Error Tolerance

The scanner never panics or returns `Result::Err`. All invalid input is expressed through style selection.

**Principle: The scanner always returns `Vec<ColorSpan>`.**

### Error Patterns

| Pattern | Detection | Display |
|---|---|---|
| Unclosed single quote `echo 'hello` | SingleQuote mode remains at scan end | `Error` style from opening `'` to end |
| Unclosed double quote `echo "hello` | DoubleQuote mode remains at scan end | `Error` style from opening `"` to end |
| Unclosed command sub `echo $(cmd` | CommandSub mode remains at scan end | `Error` style on `$(` |
| Unclosed parameter `echo ${var` | Parameter mode remains at scan end | `Error` style on `${` |
| Non-existent command `foobar arg` | CommandChecker returns Invalid | `CommandInvalid` style |
| Unclosed backtick `` echo `cmd `` | Backtick mode remains at scan end | `Error` style from opening `` ` `` to end |

### PS2 Continuation Distinction

In PS2 (multi-line input), unclosed modes are NOT errors -- they are in-progress input:

- PS1: unclosed mode at scan end -> `Error` style
- PS2: unclosed mode that was inherited from accumulated buffer -> normal style for that mode

Example: PS1 with `echo "hello` -> `"hello` displayed as `Error` (red+underline).
PS2 with `world"` -> `world"` displayed as `DoubleString` (yellow, normal).

### Fallback

If the scanner returns an empty `Vec<ColorSpan>`, `redraw()` renders all characters in `Default` style -- identical to current behavior without highlighting. Highlighting issues never affect shell input functionality.

## Testing Strategy

### Unit Tests (in `src/interactive/highlight.rs`)

**1. Basic token classification tests**

```rust
// Keywords
assert_spans("if true; then echo hi; fi", &[
    (0..2, Keyword),      // if
    (3..7, CommandValid),  // true
    (7..8, Operator),      // ;
    (9..13, Keyword),      // then
    ...
]);
```

**2. Command existence checker tests**

```rust
let env = CheckerEnv { path: "/usr/bin", aliases: &empty_aliases };
assert_eq!(checker.check("cd", &env), CommandExistence::Valid);     // builtin
assert_eq!(checker.check("xyzzy", &env), CommandExistence::Invalid);
```

**3. Error display tests**

```rust
// Unclosed quote at PS1
assert_spans_ps1("echo 'hello", &[
    (0..4, CommandValid),
    (5..11, Error),
]);

// Unclosed quote at PS2 (not an error)
assert_spans_ps2("world'", "echo 'hello\n", &[
    (0..6, String),
]);
```

**4. Incremental cache tests**

```rust
// Append at end
scanner.scan("", &['e','c','h'], &checker);
let spans = scanner.scan("", &['e','c','h','o'], &checker);
// Verify echo becomes CommandValid after 'o' added

// Mid-buffer edit
scanner.scan("", &['e','c','h','o',' ','h','i'], &checker);
let spans = scanner.scan("", &['e','c','h','o',' ','X','i'], &checker);
// Verify rescan from diff_pos=5
```

**5. Command position detection tests**

```rust
// After pipe
assert_spans("ls | grep foo", &[
    (0..2, CommandValid),
    (3..4, Operator),
    (5..9, CommandValid),
    ...
]);

// Assignment skip
assert_spans("VAR=val cmd", &[
    (0..4, Assignment),
    (4..7, Default),
    (8..11, CommandValid),
]);
```

### PTY-based E2E Tests (in `tests/pty_interactive.rs`)

Verify ANSI escape sequences appear in actual kish output:

```rust
#[test]
fn test_syntax_highlight_keyword() {
    // Start kish, type "if"
    // Verify Magenta color escape sequence in output
}

#[test]
fn test_syntax_highlight_invalid_command() {
    // Type "xyznotfound"
    // Verify Red color escape sequence in output
}
```

PTY tests follow existing patterns (expectrl crate, generous timeouts).

### Test Helpers

```rust
fn assert_spans(input: &str, expected: &[(Range<usize>, HighlightStyle)]);
fn assert_spans_ps1(input: &str, expected: &[(Range<usize>, HighlightStyle)]);
fn assert_spans_ps2(input: &str, accumulated: &str, expected: &[(Range<usize>, HighlightStyle)]);
```

`CommandChecker` uses a mock for tests: builtins and aliases return Valid, everything else returns Invalid. PATH search is tested separately with `tempdir` containing executable files.
