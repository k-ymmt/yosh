# Selector UI Modernization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unify the two duplicated interactive selection UIs (Tab completion, Ctrl+R history) into one shared fzf-style selector with match highlighting, colors, CJK-safe truncation, and richer key bindings.

**Architecture:** New module `src/interactive/selector.rs` holds a generic `SelectorUI`. `CompletionUI::run` and `FuzzySearchUI::run` keep their public signatures and become thin wrappers. `filter_and_sort` in `fuzzy_search.rs` changes its return type to `ScoredCandidate` (score + text + match positions) so the renderer can highlight matched characters.

**Tech Stack:** Rust, crossterm (already a dependency), unicode-width (already a dependency via `display_width.rs`).

**Spec:** `docs/superpowers/specs/2026-07-03-selector-ui-modernization-design.md`

## Global Constraints

- The working tree contains unrelated in-progress changes (`src/builtin/special.rs`, `src/env/locale.rs`, `src/lexer/*`, `tests/parser_integration.rs`, `tests/pty_*.rs`). **Stage only the files listed in your task** — never `git add -A` or `git add .`.
- Error-message prefix, exit codes, POSIX conventions: unchanged by this work (UI-only).
- All commits end with the trailer block:
  ```
  Original task: modernize the interactive selector UI (files & history)

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01P49L6y4p4AqxCmES9xLNsc
  ```
- Run `cargo fmt` before every commit. `cargo test` for the full suite takes 1–3+ minutes — run with a generous timeout or in the background; per-file tests (`cargo test --test interactive`, `cargo test selector`) are fast.
- Do NOT use `cargo build --workspace` / `cargo test --workspace` (wasm plugin crates fail to host-build).
- Pointer/prompt glyph is `❯` (U+276F, unambiguous width 1) — NOT `▶` (U+25B6), which is East-Asian-Ambiguous width and misaligns on CJK terminals. The spec's ASCII mockups show `▶`; this plan supersedes that glyph choice.

---

### Task 1: Add `set_bg_color` to the Terminal trait

**Files:**
- Modify: `src/interactive/terminal.rs`
- Modify: `tests/helpers/mock_terminal.rs`
- Modify: `src/interactive/completion.rs` (test-module `MockTerm`, around line 797)

**Interfaces:**
- Produces: `Terminal::set_bg_color(&mut self, color: Color) -> io::Result<()>` — used by Task 4's renderer.

- [ ] **Step 1: Add the trait method**

In `src/interactive/terminal.rs`, after the `set_fg_color` declaration in `trait Terminal` (line 52), add:

```rust
    /// Set background color.
    #[allow(dead_code)] // used by the selector renderer (Task 4); mocks implement it
    fn set_bg_color(&mut self, color: Color) -> io::Result<()>;
```

(The `#[allow(dead_code)]` is temporary; Task 4 removes it when the renderer starts calling the method. If clippy/rustc does not complain without it, omit it.)

In `impl Terminal for CrosstermTerminal`, after `set_fg_color` (line 184), add:

```rust
    fn set_bg_color(&mut self, color: Color) -> io::Result<()> {
        self.stdout.execute(SetBackgroundColor(color))?;
        Ok(())
    }
```

Update the crossterm import at the top of the file:

```rust
    style::{Attribute, Color, SetAttribute, SetBackgroundColor, SetForegroundColor},
```

- [ ] **Step 2: Implement in both mocks**

`tests/helpers/mock_terminal.rs` — after the `set_fg_color` impl (line 136), add:

```rust
    fn set_bg_color(&mut self, color: crossterm::style::Color) -> io::Result<()> {
        self.output.push(format!("[BG:{:?}]", color));
        Ok(())
    }
```

`src/interactive/completion.rs` test module — after the `set_fg_color` impl (line 844), add:

```rust
        fn set_bg_color(&mut self, _color: crossterm::style::Color) -> std::io::Result<()> {
            Ok(())
        }
```

- [ ] **Step 3: Verify it compiles and existing tests pass**

Run: `cargo test --test interactive && cargo test --lib interactive`
Expected: PASS (no behavior change; only a new trait method).

- [ ] **Step 4: Commit**

```bash
cargo fmt
git add src/interactive/terminal.rs tests/helpers/mock_terminal.rs src/interactive/completion.rs
git commit -m "feat(interactive): add set_bg_color to Terminal trait"
```

(Include the standard trailer block from Global Constraints in this and every commit.)

---

### Task 2: `ScoredCandidate` — expose fuzzy match positions

**Files:**
- Modify: `src/interactive/fuzzy_search.rs`
- Modify: `src/interactive/completion.rs` (the `CompletionUI` consumer of `filter_and_sort`)

**Interfaces:**
- Produces: 
  ```rust
  pub struct ScoredCandidate { pub score: i64, pub text: String, pub positions: Vec<usize> }
  pub fn filter_and_sort(query: &str, entries: &[String]) -> Vec<ScoredCandidate>
  ```
  `positions` holds ascending **char indices** (not byte offsets) of matched query chars in `text`. Task 3/4 consume this.

- [ ] **Step 1: Write the failing test**

In `src/interactive/fuzzy_search.rs` tests module add:

```rust
    #[test]
    fn test_filter_and_sort_positions() {
        let entries = vec!["git checkout".to_string()];
        let results = filter_and_sort("gco", &entries);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].text, "git checkout");
        // g=0, c=4 (start of "checkout"), o=9 (greedy scan)
        assert_eq!(results[0].positions, vec![0, 4, 9]);
    }

    #[test]
    fn test_filter_and_sort_empty_query_no_positions() {
        let entries = vec!["anything".to_string()];
        let results = filter_and_sort("", &entries);
        assert!(results[0].positions.is_empty());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib fuzzy_search 2>&1 | tail -20`
Expected: COMPILE FAIL (`results[0].text` — no field `text` on tuple).

- [ ] **Step 3: Implement `ScoredCandidate`**

In `src/interactive/fuzzy_search.rs`:

Remove the `#[allow(dead_code)]` attribute from the `positions` field of `FuzzyMatch` (line 5).

Replace `filter_and_sort` (lines 78–86) with:

```rust
/// A candidate with its fuzzy score and matched char indices (ascending).
#[derive(Debug, Clone)]
pub struct ScoredCandidate {
    pub score: i64,
    pub text: String,
    pub positions: Vec<usize>,
}

/// Filter entries by fuzzy match and return sorted by score descending.
/// The sort is stable: equal scores keep the input order.
pub fn filter_and_sort(query: &str, entries: &[String]) -> Vec<ScoredCandidate> {
    let mut results: Vec<ScoredCandidate> = entries
        .iter()
        .filter_map(|entry| {
            fuzzy_match(query, entry).map(|m| ScoredCandidate {
                score: m.score,
                text: entry.clone(),
                positions: m.positions,
            })
        })
        .collect();
    results.sort_by(|a, b| b.score.cmp(&a.score));
    results
}
```

