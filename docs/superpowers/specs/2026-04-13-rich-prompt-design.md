# Rich Prompt Support via Plugins

**Date:** 2026-04-13
**Status:** Approved

## Overview

Enable starship-like rich prompts through kish's plugin system. Plugins generate complete ANSI-colored, multi-line prompt strings via the existing `pre_prompt` hook and `set_var("PS1", ...)` mechanism. kish provides the display foundation to correctly render these prompts.

## Approach

**Option B: Plugin-controlled rendering** — Plugins have full freedom to generate any prompt appearance. kish's responsibility is to correctly display the result, including ANSI escape handling, multi-line support, and proper cursor positioning.

### Why this approach

- Extends the existing `pre_prompt` + `set_var("PS1", ...)` flow naturally
- No new C ABI types needed (string-based FFI only)
- Plugin authors have complete control over appearance
- kish's implementation scope is focused on display correctness

### Trade-offs accepted

- Multiple plugins competing for PS1 is unresolved (last writer wins)
- Responsive terminal-width adaptation is the plugin's responsibility
- No built-in style consistency across plugins

## Requirements

| Requirement | Decision |
|---|---|
| ANSI escape handling | Strip escapes for width calculation, pass through for display |
| Unicode East Asian Width | Full support via `unicode-width` crate |
| Multi-line prompts | Split on `\n`, only last line participates in editing |
| Right-aligned prompt | Not supported (plugin can pad with spaces if needed) |
| Terminal resize | Full redraw on `Event::Resize` |
| Ctrl+L | Clear screen and redraw upper lines + input |
| Input line wrapping | 2D cursor positioning when input exceeds terminal width |
| SDK style helpers | Lightweight `Style` builder in `kish-plugin-sdk` |
| pre_prompt timing | Synchronous only (blocks until hook returns) |

## Design

### 1. Display Width Engine

New module: `src/interactive/display_width.rs`

Two core functions:

```rust
/// Strip ANSI escape sequences (CSI sequences: \x1b[...m, etc.)
pub fn strip_ansi(s: &str) -> String

/// Calculate display width: strip ANSI escapes, then sum unicode-width per character
pub fn display_width(s: &str) -> usize
```

**Logic:**
1. Detect and skip ANSI CSI sequences (`\x1b[` followed by parameter bytes and a final byte)
2. For remaining characters, use `UnicodeWidthChar::width()` from `unicode-width`
3. Full-width characters (CJK, etc.) return 2, half-width return 1, control characters return 0

**Dependency:** Add `unicode-width = "0.2"` to `Cargo.toml`

### 2. Multi-line Prompt Handling

Prompt decomposition structure:

```rust
pub struct PromptInfo {
    /// Lines before the final line (display-only, not involved in editing)
    pub upper_lines: Vec<String>,
    /// The final line (displayed left of the input buffer)
    pub last_line: String,
    /// Display width of last_line (used for cursor calculation)
    pub last_line_width: usize,
}
```

**Construction:** Split the expanded PS1 on `\n`. All lines except the last go into `upper_lines`. The last line becomes `last_line` with its `display_width` precomputed.

**Display flow:**
1. Print `upper_lines` to stderr (display only, not re-rendered on each keystroke)
2. Print `last_line` to stderr
3. Line editor uses `last_line_width` for cursor offset calculation
4. On redraw, only `last_line` + input buffer are rewritten

**Ctrl+L (clear screen):**
1. Clear entire screen
2. Re-output all `upper_lines`
3. Re-output `last_line` + input buffer via normal redraw

### 3. Terminal Resize and Input Line Wrapping

**SIGWINCH handling:**
- Add `Event::Resize(cols, rows)` to the line editor event loop (crossterm already emits this)
- On resize: store new terminal width, perform full redraw (upper_lines + last_line + input)

**Input line wrapping — 2D cursor calculation:**

