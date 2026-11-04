//! Bracket expressions (`[...]`, `[!...]`): parsing into members,
//! POSIX character classes, collating symbols and equivalence classes.

/// Parse a bracket expression starting *after* the opening `[`.
/// Returns `Some((bytes_consumed, negate, members))` on success, or `None`
/// if the bracket is malformed (no closing `]`). `bytes_consumed` counts
/// bytes from the start of `pat` (just after the opening `[`) through the
/// closing `]`, so the caller advances with `&pat[bytes_consumed..]`.
pub(super) fn parse_bracket(pat: &str) -> Option<(usize, bool, Vec<BracketItem>)> {
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
        // ']' is treated as a literal member). Only an UNESCAPED `]`
        // closes: `\]` below is a literal member.
        if c0 == ']' && !members.is_empty() {
            rest = &rest[c0.len_utf8()..];
            found_close = true;
            break;
        }

        // Decode the member char, honoring `\x` escapes. The pattern
        // renderer (`expand_word_to_pattern`) backslash-escapes quoted
        // metacharacters, so `\]` is a quote-protected `]`: a literal
        // member that must NOT close the class (POSIX §2.13.1; bash:
        // `[a\]b]` matches `]`, and `sr[c\]` has no closer so the `[`
        // stays literal). Mirrors the mask-aware closer scan in
        // `pathname::bracket_has_close`.
        let (mut m0, m0_escaped, mut after_m0) = decode_bracket_char(rest, c0);

        // POSIX character class [:class:] — only an unescaped `[` opens one.
        if !m0_escaped && m0 == '[' && after_m0.starts_with(':') {
            let after_open = &after_m0[':'.len_utf8()..];
            if let Some((consumed, class)) = try_parse_posix_class(after_open) {
                members.push(BracketItem::Class(class));
                rest = &after_open[consumed..];
                continue;
            }
            // Fall through to literal handling on a malformed class.
        }

        // Collating symbol `[.c.]` / equivalence class `[=c=]` (XBD
        // §9.3.5). Only single-character collating elements exist in the
        // C/POSIX locale yosh implements, where an equivalence class is
        // just its one character — so both reduce to a plain member
        // (a collating symbol may also be a range endpoint).
        if !m0_escaped
            && m0 == '['
            && let Some((c, after)) = try_parse_collating_element(after_m0)
        {
            if after_m0.starts_with('=') {
                members.push(BracketItem::Char(c));
                rest = after;
                continue;
            }
            m0 = c;
            after_m0 = after;
        }

        // Range: x-y (only if '-' is followed by another member char that
        // is not an unescaped ']'; an escaped `\]` endpoint is allowed).
        if let Some('-') = after_m0.chars().next() {
            let after_dash = &after_m0['-'.len_utf8()..];
            if let Some(h0) = after_dash.chars().next() {
                let (mut hi, mut hi_escaped, mut after_hi) = decode_bracket_char(after_dash, h0);
                if !hi_escaped
                    && hi == '['
                    && after_hi.starts_with('.')
                    && let Some((c, after)) = try_parse_collating_element(after_hi)
                {
                    hi = c;
                    after_hi = after;
                    // `[.].]` names `]` without closing the class, like
                    // `\]` (dash/bash: `[A-[.].]]` matches `M`).
                    hi_escaped = true;
                }
                if hi != ']' || hi_escaped {
                    members.push(BracketItem::Range(m0, hi));
                    rest = after_hi;
                    continue;
                }
            }
        }

        members.push(BracketItem::Char(m0));
        rest = after_m0;
    }

    if !found_close {
        return None;
    }

    let consumed = pat.len() - rest.len();
    Some((consumed, negate, members))
}

/// Decode one bracket-member char at the head of `rest` (whose first
/// char is `c0`): a `\x` escape yields the escaped char with
/// `escaped = true`; a trailing lone `\` is a literal backslash.
/// Returns `(member_char, escaped, remainder_after_member)`.
fn decode_bracket_char(rest: &str, c0: char) -> (char, bool, &str) {
    if c0 == '\\' {
        let after_bs = &rest[c0.len_utf8()..];
        match after_bs.chars().next() {
            Some(e) => (e, true, &after_bs[e.len_utf8()..]),
            None => ('\\', false, after_bs),
        }
    } else {
        (c0, false, &rest[c0.len_utf8()..])
    }
}

pub(super) enum BracketItem {
    Char(char),
    Range(char, char),
    Class(PosixClass),
}

#[derive(Copy, Clone)]
pub(super) enum PosixClass {
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

/// Try to parse a `[.c.]` collating symbol or `[=c=]` equivalence class.
///
/// `pat` is the slice just after the opening `[`, so it starts with the
/// `.` or `=` delimiter. The element must be exactly one character (the
/// only collating elements in the C/POSIX locale); anything else is
/// `None` and the caller falls back to treating the `[` literally.
/// Returns the element and the remainder after the closing `.]` / `=]`.
fn try_parse_collating_element(pat: &str) -> Option<(char, &str)> {
    let delim = pat.chars().next().filter(|d| matches!(d, '.' | '='))?;
    let body = &pat[delim.len_utf8()..];
    let c = body.chars().next()?;
    let after = &body[c.len_utf8()..];
    let mut closer = after.chars();
    if closer.next() == Some(delim) && closer.next() == Some(']') {
        Some((c, &after[delim.len_utf8() + ']'.len_utf8()..]))
    } else {
        None
    }
}

impl BracketItem {
    pub(super) fn matches(&self, c: char) -> bool {
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
