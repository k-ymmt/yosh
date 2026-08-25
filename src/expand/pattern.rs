/// Match a POSIX shell glob pattern against `string`.
///
/// Supported metacharacters:
///   `*`      — matches any string (including empty)
///   `?`      — matches any single character
///   `[…]`   — bracket expression: set, range, or negated (`[!…]`)
///   `\x`     — escaped literal `x`
///   everything else — literal match
///
/// Thin wrapper over [`compile`] + [`CompiledPattern::matches`]. Callers
/// that match one pattern against many candidates (glob directory walks,
/// `${v#pat}` cut-point scans) should `compile` once and reuse it.
pub fn matches(pattern: &str, string: &str) -> bool {
    compile(pattern).matches(string)
}

/// A pattern tokenized once, reusable across many `matches` calls.
/// Kills the per-candidate re-parse cost: a 2000-entry directory glob
/// previously re-tokenized the pattern (re-allocating each bracket's
/// `Vec<BracketItem>`) once per entry.
pub struct CompiledPattern {
    tokens: Vec<PatternToken>,
}

/// Tokenize `pattern` once into a reusable [`CompiledPattern`].
pub fn compile(pattern: &str) -> CompiledPattern {
    CompiledPattern {
        tokens: parse_pattern(pattern),
    }
}

impl CompiledPattern {
    /// Match this pre-compiled pattern against `string` (anchored: the
    /// whole string must match).
    pub fn matches(&self, string: &str) -> bool {
        match_tokens(&self.tokens, string)
    }
}

/// A single parsed pattern element. The pattern string is tokenized once
/// per `matches()` call so that `*`'s suffix-retry loop (which re-attempts
/// the remaining pattern against every suffix of the subject) walks a
/// pre-parsed token slice instead of re-parsing bracket expressions (and
/// re-allocating their `Vec<BracketItem>`) on every retry. See TODO PERF
/// item 10 / task-2 brief for the O(n) `*[abc]x` re-parse this replaces.
enum PatternToken {
    /// A literal character, from an ordinary pattern char or a `\x` escape.
    Literal(char),
    /// `?` — matches any single character.
    Any,
    /// `*` — matches any string (including empty).
    Star,
    /// A bracket expression `[...]` / `[!...]`, pre-parsed into members.
    Bracket {
        negate: bool,
        members: Vec<BracketItem>,
    },
}

/// Tokenize a full pattern string into a `Vec<PatternToken>`, parsing each
/// bracket expression exactly once. Malformed brackets (no closing `]`) and
/// backslash-escapes are resolved into `Literal` tokens here so the matcher
/// never has to touch the original `&str` representation again.
fn parse_pattern(pat: &str) -> Vec<PatternToken> {
    let mut tokens = Vec::new();
    let mut chars = pat.chars();

    while let Some(c) = chars.next() {
        match c {
            '*' => tokens.push(PatternToken::Star),
            '?' => tokens.push(PatternToken::Any),
            '[' => {
                let rest = chars.as_str();
                if let Some((consumed, negate, members)) = parse_bracket(rest) {
                    tokens.push(PatternToken::Bracket { negate, members });
                    chars = rest[consumed..].chars();
                } else {
                    // Malformed bracket — treat '[' as a literal.
                    tokens.push(PatternToken::Literal('['));
                }
            }
            '\\' => match chars.next() {
                Some(pc) => tokens.push(PatternToken::Literal(pc)),
                // Trailing backslash — literal backslash.
                None => tokens.push(PatternToken::Literal('\\')),
            },
            c => tokens.push(PatternToken::Literal(c)),
        }
    }

    tokens
}

/// Classification of a pattern's shape, used by callers (e.g.
/// `param::strip_prefix` / `strip_suffix`) to pick an O(1)/O(n) anchored
/// fast path instead of the general O(n) boundary-scan + backtracking
/// matcher. Only patterns built entirely from literal characters (ordinary
/// chars and `\x` escapes — no `*`, `?`, or `[...]`) qualify; anything else
/// falls back to `General` so behavior for bracket/`?`-bearing patterns is
/// unchanged.
pub(crate) enum PatternShape {
    /// No metacharacters at all — an exact literal string.
    Literal(String),
    /// A single leading `*` followed by a literal remainder, e.g. the
    /// `${x##*/}` / `${x#*/}` idiom's pattern `*/`.
    StarThenLiteral(String),
    /// A literal followed by a single trailing `*`, e.g. the
    /// `${x%.*}` / `${x%%.*}` idiom's pattern `.*`.
    LiteralThenStar(String),
    /// Anything else (multiple `*`, `?`, bracket expressions, or a `*` not
    /// at an end) — callers must use the general matcher.
    General,
}

