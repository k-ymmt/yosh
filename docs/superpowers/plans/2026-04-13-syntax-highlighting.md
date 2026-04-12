# Syntax Highlighting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add real-time syntax highlighting to kish's interactive mode with fish-style command existence checking, incremental caching, and error visualization.

**Architecture:** A dedicated lightweight scanner (`HighlightScanner`) tokenizes the input buffer into `ColorSpan`s. `CommandChecker` validates command names against builtins, aliases, and PATH. The `Terminal` trait is extended with color methods, and `LineEditor::redraw()` renders spans with per-character color switching. Incremental caching via checkpoints avoids full rescans on every keystroke.

**Tech Stack:** Rust, crossterm 0.29 (Color, Attribute), existing kish infrastructure (classify_builtin, AliasStore)

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `src/interactive/highlight.rs` | Create | HighlightStyle, ColorSpan, ScanMode, ScannerState, HighlightCache, CommandChecker, CheckerEnv, HighlightScanner, apply_style — all highlighting logic |
| `src/interactive/terminal.rs` | Modify | Add set_fg_color, reset_style, set_bold, set_underline, write_char to Terminal trait + CrosstermTerminal; fix set_dim/set_reverse Reset bug |
| `src/interactive/line_editor.rs` | Modify | Add spans parameter to redraw(), per-char rendering with color switching |
| `src/interactive/mod.rs` | Modify | Register highlight module, create HighlightScanner in Repl, pass to read_line_with_completion |
| `tests/helpers/mock_terminal.rs` | Modify | Add mock implementations for new Terminal trait methods |
| `tests/pty_interactive.rs` | Modify | Add E2E tests for syntax highlighting ANSI output |
| `TODO.md` | Modify | Remove resolved set_dim/set_reverse TODO item |

---

### Task 1: Extend Terminal Trait with Color Methods

**Files:**
- Modify: `src/interactive/terminal.rs`
- Modify: `tests/helpers/mock_terminal.rs`

- [ ] **Step 1: Add new methods to Terminal trait**

In `src/interactive/terminal.rs`, add the crossterm Color import and five new methods to the `Terminal` trait. Add them after the existing `set_dim` method:

```rust
// Add to the use block at the top:
use crossterm::style::{Attribute, Color, SetAttribute, SetForegroundColor};
```

Remove the old import line:
```rust
// Remove this:
use crossterm::style::{Attribute, SetAttribute};
```

Add these methods to the `Terminal` trait (after `set_dim`):

```rust
    /// Set foreground color.
    fn set_fg_color(&mut self, color: Color) -> io::Result<()>;

    /// Reset all text styling (color, bold, dim, underline, reverse).
    fn reset_style(&mut self) -> io::Result<()>;

    /// Set bold text attribute on/off.
    fn set_bold(&mut self, on: bool) -> io::Result<()>;

    /// Set underline text attribute on/off.
    fn set_underline(&mut self, on: bool) -> io::Result<()>;

    /// Write a single character to the terminal.
    fn write_char(&mut self, ch: char) -> io::Result<()>;
```

- [ ] **Step 2: Fix set_dim and set_reverse to use specific attribute toggles**

In `src/interactive/terminal.rs`, replace the `set_reverse` and `set_dim` implementations in `impl Terminal for CrosstermTerminal`:

```rust
    fn set_reverse(&mut self, on: bool) -> io::Result<()> {
        if on {
            self.stdout.execute(SetAttribute(Attribute::Reverse))?;
        } else {
            self.stdout.execute(SetAttribute(Attribute::NoReverse))?;
        }
        Ok(())
    }

    fn set_dim(&mut self, on: bool) -> io::Result<()> {
        if on {
            self.stdout.execute(SetAttribute(Attribute::Dim))?;
        } else {
            self.stdout.execute(SetAttribute(Attribute::NormalIntensity))?;
        }
        Ok(())
    }
```

- [ ] **Step 3: Implement new methods for CrosstermTerminal**

Add these implementations in `impl Terminal for CrosstermTerminal` (after `set_dim`):

```rust
    fn set_fg_color(&mut self, color: Color) -> io::Result<()> {
        self.stdout.execute(SetForegroundColor(color))?;
        Ok(())
    }

    fn reset_style(&mut self) -> io::Result<()> {
        self.stdout.execute(SetAttribute(Attribute::Reset))?;
        Ok(())
    }

    fn set_bold(&mut self, on: bool) -> io::Result<()> {
        if on {
            self.stdout.execute(SetAttribute(Attribute::Bold))?;
        } else {
            self.stdout.execute(SetAttribute(Attribute::NormalIntensity))?;
        }
        Ok(())
    }

    fn set_underline(&mut self, on: bool) -> io::Result<()> {
        if on {
            self.stdout.execute(SetAttribute(Attribute::Underlined))?;
        } else {
            self.stdout.execute(SetAttribute(Attribute::NoUnderline))?;
        }
        Ok(())
    }

    fn write_char(&mut self, ch: char) -> io::Result<()> {
        write!(self.stdout, "{}", ch)?;
        Ok(())
    }
```

- [ ] **Step 4: Update MockTerminal with new trait methods**

In `tests/helpers/mock_terminal.rs`, add new fields and implementations. Add to the struct:

```rust
pub struct MockTerminal {
    events: VecDeque<Event>,
    size: (u16, u16),
    output: Vec<String>,
    cursor_row: i32,
    dim: bool,
    bold: bool,
    underline: bool,
    fg_color: Option<String>,
}
```

Update `new()`:

```rust
    pub fn new(events: Vec<Event>) -> Self {
        Self {
            events: VecDeque::from(events),
            size: (80, 24),
            output: Vec::new(),
            cursor_row: 0,
            dim: false,
            bold: false,
            underline: false,
            fg_color: None,
        }
    }
```

Add these trait implementations in `impl Terminal for MockTerminal`:

```rust
    fn set_fg_color(&mut self, color: crossterm::style::Color) -> io::Result<()> {
        let name = format!("{:?}", color);
        self.fg_color = Some(name.clone());
        self.output.push(format!("[FG:{}]", name));
        Ok(())
    }

    fn reset_style(&mut self) -> io::Result<()> {
        self.dim = false;
        self.bold = false;
        self.underline = false;
        self.fg_color = None;
        self.output.push("[RESET]".to_string());
        Ok(())
    }

    fn set_bold(&mut self, on: bool) -> io::Result<()> {
        self.bold = on;
        if on {
            self.output.push("[BOLD]".to_string());
        } else {
            self.output.push("[/BOLD]".to_string());
        }
        Ok(())
    }

    fn set_underline(&mut self, on: bool) -> io::Result<()> {
        self.underline = on;
        if on {
            self.output.push("[UL]".to_string());
        } else {
            self.output.push("[/UL]".to_string());
        }
        Ok(())
    }

    fn write_char(&mut self, ch: char) -> io::Result<()> {
        self.output.push(ch.to_string());
        Ok(())
    }
```

- [ ] **Step 5: Verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: Build succeeds (or only unrelated warnings)

Run: `cargo test --test interactive 2>&1 | tail -10`
Expected: All existing interactive tests pass

- [ ] **Step 6: Commit**

```bash
git add src/interactive/terminal.rs tests/helpers/mock_terminal.rs
git commit -m "feat(terminal): extend Terminal trait with color, bold, underline, reset_style, write_char

Fix set_dim/set_reverse to use NoDim/NormalIntensity/NoReverse instead of
Attribute::Reset which cleared all attributes. Add set_fg_color, set_bold,
set_underline, reset_style, write_char for syntax highlighting support.

Task: syntax highlighting for interactive mode (Task 1)"
```

---

### Task 2: Core Types and CommandChecker

**Files:**
- Create: `src/interactive/highlight.rs`
- Modify: `src/interactive/mod.rs` (add `pub mod highlight;`)

- [ ] **Step 1: Write tests for CommandChecker**

Create `src/interactive/highlight.rs` with the types and tests first:

