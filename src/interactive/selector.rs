//! Shared fuzzy-selector UI.
//!
//! One interactive list-selection component used by both Tab completion
//! (`CompletionUI`) and Ctrl+R history search (`FuzzySearchUI`). Renders an
//! fzf-style list above the prompt: candidates, a separator, and a query
//! line. `items` order is the display order for the empty query; index 0 is
//! the best candidate, drawn at the bottom (nearest the query line).

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::style::Color;
use std::io;
use unicode_width::UnicodeWidthChar;

use super::display_width::display_width;
use super::fuzzy_search::{ScoredCandidate, filter_and_sort};
use super::terminal::Terminal;

/// Background of the selected row (256-color light navy blue).
const SELECTED_BG: Color = Color::AnsiValue(25);
/// Fuzzy-matched character highlight (256-color amber).
const MATCH_FG: Color = Color::AnsiValue(214);

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
        let (char_count, used_cols, truncated) = fit_to_width(&cand.text, budget);

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
        // chars in amber, directories in blue.
        if is_selected {
            term.set_bg_color(SELECTED_BG)?;
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
                term.set_fg_color(MATCH_FG)?;
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
        Ok(())
    }

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
            self.output
                .push(if on { "[REV]" } else { "[/REV]" }.to_string());
            Ok(())
        }
        fn set_dim(&mut self, on: bool) -> io::Result<()> {
            self.output
                .push(if on { "[DIM]" } else { "[/DIM]" }.to_string());
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
            self.output
                .push(if on { "[BOLD]" } else { "[/BOLD]" }.to_string());
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

    fn color_opts(style: ItemStyle) -> SelectorOptions {
        SelectorOptions {
            item_style: style,
            colors: true,
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
            MockTerm::key(KeyCode::PageUp),   // → 9
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

    #[test]
    fn test_cjk_candidate_truncated_with_ellipsis() {
        // width 12 → row budget 10; "日本語のファイル名.rs" is wider, so the
        // rendered row must contain "…".
        let mut term = MockTerm::with_size(vec![MockTerm::key(KeyCode::Esc)], 12, 24);
        let _ =
            SelectorUI::run(&items(&["日本語のファイル名.rs"]), plain_opts(), &mut term).unwrap();
        assert!(term.dump().contains('…'), "output: {}", term.dump());
    }

    // ── colored rendering ───────────────────────────────────────────

    #[test]
    fn test_colors_selected_row_has_bg_and_pointer() {
        let mut term = MockTerm::new(vec![MockTerm::key(KeyCode::Esc)]);
        let _ =
            SelectorUI::run(&items(&["a", "b"]), color_opts(ItemStyle::Plain), &mut term).unwrap();
        let out = term.dump();
        assert!(out.contains("[BG:AnsiValue(25)]"), "output: {}", out);
        assert!(out.contains("❯ "), "output: {}", out);
        assert!(
            !out.contains("[REV]"),
            "reverse video must not be used: {}",
            out
        );
    }

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
        assert!(
            !term.dump().contains("[FG:Blue]"),
            "output: {}",
            term.dump()
        );
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
}
