use unicode_width::UnicodeWidthChar;

/// Strip ANSI escape sequences from a string.
///
/// Handles CSI sequences (`\x1b[...X` where X is the final byte 0x40-0x7E)
/// and OSC sequences (`\x1b]...ST`).
pub fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if (0x40..=0x7E).contains(&(c as u32)) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Calculate the display width of a string, ignoring ANSI escapes
/// and accounting for Unicode East Asian Width.
pub fn display_width(s: &str) -> usize {
    strip_ansi(s)
        .chars()
        .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_no_escapes() {
        assert_eq!(strip_ansi("hello"), "hello");
    }

    #[test]
    fn strip_ansi_sgr_color() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
    }

    #[test]
    fn strip_ansi_multiple_params() {
        assert_eq!(strip_ansi("\x1b[1;32mbold green\x1b[0m"), "bold green");
    }

    #[test]
    fn strip_ansi_256_color() {
        assert_eq!(strip_ansi("\x1b[38;5;200mpink\x1b[0m"), "pink");
    }

    #[test]
    fn strip_ansi_rgb() {
        assert_eq!(strip_ansi("\x1b[38;2;255;100;0morange\x1b[0m"), "orange");
    }

    #[test]
    fn strip_ansi_mixed_text() {
        assert_eq!(
            strip_ansi("\x1b[34m~/proj\x1b[0m \x1b[32m\u{e0a0} main\x1b[0m"),
            "~/proj \u{e0a0} main"
        );
    }

    #[test]
    fn strip_ansi_empty() {
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn width_ascii() {
        assert_eq!(display_width("hello"), 5);
    }

    #[test]
    fn width_cjk() {
        assert_eq!(display_width("日本語"), 6);
    }

    #[test]
    fn width_mixed_ascii_cjk() {
        assert_eq!(display_width("hi日本"), 6);
    }

    #[test]
    fn width_ansi_ignored() {
        // Visible: "❯ " = ❯(1) + space(1) = 2
        assert_eq!(display_width("\x1b[1;35m❯\x1b[0m "), 2);
    }

    #[test]
    fn width_ansi_color_text() {
        assert_eq!(display_width("\x1b[34m~/proj\x1b[0m"), 6);
    }

    #[test]
    fn width_complex_prompt() {
        assert_eq!(
            display_width("\x1b[34m~/proj\x1b[0m \x1b[32m main\x1b[0m"),
            12
        );
    }

    #[test]
    fn width_empty() {
        assert_eq!(display_width(""), 0);
    }

    #[test]
    fn width_prompt_like_string() {
        assert_eq!(display_width("\x1b[1;35m❯\x1b[0m "), 2);
    }

    #[test]
    fn width_cjk_in_prompt() {
        // "ディレクトリ" is 6 katakana chars × width 2 = 12
        assert_eq!(display_width("\x1b[34mディレクトリ\x1b[0m"), 12);
    }
}
