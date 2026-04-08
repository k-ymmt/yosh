pub mod token;

use crate::error::{self, ShellError, ShellErrorKind};
use crate::parser::ast::{Word, WordPart};
use token::{Span, SpannedToken, Token};

pub struct Lexer {
    input: Vec<u8>,
    pos: usize,
    line: usize,
    column: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            input: input.as_bytes().to_vec(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn current_byte(&self) -> u8 {
        if self.at_end() {
            0
        } else {
            self.input[self.pos]
        }
    }

    fn peek_byte(&self) -> u8 {
        if self.pos + 1 >= self.input.len() {
            0
        } else {
            self.input[self.pos + 1]
        }
    }

    fn advance(&mut self) -> u8 {
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

    fn current_span(&self) -> Span {
        Span {
            line: self.line,
            column: self.column,
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
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

    pub fn next_token(&mut self) -> error::Result<SpannedToken> {
        self.skip_whitespace_and_comments();

        if self.at_end() {
            let span = self.current_span();
            return Ok(SpannedToken {
                token: Token::Eof,
                span,
            });
        }

        match self.current_byte() {
            b'\n' => {
                let span = self.current_span();
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
            _ => self.read_word(),
        }
    }

    fn read_pipe(&mut self) -> error::Result<SpannedToken> {
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

    fn read_amp(&mut self) -> error::Result<SpannedToken> {
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

    fn read_semi(&mut self) -> error::Result<SpannedToken> {
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

    fn read_less(&mut self) -> error::Result<SpannedToken> {
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

    fn read_great(&mut self) -> error::Result<SpannedToken> {
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

    fn is_meta_or_whitespace(ch: u8) -> bool {
        matches!(
            ch,
            b'|' | b'&' | b';' | b'(' | b')' | b'<' | b'>' | b' ' | b'\t' | b'\n'
        )
    }

    // ---- Task 5: word scanning with quoting ----

    /// Main word-part reader that loops over input, dispatching to quoting methods.
    /// `in_double_quote`: true when inside "..."
    /// `end_byte`: if Some(b'}'), stop at unquoted `}` (for ${...} parsing)
    pub fn read_word_parts(
        &mut self,
        in_double_quote: bool,
        end_byte: Option<u8>,
    ) -> error::Result<Vec<WordPart>> {
        let mut parts: Vec<WordPart> = Vec::new();
        let mut literal = String::new();

        loop {
            if self.at_end() {
                break;
            }

            let ch = self.current_byte();

            // check end_byte (e.g. '}' for ${...})
            if let Some(end) = end_byte {
                if ch == end {
                    break;
                }
            }

            if in_double_quote {
                // inside double quotes: stop at closing "
                if ch == b'"' {
                    break;
                }
                match ch {
                    b'$' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        let part = self.read_dollar()?;
                        parts.push(part);
                    }
                    b'\\' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        let part = self.read_backslash_in_double_quote()?;
                        parts.push(part);
                    }
                    b'`' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        let part = self.read_backtick()?;
                        parts.push(part);
                    }
                    _ => {
                        literal.push(ch as char);
                        self.advance();
                    }
                }
            } else {
                // unquoted context
                if Self::is_meta_or_whitespace(ch) {
                    break;
                }
                match ch {
                    b'\'' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        let part = self.read_single_quote()?;
                        parts.push(part);
                    }
                    b'"' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        let part = self.read_double_quote()?;
                        parts.push(part);
                    }
                    b'\\' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        let part = self.read_backslash()?;
                        parts.push(part);
                    }
                    b'$' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        let part = self.read_dollar()?;
                        parts.push(part);
                    }
                    b'`' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        let part = self.read_backtick()?;
                        parts.push(part);
                    }
                    b'~' if parts.is_empty() && literal.is_empty() => {
                        let part = self.read_tilde();
                        parts.push(part);
                    }
                    _ => {
                        literal.push(ch as char);
                        self.advance();
                    }
                }
            }
        }

        if !literal.is_empty() {
            parts.push(WordPart::Literal(literal));
        }

        // filter out empty Literal("") parts (from line continuations)
        let parts = parts
            .into_iter()
            .filter(|p| !matches!(p, WordPart::Literal(s) if s.is_empty()))
            .collect();

        Ok(parts)
    }

    fn read_single_quote(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // consume opening '
        let mut content = String::new();
        loop {
            if self.at_end() {
                return Err(ShellError::new(
                    ShellErrorKind::UnterminatedSingleQuote,
                    span.line,
                    span.column,
                    "unterminated single quote",
                ));
            }
            let ch = self.current_byte();
            if ch == b'\'' {
                self.advance(); // consume closing '
                break;
            }
            content.push(ch as char);
            self.advance();
        }
        Ok(WordPart::SingleQuoted(content))
    }

    fn read_double_quote(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // consume opening "
        let inner = self.read_word_parts(true, None)?;
        if self.at_end() {
            return Err(ShellError::new(
                ShellErrorKind::UnterminatedDoubleQuote,
                span.line,
                span.column,
                "unterminated double quote",
            ));
        }
        self.advance(); // consume closing "
        Ok(WordPart::DoubleQuoted(inner))
    }

    /// Handles `\` outside double quotes.
    /// `\<newline>` is line continuation (returns empty Literal, filtered later).
    /// Otherwise returns literal of next char.
    fn read_backslash(&mut self) -> error::Result<WordPart> {
        self.advance(); // consume '\'
        if self.at_end() {
            return Ok(WordPart::Literal("\\".to_string()));
        }
        let ch = self.current_byte();
        if ch == b'\n' {
            // line continuation: consume newline, return empty literal (filtered later)
            self.advance();
            Ok(WordPart::Literal(String::new()))
        } else {
            self.advance();
            Ok(WordPart::Literal((ch as char).to_string()))
        }
    }

    /// Inside double quotes, `\` only escapes `$ ` " \ newline`.
    fn read_backslash_in_double_quote(&mut self) -> error::Result<WordPart> {
        self.advance(); // consume '\'
        if self.at_end() {
            return Ok(WordPart::Literal("\\".to_string()));
        }
        let ch = self.current_byte();
        match ch {
            b'$' | b'`' | b'"' | b'\\' => {
                self.advance();
                Ok(WordPart::Literal((ch as char).to_string()))
            }
            b'\n' => {
                // line continuation
                self.advance();
                Ok(WordPart::Literal(String::new()))
            }
            _ => {
                // backslash is kept literally
                Ok(WordPart::Literal(format!("\\{}", ch as char)))
            }
        }
    }

    /// Handles `$'...'` with C-style escape sequences.
    /// Called after `$` is consumed; current byte is `'`.
    fn read_dollar_single_quote(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // consume opening '
        let mut content = String::new();
        loop {
            if self.at_end() {
                return Err(ShellError::new(
                    ShellErrorKind::UnterminatedDollarSingleQuote,
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
                    content.push('\\');
                    break;
                }
                let esc = self.current_byte();
                self.advance();
                match esc {
                    b'a' => content.push('\x07'),
                    b'b' => content.push('\x08'),
                    b'e' | b'E' => content.push('\x1B'),
                    b'f' => content.push('\x0C'),
                    b'n' => content.push('\n'),
                    b'r' => content.push('\r'),
                    b't' => content.push('\t'),
                    b'v' => content.push('\x0B'),
                    b'\\' => content.push('\\'),
                    b'\'' => content.push('\''),
                    b'"' => content.push('"'),
                    b'x' => {
                        let val = self.read_hex_digits(2);
                        content.push(val as char);
                    }
                    b'c' => {
                        // \cX — control character
                        if !self.at_end() {
                            let ctrl = self.current_byte();
                            self.advance();
                            content.push((ctrl & 0x1f) as char);
                        }
                    }
                    b'0'..=b'7' => {
                        // octal: up to 3 digits, first digit already consumed
                        let mut val = (esc - b'0') as u32;
                        for _ in 0..2 {
                            let next = self.current_byte();
                            if next >= b'0' && next <= b'7' {
                                val = val * 8 + (next - b'0') as u32;
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        let c = char::from_u32(val).unwrap_or('\u{FFFD}');
                        content.push(c);
                    }
                    _ => {
                        // unknown escape: keep backslash and char
                        content.push('\\');
                        content.push(esc as char);
                    }
                }
            } else {
                content.push(ch as char);
                self.advance();
            }
        }
        Ok(WordPart::DollarSingleQuoted(content))
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

    /// Reads `~` or `~user` at word start.
    fn read_tilde(&mut self) -> WordPart {
        self.advance(); // consume '~'
        let mut username = String::new();
        // read until metachar, whitespace, or '/'
        while !self.at_end() {
            let ch = self.current_byte();
            if Self::is_meta_or_whitespace(ch) || ch == b'/' {
                break;
            }
            username.push(ch as char);
            self.advance();
        }
        if username.is_empty() {
            WordPart::Tilde(None)
        } else {
            WordPart::Tilde(Some(username))
        }
    }

    /// Handles `$`. Currently only handles `$'...'`; all other forms return literal `"$"`.
    /// Task 6 will implement full dollar expansion.
    fn read_dollar(&mut self) -> error::Result<WordPart> {
        self.advance(); // consume '$'
        if !self.at_end() && self.current_byte() == b'\'' {
            return self.read_dollar_single_quote();
        }
        // All other dollar forms: return literal "$" for now
        Ok(WordPart::Literal("$".to_string()))
    }

    /// Placeholder: returns literal "`".
    fn read_backtick(&mut self) -> error::Result<WordPart> {
        self.advance(); // consume '`'
        Ok(WordPart::Literal("`".to_string()))
    }

    fn read_word(&mut self) -> error::Result<SpannedToken> {
        let span = self.current_span();
        let parts = self.read_word_parts(false, None)?;
        if parts.is_empty() {
            return Err(ShellError::new(
                ShellErrorKind::UnexpectedToken,
                span.line,
                span.column,
                format!("unexpected character: '{}'", self.current_byte() as char),
            ));
        }
        Ok(SpannedToken {
            token: Token::Word(Word { parts }),
            span,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::WordPart;

    fn tokenize(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let st = lexer.next_token().unwrap();
            if st.token == Token::Eof {
                break;
            }
            tokens.push(st.token);
        }
        tokens
    }

    // ---- Task 4 tests ----

    #[test]
    fn test_empty_input() {
        assert_eq!(tokenize(""), vec![]);
    }

    #[test]
    fn test_newline() {
        assert_eq!(tokenize("\n"), vec![Token::Newline]);
    }

    #[test]
    fn test_single_char_operators() {
        assert_eq!(
            tokenize("| ; & ( )"),
            vec![Token::Pipe, Token::Semi, Token::Amp, Token::LParen, Token::RParen]
        );
    }

    #[test]
    fn test_multi_char_operators() {
        assert_eq!(
            tokenize("&& || ;; ;&"),
            vec![Token::AndIf, Token::OrIf, Token::DSemi, Token::SemiAnd]
        );
    }

    #[test]
    fn test_redirect_operators() {
        assert_eq!(
            tokenize("< > >> <& >& <> >|"),
            vec![
                Token::Less,
                Token::Great,
                Token::DGreat,
                Token::LessAnd,
                Token::GreatAnd,
                Token::LessGreat,
                Token::Clobber
            ]
        );
    }

    #[test]
    fn test_heredoc_operators() {
        assert_eq!(tokenize("<< <<-"), vec![Token::DLess, Token::DLessDash]);
    }

    #[test]
    fn test_comment_ignored() {
        assert_eq!(tokenize("# this is a comment\n"), vec![Token::Newline]);
    }

    #[test]
    fn test_whitespace_only() {
        assert_eq!(tokenize("   \t  "), vec![]);
    }

    // ---- Task 5 tests ----

    #[test]
    fn test_unquoted_words() {
        let tokens = tokenize("echo hello world");
        assert_eq!(
            tokens,
            vec![
                Token::Word(Word::literal("echo")),
                Token::Word(Word::literal("hello")),
                Token::Word(Word::literal("world")),
            ]
        );
    }

    #[test]
    fn test_single_quoted_word() {
        let tokens = tokenize("echo 'hello world'");
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![WordPart::SingleQuoted("hello world".to_string())]
            })
        );
    }

    #[test]
    fn test_double_quoted_word() {
        let tokens = tokenize("echo \"hello world\"");
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![WordPart::DoubleQuoted(vec![WordPart::Literal(
                    "hello world".to_string()
                )])],
            })
        );
    }

    #[test]
    fn test_backslash_escape() {
        let tokens = tokenize("echo hello\\ world");
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![
                    WordPart::Literal("hello".to_string()),
                    WordPart::Literal(" ".to_string()),
                    WordPart::Literal("world".to_string()),
                ],
            })
        );
    }

    #[test]
    fn test_line_continuation() {
        let tokens = tokenize("echo hel\\\nlo");
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![
                    WordPart::Literal("hel".to_string()),
                    WordPart::Literal("lo".to_string())
                ],
            })
        );
    }

    #[test]
    fn test_dollar_single_quote() {
        let tokens = tokenize("echo $'hello\\nworld'");
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![WordPart::DollarSingleQuoted("hello\nworld".to_string())],
            })
        );
    }

    #[test]
    fn test_dollar_single_quote_escapes() {
        let tokens = tokenize("$'\\t\\r\\a\\b\\\\\\\"\\''");
        assert_eq!(tokens.len(), 1);
        assert_eq!(
            tokens[0],
            Token::Word(Word {
                parts: vec![WordPart::DollarSingleQuoted(
                    "\t\r\x07\x08\\\"'".to_string()
                )],
            })
        );
    }

    #[test]
    fn test_mixed_quoting_in_word() {
        let tokens = tokenize("he\"ll\"o");
        assert_eq!(tokens.len(), 1);
        assert_eq!(
            tokens[0],
            Token::Word(Word {
                parts: vec![
                    WordPart::Literal("he".to_string()),
                    WordPart::DoubleQuoted(vec![WordPart::Literal("ll".to_string())]),
                    WordPart::Literal("o".to_string()),
                ],
            })
        );
    }

    #[test]
    fn test_unterminated_single_quote() {
        let mut lexer = Lexer::new("echo 'hello");
        let _ = lexer.next_token().unwrap();
        let err = lexer.next_token().unwrap_err();
        assert_eq!(err.kind, ShellErrorKind::UnterminatedSingleQuote);
    }

    #[test]
    fn test_unterminated_double_quote() {
        let mut lexer = Lexer::new("echo \"hello");
        let _ = lexer.next_token().unwrap();
        let err = lexer.next_token().unwrap_err();
        assert_eq!(err.kind, ShellErrorKind::UnterminatedDoubleQuote);
    }
}
