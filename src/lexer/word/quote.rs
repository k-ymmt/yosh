//! Quoting: `'...'`, `"..."`, backslash escapes, and `$'...'`.

use super::super::Lexer;
use crate::error::{self, ParseErrorKind, ShellError};
use crate::parser::ast::WordPart;

impl Lexer {
    /// Run `f` with `\<newline>` joining suppressed (single quotes,
    /// `$'...'`, here-document bodies keep backslash-newline verbatim).
    pub(crate) fn without_join<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.no_join += 1;
        let result = f(self);
        self.no_join -= 1;
        result
    }

    pub(super) fn read_single_quote(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        let content = self.without_join(|lx| {
            lx.advance(); // consume opening '
            let mut content = String::new();
            loop {
                if lx.at_end() {
                    return Err(ShellError::parse(
                        ParseErrorKind::UnterminatedSingleQuote,
                        span.line,
                        span.column,
                        "unterminated single quote",
                    ));
                }
                if lx.current_byte() == b'\'' {
                    break;
                }
                lx.push_current_char(&mut content)?;
            }
            Ok(content)
        })?;
        self.advance(); // consume closing ' (joining resumes after it)
        Ok(WordPart::SingleQuoted(content))
    }

    pub(super) fn read_double_quote(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // consume opening "
        let inner = self.read_word_parts(true, None)?;
        if self.at_end() {
            return Err(ShellError::parse(
                ParseErrorKind::UnterminatedDoubleQuote,
                span.line,
                span.column,
                "unterminated double quote",
            ));
        }
        self.advance(); // consume closing "
        Ok(WordPart::DoubleQuoted(inner))
    }

    /// Handles `\<char>` escape outside double quotes (`\<newline>` never
    /// reaches here: `advance` removes line continuations). Returns
    /// `EscapedLiteral` for the escaped character.
    pub(super) fn read_escape_unquoted(&mut self) -> error::Result<WordPart> {
        self.advance_raw(); // consume '\' — the escaped byte is taken raw
        if self.at_end() {
            // Trailing `\` with no following char: per POSIX, preserve as literal
            // backslash (implementation-defined; yosh keeps it).
            return Ok(WordPart::Literal("\\".to_string()));
        }
        let ch = self.advance_char()?;
        Ok(WordPart::EscapedLiteral(ch.to_string()))
    }

    /// Inside double quotes, `\` only escapes `$`, `` ` ``, `"`, `\`, and `<newline>`.
    /// Special escapes emit `EscapedLiteral`; `\<newline>` still returns an empty
    /// Literal that the outer filter removes; other `\<ch>` keeps the backslash
    /// as a literal two-character sequence per POSIX §2.2.3.
    pub(super) fn read_backslash_in_double_quote(&mut self) -> error::Result<WordPart> {
        self.advance_raw(); // consume '\' — the escaped byte is taken raw
        if self.at_end() {
            return Ok(WordPart::Literal("\\".to_string()));
        }
        let ch = self.current_byte();
        match ch {
            b'$' | b'`' | b'"' | b'\\' => {
                self.advance();
                Ok(WordPart::EscapedLiteral((ch as char).to_string()))
            }
            b'\n' => {
                self.advance();
                Ok(WordPart::Literal(String::new()))
            }
            _ => {
                let ch = self.advance_char()?;
                Ok(WordPart::Literal(format!("\\{}", ch)))
            }
        }
    }

    /// Handles `$'...'` with C-style escape sequences.
    /// Called after `$` is consumed; current byte is `'`.
    ///
    /// The body is assembled as BYTES: `\xHH` / `\NNN` denote raw byte
    /// values (POSIX/bash semantics), so a sequence like `\xe6\x97\xa5`
    /// must decode as the UTF-8 character 日, not as three Latin-1 chars.
    /// The byte buffer is encoded via `byteenc` at the end, so byte runs
    /// that are not valid UTF-8 are preserved losslessly end to end.
    pub(super) fn read_dollar_single_quote(&mut self) -> error::Result<WordPart> {
        // Backslash-newline is not a line continuation inside `$'...'`.
        let part = self.without_join(|lx| lx.read_dollar_single_quote_body())?;
        // Joining resumes after the closing quote.
        self.skip_line_continuations();
        Ok(part)
    }

    fn read_dollar_single_quote_body(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // consume opening '
        let mut bytes: Vec<u8> = Vec::new();
        loop {
            if self.at_end() {
                return Err(ShellError::parse(
                    ParseErrorKind::UnterminatedDollarSingleQuote,
                    span.line,
                    span.column,
                    "unterminated $'...' quote",
                ));
            }
            let ch = self.current_byte();
            if ch == b'\'' {
                self.advance(); // consume closing '
                break;
            }
            if ch == b'\\' {
                self.advance(); // consume '\'
                if self.at_end() {
                    bytes.push(b'\\');
                    break;
                }
                let esc = self.current_byte();
                if let Some(byte) = simple_escape(esc) {
                    self.advance();
                    bytes.push(byte);
                    continue;
                }
                match esc {
                    b'x' => {
                        self.advance();
                        let val = self.read_hex_digits(2);
                        bytes.push(val);
                    }
                    b'c' => {
                        self.advance();
                        // \cX — control character
                        if !self.at_end() {
                            let ctrl = self.current_byte();
                            self.advance();
                            bytes.push(ctrl & 0x1f);
                        }
                    }
                    b'0'..=b'7' => {
                        self.advance();
                        // octal: up to 3 digits, first digit already consumed
                        let mut val = (esc - b'0') as u32;
                        for _ in 0..2 {
                            let next = self.current_byte();
                            if (b'0'..=b'7').contains(&next) {
                                val = val * 8 + (next - b'0') as u32;
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        // bash truncates octal escapes to 8 bits.
                        bytes.push((val & 0xFF) as u8);
                    }
                    _ => {
                        // unknown escape: keep backslash and char
                        bytes.push(b'\\');
                        self.push_current_char_bytes(&mut bytes)?;
                    }
                }
            } else {
                self.push_current_char_bytes(&mut bytes)?;
            }
        }
        Ok(WordPart::DollarSingleQuoted(
            crate::byteenc::encode_bytes(&bytes).into_owned(),
        ))
    }

    /// Append the current (possibly multi-byte) character's UTF-8 bytes.
    fn push_current_char_bytes(&mut self, out: &mut Vec<u8>) -> error::Result<()> {
        let ch = self.advance_char()?;
        let mut buf = [0u8; 4];
        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        Ok(())
    }

    /// Helper for \xHH — reads up to `max` hex digits and returns the byte value.
    fn read_hex_digits(&mut self, max: usize) -> u8 {
        let mut val: u8 = 0;
        for _ in 0..max {
            let ch = self.current_byte();
            let digit = match ch {
                b'0'..=b'9' => ch - b'0',
                b'a'..=b'f' => ch - b'a' + 10,
                b'A'..=b'F' => ch - b'A' + 10,
                _ => break,
            };
            val = val.wrapping_mul(16).wrapping_add(digit);
            self.advance();
        }
        val
    }
}

/// The byte a single-character `$'...'` escape (`\n`, `\t`, `\\`, ...)
/// stands for; `None` for the multi-character (`\x`, `\c`, octal) and
/// unknown escapes.
fn simple_escape(esc: u8) -> Option<u8> {
    Some(match esc {
        b'a' => 0x07,
        b'b' => 0x08,
        b'e' | b'E' => 0x1B,
        b'f' => 0x0C,
        b'n' => b'\n',
        b'r' => b'\r',
        b't' => b'\t',
        b'v' => 0x0B,
        b'\\' => b'\\',
        b'\'' => b'\'',
        b'"' => b'"',
        _ => return None,
    })
}
