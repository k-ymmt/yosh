pub mod ast;
mod compound;
mod function;
mod heredoc;
mod nested;
mod nesting;
mod redirect;
mod simple;
pub(crate) mod word;

pub(crate) use simple::is_assignment_word;
pub(crate) use simple::try_parse_assignment;

use nesting::NestingGuard;
pub(crate) use nesting::{SUBPARSE_NEST_COST, StackBudget, enter_word_level};

use crate::error::{self, ParseErrorKind, ShellError};
use crate::lexer::token::{Span, SpannedToken, Token};
use crate::lexer::{Lexer, LexerState};
use ast::{AndOrList, AndOrOp, Command, CompleteCommand, Pipeline, Program, SeparatorOp};

/// A look-ahead snapshot taken by `Parser::snapshot`.
pub(super) struct Snapshot {
    lexer: LexerState,
    current: SpannedToken,
}

pub struct Parser {
    lexer: Lexer,
    current: SpannedToken,
    /// Lexer position before the current look-ahead token was read.
    pre_current_pos: usize,
    /// A lexer error raised while reading the very first token (in the
    /// constructor, which cannot fail). Reported by the first parse call.
    init_error: Option<ShellError>,
    /// Current compound-command nesting depth (0 = top level).
    depth: usize,
}

impl Parser {
    fn from_lexer(mut lexer: Lexer) -> Self {
        // Reading the first token can recurse into a nested parser (a
        // leading `$(...)`), so it counts as one nesting level.
        let first =
            NestingGuard::enter(Span { line: 1, column: 1 }).and_then(|_guard| lexer.next_token());
        let (current, init_error) = match first {
            Ok(tok) => (tok, None),
            Err(e) => (
                SpannedToken {
                    token: Token::Eof,
                    span: Span::default(),
                },
                Some(e),
            ),
        };
        Self {
            lexer,
            current,
            pre_current_pos: 0,
            init_error,
            depth: 0,
        }
    }

    pub fn new(input: &str) -> Self {
        Self::from_lexer(Lexer::new(input))
    }

    pub fn new_with_aliases(input: &str, aliases: &crate::env::aliases::AliasStore) -> Self {
        Self::from_lexer(Lexer::new_with_aliases(input, aliases))
    }

    /// Replace the alias table used for tokens not yet read. A driver that
    /// parses and executes one complete command at a time calls this after
    /// each command so aliases it defined apply to the following lines.
    pub fn set_aliases(&mut self, aliases: &crate::env::aliases::AliasStore) {
        self.lexer.set_aliases(aliases);
    }

