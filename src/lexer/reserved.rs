//! POSIX reserved words per IEEE Std 1003.1-2017 §2.4.

pub const RESERVED_WORDS: &[&str] = &[
    "!", "{", "}", "case", "do", "done", "elif", "else", "esac", "fi", "for", "if", "in", "then",
    "until", "while",
];

pub fn is_posix_reserved_word(name: &str) -> bool {
    RESERVED_WORDS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_posix_reserved_words_are_recognized() {
        for kw in [
            "!", "{", "}", "case", "do", "done", "elif", "else", "esac", "fi", "for", "if", "in",
            "then", "until", "while",
        ] {
            assert!(is_posix_reserved_word(kw), "{kw} should be reserved");
        }
    }

    #[test]
    fn non_reserved_words_return_false() {
        for s in ["echo", "foo", "", "IF", "If"] {
            assert!(!is_posix_reserved_word(s), "{s} should not be reserved");
        }
    }

    #[test]
    fn list_length_is_sixteen() {
        assert_eq!(RESERVED_WORDS.len(), 16);
    }
}