- [ ] **Step 4: Fix the two existing consumers (mechanical)**

Both `FuzzySearchUI` (fuzzy_search.rs) and `CompletionUI` (completion.rs) store `candidates: Vec<(i64, String)>`. Change both struct fields to `candidates: Vec<ScoredCandidate>` and update the four usage patterns in **each** file:

1. Empty-query candidate construction (fuzzy_search.rs lines 120 and 233; completion.rs lines 298 and 402):
   ```rust
   // before: entries.iter().cloned().map(|e| (0, e)).collect()
   entries
       .iter()
       .map(|e| ScoredCandidate {
           score: 0,
           text: e.clone(),
           positions: Vec::new(),
       })
       .collect()
   ```
   (In completion.rs the variable is `all_candidates`/`candidates` — same shape.)

2. Selection on Enter/Tab (fuzzy_search.rs line 171; completion.rs line 345):
   ```rust
   // before: if let Some((_score, line)) = self.candidates.get(self.selected)
   if let Some(cand) = self.candidates.get(self.selected) {
       SearchAction::Select(cand.text.clone())
   ```
   (completion.rs: `CompletionAction::Select(cand.text.clone())`.)

3. Draw loop (fuzzy_search.rs line 270; completion.rs line 438):
   ```rust
   // before: let (_score, ref line) = self.candidates[i];
   let line = &self.candidates[i].text;
   ```

4. In completion.rs, add `ScoredCandidate` to the import: `use super::fuzzy_search::{ScoredCandidate, filter_and_sort};`

Update the existing `test_filter_and_sort` test (fuzzy_search.rs lines 383–396): `results[0].1` → `results[0].text`, `w[0].0 >= w[1].0` → `w[0].score >= w[1].score`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib interactive && cargo test --test interactive`
Expected: PASS, including the two new position tests.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add src/interactive/fuzzy_search.rs src/interactive/completion.rs
git commit -m "refactor(interactive): return match positions from filter_and_sort"
```

---

### Task 3: `selector.rs` — shared selector with unified key handling

**Files:**
- Create: `src/interactive/selector.rs`
- Modify: `src/interactive/mod.rs` (add `pub mod selector;` after `pub mod prompt;`, keeping alphabetical order)

**Interfaces:**
- Consumes: `ScoredCandidate`, `filter_and_sort` (Task 2); `Terminal` trait (Task 1).
- Produces:
  ```rust
  pub enum ItemStyle { Plain, Path }
  pub struct SelectorOptions { pub item_style: ItemStyle, pub colors: bool }
  pub fn colors_enabled() -> bool
  impl SelectorUI {
      pub fn run<T: Terminal>(items: &[String], opts: SelectorOptions, term: &mut T)
          -> io::Result<Option<String>>;
  }
  ```
  Tasks 5–6 call `SelectorUI::run`. `items` order = display order for the empty query (index 0 is the initially selected best candidate, drawn at the bottom, nearest the query line).

This task implements the module with the **legacy look** (`colors: false` path: `> ` + reverse video) but the **new key map** and **width-aware truncation with `…`**. Task 4 adds the colored rendering path.

- [ ] **Step 1: Write the failing tests**

