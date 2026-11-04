//! The command-list grammar below a complete command: and_or lists,
//! pipelines, and the dispatch of one command to the compound, function
//! definition or simple-command production (POSIX §2.9.2, §2.9.3).

use super::ast::{AndOrList, AndOrOp, Command, Pipeline};
use super::{Parser, is_assignment_word, try_parse_assignment, word};
use crate::error::{self, ParseErrorKind, ShellError};
use crate::lexer::token::Token;

impl Parser {
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
            self.require_command_after(match op {
                AndOrOp::And => "&&",
                AndOrOp::Or => "||",
            })?;
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

        // Most pipelines are a single command: allocate for one (an empty
        // Vec would reserve four on the first push).
        let mut commands = Vec::with_capacity(1);
        commands.push(self.parse_command()?);

        while self.current.token == Token::Pipe {
            self.advance()?;
            self.skip_newlines()?;
            self.require_command_after("|")?;
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

        if let Some(compound) = self.try_parse_prefixed_compound()? {
            return Ok(compound);
        }

        if let Some(func_def) = self.try_parse_function_def()? {
            return Ok(Command::FunctionDef(func_def));
        }

        let simple = self.parse_simple_command()?;
        Ok(Command::Simple(simple))
    }

    /// `x=1 if ...`: a compound command after a leading assignment prefix.
    ///
    /// POSIX §2.4: reserved words are recognized at command position even
    /// after a leading assignment prefix. Scan ahead (saving and restoring
    /// lexer state) to check whether the token after any leading assignments
    /// is a compound-command start. If yes, consume for real and attach the
    /// assignments to the compound. Otherwise rewind and return `None`, so
    /// the normal simple-command / function-def paths handle the
    /// assignments themselves. Only a leading assignment word can start
    /// that pattern; for any other word the scan would stop at once (the
    /// caller's compound check already failed) and restore an unchanged
    /// state.
    ///
    /// The scan only classifies the assignment words; they are parsed
    /// into `Assignment`s (after rewinding) only when a compound command
    /// does follow, which is rare — a plain `x=1` or `x=1 cmd` would
    /// otherwise build and drop its assignments twice.
    fn try_parse_prefixed_compound(&mut self) -> error::Result<Option<Command>> {
        let Token::Word(w) = &self.current.token else {
            return Ok(None);
        };
        if !is_assignment_word(w) {
            return Ok(None);
        }
        let (saved, first) = self.snapshot_and_advance();

        // Assignment words scanned (each one successfully advanced past).
        let mut n_assignments = 0;
        let found_compound = first.is_ok() && {
            n_assignments = 1;
            loop {
                if let Token::Word(word) = &self.current.token
                    && is_assignment_word(word)
                {
                    if self.advance().is_err() {
                        break false;
                    }
                    n_assignments += 1;
                    continue;
                }
                break self.is_compound_command_start();
            }
        };

        if found_compound {
            // We're committed: re-read the assignment words for real,
            // then parse the compound and attach the assignments.
            self.rewind(saved);
            let mut prefix_assignments = Vec::with_capacity(n_assignments);
            for _ in 0..n_assignments {
                let assignment = match &self.current.token {
                    Token::Word(word) => try_parse_assignment(word),
                    _ => None,
                };
                let Some(assignment) = assignment else {
                    unreachable!("the scan above classified this word as an assignment");
                };
                self.advance()?;
                prefix_assignments.push(assignment);
            }
            let mut compound = self.parse_compound_command()?;
            compound.assignments = prefix_assignments;
            let redirects = self.parse_redirect_list()?;
            return Ok(Some(Command::Compound(compound, redirects)));
        }

        // `x=1 f() ...`: the POSIX grammar has no assignment prefix
        // before a function definition (function_definition derives
        // from `fname '(' ')'` with no cmd_prefix). Without this check
        // the words would fall through to parse_simple_command, which
        // stops at `(` and later produces a misleading downstream
        // error ("empty compound list in subshell") after executing
        // `x=1 f` as a command. Detect the pattern while still inside
        // the look-ahead (state is restored / abandoned either way)
        // and emit an accurate diagnostic. bash/dash both report a
        // syntax error near the `(` here.
        if n_assignments > 0 {
            let name_candidate = match &self.current.token {
                Token::Word(w) => w.as_literal().is_some_and(word::is_valid_name),
                _ => false,
            };
            if name_candidate && self.advance().is_ok() && self.current.token == Token::LParen {
                return Err(self.unexpected_token_error(
                    "syntax error near unexpected token '(': \
                     assignments may not precede a function definition",
                ));
            }
        }

        // Not a compound-after-assignments case: restore and fall through.
        self.rewind(saved);
        Ok(None)
    }

    /// Error when input ends right after a `|`, `&&`, or `||` operator:
    /// POSIX requires a following command, not a phantom empty one.
    /// `UnexpectedEof` so interactive parsing classifies it as incomplete.
    fn require_command_after(&self, op: &str) -> error::Result<()> {
        if self.current.token == Token::Eof {
            let span = self.current_span();
            return Err(ShellError::parse(
                ParseErrorKind::UnexpectedEof,
                span.line,
                span.column,
                format!("syntax error: expected a command after '{op}'"),
            ));
        }
        Ok(())
    }
}
