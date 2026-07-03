# Selector Colors (Navy/Amber) + Full-Width Selected Row Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Change the selector UI's accent colors (selected-row background → navy, matched chars → amber, both as tunable constants) and extend the selected row's background to the full terminal width.

**Architecture:** Both changes are confined to `src/interactive/selector.rs`. Colors become module constants (`SELECTED_BG`, `MATCH_FG`) substituted at the two existing call sites. Full-width comes from `fit_to_width` additionally returning the consumed display columns, letting `draw_row` pad the selected row with background-colored spaces to exactly the terminal width.

**Tech Stack:** Rust, crossterm `Color::AnsiValue` (256-color), existing MockTerm test harness in selector.rs.

**Spec:** `docs/superpowers/specs/2026-07-03-selector-ui-modernization-design.md` (Visual Design section, 2026-07-03 revision bullets).

## Global Constraints

- The working tree contains unrelated in-progress changes (`src/builtin/special.rs`, `src/env/locale.rs`, `src/exec/simple.rs`, `src/lexer/*`, `tests/parser_integration.rs`, `tests/pty_*.rs`). **Stage only `src/interactive/selector.rs`** — never `git add -A` or `git add .`.
- Exact color values from the spec: `SELECTED_BG = Color::AnsiValue(18)` (navy), `MATCH_FG = Color::AnsiValue(214)` (amber).
- Pointer `❯` and prompt `❯` stay cyan; count stays yellow; directories stay blue; separator stays dim; the `NO_COLOR` legacy look (reverse video, text-width only) is unchanged.
- Only the **selected** row is padded to full width, and only in the colored path.
- Run `cargo fmt` before every commit. Do NOT use `cargo build --workspace` / `cargo test --workspace`.
- All commits end with the trailer block:
  ```
  Original request: change selector highlight colors and extend the selected-row highlight to full terminal width

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01P49L6y4p4AqxCmES9xLNsc
  ```

---

### Task 1: Color constants (navy background, amber match highlight)

**Files:**
- Modify: `src/interactive/selector.rs` (imports block ~line 16, `draw_row` lines 338–356, tests lines 833–858)

**Interfaces:**
- Produces: module-private constants `SELECTED_BG: Color` and `MATCH_FG: Color` — Task 2 does not depend on them, but future color tuning happens only here.

- [ ] **Step 1: Update the two color tests to the new expected markers (failing first)**

In the tests module of `src/interactive/selector.rs`:

In `test_colors_selected_row_has_bg_and_pointer` (line 839), change:

```rust
        assert!(out.contains("[BG:DarkGrey]"), "output: {}", out);
```

to:

```rust
        assert!(out.contains("[BG:AnsiValue(18)]"), "output: {}", out);
```

Rename `test_colors_matched_chars_cyan` (lines 848–858) to `test_colors_matched_chars_amber` and change its body's assertion and comment:

```rust
    #[test]
    fn test_colors_matched_chars_amber() {
        let mut events = MockTerm::chars("b");
        events.push(MockTerm::key(KeyCode::Esc));
        let mut term = MockTerm::new(events);
        let _ = SelectorUI::run(&items(&["abc"]), color_opts(ItemStyle::Plain), &mut term).unwrap();
        // After typing "b", the row for "abc" must switch to the amber match
        // color right before writing the matched char 'b'.
        let out = term.dump();
        assert!(out.contains("[FG:AnsiValue(214)]b"), "output: {}", out);
    }
```

(The MockTerm records `set_bg_color`/`set_fg_color` as `format!("[BG:{:?}]", color)` / `[FG:{:?}]`, and `{:?}` for `Color::AnsiValue(18)` renders as `AnsiValue(18)`.)

- [ ] **Step 2: Run tests to verify the two fail**

Run: `cargo test --lib selector 2>&1 | tail -15`
Expected: FAIL — `test_colors_selected_row_has_bg_and_pointer` (no `[BG:AnsiValue(18)]` yet) and `test_colors_matched_chars_amber` (no `[FG:AnsiValue(214)]` yet). All other selector tests PASS.

- [ ] **Step 3: Add the constants and substitute the call sites**

After the `use` block (below line 16, above the `ItemStyle` doc comment), add:

```rust
/// Background of the selected row (256-color navy).
const SELECTED_BG: Color = Color::AnsiValue(18);
/// Fuzzy-matched character highlight (256-color amber).
const MATCH_FG: Color = Color::AnsiValue(214);
```

In `draw_row`, update the section comment (lines 338–339):

```rust
        // fzf-style: pointer + background on the selected row, matched query
        // chars in amber, directories in blue.
```

Line 341: `term.set_bg_color(Color::DarkGrey)?;` → `term.set_bg_color(SELECTED_BG)?;`

Line 356 (matched-char branch): `term.set_fg_color(Color::Cyan)?;` → `term.set_fg_color(MATCH_FG)?;`

Do NOT touch line 343 (`Color::Cyan` for the `❯` pointer) or `draw_query_line`'s cyan/yellow.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib selector`
Expected: PASS (all selector tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add src/interactive/selector.rs
git commit -m "feat(interactive): navy/amber accent colors for selector"
```

(Include the trailer block from Global Constraints.)

---

### Task 2: Full-width selected-row background

**Files:**
- Modify: `src/interactive/selector.rs` (`draw_row` lines 309–375, `fit_to_width` lines 436–457, its 3 unit tests lines 804–819, new rendering tests in the colored-rendering section)

**Interfaces:**
- Consumes: nothing from Task 1 (independent change; only merge-conflict-adjacent in `draw_row`).
- Produces: `fn fit_to_width(s: &str, budget: usize) -> (usize, usize, bool)` — `(char_count, used_cols, truncated)` where `used_cols` is the display width of the first `char_count` chars (excluding the `…` marker).

- [ ] **Step 1: Write the failing tests**

Update the three existing `fit_to_width` unit tests (lines 804–819) to the 3-tuple:

```rust
    #[test]
    fn test_fit_to_width_ascii_fits() {
        assert_eq!(fit_to_width("hello", 10), (5, 5, false));
    }

    #[test]
    fn test_fit_to_width_ascii_truncates() {
        // budget 5 → 4 chars (4 cols) + ellipsis
        assert_eq!(fit_to_width("hello!", 5), (4, 4, true));
    }

    #[test]
    fn test_fit_to_width_cjk() {
        // "日本語" = 6 columns; budget 5 → 2 chars = 4 cols + ellipsis
        assert_eq!(fit_to_width("日本語", 5), (2, 4, true));
        assert_eq!(fit_to_width("日本語", 6), (3, 6, false));
    }
```

Add these tests at the end of the colored-rendering section (after `test_plain_count_shows_filtered_and_total`):

```rust
    // ── full-width selected row ─────────────────────────────────────

    #[test]
    fn test_colors_selected_row_padded_to_full_width() {
        // width 20 → text budget 18; "abc" uses 3 cols → 15 padding spaces
        // before the style reset, so the navy background reaches the right
        // edge of the terminal.
        let mut term = MockTerm::with_size(vec![MockTerm::key(KeyCode::Esc)], 20, 24);
        let _ = SelectorUI::run(&items(&["abc"]), color_opts(ItemStyle::Plain), &mut term).unwrap();
        let out = term.dump();
        assert!(
            out.contains(&format!("abc{}[RESET]", " ".repeat(15))),
            "output: {}",
            out
        );
    }

    #[test]
    fn test_colors_unselected_row_not_padded() {
        // Only the selected row carries a background, so only it is padded.
        // "abc" is selected (index 0); "xy" is unselected and must be
        // followed immediately by the style reset.
        let mut term = MockTerm::with_size(vec![MockTerm::key(KeyCode::Esc)], 20, 24);
        let _ = SelectorUI::run(
            &items(&["abc", "xy"]),
            color_opts(ItemStyle::Plain),
            &mut term,
        )
        .unwrap();
        assert!(term.dump().contains("xy[RESET]"), "output: {}", term.dump());
    }

    #[test]
    fn test_colors_truncated_selected_row_padding_accounts_for_ellipsis() {
        // width 10 → budget 8; "abcdefghij" (10 cols) truncates to 7 chars,
        // and '…' brings the drawn width to exactly 8 → zero padding.
        let mut term = MockTerm::with_size(vec![MockTerm::key(KeyCode::Esc)], 10, 24);
        let _ = SelectorUI::run(
            &items(&["abcdefghij"]),
            color_opts(ItemStyle::Plain),
            &mut term,
        )
        .unwrap();
        assert!(term.dump().contains("…[RESET]"), "output: {}", term.dump());
    }

    #[test]
    fn test_colors_cjk_truncated_selected_row_padded_remainder() {
        // width 12 → budget 10; "日本語のファイル" (16 cols) truncates to
        // 4 chars = 8 cols (a 5th would exceed limit 9); '…' makes 9 drawn
        // cols → exactly 1 padding space closes the 10-col budget.
        let mut term = MockTerm::with_size(vec![MockTerm::key(KeyCode::Esc)], 12, 24);
        let _ = SelectorUI::run(
            &items(&["日本語のファイル"]),
            color_opts(ItemStyle::Plain),
            &mut term,
        )
        .unwrap();
        assert!(term.dump().contains("… [RESET]"), "output: {}", term.dump());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib selector 2>&1 | tail -20`
Expected: COMPILE FAIL first (the three `fit_to_width` tests destructure a 3-tuple against the current 2-tuple return). That counts as the RED step for the signature change; the rendering tests go RED once it compiles.

- [ ] **Step 3: Implement**

Replace `fit_to_width` (lines 436–457) with:

```rust
/// How many leading chars of `s` fit in `budget` display columns.
///
/// Returns `(char_count, used_cols, truncated)`. When the whole string
/// fits, count is `s.chars().count()`, `used_cols` its full display width,
/// and truncated is false. Otherwise the counted chars fit in `budget - 1`
/// columns, leaving one column for the '…' marker; `used_cols` excludes
/// that marker.
fn fit_to_width(s: &str, budget: usize) -> (usize, usize, bool) {
    let total = display_width(s);
    if total <= budget {
        return (s.chars().count(), total, false);
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
    (count, used, true)
}
```

In `draw_row`, update the destructuring (line 317):

```rust
        let (char_count, used_cols, truncated) = fit_to_width(&cand.text, budget);
```

In the colored path, insert the padding between the ellipsis write and `reset_style()` (currently lines 369–373):

```rust
        if truncated {
            term.write_char('…')?;
        }
        if is_selected {
            // Extend the background to the full terminal width: pad the
            // remaining columns with spaces while the row background is
            // still active. Writing through the last column is safe under
            // deferred auto-wrap.
            let drawn = used_cols + if truncated { 1 } else { 0 };
            let pad = budget.saturating_sub(drawn);
            if pad > 0 {
                term.write_str(&" ".repeat(pad))?;
            }
        }
        // Reset clears fg, bg, and bold together (Attribute::Reset).
        term.reset_style()?;
```

The legacy (`!self.opts.colors`) path is untouched — it ignores `used_cols`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib selector`
Expected: PASS (all selector tests, including the 4 new ones).

- [ ] **Step 5: Regression check**

Run: `cargo test --lib interactive && cargo test --test interactive`
Expected: PASS (geometry is unchanged — padding adds no lines; the no-drift tests must stay green).

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add src/interactive/selector.rs
git commit -m "feat(interactive): extend selected-row background to full width"
```

---

### Task 3: Verification sweep

**Files:**
- None new (verification only; no doc changes — the spec was already updated and committed at a844c11).

- [ ] **Step 1: Lints and format**

Run: `cargo clippy --all-targets 2>&1 | tail -10` and `cargo fmt --check`
Expected: no new warnings (one pre-existing `collapsible_if` at `src/expand/pattern.rs:133` is known and out of scope); fmt clean.

- [ ] **Step 2: PTY suite (rendering changed — the query line did not, but confirm)**

Run: `cargo build && cargo test --test pty_interactive -- --test-threads=1` (generous timeout; timing-sensitive)
Expected: PASS — the Ctrl+R expectations assert on `"2/2"` / `"1/2"` count strings, which this change does not alter.

- [ ] **Step 3: Report**

No commit (nothing changed in this task). Report results; a human visual check in a real terminal (navy row reaching the right edge, amber match chars) remains the final confirmation.
