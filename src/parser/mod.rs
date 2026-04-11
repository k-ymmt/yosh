pub mod ast;

use std::rc::Rc;
use crate::error::{self, ShellError, ShellErrorKind};
use crate::lexer::Lexer;
use crate::lexer::token::{Span, SpannedToken, Token};
use ast::{
    AndOrList, AndOrOp, Assignment, CaseItem, CaseTerminator, Command, CompleteCommand,
    CompoundCommand, CompoundCommandKind, FunctionDef, HereDoc, Pipeline, Program, Redirect,
    RedirectKind, SeparatorOp, SimpleCommand, Word, WordPart,
};

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
        Self { lexer, current, pre_current_pos: 0 }
    }

    pub fn new_with_aliases(input: &str, aliases: &crate::env::aliases::AliasStore) -> Self {
        let mut lexer = Lexer::new_with_aliases(input, aliases);
        let current = lexer.next_token().unwrap_or(SpannedToken {
            token: Token::Eof,
            span: Span::default(),
        });
        Self { lexer, current, pre_current_pos: 0 }
    }

    /// Returns the byte position in the input up to (but not including) the current
    /// look-ahead token. This is useful for incremental parsing.
    pub fn consumed_bytes(&self) -> usize {
        self.pre_current_pos
    }

    #[allow(dead_code)]
    pub fn current_token(&self) -> &Token {
        &self.current.token
    }

    pub fn current_span(&self) -> Span {
        self.current.span
    }

    pub fn advance(&mut self) -> error::Result<()> {
        self.pre_current_pos = self.lexer.position();
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
            if self.lexer.has_pending_heredocs() {
                self.lexer.process_pending_heredocs()?;
            }
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
    pub fn parse_separator_op(&mut self) -> error::Result<Option<SeparatorOp>> {
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
                if words.is_empty() && let Some(assignment) = self.try_parse_assignment(&word) {
                    self.advance()?;
                    assignments.push(assignment);
                    continue;
                }

                // It's a regular word
                self.advance()?;
                words.push(word);
                continue;
            }

            // If we hit a newline and have pending heredocs, process them now
            if self.current.token == Token::Newline && self.lexer.has_pending_heredocs() {
                self.lexer.process_pending_heredocs()?;
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

    // ---- Compound commands and function defs ----

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
        let kind = if self.is_reserved("if") {
            self.parse_if_clause()?
        } else if self.is_reserved("for") {
            self.parse_for_clause()?
        } else if self.is_reserved("while") {
            self.parse_while_clause()?
        } else if self.is_reserved("until") {
            self.parse_until_clause()?
        } else if self.is_reserved("case") {
            self.parse_case_clause()?
        } else if self.is_reserved("{") {
            self.parse_brace_group()?
        } else if self.current.token == Token::LParen {
            self.parse_subshell()?
        } else {
            let span = self.current_span();
            return Err(ShellError::new(
                ShellErrorKind::UnexpectedToken,
                span.line,
                span.column,
                "expected compound command",
            ));
        };
        Ok(CompoundCommand { kind })
    }

    /// Parse a compound_list: skip newlines, then parse complete_commands until at_end or is_complete_command_end.
    pub fn parse_compound_list(&mut self) -> error::Result<Vec<CompleteCommand>> {
        self.skip_newlines()?;
        let mut commands = Vec::new();
        while !self.is_at_end() && !self.is_complete_command_end() {
            let cmd = self.parse_complete_command()?;
            commands.push(cmd);
            self.skip_newlines()?;
        }
        Ok(commands)
    }

    /// Parse: if compound_list then compound_list [elif compound_list then compound_list]... [else compound_list] fi
    pub fn parse_if_clause(&mut self) -> error::Result<CompoundCommandKind> {
        self.expect_reserved("if")?;
        let condition = self.parse_compound_list()?;
        self.expect_reserved("then")?;
        let then_part = self.parse_compound_list()?;

        let mut elif_parts = Vec::new();
        let mut else_part = None;

        loop {
            if self.is_reserved("elif") {
                self.advance()?;
                let elif_cond = self.parse_compound_list()?;
                self.expect_reserved("then")?;
                let elif_body = self.parse_compound_list()?;
                elif_parts.push((elif_cond, elif_body));
            } else if self.is_reserved("else") {
                self.advance()?;
                else_part = Some(self.parse_compound_list()?);
                break;
            } else {
                break;
            }
        }

        self.expect_reserved("fi")?;

        Ok(CompoundCommandKind::If {
            condition,
            then_part,
            elif_parts,
            else_part,
        })
    }

    /// Parse: for name [in [word ...]] do compound_list done
    pub fn parse_for_clause(&mut self) -> error::Result<CompoundCommandKind> {
        self.expect_reserved("for")?;

        // Expect a valid variable name
        let var = match &self.current.token.clone() {
            Token::Word(word) => {
                let name = word.as_literal().ok_or_else(|| {
                    let span = self.current_span();
                    ShellError::new(
                        ShellErrorKind::UnexpectedToken,
                        span.line,
                        span.column,
                        "expected valid variable name after 'for'",
                    )
                })?;
                if !is_valid_name(name) {
                    let span = self.current_span();
                    return Err(ShellError::new(
                        ShellErrorKind::UnexpectedToken,
                        span.line,
                        span.column,
                        format!("'{}' is not a valid variable name", name),
                    ));
                }
                let name = name.to_string();
                self.advance()?;
                name
            }
            _ => {
                let span = self.current_span();
                return Err(ShellError::new(
                    ShellErrorKind::UnexpectedToken,
                    span.line,
                    span.column,
                    "expected variable name after 'for'",
                ));
            }
        };

        self.skip_newlines()?;

        let words = if self.is_reserved("in") {
            self.advance()?;
            // Read words until ; or newline or "do"
            let mut word_list = Vec::new();
            loop {
                if self.is_at_end()
                    || self.current.token == Token::Semi
                    || self.current.token == Token::Newline
                    || self.is_reserved("do")
                {
                    break;
                }
                if let Token::Word(_) = &self.current.token {
                    let w = self.expect_word("for word list")?;
                    word_list.push(w);
                } else {
                    break;
                }
            }
            // Advance past ; or newline
            if self.current.token == Token::Semi || self.current.token == Token::Newline {
                self.advance()?;
            }
            Some(word_list)
        } else {
            // No "in" clause — words is None (means "$@")
            if self.current.token == Token::Semi {
                self.advance()?;
            }
            None
        };

        self.skip_newlines()?;
        let body = self.parse_do_group()?;

        Ok(CompoundCommandKind::For { var, words, body })
    }

    /// Parse: do compound_list done
    pub fn parse_do_group(&mut self) -> error::Result<Vec<CompleteCommand>> {
        self.expect_reserved("do")?;
        let body = self.parse_compound_list()?;
        self.expect_reserved("done")?;
        Ok(body)
    }

    /// Parse: while compound_list do compound_list done
    pub fn parse_while_clause(&mut self) -> error::Result<CompoundCommandKind> {
        self.expect_reserved("while")?;
        let condition = self.parse_compound_list()?;
        let body = self.parse_do_group()?;
        Ok(CompoundCommandKind::While { condition, body })
    }

    /// Parse: until compound_list do compound_list done
    pub fn parse_until_clause(&mut self) -> error::Result<CompoundCommandKind> {
        self.expect_reserved("until")?;
        let condition = self.parse_compound_list()?;
        let body = self.parse_do_group()?;
        Ok(CompoundCommandKind::Until { condition, body })
    }

    /// Parse: case word in [pattern [| pattern]... ) compound-list ;; ]... esac
    pub fn parse_case_clause(&mut self) -> error::Result<CompoundCommandKind> {
        self.expect_reserved("case")?;
        let word = self.expect_word("case subject")?;
        self.skip_newlines()?;
        self.expect_reserved("in")?;
        self.skip_newlines()?;

        let mut items = Vec::new();

        while !self.is_at_end() && !self.is_reserved("esac") {
            // Optional leading (
            let _ = self.eat(&Token::LParen)?;

            // Read patterns separated by |
            let mut patterns = Vec::new();
            let first_pattern = self.expect_word("case pattern")?;
            patterns.push(first_pattern);
            while self.current.token == Token::Pipe {
                self.advance()?;
                let pat = self.expect_word("case pattern")?;
                patterns.push(pat);
            }

            // Expect )
            if !self.eat(&Token::RParen)? {
                let span = self.current_span();
                return Err(ShellError::new(
                    ShellErrorKind::UnexpectedToken,
                    span.line,
                    span.column,
                    "expected ')' after case pattern",
                ));
            }
            self.skip_newlines()?;

            // Parse body until ;; or ;& or esac
            let mut body = Vec::new();
            while !self.is_at_end()
                && self.current.token != Token::DSemi
                && self.current.token != Token::SemiAnd
                && !self.is_reserved("esac")
            {
                let cmd = self.parse_complete_command()?;
                body.push(cmd);
                self.skip_newlines()?;
            }

            let terminator = if self.current.token == Token::SemiAnd {
                self.advance()?;
                CaseTerminator::FallThrough
            } else if self.current.token == Token::DSemi {
                self.advance()?;
                CaseTerminator::Break
            } else {
                // esac without terminator → Break
                CaseTerminator::Break
            };

            self.skip_newlines()?;

            items.push(CaseItem {
                patterns,
                body,
                terminator,
            });
        }

        self.expect_reserved("esac")?;

        Ok(CompoundCommandKind::Case { word, items })
    }

    /// Parse: { compound_list }
    pub fn parse_brace_group(&mut self) -> error::Result<CompoundCommandKind> {
        self.expect_reserved("{")?;
        let body = self.parse_compound_list()?;
        self.expect_reserved("}")?;
        Ok(CompoundCommandKind::BraceGroup { body })
    }

    /// Parse: ( compound_list )
    pub fn parse_subshell(&mut self) -> error::Result<CompoundCommandKind> {
        self.eat(&Token::LParen)?;
        let body = self.parse_compound_list()?;
        if !self.eat(&Token::RParen)? {
            let span = self.current_span();
            return Err(ShellError::new(
                ShellErrorKind::UnexpectedToken,
                span.line,
                span.column,
                "expected ')' to close subshell",
            ));
        }
        Ok(CompoundCommandKind::Subshell { body })
    }

    /// Try to parse a function definition: NAME ( ) linebreak compound_command [redirect_list]
    pub fn try_parse_function_def(&mut self) -> error::Result<Option<FunctionDef>> {
        // Check if current token is a Word with a valid name
        let name = match &self.current.token {
            Token::Word(word) => {
                if let Some(lit) = word.as_literal() {
                    if is_valid_name(lit) {
                        lit.to_string()
                    } else {
                        return Ok(None);
                    }
                } else {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        };

        // Save state for backtracking
        let saved_lexer_state = self.lexer.save_state();
        let saved_current = self.current.clone();

        // Advance past the name
        self.advance()?;

        // Check for (
        if self.current.token != Token::LParen {
            // Restore state
            self.lexer.restore_state(saved_lexer_state);
            self.current = saved_current;
            return Ok(None);
        }
        self.advance()?;

        // Check for )
        if self.current.token != Token::RParen {
            // Restore state
            self.lexer.restore_state(saved_lexer_state);
            self.current = saved_current;
            return Ok(None);
        }
        self.advance()?;

        // Skip newlines (linebreak)
        self.skip_newlines()?;

        // Parse compound command body
        let body = self.parse_compound_command()?;

        // Parse optional redirect list
        let redirects = self.parse_redirect_list()?;

        Ok(Some(FunctionDef {
            name,
            body: Rc::new(body),
            redirects,
        }))
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
                let delimiter_word = self.expect_word("here-document delimiter")?;
                let (delimiter, quoted) = self.extract_heredoc_delimiter(&delimiter_word);
                self.lexer.register_heredoc(delimiter, quoted, false);
                RedirectKind::HereDoc(HereDoc {
                    body: vec![],
                    strip_tabs: false,
                    quoted,
                })
            }
            Token::DLessDash => {
                self.advance()?;
                let delimiter_word = self.expect_word("here-document delimiter")?;
                let (delimiter, quoted) = self.extract_heredoc_delimiter(&delimiter_word);
                self.lexer.register_heredoc(delimiter, quoted, true);
                RedirectKind::HereDoc(HereDoc {
                    body: vec![],
                    strip_tabs: true,
                    quoted,
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

    fn extract_heredoc_delimiter(&self, word: &Word) -> (String, bool) {
        let mut delimiter = String::new();
        let mut quoted = false;
        for part in &word.parts {
            match part {
                WordPart::Literal(s) => delimiter.push_str(s),
                WordPart::SingleQuoted(s) => {
                    delimiter.push_str(s);
                    quoted = true;
                }
                WordPart::DoubleQuoted(parts) => {
                    quoted = true;
                    for p in parts {
                        if let WordPart::Literal(s) = p {
                            delimiter.push_str(s);
                        }
                    }
                }
                WordPart::DollarSingleQuoted(s) => {
                    delimiter.push_str(s);
                    quoted = true;
                }
                _ => {}
            }
        }
        (delimiter, quoted)
    }

    fn fill_heredoc_bodies(&mut self, redirects: &mut Vec<Redirect>) {
        for redir in redirects {
            if let RedirectKind::HereDoc(ref mut hd) = redir.kind
                && hd.body.is_empty()
                && let Some(body) = self.lexer.take_heredoc_body()
            {
                hd.body = body;
            }
        }
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
    use ast::{AndOrOp, CaseTerminator, CompoundCommandKind, RedirectKind, SeparatorOp, WordPart};

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

    // ---- Task 10 & 11: Compound command tests ----

    fn parse_first_compound(input: &str) -> CompoundCommandKind {
        let prog = parse(input);
        let cmd = &prog.commands[0].items[0].0.first.commands[0];
        match cmd {
            Command::Compound(cc, _) => cc.kind.clone(),
            _ => panic!("expected compound command"),
        }
    }

    #[test]
    fn test_if_then_fi() {
        let kind = parse_first_compound("if true; then echo yes; fi");
        match kind {
            CompoundCommandKind::If {
                condition,
                then_part,
                elif_parts,
                else_part,
            } => {
                assert!(!condition.is_empty());
                assert!(!then_part.is_empty());
                assert!(elif_parts.is_empty());
                assert!(else_part.is_none());
            }
            _ => panic!("expected if"),
        }
    }

    #[test]
    fn test_if_else() {
        let kind = parse_first_compound("if true; then echo yes; else echo no; fi");
        match kind {
            CompoundCommandKind::If { else_part, .. } => assert!(else_part.is_some()),
            _ => panic!(),
        }
    }

    #[test]
    fn test_if_elif() {
        let kind =
            parse_first_compound("if false; then echo 1; elif true; then echo 2; else echo 3; fi");
        match kind {
            CompoundCommandKind::If {
                elif_parts,
                else_part,
                ..
            } => {
                assert_eq!(elif_parts.len(), 1);
                assert!(else_part.is_some());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_for_loop_with_words() {
        let kind = parse_first_compound("for i in a b c; do echo $i; done");
        match kind {
            CompoundCommandKind::For { var, words, body } => {
                assert_eq!(var, "i");
                assert_eq!(words.unwrap().len(), 3);
                assert!(!body.is_empty());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_for_loop_without_in() {
        let kind = parse_first_compound("for i; do echo $i; done");
        match kind {
            CompoundCommandKind::For { var, words, .. } => {
                assert_eq!(var, "i");
                assert!(words.is_none());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_for_loop_with_do_on_newline() {
        let kind = parse_first_compound("for i in a b c\ndo\necho $i\ndone");
        match kind {
            CompoundCommandKind::For { words, .. } => assert!(words.is_some()),
            _ => panic!(),
        }
    }

    #[test]
    fn test_while_loop() {
        let kind = parse_first_compound("while true; do echo loop; done");
        assert!(matches!(kind, CompoundCommandKind::While { .. }));
    }

    #[test]
    fn test_until_loop() {
        let kind = parse_first_compound("until false; do echo loop; done");
        assert!(matches!(kind, CompoundCommandKind::Until { .. }));
    }

    #[test]
    fn test_case_basic() {
        let kind = parse_first_compound("case $x in\na) echo a;;\nb) echo b;;\nesac");
        match kind {
            CompoundCommandKind::Case { items, .. } => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].terminator, CaseTerminator::Break);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_case_fallthrough() {
        let kind = parse_first_compound("case $x in\na) echo a;&\nb) echo b;;\nesac");
        match kind {
            CompoundCommandKind::Case { items, .. } => {
                assert_eq!(items[0].terminator, CaseTerminator::FallThrough);
                assert_eq!(items[1].terminator, CaseTerminator::Break);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_case_multiple_patterns() {
        let kind = parse_first_compound("case $x in\na|b|c) echo match;;\nesac");
        match kind {
            CompoundCommandKind::Case { items, .. } => {
                assert_eq!(items[0].patterns.len(), 3);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_case_empty() {
        let kind = parse_first_compound("case $x in\nesac");
        match kind {
            CompoundCommandKind::Case { items, .. } => assert!(items.is_empty()),
            _ => panic!(),
        }
    }

    #[test]
    fn test_brace_group() {
        let kind = parse_first_compound("{ echo hello; }");
        assert!(matches!(kind, CompoundCommandKind::BraceGroup { .. }));
    }

    #[test]
    fn test_subshell() {
        let kind = parse_first_compound("(echo hello)");
        assert!(matches!(kind, CompoundCommandKind::Subshell { .. }));
    }

    #[test]
    fn test_function_def() {
        let prog = parse("myfunc() { echo hello; }");
        let cmd = &prog.commands[0].items[0].0.first.commands[0];
        match cmd {
            Command::FunctionDef(fd) => assert_eq!(fd.name, "myfunc"),
            _ => panic!(),
        }
    }

    #[test]
    fn test_function_def_with_redirect() {
        let prog = parse("myfunc() { echo hello; } > out.txt");
        let cmd = &prog.commands[0].items[0].0.first.commands[0];
        match cmd {
            Command::FunctionDef(fd) => {
                assert_eq!(fd.name, "myfunc");
                assert_eq!(fd.redirects.len(), 1);
            }
            _ => panic!(),
        }
    }

    // ---- Task 12: here-document tests ----

    #[test]
    fn test_heredoc_body() {
        let sc = parse_first_simple("cat <<EOF\nhello world\nEOF");
        assert_eq!(sc.redirects.len(), 1);
        match &sc.redirects[0].kind {
            RedirectKind::HereDoc(hd) => {
                assert_eq!(hd.body, vec![WordPart::Literal("hello world\n".to_string())]);
                assert!(!hd.strip_tabs);
            }
            _ => panic!("expected heredoc"),
        }
    }

    #[test]
    fn test_heredoc_strip_tabs() {
        let sc = parse_first_simple("cat <<-EOF\n\thello\n\tworld\n\tEOF");
        match &sc.redirects[0].kind {
            RedirectKind::HereDoc(hd) => {
                assert!(hd.strip_tabs);
                assert_eq!(hd.body, vec![WordPart::Literal("hello\nworld\n".to_string())]);
            }
            _ => panic!("expected heredoc"),
        }
    }

    #[test]
    fn test_heredoc_quoted_delimiter() {
        let sc = parse_first_simple("cat <<'EOF'\nhello $name\nEOF");
        match &sc.redirects[0].kind {
            RedirectKind::HereDoc(hd) => {
                assert_eq!(hd.body, vec![WordPart::Literal("hello $name\n".to_string())]);
            }
            _ => panic!("expected heredoc"),
        }
    }

    #[test]
    fn test_heredoc_with_command_after() {
        let prog = parse("cat <<EOF\nhello\nEOF\necho done");
        assert_eq!(prog.commands.len(), 2);
    }
}
