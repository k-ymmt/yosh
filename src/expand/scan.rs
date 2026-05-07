//! Lexical balanced-bracket scanners with quote/escape awareness.
//!
//! Used by `expand::heredoc` (after PR-B) and `expand::arith` for parenthesis-,
//! brace-, and double-paren depth tracking inside string bodies.

/// Skip forward from `start` in `bytes`, tracking parenthesis depth (starting at 1),
/// while respecting single/double quotes and backslash escapes.
/// Returns the index of the byte where depth reaches 0 (the closing `)`).
/// If no matching `)` is found, returns `bytes.len()`.
pub fn skip_balanced_parens(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    let mut depth: usize = 1;
    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'\'' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'\'' {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if i < bytes.len() {
                    i += 1;
                }
            }
            b'\\' => {
                if i + 1 < bytes.len() {
                    i += 2;
                } else {
                    i += 1;
                }
            }
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
        match bytes[i] {
            b'\'' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'\'' {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if i < bytes.len() {
                    i += 1;
                }
            }
            b'\\' => {
                if i + 1 < bytes.len() {
                    i += 2;
                } else {
                    i += 1;
                }
            }
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
        match bytes[i] {
            b'\'' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'\'' {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if i < bytes.len() {
                    i += 1;
                }
            }
            b'\\' => {
                if i + 1 < bytes.len() {
                    i += 2;
                } else {
                    i += 1;
                }
            }
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