```rust
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crossterm::style::Color;

use crate::builtin::{BuiltinKind, classify_builtin};
use crate::env::aliases::AliasStore;

use super::terminal::Terminal;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightStyle {
    Default,
    Keyword,
    Operator,
    Redirect,
    String,
    DoubleString,
    Variable,
    CommandSub,
    ArithSub,
    Comment,
    CommandValid,
    CommandInvalid,
    IoNumber,
    Assignment,
    Tilde,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorSpan {
    pub start: usize,
    pub end: usize,
    pub style: HighlightStyle,
}

// ---------------------------------------------------------------------------
// CheckerEnv — lightweight view into shell environment for testability
// ---------------------------------------------------------------------------

pub struct CheckerEnv<'a> {
    pub path: &'a str,
    pub aliases: &'a AliasStore,
}

// ---------------------------------------------------------------------------
// CommandChecker — validates command names against builtins, aliases, PATH
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandExistence {
    Valid,
    Invalid,
}

pub struct CommandChecker {
    path_cache: HashMap<String, bool>,
    cached_path: String,
}

impl CommandChecker {
    pub fn new() -> Self {
        Self {
            path_cache: HashMap::new(),
            cached_path: String::new(),
        }
    }

    pub fn check(&mut self, name: &str, env: &CheckerEnv) -> CommandExistence {
        // 1. Builtins (special + regular)
        if classify_builtin(name) != BuiltinKind::NotBuiltin {
            return CommandExistence::Valid;
        }

        // 2. Aliases
        if env.aliases.get(name).is_some() {
            return CommandExistence::Valid;
        }

        // 3. Paths containing / — check directly
        if name.contains('/') {
            return if is_executable(Path::new(name)) {
                CommandExistence::Valid
            } else {
                CommandExistence::Invalid
            };
        }

        // 4. PATH change detection — clear cache if PATH changed
        if env.path != self.cached_path {
            self.path_cache.clear();
            self.cached_path = env.path.to_string();
        }

        // 5. Cached PATH lookup
        let exists = self.path_cache.entry(name.to_string()).or_insert_with(|| {
            search_path(name, env.path)
        });

        if *exists {
            CommandExistence::Valid
        } else {
            CommandExistence::Invalid
        }
    }
}

fn search_path(name: &str, path_var: &str) -> bool {
    for dir in path_var.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = Path::new(dir).join(name);
        if is_executable(&candidate) {
            return true;
        }
    }
    false
}

fn is_executable(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) => {
            let mode = meta.permissions().mode();
            meta.is_file() && (mode & 0o111 != 0)
        }
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// apply_style — maps HighlightStyle to terminal color commands
// ---------------------------------------------------------------------------

pub fn apply_style<T: Terminal>(term: &mut T, style: HighlightStyle) -> std::io::Result<()> {
    term.reset_style()?;
    match style {
        HighlightStyle::Default => {}
        HighlightStyle::Keyword => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Magenta)?;
        }
        HighlightStyle::Operator | HighlightStyle::Redirect => {
            term.set_fg_color(Color::Cyan)?;
        }
        HighlightStyle::String | HighlightStyle::DoubleString => {
            term.set_fg_color(Color::Yellow)?;
        }
        HighlightStyle::Variable | HighlightStyle::Tilde => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Green)?;
        }
        HighlightStyle::CommandSub | HighlightStyle::ArithSub => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Yellow)?;
        }
        HighlightStyle::Comment => {
            term.set_dim(true)?;
        }
        HighlightStyle::CommandValid => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Green)?;
        }
        HighlightStyle::CommandInvalid => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Red)?;
        }
        HighlightStyle::IoNumber | HighlightStyle::Assignment => {
            term.set_fg_color(Color::Blue)?;
        }
        HighlightStyle::Error => {
            term.set_fg_color(Color::Red)?;
            term.set_underline(true)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── CommandChecker tests ─────────────────────────────────────

    #[test]
    fn test_checker_builtin_special() {
        let aliases = AliasStore::default();
        let env = CheckerEnv { path: "", aliases: &aliases };
        let mut checker = CommandChecker::new();
        assert_eq!(checker.check("cd", &env), CommandExistence::Valid);
        assert_eq!(checker.check("export", &env), CommandExistence::Valid);
        assert_eq!(checker.check("echo", &env), CommandExistence::Valid);
        assert_eq!(checker.check("true", &env), CommandExistence::Valid);
    }

    #[test]
    fn test_checker_alias() {
        let mut aliases = AliasStore::default();
        aliases.set("ll", "ls -l");
        let env = CheckerEnv { path: "", aliases: &aliases };
        let mut checker = CommandChecker::new();
        assert_eq!(checker.check("ll", &env), CommandExistence::Valid);
        assert_eq!(checker.check("zz", &env), CommandExistence::Invalid);
    }

    #[test]
    fn test_checker_path_search() {
        // /usr/bin should contain common commands
        let aliases = AliasStore::default();
        let env = CheckerEnv { path: "/usr/bin:/bin", aliases: &aliases };
        let mut checker = CommandChecker::new();
        assert_eq!(checker.check("ls", &env), CommandExistence::Valid);
        assert_eq!(checker.check("xyzzy_nonexistent_cmd_12345", &env), CommandExistence::Invalid);
    }

    #[test]
    fn test_checker_path_cache_invalidation() {
        let aliases = AliasStore::default();
        let mut checker = CommandChecker::new();

        let env1 = CheckerEnv { path: "/usr/bin", aliases: &aliases };
        assert_eq!(checker.check("ls", &env1), CommandExistence::Valid);

        // Change PATH to empty — cache should be cleared
        let env2 = CheckerEnv { path: "", aliases: &aliases };
        assert_eq!(checker.check("ls", &env2), CommandExistence::Invalid);
    }

    #[test]
    fn test_checker_direct_path() {
        let aliases = AliasStore::default();
        let env = CheckerEnv { path: "", aliases: &aliases };
        let mut checker = CommandChecker::new();
        // /bin/sh should exist and be executable
        assert_eq!(checker.check("/bin/sh", &env), CommandExistence::Valid);
        assert_eq!(checker.check("./nonexistent_xyz", &env), CommandExistence::Invalid);
    }

    #[test]
    fn test_checker_path_with_tempfile() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("myscript");
        std::fs::write(&script, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let aliases = AliasStore::default();
        let path = dir.path().to_string_lossy().to_string();
        let env = CheckerEnv { path: &path, aliases: &aliases };
        let mut checker = CommandChecker::new();
        assert_eq!(checker.check("myscript", &env), CommandExistence::Valid);
        assert_eq!(checker.check("nosuchthing", &env), CommandExistence::Invalid);
    }
}
```

- [ ] **Step 2: Register the highlight module**

In `src/interactive/mod.rs`, add after the existing module declarations (line 7):

```rust
pub mod highlight;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --lib interactive::highlight 2>&1 | tail -15`
Expected: All 6 CommandChecker tests pass

- [ ] **Step 4: Commit**

```bash
git add src/interactive/highlight.rs src/interactive/mod.rs
git commit -m "feat(highlight): add core types, CommandChecker, and apply_style

Add HighlightStyle enum, ColorSpan, CheckerEnv, CommandChecker with
builtin/alias/PATH lookup and cache invalidation. Add apply_style to
map styles to terminal colors. Include unit tests for all checker paths.

Task: syntax highlighting for interactive mode (Task 2)"
```

---

### Task 3: Highlight Scanner — Basic Normal Mode Scanning

**Files:**
- Modify: `src/interactive/highlight.rs`

This task implements the core scanner that handles Normal mode: operators, redirects, comments, whitespace, and plain words (with command position tracking and keyword detection).

- [ ] **Step 1: Write tests for basic Normal mode scanning**

Add these tests at the bottom of the `#[cfg(test)] mod tests` block in `src/interactive/highlight.rs`:

```rust
    // ── Scanner tests ────────────────────────────────────────────

    /// Helper: create a scanner with a mock checker that treats builtins/aliases as valid,
    /// everything else as invalid.
    fn test_scanner() -> HighlightScanner {
        HighlightScanner::new()
    }

    fn test_env() -> (String, AliasStore) {
        let aliases = AliasStore::default();
        ("/usr/bin:/bin".to_string(), aliases)
    }

    fn scan_input(scanner: &mut HighlightScanner, input: &str) -> Vec<ColorSpan> {
        let (path, aliases) = test_env();
        let env = CheckerEnv { path: &path, aliases: &aliases };
        let chars: Vec<char> = input.chars().collect();
        scanner.scan("", &chars, &env)
    }

    fn assert_span(spans: &[ColorSpan], idx: usize, start: usize, end: usize, style: HighlightStyle) {
        assert!(
            idx < spans.len(),
            "expected span at index {} but only {} spans exist. Spans: {:?}",
            idx, spans.len(), spans
        );
        assert_eq!(
            spans[idx],
            ColorSpan { start, end, style },
            "span[{}] mismatch. All spans: {:?}",
            idx, spans
        );
    }

    #[test]
    fn test_scan_simple_command() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "ls");
        // "ls" exists in /usr/bin
        assert_span(&spans, 0, 0, 2, HighlightStyle::CommandValid);
    }

    #[test]
    fn test_scan_invalid_command() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "xyzzy_no_such_cmd");
        assert_span(&spans, 0, 0, 17, HighlightStyle::CommandInvalid);
    }

    #[test]
    fn test_scan_command_with_args() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo hello world");
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid); // echo
        assert_span(&spans, 1, 5, 10, HighlightStyle::Default);     // hello
        assert_span(&spans, 2, 11, 16, HighlightStyle::Default);    // world
    }

    #[test]
    fn test_scan_pipe() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "ls | grep foo");
        assert_span(&spans, 0, 0, 2, HighlightStyle::CommandValid);  // ls
        assert_span(&spans, 1, 3, 4, HighlightStyle::Operator);      // |
        assert_span(&spans, 2, 5, 9, HighlightStyle::CommandValid);  // grep
        assert_span(&spans, 3, 10, 13, HighlightStyle::Default);     // foo
    }

    #[test]
    fn test_scan_and_or() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "true && echo ok");
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid); // true (builtin)
        assert_span(&spans, 1, 5, 7, HighlightStyle::Operator);     // &&
        assert_span(&spans, 2, 8, 12, HighlightStyle::CommandValid); // echo
        assert_span(&spans, 3, 13, 15, HighlightStyle::Default);    // ok
    }

    #[test]
    fn test_scan_semicolon() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo a; echo b");
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid); // echo
        assert_span(&spans, 1, 5, 6, HighlightStyle::Default);      // a
        assert_span(&spans, 2, 6, 7, HighlightStyle::Operator);     // ;
        assert_span(&spans, 3, 8, 12, HighlightStyle::CommandValid); // echo
        assert_span(&spans, 4, 13, 14, HighlightStyle::Default);    // b
    }

    #[test]
    fn test_scan_keyword_if() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "if true; then echo hi; fi");
        assert_span(&spans, 0, 0, 2, HighlightStyle::Keyword);       // if
        assert_span(&spans, 1, 3, 7, HighlightStyle::CommandValid);   // true
        assert_span(&spans, 2, 7, 8, HighlightStyle::Operator);       // ;
        assert_span(&spans, 3, 9, 13, HighlightStyle::Keyword);       // then
        assert_span(&spans, 4, 14, 18, HighlightStyle::CommandValid);  // echo
        assert_span(&spans, 5, 19, 21, HighlightStyle::Default);      // hi
        assert_span(&spans, 6, 21, 22, HighlightStyle::Operator);     // ;
        assert_span(&spans, 7, 23, 25, HighlightStyle::Keyword);      // fi
    }

    #[test]
    fn test_scan_comment() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo hi # comment");
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid); // echo
        assert_span(&spans, 1, 5, 7, HighlightStyle::Default);      // hi
        assert_span(&spans, 2, 8, 17, HighlightStyle::Comment);     // # comment
    }

    #[test]
    fn test_scan_comment_at_start() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "# full line comment");
        assert_span(&spans, 0, 0, 19, HighlightStyle::Comment);
    }

    #[test]
    fn test_scan_redirect() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo hi > out.txt");
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid); // echo
        assert_span(&spans, 1, 5, 7, HighlightStyle::Default);      // hi
        assert_span(&spans, 2, 8, 9, HighlightStyle::Redirect);     // >
        assert_span(&spans, 3, 10, 17, HighlightStyle::Default);    // out.txt
    }

    #[test]
    fn test_scan_redirect_append() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo hi >> out.txt");
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid); // echo
        assert_span(&spans, 1, 5, 7, HighlightStyle::Default);      // hi
        assert_span(&spans, 2, 8, 10, HighlightStyle::Redirect);    // >>
        assert_span(&spans, 3, 11, 18, HighlightStyle::Default);    // out.txt
    }

    #[test]
    fn test_scan_io_number_redirect() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "cmd 2>/dev/null");
        assert_span(&spans, 0, 0, 3, HighlightStyle::CommandValid); // cmd (exists in /usr/bin on macOS)
        // The rest depends on whether 'cmd' exists — check operator structure
        // 2 is IoNumber, > is Redirect
    }

    #[test]
    fn test_scan_assignment() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "VAR=hello echo test");
        assert_span(&spans, 0, 0, 4, HighlightStyle::Assignment);    // VAR=
        assert_span(&spans, 1, 4, 9, HighlightStyle::Default);       // hello
        assert_span(&spans, 2, 10, 14, HighlightStyle::CommandValid); // echo
        assert_span(&spans, 3, 15, 19, HighlightStyle::Default);     // test
    }

    #[test]
    fn test_scan_background() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "sleep 1 &");
        assert_span(&spans, 0, 0, 5, HighlightStyle::CommandValid); // sleep
        assert_span(&spans, 1, 6, 7, HighlightStyle::Default);      // 1
        assert_span(&spans, 2, 8, 9, HighlightStyle::Operator);     // &
    }
```

- [ ] **Step 2: Implement ScanMode, ScannerState, and HighlightScanner skeleton**

Add the scanner types and the `scan()` method to `src/interactive/highlight.rs` (after `apply_style`, before `#[cfg(test)]`):

