/// Match a POSIX shell glob pattern against `string`.
///
/// Supported metacharacters:
///   `*`      — matches any string (including empty)
///   `?`      — matches any single character
///   `[…]`   — bracket expression: set, range, or negated (`[!…]`)
///   `\x`     — escaped literal `x`
///   everything else — literal match
pub fn matches(pattern: &str, string: &str) -> bool {
    match_pat(pattern, string)
}

fn match_pat(pat: &str, s: &str) -> bool {
    let mut pat_chars = pat.chars();
    match pat_chars.next() {
        None => s.is_empty(),

        Some('*') => {
            // Try matching `rest` against every suffix of s — s itself is the
            // first candidate, the empty string the last; once rem is exhausted
            // (chars().next() == None) we have tried them all and give up.
            let rest = pat_chars.as_str();
            let mut rem = s;
            loop {
                if match_pat(rest, rem) {
                    return true;
                }
                match rem.chars().next() {
                    Some(c) => rem = &rem[c.len_utf8()..],
                    None => return false,
                }
            }
        }

        Some('?') => match s.chars().next() {
            Some(c) => match_pat(pat_chars.as_str(), &s[c.len_utf8()..]),
            None => false,
        },

        Some('[') => {
            let after_bracket = pat_chars.as_str();
            let s_first = s.chars().next();
            if let Some((consumed, matched_char)) = parse_bracket(after_bracket, s_first) {
                // Bracket expressions always match exactly one character.
                match s_first {
                    Some(c) if matched_char => {
                        match_pat(&after_bracket[consumed..], &s[c.len_utf8()..])
                    }
                    _ => false,
                }
            } else {
                // Malformed bracket — treat '[' as a literal.
                match s.chars().next() {
                    Some('[') => match_pat(after_bracket, &s['['.len_utf8()..]),
                    _ => false,
                }
            }
        }

        Some('\\') => {
            let after_bs = pat_chars.as_str();
            match after_bs.chars().next() {
                // '\x' matches literal 'x'.
                Some(pc) => match s.chars().next() {
                    Some(sc) if sc == pc => {
                        match_pat(&after_bs[pc.len_utf8()..], &s[sc.len_utf8()..])
                    }
                    _ => false,
                },
                // Trailing backslash — match a literal backslash.
                None => match s.chars().next() {
                    Some('\\') => match_pat(after_bs, &s['\\'.len_utf8()..]),
                    _ => false,
                },
            }
        }

        Some(c) => match s.chars().next() {
            Some(sc) if sc == c => match_pat(pat_chars.as_str(), &s[sc.len_utf8()..]),
            _ => false,
        },
    }
}

/// Parse a bracket expression starting *after* the opening `[`.
/// Returns `Some((bytes_consumed, did_match))` on success, or `None` if the
/// bracket is malformed (no closing `]`). `bytes_consumed` counts bytes from
/// the start of `pat` (just after the opening `[`) through the closing `]`.
///
/// `bytes_consumed` is a byte length into `pat`, so the caller advances with
/// `&pat[bytes_consumed..]`. `ch` is the character from the string being
/// matched (if any).
fn parse_bracket(pat: &str, ch: Option<char>) -> Option<(usize, bool)> {
    if pat.is_empty() {
        return None;
    }

    let mut rest = pat;
    let negate = rest.starts_with('!');
    if negate {
        rest = &rest['!'.len_utf8()..];
    }

    let mut members: Vec<BracketItem> = Vec::new();
    let mut found_close = false;

    while let Some(c0) = rest.chars().next() {
        // Closing ']' (but not when it would make an empty class — a leading
        // ']' is treated as a literal member).
        if c0 == ']' && !members.is_empty() {
            rest = &rest[c0.len_utf8()..];
            found_close = true;
            break;
        }

        // The remainder of the class body after the current member char.
        let after_c0 = &rest[c0.len_utf8()..];

        // POSIX character class [:class:]
        if c0 == '[' && after_c0.starts_with(':') {
            let after_open = &after_c0[':'.len_utf8()..];
            if let Some((consumed, class)) = try_parse_posix_class(after_open) {
                members.push(BracketItem::Class(class));
                rest = &after_open[consumed..];
                continue;
            }
            // Fall through to literal handling on a malformed class.
        }

        // Range: x-y  (only if '-' is followed by another non-']' char).
        if let Some('-') = after_c0.chars().next() {
            let after_dash = &after_c0['-'.len_utf8()..];
            if let Some(hi) = after_dash.chars().next() {
                if hi != ']' {
                    members.push(BracketItem::Range(c0, hi));
                    rest = &after_dash[hi.len_utf8()..];
                    continue;
                }
            }
        }

        members.push(BracketItem::Char(c0));
        rest = &rest[c0.len_utf8()..];
    }

    if !found_close {
        return None;
    }

    // When `ch` is None (empty subject), no member matches; a negated class
    // would then report `true`, but the caller's `[` arm rejects an empty
    // subject before using this result, so the bracket never matches "".
    let inner_match = ch
        .map(|c| members.iter().any(|m| m.matches(c)))
        .unwrap_or(false);
    let result = if negate { !inner_match } else { inner_match };

    let consumed = pat.len() - rest.len();
    Some((consumed, result))
}

