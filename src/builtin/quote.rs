//! Single-quoting for shell re-input.
//!
//! POSIX requires the listings of `set`, `export -p`, `readonly -p`,
//! `trap`, `alias` and `command -v` to be written "in a format that can
//! be reused as input"; every builtin that quotes a value for that
//! purpose goes through this module.

/// Quote a variable value so the printed assignment is suitable for shell
/// re-input (POSIX §2.15: `set`, `export -p`, `readonly -p` must write
/// their output "in a format that can be reused as input"). Values made
/// only of safe characters print unquoted; anything else is wrapped in
/// single quotes, with embedded single quotes emitted as `'\''`.
/// byteenc-escaped bytes (U+10FE00 range) are not in the safe set, so they
/// land inside single quotes where the raw bytes decoded at the stdout
/// boundary are literal — encoded values round-trip unchanged.
pub(crate) fn quote_for_reinput(value: &str) -> std::borrow::Cow<'_, str> {
    fn is_safe(c: char) -> bool {
        c.is_ascii_alphanumeric()
            || matches!(c, '_' | '.' | '/' | ':' | '-' | '+' | ',' | '@' | '%')
    }
    if !value.is_empty() && value.chars().all(is_safe) {
        return std::borrow::Cow::Borrowed(value);
    }
    std::borrow::Cow::Owned(single_quote(value))
}

/// Unconditionally wrap `value` in single quotes for shell re-input,
/// emitting each embedded `'` as `'\''`. Used where the output format
/// always quotes (the `trap` and `alias` listings) and by
/// [`quote_for_reinput`] for values that need quoting.
pub(crate) fn single_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── quote_for_reinput (POSIX §2.15 re-input format) ──

    #[test]
    fn quote_for_reinput_safe_value_unquoted() {
        assert_eq!(quote_for_reinput("plain"), "plain");
        assert_eq!(
            quote_for_reinput("/usr/local/bin:/usr/bin"),
            "/usr/local/bin:/usr/bin"
        );
        assert_eq!(quote_for_reinput("v1.2-rc+3"), "v1.2-rc+3");
    }

    #[test]
    fn quote_for_reinput_empty_value_quoted() {
        assert_eq!(quote_for_reinput(""), "''");
    }

    #[test]
    fn quote_for_reinput_space_and_specials_quoted() {
        assert_eq!(quote_for_reinput("a b"), "'a b'");
        assert_eq!(quote_for_reinput("say \"hi\""), "'say \"hi\"'");
        assert_eq!(quote_for_reinput("$HOME `x` \\"), "'$HOME `x` \\'");
        assert_eq!(quote_for_reinput("has\nnewline"), "'has\nnewline'");
    }

    #[test]
    fn quote_for_reinput_embedded_single_quote() {
        assert_eq!(quote_for_reinput("it's"), "'it'\\''s'");
    }

    #[test]
    fn quote_for_reinput_byteenc_escape_round_trips() {
        // A value carrying byteenc-escaped raw bytes must pass through
        // unchanged apart from the surrounding quotes; the escape
        // codepoints decode to their original bytes at the stdout
        // boundary and single quotes keep those bytes literal on
        // re-input.
        let encoded = crate::byteenc::encode_bytes(b"f\xe9g").into_owned();
        let quoted = quote_for_reinput(&encoded);
        assert_eq!(quoted, format!("'{}'", encoded));
    }

    #[test]
    fn single_quote_always_quotes() {
        assert_eq!(single_quote("plain"), "'plain'");
        assert_eq!(single_quote(""), "''");
        assert_eq!(single_quote("it's"), "'it'\\''s'");
    }
}
