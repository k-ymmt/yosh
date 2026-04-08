pub mod token;

use crate::error::{self, ShellError, ShellErrorKind};
use crate::parser::ast::Word;
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

    /// Temporary placeholder: reads unquoted characters until metachar/whitespace.
    /// Will be replaced in Task 5.
    fn read_word(&mut self) -> error::Result<SpannedToken> {
        let span = self.current_span();
        let mut s = String::new();
        while !self.at_end() && !Self::is_meta_or_whitespace(self.current_byte()) {
            s.push(self.current_byte() as char);
            self.advance();
        }
        if s.is_empty() {
            return Err(ShellError::new(
                ShellErrorKind::UnexpectedToken,
                span.line,
                span.column,
                format!("unexpected character: '{}'", self.current_byte() as char),
            ));
        }
        Ok(SpannedToken {
            token: Token::Word(Word::literal(&s)),
            span,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