    /// Report a lexer error deferred from the constructor.
    fn take_init_error(&mut self) -> error::Result<()> {
        match self.init_error.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Returns the byte position in the input up to (but not including) the current
    /// look-ahead token. This is useful for incremental parsing.
    pub fn consumed_bytes(&self) -> usize {
        self.pre_current_pos
    }

    /// The lexer's byte position in the input: everything before it,
    /// including the current look-ahead token, has been read.
    pub fn input_position(&self) -> usize {
        self.lexer.position()
    }

    /// The raw input text between two byte positions (`consumed_bytes` /
    /// `input_position` values).
    pub fn source_text(&self, start: usize, end: usize) -> String {
        self.lexer.source_text(start, end)
    }

    pub fn current_token(&self) -> &Token {
        &self.current.token
    }

    pub(super) fn current_span(&self) -> Span {
        self.current.span
    }

    /// Save the lexer state and the current token for look-ahead that may
    /// have to be undone with `rewind`.
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
    /// `pre_current_pos` is not part of the snapshot.
    pub(super) fn snapshot(&self) -> Snapshot {
        Snapshot {
            lexer: self.lexer.save_state(),
            current: self.current.clone(),
        }
    }

    /// Return to a state saved by `snapshot`.
    pub(super) fn rewind(&mut self, snapshot: Snapshot) {
        self.lexer.restore_state(snapshot.lexer);
        self.current = snapshot.current;
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
    pub fn skip_newlines(&mut self) -> error::Result<()> {
        while self.current.token == Token::Newline {
            // Pending heredoc bodies start right after this newline, so they
            // must be read BEFORE advancing — advancing first would lex the
            // first body line as an ordinary token.
            if self.lexer.has_pending_heredocs() {
                self.lexer.process_pending_heredocs()?;
            }
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

    // ---- Grammar productions ----

    pub fn parse_program(&mut self) -> error::Result<Program> {
        self.take_init_error()?;
        // A program may itself be the body of a command substitution being
        // lexed by an outer parser; count it toward the nesting limit.
        let _nesting = NestingGuard::enter(self.current_span())?;
        self.skip_newlines()?;
        let mut commands = Vec::new();
        while !self.is_at_end() {
            let cmd = self.parse_complete_command()?;
            commands.push(cmd);
            self.skip_newlines()?;
        }
        Ok(Program { commands })
    }

    /// Parse one complete_command: and_or lists joined by `;` / `&`, ended
    /// by a newline, EOF, or a token that closes an enclosing construct.
    ///
    /// A terminating newline is left as the look-ahead token (any pending
    /// here-document bodies are read first): lexing past it would read the
    /// next line before this command runs, so aliases it defines would not
    /// apply and a lexer error on that line would discard this command.
    /// Callers consume it with `skip_newlines`.
    pub fn parse_complete_command(&mut self) -> error::Result<CompleteCommand> {
        self.take_init_error()?;
        // Look-ahead never rewinds past a complete-command boundary, so
        // tokens cached for an earlier command can no longer be replayed.
        self.lexer.clear_token_cache();
        let mut items = Vec::new();

        loop {
            let aol = self.parse_and_or()?;
            let (sep, ended) = match self.current.token {
                Token::Semi => {
                    self.advance()?;
                    (Some(SeparatorOp::Semi), false)
                }
                Token::Amp => {
                    self.advance()?;
                    (Some(SeparatorOp::Amp), false)
                }
                Token::Newline => {
                    self.read_heredocs_at_newline()?;
                    (Some(SeparatorOp::Semi), true)
                }
                _ => {
                    self.check_command_terminator()?;
                    (None, true)
                }
            };
            items.push((aol, sep));
            if ended || self.is_at_end() || self.is_complete_command_end() {
                break;
            }
            if self.current.token == Token::Newline {
                // A terminator newline after `;` or `&`.
                self.read_heredocs_at_newline()?;
                break;
            }
        }

        let mut cc = CompleteCommand { items };
        // Attach heredoc bodies that were read after their command's AST node
        // was built (e.g. `done <<EOF`, `cat <<A; cat <<B`).
        if self.lexer.has_heredoc_bodies() {
            self.fill_heredoc_bodies_deep(&mut cc);
        }
        Ok(cc)
    }

    /// The current token is a newline: read any here-document bodies that
    /// start after it, without consuming the newline itself.
    fn read_heredocs_at_newline(&mut self) -> error::Result<()> {
        if self.lexer.has_pending_heredocs() {
            self.lexer.process_pending_heredocs()?;
        }
        Ok(())
    }

    /// After an and_or list with no `;`/`&`/newline separator, the next
    /// token must end the enclosing construct. Anything else (`echo a
    /// (echo b)`, `{ :; } echo b`, `(echo a) echo b`) is a syntax error.
    /// At the top level nothing but EOF may follow.
    fn check_command_terminator(&self) -> error::Result<()> {
        let nested = self.depth > 0;
        let ok = match &self.current.token {
            Token::Eof | Token::Newline => true,
            Token::RParen | Token::DSemi | Token::SemiAnd => nested,
            Token::Word(w) => nested && w.as_literal().is_some_and(is_closing_reserved_word),
            _ => false,
        };
        if ok {
            return Ok(());
        }
        let span = self.current_span();
        Err(ShellError::parse(
            ParseErrorKind::UnexpectedToken,
            span.line,
            span.column,
            format!(
                "syntax error near unexpected token '{}'",
                token_display(&self.current.token)
            ),
        ))
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
    fn try_parse_prefixed_compound(&mut self) -> error::Result<Option<Command>> {
        let Token::Word(w) = &self.current.token else {
            return Ok(None);
        };
        if !is_assignment_word(w) {
            return Ok(None);
        }
        let saved = self.snapshot();

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

    /// Returns true when we've reached a token that ends a complete command.
    pub(super) fn is_complete_command_end(&self) -> bool {
        match &self.current.token {
            Token::Eof => true,
            Token::RParen => true,
            Token::Word(word) => word.as_literal().is_some_and(is_closing_reserved_word),
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

/// Reserved words that close a compound command or one of its parts, and
/// so may end a command list without a separator (`{ :; }`, `if a; then
/// b; fi`, ...).
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
    fn lexer_error_on_first_token_is_reported() {
        for src in ["\"abc", "${x", "'abc"] {
            let mut p = Parser::new(src);
            assert!(
                !p.is_at_end(),
                "{src}: pending error must not look like EOF"
            );
            let err = p.parse_program().expect_err(src);
            assert_eq!(err.exit_code(), 2, "{src}");
        }
        let err = Parser::new("\"abc").parse_complete_command().unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn missing_separator_after_command_is_syntax_error() {
        for src in [
            "echo a (echo b)",
            "{ echo a; } echo b",
            "(echo a) echo b",
            "if true; then echo a; fi echo b",
            "f() { :; } (echo y)",
            "(echo a) fi",
            "echo a )",
            "echo a ;;",
            "{ (echo a) echo b; }",
        ] {
            let err = Parser::new(src).parse_program().expect_err(src);
            assert!(
                err.to_string().contains("syntax error"),
                "{src}: unexpected message {err}"
            );
        }
    }

    #[test]
    fn closer_may_follow_command_without_separator() {
        for src in [
            "if (true) then echo ok; fi",
            "{ (echo b) }",
            "case x in x) (echo c) esac",
            "while (false) do :; done",
            "(echo a)",
        ] {
            assert!(Parser::new(src).parse_program().is_ok(), "{src}");
        }
    }

    #[test]
    fn bang_only_before_a_whole_pipeline() {
        assert!(Parser::new("echo a | ! cat").parse_program().is_err());
        assert!(Parser::new("! ! true").parse_program().is_err());
        assert!(Parser::new("! true | cat").parse_program().is_ok());
        assert!(Parser::new("true && ! false").parse_program().is_ok());
    }

    #[test]
    fn complete_command_leaves_terminating_newline_unread() {
        // The next line must not be lexed before the command runs: a lexer
        // error there may not discard the command, and aliases it defines
        // must apply to the next line.
        let mut p = Parser::new("echo 1\n'bad");
        let cc = p.parse_complete_command().unwrap();
        assert_eq!(cc.items.len(), 1);
        assert_eq!(p.current_token(), &Token::Newline);
        assert!(p.skip_newlines().is_err());
    }

    #[test]
    fn set_aliases_applies_to_lines_not_yet_read() {
        use crate::env::aliases::AliasStore;
        let mut p = Parser::new_with_aliases("alias e=echo\ne hi\n", &AliasStore::default());
        p.parse_complete_command().unwrap();
        let mut aliases = AliasStore::default();
        aliases.set("e", "echo");
        p.set_aliases(&aliases);
        p.skip_newlines().unwrap();
        let cc = p.parse_complete_command().unwrap();
        let Command::Simple(sc) = &cc.items[0].0.first.commands[0] else {
            panic!("expected simple command");
        };
        assert_eq!(sc.words[0].as_literal(), Some("echo"));
    }

    #[test]
    fn heredoc_delimiter_keeps_expansion_syntax_literally() {
        use ast::RedirectKind;
        for (src, delim) in [
            ("cat <<$x\nbody\n$x\n", "$x"),
            ("cat <<\"$x\"\nbody\n$x\n", "$x"),
            ("cat <<~\nbody\n~\n", "~"),
            ("cat <<a$b\nbody\na$b\n", "a$b"),
            ("cat <<${x}y\nbody\n${x}y\n", "${x}y"),
            ("cat <<a\"b\"'c'\\d\nbody\nabcd\n", "abcd"),
        ] {
            let sc = parse_first_simple(src);
            let RedirectKind::HereDoc(hd) = &sc.redirects[0].kind else {
                panic!("expected heredoc for {src:?}");
            };
            assert_eq!(
                hd.body.len(),
                1,
                "{src:?} (delimiter {delim}) body not closed"
            );
        }
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
    // `try_parse_prefixed_compound`'s assignment-prefix lookahead (this
    // file, around `snapshot`/`rewind`) calls `self.advance()` inside the loop,
    // which is `Parser::advance` — it drives the lexer's `next_token()` and
    // CAN dequeue from the alias token queue / change command-name position
    // or the recursion guard. That means this call site must keep the FULL
    // `save_state` (alias-state clone), unlike the io-number lookahead
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
