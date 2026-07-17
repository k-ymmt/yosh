//! Lossless byte <-> `String` escaping for POSIX byte semantics.
//!
//! POSIX shells treat words, variable values, argv, and paths as byte
//! strings; Rust `String` requires valid UTF-8. Instead of migrating every
//! carrier type to `Vec<u8>`, yosh keeps `String` and makes it losslessly
//! representable for arbitrary bytes (the approach used by fish shell and
//! Python's PEP 383):
//!
//! - At *ingress* boundaries (script files, `$'\xHH'`, command-substitution
//!   capture, `read`, the inherited environment, directory entries), each
//!   byte that is not part of a valid UTF-8 sequence is mapped to the
//!   private-use codepoint `U+10FE00 + byte`.
//! - At *egress* boundaries (exec argv/environ, redirect paths, `echo` /
//!   `printf` output, `cd`), escape codepoints are mapped back to their raw
//!   bytes.
//!
//! Injectivity: a *real* `U+10FE80..=U+10FEFF` codepoint arriving as valid
//! UTF-8 is escaped byte-by-byte (each of its UTF-8 bytes is `>= 0x80`), so
//! `decode(encode(x)) == x` for every byte string and no two byte strings
//! share an encoded form.
//!
//! See `docs/superpowers/specs/2026-07-17-posix-byte-semantics-stage2-design.md`.

use std::borrow::Cow;

/// Base of the escape range. `0x80..=0xFF` map to `BASE+0x80 ..= BASE+0xFF`.
pub const ESCAPE_BASE: u32 = 0x10FE00;

/// Map a raw byte to its escape codepoint.
///
/// Only bytes `>= 0x80` ever need escaping (ASCII is always valid UTF-8),
/// but the mapping is defined for all bytes for round-trip helpers.
#[inline]
pub fn escape_char(b: u8) -> char {
    // SAFETY-free: 0x10FE00..=0x10FEFF are valid Unicode scalar values.
    char::from_u32(ESCAPE_BASE + b as u32).unwrap()
}

/// Map an escape codepoint back to its raw byte, if `c` is in the range.
#[inline]
pub fn unescape_char(c: char) -> Option<u8> {
    let v = c as u32;
    if (ESCAPE_BASE..=ESCAPE_BASE + 0xFF).contains(&v) {
        Some((v - ESCAPE_BASE) as u8)
    } else {
        None
    }
}

/// True if `s` may contain an escape codepoint.
///
/// Every escape codepoint's UTF-8 encoding starts with `0xF4`, so absence of
/// that byte proves absence of escapes (cheap fast path).
#[inline]
fn may_contain_escapes(s: &str) -> bool {
    s.as_bytes().contains(&0xF4)
}

/// True if the valid UTF-8 chunk `s` contains a real escape-range codepoint.
#[inline]
fn contains_escape_range_char(s: &str) -> bool {
    may_contain_escapes(s) && s.chars().any(|c| unescape_char(c).is_some())
}

/// Encode arbitrary bytes as a `String`, escaping invalid UTF-8 bytes (and
/// any real escape-range codepoints) so the original bytes are recoverable
/// with [`decode_bytes`]. Borrows when no escaping is needed.
pub fn encode_bytes(bytes: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(bytes) {
        Ok(s) if !contains_escape_range_char(s) => Cow::Borrowed(s),
        _ => Cow::Owned(encode_bytes_owned(bytes)),
    }
}

fn encode_bytes_owned(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut rest = bytes;
    loop {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                push_valid_chunk(&mut out, s);
                return out;
            }
            Err(e) => {
                let (valid, after) = rest.split_at(e.valid_up_to());
                // `valid_up_to` guarantees this chunk is valid UTF-8.
                push_valid_chunk(&mut out, unsafe { std::str::from_utf8_unchecked(valid) });
                let bad_len = e.error_len().unwrap_or(after.len());
                for &b in &after[..bad_len] {
                    out.push(escape_char(b));
                }
                rest = &after[bad_len..];
            }
        }
    }
}

/// Append a valid-UTF-8 chunk, escaping real escape-range codepoints
/// byte-by-byte to keep the encoding injective.
fn push_valid_chunk(out: &mut String, s: &str) {
    if !contains_escape_range_char(s) {
        out.push_str(s);
        return;
    }
    for c in s.chars() {
        if unescape_char(c).is_some() {
            let mut buf = [0u8; 4];
            for &b in c.encode_utf8(&mut buf).as_bytes() {
                out.push(escape_char(b));
            }
        } else {
            out.push(c);
        }
    }
}

