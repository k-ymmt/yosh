//! The parser's token cursor: the current look-ahead token, advancing
//! past it, undoable look-ahead (`snapshot_and_advance` / `rewind`), and
//! the token classifications the grammar productions branch on.

use super::Parser;
use crate::error::{self, ParseErrorKind, ShellError};
use crate::lexer::LexerState;
use crate::lexer::token::{Span, SpannedToken, Token};

/// A look-ahead snapshot taken by `Parser::snapshot_and_advance`.
pub(super) struct Snapshot {
    lexer: LexerState,
    /// The token that was current when the snapshot was taken.
    pub(super) current: SpannedToken,
    pre_current_pos: usize,
}

impl Parser {
    /// The byte position in the input up to (but not including) the
    /// current look-ahead token.
    #[cfg(test)]
    pub(super) fn consumed_bytes(&self) -> usize {
        self.pre_current_pos
    }

    pub fn current_token(&self) -> &Token {
        &self.current.token
    }

    pub(super) fn current_span(&self) -> Span {
        self.current.span
    }

    /// An `UnexpectedToken` syntax error located at the current token.
    pub(super) fn unexpected_token_error(&self, message: impl Into<String>) -> ShellError {
        let span = self.current_span();
        ShellError::parse(
            ParseErrorKind::UnexpectedToken,
            span.line,
            span.column,
            message,
        )
    }

    /// Save the lexer state and the current token for look-ahead that may
    /// have to be undone with `rewind`, then `advance` past the current
    /// token (every look-ahead starts by stepping past it, so the token
    /// is moved into the snapshot instead of cloned). On a lexer error
    /// the parser is left exactly as a failed `advance` leaves it (the
    /// current token kept), and the snapshot still rewinds to the state
    /// before the call.
    ///
    /// The full `Lexer::save_state` (not the lexer's light `CursorState`)
    /// is required: look-ahead calls `advance`, which drives
    /// `Lexer::next_token()` and can dequeue the alias token queue / flip
    /// command-name position / change the alias recursion guard when the
    /// look-ahead words came from (or trigger) alias expansion. A
    /// cursor-only snapshot would lose that state and corrupt subsequent
    /// alias expansion. See `Lexer::try_read_io_number` for the
    /// contrasting call site where the light snapshot IS safe.
    ///
    /// `pre_current_pos` is saved too, so `consumed_bytes` after a rewind
    /// still names the start of the restored look-ahead token.
    pub(super) fn snapshot_and_advance(&mut self) -> (Snapshot, error::Result<()>) {
        let lexer = self.lexer.save_state();
        let pre_current_pos = self.pre_current_pos;
        self.pre_current_pos = self.lexer.position();
        let (current, result) = match self.lexer.next_token() {
            Ok(next) => (std::mem::replace(&mut self.current, next), Ok(())),
            Err(e) => (self.current.clone(), Err(e)),
        };
        let snapshot = Snapshot {
            lexer,
            current,
            pre_current_pos,
        };
        (snapshot, result)
    }

    /// Return to a state saved by `snapshot`.
    pub(super) fn rewind(&mut self, snapshot: Snapshot) {
        self.lexer.restore_state(snapshot.lexer);
        self.current = snapshot.current;
        self.pre_current_pos = snapshot.pre_current_pos;
    }

    pub fn advance(&mut self) -> error::Result<()> {
        self.pre_current_pos = self.lexer.position();
        self.current = self.lexer.next_token()?;
        Ok(())
    }

