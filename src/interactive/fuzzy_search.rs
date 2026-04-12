/// Result of a fuzzy match: the score and matched character positions.
#[derive(Debug)]
pub struct FuzzyMatch {
    pub score: i64,
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
        return Some(FuzzyMatch { score: 0, positions: vec![] });
    }

    let query_chars: Vec<char> = query.chars().collect();
    let target_chars: Vec<char> = target.chars().collect();

    // First pass: verify all query chars exist in order (case-insensitive)
    let mut qi = 0;
    for &tc in &target_chars {
        if qi < query_chars.len()
            && tc.to_ascii_lowercase() == query_chars[qi].to_ascii_lowercase()
        {
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
        if tc.to_ascii_lowercase() == query_chars[qi].to_ascii_lowercase() {
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
        .filter_map(|entry| {
            fuzzy_match(query, entry).map(|m| (m.score, entry.clone()))
        })
        .collect();
    results.sort_by(|a, b| b.0.cmp(&a.0));
    results
}

// ---------------------------------------------------------------------------
// Fuzzy search UI (Ctrl+R)
// ---------------------------------------------------------------------------

use std::io::{self, Write, Stdout, stdout};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    style::{Attribute, SetAttribute},
    terminal::{self, ClearType},
    ExecutableCommand,
};

use super::history::History;

pub struct FuzzySearchUI {
    query: Vec<char>,
    selected: usize,
    scroll_offset: usize,
    candidates: Vec<(i64, String)>,
    max_visible: usize,
}

impl FuzzySearchUI {
    pub fn run(history: &History) -> io::Result<Option<String>> {
        let entries = history.entries();
        if entries.is_empty() {
            return Ok(None);
        }

        let (_, term_height) = terminal::size()?;
        let max_visible = ((term_height as f32) * 0.4).max(3.0) as usize;

        let mut ui = FuzzySearchUI {
            query: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            candidates: entries.iter().cloned().map(|e| (0, e)).collect(),
            max_visible,
        };
        ui.candidates.reverse(); // newest first

        let mut stdout = stdout();
        let draw_lines = ui.max_visible + 2; // candidates + separator + query
        for _ in 0..draw_lines {
            write!(stdout, "\r\n")?;
        }
        stdout.execute(cursor::MoveUp(draw_lines as u16))?;
        ui.draw(&mut stdout)?;

        loop {
            stdout.flush()?;
            if let Event::Key(key_event) = event::read()? {
                match ui.handle_key(key_event, entries) {
                    SearchAction::Continue => {}
                    SearchAction::Select(line) => {
                        ui.clear_ui(&mut stdout, draw_lines)?;
                        return Ok(Some(line));
                    }
                    SearchAction::Cancel => {
                        ui.clear_ui(&mut stdout, draw_lines)?;
                        return Ok(None);
                    }
                }
                ui.draw(&mut stdout)?;
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

    fn draw(&self, stdout: &mut Stdout) -> io::Result<()> {
        let (term_width, _) = terminal::size()?;
        let width = term_width as usize;

        stdout.execute(cursor::MoveToColumn(0))?;

        let visible_end = (self.scroll_offset + self.max_visible).min(self.candidates.len());
        let visible_range = self.scroll_offset..visible_end;
        let visible_count = visible_range.len();

        // Fill empty lines if fewer candidates than max_visible
        for _ in 0..(self.max_visible - visible_count) {
            stdout.execute(terminal::Clear(ClearType::CurrentLine))?;
            write!(stdout, "\r\n")?;
        }

        // Draw candidates in reverse order (highest index = top of UI)
        for i in (visible_range).rev() {
            stdout.execute(terminal::Clear(ClearType::CurrentLine))?;
            let (_score, ref line) = self.candidates[i];
            let display: String = line.chars().take(width.saturating_sub(2)).collect();
            if i == self.selected {
                stdout.execute(SetAttribute(Attribute::Reverse))?;
                write!(stdout, "> {}", display)?;
                stdout.execute(SetAttribute(Attribute::Reset))?;
            } else {
                write!(stdout, "  {}", display)?;
            }
            write!(stdout, "\r\n")?;
        }

        // Separator
        stdout.execute(terminal::Clear(ClearType::CurrentLine))?;
        let sep: String = "\u{2500}".repeat(width.min(40));
        write!(stdout, "  {}\r\n", sep)?;

        // Query line
        stdout.execute(terminal::Clear(ClearType::CurrentLine))?;
        let query_str: String = self.query.iter().collect();
        let total = self.candidates.len();
        write!(stdout, "  {}/{} > {}", total, total, query_str)?;

        // Move back to top
        let total_lines = self.max_visible + 2;
        stdout.execute(cursor::MoveUp(total_lines as u16))?;
        stdout.flush()?;
        Ok(())
    }

    fn clear_ui(&self, stdout: &mut Stdout, draw_lines: usize) -> io::Result<()> {
        stdout.execute(cursor::MoveToColumn(0))?;
        for _ in 0..draw_lines {
            stdout.execute(terminal::Clear(ClearType::CurrentLine))?;
            write!(stdout, "\r\n")?;
        }
        stdout.execute(cursor::MoveUp(draw_lines as u16))?;
        stdout.flush()?;
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