Create `src/interactive/selector.rs` containing ONLY a tests module first is impractical in Rust (module must compile), so write tests and implementation in one file but **write the test module first** as your checklist, then make it compile. Test module (bottom of the new file):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{Event, KeyEvent, KeyModifiers};
    use std::collections::VecDeque;

    /// Mock terminal that replays events and records output + style markers.
    struct MockTerm {
        events: VecDeque<Event>,
        cursor_row: i32,
        output: Vec<String>,
        size: (u16, u16),
    }

    impl MockTerm {
        fn new(events: Vec<Event>) -> Self {
            Self {
                events: VecDeque::from(events),
                cursor_row: 0,
                output: Vec::new(),
                size: (80, 24),
            }
        }

        fn with_size(events: Vec<Event>, w: u16, h: u16) -> Self {
            let mut t = Self::new(events);
            t.size = (w, h);
            t
        }

        fn key(code: KeyCode) -> Event {
            Event::Key(KeyEvent::new(code, KeyModifiers::empty()))
        }

        fn ctrl(ch: char) -> Event {
            Event::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL))
        }

        fn chars(s: &str) -> Vec<Event> {
            s.chars().map(|c| Self::key(KeyCode::Char(c))).collect()
        }

        fn dump(&self) -> String {
            self.output.concat()
        }
    }

    impl Terminal for MockTerm {
        fn read_event(&mut self) -> io::Result<Event> {
            self.events
                .pop_front()
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "no events"))
        }
        fn size(&self) -> io::Result<(u16, u16)> {
            Ok(self.size)
        }
        fn enable_raw_mode(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn disable_raw_mode(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn move_to_column(&mut self, _col: u16) -> io::Result<()> {
            Ok(())
        }
        fn move_up(&mut self, n: u16) -> io::Result<()> {
            self.cursor_row -= n as i32;
            Ok(())
        }
        fn move_down(&mut self, n: u16) -> io::Result<()> {
            self.cursor_row += n as i32;
            Ok(())
        }
        fn clear_current_line(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn clear_until_newline(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn clear_all(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn write_str(&mut self, s: &str) -> io::Result<()> {
            self.cursor_row += s.chars().filter(|&c| c == '\n').count() as i32;
            self.output.push(s.to_string());
            Ok(())
        }
        fn set_reverse(&mut self, on: bool) -> io::Result<()> {
            self.output.push(if on { "[REV]" } else { "[/REV]" }.to_string());
            Ok(())
        }
        fn set_dim(&mut self, on: bool) -> io::Result<()> {
            self.output.push(if on { "[DIM]" } else { "[/DIM]" }.to_string());
            Ok(())
        }
        fn set_fg_color(&mut self, color: crossterm::style::Color) -> io::Result<()> {
            self.output.push(format!("[FG:{:?}]", color));
            Ok(())
        }
        fn set_bg_color(&mut self, color: crossterm::style::Color) -> io::Result<()> {
            self.output.push(format!("[BG:{:?}]", color));
            Ok(())
        }
        fn reset_style(&mut self) -> io::Result<()> {
            self.output.push("[RESET]".to_string());
            Ok(())
        }
        fn set_bold(&mut self, on: bool) -> io::Result<()> {
            self.output.push(if on { "[BOLD]" } else { "[/BOLD]" }.to_string());
            Ok(())
        }
        fn set_underline(&mut self, _on: bool) -> io::Result<()> {
            Ok(())
        }
        fn write_char(&mut self, ch: char) -> io::Result<()> {
            if ch == '\n' {
                self.cursor_row += 1;
            }
            self.output.push(ch.to_string());
            Ok(())
        }
        fn hide_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn show_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn plain_opts() -> SelectorOptions {
        SelectorOptions {
            item_style: ItemStyle::Plain,
            colors: false,
        }
    }

    fn items(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    // ── selection & cancel ──────────────────────────────────────────

    #[test]
    fn test_enter_selects_first() {
        let mut term = MockTerm::new(vec![MockTerm::key(KeyCode::Enter)]);
        let result = SelectorUI::run(&items(&["a", "b", "c"]), plain_opts(), &mut term).unwrap();
        assert_eq!(result, Some("a".to_string()));
    }

    #[test]
    fn test_up_then_enter_selects_second() {
        let mut term = MockTerm::new(vec![
            MockTerm::key(KeyCode::Up),
            MockTerm::key(KeyCode::Enter),
        ]);
        let result = SelectorUI::run(&items(&["a", "b", "c"]), plain_opts(), &mut term).unwrap();
        assert_eq!(result, Some("b".to_string()));
    }

    #[test]
    fn test_esc_cancels() {
        let mut term = MockTerm::new(vec![MockTerm::key(KeyCode::Esc)]);
        let result = SelectorUI::run(&items(&["a", "b"]), plain_opts(), &mut term).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_ctrl_g_cancels() {
        let mut term = MockTerm::new(vec![MockTerm::ctrl('g')]);
        let result = SelectorUI::run(&items(&["a", "b"]), plain_opts(), &mut term).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_ctrl_c_cancels() {
        let mut term = MockTerm::new(vec![MockTerm::ctrl('c')]);
        let result = SelectorUI::run(&items(&["a", "b"]), plain_opts(), &mut term).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_empty_items_returns_none() {
        let mut term = MockTerm::new(vec![]);
        let result = SelectorUI::run(&[], plain_opts(), &mut term).unwrap();
        assert_eq!(result, None);
    }

    // ── Tab / Shift-Tab cycling ─────────────────────────────────────

    #[test]
    fn test_tab_cycles_to_next() {
        let mut term = MockTerm::new(vec![
            MockTerm::key(KeyCode::Tab),
            MockTerm::key(KeyCode::Enter),
        ]);
        let result = SelectorUI::run(&items(&["a", "b", "c"]), plain_opts(), &mut term).unwrap();
        assert_eq!(result, Some("b".to_string()));
    }

    #[test]
    fn test_tab_wraps_at_end() {
        let mut term = MockTerm::new(vec![
            MockTerm::key(KeyCode::Tab),
            MockTerm::key(KeyCode::Tab),
            MockTerm::key(KeyCode::Tab),
            MockTerm::key(KeyCode::Enter),
        ]);
        let result = SelectorUI::run(&items(&["a", "b", "c"]), plain_opts(), &mut term).unwrap();
        assert_eq!(result, Some("a".to_string()));
    }

    #[test]
    fn test_backtab_wraps_to_last() {
        let mut term = MockTerm::new(vec![
            MockTerm::key(KeyCode::BackTab),
            MockTerm::key(KeyCode::Enter),
        ]);
        let result = SelectorUI::run(&items(&["a", "b", "c"]), plain_opts(), &mut term).unwrap();
        assert_eq!(result, Some("c".to_string()));
    }

    // ── PageUp / PageDown / Home / End ──────────────────────────────

    #[test]
    fn test_pageup_moves_by_page_and_clamps() {
        // 24-row terminal → max_visible = 9. Two PageUps from 0 on 12 items:
        // 0 → 9 → 11 (clamped to last index).
        let names: Vec<String> = (0..12).map(|i| format!("item{:02}", i)).collect();
        let mut term = MockTerm::new(vec![
            MockTerm::key(KeyCode::PageUp),
            MockTerm::key(KeyCode::PageUp),
            MockTerm::key(KeyCode::Enter),
        ]);
        let result = SelectorUI::run(&names, plain_opts(), &mut term).unwrap();
        assert_eq!(result, Some("item11".to_string()));
    }

    #[test]
    fn test_pagedown_moves_back_and_clamps() {
        let names: Vec<String> = (0..12).map(|i| format!("item{:02}", i)).collect();
        let mut term = MockTerm::new(vec![
            MockTerm::key(KeyCode::PageUp),  // → 9
            MockTerm::key(KeyCode::PageDown), // → 0
            MockTerm::key(KeyCode::PageDown), // clamped at 0
            MockTerm::key(KeyCode::Enter),
        ]);
        let result = SelectorUI::run(&names, plain_opts(), &mut term).unwrap();
        assert_eq!(result, Some("item00".to_string()));
    }

    #[test]
    fn test_end_jumps_to_last_home_to_first() {
        let mut term = MockTerm::new(vec![
            MockTerm::key(KeyCode::End),
            MockTerm::key(KeyCode::Enter),
        ]);
        let result = SelectorUI::run(&items(&["a", "b", "c"]), plain_opts(), &mut term).unwrap();
        assert_eq!(result, Some("c".to_string()));

        let mut term = MockTerm::new(vec![
            MockTerm::key(KeyCode::End),
            MockTerm::key(KeyCode::Home),
            MockTerm::key(KeyCode::Enter),
        ]);
        let result = SelectorUI::run(&items(&["a", "b", "c"]), plain_opts(), &mut term).unwrap();
        assert_eq!(result, Some("a".to_string()));
    }

    // ── query editing ───────────────────────────────────────────────

    #[test]
    fn test_typed_query_filters() {
        let mut events = MockTerm::chars("ban");
        events.push(MockTerm::key(KeyCode::Enter));
        let mut term = MockTerm::new(events);
        let result = SelectorUI::run(
            &items(&["apple.txt", "banana.txt", "cherry.txt"]),
            plain_opts(),
            &mut term,
        )
        .unwrap();
        assert_eq!(result, Some("banana.txt".to_string()));
    }

    #[test]
    fn test_ctrl_u_clears_query() {
        // "ban" narrows to banana; Ctrl+U restores the full list, so Enter
        // picks the original first item again.
        let mut events = MockTerm::chars("ban");
        events.push(MockTerm::ctrl('u'));
        events.push(MockTerm::key(KeyCode::Enter));
        let mut term = MockTerm::new(events);
        let result = SelectorUI::run(
            &items(&["apple.txt", "banana.txt", "cherry.txt"]),
            plain_opts(),
            &mut term,
        )
        .unwrap();
        assert_eq!(result, Some("apple.txt".to_string()));
    }

    #[test]
    fn test_ctrl_r_moves_to_next() {
        let mut term = MockTerm::new(vec![MockTerm::ctrl('r'), MockTerm::key(KeyCode::Enter)]);
        let result = SelectorUI::run(&items(&["a", "b", "c"]), plain_opts(), &mut term).unwrap();
        assert_eq!(result, Some("b".to_string()));
    }

    // ── cursor discipline ───────────────────────────────────────────

    #[test]
    fn test_no_cursor_drift_on_cancel_and_select() {
        let mut term = MockTerm::new(vec![
            MockTerm::key(KeyCode::Up),
            MockTerm::key(KeyCode::Up),
            MockTerm::key(KeyCode::Down),
            MockTerm::key(KeyCode::Esc),
        ]);
        let _ = SelectorUI::run(&items(&["a", "b", "c"]), plain_opts(), &mut term).unwrap();
        assert_eq!(term.cursor_row, 0);

        let mut term = MockTerm::new(vec![
            MockTerm::key(KeyCode::Up),
            MockTerm::key(KeyCode::Enter),
        ]);
        let _ = SelectorUI::run(&items(&["a", "b", "c"]), plain_opts(), &mut term).unwrap();
        assert_eq!(term.cursor_row, 0);
    }

    // ── truncation ──────────────────────────────────────────────────

    #[test]
    fn test_fit_to_width_ascii_fits() {
        assert_eq!(fit_to_width("hello", 10), (5, false));
    }

    #[test]
    fn test_fit_to_width_ascii_truncates() {
        // budget 5 → 4 chars + ellipsis
        assert_eq!(fit_to_width("hello!", 5), (4, true));
    }

    #[test]
    fn test_fit_to_width_cjk() {
        // "日本語" = 6 columns; budget 5 → chars fitting 4 cols = 2 chars + ellipsis
        assert_eq!(fit_to_width("日本語", 5), (2, true));
        assert_eq!(fit_to_width("日本語", 6), (3, false));
    }

    #[test]
    fn test_cjk_candidate_truncated_with_ellipsis() {
        // width 12 → row budget 10; "日本語のファイル名.rs" is wider, so the
        // rendered row must contain "…".
        let mut term = MockTerm::with_size(vec![MockTerm::key(KeyCode::Esc)], 12, 24);
        let _ = SelectorUI::run(
            &items(&["日本語のファイル名.rs"]),
            plain_opts(),
            &mut term,
        )
        .unwrap();
        assert!(term.dump().contains('…'), "output: {}", term.dump());
    }
}
```

- [ ] **Step 2: Write the implementation**

Top of `src/interactive/selector.rs` (above the tests module):

```rust
//! Shared fuzzy-selector UI.
//!
//! One interactive list-selection component used by both Tab completion
//! (`CompletionUI`) and Ctrl+R history search (`FuzzySearchUI`). Renders an
//! fzf-style list above the prompt: candidates, a separator, and a query
//! line. `items` order is the display order for the empty query; index 0 is
//! the best candidate, drawn at the bottom (nearest the query line).

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::io;
use unicode_width::UnicodeWidthChar;

use super::display_width::display_width;
use super::fuzzy_search::{ScoredCandidate, filter_and_sort};
use super::terminal::Terminal;

// (Task 4 adds `use crossterm::style::Color;` when colored rendering lands.)

/// How candidate rows are styled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStyle {
    /// No per-item styling (history entries).
    Plain,
    /// Path candidates: a trailing '/' marks a directory (drawn blue).
    Path,
}

#[derive(Debug, Clone, Copy)]
pub struct SelectorOptions {
    pub item_style: ItemStyle,
    pub colors: bool,
}

/// Whether the selector should use colors, honoring NO_COLOR / CLICOLOR
/// conventions. Interactive mode implies a tty, so no isatty check here.
pub fn colors_enabled() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if let Some(val) = std::env::var_os("CLICOLOR_FORCE")
        && val != "0"
    {
        return true;
    }
    if let Some(val) = std::env::var_os("CLICOLOR")
        && val == "0"
    {
        return false;
    }
    true
}

enum SelectAction {
    Continue,
    Select(String),
    Cancel,
}

pub struct SelectorUI {
    query: Vec<char>,
    selected: usize,
    scroll_offset: usize,
    candidates: Vec<ScoredCandidate>,
    max_visible: usize,
    total: usize,
    opts: SelectorOptions,
}

impl SelectorUI {
    /// Show the selector for `items`. Returns `Some(selected)` or `None` on
    /// cancel. Blocking; takes over the terminal below the current line.
    pub fn run<T: Terminal>(
        items: &[String],
        opts: SelectorOptions,
        term: &mut T,
    ) -> io::Result<Option<String>> {
        if items.is_empty() {
            return Ok(None);
        }

        let (_, term_height) = term.size()?;
        let max_visible = ((term_height as f32) * 0.4).max(3.0) as usize;

        let mut ui = SelectorUI {
            query: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            candidates: unfiltered(items),
            max_visible,
            total: items.len(),
            opts,
        };

        let draw_lines = ui.max_visible + 2; // candidates + separator + query
        term.hide_cursor()?;
        for _ in 0..draw_lines {
            term.write_str("\r\n")?;
        }
        term.move_up(draw_lines as u16)?;
        ui.draw(term)?;

        // Enable raw mode for character-by-character input. The caller
        // (read_line_loop) disabled raw mode before invoking us.
        term.enable_raw_mode()?;
        let result = ui.run_loop(term, items, draw_lines);
        // Disable raw mode regardless of result so the caller can re-enable.
        let _ = term.disable_raw_mode();
        let _ = term.show_cursor();
        result
    }

    fn run_loop<T: Terminal>(
        &mut self,
        term: &mut T,
        items: &[String],
        draw_lines: usize,
    ) -> io::Result<Option<String>> {
        loop {
            term.flush()?;
            if let Event::Key(key_event) = term.read_event()? {
                match self.handle_key(key_event, items) {
                    SelectAction::Continue => {}
                    SelectAction::Select(line) => {
                        self.clear_ui(term, draw_lines)?;
                        return Ok(Some(line));
                    }
                    SelectAction::Cancel => {
                        self.clear_ui(term, draw_lines)?;
                        return Ok(None);
                    }
                }
                self.draw(term)?;
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent, items: &[String]) -> SelectAction {
        match (key.code, key.modifiers) {
            (KeyCode::Enter, _) => {
                if let Some(cand) = self.candidates.get(self.selected) {
                    SelectAction::Select(cand.text.clone())
                } else {
                    SelectAction::Cancel
                }
            }
            (KeyCode::Esc, _) => SelectAction::Cancel,
            (KeyCode::Char('g'), m) | (KeyCode::Char('c'), m)
                if m.contains(KeyModifiers::CONTROL) =>
            {
                SelectAction::Cancel
            }
            (KeyCode::Char('p'), m) | (KeyCode::Char('r'), m)
                if m.contains(KeyModifiers::CONTROL) =>
            {
                self.move_visual_up();
                SelectAction::Continue
            }
            (KeyCode::Char('n'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.move_visual_down();
                SelectAction::Continue
            }
            (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => {
                if !self.query.is_empty() {
                    self.query.clear();
                    self.update_candidates(items);
                }
                SelectAction::Continue
            }
            (KeyCode::Up, _) => {
                self.move_visual_up();
                SelectAction::Continue
            }
            (KeyCode::Down, _) => {
                self.move_visual_down();
                SelectAction::Continue
            }
            (KeyCode::Tab, _) => {
                self.cycle_next();
                SelectAction::Continue
            }
            (KeyCode::BackTab, _) => {
                self.cycle_prev();
                SelectAction::Continue
            }
            (KeyCode::PageUp, _) => {
                if !self.candidates.is_empty() {
                    self.selected =
                        (self.selected + self.max_visible).min(self.candidates.len() - 1);
                    self.adjust_scroll();
                }
                SelectAction::Continue
            }
            (KeyCode::PageDown, _) => {
                self.selected = self.selected.saturating_sub(self.max_visible);
                self.adjust_scroll();
                SelectAction::Continue
            }
            (KeyCode::Home, _) => {
                self.selected = 0;
                self.adjust_scroll();
                SelectAction::Continue
            }
            (KeyCode::End, _) => {
                self.selected = self.candidates.len().saturating_sub(1);
                self.adjust_scroll();
                SelectAction::Continue
            }
            (KeyCode::Backspace, _) => {
                if !self.query.is_empty() {
                    self.query.pop();
                    self.update_candidates(items);
                }
                SelectAction::Continue
            }
            (KeyCode::Char(ch), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.query.push(ch);
                self.update_candidates(items);
                SelectAction::Continue
            }
            _ => SelectAction::Continue,
        }
    }

    /// Move toward higher indices (visually up: index 0 sits at the bottom).
    fn move_visual_up(&mut self) {
        if self.selected + 1 < self.candidates.len() {
            self.selected += 1;
            self.adjust_scroll();
        }
    }

    fn move_visual_down(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.adjust_scroll();
        }
    }

    fn cycle_next(&mut self) {
        let len = self.candidates.len();
        if len > 0 {
            self.selected = (self.selected + 1) % len;
            self.adjust_scroll();
        }
    }

    fn cycle_prev(&mut self) {
        let len = self.candidates.len();
        if len > 0 {
            self.selected = (self.selected + len - 1) % len;
            self.adjust_scroll();
        }
    }

    fn update_candidates(&mut self, items: &[String]) {
        let query: String = self.query.iter().collect();
        self.candidates = if query.is_empty() {
            unfiltered(items)
        } else {
            filter_and_sort(&query, items)
        };
        self.selected = 0;
        self.scroll_offset = 0;
    }

    fn adjust_scroll(&mut self) {
        if self.selected >= self.scroll_offset + self.max_visible {
            self.scroll_offset = self.selected - self.max_visible + 1;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
    }

    fn draw<T: Terminal>(&self, term: &mut T) -> io::Result<()> {
        let (term_width, _) = term.size()?;
        let width = term_width as usize;

        term.move_to_column(0)?;

        let visible_end = (self.scroll_offset + self.max_visible).min(self.candidates.len());
        let visible_range = self.scroll_offset..visible_end;
        let visible_count = visible_range.len();

        // Fill empty lines if fewer candidates than max_visible
        for _ in 0..(self.max_visible - visible_count) {
            term.clear_current_line()?;
            term.write_str("\r\n")?;
        }

        // Draw candidates in reverse order (highest index = top of UI)
        for i in visible_range.rev() {
            term.clear_current_line()?;
            self.draw_row(term, &self.candidates[i], i == self.selected, width)?;
            term.write_str("\r\n")?;
        }

        self.draw_separator(term, width)?;
        self.draw_query_line(term)?;

        // Move back to the top of the UI area. We wrote (max_visible + 1)
        // newlines: max_visible candidate rows (including padding) plus the
        // separator. The query line has no trailing newline.
        let total_lines = self.max_visible + 1;
        term.move_up(total_lines as u16)?;
        term.flush()?;
        Ok(())
    }

    fn draw_row<T: Terminal>(
        &self,
        term: &mut T,
        cand: &ScoredCandidate,
        is_selected: bool,
        width: usize,
    ) -> io::Result<()> {
        let budget = width.saturating_sub(2); // 2 columns for the pointer prefix
        let (char_count, truncated) = fit_to_width(&cand.text, budget);

        // Legacy look (colors disabled): "> " + reverse video.
        if is_selected {
            term.set_reverse(true)?;
            term.write_str("> ")?;
        } else {
            term.write_str("  ")?;
        }
        let mut text: String = cand.text.chars().take(char_count).collect();
        if truncated {
            text.push('…');
        }
        term.write_str(&text)?;
        if is_selected {
            term.set_reverse(false)?;
        }
        Ok(())
    }

    fn draw_separator<T: Terminal>(&self, term: &mut T, width: usize) -> io::Result<()> {
        term.clear_current_line()?;
        let sep: String = "\u{2500}".repeat(width.min(40));
        term.write_str(&format!("  {}\r\n", sep))?;
        Ok(())
    }

    fn draw_query_line<T: Terminal>(&self, term: &mut T) -> io::Result<()> {
        term.clear_current_line()?;
        let query_str: String = self.query.iter().collect();
        term.write_str(&format!(
            "  {}/{} > {}",
            self.candidates.len(),
            self.total,
            query_str
        ))?;
        Ok(())
    }

    fn clear_ui<T: Terminal>(&self, term: &mut T, draw_lines: usize) -> io::Result<()> {
        term.move_to_column(0)?;
        for _ in 0..draw_lines {
            term.clear_current_line()?;
            term.write_str("\r\n")?;
        }
        term.move_up(draw_lines as u16)?;
        term.flush()?;
        Ok(())
    }
}

fn unfiltered(items: &[String]) -> Vec<ScoredCandidate> {
    items
        .iter()
        .map(|e| ScoredCandidate {
            score: 0,
            text: e.clone(),
            positions: Vec::new(),
        })
        .collect()
}

/// How many leading chars of `s` fit in `budget` display columns.
///
/// Returns `(char_count, truncated)`. When the whole string fits, count is
/// `s.chars().count()` and truncated is false. Otherwise the count fits in
/// `budget - 1` columns, leaving one column for the '…' marker.
fn fit_to_width(s: &str, budget: usize) -> (usize, bool) {
    if display_width(s) <= budget {
        return (s.chars().count(), false);
    }
    let limit = budget.saturating_sub(1);
    let mut used = 0;
    let mut count = 0;
    for c in s.chars() {
        let w = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > limit {
            break;
        }
        used += w;
        count += 1;
    }
    (count, true)
}
```

Note: the `opts` field is not yet read by `draw_row` — that arrives in Task 4. If rustc/clippy flags `opts` (or `ItemStyle`) as dead code in this task, add a temporary `#[allow(dead_code)]` on the `opts` field and remove it in Task 4.

Add to `src/interactive/mod.rs` (alphabetical, after `pub mod prompt;`):

```rust
pub mod selector;
```

- [ ] **Step 3: Run the selector tests**

Run: `cargo test --lib selector`
Expected: PASS (all ~18 tests).

- [ ] **Step 4: Run the wider interactive suites**

Run: `cargo test --lib interactive && cargo test --test interactive`
Expected: PASS (nothing consumes selector yet).

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add src/interactive/selector.rs src/interactive/mod.rs
git commit -m "feat(interactive): add shared SelectorUI with unified key map"
```

---

### Task 4: fzf-style colored rendering

**Files:**
- Modify: `src/interactive/selector.rs` (draw_row, draw_separator, draw_query_line)
- Modify: `src/interactive/terminal.rs` (remove the temporary `#[allow(dead_code)]` on `set_bg_color` if it was added in Task 1)

**Interfaces:**
- Consumes: `Terminal::set_bg_color` (Task 1), `ScoredCandidate::positions` (Task 2).
- Produces: no API change; rendering honors `SelectorOptions::colors` and `ItemStyle`.

Color scheme (ANSI 16, matching `highlight.rs` conventions):

| Element | colors=true | colors=false |
|---|---|---|
| Selected row | `❯ ` pointer (cyan) + bg DarkGrey + bold | `> ` + reverse video |
| Matched query chars | cyan (+bold on unselected rows) | plain |
| Directory (`ItemStyle::Path`, trailing `/`) | blue | plain |
| Separator | dim | plain |
| Count `filtered/total` | yellow | plain |
| Query prompt | `❯` cyan | `>` plain |

- [ ] **Step 1: Write the failing tests**

Add to the tests module in `src/interactive/selector.rs`:

```rust
    fn color_opts(style: ItemStyle) -> SelectorOptions {
        SelectorOptions {
            item_style: style,
            colors: true,
        }
    }

    // ── colored rendering ───────────────────────────────────────────

    #[test]
    fn test_colors_selected_row_has_bg_and_pointer() {
        let mut term = MockTerm::new(vec![MockTerm::key(KeyCode::Esc)]);
        let _ = SelectorUI::run(&items(&["a", "b"]), color_opts(ItemStyle::Plain), &mut term)
            .unwrap();
        let out = term.dump();
        assert!(out.contains("[BG:DarkGrey]"), "output: {}", out);
        assert!(out.contains("❯ "), "output: {}", out);
        assert!(!out.contains("[REV]"), "reverse video must not be used: {}", out);
    }

    #[test]
    fn test_colors_matched_chars_cyan() {
        let mut events = MockTerm::chars("b");
        events.push(MockTerm::key(KeyCode::Esc));
        let mut term = MockTerm::new(events);
        let _ = SelectorUI::run(
            &items(&["abc"]),
            color_opts(ItemStyle::Plain),
            &mut term,
        )
        .unwrap();
        // After typing "b", the row for "abc" must switch to cyan right
        // before writing the matched char 'b'.
        let out = term.dump();
        assert!(out.contains("[FG:Cyan]b"), "output: {}", out);
    }

    #[test]
    fn test_colors_directory_blue_in_path_style() {
        let mut term = MockTerm::new(vec![MockTerm::key(KeyCode::Esc)]);
        let _ = SelectorUI::run(
            &items(&["src/", "main.rs"]),
            color_opts(ItemStyle::Path),
            &mut term,
        )
        .unwrap();
        assert!(term.dump().contains("[FG:Blue]"), "output: {}", term.dump());
    }

    #[test]
    fn test_colors_plain_style_no_blue() {
        // History entries ending in '/' must NOT be colored as directories.
        let mut term = MockTerm::new(vec![MockTerm::key(KeyCode::Esc)]);
        let _ = SelectorUI::run(
            &items(&["ls src/"]),
            color_opts(ItemStyle::Plain),
            &mut term,
        )
        .unwrap();
        assert!(!term.dump().contains("[FG:Blue]"), "output: {}", term.dump());
    }

    #[test]
    fn test_colors_count_yellow_and_filtered_total() {
        let mut events = MockTerm::chars("ban");
        events.push(MockTerm::key(KeyCode::Esc));
        let mut term = MockTerm::new(events);
        let _ = SelectorUI::run(
            &items(&["apple.txt", "banana.txt", "cherry.txt"]),
            color_opts(ItemStyle::Plain),
            &mut term,
        )
        .unwrap();
        let out = term.dump();
        // filtered=1, total=3 — the old code printed "1/1"; this must be "1/3".
        assert!(out.contains("[FG:Yellow]1/3"), "output: {}", out);
    }

    #[test]
    fn test_no_colors_keeps_legacy_look() {
        let mut term = MockTerm::new(vec![MockTerm::key(KeyCode::Esc)]);
        let _ = SelectorUI::run(&items(&["a", "b"]), plain_opts(), &mut term).unwrap();
        let out = term.dump();
        assert!(out.contains("[REV]"), "output: {}", out);
        assert!(out.contains("> "), "output: {}", out);
        assert!(!out.contains("[FG:"), "no colors expected: {}", out);
        assert!(!out.contains("[BG:"), "no bg expected: {}", out);
    }

    #[test]
    fn test_plain_count_shows_filtered_and_total() {
        let mut events = MockTerm::chars("ban");
        events.push(MockTerm::key(KeyCode::Esc));
        let mut term = MockTerm::new(events);
        let _ = SelectorUI::run(
            &items(&["apple.txt", "banana.txt", "cherry.txt"]),
            plain_opts(),
            &mut term,
        )
        .unwrap();
        assert!(term.dump().contains("1/3 > ban"), "output: {}", term.dump());
    }
```

- [ ] **Step 2: Run tests to verify the new ones fail**

Run: `cargo test --lib selector 2>&1 | tail -20`
Expected: FAIL — `test_colors_*` tests (no colored output yet). `test_no_colors_keeps_legacy_look` and `test_plain_count_shows_filtered_and_total` should already PASS.

- [ ] **Step 3: Implement colored rendering**

Add the import back at the top of selector.rs (if removed in Task 3):

```rust
use crossterm::style::Color;
```

Replace `draw_row` with:

```rust
    fn draw_row<T: Terminal>(
        &self,
        term: &mut T,
        cand: &ScoredCandidate,
        is_selected: bool,
        width: usize,
    ) -> io::Result<()> {
        let budget = width.saturating_sub(2); // 2 columns for the pointer prefix
        let (char_count, truncated) = fit_to_width(&cand.text, budget);

        if !self.opts.colors {
            // Legacy look: "> " + reverse video for the selected row.
            if is_selected {
                term.set_reverse(true)?;
                term.write_str("> ")?;
            } else {
                term.write_str("  ")?;
            }
            let mut text: String = cand.text.chars().take(char_count).collect();
            if truncated {
                text.push('…');
            }
            term.write_str(&text)?;
            if is_selected {
                term.set_reverse(false)?;
            }
            return Ok(());
        }

        // fzf-style: pointer + background on the selected row, matched query
        // chars in cyan, directories in blue.
        if is_selected {
            term.set_bg_color(Color::DarkGrey)?;
            term.set_bold(true)?;
            term.set_fg_color(Color::Cyan)?;
            term.write_str("\u{276F} ")?; // ❯
        } else {
            term.write_str("  ")?;
        }

        let is_dir = self.opts.item_style == ItemStyle::Path && cand.text.ends_with('/');
        let base = if is_dir { Color::Blue } else { Color::Reset };
        term.set_fg_color(base)?;

        for (ci, ch) in cand.text.chars().take(char_count).enumerate() {
            let matched = cand.positions.binary_search(&ci).is_ok();
            if matched {
                term.set_fg_color(Color::Cyan)?;
                if !is_selected {
                    term.set_bold(true)?;
                }
                term.write_char(ch)?;
                if !is_selected {
                    term.set_bold(false)?;
                }
                term.set_fg_color(base)?;
            } else {
                term.write_char(ch)?;
            }
        }
        if truncated {
            term.write_char('…')?;
        }
        // Reset clears fg, bg, and bold together (Attribute::Reset).
        term.reset_style()?;
        Ok(())
    }
```

Replace `draw_separator` with:

```rust
    fn draw_separator<T: Terminal>(&self, term: &mut T, width: usize) -> io::Result<()> {
        term.clear_current_line()?;
        let sep: String = "\u{2500}".repeat(width.min(40));
        if self.opts.colors {
            term.set_dim(true)?;
            term.write_str(&format!("  {}", sep))?;
            term.set_dim(false)?;
            term.write_str("\r\n")?;
        } else {
            term.write_str(&format!("  {}\r\n", sep))?;
        }
        Ok(())
    }
```

Replace `draw_query_line` with:

```rust
    fn draw_query_line<T: Terminal>(&self, term: &mut T) -> io::Result<()> {
        term.clear_current_line()?;
        let query_str: String = self.query.iter().collect();
        if self.opts.colors {
            term.write_str("  ")?;
            term.set_fg_color(Color::Yellow)?;
            term.write_str(&format!("{}/{}", self.candidates.len(), self.total))?;
            term.set_fg_color(Color::Cyan)?;
            term.write_str(" \u{276F} ")?; // ❯
            term.set_fg_color(Color::Reset)?;
            term.write_str(&query_str)?;
        } else {
            term.write_str(&format!(
                "  {}/{} > {}",
                self.candidates.len(),
                self.total,
                query_str
            ))?;
        }
        Ok(())
    }
```

Remove any temporary `#[allow(dead_code)]` left from Tasks 1/3 (on `Terminal::set_bg_color`, on the `opts` field).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib selector`
Expected: PASS (all tests including the 7 new ones).

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add src/interactive/selector.rs src/interactive/terminal.rs
git commit -m "feat(interactive): fzf-style colored rendering for SelectorUI"
```

---

### Task 5: Rewire `CompletionUI` onto the shared selector

**Files:**
- Modify: `src/interactive/completion.rs`

**Interfaces:**
- Consumes: `SelectorUI::run`, `SelectorOptions`, `ItemStyle`, `colors_enabled` (Tasks 3–4).
- Produces: `CompletionUI::run<T: Terminal>(candidates: &[String], term: &mut T) -> io::Result<Option<String>>` — **unchanged signature**; `line_editor.rs:1136` keeps compiling untouched.

- [ ] **Step 1: Replace the implementation**

In `src/interactive/completion.rs`, delete:
- `enum CompletionAction` (line 268)
- the `CompletionUI` struct fields and its entire `impl` block (lines 274–479)

Replace with:

```rust
/// Interactive fuzzy-filter UI for selecting a completion candidate.
/// Thin wrapper over the shared [`SelectorUI`].
pub struct CompletionUI;

impl CompletionUI {
    /// Returns `Some(selected)` or `None` on cancel.
    pub fn run<T: Terminal>(candidates: &[String], term: &mut T) -> io::Result<Option<String>> {
        SelectorUI::run(
            candidates,
            SelectorOptions {
                item_style: ItemStyle::Path,
                colors: colors_enabled(),
            },
            term,
        )
    }
}
```

Update imports at the top of the file:
- Remove: `use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};` and `use super::fuzzy_search::{ScoredCandidate, filter_and_sort};` (no longer used by production code in this file).
- Add: `use super::selector::{ItemStyle, SelectorOptions, SelectorUI, colors_enabled};`
- Keep: `use super::terminal::Terminal;` and the `std::io` import.

- [ ] **Step 2: Remove superseded tests**

In the completion.rs test module, delete `struct MockTerm`, its impls, and the six UI tests (`test_completion_ui_select_first`, `test_completion_ui_navigate_and_select`, `test_completion_ui_cancel`, `test_completion_ui_tab_confirms`, `test_completion_ui_fuzzy_filter`, `test_completion_ui_no_cursor_drift`) — all superseded by the selector.rs test suite (Tab-confirms is intentionally superseded by Tab-cycles). Also remove the now-unused test imports (`crossterm::event::...`, `VecDeque`, `Terminal as TerminalTrait`) — keep whatever the remaining `extract_*`/`split_*`/`complete_*` tests still use.

Note: end-to-end coverage of the wrapper through `LineEditor` remains in `tests/interactive.rs::test_double_tab_opens_completion_ui` (Up + Enter inside the UI — unaffected by the Tab-cycle change).

- [ ] **Step 3: Run tests**

Run: `cargo test --lib interactive && cargo test --test interactive`
Expected: PASS. `test_double_tab_opens_completion_ui` still passes (same selection semantics for Up/Enter).

- [ ] **Step 4: Commit**

```bash
cargo fmt
git add src/interactive/completion.rs
git commit -m "refactor(interactive): CompletionUI delegates to shared SelectorUI"
```

---

### Task 6: Rewire `FuzzySearchUI` onto the shared selector

**Files:**
- Modify: `src/interactive/fuzzy_search.rs`

**Interfaces:**
- Consumes: `SelectorUI::run` (Task 3–4), `History::entries() -> &[String]`.
- Produces: `FuzzySearchUI::run<T: Terminal>(history: &History, term: &mut T) -> io::Result<Option<String>>` — **unchanged signature**; `line_editor.rs:525` and `:1034` keep compiling untouched.

- [ ] **Step 1: Replace the implementation**

In `src/interactive/fuzzy_search.rs`, delete everything from the `// Fuzzy search UI (Ctrl+R)` section comment (around line 88) through the end of `enum SearchAction` — i.e. the `FuzzySearchUI` struct fields, its whole `impl` block, and `SearchAction`. Keep `FuzzyMatch`, `fuzzy_match`, `ScoredCandidate`, `filter_and_sort`, and the tests module.

Replace with:

```rust
// ---------------------------------------------------------------------------
// Fuzzy search UI (Ctrl+R)
// ---------------------------------------------------------------------------

use std::io;

use super::history::History;
use super::selector::{ItemStyle, SelectorOptions, SelectorUI, colors_enabled};
use super::terminal::Terminal;

/// Ctrl+R history search. Thin wrapper over the shared [`SelectorUI`].
pub struct FuzzySearchUI;

impl FuzzySearchUI {
    pub fn run<T: Terminal>(history: &History, term: &mut T) -> io::Result<Option<String>> {
        // Newest first: SelectorUI treats index 0 as the best candidate, and
        // filter_and_sort's stable sort keeps this order for equal scores.
        let mut entries: Vec<String> = history.entries().to_vec();
        entries.reverse();
        SelectorUI::run(
            &entries,
            SelectorOptions {
                item_style: ItemStyle::Plain,
                colors: colors_enabled(),
            },
            term,
        )
    }
}
```

Remove the now-unused `use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};` import.

- [ ] **Step 2: Run tests**

Run: `cargo test --lib interactive && cargo test --test interactive`
Expected: PASS. The pre-existing history-UI tests in `tests/interactive.rs` (`test_mock_fuzzy_search_direct_select`, `_cancel`, `_empty_history`, `test_mock_ctrl_r_with_ctrl_g_cancel`, `test_fuzzy_search_arrow_keys_no_cursor_drift`, `test_fuzzy_search_select_no_cursor_drift`) all still pass — the selector reproduces the same geometry (`move_up(max_visible + 1)` after draw) and selection semantics.

- [ ] **Step 3: Commit**

```bash
cargo fmt
git add src/interactive/fuzzy_search.rs
git commit -m "refactor(interactive): FuzzySearchUI delegates to shared SelectorUI"
```

---

### Task 7: Update PTY test expectations

**Files:**
- Modify: `tests/pty_interactive.rs` (test `test_pty_ctrl_r_history_search`, lines 213–224)

**Interfaces:**
- Consumes: the new query-line rendering — colored output puts ANSI sequences between the count and the prompt glyph, so the literal `"2/2 > "` no longer appears.

NOTE: `tests/pty_interactive.rs` carries unrelated uncommitted modifications in the working tree (POSIX byte-semantics work per recent commits). Do NOT revert or alter them. Run `git diff tests/pty_interactive.rs` first to see where they are; when committing, use `git add -p tests/pty_interactive.rs` and select ONLY the two expect-line hunks below.

- [ ] **Step 1: Update the expectations**

Line 216 — only the ` > ` suffix is dropped (color escapes and `❯` now follow the count):
```rust
    s.expect("2/2").expect("Ctrl+R search UI did not appear");
```

Line 223–224 — the old code printed the filtered count twice (`1/1`); with the filtered/total fix, 1 match out of 2 history entries renders `1/2`:
```rust
    s.expect("1/2")
        .expect("search query did not filter to unique match");
```

- [ ] **Step 2: Run the PTY suite**

Run: `cargo build && cargo test --test pty_interactive -- --test-threads=1` (background, generous timeout — PTY tests are timing-sensitive)
Expected: PASS, in particular `test_pty_ctrl_r_history_search`, `test_pty_tab_completion`, `test_pty_command_completion*`, `test_pty_path_completion_in_argument_position`.

If `test_pty_ctrl_r_history_search` still fails on the count expectation, read the raw PTY output in the failure message and adjust the expected `filtered/total` string to what the (now correct) counter actually prints — the filtered count is 1 either way.

- [ ] **Step 3: Commit**

```bash
git add -p tests/pty_interactive.rs   # select only the expect-line hunks
git commit -m "test(pty): adapt Ctrl+R expectations to new selector query line"
```

---

### Task 8: Docs touch-up and full verification

**Files:**
- Modify: `docs/superpowers/specs/2026-07-03-selector-ui-modernization-design.md` (pointer glyph note)
- Modify: `TODO.md` (only if follow-ups were discovered)

- [ ] **Step 1: Record the glyph decision in the spec**

In the spec's Visual Design section, replace the two `▶` occurrences with `❯` and add one sentence: "Pointer uses `❯` (U+276F) instead of `▶` (U+25B6) because the latter is East-Asian-Ambiguous width and misaligns on CJK terminals."

- [ ] **Step 2: Full test suite + lints**

Run (in background, generous timeout):
```bash
cargo test 2>&1 | tail -30
cargo clippy --all-targets 2>&1 | tail -20
cargo fmt --check
```
Expected: all tests pass, no clippy warnings in the touched files, fmt clean. (Pre-existing failures unrelated to this work — e.g. the known cli help color test — are out of scope; report them but do not fix.)

- [ ] **Step 3: Manual smoke test (optional but recommended)**

```bash
cargo build
# In a real terminal: ./target/debug/yosh, then:
#   - type `ls src/` + Tab Tab → colored candidate list, dirs blue, ❯ pointer
#   - Tab cycles, Shift-Tab cycles back, Ctrl+U clears query, Esc closes
#   - Ctrl+R → history list newest-first, typing highlights matched chars
#   - NO_COLOR=1 ./target/debug/yosh → legacy reverse-video look
```
(A human at a terminal is needed for full visual confirmation; note this in the final report.)

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/specs/2026-07-03-selector-ui-modernization-design.md TODO.md
git commit -m "docs: record selector pointer glyph decision"
```
(Skip TODO.md from the add list if unchanged.)
