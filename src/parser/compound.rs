use super::Parser;
use super::ast::{CaseItem, CaseTerminator, CompleteCommand, CompoundCommand, CompoundCommandKind};
use super::word::is_valid_name;
use crate::error::{self, ParseErrorKind, ShellError};
use crate::lexer::token::Token;

impl Parser {
    pub(super) fn parse_compound_command(&mut self) -> error::Result<CompoundCommand> {
        let line = self.current.span.line;
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
            return Err(ShellError::parse(
                ParseErrorKind::UnexpectedToken,
                span.line,
                span.column,
                "expected compound command",
            ));
        };
        Ok(CompoundCommand {
            kind,
            line,
            assignments: Vec::new(),
        })
    }

    /// Parse a compound_list: skip newlines, then parse complete_commands until at_end or is_complete_command_end.
    ///
    /// POSIX §2.10 requires at least one `and_or`. If the list would be
    /// empty, returns a parse error of the form
    /// `syntax error: empty compound list in {context}` so callers can
    /// surface context-aware diagnostics.
    pub(super) fn parse_compound_list(
        &mut self,
        context: &str,
    ) -> error::Result<Vec<CompleteCommand>> {
        self.skip_newlines()?;
        let mut commands = Vec::new();
        while !self.is_at_end() && !self.is_complete_command_end() {
            let cmd = self.parse_complete_command()?;
            commands.push(cmd);
            self.skip_newlines()?;
        }
        if commands.is_empty() {
            let span = self.current_span();
            return Err(ShellError::parse(
                ParseErrorKind::UnexpectedToken,
                span.line,
                span.column,
                format!("syntax error: empty compound list in {context}"),
            ));
        }
        Ok(commands)
    }

    /// Parse: if compound_list then compound_list [elif compound_list then compound_list]... [else compound_list] fi
    pub(super) fn parse_if_clause(&mut self) -> error::Result<CompoundCommandKind> {
        self.expect_reserved("if")?;
        let condition = self.parse_compound_list("'if' condition")?;
        self.expect_reserved("then")?;
        let then_part = self.parse_compound_list("'then' body")?;

        let mut elif_parts = Vec::new();
        let mut else_part = None;

        loop {
            if self.is_reserved("elif") {
                self.advance()?;
                let elif_cond = self.parse_compound_list("'elif' condition")?;
                self.expect_reserved("then")?;
                let elif_body = self.parse_compound_list("'elif' body")?;
                elif_parts.push((elif_cond, elif_body));
            } else if self.is_reserved("else") {
                self.advance()?;
                else_part = Some(self.parse_compound_list("'else' body")?);
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
    pub(super) fn parse_for_clause(&mut self) -> error::Result<CompoundCommandKind> {
        self.expect_reserved("for")?;

        // Expect a valid variable name
        let var = match &self.current.token.clone() {
            Token::Word(word) => {
                let name = word.as_literal().ok_or_else(|| {
                    let span = self.current_span();
                    ShellError::parse(
                        ParseErrorKind::UnexpectedToken,
                        span.line,
                        span.column,
                        "expected valid variable name after 'for'",
                    )
                })?;
                if !is_valid_name(name) {
                    let span = self.current_span();
                    return Err(ShellError::parse(
                        ParseErrorKind::UnexpectedToken,
                        span.line,
                        span.column,
                        format!("'{}' is not a valid variable name", name),
                    ));
                }
                if crate::lexer::reserved::is_posix_reserved_word(name) {
                    let span = self.current_span();
                    return Err(ShellError::parse(
                        ParseErrorKind::UnexpectedToken,
                        span.line,
                        span.column,
                        format!(
                            "'{}' is a reserved word and cannot be used as a for-loop variable name",
                            name
                        ),
                    ));
                }
                let name = name.to_string();
                self.advance()?;
                name
            }
            _ => {
                let span = self.current_span();
                return Err(ShellError::parse(
                    ParseErrorKind::UnexpectedToken,
                    span.line,
                    span.column,
                    "expected variable name after 'for'",
                ));
            }
        };

        self.skip_newlines()?;

        let words = if self.is_reserved("in") {
            self.advance()?;
            // Read words until ; or newline. POSIX requires a sequential
            // separator before `do`, so `do` here is an ordinary list word
            // (`for i in a b do` without a separator then fails in
            // parse_do_group, matching bash/dash).
            let mut word_list = Vec::new();
            loop {
                if self.is_at_end()
                    || self.current.token == Token::Semi
                    || self.current.token == Token::Newline
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
    pub(super) fn parse_do_group(&mut self) -> error::Result<Vec<CompleteCommand>> {
        self.expect_reserved("do")?;
        let body = self.parse_compound_list("'do' body")?;
        self.expect_reserved("done")?;
        Ok(body)
    }

    /// Parse: while compound_list do compound_list done
    pub(super) fn parse_while_clause(&mut self) -> error::Result<CompoundCommandKind> {
        self.expect_reserved("while")?;
        let condition = self.parse_compound_list("'while' condition")?;
        let body = self.parse_do_group()?;
        Ok(CompoundCommandKind::While { condition, body })
    }

    /// Parse: until compound_list do compound_list done
    pub(super) fn parse_until_clause(&mut self) -> error::Result<CompoundCommandKind> {
        self.expect_reserved("until")?;
        let condition = self.parse_compound_list("'until' condition")?;
        let body = self.parse_do_group()?;
        Ok(CompoundCommandKind::Until { condition, body })
    }

    /// Parse: case word in [pattern [| pattern]... ) compound-list ;; ]... esac
    pub(super) fn parse_case_clause(&mut self) -> error::Result<CompoundCommandKind> {
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
                return Err(ShellError::parse(
                    ParseErrorKind::UnexpectedToken,
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
    pub(super) fn parse_brace_group(&mut self) -> error::Result<CompoundCommandKind> {
        self.expect_reserved("{")?;
        let body = self.parse_compound_list("brace group")?;
        self.expect_reserved("}")?;
        Ok(CompoundCommandKind::BraceGroup { body })
    }

    /// Parse: ( compound_list )
    pub(super) fn parse_subshell(&mut self) -> error::Result<CompoundCommandKind> {
        self.eat(&Token::LParen)?;
        let body = self.parse_compound_list("subshell")?;
        if !self.eat(&Token::RParen)? {
            let span = self.current_span();
            return Err(ShellError::parse(
                ParseErrorKind::UnexpectedToken,
                span.line,
                span.column,
                "expected ')' to close subshell",
            ));
        }
        Ok(CompoundCommandKind::Subshell { body })
    }
}

#[cfg(test)]
mod tests {
    use super::super::Parser;
    use super::super::ast::{self, Command, CompoundCommandKind};
    use super::super::tests::parse;
    use crate::error::ShellError;

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
                assert_eq!(items[0].terminator, ast::CaseTerminator::Break);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_case_fallthrough() {
        let kind = parse_first_compound("case $x in\na) echo a;&\nb) echo b;;\nesac");
        match kind {
            CompoundCommandKind::Case { items, .. } => {
                assert_eq!(items[0].terminator, ast::CaseTerminator::FallThrough);
                assert_eq!(items[1].terminator, ast::CaseTerminator::Break);
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

    // ── empty compound_list rejection (POSIX §2.10) ─────────────

    fn parse_err(source: &str) -> ShellError {
        Parser::new(source).parse_program().unwrap_err()
    }

    fn parse_ok(source: &str) {
        Parser::new(source)
            .parse_program()
            .unwrap_or_else(|e| panic!("expected OK, got: {e}"));
    }

    #[test]
    fn empty_if_then_errors() {
        let err = parse_err("if true; then fi\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("syntax"), "message: {s}");
        assert!(s.contains("'then' body"), "message: {s}");
    }

    #[test]
    fn empty_if_condition_errors() {
        let err = parse_err("if then true; fi\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("syntax"), "message: {s}");
        assert!(s.contains("'if' condition"), "message: {s}");
    }

    #[test]
    fn empty_elif_condition_errors() {
        let err = parse_err("if true; then :; elif then :; fi\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("'elif' condition"), "message: {s}");
    }

    #[test]
    fn empty_elif_body_errors() {
        let err = parse_err("if true; then :; elif true; then fi\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("'elif' body"), "message: {s}");
    }

    #[test]
    fn empty_else_body_errors() {
        let err = parse_err("if true; then :; else fi\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("'else' body"), "message: {s}");
    }

    #[test]
    fn empty_while_condition_errors() {
        let err = parse_err("while do done\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("'while' condition"), "message: {s}");
    }

    #[test]
    fn empty_while_body_errors() {
        let err = parse_err("while true; do done\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("'do' body"), "message: {s}");
    }

    #[test]
    fn empty_until_condition_errors() {
        let err = parse_err("until do done\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("'until' condition"), "message: {s}");
    }

    #[test]
    fn empty_until_body_errors() {
        let err = parse_err("until false; do done\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("'do' body"), "message: {s}");
    }

    #[test]
    fn empty_for_body_errors() {
        let err = parse_err("for i in a b; do done\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("'do' body"), "message: {s}");
    }

    #[test]
    fn empty_brace_group_errors() {
        let err = parse_err("{ }\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("brace group"), "message: {s}");
    }

    #[test]
    fn empty_subshell_errors() {
        let err = parse_err("( )\n");
        assert_eq!(err.exit_code(), 2);
        let s = err.to_string();
        assert!(s.contains("subshell"), "message: {s}");
    }

    #[test]
    fn nonempty_if_parses_ok() {
        parse_ok("if true; then :; fi\n");
    }

    #[test]
    fn nonempty_while_parses_ok() {
        parse_ok("while false; do :; done\n");
    }

    #[test]
    fn nonempty_until_parses_ok() {
        parse_ok("until true; do :; done\n");
    }

    #[test]
    fn nonempty_for_parses_ok() {
        parse_ok("for i in a b; do :; done\n");
    }

    #[test]
    fn nonempty_brace_group_parses_ok() {
        parse_ok("{ :; }\n");
    }

    #[test]
    fn nonempty_subshell_parses_ok() {
        parse_ok("( : )\n");
    }

    #[test]
    fn case_empty_body_still_parses_ok() {
        parse_ok("case x in pat) ;; esac\n");
    }

    #[test]
    fn comment_only_body_errors_per_posix() {
        let err = parse_err("if true; then\n#only comment\nfi\n");
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("'then' body"));
    }

    // ── LINENO line-capture tests ───────────────────────────────

    fn first_compound_cmd(source: &str) -> ast::CompoundCommand {
        let program = Parser::new(source)
            .parse_program()
            .expect("source should parse");
        let cc = program
            .commands
            .into_iter()
            .next()
            .expect("program should contain at least one CompleteCommand");
        let (aol, _) = cc
            .items
            .into_iter()
            .next()
            .expect("CompleteCommand should contain at least one AndOrList");
        let cmd = aol
            .first
            .commands
            .into_iter()
            .next()
            .expect("Pipeline should contain at least one Command");
        match cmd {
            Command::Compound(c, _) => c,
            _ => panic!("expected compound command"),
        }
    }

    #[test]
    fn parse_compound_if_captures_line() {
        let cmd = first_compound_cmd("if true; then :; fi\n");
        assert_eq!(cmd.line, 1);
        assert!(matches!(cmd.kind, CompoundCommandKind::If { .. }));
    }

    #[test]
    fn parse_compound_if_on_second_line() {
        let cmd = first_compound_cmd("\nif true; then :; fi\n");
        assert_eq!(cmd.line, 2);
    }

    #[test]
    fn parse_brace_group_captures_line() {
        let cmd = first_compound_cmd("{ :; }\n");
        assert_eq!(cmd.line, 1);
        assert!(matches!(cmd.kind, CompoundCommandKind::BraceGroup { .. }));
    }

    #[test]
    fn parse_subshell_captures_line() {
        let cmd = first_compound_cmd("( :; )\n");
        assert_eq!(cmd.line, 1);
        assert!(matches!(cmd.kind, CompoundCommandKind::Subshell { .. }));
    }

    #[test]
    fn parse_while_captures_line() {
        let cmd = first_compound_cmd("while true; do :; done\n");
        assert_eq!(cmd.line, 1);
        assert!(matches!(cmd.kind, CompoundCommandKind::While { .. }));
    }

    #[test]
    fn parse_nested_if_then_captures_body_line() {
        let outer = first_compound_cmd("if true; then\necho hi\nfi\n");
        assert_eq!(outer.line, 1);
        if let CompoundCommandKind::If { then_part, .. } = &outer.kind {
            let inner_cc = then_part.first().expect("then body non-empty");
            let (inner_aol, _) = inner_cc.items.first().expect("inner AOL");
            let inner_cmd = inner_aol.first.commands.first().expect("inner cmd");
            if let Command::Simple(inner_simple) = inner_cmd {
                assert_eq!(inner_simple.line, 2);
            } else {
                panic!("expected inner simple command");
            }
        } else {
            panic!("expected If kind");
        }
    }

    #[test]
    fn parse_for_reserved_word_if_rejected() {
        // POSIX §2.10.2 Rule 5: NAME in `for` must not be a reserved word.
        // `if` passes `is_valid_name`, so the only rejection path is
        // Rule 5 — pin that exact message.
        let src = "for if in a; do :; done\n";
        let err = Parser::new(src).parse_program().unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("reserved word"),
            "expected reserved-word error, got: {}",
            msg
        );
    }

    #[test]
    fn parse_for_reserved_word_in_rejected() {
        let src = "for in in a; do :; done\n";
        let err = Parser::new(src).parse_program().unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("reserved word"),
            "expected reserved-word error, got: {}",
            msg
        );
    }

    #[test]
    fn parse_for_valid_name_ok() {
        // Regression: a plain identifier NAME continues to parse cleanly.
        let src = "for i in a b c; do echo $i; done\n";
        assert!(
            Parser::new(src).parse_program().is_ok(),
            "valid for-loop should parse"
        );
    }

    #[test]
    fn parse_for_time_word_ok() {
        // POSIX §2.4 RESERVED_WORDS does NOT include `time` (that is a bash
        // extension from pipeline-prefix context). `for time in ...` must
        // therefore still parse in yosh.
        let src = "for time in a; do :; done\n";
        assert!(
            Parser::new(src).parse_program().is_ok(),
            "'for time' should parse because `time` is not in RESERVED_WORDS"
        );
    }
}