    /// Advance if current token matches expected, returns true if matched.
    pub(super) fn eat(&mut self, expected: &Token) -> error::Result<bool> {
        if self.current.token == *expected {
            self.advance()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Advance if current token is reserved word matching keyword, else error.
    pub(super) fn expect_reserved(&mut self, keyword: &str) -> error::Result<()> {
        if self.current.token.matches_keyword(keyword) {
            self.advance()?;
            Ok(())
        } else {
            Err(self
                .unexpected_token_error(format!("expected '{}', got unexpected token", keyword)))
        }
    }

    /// The current token is a newline: read any here-document bodies that
    /// start after it, without consuming the newline itself.
    pub(super) fn read_heredocs_at_newline(&mut self) -> error::Result<()> {
        if self.lexer.has_pending_heredocs() {
            self.lexer.process_pending_heredocs()?;
        }
        Ok(())
    }

    /// Consume all consecutive Newline tokens.
    pub fn skip_newlines(&mut self) -> error::Result<()> {
        while self.current.token == Token::Newline {
            // Pending heredoc bodies start right after this newline, so they
            // must be read BEFORE advancing — advancing first would lex the
            // first body line as an ordinary token.
            self.read_heredocs_at_newline()?;
            self.advance()?;
        }
        Ok(())
    }

    pub fn is_at_end(&self) -> bool {
        self.current.token == Token::Eof && self.init_error.is_none()
    }

    pub(super) fn is_reserved(&self, keyword: &str) -> bool {
        self.current.token.matches_keyword(keyword)
    }

    /// Returns true when we've reached a token that ends a complete command.
    pub(super) fn is_complete_command_end(&self) -> bool {
        match &self.current.token {
            Token::Eof | Token::RParen | Token::DSemi | Token::SemiAnd => true,
            Token::Word(word) => word.as_literal().is_some_and(is_closing_reserved_word),
            _ => false,
        }
    }

    pub(super) fn is_compound_command_start(&self) -> bool {
        match &self.current.token {
            Token::LParen => true,
            Token::Word(word) => matches!(
                word.as_literal(),
                Some("if" | "for" | "while" | "until" | "case" | "{")
            ),
            _ => false,
        }
    }
}

/// Reserved words that close a compound command or one of its parts, and
/// so may end a command list without a separator (`{ :; }`, `if a; then
/// b; fi`, ...).
#[inline]
pub(super) fn is_closing_reserved_word(word: &str) -> bool {
    matches!(
        word,
        "}" | "fi" | "done" | "esac" | "then" | "else" | "elif" | "do"
    )
}

/// Short source-like rendering of a token for syntax error messages.
pub(super) fn token_display(token: &Token) -> String {
    let s = match token {
        Token::Word(w) => return w.as_literal().unwrap_or("word").to_string(),
        Token::IoNumber(n) => return n.to_string(),
        Token::Newline => "newline",
        Token::Eof => "end of file",
        Token::Pipe => "|",
        Token::AndIf => "&&",
        Token::OrIf => "||",
        Token::Semi => ";",
        Token::Amp => "&",
        Token::DSemi => ";;",
        Token::SemiAnd => ";&",
        Token::Less => "<",
        Token::Great => ">",
        Token::DLess => "<<",
        Token::DGreat => ">>",
        Token::LessAnd => "<&",
        Token::GreatAnd => ">&",
        Token::LessGreat => "<>",
        Token::DLessDash => "<<-",
        Token::Clobber => ">|",
        Token::LParen => "(",
        Token::RParen => ")",
    };
    s.to_string()
}

#[cfg(test)]
mod tests {
    use crate::lexer::token::Token;
    use crate::parser::Parser;
    use crate::parser::ast::Word;

    #[test]
    fn rewind_restores_consumed_bytes() {
        let mut p = Parser::new("a=1 b=2 echo x\n");
        p.advance().unwrap();
        let before = p.consumed_bytes();
        assert_eq!(before, 3, "look-ahead `b=2` starts after `a=1`");
        let (saved, advanced) = p.snapshot_and_advance();
        advanced.unwrap();
        p.advance().unwrap();
        assert_ne!(p.consumed_bytes(), before);
        p.rewind(saved);
        assert_eq!(p.consumed_bytes(), before);
        assert_eq!(p.current_token(), &Token::Word(Word::literal("b=2")));
    }
}
