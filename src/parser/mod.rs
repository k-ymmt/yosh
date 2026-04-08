pub mod ast;

use crate::error::{self, ShellError, ShellErrorKind};
use crate::lexer::Lexer;
use crate::lexer::token::{Span, SpannedToken, Token};
use ast::{
    AndOrList, AndOrOp, Assignment, Command, CompleteCommand, FunctionDef, Pipeline, Program,
    Redirect, RedirectKind, HereDoc, SeparatorOp, SimpleCommand, Word, CompoundCommand,
};

pub struct Parser {
    lexer: Lexer,
    current: SpannedToken,
}

impl Parser {
    pub fn new(input: &str) -> Self {
        let mut lexer = Lexer::new(input);
        // Read first token; on error use Eof
        let current = lexer.next_token().unwrap_or(SpannedToken {
            token: Token::Eof,
            span: Span::default(),
        });
        Self { lexer, current }
    }

    pub fn current_token(&self) -> &Token {
        &self.current.token
    }

    pub fn current_span(&self) -> Span {
        self.current.span
    }

    pub fn advance(&mut self) -> error::Result<()> {
        self.current = self.lexer.next_token()?;
        Ok(())
    }

    /// Advance if current token matches expected, returns true if matched.
    pub fn eat(&mut self, expected: &Token) -> error::Result<bool> {
        if self.current.token == *expected {
            self.advance()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Advance if current token is reserved word matching keyword, else error.
    pub fn expect_reserved(&mut self, keyword: &str) -> error::Result<()> {
        if self.current.token.is_reserved_word(keyword) {
            self.advance()?;
            Ok(())
        } else {
            let span = self.current_span();
            Err(ShellError::new(
                ShellErrorKind::UnexpectedToken,
                span.line,
                span.column,
                format!("expected '{}', got unexpected token", keyword),
            ))
        }
    }

    /// Consume all consecutive Newline tokens.
    pub fn skip_newlines(&mut self) -> error::Result<()> {
        while self.current.token == Token::Newline {
            self.advance()?;
        }
        Ok(())
    }

    pub fn is_at_end(&self) -> bool {
        self.current.token == Token::Eof
    }

    pub fn is_reserved(&self, keyword: &str) -> bool {
        self.current.token.is_reserved_word(keyword)
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
        let sep = self.parse_separator_op()?;
        let _is_newline_sep = matches!(sep, Some(SeparatorOp::Semi));

        items.push((first_aol, sep.clone()));

        // After a newline-as-separator (or real Semi/Amp), check if we should continue
        // A bare newline separator ends the command, but real semicolons can chain
        // We need to loop for semicolons and ampersands
        if matches!(sep, Some(SeparatorOp::Semi) | Some(SeparatorOp::Amp)) {
            // Check if next is a real separator (not a newline acting as semicolon)
            // We continue only if we consumed ; or & (not \n)
        }

        // Continue parsing while there are more and_or lists separated by ; or &
        loop {
            if self.is_at_end() || self.is_complete_command_end() {
                break;
            }
            // Check if we got a Newline (which ends a complete command at top level)
            if self.current.token == Token::Newline {
                break;
            }
            let aol = self.parse_and_or()?;
            let sep = self.parse_separator_op()?;
            let ended = matches!(sep, None);
            items.push((aol, sep));
            if ended {
                break;
            }
        }

        Ok(CompleteCommand { items })
    }

    /// Parse separator: ; → Semi, & → Amp, Newline → Semi (as terminator)
    /// Returns None if no separator found.
    pub fn parse_separator_op(&mut self) -> error::Result<Option<SeparatorOp>> {
        match &self.current.token {
            Token::Semi => {
                self.advance()?;
                self.skip_newlines()?;
                Ok(Some(SeparatorOp::Semi))
            }
            Token::Amp => {
                self.advance()?;
                self.skip_newlines()?;
                Ok(Some(SeparatorOp::Amp))
            }
            Token::Newline => {
                // Newline terminates a complete command (acts like Semi)
                // But we don't consume it here — let parse_program handle
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    pub fn parse_and_or(&mut self) -> error::Result<AndOrList> {
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

    pub fn parse_pipeline(&mut self) -> error::Result<Pipeline> {
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

        Ok(Pipeline { negated, commands })
    }

    pub fn parse_command(&mut self) -> error::Result<Command> {
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

    pub fn parse_simple_command(&mut self) -> error::Result<SimpleCommand> {
        let mut assignments = Vec::new();
        let mut words = Vec::new();
        let mut redirects = Vec::new();

        loop {
            // Try redirect first
            if let Some(redirect) = self.try_parse_redirect()? {
                redirects.push(redirect);
                continue;
            }

            // Check for word token
            if let Token::Word(word) = &self.current.token.clone() {
                let word = word.clone();

                // Only try assignments before any command words have been seen
                if words.is_empty() {
                    if let Some(assignment) = self.try_parse_assignment(&word) {
                        self.advance()?;
                        assignments.push(assignment);
                        continue;
                    }
                }

                // It's a regular word
                self.advance()?;
                words.push(word);
                continue;
            }

            // End of simple command
            break;
        }

        Ok(SimpleCommand {
            assignments,
            words,
            redirects,
        })
    }

    /// Try to parse an assignment from a word.
    /// Returns Some(Assignment) if the word contains an `=` and a valid name prefix.
    pub fn try_parse_assignment(&self, word: &Word) -> Option<Assignment> {
        use ast::WordPart;

        // We need the first part to be a Literal containing '='
        // (or the word might be entirely a literal like "FOO=bar")
        if word.parts.is_empty() {
            return None;
        }

        // Collect the full literal text from the first part (if it's a Literal)
        let first_part_text = match &word.parts[0] {
            WordPart::Literal(s) => s.clone(),
            _ => return None,
        };

        // Find '=' in the literal
        let eq_pos = first_part_text.find('=')?;

        let name = &first_part_text[..eq_pos];
        if !is_valid_name(name) {
            return None;
        }

        // Value: rest after '=' in the first part + remaining parts
        let after_eq = &first_part_text[eq_pos + 1..];
        let remaining_parts = &word.parts[1..];

        if after_eq.is_empty() && remaining_parts.is_empty() {
            // FOO= with nothing after
            return Some(Assignment {
                name: name.to_string(),
                value: None,
            });
        }

        // Build value word
        let mut value_parts = Vec::new();
        if !after_eq.is_empty() {
            value_parts.push(WordPart::Literal(after_eq.to_string()));
        }
        value_parts.extend_from_slice(remaining_parts);

        Some(Assignment {
            name: name.to_string(),
            value: Some(Word { parts: value_parts }),
        })
    }

    /// Returns true when we've reached a token that ends a complete command.
    pub fn is_complete_command_end(&self) -> bool {
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

    // ---- Stubs for compound commands and function defs (Task 10+) ----

    pub fn is_compound_command_start(&self) -> bool {
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

    pub fn parse_compound_command(&mut self) -> error::Result<CompoundCommand> {
        let span = self.current_span();
        Err(ShellError::new(
            ShellErrorKind::UnexpectedToken,
            span.line,
            span.column,
            "compound commands not yet implemented",
        ))
    }

    pub fn try_parse_function_def(&mut self) -> error::Result<Option<FunctionDef>> {
        Ok(None)
    }

    // ---- Redirect parsing (Task 9) ----

    pub fn try_parse_redirect(&mut self) -> error::Result<Option<Redirect>> {
        // Check for optional IO number (e.g., 2> or 1<)
        let fd = if let Token::IoNumber(n) = &self.current.token {
            let n = *n;
            self.advance()?;
            Some(n)
        } else {
            None
        };

        let span = self.current_span();

        let kind = match &self.current.token {
            Token::Less => {
                self.advance()?;
                let word = self.expect_word("redirect target")?;
                RedirectKind::Input(word)
            }
            Token::Great => {
                self.advance()?;
                let word = self.expect_word("redirect target")?;
                RedirectKind::Output(word)
            }
            Token::DGreat => {
                self.advance()?;
                let word = self.expect_word("redirect target")?;
                RedirectKind::Append(word)
            }
            Token::Clobber => {
                self.advance()?;
                let word = self.expect_word("redirect target")?;
                RedirectKind::OutputClobber(word)
            }
            Token::LessAnd => {
                self.advance()?;
                let word = self.expect_word("redirect target")?;
                RedirectKind::DupInput(word)
            }
            Token::GreatAnd => {
                self.advance()?;
                let word = self.expect_word("redirect target")?;
                RedirectKind::DupOutput(word)
            }
            Token::LessGreat => {
                self.advance()?;
                let word = self.expect_word("redirect target")?;
                RedirectKind::ReadWrite(word)
            }
            Token::DLess => {
                self.advance()?;
                let _delimiter = self.expect_word("here-doc delimiter")?;
                RedirectKind::HereDoc(HereDoc {
                    body: vec![],
                    strip_tabs: false,
                })
            }
            Token::DLessDash => {
                self.advance()?;
                let _delimiter = self.expect_word("here-doc delimiter")?;
                RedirectKind::HereDoc(HereDoc {
                    body: vec![],
                    strip_tabs: true,
                })
            }
            _ => {
                if fd.is_some() {
                    return Err(ShellError::new(
                        ShellErrorKind::InvalidRedirect,
                        span.line,
                        span.column,
                        "expected redirect operator after IO number",
                    ));
                }
                return Ok(None);
            }
        };

        Ok(Some(Redirect { fd, kind }))
    }

    pub fn parse_redirect_list(&mut self) -> error::Result<Vec<Redirect>> {
        let mut redirects = Vec::new();
        while let Some(redirect) = self.try_parse_redirect()? {
            redirects.push(redirect);
        }
        Ok(redirects)
    }

    pub fn expect_word(&mut self, context: &str) -> error::Result<Word> {
        if let Token::Word(word) = &self.current.token.clone() {
            let word = word.clone();
            self.advance()?;
            Ok(word)
        } else {
            let span = self.current_span();
            Err(ShellError::new(
                ShellErrorKind::UnexpectedToken,
                span.line,
                span.column,
                format!("expected word for {}", context),
            ))
        }
    }
}

fn is_valid_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::{AndOrOp, RedirectKind, SeparatorOp};

    fn parse(input: &str) -> Program {
        let mut parser = Parser::new(input);
        parser.parse_program().unwrap()
    }

    fn parse_first_simple(input: &str) -> SimpleCommand {
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
    fn test_simple_command() {
        let sc = parse_first_simple("echo hello world");
        assert_eq!(sc.words.len(), 3);
        assert_eq!(sc.words[0].as_literal(), Some("echo"));
        assert_eq!(sc.words[1].as_literal(), Some("hello"));
        assert_eq!(sc.words[2].as_literal(), Some("world"));
        assert!(sc.assignments.is_empty());
        assert!(sc.redirects.is_empty());
    }

    #[test]
    fn test_assignment_only() {
        let sc = parse_first_simple("FOO=bar");
        assert!(sc.words.is_empty());
        assert_eq!(sc.assignments.len(), 1);
        assert_eq!(sc.assignments[0].name, "FOO");
        assert_eq!(
            sc.assignments[0].value.as_ref().unwrap().as_literal(),
            Some("bar")
        );
    }

    #[test]
    fn test_assignment_with_command() {
        let sc = parse_first_simple("FOO=bar echo hello");
        assert_eq!(sc.assignments.len(), 1);
        assert_eq!(sc.words.len(), 2);
    }

    #[test]
    fn test_assignment_empty_value() {
        let sc = parse_first_simple("FOO=");
        assert_eq!(sc.assignments.len(), 1);
        assert_eq!(sc.assignments[0].name, "FOO");
        assert_eq!(sc.assignments[0].value, None);
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

    // Task 9 redirect tests

    #[test]
    fn test_output_redirect() {
        let sc = parse_first_simple("echo hello > out.txt");
        assert_eq!(sc.words.len(), 2);
        assert_eq!(sc.redirects.len(), 1);
        assert_eq!(sc.redirects[0].fd, None);
        assert!(
            matches!(&sc.redirects[0].kind, RedirectKind::Output(w) if w.as_literal() == Some("out.txt"))
        );
    }

    #[test]
    fn test_input_redirect() {
        let sc = parse_first_simple("cat < input.txt");
        assert_eq!(sc.redirects.len(), 1);
        assert!(
            matches!(&sc.redirects[0].kind, RedirectKind::Input(w) if w.as_literal() == Some("input.txt"))
        );
    }

    #[test]
    fn test_append_redirect() {
        let sc = parse_first_simple("echo hello >> log.txt");
        assert!(
            matches!(&sc.redirects[0].kind, RedirectKind::Append(w) if w.as_literal() == Some("log.txt"))
        );
    }

    #[test]
    fn test_fd_redirect() {
        let sc = parse_first_simple("cmd 2>/dev/null");
        assert_eq!(sc.redirects[0].fd, Some(2));
        assert!(
            matches!(&sc.redirects[0].kind, RedirectKind::Output(w) if w.as_literal() == Some("/dev/null"))
        );
    }

    #[test]
    fn test_dup_output() {
        let sc = parse_first_simple("cmd 2>&1");
        assert_eq!(sc.redirects[0].fd, Some(2));
        assert!(
            matches!(&sc.redirects[0].kind, RedirectKind::DupOutput(w) if w.as_literal() == Some("1"))
        );
    }

    #[test]
    fn test_heredoc_redirect() {
        let sc = parse_first_simple("cat <<EOF");
        assert_eq!(sc.redirects.len(), 1);
        assert!(matches!(&sc.redirects[0].kind, RedirectKind::HereDoc(_)));
    }

    #[test]
    fn test_clobber_redirect() {
        let sc = parse_first_simple("echo hello >| out.txt");
        assert!(
            matches!(&sc.redirects[0].kind, RedirectKind::OutputClobber(w) if w.as_literal() == Some("out.txt"))
        );
    }

    #[test]
    fn test_read_write_redirect() {
        let sc = parse_first_simple("cmd 3<>file");
        assert_eq!(sc.redirects[0].fd, Some(3));
        assert!(
            matches!(&sc.redirects[0].kind, RedirectKind::ReadWrite(w) if w.as_literal() == Some("file"))
        );
    }

    #[test]
    fn test_multiple_redirects() {
        let sc = parse_first_simple("cmd < in > out 2>&1");
        assert_eq!(sc.redirects.len(), 3);
    }
}
