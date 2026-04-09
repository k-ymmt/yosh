/// Match a POSIX shell glob pattern against `string`.
///
/// Supported metacharacters:
///   `*`      — matches any string (including empty)
///   `?`      — matches any single character
///   `[…]`   — bracket expression: set, range, or negated (`[!…]`)
///   `\x`     — escaped literal `x`
///   everything else — literal match
pub fn matches(pattern: &str, string: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = string.chars().collect();
    match_pat(&pat, &s)
}

fn match_pat(pat: &[char], s: &[char]) -> bool {
    match pat.first() {
        None => s.is_empty(),

        Some('*') => {
            // Try matching the rest of the pattern against every suffix of s.
            let rest = &pat[1..];
            for i in 0..=s.len() {
                if match_pat(rest, &s[i..]) {
                    return true;
                }
            }
            false
        }

        Some('?') => {
            !s.is_empty() && match_pat(&pat[1..], &s[1..])
        }

        Some('[') => {
            // Find the closing ']'
            if let Some((consumed, matched_char)) = parse_bracket(&pat[1..], s.first().copied()) {
                // Bracket expressions always match exactly one character
                !s.is_empty() && matched_char && match_pat(&pat[1 + consumed..], &s[1..])
            } else {
                // Malformed bracket — treat '[' as literal
                !s.is_empty() && s[0] == '[' && match_pat(&pat[1..], &s[1..])
            }
        }

        Some('\\') => {
            // '\x' matches literal 'x'
            if pat.len() >= 2 {
                !s.is_empty() && s[0] == pat[1] && match_pat(&pat[2..], &s[1..])
            } else {
                // trailing backslash — match literal backslash
                !s.is_empty() && s[0] == '\\' && match_pat(&pat[1..], &s[1..])
            }
        }

        Some(&c) => {
            !s.is_empty() && s[0] == c && match_pat(&pat[1..], &s[1..])
        }
    }
}

/// Parse a bracket expression starting *after* the opening `[`.
/// Returns `Some((chars_consumed_including_closing_bracket, did_match))` on
/// success, or `None` if the bracket is malformed (no closing `]`).
///
/// `ch` is the character from the string being matched (if any).
fn parse_bracket(pat: &[char], ch: Option<char>) -> Option<(usize, bool)> {
    if pat.is_empty() {
        return None;
    }

    let mut i = 0;
    let negate = pat[0] == '!';
    if negate {
        i += 1;
    }

    // Allow ']' as the first character inside the class (treated as literal)
    let mut members: Vec<BracketItem> = Vec::new();
    let mut found_close = false;

    while i < pat.len() {
        if pat[i] == ']' && !members.is_empty() {
            // Found the closing bracket
            i += 1;
            found_close = true;
            break;
        }
        // Range: x-y  (only if there is a '-' followed by another char before ']')
        if i + 2 < pat.len() && pat[i + 1] == '-' && pat[i + 2] != ']' {
            members.push(BracketItem::Range(pat[i], pat[i + 2]));
            i += 3;
        } else {
            members.push(BracketItem::Char(pat[i]));
            i += 1;
        }
    }

    if !found_close {
        return None;
    }

    let inner_match = ch.map(|c| members.iter().any(|m| m.matches(c))).unwrap_or(false);
    let result = if negate { !inner_match } else { inner_match };

    // Consume includes negate flag + all member chars + closing ']' = i
    // (i already accounts for everything from the char after '[' through ']')
    Some((i, result))
}

enum BracketItem {
    Char(char),
    Range(char, char),
}

impl BracketItem {
    fn matches(&self, c: char) -> bool {
        match self {
            BracketItem::Char(x) => *x == c,
            BracketItem::Range(lo, hi) => {
                let lo = *lo as u32;
                let hi = *hi as u32;
                let c = c as u32;
                c >= lo && c <= hi
            }
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Literal ──
    #[test]
    fn test_literal_match() {
        assert!(matches("hello", "hello"));
    }

    #[test]
    fn test_literal_no_match() {
        assert!(!matches("hello", "world"));
    }

    #[test]
    fn test_empty_pattern_empty_string() {
        assert!(matches("", ""));
    }

    #[test]
    fn test_empty_pattern_nonempty_string() {
        assert!(!matches("", "a"));
    }

    // ── Star ──
    #[test]
    fn test_star_matches_empty() {
        assert!(matches("*", ""));
    }

    #[test]
    fn test_star_matches_any() {
        assert!(matches("*", "anything"));
    }

    #[test]
    fn test_star_prefix() {
        assert!(matches("*.txt", "file.txt"));
        assert!(!matches("*.txt", "file.rs"));
    }

    #[test]
    fn test_star_suffix() {
        assert!(matches("file.*", "file.txt"));
        assert!(matches("file.*", "file.rs"));
        assert!(!matches("file.*", "other.txt"));
    }

    #[test]
    fn test_double_star() {
        assert!(matches("a**b", "ab"));
        assert!(matches("a**b", "axyzb"));
    }

    // ── Question ──
    #[test]
    fn test_question_single_char() {
        assert!(matches("?", "a"));
        assert!(matches("?", "z"));
        assert!(!matches("?", ""));
        assert!(!matches("?", "ab"));
    }

    #[test]
    fn test_question_in_middle() {
        assert!(matches("a?c", "abc"));
        assert!(!matches("a?c", "ac"));
    }

    // ── Bracket ──
    #[test]
    fn test_bracket_set() {
        assert!(matches("[abc]", "a"));
        assert!(matches("[abc]", "b"));
        assert!(matches("[abc]", "c"));
        assert!(!matches("[abc]", "d"));
    }

    #[test]
    fn test_bracket_range() {
        assert!(matches("[a-z]", "a"));
        assert!(matches("[a-z]", "m"));
        assert!(matches("[a-z]", "z"));
        assert!(!matches("[a-z]", "A"));
        assert!(!matches("[a-z]", "0"));
    }

    #[test]
    fn test_bracket_negated() {
        assert!(!matches("[!abc]", "a"));
        assert!(matches("[!abc]", "d"));
    }

    #[test]
    fn test_bracket_negated_range() {
        assert!(!matches("[!a-z]", "m"));
        assert!(matches("[!a-z]", "A"));
        assert!(matches("[!a-z]", "0"));
    }

    // ── Backslash escape ──
    #[test]
    fn test_backslash_literal_star() {
        assert!(matches("\\*", "*"));
        assert!(!matches("\\*", "a"));
    }

    #[test]
    fn test_backslash_literal_char() {
        assert!(matches("\\a", "a"));
        assert!(!matches("\\a", "b"));
    }

    // ── Complex patterns ──
    #[test]
    fn test_complex_pattern() {
        assert!(matches("file[0-9].txt", "file3.txt"));
        assert!(!matches("file[0-9].txt", "fileA.txt"));
    }

    #[test]
    fn test_star_question_combined() {
        assert!(matches("*?", "a"));
        assert!(matches("*?", "ab"));
        assert!(!matches("*?", ""));
    }
}
