use crate::error::{self, ParseErrorKind, ShellError};
use crate::lexer::token::Token;
use super::Parser;
use super::ast::{HereDoc, Redirect, RedirectKind, Word, WordPart};

impl Parser {
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
                WordPart::EscapedLiteral(s) => {
                    // Per POSIX §2.7.4, any escape in the heredoc delimiter word
                    // marks the heredoc as quoted (body expansion disabled) and
                    // the escaped character is part of the delimiter itself.
                    delimiter.push_str(s);
                    quoted = true;
                }
                WordPart::SingleQuoted(s) => {
                    delimiter.push_str(s);
                    quoted = true;
                }
                WordPart::DoubleQuoted(parts) => {
                    quoted = true;
                    for p in parts {
                        match p {
                            WordPart::Literal(s) | WordPart::EscapedLiteral(s) => {
                                delimiter.push_str(s);
                            }
                            _ => {}
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

    pub(super) fn fill_heredoc_bodies(&mut self, redirects: &mut Vec<Redirect>) {
        for redir in redirects {
            if let RedirectKind::HereDoc(ref mut hd) = redir.kind
                && hd.body.is_empty()
                && let Some(body) = self.lexer.take_heredoc_body()
            {
                hd.body = body;
            }
        }
    }
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
