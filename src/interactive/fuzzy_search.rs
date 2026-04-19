/// Result of a fuzzy match: the score and matched character positions.
#[derive(Debug)]
pub struct FuzzyMatch {
    pub score: i64,
    #[allow(dead_code)]
    pub positions: Vec<usize>,
}

const SCORE_MATCH: i64 = 16;
const SCORE_WORD_BOUNDARY: i64 = 32;
const SCORE_EXACT_CASE: i64 = 4;
const PROXIMITY_MAX: i64 = 4;
const LENGTH_PENALTY: i64 = 5;

/// Perform a fuzzy match of `query` against `target`.
/// Returns `None` if query chars don't appear in order in target.
pub fn fuzzy_match(query: &str, target: &str) -> Option<FuzzyMatch> {
    if query.is_empty() {
        return Some(FuzzyMatch {
            score: 0,
            positions: vec![],
        });
    }

    let query_chars: Vec<char> = query.chars().collect();
    let target_chars: Vec<char> = target.chars().collect();

    // First pass: verify all query chars exist in order (case-insensitive)
    let mut qi = 0;
    for &tc in &target_chars {
        if qi < query_chars.len() && tc.eq_ignore_ascii_case(&query_chars[qi]) {
            qi += 1;
        }
    }
    if qi < query_chars.len() {
        return None;
    }

    // Second pass: greedy scoring
    let mut positions = Vec::with_capacity(query_chars.len());
    let mut score: i64 = 0;
    let mut qi = 0;
    let mut prev_match_idx: Option<usize> = None;

    for (ti, &tc) in target_chars.iter().enumerate() {
        if qi >= query_chars.len() {
            break;
        }
        if tc.eq_ignore_ascii_case(&query_chars[qi]) {
            positions.push(ti);
            score += SCORE_MATCH;

            if tc == query_chars[qi] {
                score += SCORE_EXACT_CASE;
            }

            if let Some(prev) = prev_match_idx {
                let gap = (ti - prev - 1) as i64;
                let proximity = (PROXIMITY_MAX - gap).max(0);
                score += proximity;
            }

            if ti == 0 || matches!(target_chars[ti - 1], ' ' | '/' | '_' | '.') {
                score += SCORE_WORD_BOUNDARY;
            }

            prev_match_idx = Some(ti);
            qi += 1;
        }
    }

    // Length penalty: prefer shorter targets when scores are close
    score -= target_chars.len() as i64 * LENGTH_PENALTY;

    Some(FuzzyMatch { score, positions })
}

/// Filter entries by fuzzy match and return sorted by score descending.
pub fn filter_and_sort(query: &str, entries: &[String]) -> Vec<(i64, String)> {
    let mut results: Vec<(i64, String)> = entries
        .iter()
        .filter_map(|entry| fuzzy_match(query, entry).map(|m| (m.score, entry.clone())))
        .collect();
    results.sort_by(|a, b| b.0.cmp(&a.0));
    results
}

// ---------------------------------------------------------------------------
// Fuzzy search UI (Ctrl+R)
// ---------------------------------------------------------------------------

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::io;

use super::history::History;
use super::terminal::Terminal;

pub struct FuzzySearchUI {
    query: Vec<char>,
    selected: usize,
    scroll_offset: usize,
    candidates: Vec<(i64, String)>,
    max_visible: usize,
}

impl FuzzySearchUI {
    pub fn run<T: Terminal>(history: &History, term: &mut T) -> io::Result<Option<String>> {
        let entries = history.entries();
        if entries.is_empty() {
            return Ok(None);
        }

        let (_, term_height) = term.size()?;
        let max_visible = ((term_height as f32) * 0.4).max(3.0) as usize;

        let mut ui = FuzzySearchUI {
            query: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            candidates: entries.iter().cloned().map(|e| (0, e)).collect(),
            max_visible,
        };
        ui.candidates.reverse(); // newest first

        let draw_lines = ui.max_visible + 2; // candidates + separator + query
        term.hide_cursor()?;
        for _ in 0..draw_lines {
            term.write_str("\r\n")?;
        }
        term.move_up(draw_lines as u16)?;
        ui.draw(term)?;

        // Enable raw mode for character-by-character input in the search UI.
        // The caller (read_line_loop) disabled raw mode before invoking us.
        term.enable_raw_mode()?;
        let result = ui.run_loop(term, entries, draw_lines);
        // Disable raw mode regardless of result so the caller can re-enable.
        let _ = term.disable_raw_mode();
        let _ = term.show_cursor();
        result
    }

