//! Byte-level text helpers: backslash escapes and character counting
//! (a whole UTF-8 sequence is one character, an invalid byte another).

/// Parse a run of ASCII digits; returns (value, bytes consumed).
pub(super) fn digits(s: &[u8]) -> (usize, usize) {
    let mut n: usize = 0;
    let mut i = 0;
    while let Some(&c) = s.get(i) {
        if !c.is_ascii_digit() {
            break;
        }
        n = n.saturating_mul(10).saturating_add((c - b'0') as usize);
        i += 1;
    }
    (n, i)
}

/// Decode one backslash escape at `s[0] == b'\\'` into `out`.
/// Returns (bytes consumed, stop) where `stop` is `\c` in `%b` mode.
pub(super) fn unescape_into(s: &[u8], b_mode: bool, out: &mut Vec<u8>) -> (usize, bool) {
    let Some(&c) = s.get(1) else {
        out.push(b'\\');
        return (1, false);
    };
    let simple = match c {
        b'\\' => Some(b'\\'),
        b'a' => Some(0x07),
        b'b' => Some(0x08),
        b'f' => Some(0x0c),
        b'n' => Some(b'\n'),
        b'r' => Some(b'\r'),
        b't' => Some(b'\t'),
        b'v' => Some(0x0b),
        _ => None,
    };
    if let Some(b) = simple {
        out.push(b);
        return (2, false);
    }
    if b_mode && c == b'c' {
        return (2, true);
    }
    if c.is_ascii_digit() && c < b'8' {
        // Format: \ddd (1-3 octal digits). %b: \0ddd (0 then up to 3
        // digits); a non-zero leading digit is accepted as \ddd too.
        let mut i = 1;
        let mut max_digits = 3;
        if b_mode && c == b'0' {
            i = 2;
            max_digits = 3;
            // If nothing follows the 0, the escape is just NUL.
        }
        let mut v: u32 = 0;
        let mut n = 0;
        while n < max_digits {
            match s.get(i) {
                Some(&d) if (b'0'..=b'7').contains(&d) => {
                    v = v * 8 + (d - b'0') as u32;
                    i += 1;
                    n += 1;
                }
                _ => break,
            }
        }
        if b_mode && c == b'0' && n == 0 {
            out.push(0);
            return (2, false);
        }
        out.push((v & 0xff) as u8);
        return (i, false);
    }
    // Unknown escape: emit the backslash and let the next byte be
    // processed literally by the caller.
    out.push(b'\\');
    (1, false)
}

/// Length in bytes of the first character of `s` (a whole UTF-8
/// sequence when valid, otherwise a single byte); 0 for empty input.
pub(super) fn first_char_len(s: &[u8]) -> usize {
    if s.is_empty() {
        return 0;
    }
    let want = match s[0] {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return 1,
    };
    if s.len() >= want && std::str::from_utf8(&s[..want]).is_ok() {
        want
    } else {
        1
    }
}

/// Number of characters in `s` counting each invalid byte as one.
pub(super) fn char_len(s: &[u8]) -> usize {
    let mut i = 0;
    let mut n = 0;
    while i < s.len() {
        i += first_char_len(&s[i..]);
        n += 1;
    }
    n
}

/// First `n` characters of `s` (see [`char_len`]).
pub(super) fn truncate_chars(s: &[u8], n: usize) -> &[u8] {
    let mut i = 0;
    let mut k = 0;
    while i < s.len() && k < n {
        i += first_char_len(&s[i..]);
        k += 1;
    }
    &s[..i]
}