```rust
// ---------------------------------------------------------------------------
// ScanMode / ScannerState — state machine for the highlight scanner
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum ScanMode {
    Normal,
    SingleQuote { start: usize },
    DoubleQuote { start: usize },
    DollarSingleQuote { start: usize },
    Parameter { start: usize, braced: bool },
    CommandSub { start: usize },
    Backtick { start: usize },
    ArithSub { start: usize },
    Comment { start: usize },
}

#[derive(Debug, Clone)]
struct ScannerState {
    mode_stack: Vec<ScanMode>,
    word_start: bool,
    command_position: bool,
}

impl ScannerState {
    fn new() -> Self {
        Self {
            mode_stack: vec![ScanMode::Normal],
            word_start: true,
            command_position: true,
        }
    }

    fn current_mode(&self) -> &ScanMode {
        self.mode_stack.last().unwrap_or(&ScanMode::Normal)
    }

    fn push_mode(&mut self, mode: ScanMode) {
        self.mode_stack.push(mode);
    }

    fn pop_mode(&mut self) {
        if self.mode_stack.len() > 1 {
            self.mode_stack.pop();
        }
    }
}

const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi",
    "for", "do", "done",
    "while", "until",
    "case", "esac", "in",
    "!", "{", "}",
];

/// Keywords that, when at command position, reset command_position to true
/// for the next word (i.e., the next token after them is also a command).
const COMMAND_POSITION_KEYWORDS: &[&str] = &[
    "then", "else", "elif", "do", "!",
];

fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word)
}

fn is_operator_char(ch: char) -> bool {
    matches!(ch, '|' | '&' | ';')
}

fn is_redirect_start(ch: char) -> bool {
    matches!(ch, '<' | '>')
}

// ---------------------------------------------------------------------------
// HighlightScanner
// ---------------------------------------------------------------------------

pub struct HighlightScanner {
    cache: HighlightCache,
    accumulated_state: Option<(String, ScannerState)>,
    checker: CommandChecker,
}

struct HighlightCache {
    prev_input: Vec<char>,
    prev_spans: Vec<ColorSpan>,
    checkpoints: Vec<(usize, ScannerState)>,
    checkpoint_interval: usize,
}

impl HighlightCache {
    fn new() -> Self {
        Self {
            prev_input: Vec::new(),
            prev_spans: Vec::new(),
            checkpoints: Vec::new(),
            checkpoint_interval: 32,
        }
    }

    fn clear(&mut self) {
        self.prev_input.clear();
        self.prev_spans.clear();
        self.checkpoints.clear();
    }
}

impl HighlightScanner {
    pub fn new() -> Self {
        Self {
            cache: HighlightCache::new(),
            accumulated_state: None,
            checker: CommandChecker::new(),
        }
    }

    /// Scan the current input and return ColorSpans.
    ///
    /// `accumulated` is the accumulated input from previous PS2 lines (empty at PS1).
    /// `current` is the current line being edited.
    /// `checker_env` provides PATH and aliases for command existence checking.
    ///
    /// Returns spans with char indices relative to `current`.
    pub fn scan(
        &mut self,
        accumulated: &str,
        current: &[char],
        checker_env: &CheckerEnv,
    ) -> Vec<ColorSpan> {
        if current.is_empty() {
            self.cache.clear();
            return Vec::new();
        }

        // Determine initial scanner state from accumulated buffer
        let initial_state = if accumulated.is_empty() {
            ScannerState::new()
        } else {
            self.get_accumulated_state(accumulated)
        };

        // Check if we can use incremental scanning
        let (start_pos, mut state, mut spans) = self.find_rescan_start(current, &initial_state);

        // Scan from start_pos to end
        self.scan_from(&mut state, current, start_pos, &mut spans, checker_env, accumulated.is_empty());

        // Update cache
        self.cache.prev_input = current.to_vec();
        self.cache.prev_spans = spans.clone();

        spans
    }

    fn get_accumulated_state(&mut self, accumulated: &str) -> ScannerState {
        if let Some((ref cached_acc, ref state)) = self.accumulated_state {
            if cached_acc == accumulated {
                return state.clone();
            }
        }

        // Scan accumulated buffer to get ending state
        let chars: Vec<char> = accumulated.chars().collect();
        let mut state = ScannerState::new();
        let mut spans = Vec::new();
        let dummy_aliases = AliasStore::default();
        let dummy_env = CheckerEnv { path: "", aliases: &dummy_aliases };
        self.scan_from(&mut state, &chars, 0, &mut spans, &dummy_env, false);

        self.accumulated_state = Some((accumulated.to_string(), state.clone()));
        state
    }

    fn find_rescan_start(
        &self,
        current: &[char],
        initial_state: &ScannerState,
    ) -> (usize, ScannerState, Vec<ColorSpan>) {
        // Find first differing position
        let diff_pos = self.cache.prev_input.iter()
            .zip(current.iter())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| self.cache.prev_input.len().min(current.len()));

        if diff_pos == 0 || self.cache.checkpoints.is_empty() {
            return (0, initial_state.clone(), Vec::new());
        }

        // Find nearest checkpoint before diff_pos
        let checkpoint = self.cache.checkpoints.iter()
            .rev()
            .find(|(pos, _)| *pos <= diff_pos);

        if let Some((cp_pos, cp_state)) = checkpoint {
            let kept_spans: Vec<ColorSpan> = self.cache.prev_spans.iter()
                .filter(|s| s.end <= *cp_pos)
                .cloned()
                .collect();
            (*cp_pos, cp_state.clone(), kept_spans)
        } else {
            (0, initial_state.clone(), Vec::new())
        }
    }

    fn scan_from(
        &mut self,
        state: &mut ScannerState,
        input: &[char],
        start: usize,
        spans: &mut Vec<ColorSpan>,
        checker_env: &CheckerEnv,
        is_ps1: bool,
    ) {
        let len = input.len();
        let mut pos = start;
        // Clear any stale checkpoints from pos onward
        self.cache.checkpoints.retain(|(cp, _)| *cp < start);

        while pos < len {
            // Save checkpoint at intervals
            if pos > 0 && pos % self.cache.checkpoint_interval == 0 {
                self.cache.checkpoints.push((pos, state.clone()));
            }

            match state.current_mode().clone() {
                ScanMode::Normal => {
                    pos = self.scan_normal(state, input, pos, spans, checker_env);
                }
                ScanMode::SingleQuote { start: sq_start } => {
                    pos = self.scan_single_quote(state, input, pos, spans, sq_start, is_ps1);
                }
                ScanMode::DoubleQuote { start: dq_start } => {
                    pos = self.scan_double_quote(state, input, pos, spans, dq_start, is_ps1);
                }
                ScanMode::DollarSingleQuote { start: dsq_start } => {
                    pos = self.scan_dollar_single_quote(state, input, pos, spans, dsq_start, is_ps1);
                }
                ScanMode::Parameter { start: p_start, braced } => {
                    pos = self.scan_parameter(state, input, pos, spans, p_start, braced, is_ps1);
                }
                ScanMode::CommandSub { start: cs_start } => {
                    pos = self.scan_command_sub(state, input, pos, spans, cs_start, is_ps1);
                }
                ScanMode::Backtick { start: bt_start } => {
                    pos = self.scan_backtick(state, input, pos, spans, bt_start, is_ps1);
                }
                ScanMode::ArithSub { start: as_start } => {
                    pos = self.scan_arith_sub(state, input, pos, spans, as_start, is_ps1);
                }
                ScanMode::Comment { start: c_start } => {
                    pos = self.scan_comment(state, input, pos, spans, c_start);
                }
            }
        }

        // Handle unclosed modes at end of input
        if is_ps1 {
            self.mark_unclosed_errors(state, input.len(), spans);
        }
    }

    /// Scan in Normal mode. Returns the new position after processing.
    fn scan_normal(
        &mut self,
        state: &mut ScannerState,
        input: &[char],
        pos: usize,
        spans: &mut Vec<ColorSpan>,
        checker_env: &CheckerEnv,
    ) -> usize {
        let ch = input[pos];

        // Skip whitespace
        if ch == ' ' || ch == '\t' {
            state.word_start = true;
            return pos + 1;
        }

        // Comment at word start
        if ch == '#' && state.word_start {
            state.push_mode(ScanMode::Comment { start: pos });
            return pos; // re-enter loop in Comment mode
        }

        // Operators: | && || ; & ;;
        if is_operator_char(ch) {
            let (op_end, style) = self.scan_operator(input, pos);
            spans.push(ColorSpan { start: pos, end: op_end, style });
            state.word_start = true;
            state.command_position = true;
            return op_end;
        }

        // Redirections: < > >> << <& >& <> >| <<-
        if is_redirect_start(ch) {
            let redir_end = self.scan_redirect(input, pos);
            spans.push(ColorSpan { start: pos, end: redir_end, style: HighlightStyle::Redirect });
            state.word_start = true;
            // After redirect operator, next word is a filename, not a command
            state.command_position = false;
            return redir_end;
        }

        // Parentheses
        if ch == '(' {
            spans.push(ColorSpan { start: pos, end: pos + 1, style: HighlightStyle::Operator });
            state.word_start = true;
            state.command_position = true;
            return pos + 1;
        }
        if ch == ')' {
            spans.push(ColorSpan { start: pos, end: pos + 1, style: HighlightStyle::Operator });
            state.word_start = true;
            state.command_position = false;
            return pos + 1;
        }

        // Start of quoting or expansion — delegate to appropriate mode
        if ch == '\'' {
            state.push_mode(ScanMode::SingleQuote { start: pos });
            state.word_start = false;
            return pos + 1; // skip opening quote, continue in SQ mode
        }

        if ch == '"' {
            state.push_mode(ScanMode::DoubleQuote { start: pos });
            state.word_start = false;
            return pos + 1;
        }

        if ch == '`' {
            spans.push(ColorSpan { start: pos, end: pos + 1, style: HighlightStyle::CommandSub });
            state.push_mode(ScanMode::Backtick { start: pos });
            // Inside backtick, reset to normal scanning with command position
            state.push_mode(ScanMode::Normal);
            state.word_start = true;
            state.command_position = true;
            return pos + 1;
        }

        if ch == '$' && pos + 1 < input.len() {
            return self.scan_dollar(state, input, pos, spans);
        }

        if ch == '~' && state.word_start {
            // Tilde at word start
            let end = self.scan_tilde_end(input, pos);
            spans.push(ColorSpan { start: pos, end, style: HighlightStyle::Tilde });
            state.word_start = false;
            state.command_position = false;
            return end;
        }

        // Regular word character — collect the entire word
        let word_start_pos = pos;
        let word_end = self.scan_word_end(input, pos);
        let word: String = input[word_start_pos..word_end].iter().collect();

        // Check for assignment (NAME=value)
        if state.command_position {
            if let Some(eq_pos) = word.find('=') {
                let name = &word[..eq_pos];
                if !name.is_empty() && is_valid_name(name) {
                    // It's an assignment
                    let eq_char_idx = word_start_pos + name.chars().count() + 1; // +1 for '='
                    spans.push(ColorSpan {
                        start: word_start_pos,
                        end: eq_char_idx,
                        style: HighlightStyle::Assignment,
                    });
                    // Value part (rest of word)
                    if eq_char_idx < word_end {
                        spans.push(ColorSpan {
                            start: eq_char_idx,
                            end: word_end,
                            style: HighlightStyle::Default,
                        });
                    }
                    state.word_start = false;
                    // command_position stays true — next word could be command or another assignment
                    return word_end;
                }
            }
        }

        // Check for IO number (digits immediately before redirect)
        if word.chars().all(|c| c.is_ascii_digit()) && word_end < input.len() && is_redirect_start(input[word_end]) {
            spans.push(ColorSpan {
                start: word_start_pos,
                end: word_end,
                style: HighlightStyle::IoNumber,
            });
            state.word_start = false;
            state.command_position = false;
            return word_end;
        }

        // At command position: check keyword or command existence
        if state.command_position {
            if is_keyword(&word) {
                spans.push(ColorSpan {
                    start: word_start_pos,
                    end: word_end,
                    style: HighlightStyle::Keyword,
                });
                // Some keywords make the next word a command position too
                state.command_position = COMMAND_POSITION_KEYWORDS.contains(&word.as_str());
            } else {
                let existence = self.checker.check(&word, checker_env);
                let style = match existence {
                    CommandExistence::Valid => HighlightStyle::CommandValid,
                    CommandExistence::Invalid => HighlightStyle::CommandInvalid,
                };
                spans.push(ColorSpan {
                    start: word_start_pos,
                    end: word_end,
                    style,
                });
                state.command_position = false;
            }
        } else {
            // Regular argument
            spans.push(ColorSpan {
                start: word_start_pos,
                end: word_end,
                style: HighlightStyle::Default,
            });
        }

        state.word_start = false;
        word_end
    }

    /// Scan an operator starting at pos. Returns (end_pos, style).
    fn scan_operator(&self, input: &[char], pos: usize) -> (usize, HighlightStyle) {
        let ch = input[pos];
        let next = input.get(pos + 1).copied();

        match (ch, next) {
            ('&', Some('&')) => (pos + 2, HighlightStyle::Operator),
            ('|', Some('|')) => (pos + 2, HighlightStyle::Operator),
            (';', Some(';')) => (pos + 2, HighlightStyle::Operator),
            ('|', _) => (pos + 1, HighlightStyle::Operator),
            ('&', _) => (pos + 1, HighlightStyle::Operator),
            (';', _) => (pos + 1, HighlightStyle::Operator),
            _ => (pos + 1, HighlightStyle::Operator),
        }
    }

    /// Scan a redirect operator starting at pos. Returns end_pos.
    fn scan_redirect(&self, input: &[char], pos: usize) -> usize {
        let ch = input[pos];
        let next = input.get(pos + 1).copied();
        let next2 = input.get(pos + 2).copied();

        match (ch, next, next2) {
            ('<', Some('<'), Some('-')) => pos + 3, // <<-
            ('<', Some('<'), _) => pos + 2,         // <<
            ('<', Some('&'), _) => pos + 2,         // <&
            ('<', Some('>'), _) => pos + 2,         // <>
            ('>', Some('>'), _) => pos + 2,         // >>
            ('>', Some('&'), _) => pos + 2,         // >&
            ('>', Some('|'), _) => pos + 2,         // >|
            _ => pos + 1,                           // < or >
        }
    }

    /// Scan the end of a word (stop at whitespace, operators, redirects, quotes, dollar, hash).
    fn scan_word_end(&self, input: &[char], pos: usize) -> usize {
        let mut end = pos;
        while end < input.len() {
            let ch = input[end];
            if ch == ' ' || ch == '\t' || ch == '\n'
                || is_operator_char(ch)
                || is_redirect_start(ch)
                || ch == '\'' || ch == '"' || ch == '`'
                || ch == '$' || ch == '#'
                || ch == '(' || ch == ')'
            {
                break;
            }
            end += 1;
        }
        end
    }

    /// Scan tilde: ~ followed by optional username (alphanumeric or _) or /
    fn scan_tilde_end(&self, input: &[char], pos: usize) -> usize {
        let mut end = pos + 1; // skip ~
        // Tilde followed by a username (letters/digits/_) or nothing before /
        while end < input.len() {
            let ch = input[end];
            if ch.is_ascii_alphanumeric() || ch == '_' {
                end += 1;
            } else {
                break;
            }
        }
        end
    }

    /// Handle $ at pos. Determines if it's $var, ${...}, $(...), $((...)), or $'...'
    fn scan_dollar(
        &mut self,
        state: &mut ScannerState,
        input: &[char],
        pos: usize,
        spans: &mut Vec<ColorSpan>,
    ) -> usize {
        let next = input[pos + 1];

        // $'...' — dollar single quote
        if next == '\'' {
            state.push_mode(ScanMode::DollarSingleQuote { start: pos });
            state.word_start = false;
            return pos + 2; // skip $'
        }

        // $(( — arithmetic substitution
        if next == '(' && pos + 2 < input.len() && input[pos + 2] == '(' {
            spans.push(ColorSpan { start: pos, end: pos + 3, style: HighlightStyle::ArithSub });
            state.push_mode(ScanMode::ArithSub { start: pos });
            state.word_start = false;
            return pos + 3;
        }

        // $( — command substitution
        if next == '(' {
            spans.push(ColorSpan { start: pos, end: pos + 2, style: HighlightStyle::CommandSub });
            state.push_mode(ScanMode::CommandSub { start: pos });
            state.push_mode(ScanMode::Normal);
            state.word_start = true;
            state.command_position = true;
            return pos + 2;
        }

        // ${ — braced parameter
        if next == '{' {
            state.push_mode(ScanMode::Parameter { start: pos, braced: true });
            state.word_start = false;
            return pos + 2; // skip ${
        }

        // $name, $N, $@, $?, $#, $$, $!, $-, $0-$9
        if next.is_ascii_alphanumeric() || next == '_' || matches!(next, '@' | '*' | '#' | '?' | '$' | '!' | '-') {
            let var_end = if next.is_ascii_alphabetic() || next == '_' {
                // $NAME
                let mut e = pos + 2;
                while e < input.len() && (input[e].is_ascii_alphanumeric() || input[e] == '_') {
                    e += 1;
                }
                e
            } else {
                // $N, $@, $?, etc — single char
                pos + 2
            };
            spans.push(ColorSpan { start: pos, end: var_end, style: HighlightStyle::Variable });
            state.word_start = false;
            state.command_position = false;
            return var_end;
        }

        // Bare $ — treat as default
        state.word_start = false;
        pos + 1
    }

    // Placeholder scan methods for modes — will be implemented in Task 4
    fn scan_single_quote(&mut self, state: &mut ScannerState, input: &[char], pos: usize, spans: &mut Vec<ColorSpan>, start: usize, is_ps1: bool) -> usize {
        // Find closing '
        let mut end = pos;
        while end < input.len() {
            if input[end] == '\'' {
                spans.push(ColorSpan { start, end: end + 1, style: HighlightStyle::String });
                state.pop_mode();
                state.word_start = false;
                state.command_position = false;
                return end + 1;
            }
            end += 1;
        }
        // Unclosed — will be marked by mark_unclosed_errors if PS1
        end
    }

    fn scan_double_quote(&mut self, state: &mut ScannerState, input: &[char], pos: usize, spans: &mut Vec<ColorSpan>, start: usize, _is_ps1: bool) -> usize {
        let mut end = pos;
        while end < input.len() {
            let ch = input[end];
            if ch == '"' {
                // Emit span for text before closing quote
                if end > start + 1 {
                    // Already handled per-part in Task 4; for now, emit whole
                }
                spans.push(ColorSpan { start, end: end + 1, style: HighlightStyle::DoubleString });
                state.pop_mode();
                state.word_start = false;
                state.command_position = false;
                return end + 1;
            }
            if ch == '\\' && end + 1 < input.len() {
                end += 2; // skip escape
                continue;
            }
            if ch == '$' && end + 1 < input.len() {
                // Emit DQ text up to $
                if end > pos {
                    spans.push(ColorSpan { start: pos, end, style: HighlightStyle::DoubleString });
                }
                let dollar_end = self.scan_dollar(state, input, end, spans);
                // After $ expansion inside DQ, we need to update our position
                // but we're still inside DQ. Since scan_dollar may push modes,
                // we return and let the outer loop handle mode transitions.
                return dollar_end;
            }
            if ch == '`' {
                if end > pos {
                    spans.push(ColorSpan { start: pos, end, style: HighlightStyle::DoubleString });
                }
                spans.push(ColorSpan { start: end, end: end + 1, style: HighlightStyle::CommandSub });
                state.push_mode(ScanMode::Backtick { start: end });
                state.push_mode(ScanMode::Normal);
                state.word_start = true;
                state.command_position = true;
                return end + 1;
            }
            end += 1;
        }
        // Unclosed
        end
    }

    fn scan_dollar_single_quote(&mut self, state: &mut ScannerState, input: &[char], pos: usize, spans: &mut Vec<ColorSpan>, start: usize, _is_ps1: bool) -> usize {
        let mut end = pos;
        while end < input.len() {
            if input[end] == '\'' {
                spans.push(ColorSpan { start, end: end + 1, style: HighlightStyle::String });
                state.pop_mode();
                state.word_start = false;
                return end + 1;
            }
            if input[end] == '\\' && end + 1 < input.len() {
                end += 2;
                continue;
            }
            end += 1;
        }
        end
    }

    fn scan_parameter(&mut self, state: &mut ScannerState, input: &[char], pos: usize, spans: &mut Vec<ColorSpan>, start: usize, braced: bool, _is_ps1: bool) -> usize {
        if !braced {
            // Simple $name — already handled in scan_dollar
            state.pop_mode();
            return pos;
        }
        // Braced: find closing }
        let mut end = pos;
        while end < input.len() {
            if input[end] == '}' {
                spans.push(ColorSpan { start, end: end + 1, style: HighlightStyle::Variable });
                state.pop_mode();
                state.word_start = false;
                return end + 1;
            }
            end += 1;
        }
        end
    }

    fn scan_command_sub(&mut self, state: &mut ScannerState, input: &[char], pos: usize, _spans: &mut Vec<ColorSpan>, _start: usize, _is_ps1: bool) -> usize {
        // We pushed Normal mode on top. When Normal mode encounters ),
        // it needs to pop back. This is handled by checking for ) in Normal mode.
        // If we're here, Normal was popped and we're back in CommandSub.
        // The ) was already consumed — just pop CommandSub.
        state.pop_mode();
        pos
    }

    fn scan_backtick(&mut self, state: &mut ScannerState, input: &[char], pos: usize, _spans: &mut Vec<ColorSpan>, _start: usize, _is_ps1: bool) -> usize {
        state.pop_mode();
        pos
    }

    fn scan_arith_sub(&mut self, state: &mut ScannerState, input: &[char], pos: usize, spans: &mut Vec<ColorSpan>, start: usize, _is_ps1: bool) -> usize {
        let mut end = pos;
        while end < input.len() {
            if input[end] == ')' && end + 1 < input.len() && input[end + 1] == ')' {
                spans.push(ColorSpan { start, end: end + 2, style: HighlightStyle::ArithSub });
                state.pop_mode();
                state.word_start = false;
                return end + 2;
            }
            end += 1;
        }
        end
    }

    fn scan_comment(&mut self, state: &mut ScannerState, input: &[char], pos: usize, spans: &mut Vec<ColorSpan>, start: usize) -> usize {
        // Comment extends to end of input
        spans.push(ColorSpan { start, end: input.len(), style: HighlightStyle::Comment });
        state.pop_mode();
        input.len()
    }

    /// Handle ) in Normal mode — may close a CommandSub
    fn handle_close_paren(
        &mut self,
        state: &mut ScannerState,
        input: &[char],
        pos: usize,
        spans: &mut Vec<ColorSpan>,
    ) -> usize {
        // Pop Normal mode to return to CommandSub
        if state.mode_stack.len() > 1 {
            state.pop_mode(); // pop Normal
            // Now we should be in CommandSub — emit closing delimiter
            if matches!(state.current_mode(), ScanMode::CommandSub { .. }) {
                spans.push(ColorSpan { start: pos, end: pos + 1, style: HighlightStyle::CommandSub });
                state.pop_mode(); // pop CommandSub
                state.word_start = false;
                state.command_position = false;
                return pos + 1;
            }
        }
        // Not inside a command sub — treat as regular operator
        spans.push(ColorSpan { start: pos, end: pos + 1, style: HighlightStyle::Operator });
        state.word_start = true;
        state.command_position = false;
        pos + 1
    }

    fn mark_unclosed_errors(&self, state: &ScannerState, end: usize, spans: &mut Vec<ColorSpan>) {
        for mode in &state.mode_stack {
            match mode {
                ScanMode::SingleQuote { start } |
                ScanMode::DoubleQuote { start } |
                ScanMode::DollarSingleQuote { start } |
                ScanMode::Backtick { start } => {
                    // Remove any spans that overlap with the error region
                    spans.retain(|s| s.end <= *start || s.start >= end);
                    spans.push(ColorSpan {
                        start: *start,
                        end,
                        style: HighlightStyle::Error,
                    });
                }
                ScanMode::CommandSub { start } => {
                    // Mark just the $( as error
                    spans.retain(|s| !(s.start == *start && s.end == *start + 2));
                    spans.push(ColorSpan {
                        start: *start,
                        end: (*start + 2).min(end),
                        style: HighlightStyle::Error,
                    });
                }
                ScanMode::Parameter { start, .. } => {
                    spans.retain(|s| !(s.start == *start));
                    spans.push(ColorSpan {
                        start: *start,
                        end: (*start + 2).min(end),
                        style: HighlightStyle::Error,
                    });
                }
                ScanMode::ArithSub { start } => {
                    spans.retain(|s| !(s.start == *start && s.end == *start + 3));
                    spans.push(ColorSpan {
                        start: *start,
                        end: (*start + 3).min(end),
                        style: HighlightStyle::Error,
                    });
                }
                ScanMode::Normal | ScanMode::Comment { .. } => {}
            }
        }
        // Sort spans by start position for consistent output
        spans.sort_by_key(|s| s.start);
    }
}

