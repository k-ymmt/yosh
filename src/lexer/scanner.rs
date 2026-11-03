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

    /// True when `input[i..]` starts with a `\<newline>` line continuation.
    #[inline]
    fn is_line_continuation_at(&self, i: usize) -> bool {
        i + 1 < self.input.len() && self.input[i] == b'\\' && self.input[i + 1] == b'\n'
    }

    /// Index of the first byte at or after `i` that is not part of a
    /// `\<newline>` line continuation (only while joining is enabled).
    #[inline]
    fn logical_index(&self, mut i: usize) -> usize {
        if self.no_join == 0 {
            while self.is_line_continuation_at(i) {
                i += 2;
            }
        }
        i
    }

    /// The byte after the current one, skipping line continuations.
    ///
    /// Only meaningful when the current byte is not itself a backslash
    /// (the byte after an escaping `\` is taken raw — escapes consume the
    /// `\` with `advance_raw`).
    pub(crate) fn peek_byte(&self) -> u8 {
        let i = self.logical_index(self.pos + 1);
        self.input.get(i).copied().unwrap_or(0)
    }

    pub(crate) fn peek_byte2(&self) -> u8 {
        let i = self.logical_index(self.pos + 1);
        let j = self.logical_index(i + 1);
        self.input.get(j).copied().unwrap_or(0)
    }

    /// Consume one byte WITHOUT removing a following `\<newline>`.
    /// Used for the escaping backslash of `\x` (the escaped byte is taken
    /// raw), and where line continuation is not recognized (single
    /// quotes, comments, here-document bodies).
    pub(crate) fn advance_raw(&mut self) -> u8 {
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

    /// Consume one byte, then remove any `\<newline>` line continuations
    /// that follow it. POSIX §2.2.1 removes a backslash-newline before
    /// tokenization, so it may appear anywhere — inside a word, inside an
    /// operator (`&\<nl>&`), between `$` and a name, before a blank, ….
    /// Joining is suppressed while `no_join` is non-zero.
    pub(crate) fn advance(&mut self) -> u8 {
        let ch = self.advance_raw();
        self.skip_line_continuations();
        ch
    }

    /// Remove any `\<newline>` line continuations at the current position
    /// (no-op while joining is suppressed).
    #[inline]
    pub(crate) fn skip_line_continuations(&mut self) {
        if self.no_join != 0 {
            return;
        }
        while self.is_line_continuation_at(self.pos) {
            self.pos += 2;
            self.line += 1;
            self.column = 1;
        }
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
        self.skip_line_continuations();
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
        // A token may start right after a raw-consumed newline (or at the
        // start of input, or after a here-document body), so drop any
        // leading line continuations first.
        self.skip_line_continuations();
        loop {
            match self.current_byte() {
                b' ' | b'\t' => {
                    self.advance();
                }
                b'#' => {
                    // Skip until newline (but don't consume the newline).
                    // Line continuation is not recognized inside a
                    // comment: a trailing `\` does not extend it.
                    while !self.at_end() && self.current_byte() != b'\n' {
                        self.advance_raw();
                    }
                }
                _ => break,
            }
        }
    }

    pub(crate) fn next_token_raw(&mut self) -> error::Result<SpannedToken> {
        self.skip_whitespace_and_comments();

        // Lexing is a pure function of the input from here, so a token
        // that contained command substitutions (expensive: nested parse)
        // is replayed from the cache when the parser's look-ahead rewinds
        // and lexes it again.
        let start = self.pos;
        // A hit is taken out of the cache: look-ahead re-lexes a token
        // once, so this frees it early (a later re-lex just recomputes).
        if !self.token_cache.is_empty()
            && let Some(hit) = self.token_cache.remove(&start)
        {
            if hit.line == self.line && hit.column == self.column {
                (self.pos, self.line, self.column) = hit.end;
                return Ok(hit.token);
            }
            self.token_cache.insert(start, hit);
        }
        let (line, column) = (self.line, self.column);
        let subparses_before = self.subparse_count;
        let token = self.next_token_raw_uncached()?;
        if self.subparse_count != subparses_before {
            self.token_cache.insert(
                start,
                super::CachedToken {
                    line,
                    column,
                    token: token.clone(),
                    end: (self.pos, self.line, self.column),
                },
            );
        }
        Ok(token)
    }

    fn next_token_raw_uncached(&mut self) -> error::Result<SpannedToken> {
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
                // Raw: a here-document body may start right after this
                // newline and must see its first line untouched.
                self.advance_raw();
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
                // Raw: `$(...)` end-finding (`Lexer::parse_command_sub`)
                // relies on `pos` sitting right after this `)`.
                self.advance_raw();
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
    /// of the full `LexerState` (which also clones the alias-substitution
    /// state). This is safe here because the only operations performed
    /// between save and restore are the byte-level `advance` and
    /// `current_byte`/`at_end` — none of which touch the alias state
    /// (token queue, command-name position, recursion guard). It is only
    /// ever mutated inside `Lexer::next_token`, which is never called in this
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
