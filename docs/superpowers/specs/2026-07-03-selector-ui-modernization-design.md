# Selector UI Modernization — Design

**Date:** 2026-07-03
**Status:** Approved

## Goal

Modernize the interactive-mode selection UIs — Tab completion candidate
selection (`CompletionUI`) and Ctrl+R history search (`FuzzySearchUI`) — by
unifying them into one shared selector component with an fzf-style look and
richer key bindings.

## Background / Current State

Two near-identical fzf-style UIs exist:

- `CompletionUI` (`src/interactive/completion.rs`) — file/command completion
- `FuzzySearchUI` (`src/interactive/fuzzy_search.rs`) — history search

Both render a candidate list above a separator and query line. Known
problems caused or worsened by the duplication:

- No colors at all; selected row is `> ` + reverse video only.
- Fuzzy match positions are computed but unused (`#[allow(dead_code)]`).
- Truncation uses `chars().take()`, ignoring display width (CJK breaks
  alignment); no ellipsis.
- No visual distinction between files and directories.
- Count line prints `{filtered}/{total}` but both values are the filtered
  count (completion.rs bug).
- Ctrl+R next-candidate handling exists only in the history UI (drift).
- Ctrl+C is silently ignored in both UIs (cannot cancel with it).

## Scope

In scope: unification, fzf-style visuals, key-binding enhancements.
Out of scope (explicitly rejected during brainstorming): preview pane,
multi-select, candidate metadata display, scrollbar (counter suffices),
changes to the fuzzy scoring algorithm.

## Architecture

New module `src/interactive/selector.rs` holding the shared UI:

```rust
pub enum ItemStyle {
    Plain, // history entries
    Path,  // completion: trailing '/' means directory, drawn blue
}

pub struct SelectorOptions {
    pub item_style: ItemStyle,
}

pub struct SelectorUI { /* query, selected, scroll_offset, candidates, max_visible, total */ }

impl SelectorUI {
    pub fn run<T: Terminal>(
        items: &[String],
        opts: SelectorOptions,
        term: &mut T,
    ) -> io::Result<Option<String>>;
}
```

- `CompletionUI::run(candidates, term)` and `FuzzySearchUI::run(history,
  term)` keep their existing signatures and become thin wrappers that call
  `SelectorUI::run`. The caller (`line_editor.rs`) is unchanged.
- The history wrapper pre-reverses entries so newest-first ordering holds
  for the empty query; `filter_and_sort` uses a stable sort so score ties
  also stay newest-first.
- `fuzzy_match` / `filter_and_sort` stay in `fuzzy_search.rs`.
  `filter_and_sort` return type changes from `Vec<(i64, String)>` to
  `Vec<ScoredCandidate>`:

```rust
pub struct ScoredCandidate {
    pub score: i64,
    pub text: String,
    pub positions: Vec<usize>, // char indices of matched query chars
}
```

  This removes the `dead_code` allowance on `positions` and feeds the
  match-highlight rendering.

## Visual Design

fzf-style, using the same ANSI 16-color palette as the existing syntax
highlighter (`highlight.rs`). Layout (list drawn bottom-up, best candidate
nearest the query line, matching current behavior):

```
  src/lexer/mod.rs
  src/lexer/word.rs
❯ src/lexer/scanner.rs      <- selected: cyan pointer + DarkGrey background + bold
  src/builtin/              <- directories blue (ItemStyle::Path only)
  ──────────────────        <- separator: dim
  4/17 ❯ lex█               <- count yellow (filtered/total, bug fixed); prompt ❯ cyan
```

- Pointer uses `❯` (U+276F) instead of `▶` (U+25B6) because the latter is
  East-Asian-Ambiguous width and misaligns on CJK terminals.
- Fuzzy-matched characters: cyan + bold, in both selected and unselected
  rows (rendered from `ScoredCandidate::positions`).
- Truncation: display-width-aware via the existing `display_width.rs`
  helpers; a truncated candidate ends with `…`. CJK-safe.
- `NO_COLOR` respected (same env-var semantics as `should_colorize()` in
  `main.rs`): when colors are disabled, fall back to the current look —
  reverse-video selected row, no colors, plain `>` pointer.
- `Terminal` trait gains `set_bg_color(Color)` backed by crossterm's
  `SetBackgroundColor`; all mock terminals in tests implement it.

## Key Bindings (both UIs, unified)

| Key | Action |
|---|---|
| Enter | accept selected candidate |
| Esc / Ctrl+G / Ctrl+C (new) | cancel |
| Up / Ctrl+P | move up (stops at end, as today) |
| Down / Ctrl+N | move down (stops at end, as today) |
| Tab (new behavior) | next candidate in ranking order (index +1, visually upward since the best candidate sits at the bottom); wraps at end |
| Shift-Tab (new) | previous candidate (index -1, visually downward); wraps at start |
| PageUp / PageDown (new) | move by one visible-page height |
| Home / End (new) | jump to first (best) / last candidate |
| Ctrl+U (new) | clear query |
| Ctrl+R | next candidate (now in both UIs, not just history) |
| Backspace | delete last query char |
| printable char | append to query, re-filter |

Note: Tab previously meant "accept" in `CompletionUI`; it now cycles
(zsh menu-select style). Enter remains the accept key.

## Error Handling / Edge Cases

- Empty candidate list: return `None` without showing the UI (unchanged).
- Very short terminals: minimum 3 visible rows (unchanged).
- Narrow terminals: width-safe truncation with ellipsis.
- Raw-mode enable/disable and cursor hide/show follow the existing
  run/cleanup pattern (restore on all exit paths).

## Testing

- Extend the existing `MockTerm`-based tests against `SelectorUI`:
  Tab/Shift-Tab wrap cycling, PageUp/PageDown, Home/End, Ctrl+U,
  Ctrl+C cancel, CJK truncation with ellipsis, filtered/total count.
- New assertions for `fuzzy_match` position output through
  `filter_and_sort` / `ScoredCandidate`.
- Existing PTY tests (`tests/pty_interactive.rs`) that assert on the old
  output (e.g. `> ` marker) are updated to the new rendering.
- All existing unit/integration tests must keep passing.