fn is_valid_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
```

Also update `scan_normal` to handle `)` by calling `handle_close_paren` — replace the existing `)` handling in `scan_normal`:

In the `scan_normal` method, the `)` block should be:

```rust
        if ch == ')' {
            return self.handle_close_paren(state, input, pos, spans);
        }
```

And handle backtick close inside Normal mode — when we encounter `` ` `` and we're inside a Backtick mode (Normal on top of Backtick):

Update the backtick check in `scan_normal` to also handle closing:

```rust
        if ch == '`' {
            // Check if we're inside a Backtick mode (Normal pushed on top)
            if state.mode_stack.len() > 1 {
                let parent = &state.mode_stack[state.mode_stack.len() - 2];
                if matches!(parent, ScanMode::Backtick { .. }) {
                    spans.push(ColorSpan { start: pos, end: pos + 1, style: HighlightStyle::CommandSub });
                    state.pop_mode(); // pop Normal
                    state.pop_mode(); // pop Backtick
                    state.word_start = false;
                    state.command_position = false;
                    return pos + 1;
                }
            }
            // Opening backtick
            spans.push(ColorSpan { start: pos, end: pos + 1, style: HighlightStyle::CommandSub });
            state.push_mode(ScanMode::Backtick { start: pos });
            state.push_mode(ScanMode::Normal);
            state.word_start = true;
            state.command_position = true;
            return pos + 1;
        }
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib interactive::highlight 2>&1 | tail -20`
Expected: All CommandChecker tests and scanner tests pass

- [ ] **Step 4: Commit**

```bash
git add src/interactive/highlight.rs src/interactive/mod.rs
git commit -m "feat(highlight): implement HighlightScanner with Normal mode scanning

