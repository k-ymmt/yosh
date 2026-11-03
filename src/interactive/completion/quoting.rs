//! Shell quoting for completion: unquoting what the user typed, and
//! quoting candidates for insertion so the shell reads them back
//! literally (including the quote context the typed word left open).

/// Remove shell quoting from a (possibly partial) word as the lexer
/// would: `'...'` is literal, `"..."` keeps its content with `\`
/// escaping only `$ \` backtick `"` newline, and an unquoted `\` escapes
/// the next character. Unterminated quotes and a trailing lone `\` are
/// tolerated (the word is still being typed).
pub fn shell_unquote(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = word.chars();
    while let Some(c) = chars.next() {
        if in_single {
            if c == '\'' {
                in_single = false;
            } else {
                out.push(c);
            }
            continue;
        }
        match c {
            '\'' if !in_double => in_single = true,
            '"' => in_double = !in_double,
            '\\' => match chars.next() {
                None | Some('\n') => {}
                Some(n) if in_double && !matches!(n, '$' | '`' | '"' | '\\') => {
                    out.push('\\');
                    out.push(n);
                }
                Some(n) => out.push(n),
            },
            c => out.push(c),
        }
    }
    out
}

/// Quote `text` (a completion candidate or common prefix) for insertion
/// right after `prefix`, the verbatim text kept before it, so the shell
/// reads back exactly `text`:
/// - inside an open `'` (per [`unclosed_quote`]): as-is, except that a
///   `'` becomes `'\''`;
/// - inside an open `"`: `"`, `$`, `` ` `` and `\` are backslash-escaped;
/// - unquoted: every shell metacharacter is backslash-escaped (blanks,
///   quotes, `\ $ ` | & ; < > ( ) * ? [ ] { } !`, plus `#` and `~` at
///   the start of the word), and a newline is written as `'<newline>'`
///   since `\<newline>` is a line continuation. `=` and `%` are left
///   alone: they are not special inside an argument word, and spec
///   candidates such as `--format=` must insert verbatim.
///
/// A raw byte that is not valid UTF-8 (a byteenc escape char, as glob
/// matches and directory listings carry for such file names) is written
/// as a `$'\xHH'` segment, closing and reopening an enclosing quote.
/// The escape char itself must never reach the edit buffer: the REPL
/// re-encodes typed text, so it would read back as different bytes.
pub fn quote_for_insertion(prefix: &str, text: &str) -> String {
    fn raw_byte(c: char, quote: Option<char>) -> Option<String> {
        let b = crate::byteenc::unescape_char(c)?;
        Some(match quote {
            Some(q) => format!("{q}$'\\x{b:02x}'{q}"),
            None => format!("$'\\x{b:02x}'"),
        })
    }
    let mut out = String::with_capacity(text.len() + 4);
    match unclosed_quote(prefix) {
        Some('\'') => {
            for c in text.chars() {
                if c == '\'' {
                    out.push_str("'\\''");
                } else if let Some(seg) = raw_byte(c, Some('\'')) {
                    out.push_str(&seg);
                } else {
                    out.push(c);
                }
            }
        }
        Some(q) => {
            for c in text.chars() {
                if let Some(seg) = raw_byte(c, Some(q)) {
                    out.push_str(&seg);
                    continue;
                }
                if matches!(c, '"' | '$' | '`' | '\\') {
                    out.push('\\');
                }
                out.push(c);
            }
        }
        None => {
            let at_word_start = prefix.is_empty();
            for (i, c) in text.chars().enumerate() {
                if let Some(seg) = raw_byte(c, None) {
                    out.push_str(&seg);
                    continue;
                }
                match c {
                    '\n' => out.push_str("'\n'"),
                    ' ' | '\t' | '\'' | '"' | '\\' | '$' | '`' | '|' | '&' | ';' | '<' | '>'
                    | '(' | ')' | '*' | '?' | '[' | ']' | '{' | '}' | '!' => {
                        out.push('\\');
                        out.push(c);
                    }
                    '#' | '~' if i == 0 && at_word_start => {
                        out.push('\\');
                        out.push(c);
                    }
                    c => out.push(c),
                }
            }
        }
    }
    out
}