```rust
let input_col = display_width(&buffer[..pos]);
let total_col = last_line_width + input_col;
let cursor_row = total_col / terminal_width;
let cursor_col = total_col % terminal_width;
```

**Redraw changes:**
- Current: `move_to_column(col)` (single-line assumption)
- New: `move_to(row, col)` relative to the prompt's last line, clearing wrapped overflow lines

### 4. SDK Style Helper

New module: `crates/kish-plugin-sdk/src/style.rs`

```rust
pub enum Color {
    Black, Red, Green, Yellow, Blue, Magenta, Cyan, White,
    Rgb(u8, u8, u8),
    Fixed(u8),       // 256-color
    Default,
}

pub struct Style {
    fg: Option<Color>,
    bg: Option<Color>,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
}

impl Style {
    pub fn new() -> Self;
    pub fn fg(self, color: Color) -> Self;
    pub fn bg(self, color: Color) -> Self;
    pub fn bold(self) -> Self;
    pub fn dim(self) -> Self;
    pub fn italic(self) -> Self;
    pub fn underline(self) -> Self;

    /// Returns text wrapped in ANSI escape codes, with automatic reset at end
    /// Example: Style::new().fg(Green).bold().paint(" main") → "\x1b[1;32m main\x1b[0m"
    pub fn paint(&self, text: &str) -> String;
}
```

**Design principles:**
- Pure string generation, no external crate dependencies
- Builder pattern for ergonomic method chaining
- `paint()` always appends `\x1b[0m` to prevent style leaking
- Rust-only utility, no impact on C ABI

**Plugin usage example:**

```rust
fn hook_pre_prompt(&mut self, api: &PluginApi) {
    let cwd = Style::new().fg(Blue).paint(&api.cwd());
    let branch = Style::new().fg(Green).bold().paint(" main");
    let prompt_char = Style::new().fg(Magenta).bold().paint("❯");
    api.set_var("PS1", &format!("{cwd} {branch}\n{prompt_char} ")).ok();
}
```

## Data Flow

```
1. Reap zombies, display job notifications
2. Call pre_prompt hook
   └─ Plugin: set_var("PS1", "\x1b[34m~/proj\x1b[0m \x1b[32m main\x1b[0m\n\x1b[1;35m❯\x1b[0m ")
3. Expand PS1 (parameter/command substitution)
4. Build PromptInfo
   ├─ upper_lines: ["\x1b[34m~/proj\x1b[0m \x1b[32m main\x1b[0m"]
   ├─ last_line: "\x1b[1;35m❯\x1b[0m "
   └─ last_line_width: 2
5. Print upper_lines to stderr
6. Pass last_line + last_line_width to line editor
7. Line editor event loop
   ├─ Key event → process input → 2D cursor calc → redraw
   ├─ Resize event → update terminal width → full redraw
   └─ Ctrl+L → clear screen → re-output upper_lines → redraw
8. Input confirmed → execute command → back to step 1
```

## Files Changed

| File | Change |
|---|---|
| `Cargo.toml` | Add `unicode-width` dependency |
| `src/interactive/display_width.rs` | **New** — `strip_ansi()`, `display_width()` |
| `src/interactive/prompt.rs` | `PromptInfo` struct, prompt splitting logic |
| `src/interactive/line_editor.rs` | Display-width cursor calc, 2D cursor, wrapping, Resize handling |
| `src/interactive/terminal.rs` | Terminal width query helper (if needed) |
| `src/interactive/mod.rs` | Build PromptInfo in REPL loop, Ctrl+L upper_lines re-output |
| `crates/kish-plugin-sdk/src/style.rs` | **New** — `Style`, `Color`, `paint()` |
| `crates/kish-plugin-sdk/src/lib.rs` | Add `pub mod style;` |

## Out of Scope

- Right-aligned prompt (`PS1_RIGHT`)
- pre_prompt timeout / async support
- Segment API (Approach C)