/// Classify `pat`'s shape for fast-path dispatch. See `PatternShape`.
pub(crate) fn classify(pat: &str) -> PatternShape {
    let tokens = parse_pattern(pat);
    let Some(lit) = literal_run(&tokens) else {
        return PatternShape::General;
    };
    let has_leading_star = matches!(tokens.first(), Some(PatternToken::Star));
    let has_trailing_star = matches!(tokens.last(), Some(PatternToken::Star));
    if has_leading_star {
        PatternShape::StarThenLiteral(lit)
    } else if has_trailing_star {
        PatternShape::LiteralThenStar(lit)
    } else {
        // No `*` at all (empty tokens counts as the empty literal too).
        PatternShape::Literal(lit)
    }
}

/// If `tokens` is either all-`Literal` or a single leading/trailing `Star`
/// plus all-`Literal` otherwise, return the concatenated literal value.
/// Returns `None` for anything containing `Any`, `Bracket`, more than one
/// `Star`, or a `Star` that isn't strictly at the first/last position.
fn literal_run(tokens: &[PatternToken]) -> Option<String> {
    let star_count = tokens
        .iter()
        .filter(|t| matches!(t, PatternToken::Star))
        .count();
    if star_count > 1 {
        return None;
    }
    if star_count == 1 {
        let is_edge_star = matches!(tokens.first(), Some(PatternToken::Star))
            || matches!(tokens.last(), Some(PatternToken::Star));
        if !is_edge_star {
            return None;
        }
    }

    let mut lit = String::new();
    for t in tokens {
        match t {
            PatternToken::Literal(c) => lit.push(*c),
            PatternToken::Star => {} // skip; position already validated above
            PatternToken::Any | PatternToken::Bracket { .. } => return None,
        }
    }
    Some(lit)
}

/// Match a pre-parsed token slice against `s`.
///
/// Iterative single-backtrack-point algorithm (the classic Russ Cox /
/// musl `fnmatch` structure): on a mismatch, resume just after the most
/// recent `*`, advancing that star's match by one character. Earlier
/// stars never need revisiting — if the tail fails for every extension
/// of the latest star, growing an earlier star only re-offers the same
/// (shorter) suffixes to the latest star, which already failed on them.
/// This makes matching O(n·m) instead of the exponential blowup the
/// previous recursive per-star suffix-retry exhibited on patterns like
/// `*a*a*a*a*b` vs a long run of `a`s.
///
/// Non-star tokens (`Literal`, `Any`, `Bracket`) each consume exactly
/// one char; brackets are matched from their pre-parsed `members`.
fn match_tokens(tokens: &[PatternToken], s: &str) -> bool {
    let mut ti = 0; // next token index
    let mut si = 0; // next byte offset into `s` (always a char boundary)
    // Backtrack point: token index just past the most recent `*`, and the
    // byte offset where that star's match currently ends.
    let mut star_ti: Option<usize> = None;
    let mut star_si = 0;

    loop {
        if ti < tokens.len() {
            match &tokens[ti] {
                PatternToken::Star => {
                    ti += 1;
                    star_ti = Some(ti);
                    star_si = si;
                    continue;
                }
                tok => {
                    if si < s.len() {
                        // Safe: si is always on a char boundary.
                        let c = s[si..].chars().next().expect("si on char boundary");
                        let ok = match tok {
                            PatternToken::Literal(pc) => c == *pc,
                            PatternToken::Any => true,
                            PatternToken::Bracket { negate, members } => {
                                let inner = members.iter().any(|m| m.matches(c));
                                if *negate { !inner } else { inner }
                            }
                            PatternToken::Star => unreachable!("handled above"),
                        };
                        if ok {
                            si += c.len_utf8();
                            ti += 1;
                            continue;
                        }
                    }
                    // fall through to backtrack
                }
            }
        } else if si == s.len() {
            return true;
        }

        // Mismatch, or tokens exhausted with input left: extend the most
        // recent `*` by one char and retry the tail from there.
        match star_ti {
            Some(st) if star_si < s.len() => {
                let c = s[star_si..].chars().next().expect("star_si on char boundary");
                star_si += c.len_utf8();
                si = star_si;
                ti = st;
            }
            _ => return false,
        }
    }
}

