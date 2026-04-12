/// Path completion for interactive tab-completion.
///
/// This module provides the core logic for completing file and directory
/// paths when the user presses Tab in interactive mode.

/// Scan leftward from `cursor` to find the start of the completion word.
///
/// Delimiters that break a word: space, `|`, `;`, `&`, `<`, `>`, `(`, `)`.
/// Inside quotes (single or double), spaces do not act as delimiters,
/// but the quote character itself is included in the returned word.
///
/// Returns `(word_start_index, word_slice)`.
pub fn extract_completion_word(buf: &str, cursor: usize) -> (usize, &str) {
    let bytes = buf.as_bytes();
    let end = cursor.min(buf.len());

    // Scan left-to-right from the beginning up to `end`, tracking the last
    // unquoted delimiter. The completion word starts right after that delimiter.
    let mut word_start: usize = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    let mut i = 0;
    while i < end {
        let ch = bytes[i];
        match ch {
            b'\'' if !in_double_quote => {
                if !in_single_quote {
                    // Opening quote — this is the start of a new word
                    // only if preceded by a delimiter (or at start).
                    // We treat the quote as part of the word, so update
                    // word_start to here.
                    if i == 0 || is_unquoted_delimiter(bytes[i - 1]) {
                        word_start = i;
                    }
                }
                in_single_quote = !in_single_quote;
            }
            b'"' if !in_single_quote => {
                if !in_double_quote {
                    if i == 0 || is_unquoted_delimiter(bytes[i - 1]) {
                        word_start = i;
                    }
                }
                in_double_quote = !in_double_quote;
            }
            b' ' | b'|' | b';' | b'&' | b'<' | b'>' | b'(' | b')'
                if !in_single_quote && !in_double_quote =>
            {
                word_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }

    (word_start, &buf[word_start..end])
}

fn is_unquoted_delimiter(ch: u8) -> bool {
    matches!(ch, b' ' | b'|' | b';' | b'&' | b'<' | b'>' | b'(' | b')')
}

/// Split a completion word at the last `/` into (directory_part, prefix).
///
/// - If the word starts with `~`, the tilde is expanded to `home`.
/// - A leading quote character (`'` or `"`) is stripped before processing.
/// - The directory part retains its trailing `/`.
///
/// Returns `(directory_string, prefix_slice)`.
pub fn split_path<'a>(word: &'a str, home: &str) -> (String, &'a str) {
    // Strip leading quote character
    let stripped = if word.starts_with('\'') || word.starts_with('"') {
        &word[1..]
    } else {
        word
    };

    match stripped.rfind('/') {
        Some(pos) => {
            let dir_part = &stripped[..=pos]; // includes the '/'
            let prefix = &stripped[pos + 1..];
            // Expand tilde
            let dir_expanded = if dir_part.starts_with('~') {
                format!("{}{}", home, &dir_part[1..])
            } else {
                dir_part.to_string()
            };

            // Map slice back to the original word's lifetime
            // prefix is a slice of `stripped`, which is a sub-slice of `word`
            (dir_expanded, prefix)
        }
        None => {
            // No slash: expand lone tilde prefix
            if stripped == "~" {
                (format!("{}/", home), "")
            } else {
                (String::new(), stripped)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_completion_word ──────────────────────────────────────

    #[test]
    fn test_extract_simple_word() {
        let (start, word) = extract_completion_word("ls foo", 6);
        assert_eq!(start, 3);
        assert_eq!(word, "foo");
    }

    #[test]
    fn test_extract_at_start() {
        let (start, word) = extract_completion_word("foo", 3);
        assert_eq!(start, 0);
        assert_eq!(word, "foo");
    }

    #[test]
    fn test_extract_after_pipe() {
        let (start, word) = extract_completion_word("cat foo | grep b", 16);
        assert_eq!(start, 15);
        assert_eq!(word, "b");
    }

    #[test]
    fn test_extract_after_semicolon() {
        let (start, word) = extract_completion_word("echo a; ls sr", 13);
        assert_eq!(start, 11);
        assert_eq!(word, "sr");
    }

    #[test]
    fn test_extract_empty_at_space() {
        let (start, word) = extract_completion_word("ls ", 3);
        assert_eq!(start, 3);
        assert_eq!(word, "");
    }

    #[test]
    fn test_extract_path_with_slash() {
        let (start, word) = extract_completion_word("ls src/int", 10);
        assert_eq!(start, 3);
        assert_eq!(word, "src/int");
    }

    #[test]
    fn test_extract_with_double_quote() {
        let (start, word) = extract_completion_word("ls \"My Doc", 10);
        assert_eq!(start, 3);
        assert_eq!(word, "\"My Doc");
    }

    #[test]
    fn test_extract_with_single_quote() {
        let (start, word) = extract_completion_word("ls 'My Doc", 10);
        assert_eq!(start, 3);
        assert_eq!(word, "'My Doc");
    }

    // ── split_path ──────────────────────────────────────────────────

    #[test]
    fn test_split_relative_path() {
        let (dir, prefix) = split_path("src/int", "/home/user");
        assert_eq!(dir, "src/");
        assert_eq!(prefix, "int");
    }

    #[test]
    fn test_split_no_directory() {
        let (dir, prefix) = split_path("foo", "/home/user");
        assert_eq!(dir, "");
        assert_eq!(prefix, "foo");
    }

    #[test]
    fn test_split_absolute_path() {
        let (dir, prefix) = split_path("/usr/lo", "/home/user");
        assert_eq!(dir, "/usr/");
        assert_eq!(prefix, "lo");
    }

    #[test]
    fn test_split_tilde_path() {
        let (dir, prefix) = split_path("~/Doc", "/home/user");
        assert_eq!(dir, "/home/user/");
        assert_eq!(prefix, "Doc");
    }

    #[test]
    fn test_split_trailing_slash() {
        let (dir, prefix) = split_path("src/", "/home/user");
        assert_eq!(dir, "src/");
        assert_eq!(prefix, "");
    }
}
