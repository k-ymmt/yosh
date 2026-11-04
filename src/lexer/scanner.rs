use super::Lexer;
use super::token::{Span, SpannedToken, Token};
use crate::error::{self, ParseErrorKind, ShellError};

/// A set of bytes that `Lexer::push_plain_run` may copy in bulk into a
/// literal: bytes the word loops would otherwise push one at a time as
/// ordinary characters. A set holds only ASCII bytes other than `\` and
/// newline (checked at compile time when the set is built).
pub(crate) struct PlainBytes([bool; 256]);

impl PlainBytes {
    /// Every printable non-blank ASCII byte, plus `extra`, minus `stop`.
    const fn new(extra: &[u8], stop: &[u8]) -> Self {
        let mut set = [false; 256];
        let mut b = 0x21;
        while b <= 0x7e {
            set[b] = true;
            b += 1;
        }
        let mut i = 0;
        while i < extra.len() {
            set[extra[i] as usize] = true;
            i += 1;
        }
        let mut i = 0;
        while i < stop.len() {
            set[stop[i] as usize] = false;
            i += 1;
        }
        assert!(!set[b'\\' as usize] && !set[b'\n' as usize]);
        let mut b = 0x80;
        while b < 256 {
            assert!(!set[b]);
            b += 1;
        }
        PlainBytes(set)
    }
}

/// Ordinary bytes of an unquoted word: not a metacharacter or blank, a
/// quote, `\`, `$` or `` ` ``, and not `}` (which ends the word of a
/// `${x-word}` expansion).
pub(crate) static UNQUOTED_PLAIN: PlainBytes = PlainBytes::new(&[], b"|&;()<>'\"\\$`}");

/// Ordinary bytes inside double quotes, where blanks and metacharacters
/// are literal: all but `"`, `\`, `$`, `` ` `` and `}`.
pub(crate) static DQUOTED_PLAIN: PlainBytes = PlainBytes::new(b" \t", b"\"\\$`}");

/// Ordinary bytes inside single quotes: all but the closing `'` (and,
/// as in every set, `\` and newline, which take the per-byte path).
pub(crate) static SQUOTED_PLAIN: PlainBytes = PlainBytes::new(b" \t", b"'\\");

impl Lexer {
    pub(crate) fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    pub(crate) fn current_byte(&self) -> u8 {
        if self.at_end() {
            0
        } else {
            self.input.as_bytes()[self.pos]
        }
    }

    /// True when `input[i..]` starts with a `\<newline>` line continuation.
    #[inline]
    fn is_line_continuation_at(&self, i: usize) -> bool {
        i + 1 < self.input.len()
            && self.input.as_bytes()[i] == b'\\'
            && self.input.as_bytes()[i + 1] == b'\n'
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
        self.input.as_bytes().get(i).copied().unwrap_or(0)
    }

    pub(crate) fn peek_byte2(&self) -> u8 {
        let i = self.logical_index(self.pos + 1);
        let j = self.logical_index(i + 1);
        self.input.as_bytes().get(j).copied().unwrap_or(0)
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
        let s = self
            .input
            .get(self.pos..end)
            .ok_or_else(|| self.invalid_utf8_error())?;
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

    /// Append the current character to `out` like `push_current_char`,
    /// together with the run of bytes after it that `plain` accepts, in
    /// one slice copy instead of one `advance` per byte.
    ///
    /// `plain` only accepts ASCII bytes other than `\` and newline (see
    /// [`PlainBytes`]): each advances the column by one, a `\<newline>`
    /// line continuation can only follow the last byte of the run, and
    /// the run starts and ends on character boundaries. A byte `plain`
    /// rejects (or a non-ASCII character) ends the run, and the caller's
    /// loop classifies it as before.
    #[inline]
    pub(crate) fn push_plain_run(
        &mut self,
        out: &mut String,
        plain: &PlainBytes,
    ) -> error::Result<()> {
        let rest = &self.input.as_bytes()[self.pos..];
        let n = rest
            .iter()
            .position(|&b| !plain.0[b as usize])
            .unwrap_or(rest.len());
        if n == 0 {
            return self.push_current_char(out);
        }
        out.push_str(&self.input[self.pos..self.pos + n]);
        self.pos += n;
        self.column += n;
        self.skip_line_continuations();
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
            b'|' => Ok(self.read_pipe()),
            b'&' => Ok(self.read_amp()),
            b';' => Ok(self.read_semi()),
            b'(' => Ok(self.read_operator(|_| Token::LParen)),
            b')' => {
                // Raw: `$(...)` end-finding (`Lexer::parse_command_sub`)
                // relies on `pos` sitting right after this `)`.
                self.advance_raw();
                Ok(SpannedToken {
                    token: Token::RParen,
                    span,
                })
            }
            b'<' => Ok(self.read_less()),
            b'>' => Ok(self.read_great()),
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

    /// Consume the current byte if it is `b`.
    #[inline]
    fn eat_byte(&mut self, b: u8) -> bool {
        if self.current_byte() == b {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Read an operator token: consume its first byte, then let `rest`
    /// consume any further bytes and pick the token.
    #[inline]
    fn read_operator(&mut self, rest: impl FnOnce(&mut Self) -> Token) -> SpannedToken {
        let span = self.current_span();
        self.advance(); // consume the first byte
        SpannedToken {
            token: rest(self),
            span,
        }
    }

    /// `|` or `||`.
    fn read_pipe(&mut self) -> SpannedToken {
        self.read_operator(|lx| {
            if lx.eat_byte(b'|') {
                Token::OrIf
            } else {
                Token::Pipe
            }
        })
    }

    /// `&` or `&&`.
    fn read_amp(&mut self) -> SpannedToken {
        self.read_operator(|lx| {
            if lx.eat_byte(b'&') {
                Token::AndIf
            } else {
                Token::Amp
            }
        })
    }

    /// `;`, `;;` or `;&`.
    fn read_semi(&mut self) -> SpannedToken {
        self.read_operator(|lx| {
            if lx.eat_byte(b';') {
                Token::DSemi
            } else if lx.eat_byte(b'&') {
                Token::SemiAnd
            } else {
                Token::Semi
            }
        })
    }

    /// `<`, `<<`, `<<-`, `<&` or `<>`.
    fn read_less(&mut self) -> SpannedToken {
        self.read_operator(|lx| {
            if lx.eat_byte(b'<') {
                if lx.eat_byte(b'-') {
                    Token::DLessDash
                } else {
                    Token::DLess
                }
            } else if lx.eat_byte(b'&') {
                Token::LessAnd
            } else if lx.eat_byte(b'>') {
                Token::LessGreat
            } else {
                Token::Less
            }
        })
    }

    /// `>`, `>>`, `>&` or `>|`.
    fn read_great(&mut self) -> SpannedToken {
        self.read_operator(|lx| {
            if lx.eat_byte(b'>') {
                Token::DGreat
            } else if lx.eat_byte(b'&') {
                Token::GreatAnd
            } else if lx.eat_byte(b'|') {
                Token::Clobber
            } else {
                Token::Great
            }
        })
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
    /// scan. Contrast with the parser's assignment lookahead
    /// (`try_parse_prefixed_compound` in src/parser/pipeline.rs, snapshots
    /// in src/parser/cursor.rs), which DOES call `next_token` (via
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
