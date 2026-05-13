pub mod ast;
mod compound;
mod function;
mod redirect;
mod simple;
pub(crate) mod word;

use crate::error::{self, ParseErrorKind, ShellError};
use crate::lexer::Lexer;
use crate::lexer::token::{Span, SpannedToken, Token};
use ast::{AndOrList, AndOrOp, Command, CompleteCommand, Pipeline, Program, SeparatorOp};

pub struct Parser {
    lexer: Lexer,
    current: SpannedToken,
    /// Lexer position before the current look-ahead token was read.
    pre_current_pos: usize,
}

impl Parser {
    pub fn new(input: &str) -> Self {
        let mut lexer = Lexer::new(input);
        // Read first token; on error use Eof
        let current = lexer.next_token().unwrap_or(SpannedToken {
            token: Token::Eof,
            span: Span::default(),
        });
        Self {
            lexer,
            current,
            pre_current_pos: 0,
        }
    }

    pub fn new_with_aliases(input: &str, aliases: &crate::env::aliases::AliasStore) -> Self {
        let mut lexer = Lexer::new_with_aliases(input, aliases);
        let current = lexer.next_token().unwrap_or(SpannedToken {
            token: Token::Eof,
            span: Span::default(),
        });
        Self {
            lexer,
            current,
            pre_current_pos: 0,
        }
    }

    /// Like `new_with_aliases` but the lexer's line counter starts at `start_line`
    /// instead of 1. Used when a script is split into chunks for incremental parsing
    /// so that each chunk reports the correct source-file line number.
    pub fn new_with_aliases_at_line(
        input: &str,
        aliases: &crate::env::aliases::AliasStore,
        start_line: usize,
    ) -> Self {
        let mut lexer = Lexer::new_with_aliases_at_line(input, aliases, start_line);
        let current = lexer.next_token().unwrap_or(SpannedToken {
            token: Token::Eof,
            span: Span::default(),
        });
        Self {
            lexer,
            current,
            pre_current_pos: 0,
        }
    }

    /// Returns the byte position in the input up to (but not including) the current
    /// look-ahead token. This is useful for incremental parsing.
    pub fn consumed_bytes(&self) -> usize {
        self.pre_current_pos
    }

    pub fn current_token(&self) -> &Token {
        &self.current.token
    }

