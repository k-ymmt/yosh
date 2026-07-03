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

// ---------------------------------------------------------------------------
// Fuzzy search UI (Ctrl+R)
// ---------------------------------------------------------------------------

use std::io;

use super::history::History;
use super::selector::{ItemStyle, SelectorOptions, SelectorUI, colors_enabled};
use super::terminal::Terminal;

/// Ctrl+R history search. Thin wrapper over the shared [`SelectorUI`].
pub struct FuzzySearchUI;

/// Drop later duplicates, keeping the first occurrence of each entry.
/// Used to collapse identical commands in the history search list; the
/// stored history keeps them (see `HISTCONTROL`), but showing the same
/// command more than once in the selector is just noise.
fn dedup_preserving_order(entries: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    entries
        .into_iter()
        .filter(|e| seen.insert(e.clone()))
        .collect()
}

impl FuzzySearchUI {
    pub fn run<T: Terminal>(history: &History, term: &mut T) -> io::Result<Option<String>> {
        // Newest first: SelectorUI treats index 0 as the best candidate, and
        // filter_and_sort's stable sort keeps this order for equal scores.
        let mut entries: Vec<String> = history.entries().to_vec();
        entries.reverse();
        // Collapse identical commands so each unique entry appears once,
        // keeping the most recent occurrence (first after the reverse).
        let entries = dedup_preserving_order(entries);
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
        assert!(results[0].text.contains("checkout"));
        for w in results.windows(2) {
            assert!(w[0].score >= w[1].score);
        }
    }

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

    #[test]
    fn test_dedup_preserving_order_keeps_first() {
        // Simulates the newest-first list handed to the selector: identical
        // commands collapse to their first (most recent) occurrence, and the
        // relative order of distinct entries is preserved.
        let entries = vec![
            "ls".to_string(),
            "git status".to_string(),
            "ls".to_string(),
            "cd ..".to_string(),
            "git status".to_string(),
            "ls".to_string(),
        ];
        let deduped = dedup_preserving_order(entries);
        assert_eq!(
            deduped,
            vec![
                "ls".to_string(),
                "git status".to_string(),
                "cd ..".to_string(),
            ]
        );
    }

    #[test]
    fn test_dedup_preserving_order_no_duplicates() {
        let entries = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(dedup_preserving_order(entries.clone()), entries);
    }

    #[test]
    fn test_dedup_preserving_order_only_exact_matches() {
        // Commands that merely share a prefix are distinct and both kept.
        let entries = vec!["git log".to_string(), "git log --oneline".to_string()];
        assert_eq!(dedup_preserving_order(entries.clone()), entries);
    }
}
