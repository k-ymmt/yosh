//! Nested programs: the bodies of `$(...)` and backquoted command
//! substitutions, which the lexer hands to a fresh parser.

use super::Parser;
use super::ast::{self, Program};
use crate::error::{self, ParseErrorKind, ShellError, ShellErrorKind};
use crate::lexer::Lexer;
use crate::lexer::token::{Span, Token};

impl Parser {
    /// Build a parser over a nested lexer, propagating an error from the
    /// first token instead of deferring it. `depth` is the starting
    /// compound nesting: 1 for a `$(...)` body, whose commands may be
    /// followed directly by the closing `)`.
    fn from_nested_lexer(mut lexer: Lexer, depth: usize) -> error::Result<Self> {
        let current = lexer.next_token()?;
        Ok(Parser {
            lexer,
            current,
            pre_current_pos: 0,
            init_error: None,
            depth,
        })
    }

    /// Parse a whole nested program (the body of a backquoted command
    /// substitution, already unescaped into its own input).
    pub(crate) fn parse_nested_program(lexer: Lexer) -> error::Result<Program> {
        Self::from_nested_lexer(lexer, 0)?.parse_program()
    }

    /// Parse the body of a `$(...)` command substitution from `lexer`
    /// (positioned just after the opening `(`) up to the `)` that closes
    /// it, using the full grammar so the closing parenthesis is found the
    /// way dash/ksh find it. Returns the program and the lexer, whose
    /// position is just past the closing `)`.
    ///
    /// Reaching end of input — directly or as a syntax error at EOF
    /// (`$(if true`) — is reported as an unterminated command
    /// substitution anchored at `span`, so interactive input continues
    /// on the next line.
    pub(crate) fn parse_command_sub_body(
        lexer: Lexer,
        span: Span,
    ) -> error::Result<(Program, Lexer)> {
        let unterminated = || {
            ShellError::parse(
                ParseErrorKind::UnterminatedCommandSub,
                span.line,
                span.column,
                "unterminated command substitution",
            )
        };
        let mut parser = Self::from_nested_lexer(lexer, 1)?;
        match parser.parse_until_rparen() {
            Ok(commands) if parser.current.token == Token::RParen => {
                Ok((Program { commands }, parser.lexer))
            }
            Ok(_) => Err(unterminated()),
            Err(e)
                if parser.is_at_end()
                    && matches!(
                        e.kind,
                        ShellErrorKind::Parse(
                            ParseErrorKind::UnexpectedToken | ParseErrorKind::UnexpectedEof
                        )
                    ) =>
            {
                Err(unterminated())
            }
            Err(e) => Err(e),
        }
    }

    fn parse_until_rparen(&mut self) -> error::Result<Vec<ast::CompleteCommand>> {
        self.skip_newlines()?;
        let mut commands = Vec::new();
        while !self.is_at_end() && self.current.token != Token::RParen {
            commands.push(self.parse_complete_command()?);
            self.skip_newlines()?;
        }
        Ok(commands)
    }
}