enum BracketItem {
    Char(char),
    Range(char, char),
    Class(PosixClass),
}

#[derive(Copy, Clone)]
enum PosixClass {
    Alpha,
    Upper,
    Lower,
    Digit,
    Alnum,
    Xdigit,
    Space,
    Blank,
    Cntrl,
    Print,
    Graph,
    Punct,
}

impl PosixClass {
    /// LC_CTYPE=C semantics: ASCII-only. Non-C locale values are
    /// currently treated as C per yosh's POSIX-compliance doc
    /// (XBD §7.2 implementation-defined).
    fn matches(self, c: char) -> bool {
        match self {
            PosixClass::Alpha => c.is_ascii_alphabetic(),
            PosixClass::Upper => c.is_ascii_uppercase(),
            PosixClass::Lower => c.is_ascii_lowercase(),
            PosixClass::Digit => c.is_ascii_digit(),
            PosixClass::Alnum => c.is_ascii_alphanumeric(),
            PosixClass::Xdigit => c.is_ascii_hexdigit(),
            PosixClass::Space => matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'),
            PosixClass::Blank => matches!(c, ' ' | '\t'),
            PosixClass::Cntrl => c.is_ascii_control(),
            PosixClass::Print => matches!(c, '\x20'..='\x7e'),
            PosixClass::Graph => matches!(c, '\x21'..='\x7e'),
            PosixClass::Punct => c.is_ascii_punctuation(),
        }
    }
}

const POSIX_CLASSES: &[(&str, PosixClass)] = &[
    ("alpha", PosixClass::Alpha),
    ("upper", PosixClass::Upper),
    ("lower", PosixClass::Lower),
    ("digit", PosixClass::Digit),
    ("alnum", PosixClass::Alnum),
    ("xdigit", PosixClass::Xdigit),
    ("space", PosixClass::Space),
    ("blank", PosixClass::Blank),
    ("cntrl", PosixClass::Cntrl),
    ("print", PosixClass::Print),
    ("graph", PosixClass::Graph),
    ("punct", PosixClass::Punct),
];

/// Try to parse a `[:class:]` POSIX character-class form.
///
/// `pat` is the slice starting AFTER the opening `[:` (i.e., the first
/// character of the class name). Returns `Some((bytes_consumed, class))`
/// where `bytes_consumed` covers the class name and the trailing `:]` (so the
/// caller advances by that many bytes from the first name character).
fn try_parse_posix_class(pat: &str) -> Option<(usize, PosixClass)> {
    let pos = pat.find(":]")?;
    let name = &pat[..pos];
    for (n, c) in POSIX_CLASSES {
        if name == *n {
            return Some((pos + ":]".len(), *c));
        }
    }
    None
}

