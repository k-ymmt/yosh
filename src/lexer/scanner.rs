use super::Lexer;
use super::token::{Span, SpannedToken, Token};
use crate::error::{self, ParseErrorKind, ShellError};

impl Lexer {
    pub(crate) fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    pub(crate) fn current_byte(&self) -> u8 {
        if self.at_end() {
            0
        } else {
            self.input[self.pos]
        }
    }

    pub(crate) fn peek_byte(&self) -> u8 {
        if self.pos + 1 >= self.input.len() {
            0
        } else {
            self.input[self.pos + 1]
        }
    }

    pub(crate) fn peek_byte2(&self) -> u8 {
        if self.pos + 2 >= self.input.len() {
            0
        } else {
            self.input[self.pos + 2]
        }
    }

    pub(crate) fn advance(&mut self) -> u8 {
        let ch = self.current_byte();
        if !self.at_end() {
            if ch == b'\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
            self.pos += 1;
        }
        ch
    }

    fn invalid_utf8_error(&self) -> ShellError {
        ShellError::parse(
            ParseErrorKind::UnexpectedToken,
            self.line,
            self.column,
            "invalid UTF-8 in shell input",
        )
    }

    fn current_utf8_char(&self) -> error::Result<(char, usize)> {
        let first = self.current_byte();
        let len = match first {
            0x00..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => return Err(self.invalid_utf8_error()),
        };
        let end = self
            .pos
            .checked_add(len)
            .filter(|end| *end <= self.input.len())
            .ok_or_else(|| self.invalid_utf8_error())?;
        let s = std::str::from_utf8(&self.input[self.pos..end])
            .map_err(|_| self.invalid_utf8_error())?;
        let ch = s.chars().next().ok_or_else(|| self.invalid_utf8_error())?;
        Ok((ch, len))
    }

    pub(crate) fn advance_char(&mut self) -> error::Result<char> {
        if self.current_byte() < 0x80 {
            return Ok(self.advance() as char);
        }

        let (ch, len) = self.current_utf8_char()?;
        self.pos += len;
        self.column += 1;
        Ok(ch)
    }

    pub(crate) fn push_current_char(&mut self, out: &mut String) -> error::Result<()> {
        let ch = self.advance_char()?;
        out.push(ch);
        Ok(())
    }

    pub(crate) fn current_span(&self) -> Span {
        Span {
            line: self.line,
            column: self.column,
        }
    }

