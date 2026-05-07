//! Character classification helpers and keyword tables for the highlight scanner.
//!
//! These are pure functions that read no state. They identify shell-syntax
//! categories: keywords, operators, redirect starts, valid name characters,
//! and word boundaries.

pub(super) const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "do", "done", "while", "until", "case", "esac",
    "in", "!", "{", "}",
];

/// Keywords after which the *next* word is also in command position.
pub(super) const COMMAND_POSITION_KEYWORDS: &[&str] = &["then", "else", "elif", "do", "!", "time"];

pub(super) fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word)
}

pub(super) fn is_operator_char(ch: char) -> bool {
    matches!(ch, '|' | '&' | ';')
}

pub(super) fn is_redirect_start(ch: char) -> bool {
    matches!(ch, '<' | '>')
}

pub(super) fn is_valid_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// True for characters that cannot appear inside an unquoted word.
pub(super) fn is_word_break(ch: char) -> bool {
    ch.is_ascii_whitespace()
        || is_operator_char(ch)
        || is_redirect_start(ch)
        || matches!(ch, '(' | ')' | '\'' | '"' | '`' | '$' | '#')
}
