use super::Parser;
use super::ast::{HereDoc, Redirect, RedirectKind, Word};
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
            Token::DLess | Token::DLessDash => {
                let strip_tabs = self.current.token == Token::DLessDash;
                self.advance()?;
                let (delimiter, quoted) = self.parse_heredoc_delimiter()?;
                let id = self.lexer.register_heredoc(delimiter, strip_tabs, quoted);
                RedirectKind::HereDoc(HereDoc {
                    id,
                    body: vec![],
                    strip_tabs,
                    quoted,
                })
            }
            token => {
                let Some(make_kind) = file_redirect_kind(token) else {
                    if fd.is_some() {
                        return Err(ShellError::parse(
                            ParseErrorKind::InvalidRedirect,
                            span.line,
                            span.column,
                            "expected redirect operator after IO number",
                        ));
                    }
                    return Ok(None);
                };
                self.advance()?;
                make_kind(self.expect_word("redirect target")?)
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
}

/// The `RedirectKind` constructor for a redirect operator that takes a
/// target word (every operator except the here-document ones).
fn file_redirect_kind(token: &Token) -> Option<fn(Word) -> RedirectKind> {
    Some(match token {
        Token::Less => RedirectKind::Input,
        Token::Great => RedirectKind::Output,
        Token::DGreat => RedirectKind::Append,
        Token::Clobber => RedirectKind::OutputClobber,
        Token::LessAnd => RedirectKind::DupInput,
        Token::GreatAnd => RedirectKind::DupOutput,
        Token::LessGreat => RedirectKind::ReadWrite,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::super::ast::RedirectKind;
    use super::super::tests::parse_first_simple;

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