    pub(crate) fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.current_byte() {
                b' ' | b'\t' => {
                    self.advance();
                }
                b'#' => {
                    // skip until newline (but don't consume the newline)
                    while !self.at_end() && self.current_byte() != b'\n' {
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    pub(crate) fn next_token_raw(&mut self) -> error::Result<SpannedToken> {
        self.skip_whitespace_and_comments();

        if self.at_end() {
            let span = self.current_span();
            return Ok(SpannedToken {
                token: Token::Eof,
                span,
            });
        }

        let span = self.current_span();
        match self.current_byte() {
            b'\n' => {
                self.advance();
                Ok(SpannedToken {
                    token: Token::Newline,
                    span,
                })
            }
            b'|' => self.read_pipe(),
            b'&' => self.read_amp(),
            b';' => self.read_semi(),
            b'(' => {
                let span = self.current_span();
                self.advance();
                Ok(SpannedToken {
                    token: Token::LParen,
                    span,
                })
            }
            b')' => {
                let span = self.current_span();
                self.advance();
                Ok(SpannedToken {
                    token: Token::RParen,
                    span,
                })
            }
            b'<' => self.read_less(),
            b'>' => self.read_great(),
            ch => {
                if ch.is_ascii_digit()
                    && let Some(io_num) = self.try_read_io_number()
                {
                    return Ok(SpannedToken {
                        token: io_num,
                        span,
                    });
                }
                self.read_word()
            }
        }
    }

    pub(crate) fn read_pipe(&mut self) -> error::Result<SpannedToken> {
        let span = self.current_span();
        self.advance(); // consume '|'
        if self.current_byte() == b'|' {
            self.advance();
            Ok(SpannedToken {
                token: Token::OrIf,
                span,
            })
        } else {
            Ok(SpannedToken {
                token: Token::Pipe,
                span,
            })
        }
    }

    pub(crate) fn read_amp(&mut self) -> error::Result<SpannedToken> {
        let span = self.current_span();
        self.advance(); // consume '&'
        if self.current_byte() == b'&' {
            self.advance();
            Ok(SpannedToken {
                token: Token::AndIf,
                span,
            })
        } else {
            Ok(SpannedToken {
                token: Token::Amp,
                span,
            })
        }
    }

    pub(crate) fn read_semi(&mut self) -> error::Result<SpannedToken> {
        let span = self.current_span();
        self.advance(); // consume ';'
        match self.current_byte() {
            b';' => {
                self.advance();
                Ok(SpannedToken {
                    token: Token::DSemi,
                    span,
                })
            }
            b'&' => {
                self.advance();
                Ok(SpannedToken {
                    token: Token::SemiAnd,
                    span,
                })
            }
            _ => Ok(SpannedToken {
                token: Token::Semi,
                span,
            }),
        }
    }

    pub(crate) fn read_less(&mut self) -> error::Result<SpannedToken> {
        let span = self.current_span();
        self.advance(); // consume '<'
        match self.current_byte() {
            b'<' => {
                self.advance();
                if self.current_byte() == b'-' {
                    self.advance();
                    Ok(SpannedToken {
                        token: Token::DLessDash,
                        span,
                    })
                } else {
                    Ok(SpannedToken {
                        token: Token::DLess,
                        span,
                    })
                }
            }
            b'&' => {
                self.advance();
                Ok(SpannedToken {
                    token: Token::LessAnd,
                    span,
                })
            }
            b'>' => {
                self.advance();
                Ok(SpannedToken {
                    token: Token::LessGreat,
                    span,
                })
            }
            _ => Ok(SpannedToken {
                token: Token::Less,
                span,
            }),
        }
    }

    pub(crate) fn read_great(&mut self) -> error::Result<SpannedToken> {
        let span = self.current_span();
        self.advance(); // consume '>'
        match self.current_byte() {
            b'>' => {
                self.advance();
                Ok(SpannedToken {
                    token: Token::DGreat,
                    span,
                })
            }
            b'&' => {
                self.advance();
                Ok(SpannedToken {
                    token: Token::GreatAnd,
                    span,
                })
            }
            b'|' => {
                self.advance();
                Ok(SpannedToken {
                    token: Token::Clobber,
                    span,
                })
            }
            _ => Ok(SpannedToken {
                token: Token::Great,
                span,
            }),
        }
    }

    pub(crate) fn is_meta_or_whitespace(ch: u8) -> bool {
        matches!(
            ch,
            b'|' | b'&' | b';' | b'(' | b')' | b'<' | b'>' | b' ' | b'\t' | b'\n'
        )
    }

    /// Tries to read an IO_NUMBER token (digits immediately followed by `<` or `>`).
    /// Returns None and restores state if not followed by a redirect operator.
    ///
    /// Uses the light `CursorState` snapshot (pos/line/column only) instead
    /// of the full `LexerState` (which also clones `alias_token_queue` and
    /// `expanding_aliases`). This is safe here because the only operations
    /// performed between save and restore are the byte-level `advance` and
    /// `current_byte`/`at_end` — none of which touch the alias queue,
    /// `check_alias`, or `expanding_aliases`. Those fields are only ever
    /// mutated inside `Lexer::next_token`, which is never called in this
    /// scan. Contrast with `Parser::parse_command`'s assignment lookahead
    /// (src/parser/mod.rs), which DOES call `next_token` (via
    /// `Parser::advance`) between save and restore and therefore must keep
    /// the full `save_state`/`restore_state`.
    pub(crate) fn try_read_io_number(&mut self) -> Option<Token> {
        let cursor = self.save_cursor();
        let mut digits = String::new();

        while !self.at_end() && self.current_byte().is_ascii_digit() {
            digits.push(self.current_byte() as char);
            self.advance();
        }

        let next = self.current_byte();
        if (next == b'<' || next == b'>')
            && let Ok(n) = digits.parse::<i32>()
        {
            return Some(Token::IoNumber(n));
        }

        // Not an IO_NUMBER: restore cursor only (alias state is untouched).
        self.restore_cursor(cursor);
        None
    }
}
