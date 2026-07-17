pub mod ast;
mod compound;
mod function;
mod redirect;
mod simple;
pub(crate) mod word;

pub(crate) use simple::try_parse_assignment;

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

        let mut commands = Vec::new();
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

        // POSIX §2.4: reserved words are recognized at command position even
        // after a leading assignment prefix. Scan ahead (saving and restoring
        // lexer state) to check whether the token after any leading assignments
        // is a compound-command start. If yes, consume for real and attach the
        // assignments to the compound. Otherwise fall through to the normal
        // simple-command / function-def paths which handle assignments themselves.
        if let Token::Word(_) = &self.current.token {
            // Full `save_state` (not the lexer's light `CursorState`) is
            // required here: the loop below calls `self.advance()`
            // (`Parser::advance`), which drives `Lexer::next_token()` and
            // can dequeue `alias_token_queue` / flip `check_alias` / mutate
            // `expanding_aliases` when the look-ahead words came from (or
            // trigger) alias expansion. A cursor-only snapshot would lose
            // that state on the restore path below and corrupt subsequent
            // alias expansion. See `Lexer::try_read_io_number` for the
            // contrasting call site where the light snapshot IS safe.
            let saved_state = self.lexer.save_state();
            let saved_current = self.current.clone();

            let mut prefix_assignments = Vec::new();
            let found_compound = loop {
                if let Token::Word(word) = &self.current.token
                    && let Some(a) = try_parse_assignment(word)
                {
                    if self.advance().is_err() {
                        break false;
                    }
                    prefix_assignments.push(a);
                    continue;
                }
                break self.is_compound_command_start();
            };

            if found_compound && !prefix_assignments.is_empty() {
                // We're committed: parse the compound and attach assignments.
                let mut compound = self.parse_compound_command()?;
                compound.assignments = prefix_assignments;
                let redirects = self.parse_redirect_list()?;
                return Ok(Command::Compound(compound, redirects));
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
            if !prefix_assignments.is_empty() {
                let name_candidate = match &self.current.token {
                    Token::Word(w) => w.as_literal().is_some_and(word::is_valid_name),
                    _ => false,
                };
                if name_candidate && self.advance().is_ok() && self.current.token == Token::LParen {
                    let span = self.current_span();
                    return Err(ShellError::parse(
                        ParseErrorKind::UnexpectedToken,
                        span.line,
                        span.column,
                        "syntax error near unexpected token '(': \
                         assignments may not precede a function definition",
                    ));
                }
            }

            // Not a compound-after-assignments case: restore and fall through.
            self.lexer.restore_state(saved_state);
            self.current = saved_current;
        }

        if let Some(func_def) = self.try_parse_function_def()? {
            return Ok(Command::FunctionDef(func_def));
        }

        let simple = self.parse_simple_command()?;
        Ok(Command::Simple(simple))
    }

    /// Returns true when we've reached a token that ends a complete command.
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

    pub(super) fn is_complete_command_end(&self) -> bool {
        match &self.current.token {
            Token::Eof => true,
            Token::RParen => true,
            Token::Word(word) => matches!(
                word.as_literal(),
                Some("}" | "fi" | "done" | "esac" | "then" | "else" | "elif" | "do")
            ),
            _ => false,
        }
    }

    // ---- Compound commands and function defs ----

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

    // ── assignment prefix before a function definition (SP5) ──

    #[test]
    fn assignment_prefix_before_function_def_is_explicit_syntax_error() {
        let err = Parser::new("x=1 f() { :; }")
            .parse_program()
            .expect_err("must be a syntax error");
        assert_eq!(err.exit_code(), 2);
        let msg = err.to_string();
        assert!(
            msg.contains("syntax error near unexpected token '('"),
            "unexpected message: {msg}"
        );
        assert!(
            msg.contains("function definition"),
            "message must explain the cause: {msg}"
        );
    }

    #[test]
    fn multiple_assignment_prefixes_before_function_def_also_error() {
        let err = Parser::new("a=1 b=2 f ( ) { :; }")
            .parse_program()
            .expect_err("must be a syntax error");
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn assignment_prefix_before_function_call_still_parses() {
        let sc = parse_first_simple("x=1 f arg");
        assert_eq!(sc.assignments.len(), 1);
        assert_eq!(sc.words.len(), 2);
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

    // ---- Task 4 item 17 locking tests ----
    //
    // `parse_command`'s assignment-prefix lookahead (this file, around
    // `save_state`/`restore_state`) calls `self.advance()` inside the loop,
    // which is `Parser::advance` — it drives the lexer's `next_token()` and
    // CAN dequeue from `alias_token_queue` / mutate `check_alias` /
    // `expanding_aliases`. That means this call site must keep the FULL
    // `save_state` (queue + HashSet clone), unlike the io-number lookahead
    // in the lexer. These tests exercise alias expansion across exactly
    // this save/restore pair on both outcomes (compound found, and
    // fallback/restore) to lock in correct behavior regardless of how
    // `save_state` is implemented.

    #[test]
    fn assignment_lookahead_restores_correctly_after_alias_expanded_words() {
        // `ll` expands to `ls -l` (two queued tokens). Neither looks like an
        // assignment, so the lookahead loop's very first iteration takes the
        // `is_compound_command_start()` branch immediately (false), and the
        // outer code must restore lexer + current state so the alias-queued
        // tokens are not lost or duplicated by the subsequent real parse.
        use crate::env::aliases::AliasStore;
        let mut aliases = AliasStore::default();
        aliases.set("ll", "ls -l");
        let mut parser = Parser::new_with_aliases("ll /tmp\n", &aliases);
        let prog = parser.parse_program().unwrap();
        let cmd = &prog.commands[0].items[0].0.first.commands[0];
        let Command::Simple(sc) = cmd else {
            panic!("expected simple command, got {:?}", cmd);
        };
        assert!(sc.assignments.is_empty());
        assert_eq!(sc.words.len(), 3);
        assert_eq!(sc.words[0].as_literal(), Some("ls"));
        assert_eq!(sc.words[1].as_literal(), Some("-l"));
        assert_eq!(sc.words[2].as_literal(), Some("/tmp"));
    }

    #[test]
    fn assignment_lookahead_commits_compound_after_alias_expanded_assignment() {
        // `setx` expands to `x=1`, a real assignment word. The lookahead
        // loop consumes it for real (advance) and then finds `if`, a
        // compound-command start, so this path commits (does NOT restore)
        // and attaches the assignment to the compound. This exercises the
        // "found_compound" branch with an alias-produced assignment word
        // flowing through the save/restore pair.
        use crate::env::aliases::AliasStore;
        use ast::{Command, CompoundCommandKind};
        let mut aliases = AliasStore::default();
        aliases.set("setx", "x=1");
        let mut parser = Parser::new_with_aliases("setx if true; then echo y; fi\n", &aliases);
        let prog = parser.parse_program().unwrap();
        let cmd = &prog.commands[0].items[0].0.first.commands[0];
        let Command::Compound(comp, _redirs) = cmd else {
            panic!("expected Compound, got {:?}", cmd);
        };
        assert!(matches!(comp.kind, CompoundCommandKind::If { .. }));
        assert_eq!(comp.assignments.len(), 1);
        assert_eq!(comp.assignments[0].name, "x");
        assert_eq!(
            comp.assignments[0].value.as_ref().unwrap().as_literal(),
            Some("1")
        );
    }

    #[test]
    fn assignment_lookahead_restores_alias_queue_when_not_assignment_led() {
        // `greet` expands to `echo hi` (two queued tokens, neither an
        // assignment). The lookahead loop's first token is a Word but fails
        // try_parse_assignment immediately, so is_compound_command_start()
        // is checked on the SAME (still unconsumed) token and is false —
        // the outer restore path fires. This must not drop or duplicate the
        // still-queued alias token ("hi").
        use crate::env::aliases::AliasStore;
        let mut aliases = AliasStore::default();
        aliases.set("greet", "echo hi");
        let mut parser = Parser::new_with_aliases("greet\n", &aliases);
        let prog = parser.parse_program().unwrap();
        let cmd = &prog.commands[0].items[0].0.first.commands[0];
        let Command::Simple(sc) = cmd else {
            panic!("expected simple command, got {:?}", cmd);
        };
        assert_eq!(sc.words.len(), 2);
        assert_eq!(sc.words[0].as_literal(), Some("echo"));
        assert_eq!(sc.words[1].as_literal(), Some("hi"));
    }
}