/// The full replacement for a completion word: `prefix` (kept verbatim)
/// followed by `candidate` quoted for its context
/// ([`quote_for_insertion`]). With `finish` set and a non-directory
/// candidate, a quote left open by `prefix` is closed and a space ends
/// the argument (bash-like: `cat "My D<Tab>` → `cat "My Doc.txt" `);
/// directories stay open for further completion.
pub fn completion_replacement(prefix: &str, candidate: &str, finish: bool) -> String {
    let mut out = format!("{}{}", prefix, quote_for_insertion(prefix, candidate));
    if finish && !candidate.ends_with('/') {
        if let Some(q) = unclosed_quote(prefix) {
            out.push(q);
        }
        out.push(' ');
    }
    out
}

/// If `prefix` (the verbatim re-inserted part of a completion
/// replacement, e.g. `"/tmp/My ` or `--file='sub/`) leaves a quote
/// open, return that quote character so the caller can close it after
/// a completed filename — bash-like: `cat "My D<Tab>` completes to
/// `cat "My Doc.txt" ` (directories stay open for further completion).
pub fn unclosed_quote(prefix: &str) -> Option<char> {
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = prefix.chars();
    while let Some(ch) = chars.next() {
        match ch {
            // A backslash outside single quotes escapes the next char
            // (`it\'s` or `"a\"b` leave no quote open).
            '\\' if !in_single => {
                chars.next();
            }
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            _ => {}
        }
    }
    if in_single {
        Some('\'')
    } else if in_double {
        Some('"')
    } else {
        None
    }
}