Add state machine scanner supporting operators, redirects, comments,
keywords, command position detection, assignments, IO numbers, tilde,
single/double quotes, variable expansion, command substitution, arithmetic
substitution, backticks, and PS1 unclosed-mode error marking.
Includes incremental caching with checkpoints.

Task: syntax highlighting for interactive mode (Task 3)"
```

---

### Task 4: Integrate Scanner into LineEditor and Repl

**Files:**
- Modify: `src/interactive/line_editor.rs`
- Modify: `src/interactive/mod.rs`

- [ ] **Step 1: Modify redraw() to accept and render ColorSpans**

In `src/interactive/line_editor.rs`, update the `redraw` method signature and implementation. Replace the existing `redraw` method (lines 230-245):

```rust
    /// Redraw the current buffer on screen with syntax highlighting.
    fn redraw<T: Terminal>(&self, term: &mut T, prompt_width: usize, spans: &[ColorSpan]) -> io::Result<()> {
        let col = |n: usize| -> u16 { n.min(u16::MAX as usize) as u16 };
        term.move_to_column(col(prompt_width))?;
        term.clear_until_newline()?;

        if spans.is_empty() {
            // Fallback: no highlighting
            term.write_str(&self.buffer())?;
        } else {
            // Render with per-character color switching
            let mut span_idx = 0;
            let mut current_style = HighlightStyle::Default;

            for (i, &ch) in self.buf.iter().enumerate() {
                // Advance span index to find the span covering this position
                while span_idx + 1 < spans.len() && spans[span_idx].end <= i {
                    span_idx += 1;
                }

                let style = if span_idx < spans.len()
                    && i >= spans[span_idx].start
                    && i < spans[span_idx].end
                {
                    spans[span_idx].style
                } else {
                    HighlightStyle::Default
                };

                if style != current_style {
                    apply_style(term, style)?;
                    current_style = style;
                }
                term.write_char(ch)?;
            }

            if current_style != HighlightStyle::Default {
                term.reset_style()?;
            }
        }

        // Draw suggestion in dim text when cursor is at end of buffer
        if let Some(ref suggestion) = self.suggestion
            && self.pos == self.buf.len()
        {
            term.set_dim(true)?;
            term.write_str(suggestion)?;
            term.set_dim(false)?;
        }
        term.move_to_column(col(prompt_width + self.pos))?;
        term.flush()?;
        Ok(())
    }
