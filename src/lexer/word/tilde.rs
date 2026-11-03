//! Tilde-prefixes at word start (POSIX §2.6.1).

use super::super::Lexer;
use crate::error;
use crate::parser::ast::WordPart;

impl Lexer {
    /// Reads a tilde-prefix (`~` or `~user`) at word start.
    ///
    /// POSIX §2.6.1: the tilde-prefix runs to the first unquoted `/` (or
    /// the end of the word — here the closing `}` when `in_brace`, i.e.
    /// the word of `${x:-~}`). If any character of it is quoted, or it
    /// contains an expansion (`$`, backquote), it is not a tilde-prefix
    /// and the `~` is an ordinary character: `~'root'`, `~"/q"`, `~$U`,
    /// `~root$(cmd)` all keep a literal `~` (dash/bash agree). Returns
    /// `None` in that case, having consumed only the `~`.
    pub(super) fn read_tilde(&mut self, in_brace: bool) -> error::Result<Option<WordPart>> {
        self.advance(); // consume '~'
        let start = self.save_cursor();
        let mut username = String::new();
        let mut quoted = false;
        while !self.at_end() {
            let ch = self.current_byte();
            let ends = if in_brace {
                ch == b'}'
            } else {
                Self::is_meta_or_whitespace(ch)
            };
            if ends || ch == b'/' {
                break;
            }
            if matches!(ch, b'\'' | b'"' | b'\\' | b'$' | b'`') {
                quoted = true;
                break;
            }
            self.push_current_char(&mut username)?;
        }
        if quoted {
            self.restore_cursor(start);
            return Ok(None);
        }
        if username.is_empty() {
            Ok(Some(WordPart::Tilde(None)))
        } else {
            Ok(Some(WordPart::Tilde(Some(username))))
        }
    }
}