/// The directory prefix of `word` exactly as the user typed it (up to
/// and including the last `/`, no tilde expansion or unquoting) —
/// mirroring [`split_path`] so quoted or escaped words (`'sub/xy`,
/// `sub\ dir/xy`, or the value part of `--file='sub/xy`) reconstruct
/// their replacement text consistently.
///
/// The returned prefix leaves the same quote open as the whole typed
/// word does, because the text after the cursor (a closing quote the
/// user already typed, or nothing) relies on it:
///
/// - A quote opened in the file-name part is re-opened: `cd "/tmp/My
///   D<Tab>` must complete to `"/tmp/My Dir/`, not `/tmp/My Dir/`, and
///   `Ca"rgo<Tab>"` to `"Cargo.toml"`, not the unbalanced `Cargo.toml"`.
/// - A quote the directory part opened but the file-name part closed is
///   closed again, so the candidate is quoted for the unquoted context
///   the rest of the line expects.
pub fn dir_prefix_of(word: &str) -> String {
    let mut dir = match word.rfind('/') {
        Some(pos) => word[..=pos].to_string(),
        None => String::new(),
    };
    let dir_open = unclosed_quote(&dir);
    let word_open = unclosed_quote(word);
    if dir_open != word_open {
        if let Some(q) = dir_open {
            dir.push(q);
        }
        if let Some(q) = word_open {
            dir.push(q);
        }
    }
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dir_prefix_keeps_leading_double_quote() {
        // `cd "/tmp/My D<Tab>` must re-insert the opening quote, or a
        // space-containing match replaces the word unquoted.
        assert_eq!(dir_prefix_of("\"/tmp/My D"), "\"/tmp/");
        assert_eq!(dir_prefix_of("'/tmp/My D"), "'/tmp/");
    }

    #[test]
    fn test_dir_prefix_quote_without_slash() {
        assert_eq!(dir_prefix_of("\"My D"), "\"");
        assert_eq!(dir_prefix_of("'My D"), "'");
    }

    #[test]
    fn test_dir_prefix_mirrors_the_words_open_quote() {
        // A quote opened mid-word is re-opened, so a closing quote after
        // the cursor stays balanced: `Ca"rgo<Tab>"` -> `"Cargo.toml"`.
        assert_eq!(dir_prefix_of("Ca\"rgo.toml"), "\"");
        assert_eq!(dir_prefix_of("src/ma'i"), "src/'");
        // A quote the directory part opened and the file part closed is
        // closed again.
        assert_eq!(dir_prefix_of("\"a/\"bc"), "\"a/\"");
        assert_eq!(dir_prefix_of("\"a/\"b'c"), "\"a/\"'");
        // A fully quoted file part leaves nothing open.
        assert_eq!(dir_prefix_of("\"Car\""), "");
    }

    #[test]
    fn test_dir_prefix_unquoted_unchanged() {
        assert_eq!(dir_prefix_of("src/int"), "src/");
        assert_eq!(dir_prefix_of("foo"), "");
        assert_eq!(dir_prefix_of("~/Doc/x"), "~/Doc/");
    }

    #[test]
    fn test_dir_prefix_escaped_and_mid_quote() {
        assert_eq!(dir_prefix_of("sub\\ dir/x"), "sub\\ dir/");
        assert_eq!(dir_prefix_of("sub/\"fo"), "sub/\"");
        // The quote opened mid-word stays open in the replacement, so a
        // closing quote after the cursor still balances (`"sub dir/`).
        assert_eq!(dir_prefix_of("su\"b"), "\"");
    }

    #[test]
    fn test_unclosed_quote() {
        assert_eq!(unclosed_quote("\"/tmp/My "), Some('"'));
        assert_eq!(unclosed_quote("'sub/"), Some('\''));
        assert_eq!(unclosed_quote("--file='sub/"), Some('\''));
        assert_eq!(unclosed_quote("src/"), None);
        assert_eq!(unclosed_quote("\"done\" "), None);
        // A double quote inside single quotes does not open a string.
        assert_eq!(unclosed_quote("'a\"b' "), None);
    }

    #[test]
    fn test_unclosed_quote_ignores_escaped_quotes() {
        assert_eq!(unclosed_quote("it\\'s/"), None);
        assert_eq!(unclosed_quote("\"a\\\"b/"), Some('"'));
    }

    #[test]
    fn test_shell_unquote() {
        assert_eq!(shell_unquote("sub\\ d"), "sub d");
        assert_eq!(shell_unquote("\"My D"), "My D");
        assert_eq!(shell_unquote("'it'\\''s"), "it's");
        assert_eq!(shell_unquote("\"a\\$b\\x"), "a$b\\x");
        assert_eq!(shell_unquote("x\\$\\(y"), "x$(y");
        assert_eq!(shell_unquote("trail\\"), "trail");
    }

    #[test]
    fn test_quote_for_insertion_unquoted() {
        assert_eq!(quote_for_insertion("", "sub dir/"), "sub\\ dir/");
        assert_eq!(
            quote_for_insertion("", "x$(echo PWNED)y"),
            "x\\$\\(echo\\ PWNED\\)y"
        );
        assert_eq!(quote_for_insertion("", "a'b\"c"), "a\\'b\\\"c");
        assert_eq!(quote_for_insertion("", "#x~"), "\\#x~");
        assert_eq!(quote_for_insertion("d/", "#x"), "#x");
        assert_eq!(quote_for_insertion("", "--format="), "--format=");
        assert_eq!(quote_for_insertion("", "a\nb"), "a'\n'b");
        assert_eq!(
            quote_for_insertion("", "*?[]&;|<>"),
            "\\*\\?\\[\\]\\&\\;\\|\\<\\>"
        );
    }

    #[test]
    fn test_quote_for_insertion_in_quotes() {
        assert_eq!(
            quote_for_insertion("\"", "a $b\"`\\ c"),
            "a \\$b\\\"\\`\\\\ c"
        );
        assert_eq!(quote_for_insertion("'", "it's $x"), "it'\\''s $x");
        assert_eq!(quote_for_insertion("--file='sub/", "a b"), "a b");
    }

    /// A raw non-UTF-8 byte (byteenc escape char) is inserted as
    /// `$'\xHH'`, never as the escape char itself.
    #[test]
    fn test_quote_for_insertion_raw_bytes() {
        let name = format!("a{}b", crate::byteenc::escape_char(0xff));
        assert_eq!(quote_for_insertion("", &name), "a$'\\xff'b");
        assert_eq!(quote_for_insertion("\"", &name), "a\"$'\\xff'\"b");
        assert_eq!(quote_for_insertion("'", &name), "a'$'\\xff''b");
    }

    #[test]
    fn test_completion_replacement() {
        assert_eq!(completion_replacement("", "sub dir/", true), "sub\\ dir/");
        assert_eq!(completion_replacement("", "a b", true), "a\\ b ");
        assert_eq!(completion_replacement("\"", "a b", true), "\"a b\" ");
        assert_eq!(completion_replacement("'", "it's", true), "'it'\\''s' ");
        assert_eq!(completion_replacement("", "a b", false), "a\\ b");
    }
}