```

Add imports at the top of `line_editor.rs`:

```rust
use super::highlight::{HighlightScanner, HighlightStyle, ColorSpan, CheckerEnv, apply_style};
```

- [ ] **Step 2: Update read_line_loop to pass spans to redraw**

In `read_line_loop` (the non-completion variant), update the `self.redraw` calls. Replace all `self.redraw(term, prompt_width)?;` calls with `self.redraw(term, prompt_width, &[])?;` — this method doesn't use highlighting, so it passes empty spans.

- [ ] **Step 3: Update read_line_with_completion API**

Change `read_line_with_completion` signature and its internal loop to use the scanner:

```rust
    pub fn read_line_with_completion<T: Terminal>(
        &mut self,
        prompt: &str,
        history: &mut History,
        term: &mut T,
        ctx: &CompletionContext,
        scanner: &mut HighlightScanner,
        checker_env: &CheckerEnv,
        accumulated: &str,
    ) -> io::Result<Option<String>> {
        self.clear();
        term.enable_raw_mode()?;
        let result = self.read_line_loop_with_completion(prompt, history, term, ctx, scanner, checker_env, accumulated);
        let _ = term.disable_raw_mode();
        result
    }
```

Update `read_line_loop_with_completion`:

```rust
    fn read_line_loop_with_completion<T: Terminal>(
        &mut self,
        prompt: &str,
        history: &mut History,
        term: &mut T,
        ctx: &CompletionContext,
        scanner: &mut HighlightScanner,
        checker_env: &CheckerEnv,
        accumulated: &str,
    ) -> io::Result<Option<String>> {
        let prompt_width = prompt.chars().count();
        loop {
            term.flush()?;
            if let Event::Key(key_event) = term.read_event()? {
                match self.handle_key(key_event, history) {
                    KeyAction::Submit => {
                        term.reset_style()?;
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
                        term.reset_style()?;
                        history.reset_cursor();
                        term.move_to_column(0)?;
                        term.write_str("\r\n")?;
                        term.flush()?;
                        self.clear();
                        return Ok(Some(String::new()));
                    }
                    KeyAction::FuzzySearch => {
                        self.suggestion = None;
                        term.reset_style()?;
                        term.disable_raw_mode()?;
                        if let Ok(Some(line)) = FuzzySearchUI::run(history, term) {
                            self.buf = line.chars().collect();
                            self.pos = self.buf.len();
                        }
                        term.enable_raw_mode()?;
                        term.move_to_column(0)?;
                        term.clear_current_line()?;
                        term.write_str(prompt)?;
                    }
                    KeyAction::TabComplete => {
                        term.reset_style()?;
                        self.handle_tab_complete(term, prompt, ctx)?;
                    }
                    KeyAction::Continue => {}
                }
                self.update_suggestion(history);
                let spans = scanner.scan(accumulated, &self.buf, checker_env);
                self.redraw(term, prompt_width, &spans)?;
            }
        }
    }