impl BracketItem {
    fn matches(&self, c: char) -> bool {
        match self {
            BracketItem::Char(x) => *x == c,
            BracketItem::Range(lo, hi) => {
                // LC_COLLATE=C semantics: byte/codepoint ordering.
                // Non-C locale values are currently treated as C
                // per yosh's POSIX-compliance doc (XBD §7.2
                // implementation-defined).
                let lo = *lo as u32;
                let hi = *hi as u32;
                let c = c as u32;
                c >= lo && c <= hi
            }
            BracketItem::Class(cls) => cls.matches(c),
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

    // ── POSIX character classes ──

    #[test]
    fn class_alpha_matches_letter() {
        assert!(matches("[[:alpha:]]", "a"));
        assert!(matches("[[:alpha:]]", "Z"));
    }

    #[test]
    fn class_alpha_rejects_digit() {
        assert!(!matches("[[:alpha:]]", "5"));
        assert!(!matches("[[:alpha:]]", "_"));
    }

    #[test]
    fn class_upper_matches_only_upper() {
        assert!(matches("[[:upper:]]", "A"));
        assert!(!matches("[[:upper:]]", "a"));
    }

    #[test]
    fn class_lower_matches_only_lower() {
        assert!(matches("[[:lower:]]", "z"));
        assert!(!matches("[[:lower:]]", "Z"));
    }

    #[test]
    fn class_digit() {
        assert!(matches("[[:digit:]]", "0"));
        assert!(matches("[[:digit:]]", "9"));
        assert!(!matches("[[:digit:]]", "a"));
    }

    #[test]
    fn class_alnum() {
        assert!(matches("[[:alnum:]]", "5"));
        assert!(matches("[[:alnum:]]", "a"));
        assert!(!matches("[[:alnum:]]", "_"));
    }

    #[test]
    fn class_xdigit() {
        assert!(matches("[[:xdigit:]]", "0"));
        assert!(matches("[[:xdigit:]]", "f"));
        assert!(matches("[[:xdigit:]]", "F"));
        assert!(!matches("[[:xdigit:]]", "g"));
    }

    #[test]
    fn class_space_matches_whitespace() {
        assert!(matches("[[:space:]]", " "));
        assert!(matches("[[:space:]]", "\t"));
        assert!(!matches("[[:space:]]", "a"));
    }

    #[test]
    fn class_blank_matches_horizontal_only() {
        assert!(matches("[[:blank:]]", " "));
        assert!(matches("[[:blank:]]", "\t"));
        assert!(!matches("[[:blank:]]", "\n"));
    }

    #[test]
    fn class_cntrl_matches_control() {
        assert!(matches("[[:cntrl:]]", "\x01"));
        assert!(matches("[[:cntrl:]]", "\x7f"));
        assert!(!matches("[[:cntrl:]]", "a"));
    }

    #[test]
    fn class_print_includes_space() {
        assert!(matches("[[:print:]]", " "));
        assert!(matches("[[:print:]]", "a"));
        assert!(!matches("[[:print:]]", "\x01"));
    }

    #[test]
    fn class_graph_excludes_space() {
        assert!(matches("[[:graph:]]", "a"));
        assert!(!matches("[[:graph:]]", " "));
        assert!(!matches("[[:graph:]]", "\x01"));
    }

    #[test]
    fn class_punct_is_print_minus_alnum_space() {
        assert!(matches("[[:punct:]]", "."));
        assert!(matches("[[:punct:]]", "_"));
        assert!(!matches("[[:punct:]]", "a"));
        assert!(!matches("[[:punct:]]", "5"));
        assert!(!matches("[[:punct:]]", " "));
    }

    #[test]
    fn class_combined_with_range() {
        // [[:alpha:]0-9] matches letters OR digits
        assert!(matches("[[:alpha:]0-9]", "a"));
        assert!(matches("[[:alpha:]0-9]", "5"));
        assert!(!matches("[[:alpha:]0-9]", "_"));
    }

    #[test]
    fn class_negation_with_outer_bang() {
        // [![:digit:]] matches non-digit
        assert!(matches("[![:digit:]]", "a"));
        assert!(!matches("[![:digit:]]", "5"));
    }

    #[test]
    fn unknown_class_name_falls_through_to_literal_chars() {
        // [[:unknown:]] does not panic. The class name "unknown"
        // is not in POSIX_CLASSES, so `try_parse_posix_class`
        // returns None and the loop falls through to char-by-char
        // handling. The outer bracket then contains literals
        // `[`, `:`, `u`, `n`, `k`, `o`, `w`, `:` and is closed by
        // the second-to-last `]`. The final `]` is a trailing
        // literal char. So the pattern matches 2-char strings
        // whose first char is one of those literals and whose
        // second char is `]`.
        assert!(matches("[[:unknown:]]", "[]"));
        assert!(matches("[[:unknown:]]", "u]"));
        assert!(!matches("[[:unknown:]]", "a]"));
    }

    #[test]
    fn missing_colon_close_does_not_panic() {
        // [[:alpha] (no `:]` inside) — `try_parse_posix_class`
        // scans `alpha]` and never finds `:]`, so it returns
        // None. The outer bracket then eats `[`, `:`, `a`, `l`,
        // `p`, `h`, `a` as literals; the final `]` closes the
        // bracket. Pattern matches single chars from that set.
        assert!(matches("[[:alpha]", "a"));
        assert!(matches("[[:alpha]", "["));
        assert!(!matches("[[:alpha]", "z"));
    }

    // ── Multibyte (UTF-8) boundary tests ──
    // These pass on the &[char] implementation and guard the &str rewrite
    // against splitting a multibyte char at a non-char-boundary byte offset.
    #[test]
    fn multibyte_literal_and_star() {
        assert!(matches("日*", "日本語"));
        assert!(matches("*語", "日本語"));
        assert!(matches("日本語", "日本語"));
        assert!(!matches("日*", "本日"));
    }

    #[test]
    fn multibyte_question() {
        assert!(matches("?", "あ"));
        assert!(!matches("?", "あい"));
        assert!(matches("a?c", "aあc"));
    }

    #[test]
    fn multibyte_bracket_range() {
        // あ=U+3042, か=U+304B, ん=U+3093, ン=U+30F3 (katakana, out of range)
        assert!(matches("[あ-ん]", "か"));
        assert!(!matches("[あ-ん]", "ン"));
        assert!(matches("[0-9]語", "5語"));
    }

    #[test]
    fn multibyte_backslash_trailing() {
        assert!(matches("あ\\", "あ\\"));
        assert!(matches("\\あ", "あ"));
    }
}