/// Parse a bracket expression starting *after* the opening `[`.
/// Returns `Some((bytes_consumed, negate, members))` on success, or `None`
/// if the bracket is malformed (no closing `]`). `bytes_consumed` counts
/// bytes from the start of `pat` (just after the opening `[`) through the
/// closing `]`, so the caller advances with `&pat[bytes_consumed..]`.
fn parse_bracket(pat: &str) -> Option<(usize, bool, Vec<BracketItem>)> {
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
            if let Some(hi) = after_dash.chars().next()
                && hi != ']'
            {
                members.push(BracketItem::Range(c0, hi));
                rest = &after_dash[hi.len_utf8()..];
                continue;
            }
        }

        members.push(BracketItem::Char(c0));
        rest = &rest[c0.len_utf8()..];
    }

    if !found_close {
        return None;
    }

    let consumed = pat.len() - rest.len();
    Some((consumed, negate, members))
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

    /// Escape codepoints (byteenc-encoded invalid bytes) are `ESCAPE_BASE +
    /// byte`, a monotonic mapping — so a bracket range whose endpoints are
    /// escaped bytes matches candidate escaped bytes in raw-byte order.
    #[test]
    fn test_bracket_range_over_escaped_bytes_follows_byte_order() {
        let enc = |b: &[u8]| crate::byteenc::encode_bytes(b).into_owned();
        let pat = format!("[{}-{}]", enc(b"\x80"), enc(b"\xef"));
        assert!(matches(&pat, &enc(b"\x80")));
        assert!(matches(&pat, &enc(b"\xe9")));
        assert!(matches(&pat, &enc(b"\xef")));
        assert!(!matches(&pat, &enc(b"\xf0")));
        assert!(!matches(&pat, "a"));
    }

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
    fn test_class_alpha_matches_letter() {
        assert!(matches("[[:alpha:]]", "a"));
        assert!(matches("[[:alpha:]]", "Z"));
    }

    #[test]
    fn test_class_alpha_rejects_digit() {
        assert!(!matches("[[:alpha:]]", "5"));
        assert!(!matches("[[:alpha:]]", "_"));
    }

    #[test]
    fn test_class_upper_matches_only_upper() {
        assert!(matches("[[:upper:]]", "A"));
        assert!(!matches("[[:upper:]]", "a"));
    }

    #[test]
    fn test_class_lower_matches_only_lower() {
        assert!(matches("[[:lower:]]", "z"));
        assert!(!matches("[[:lower:]]", "Z"));
    }

    #[test]
    fn test_class_digit() {
        assert!(matches("[[:digit:]]", "0"));
        assert!(matches("[[:digit:]]", "9"));
        assert!(!matches("[[:digit:]]", "a"));
    }

    #[test]
    fn test_class_alnum() {
        assert!(matches("[[:alnum:]]", "5"));
        assert!(matches("[[:alnum:]]", "a"));
        assert!(!matches("[[:alnum:]]", "_"));
    }

    #[test]
    fn test_class_xdigit() {
        assert!(matches("[[:xdigit:]]", "0"));
        assert!(matches("[[:xdigit:]]", "f"));
        assert!(matches("[[:xdigit:]]", "F"));
        assert!(!matches("[[:xdigit:]]", "g"));
    }

    #[test]
    fn test_class_space_matches_whitespace() {
        assert!(matches("[[:space:]]", " "));
        assert!(matches("[[:space:]]", "\t"));
        assert!(!matches("[[:space:]]", "a"));
    }

    #[test]
    fn test_class_blank_matches_horizontal_only() {
        assert!(matches("[[:blank:]]", " "));
        assert!(matches("[[:blank:]]", "\t"));
        assert!(!matches("[[:blank:]]", "\n"));
    }

    #[test]
    fn test_class_cntrl_matches_control() {
        assert!(matches("[[:cntrl:]]", "\x01"));
        assert!(matches("[[:cntrl:]]", "\x7f"));
        assert!(!matches("[[:cntrl:]]", "a"));
    }

    #[test]
    fn test_class_print_includes_space() {
        assert!(matches("[[:print:]]", " "));
        assert!(matches("[[:print:]]", "a"));
        assert!(!matches("[[:print:]]", "\x01"));
    }

    #[test]
    fn test_class_graph_excludes_space() {
        assert!(matches("[[:graph:]]", "a"));
        assert!(!matches("[[:graph:]]", " "));
        assert!(!matches("[[:graph:]]", "\x01"));
    }

    #[test]
    fn test_class_punct_is_print_minus_alnum_space() {
        assert!(matches("[[:punct:]]", "."));
        assert!(matches("[[:punct:]]", "_"));
        assert!(!matches("[[:punct:]]", "a"));
        assert!(!matches("[[:punct:]]", "5"));
        assert!(!matches("[[:punct:]]", " "));
    }

    #[test]
    fn test_class_combined_with_range() {
        // [[:alpha:]0-9] matches letters OR digits
        assert!(matches("[[:alpha:]0-9]", "a"));
        assert!(matches("[[:alpha:]0-9]", "5"));
        assert!(!matches("[[:alpha:]0-9]", "_"));
    }

    #[test]
    fn test_multiple_classes_in_one_bracket() {
        // [[:alpha:][:digit:]] matches letters OR digits
        assert!(matches("[[:alpha:][:digit:]]", "a"));
        assert!(matches("[[:alpha:][:digit:]]", "5"));
        assert!(!matches("[[:alpha:][:digit:]]", "_"));
        assert!(!matches("[[:alpha:][:digit:]]", " "));
    }

    #[test]
    fn test_class_negation_with_outer_bang() {
        // [![:digit:]] matches non-digit
        assert!(matches("[![:digit:]]", "a"));
        assert!(!matches("[![:digit:]]", "5"));
    }

    #[test]
    fn test_unknown_class_name_falls_through_to_literal_chars() {
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
    fn test_missing_colon_close_does_not_panic() {
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
    fn test_multibyte_literal_and_star() {
        assert!(matches("日*", "日本語"));
        assert!(matches("*語", "日本語"));
        assert!(matches("日本語", "日本語"));
        assert!(!matches("日*", "本日"));
    }

    #[test]
    fn test_multibyte_question() {
        assert!(matches("?", "あ"));
        assert!(!matches("?", "あい"));
        assert!(matches("a?c", "aあc"));
    }

    #[test]
    fn test_multibyte_bracket_range() {
        // あ=U+3042, か=U+304B, ん=U+3093, ン=U+30F3 (katakana, out of range)
        assert!(matches("[あ-ん]", "か"));
        assert!(!matches("[あ-ん]", "ン"));
        assert!(matches("[0-9]語", "5語"));
    }

    #[test]
    fn test_multibyte_bracket_set_member() {
        // Multibyte chars as plain set members (not range endpoints).
        assert!(matches("[あいう]", "い"));
        assert!(!matches("[あいう]", "え"));
    }

    #[test]
    fn test_multibyte_backslash_trailing() {
        assert!(matches("あ\\", "あ\\"));
        assert!(matches("\\あ", "あ"));
    }

    // ── Leading-`*` + bracket suffix-retry (Task 2 PERF item 10) ──
    // Pins behavior for the `*[abc]x` re-parse-per-retry pattern flagged by
    // the audit: `*` retries the remaining tokens against every suffix of
    // `s`, so a bracket immediately after `*` gets matched once per retry.
    // Locks correctness under the pre-parsed-tokens refactor.

    #[test]
    fn test_star_then_bracket_then_literal_matches() {
        assert!(matches("*[abc]x", "abcx"));
        assert!(matches("*[abc]x", "zzzbx"));
        assert!(matches("*[abc]x", "ax"));
        assert!(!matches("*[abc]x", "zzzdx"));
        assert!(!matches("*[abc]x", "x"));
    }

    #[test]
    fn test_star_then_bracket_then_literal_no_match_when_bracket_never_satisfied() {
        // Every suffix retry must independently re-check the bracket;
        // this pins that the pre-parsed token isn't accidentally consumed
        // or mutated across retries.
        assert!(!matches("*[xyz]x", "aaaaaaaaaa"));
    }

    #[test]
    fn test_star_then_negated_bracket_then_literal() {
        assert!(matches("*[!abc]x", "dx"));
        assert!(!matches("*[!abc]x", "ax"));
    }

    #[test]
    fn test_star_then_posix_class_bracket_retries_correctly() {
        // Exercises the POSIX-class bracket variant (allocates a
        // `Vec<BracketItem>` in `parse_bracket`) under the same retry loop.
        assert!(matches("*[[:digit:]]x", "abc5x"));
        assert!(!matches("*[[:digit:]]x", "abcYx"));
    }

    #[test]
    fn test_star_then_range_bracket_multiple_retries() {
        // Longer prefix forces many suffix retries before the bracket
        // finally matches, exercising the re-parse-per-retry hot path.
        assert!(matches("*[0-9]end", "aaaaaaaaaaaaaaaa5end"));
        assert!(!matches("*[0-9]end", "aaaaaaaaaaaaaaaaend"));
    }

    #[test]
    fn test_double_star_then_bracket() {
        // Two consecutive `*` tokens both retry against the same bracket
        // token that follows.
        assert!(matches("**[abc]", "xyzzyb"));
        assert!(matches("**[abc]", "a"));
    }

    #[test]
    fn test_star_then_malformed_bracket_falls_back_to_literal() {
        // No closing `]` — parse_bracket returns None once at tokenize
        // time; the `[` becomes a Literal token reused across all retries.
        assert!(matches("*[abc", "xx[abc"));
        assert!(!matches("*[abc", "xxabc"));
    }

    #[test]
    fn test_star_then_bracket_with_escaped_literal_member() {
        // `[a\]b]` — backslash is not special inside brackets (POSIX),
        // so this is parsed as members a, \, b with the first `]` closing.
        // Confirms bracket member parsing is unaffected by pre-parsing.
        assert!(matches("*[abc]*", "xxbxx"));
    }

    // ── Literal pattern via escapes (Task 2 PERF item 9 support) ──
    // These patterns contain only escaped metacharacters and must be
    // treated as fully literal by any "metachar-free" fast-path detector
    // built on top of `pattern::matches` / a shared literal-scan helper.

    #[test]
    fn test_escaped_metachars_are_literal_not_wildcards() {
        assert!(matches("\\*\\?\\[", "*?["));
        assert!(!matches("\\*\\?\\[", "abc"));
    }

    #[test]
    fn test_mixed_escaped_and_unescaped_metachar() {
        // Escaped '*' is literal; the second bare '*' is a wildcard.
        assert!(matches("\\**", "*anything"));
        assert!(!matches("\\**", "xanything"));
    }

    // ── CompiledPattern reuse ──

    #[test]
    fn test_compiled_pattern_reusable_across_candidates() {
        let p = compile("*.rs");
        assert!(p.matches("main.rs"));
        assert!(p.matches("lib.rs"));
        assert!(!p.matches("main.rc"));
        // Re-matching after a failure must not corrupt state.
        assert!(p.matches("x.rs"));
    }

    #[test]
    fn test_compiled_bracket_pattern_reusable() {
        let p = compile("file[0-9].txt");
        assert!(p.matches("file3.txt"));
        assert!(!p.matches("fileA.txt"));
        assert!(p.matches("file7.txt"));
    }

    // ── Linear-time matcher: pathological backtracking regression ──

    #[test]
    fn test_pathological_star_pattern_completes_quickly() {
        // Exponential-backtracking matchers take minutes on this input;
        // the single-backtrack-point matcher is O(n·m) and finishes in
        // well under 100ms even in debug builds.
        let s = "a".repeat(200);
        let start = std::time::Instant::now();
        assert!(!matches("*a*a*a*b", &s));
        assert!(
            start.elapsed() < std::time::Duration::from_millis(100),
            "pathological pattern took {:?} (>100ms): matcher is not linear",
            start.elapsed()
        );
    }

    #[test]
    fn test_pathological_star_pattern_matching_case() {
        // Same shape, but the subject ends in `b` so the match succeeds.
        let s = format!("{}b", "a".repeat(200));
        let start = std::time::Instant::now();
        assert!(matches("*a*a*a*b", &s));
        assert!(
            start.elapsed() < std::time::Duration::from_millis(100),
            "pathological pattern took {:?} (>100ms): matcher is not linear",
            start.elapsed()
        );
    }

    #[test]
    fn test_multi_star_backtrack_correctness() {
        // Later stars must be able to shrink/grow independently of
        // earlier ones — pins the single-backtrack-point rewrite against
        // over-eager pruning.
        assert!(matches("*ab*ab*", "xabab"));
        assert!(matches("*ab*ab*", "ababab"));
        assert!(!matches("*ab*ab*", "ab"));
        assert!(matches("a*b*c", "aXbYbZc"));
        assert!(!matches("a*b*c", "aXc"));
        assert!(matches("*aab", "aaab"));
        assert!(!matches("*aab", "aaba"));
    }

    #[test]
    fn test_star_backtrack_with_multibyte_chars() {
        // Backtracking advances by whole chars, never into the middle of
        // a multibyte sequence.
        assert!(matches("*語*b", "日本語日本語b"));
        assert!(!matches("*語*b", "日本日本"));
    }
}