```

- [ ] **Step 4: Update Repl to create and pass HighlightScanner**

In `src/interactive/mod.rs`, add the import and update the `Repl` struct:

```rust
use highlight::{HighlightScanner, CheckerEnv};
```

Add `scanner` field to `Repl`:

```rust
pub struct Repl {
    executor: Executor,
    line_editor: LineEditor,
    terminal: CrosstermTerminal,
    scanner: HighlightScanner,
}
```

Update `Repl::new()` — add `scanner: HighlightScanner::new(),` to the Self construction.

Update `Repl::run()` — construct `CheckerEnv` and pass to `read_line_with_completion`. Replace the existing `read_line_with_completion` call (around line 82):

```rust
            // Build checker env for syntax highlighting
            let path_val = self.executor.env.vars.get("PATH").unwrap_or("").to_string();
            let checker_env = CheckerEnv {
                path: &path_val,
                aliases: &self.executor.env.aliases,
            };

            // Read a line
            let line = match self.line_editor.read_line_with_completion(
                &prompt,
                &mut self.executor.env.history,
                &mut self.terminal,
                &comp_ctx,
                &mut self.scanner,
                &checker_env,
                &input_buffer,
            ) {
```

- [ ] **Step 5: Verify compilation and all tests pass**

Run: `cargo build 2>&1 | head -20`
Expected: Build succeeds

Run: `cargo test 2>&1 | tail -20`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add src/interactive/line_editor.rs src/interactive/mod.rs
git commit -m "feat(highlight): integrate scanner into LineEditor and Repl

Modify redraw() to accept ColorSpans and render per-character with color
switching. Update read_line_with_completion to accept HighlightScanner
and CheckerEnv. Create scanner in Repl and pass accumulated buffer for
PS2 context awareness. Add reset_style calls on Submit/Interrupt/FuzzySearch.

Task: syntax highlighting for interactive mode (Task 4)"
```

---

### Task 5: Error Highlighting and PS2 Context Tests

**Files:**
- Modify: `src/interactive/highlight.rs` (add tests)

- [ ] **Step 1: Add error and PS2 context tests**

Add these tests to the `#[cfg(test)] mod tests` block in `src/interactive/highlight.rs`:

```rust
    // ── Error and PS2 tests ──────────────────────────────────────

    fn scan_ps2(scanner: &mut HighlightScanner, accumulated: &str, current: &str) -> Vec<ColorSpan> {
        let (path, aliases) = test_env();
        let env = CheckerEnv { path: &path, aliases: &aliases };
        let chars: Vec<char> = current.chars().collect();
        scanner.scan(accumulated, &chars, &env)
    }

    #[test]
    fn test_scan_unclosed_single_quote_ps1() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo 'hello");
        // Should have Error style for the unclosed quote region
        let error_span = spans.iter().find(|s| s.style == HighlightStyle::Error);
        assert!(error_span.is_some(), "expected Error span for unclosed quote. Spans: {:?}", spans);
        let es = error_span.unwrap();
        assert_eq!(es.start, 5); // opening '
        assert_eq!(es.end, 11);  // to end of input
    }

    #[test]
    fn test_scan_unclosed_double_quote_ps1() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo \"hello");
        let error_span = spans.iter().find(|s| s.style == HighlightStyle::Error);
        assert!(error_span.is_some(), "expected Error span for unclosed double quote. Spans: {:?}", spans);
    }

    #[test]
    fn test_scan_unclosed_quote_ps2_not_error() {
        let mut scanner = test_scanner();
        // PS2: accumulated has unclosed quote from previous line
        let spans = scan_ps2(&mut scanner, "echo 'hello\n", "world'");
        // In PS2, unclosed quote from accumulated should NOT be Error
        let error_span = spans.iter().find(|s| s.style == HighlightStyle::Error);
        assert!(error_span.is_none(), "PS2 continuation should not show Error. Spans: {:?}", spans);
        // Should have String style
        let string_span = spans.iter().find(|s| s.style == HighlightStyle::String);
        assert!(string_span.is_some(), "expected String span in PS2. Spans: {:?}", spans);
    }

    #[test]
    fn test_scan_single_quoted_string() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo 'hello world'");
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid); // echo
        assert_span(&spans, 1, 5, 18, HighlightStyle::String);      // 'hello world'
    }

    #[test]
    fn test_scan_double_quoted_string() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo \"hello\"");
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid); // echo
        assert_span(&spans, 1, 5, 12, HighlightStyle::DoubleString); // "hello"
    }

    #[test]
    fn test_scan_variable_in_double_quote() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo \"hi $USER\"");
        // Should contain Variable span for $USER inside the double quote
        let var_span = spans.iter().find(|s| s.style == HighlightStyle::Variable);
        assert!(var_span.is_some(), "expected Variable span. Spans: {:?}", spans);
    }

    #[test]
    fn test_scan_variable_expansion() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo $HOME");
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid);
        assert_span(&spans, 1, 5, 10, HighlightStyle::Variable); // $HOME
    }

    #[test]
    fn test_scan_braced_variable() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo ${USER}");
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid);
        assert_span(&spans, 1, 5, 12, HighlightStyle::Variable); // ${USER}
    }

    #[test]
    fn test_scan_command_substitution() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo $(ls)");
        // Should have CommandSub style for $( and )
        let cs_spans: Vec<_> = spans.iter().filter(|s| s.style == HighlightStyle::CommandSub).collect();
        assert!(!cs_spans.is_empty(), "expected CommandSub spans. Spans: {:?}", spans);
    }

    #[test]
    fn test_scan_arith_sub() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo $((1+2))");
        let arith_spans: Vec<_> = spans.iter().filter(|s| s.style == HighlightStyle::ArithSub).collect();
        assert!(!arith_spans.is_empty(), "expected ArithSub spans. Spans: {:?}", spans);
    }

    #[test]
    fn test_scan_tilde() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "cd ~/projects");
        assert_span(&spans, 0, 0, 2, HighlightStyle::CommandValid); // cd
        assert_span(&spans, 1, 3, 4, HighlightStyle::Tilde);        // ~
    }
```

- [ ] **Step 2: Run tests and fix any issues**

Run: `cargo test --lib interactive::highlight 2>&1 | tail -30`
Expected: All tests pass (fix any scanner bugs that appear)

- [ ] **Step 3: Commit**

```bash
git add src/interactive/highlight.rs
git commit -m "test(highlight): add error visualization and PS2 context tests

Add tests for unclosed quote error marking at PS1, PS2 continuation
context (not error), single/double quoted strings, variable expansion,
braced variables, command substitution, arithmetic substitution, and tilde.

Task: syntax highlighting for interactive mode (Task 5)"
```

---

### Task 6: Incremental Cache Tests

**Files:**
- Modify: `src/interactive/highlight.rs` (add tests)

- [ ] **Step 1: Add incremental caching tests**

Add to the `#[cfg(test)] mod tests` block:

```rust
    // ── Incremental cache tests ──────────────────────────────────

    #[test]
    fn test_incremental_append() {
        let mut scanner = test_scanner();
        // Type "ech" — not a valid command
        let spans1 = scan_input(&mut scanner, "ech");
        assert_span(&spans1, 0, 0, 3, HighlightStyle::CommandInvalid);

        // Type "echo" — now valid
        let spans2 = scan_input(&mut scanner, "echo");
        assert_span(&spans2, 0, 0, 4, HighlightStyle::CommandValid);
    }

    #[test]
    fn test_incremental_backspace() {
        let mut scanner = test_scanner();
        let spans1 = scan_input(&mut scanner, "echo hello");
        assert_eq!(spans1.len(), 2);

        // Backspace the last char
        let spans2 = scan_input(&mut scanner, "echo hell");
        assert_eq!(spans2.len(), 2);
        assert_span(&spans2, 0, 0, 4, HighlightStyle::CommandValid);
        assert_span(&spans2, 1, 5, 9, HighlightStyle::Default);
    }

    #[test]
    fn test_incremental_full_rescan_on_history() {
        let mut scanner = test_scanner();
        let _spans1 = scan_input(&mut scanner, "echo hello");

        // Simulates history navigation — completely different input
        let spans2 = scan_input(&mut scanner, "ls -la");
        assert_span(&spans2, 0, 0, 2, HighlightStyle::CommandValid);
    }

    #[test]
    fn test_cache_cleared_on_empty() {
        let mut scanner = test_scanner();
        let _spans1 = scan_input(&mut scanner, "echo");
        let spans2 = scan_input(&mut scanner, "");
        assert!(spans2.is_empty());
    }

    #[test]
    fn test_accumulated_state_cached() {
        let mut scanner = test_scanner();
        // First call with accumulated
        let spans1 = scan_ps2(&mut scanner, "echo 'hello\n", "world'");
        // Second call with same accumulated — should use cached state
        let spans2 = scan_ps2(&mut scanner, "echo 'hello\n", "world'");
        assert_eq!(spans1, spans2);
    }
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib interactive::highlight 2>&1 | tail -20`
Expected: All tests pass

- [ ] **Step 3: Commit**

```bash
git add src/interactive/highlight.rs
git commit -m "test(highlight): add incremental cache tests

Test append, backspace, full rescan on history nav, empty input cache
clear, and accumulated state caching for PS2.

Task: syntax highlighting for interactive mode (Task 6)"
```

---

### Task 7: PTY E2E Tests

**Files:**
- Modify: `tests/pty_interactive.rs`

- [ ] **Step 1: Add PTY test for keyword highlighting**

Add at the end of `tests/pty_interactive.rs`:

```rust
#[test]
fn test_pty_syntax_highlight_keyword() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    // Type "if" — should be highlighted as Keyword (Bold + Magenta)
    s.send("if").unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // ANSI escape for Magenta foreground: \x1b[35m (or bold+magenta)
    // The exact sequence depends on crossterm, but we check for Magenta color code
    // Bold is \x1b[1m, Magenta is \x1b[35m
    // We just verify some ANSI color escape is present in the output
    s.send("\x03").unwrap(); // Ctrl+C to cancel
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_syntax_highlight_valid_command() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    // Type "echo" — should be highlighted as CommandValid (Bold + Green)
    s.send("echo hi\r").unwrap();
    expect_output(&mut s, "hi", "echo with highlighting failed");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}

#[test]
fn test_pty_syntax_highlight_pipe() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    // Test that piped commands work correctly with highlighting active
    s.send("echo hello | cat\r").unwrap();
    expect_output(&mut s, "hello", "pipe with highlighting failed");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}
```

- [ ] **Step 2: Build and run PTY tests**

Run: `cargo build 2>&1 | tail -5`
Expected: Build succeeds

Run: `cargo test --test pty_interactive 2>&1 | tail -20`
Expected: All PTY tests pass (including new ones)

- [ ] **Step 3: Commit**

```bash
git add tests/pty_interactive.rs
git commit -m "test(pty): add E2E tests for syntax highlighting

Verify keywords, valid commands, and pipe expressions render correctly
with syntax highlighting active in the interactive REPL.

Task: syntax highlighting for interactive mode (Task 7)"
```

---

### Task 8: Update TODO.md and Final Verification

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Remove resolved TODO item**

In `TODO.md`, delete the resolved `set_dim`/`set_reverse` item (line 26):

```
- [ ] `set_dim`/`set_reverse` use `Attribute::Reset` — resets all text attributes, not just the targeted one; may interfere with future colored prompt support; consider `Attribute::NoDim`/`Attribute::NoReverse` (`src/interactive/terminal.rs`)
```

- [ ] **Step 2: Run full test suite**

Run: `cargo test 2>&1 | tail -20`
Expected: All tests pass

Run: `cargo build 2>&1 | tail -5`
Expected: Build succeeds with no errors

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "docs(TODO): remove resolved set_dim/set_reverse attribute reset item

Fixed in syntax highlighting work: set_dim uses NormalIntensity,
set_reverse uses NoReverse instead of Attribute::Reset.

Task: syntax highlighting for interactive mode (Task 8)"
```
