//! Lexical balanced-bracket scanners with quote/escape awareness.
//!
//! Used by `expand::heredoc` (after PR-B) and `expand::arith` for parenthesis-,
//! brace-, and double-paren depth tracking inside string bodies.

/// If `bytes[i]` begins a quoted run or a backslash escape, return
/// `Some(j)` where `j` is the index just past it; otherwise `None`.
///
/// - `'...'`: scans to the closing quote (or end of input).
/// - `"..."`: scans to the closing quote, honoring `\x` escapes inside
///   (or end of input).
/// - `\x`: skips the escaped byte (a trailing `\` skips just itself).
///
/// Unterminated quotes consume the rest of the input, preserving the
/// historical contract of the three balanced scanners below.
fn skip_quoted_or_escaped(bytes: &[u8], i: usize) -> Option<usize> {
    match bytes[i] {
        b'\'' => {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'\'' {
                j += 1;
            }
            if j < bytes.len() {
                j += 1;
            }
            Some(j)
        }
        b'"' => {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'"' {
                if bytes[j] == b'\\' && j + 1 < bytes.len() {
                    j += 2;
                } else {
                    j += 1;
                }
            }
            if j < bytes.len() {
                j += 1;
            }
            Some(j)
        }
        b'\\' => {
            if i + 1 < bytes.len() {
                Some(i + 2)
            } else {
                Some(i + 1)
            }
        }
        _ => None,
    }
}

/// Skip forward from `start` in `bytes`, tracking parenthesis depth (starting at 1),
/// while respecting single/double quotes and backslash escapes.
/// Returns the index of the byte where depth reaches 0 (the closing `)`).
/// If no matching `)` is found, returns `bytes.len()`.
pub fn skip_balanced_parens(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    let mut depth: usize = 1;
    while i < bytes.len() && depth > 0 {
        if let Some(next) = skip_quoted_or_escaped(bytes, i) {
            i = next;
            continue;
        }
        match bytes[i] {
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                if depth > 0 {
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    i
}

/// Like `skip_balanced_parens`, but for `{`/`}` braces.
/// Used for `${...}` parameter expansion scanning in heredoc strings.
/// Returns the index of the closing `}` where depth reaches 0.
/// If no matching `}` is found, returns `bytes.len()`.
pub fn skip_balanced_braces(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    let mut depth: usize = 1;
    while i < bytes.len() && depth > 0 {
        if let Some(next) = skip_quoted_or_escaped(bytes, i) {
            i = next;
            continue;
        }
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                if depth > 0 {
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    i
}

/// Like `skip_balanced_parens`, but terminates when `))` is found at depth 1.
/// Used for `$((...))` arithmetic substitution scanning.
/// Returns the index of the first `)` in the closing `))`.
/// If no matching `))` is found, returns `bytes.len()`.
pub fn skip_balanced_double_parens(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    let mut depth: usize = 1;
    while i + 1 < bytes.len() && depth > 0 {
        if let Some(next) = skip_quoted_or_escaped(bytes, i) {
            i = next;
            continue;
        }
        match bytes[i] {
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' if bytes[i + 1] == b')' && depth == 1 => {
                break;
            }
            b')' => {
                depth -= 1;
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── existing: balanced-parens ─────────────────────────────────────
    #[test]
    fn test_skip_balanced_parens_simple() {
        let input = b"echo hello)";
        assert_eq!(skip_balanced_parens(input, 0), 10);
    }

    #[test]
    fn test_skip_balanced_parens_nested() {
        let input = b"(inner) outer)";
        assert_eq!(skip_balanced_parens(input, 0), 13);
    }

    #[test]
    fn test_skip_balanced_parens_single_quoted() {
        let input = b"')' real)";
        assert_eq!(skip_balanced_parens(input, 0), 8);
    }

    #[test]
    fn test_skip_balanced_parens_double_quoted() {
        let input = b"\")(\" real)";
        assert_eq!(skip_balanced_parens(input, 0), 9);
    }

    #[test]
    fn test_skip_balanced_parens_backslash_escape() {
        let input = b"\\) real)";
        assert_eq!(skip_balanced_parens(input, 0), 7);
    }

    // ── new: unterminated-input contract ─────────────────────────────
    #[test]
    fn test_skip_balanced_parens_unterminated_returns_len() {
        let input = b"echo hello";
        assert_eq!(skip_balanced_parens(input, 0), input.len());
    }

    // ── existing: balanced-double-parens ─────────────────────────────
    #[test]
    fn test_skip_balanced_double_parens_simple() {
        let input = b"1 + 2))";
        assert_eq!(skip_balanced_double_parens(input, 0), 5);
    }

    #[test]
    fn test_skip_balanced_double_parens_nested() {
        let input = b"(1 + 2) * 3))";
        assert_eq!(skip_balanced_double_parens(input, 0), 11);
    }

    // ── new: unterminated-input contract ─────────────────────────────
    #[test]
    fn test_skip_balanced_double_parens_unterminated_returns_len() {
        let input = b"1 + 2 + expr";
        // Double-parens requires i + 1 < bytes.len() to check for ))
        // so on unterminated input, it exits when it can't look ahead 2 bytes
        assert_eq!(skip_balanced_double_parens(input, 0), input.len() - 1);
    }

    // ── existing: balanced-braces ────────────────────────────────────
    #[test]
    fn test_skip_balanced_braces_simple() {
        let input = b"var}";
        assert_eq!(skip_balanced_braces(input, 0), 3);
    }

    #[test]
    fn test_skip_balanced_braces_nested() {
        let input = b"{inner} outer}";
        assert_eq!(skip_balanced_braces(input, 0), 13);
    }

    #[test]
    fn test_skip_balanced_braces_single_quoted() {
        let input = b"var:-'}'}";
        assert_eq!(skip_balanced_braces(input, 0), 8);
    }

    #[test]
    fn test_skip_balanced_braces_double_quoted() {
        let input = b"var:-\"}{\"}";
        assert_eq!(skip_balanced_braces(input, 0), 9);
    }

    #[test]
    fn test_skip_balanced_braces_backslash_escape() {
        let input = b"var:-\\} real}";
        assert_eq!(skip_balanced_braces(input, 0), 12);
    }

    // ── new: unterminated-input contract ─────────────────────────────
    #[test]
    fn test_skip_balanced_braces_unterminated_returns_len() {
        let input = b"var:-default";
        assert_eq!(skip_balanced_braces(input, 0), input.len());
    }
}