/// Decode an encoded `String` back to raw bytes. Borrows when the string
/// contains no escape codepoints (the common all-UTF-8 case).
pub fn decode_bytes(s: &str) -> Cow<'_, [u8]> {
    if !may_contain_escapes(s) {
        return Cow::Borrowed(s.as_bytes());
    }
    let mut out: Vec<u8> = Vec::with_capacity(s.len());
    let mut changed = false;
    for c in s.chars() {
        if let Some(b) = unescape_char(c) {
            out.push(b);
            changed = true;
        } else {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    if changed {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(s.as_bytes())
    }
}

/// Write a byteenc-encoded string to stderr as raw bytes (escape
/// codepoints become their original bytes), newline-terminated. Used for
/// diagnostics that embed user data (xtrace lines, command names) so they
/// print the original bytes like other byte-oriented shells.
pub fn write_stderr_decoded_line(s: &str) {
    use std::io::Write;
    let stderr = std::io::stderr();
    let mut h = stderr.lock();
    let _ = h.write_all(&decode_bytes(s));
    let _ = h.write_all(b"\n");
}

/// Decode an encoded `String` for a UTF-8-only surface (the plugin API):
/// raw invalid bytes become U+FFFD instead of escape codepoints.
pub fn decode_lossy(s: &str) -> Cow<'_, str> {
    if !may_contain_escapes(s) {
        return Cow::Borrowed(s);
    }
    match decode_bytes(s) {
        Cow::Borrowed(_) => Cow::Borrowed(s),
        Cow::Owned(bytes) => Cow::Owned(String::from_utf8_lossy(&bytes).into_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_and_utf8_borrow_through() {
        for s in ["", "hello", "日本語", "a\tb\nc"] {
            assert!(matches!(encode_bytes(s.as_bytes()), Cow::Borrowed(_)));
            assert!(matches!(decode_bytes(s), Cow::Borrowed(_)));
            assert_eq!(decode_bytes(s).as_ref(), s.as_bytes());
        }
    }

    #[test]
    fn every_single_byte_round_trips() {
        for b in 0u8..=255 {
            let src = [b];
            let enc = encode_bytes(&src);
            assert_eq!(decode_bytes(&enc).as_ref(), &[b], "byte {b:#x}");
        }
    }

    #[test]
    fn invalid_byte_encodes_to_escape_char() {
        let enc = encode_bytes(b"\xe9");
        assert_eq!(enc.chars().collect::<Vec<_>>(), vec![escape_char(0xe9)]);
    }

    #[test]
    fn mixed_valid_invalid_round_trips() {
        let cases: &[&[u8]] = &[
            b"a\xe9b",
            b"\xff\xfe",
            b"\xe6\x97",             // truncated multi-byte
            b"\xe6\x97\xa5",         // valid multi-byte stays intact
            b"pre\x80mid\xc3\xa9\xf0end",
            b"\x80\x81\x82\x83",
        ];
        for &c in cases {
            let enc = encode_bytes(c);
            assert_eq!(decode_bytes(&enc).as_ref(), c, "case {c:?}");
        }
    }

    #[test]
    fn valid_multibyte_not_escaped() {
        let enc = encode_bytes("日".as_bytes());
        assert_eq!(enc.as_ref(), "日");
    }

    #[test]
    fn real_escape_range_char_round_trips() {
        // A genuine U+10FE85 in valid UTF-8 input must survive decode.
        let s = "a\u{10FE85}b";
        let enc = encode_bytes(s.as_bytes());
        // Injectivity: it is re-escaped byte-by-byte, not passed through.
        assert_ne!(enc.as_ref(), s);
        assert_eq!(decode_bytes(&enc).as_ref(), s.as_bytes());
    }

    #[test]
    fn escape_encoding_is_injective_across_forms() {
        // encode(raw 0xe9) and encode(UTF-8 of U+10FE69) must differ.
        let a = encode_bytes(b"\xe9").into_owned();
        let b_src = "\u{10FE69}".as_bytes();
        let b = encode_bytes(b_src).into_owned();
        assert_ne!(a, b);
        assert_eq!(decode_bytes(&a).as_ref(), b"\xe9");
        assert_eq!(decode_bytes(&b).as_ref(), b_src);
    }

    #[test]
    fn decode_lossy_replaces_invalid_bytes() {
        let enc = encode_bytes(b"a\xe9b");
        assert_eq!(decode_lossy(&enc).as_ref(), "a\u{FFFD}b");
        assert!(matches!(decode_lossy("plain"), Cow::Borrowed(_)));
    }

    #[test]
    fn unescape_char_bounds() {
        assert_eq!(unescape_char(escape_char(0x00)), Some(0x00));
        assert_eq!(unescape_char(escape_char(0xFF)), Some(0xFF));
        assert_eq!(unescape_char('a'), None);
        assert_eq!(unescape_char('\u{10FDFF}'), None);
        assert_eq!(unescape_char('\u{10FF00}'), None);
    }
}
