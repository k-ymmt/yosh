use crate::parser::ast::Word;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Span {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpannedToken {
    pub token: Token,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Word(Word),
    IoNumber(i32),
    Newline,
    Eof,
    Pipe,
    AndIf,
    OrIf,
    Semi,
    Amp,
    DSemi,
    SemiAnd,
    Less,
    Great,
    DLess,
    DGreat,
    LessAnd,
    GreatAnd,
    LessGreat,
    DLessDash,
    Clobber,
    LParen,
    RParen,
}

impl Token {
    pub fn is_reserved_word(&self, keyword: &str) -> bool {
        if let Token::Word(w) = self {
            w.as_literal() == Some(keyword)
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{Word, WordPart};

    #[test]
    fn test_is_reserved_word_literal() {
        let tok = Token::Word(Word::literal("if"));
        assert!(tok.is_reserved_word("if"));
        assert!(!tok.is_reserved_word("then"));
    }

    #[test]
    fn test_is_reserved_word_quoted_not_reserved() {
        let tok = Token::Word(Word {
            parts: vec![WordPart::SingleQuoted("if".to_string())],
        });
        assert!(!tok.is_reserved_word("if"));
    }

    #[test]
    fn test_is_reserved_word_non_word_token() {
        assert!(!Token::Pipe.is_reserved_word("if"));
    }
}
