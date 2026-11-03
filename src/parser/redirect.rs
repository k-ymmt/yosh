use super::Parser;
use super::ast;
use super::ast::{HereDoc, Redirect, RedirectKind, Word, WordPart};
use crate::error::{self, ParseErrorKind, ShellError};
use crate::lexer::token::Token;

impl Parser {
    pub(super) fn try_parse_redirect(&mut self) -> error::Result<Option<Redirect>> {
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
                let (delimiter, quoted) = self.parse_heredoc_delimiter()?;
                let id = self.lexer.register_heredoc(delimiter, false, quoted);
                RedirectKind::HereDoc(HereDoc {
                    id,
                    body: vec![],
                    strip_tabs: false,
                    quoted,
                })
            }
            Token::DLessDash => {
                self.advance()?;
                let (delimiter, quoted) = self.parse_heredoc_delimiter()?;
                let id = self.lexer.register_heredoc(delimiter, true, quoted);
                RedirectKind::HereDoc(HereDoc {
                    id,
                    body: vec![],
                    strip_tabs: true,
                    quoted,
                })
            }
            _ => {
                if fd.is_some() {
                    return Err(ShellError::parse(
                        ParseErrorKind::InvalidRedirect,
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

    pub(super) fn parse_redirect_list(&mut self) -> error::Result<Vec<Redirect>> {
        let mut redirects = Vec::new();
        while let Some(redirect) = self.try_parse_redirect()? {
            redirects.push(redirect);
        }
        Ok(redirects)
    }

    /// Read the here-document delimiter word (the current token) and return
    /// the delimiter string plus whether any part of it was quoted.
    ///
    /// POSIX §2.7.4: the delimiter is the word after quote removal only — no
    /// parameter, command, arithmetic, or tilde expansion. The lexer has
    /// already split the word into expansion parts, so the delimiter is
    /// rebuilt from the word's raw source text (`<<$x` is delimited by a
    /// `$x` line, `<<~` by `~`, `<<a$b` by `a$b`).
    fn parse_heredoc_delimiter(&mut self) -> error::Result<(String, bool)> {
        // The current token was just lexed, so its source text spans from
        // the pre-token position to the lexer's position. A token replayed
        // from an alias expansion does not advance the lexer (empty span):
        // fall back to the parts.
        let source = self
            .lexer
            .source_text(self.pre_current_pos, self.lexer.position());
        let word = self.expect_word("here-document delimiter")?;
        let quoted = word_has_quoting(&word);
        let source = trim_leading_blanks(&source);
        let delimiter = if source.is_empty() {
            delimiter_from_parts(&word)
        } else {
            delimiter_from_source(source, &word)
        };
        Ok((delimiter, quoted))
    }

    pub(super) fn fill_heredoc_bodies(&mut self, redirects: &mut Vec<Redirect>) {
        for redir in redirects {
            if let RedirectKind::HereDoc(ref mut hd) = redir.kind
                && hd.body.is_empty()
                && let Some(body) = self.lexer.take_heredoc_body(hd.id)
            {
                hd.body = body;
            }
        }
    }

    // ---- Deep heredoc-body fill ----
    //
    // A heredoc body starts after the NEXT newline token, which can arrive
    // after the registering command's AST node has already been built and
    // returned (e.g. `done <<EOF`, `cat <<A; cat <<B`, `{ ...; } <<EOF &`).
    // Once the terminating newline of a complete_command has been consumed,
    // every body for heredocs registered within it has been read, so a
    // recursive walk attaches any still-empty bodies by id.

    pub(super) fn fill_heredoc_bodies_deep(&mut self, cc: &mut ast::CompleteCommand) {
        for (aol, _) in &mut cc.items {
            self.fill_and_or(aol);
        }
    }

    fn fill_and_or(&mut self, aol: &mut ast::AndOrList) {
        self.fill_pipeline(&mut aol.first);
        for (_, p) in &mut aol.rest {
            self.fill_pipeline(p);
        }
    }

    fn fill_pipeline(&mut self, p: &mut ast::Pipeline) {
        for cmd in &mut p.commands {
            self.fill_command(cmd);
        }
    }

    fn fill_command(&mut self, cmd: &mut ast::Command) {
        match cmd {
            ast::Command::Simple(simple) => self.fill_heredoc_bodies(&mut simple.redirects),
            ast::Command::Compound(compound, redirects) => {
                self.fill_compound(compound);
                self.fill_heredoc_bodies(redirects);
            }
            ast::Command::FunctionDef(fd) => {
                // Fresh at parse time, so the Rc is unshared; get_mut cannot
                // fail here, but degrade to skipping rather than panicking.
                if let Some(body) = std::rc::Rc::get_mut(&mut fd.body) {
                    self.fill_compound(body);
                }
                self.fill_heredoc_bodies(&mut fd.redirects);
            }
        }
    }

    fn fill_compound(&mut self, compound: &mut ast::CompoundCommand) {
        use ast::CompoundCommandKind::*;
        match &mut compound.kind {
            BraceGroup { body } | Subshell { body } | For { body, .. } => self.fill_list(body),
            If {
                condition,
                then_part,
                elif_parts,
                else_part,
            } => {
                self.fill_list(condition);
                self.fill_list(then_part);
                for (cond, body) in elif_parts {
                    self.fill_list(cond);
                    self.fill_list(body);
                }
                if let Some(body) = else_part {
                    self.fill_list(body);
                }
            }
            While { condition, body } | Until { condition, body } => {
                self.fill_list(condition);
                self.fill_list(body);
            }
            Case { items, .. } => {
                for item in items {
                    self.fill_list(&mut item.body);
                }
            }
        }
    }

    fn fill_list(&mut self, list: &mut ast::CommandList) {
        for cc in list {
            self.fill_heredoc_bodies_deep(cc);
        }
    }
}

/// True if any part of the delimiter word was quoted or escaped, which
/// disables expansion of the here-document body (POSIX §2.7.4).
fn word_has_quoting(word: &Word) -> bool {
    word.parts.iter().any(|p| {
        matches!(
            p,
            WordPart::EscapedLiteral(_)
                | WordPart::SingleQuoted(_)
                | WordPart::DoubleQuoted(_)
                | WordPart::DollarSingleQuoted(_)
        )
    })
}

/// Strip the blanks (and line continuations) the lexer skipped before the
/// delimiter word.
fn trim_leading_blanks(mut s: &str) -> &str {
    loop {
        if let Some(rest) = s.strip_prefix([' ', '\t']) {
            s = rest;
        } else if let Some(rest) = s.strip_prefix("\\\n") {
            s = rest;
        } else {
            return s;
        }
    }
}

/// Apply quote removal to the delimiter's raw source text. `$'...'`
/// segments take their decoded value from the word's matching
/// `DollarSingleQuoted` parts (in order).
fn delimiter_from_source(source: &str, word: &Word) -> String {
    let mut dollar_single = word.parts.iter().filter_map(|p| match p {
        WordPart::DollarSingleQuoted(s) => Some(s.as_str()),
        _ => None,
    });
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('\n') => {}
                Some(n) => out.push(n),
                None => out.push('\\'),
            },
            '\'' => {
                for n in chars.by_ref() {
                    if n == '\'' {
                        break;
                    }
                    out.push(n);
                }
            }
            '"' => {
                while let Some(n) = chars.next() {
                    match n {
                        '"' => break,
                        '\\' => match chars.peek().copied() {
                            Some(e @ ('$' | '`' | '"' | '\\')) => {
                                out.push(e);
                                chars.next();
                            }
                            Some('\n') => {
                                chars.next();
                            }
                            _ => out.push('\\'),
                        },
                        _ => out.push(n),
                    }
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                while let Some(n) = chars.next() {
                    match n {
                        '\\' => {
                            chars.next();
                        }
                        '\'' => break,
                        _ => {}
                    }
                }
                out.push_str(dollar_single.next().unwrap_or(""));
            }
            _ => out.push(c),
        }
    }
    out
}

/// Fallback when no source text is available (delimiter word produced by
/// alias substitution): concatenate the literal and quoted parts.
fn delimiter_from_parts(word: &Word) -> String {
    let mut delimiter = String::new();
    for part in &word.parts {
        match part {
            WordPart::Literal(s)
            | WordPart::EscapedLiteral(s)
            | WordPart::SingleQuoted(s)
            | WordPart::DollarSingleQuoted(s) => delimiter.push_str(s),
            WordPart::DoubleQuoted(parts) => {
                for p in parts {
                    if let WordPart::Literal(s) | WordPart::EscapedLiteral(s) = p {
                        delimiter.push_str(s);
                    }
                }
            }
            WordPart::Tilde(user) => {
                delimiter.push('~');
                delimiter.push_str(user.as_deref().unwrap_or(""));
            }
            _ => {}
        }
    }
    delimiter
}

#[cfg(test)]
mod tests {
    use super::super::ast::{RedirectKind, WordPart};
    use super::super::tests::{parse, parse_first_simple};

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

    #[test]
    fn test_heredoc_body() {
        let sc = parse_first_simple("cat <<EOF\nhello world\nEOF");
        assert_eq!(sc.redirects.len(), 1);
        match &sc.redirects[0].kind {
            RedirectKind::HereDoc(hd) => {
                assert_eq!(
                    hd.body,
                    vec![WordPart::Literal("hello world\n".to_string())]
                );
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
                assert_eq!(
                    hd.body,
                    vec![WordPart::Literal("hello\nworld\n".to_string())]
                );
            }
            _ => panic!("expected heredoc"),
        }
    }

    #[test]
    fn test_heredoc_quoted_delimiter() {
        let sc = parse_first_simple("cat <<'EOF'\nhello $name\nEOF");
        match &sc.redirects[0].kind {
            RedirectKind::HereDoc(hd) => {
                assert_eq!(
                    hd.body,
                    vec![WordPart::Literal("hello $name\n".to_string())]
                );
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