    pub(super) fn current_span(&self) -> Span {
        self.current.span
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
            let span = self.current_span();
            Err(ShellError::parse(
                ParseErrorKind::UnexpectedToken,
                span.line,
                span.column,
                format!("expected '{}', got unexpected token", keyword),
            ))
        }
    }

    /// Consume all consecutive Newline tokens.
    pub(super) fn skip_newlines(&mut self) -> error::Result<()> {
        while self.current.token == Token::Newline {
            self.advance()?;
            if self.lexer.has_pending_heredocs() {
                self.lexer.process_pending_heredocs()?;
            }
        }
        Ok(())
    }

    pub fn is_at_end(&self) -> bool {
        self.current.token == Token::Eof
    }

    pub(super) fn is_reserved(&self, keyword: &str) -> bool {
        self.current.token.matches_keyword(keyword)
    }

    // ---- Grammar productions ----

    pub fn parse_program(&mut self) -> error::Result<Program> {
        self.skip_newlines()?;
        let mut commands = Vec::new();
        while !self.is_at_end() {
            let cmd = self.parse_complete_command()?;
            commands.push(cmd);
            self.skip_newlines()?;
        }
        Ok(Program { commands })
    }

    pub fn parse_complete_command(&mut self) -> error::Result<CompleteCommand> {
        let mut items = Vec::new();

        let first_aol = self.parse_and_or()?;
        let was_newline = self.current.token == Token::Newline;
        let sep = self.parse_separator_op()?;
        let ended = sep.is_none() || was_newline;
        items.push((first_aol, sep));

        if !ended {
            // Continue parsing while there are more and_or lists separated by ; or &
            loop {
                if self.is_at_end() || self.is_complete_command_end() {
                    break;
                }
                if self.current.token == Token::Newline {
                    break;
                }
                let aol = self.parse_and_or()?;
                let was_newline = self.current.token == Token::Newline;
                let sep = self.parse_separator_op()?;
                let ended = sep.is_none() || was_newline;
                items.push((aol, sep));
                if ended {
                    break;
                }
            }
        }

        Ok(CompleteCommand { items })
    }

    /// Parse separator: ; → Semi, & → Amp, Newline → Semi (as terminator)
    /// Returns None if no separator found.
    pub(super) fn parse_separator_op(&mut self) -> error::Result<Option<SeparatorOp>> {
        match self.current.token {
            Token::Semi => {
                self.advance()?;
                Ok(Some(SeparatorOp::Semi))
            }
            Token::Amp => {
                self.advance()?;
                Ok(Some(SeparatorOp::Amp))
            }
            Token::Newline => {
                self.advance()?;
                if self.lexer.has_pending_heredocs() {
                    self.lexer.process_pending_heredocs()?;
                }
                Ok(Some(SeparatorOp::Semi))
            }
            _ => Ok(None),
        }
    }

    pub(super) fn parse_and_or(&mut self) -> error::Result<AndOrList> {
        let first = self.parse_pipeline()?;
        let mut rest = Vec::new();

        loop {
            let op = match &self.current.token {
                Token::AndIf => AndOrOp::And,
                Token::OrIf => AndOrOp::Or,
                _ => break,
            };
            self.advance()?;
            self.skip_newlines()?;
            let pipeline = self.parse_pipeline()?;
            rest.push((op, pipeline));
        }

        Ok(AndOrList { first, rest })
    }

    pub(super) fn parse_pipeline(&mut self) -> error::Result<Pipeline> {
        let negated = if self.is_reserved("!") {
            self.advance()?;
            true
        } else {
            false
        };

        let mut commands = Vec::new();
        commands.push(self.parse_command()?);

        while self.current.token == Token::Pipe {
            self.advance()?;
            self.skip_newlines()?;
            commands.push(self.parse_command()?);
        }

        // Fill heredoc bodies across all pipeline commands.
        // Heredoc bodies are read by process_pending_heredocs (triggered at newlines),
        // which may occur during a later command's parsing. This pass ensures bodies
        // queued by the lexer are assigned to the correct command's redirects.
        for cmd in &mut commands {
            match cmd {
                Command::Simple(simple) => {
                    self.fill_heredoc_bodies(&mut simple.redirects);
                }
                Command::Compound(_, redirects) => {
                    self.fill_heredoc_bodies(redirects);
                }
                Command::FunctionDef(_) => {}
            }
        }

        Ok(Pipeline { negated, commands })
    }

    pub(super) fn parse_command(&mut self) -> error::Result<Command> {
        if self.is_compound_command_start() {
            let compound = self.parse_compound_command()?;
            let redirects = self.parse_redirect_list()?;
            return Ok(Command::Compound(compound, redirects));
        }

        if let Some(func_def) = self.try_parse_function_def()? {
            return Ok(Command::FunctionDef(func_def));
        }

        let simple = self.parse_simple_command()?;
        Ok(Command::Simple(simple))
    }

    /// Returns true when we've reached a token that ends a complete command.
    pub(super) fn is_complete_command_end(&self) -> bool {
        match &self.current.token {
            Token::Eof => true,
            Token::RParen => true,
            Token::Word(_) => {
                self.is_reserved("}")
                    || self.is_reserved("fi")
                    || self.is_reserved("done")
                    || self.is_reserved("esac")
                    || self.is_reserved("then")
                    || self.is_reserved("else")
                    || self.is_reserved("elif")
                    || self.is_reserved("do")
            }
            _ => false,
        }
    }

    // ---- Compound commands and function defs ----

    pub(super) fn is_compound_command_start(&self) -> bool {
        match &self.current.token {
            Token::LParen => true,
            Token::Word(_) => {
                self.is_reserved("if")
                    || self.is_reserved("for")
                    || self.is_reserved("while")
                    || self.is_reserved("until")
                    || self.is_reserved("case")
                    || self.is_reserved("{")
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::{AndOrOp, SeparatorOp, SimpleCommand};

    pub(super) fn parse(input: &str) -> Program {
        let mut parser = Parser::new(input);
        parser.parse_program().unwrap()
    }

    pub(super) fn parse_first_simple(input: &str) -> SimpleCommand {
        let prog = parse(input);
        let cmd = &prog.commands[0].items[0].0.first.commands[0];
        match cmd {
            Command::Simple(sc) => sc.clone(),
            _ => panic!("expected simple command"),
        }
    }

    #[test]
    fn test_empty_program() {
        let prog = parse("");
        assert!(prog.commands.is_empty());
    }

    #[test]
    fn test_multiple_newlines() {
        let prog = parse("\n\necho hello\n\n");
        assert_eq!(prog.commands.len(), 1);
    }

    #[test]
    fn test_pipeline() {
        let prog = parse("echo hello | grep h");
        let pipeline = &prog.commands[0].items[0].0.first;
        assert_eq!(pipeline.commands.len(), 2);
        assert!(!pipeline.negated);
    }

    #[test]
    fn test_negated_pipeline() {
        let prog = parse("! echo hello");
        let pipeline = &prog.commands[0].items[0].0.first;
        assert!(pipeline.negated);
    }

    #[test]
    fn test_and_or_list() {
        let prog = parse("true && echo yes || echo no");
        let aol = &prog.commands[0].items[0].0;
        assert_eq!(aol.rest.len(), 2);
        assert_eq!(aol.rest[0].0, AndOrOp::And);
        assert_eq!(aol.rest[1].0, AndOrOp::Or);
    }

    #[test]
    fn test_semicolon_list() {
        let prog = parse("echo a; echo b; echo c");
        assert!(prog.commands[0].items.len() >= 3);
    }

    #[test]
    fn test_async_command() {
        let prog = parse("echo hello &");
        let sep = &prog.commands[0].items[0].1;
        assert_eq!(*sep, Some(SeparatorOp::Amp));
    }

    #[test]
    fn parse_program_on_leading_dsemi_errs_not_hangs() {
        // Regression guard: DSemi at start of a simple command used to cause
        // parse_simple_command to return Ok with zero progress, which made
        // parse_compound_list loop forever. See
        // docs/superpowers/specs/2026-04-20-classify-parse-hang-fix-design.md.
        let mut p = Parser::new(";;");
        let err = p
            .parse_program()
            .expect_err("';;' must not parse as a program");
        assert!(
            err.message.contains("unexpected token") || err.message.contains("syntax error"),
            "unexpected message: {}",
            err.message
        );
    }

    #[test]
    fn parse_program_on_leading_pipe_errs() {
        let mut p = Parser::new("|");
        assert!(p.parse_program().is_err());
    }

    #[test]
    fn parse_program_on_dsemi_in_then_body_errs_not_hangs() {
        // The exact input that the original hang reproduced on — the 6th
        // is_completable probe candidate for "if true; then\n".
        let mut p = Parser::new("if true; then\n\n;;\nesac\n");
        assert!(p.parse_program().is_err());
    }
}