    fn run_loop<T: Terminal>(
        &mut self,
        term: &mut T,
        entries: &[String],
        draw_lines: usize,
    ) -> io::Result<Option<String>> {
        loop {
            term.flush()?;
            if let Event::Key(key_event) = term.read_event()? {
                match self.handle_key(key_event, entries) {
                    SearchAction::Continue => {}
                    SearchAction::Select(line) => {
                        self.clear_ui(term, draw_lines)?;
                        return Ok(Some(line));
                    }
                    SearchAction::Cancel => {
                        self.clear_ui(term, draw_lines)?;
                        return Ok(None);
                    }
                }
                self.draw(term)?;
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent, entries: &[String]) -> SearchAction {
        match (key.code, key.modifiers) {
            (KeyCode::Enter, _) => {
                if let Some((_score, line)) = self.candidates.get(self.selected) {
                    SearchAction::Select(line.clone())
                } else {
                    SearchAction::Cancel
                }
            }
            (KeyCode::Esc, _) => SearchAction::Cancel,
            (KeyCode::Char('g'), m) if m.contains(KeyModifiers::CONTROL) => SearchAction::Cancel,
            (KeyCode::Up, _) => {
                if self.selected + 1 < self.candidates.len() {
                    self.selected += 1;
                    self.adjust_scroll();
                }
                SearchAction::Continue
            }
            (KeyCode::Char('p'), m) if m.contains(KeyModifiers::CONTROL) => {
                if self.selected + 1 < self.candidates.len() {
                    self.selected += 1;
                    self.adjust_scroll();
                }
                SearchAction::Continue
            }
            (KeyCode::Char('r'), m) if m.contains(KeyModifiers::CONTROL) => {
                if self.selected + 1 < self.candidates.len() {
                    self.selected += 1;
                    self.adjust_scroll();
                }
                SearchAction::Continue
            }
            (KeyCode::Down, _) => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.adjust_scroll();
                }
                SearchAction::Continue
            }
            (KeyCode::Char('n'), m) if m.contains(KeyModifiers::CONTROL) => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.adjust_scroll();
                }
                SearchAction::Continue
            }
            (KeyCode::Backspace, _) => {
                if !self.query.is_empty() {
                    self.query.pop();
                    self.update_candidates(entries);
                }
                SearchAction::Continue
            }
            (KeyCode::Char(ch), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.query.push(ch);
                self.update_candidates(entries);
                SearchAction::Continue
            }
            _ => SearchAction::Continue,
        }
    }

    fn update_candidates(&mut self, entries: &[String]) {
        let query: String = self.query.iter().collect();
        if query.is_empty() {
            self.candidates = entries.iter().cloned().map(|e| (0, e)).collect();
            self.candidates.reverse();
        } else {
            self.candidates = filter_and_sort(&query, entries);
        }
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
        for i in (visible_range).rev() {
            term.clear_current_line()?;
            let (_score, ref line) = self.candidates[i];
            let display: String = line.chars().take(width.saturating_sub(2)).collect();
            if i == self.selected {
                term.set_reverse(true)?;
                term.write_str(&format!("> {}", display))?;
                term.set_reverse(false)?;
            } else {
                term.write_str(&format!("  {}", display))?;
            }
            term.write_str("\r\n")?;
        }

        // Separator
        term.clear_current_line()?;
        let sep: String = "\u{2500}".repeat(width.min(40));
        term.write_str(&format!("  {}\r\n", sep))?;

        // Query line
        term.clear_current_line()?;
        let query_str: String = self.query.iter().collect();
        let total = self.candidates.len();
        term.write_str(&format!("  {}/{} > {}", total, total, query_str))?;

        // Move back to top of the UI area.
        // We wrote (max_visible + 1) newlines: max_visible for candidate rows
        // (including padding) and 1 for the separator.  The query line has no
        // trailing newline, so the cursor sits on line (max_visible + 1).
        let total_lines = self.max_visible + 1;
        term.move_up(total_lines as u16)?;
        term.flush()?;
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

enum SearchAction {
    Continue,
    Select(String),
    Cancel,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let m = fuzzy_match("ls", "ls").unwrap();
        assert!(m.score > 0);
    }

    #[test]
    fn test_substring_match() {
        let m = fuzzy_match("check", "git checkout").unwrap();
        assert!(m.score > 0);
    }

    #[test]
    fn test_fuzzy_order_preserving() {
        let m = fuzzy_match("gco", "git checkout").unwrap();
        assert!(m.score > 0);
        for w in m.positions.windows(2) {
            assert!(w[0] < w[1]);
        }
    }

    #[test]
    fn test_no_match() {
        assert!(fuzzy_match("xyz", "git checkout").is_none());
    }

    #[test]
    fn test_empty_query_matches_all() {
        let m = fuzzy_match("", "anything").unwrap();
        assert_eq!(m.score, 0);
    }

    #[test]
    fn test_consecutive_bonus() {
        let consecutive = fuzzy_match("che", "checkout").unwrap();
        let spread = fuzzy_match("che", "c-h-e-ckout").unwrap();
        assert!(consecutive.score > spread.score);
    }

    #[test]
    fn test_word_boundary_bonus() {
        let boundary = fuzzy_match("gc", "git checkout").unwrap();
        let inside = fuzzy_match("gc", "xgcdef").unwrap();
        assert!(boundary.score > inside.score);
    }

    #[test]
    fn test_case_sensitive_bonus() {
        let exact = fuzzy_match("Make", "Makefile").unwrap();
        let wrong_case = fuzzy_match("Make", "makefile").unwrap();
        assert!(exact.score > wrong_case.score);
    }

    #[test]
    fn test_filter_and_sort() {
        let entries = vec![
            "git checkout main".to_string(),
            "git commit -m 'fix'".to_string(),
            "ls -la".to_string(),
            "grep pattern file".to_string(),
        ];
        let results = filter_and_sort("gco", &entries);
        assert!(!results.is_empty());
        assert!(results[0].1.contains("checkout"));
        for w in results.windows(2) {
            assert!(w[0].0 >= w[1].0);
        }
    }
}
