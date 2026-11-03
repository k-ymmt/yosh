use super::Parser;
use super::ast::{self, Program, Word};
use crate::error::{self, ParseErrorKind, ShellError, ShellErrorKind};
use crate::lexer::Lexer;
use crate::lexer::token::{Span, Token};

impl Parser {
    /// Build a parser over a nested lexer, propagating an error from the
    /// first token instead of deferring it. `depth` is the starting
    /// compound nesting: 1 for a `$(...)` body, whose commands may be
    /// followed directly by the closing `)`.
    fn from_nested_lexer(mut lexer: Lexer, depth: usize) -> error::Result<Self> {
        let current = lexer.next_token()?;
        Ok(Parser {
            lexer,
            current,
            pre_current_pos: 0,
            init_error: None,
            depth,
        })
    }

    /// Parse a whole nested program (the body of a backquoted command
    /// substitution, already unescaped into its own input).
    pub(crate) fn parse_nested_program(lexer: Lexer) -> error::Result<Program> {
        Self::from_nested_lexer(lexer, 0)?.parse_program()
    }

    /// Parse the body of a `$(...)` command substitution from `lexer`
    /// (positioned just after the opening `(`) up to the `)` that closes
    /// it, using the full grammar so the closing parenthesis is found the
    /// way dash/ksh find it. Returns the program and the lexer, whose
    /// position is just past the closing `)`.
    ///
    /// Reaching end of input — directly or as a syntax error at EOF
    /// (`$(if true`) — is reported as an unterminated command
    /// substitution anchored at `span`, so interactive input continues
    /// on the next line.
    pub(crate) fn parse_command_sub_body(
        lexer: Lexer,
        span: Span,
    ) -> error::Result<(Program, Lexer)> {
        let unterminated = || {
            ShellError::parse(
                ParseErrorKind::UnterminatedCommandSub,
                span.line,
                span.column,
                "unterminated command substitution",
            )
        };
        let mut parser = Self::from_nested_lexer(lexer, 1)?;
        match parser.parse_until_rparen() {
            Ok(commands) if parser.current.token == Token::RParen => {
                Ok((Program { commands }, parser.lexer))
            }
            Ok(_) => Err(unterminated()),
            Err(e)
                if parser.is_at_end()
                    && matches!(
                        e.kind,
                        ShellErrorKind::Parse(
                            ParseErrorKind::UnexpectedToken | ParseErrorKind::UnexpectedEof
                        )
                    ) =>
            {
                Err(unterminated())
            }
            Err(e) => Err(e),
        }
    }

    fn parse_until_rparen(&mut self) -> error::Result<Vec<ast::CompleteCommand>> {
        self.skip_newlines()?;
        let mut commands = Vec::new();
        while !self.is_at_end() && self.current.token != Token::RParen {
            commands.push(self.parse_complete_command()?);
            self.skip_newlines()?;
        }
        Ok(commands)
    }

    pub(super) fn expect_word(&mut self, context: &str) -> error::Result<Word> {
        if let Token::Word(_) = &self.current.token {
            // Move the word out, leaving a placeholder `current.token`. The
            // placeholder is immediately overwritten by `advance()?` below on
            // the success path; if `advance()` errors, the placeholder Eof is
            // still a valid Token and is never printed (the caller propagates
            // the error rather than reading `current` again).
            let Token::Word(word) = std::mem::replace(&mut self.current.token, Token::Eof) else {
                unreachable!("matched Token::Word above")
            };
            self.advance()?;
            Ok(word)
        } else {
            let span = self.current_span();
            Err(ShellError::parse(
                ParseErrorKind::UnexpectedToken,
                span.line,
                span.column,
                format!("expected word for {}", context),
            ))
        }
    }
}

pub(crate) fn is_valid_name(s: &str) -> bool {
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

/// Scan a literal assignment-value segment and promote unquoted
/// tilde-prefixes at segment boundaries into `WordPart::Tilde` nodes.
/// Segments are delimited by `:` so that forms like `PATH=~/a:~/b`
/// expand at both tildes (POSIX §2.6.1).
///
/// If `start_at_boundary` is true, the first segment is eligible for
/// tilde recognition (as when `s` comes directly after `=` or a
/// preceding Literal that ended with `:`). If false, the leading `~`
/// (if any) is treated as a literal character. Internal `:` always
/// starts a new segment at a boundary regardless of `start_at_boundary`.
///
/// Returns the produced AST parts together with a flag indicating
/// whether `s` ended on an unquoted `:` — callers walking a multi-part
/// word use this flag to decide whether the NEXT `WordPart::Literal`
/// begins at a segment boundary.
///
/// Tildes inside quoted, escaped, or substituted parts must never
/// reach this function.
///
/// `followed_by_part`: another (quoted, escaped or expansion) part comes
/// right after `s` in the word. A tilde-prefix in the LAST segment that
/// runs to the end of `s` without a `/` then continues into that part —
/// it contains a quoted character or an expansion, so it is not a
/// tilde-prefix and the `~` stays literal (`x=~'root'`, `x=a:~$y`;
/// POSIX §2.6.1, dash/bash agree).
pub(super) fn split_tildes_in_literal(
    s: &str,
    start_at_boundary: bool,
    followed_by_part: bool,
) -> (Vec<ast::WordPart>, bool) {
    use ast::WordPart;

    fn is_name_safe(ch: char) -> bool {
        ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '-'
    }

    let mut out: Vec<WordPart> = Vec::new();
    let push_literal = |out: &mut Vec<WordPart>, s: &str| {
        if s.is_empty() {
            return;
        }
        if let Some(WordPart::Literal(last)) = out.last_mut() {
            last.push_str(s);
        } else {
            out.push(WordPart::Literal(s.to_string()));
        }
    };

    let last_segment = s.split(':').count() - 1;
    for (i, segment) in s.split(':').enumerate() {
        if i > 0 {
            push_literal(&mut out, ":");
        }
        let eligible = if i == 0 { start_at_boundary } else { true };
        if eligible && let Some(rest_after_tilde) = segment.strip_prefix('~') {
            let (user, tail, unterminated) = match rest_after_tilde.find('/') {
                Some(p) => (&rest_after_tilde[..p], &rest_after_tilde[p..], false),
                None => (rest_after_tilde, "", i == last_segment),
            };
            let runs_into_next_part = unterminated && followed_by_part;
            if !runs_into_next_part && (user.is_empty() || user.chars().all(is_name_safe)) {
                if user.is_empty() {
                    out.push(WordPart::Tilde(None));
                } else {
                    out.push(WordPart::Tilde(Some(user.to_string())));
                }
                if !tail.is_empty() {
                    push_literal(&mut out, tail);
                }
                continue;
            }
            // Fall through: segment stays as a plain literal
        }
        push_literal(&mut out, segment);
    }

    (out, s.ends_with(':'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::WordPart;

    fn lit(s: &str) -> WordPart {
        WordPart::Literal(s.to_string())
    }

    #[test]
    fn split_no_tilde_returns_single_literal() {
        assert_eq!(
            split_tildes_in_literal("foo/bar", true, false).0,
            vec![lit("foo/bar")]
        );
    }

    #[test]
    fn split_leading_tilde_only() {
        assert_eq!(
            split_tildes_in_literal("~", true, false).0,
            vec![WordPart::Tilde(None)]
        );
    }

    #[test]
    fn split_leading_tilde_slash() {
        assert_eq!(
            split_tildes_in_literal("~/bin", true, false).0,
            vec![WordPart::Tilde(None), lit("/bin")]
        );
    }

    #[test]
    fn split_leading_tilde_user() {
        assert_eq!(
            split_tildes_in_literal("~user/bin", true, false).0,
            vec![WordPart::Tilde(Some("user".to_string())), lit("/bin")]
        );
    }

    #[test]
    fn split_colon_separated_tildes() {
        assert_eq!(
            split_tildes_in_literal("~/a:~/b", true, false).0,
            vec![
                WordPart::Tilde(None),
                lit("/a:"),
                WordPart::Tilde(None),
                lit("/b"),
            ]
        );
    }

    #[test]
    fn split_middle_segment_with_tilde() {
        assert_eq!(
            split_tildes_in_literal("/usr:~/bin", true, false).0,
            vec![lit("/usr:"), WordPart::Tilde(None), lit("/bin")]
        );
    }

    #[test]
    fn split_trailing_colon() {
        assert_eq!(
            split_tildes_in_literal("~/a:", true, false).0,
            vec![WordPart::Tilde(None), lit("/a:")]
        );
    }

    #[test]
    fn split_leading_colon() {
        assert_eq!(
            split_tildes_in_literal(":~/a", true, false).0,
            vec![lit(":"), WordPart::Tilde(None), lit("/a")]
        );
    }

    #[test]
    fn split_consecutive_colons() {
        assert_eq!(
            split_tildes_in_literal("::~/a", true, false).0,
            vec![lit("::"), WordPart::Tilde(None), lit("/a")]
        );
    }

    #[test]
    fn split_mid_word_tilde_stays_literal() {
        assert_eq!(
            split_tildes_in_literal("foo~/bin", true, false).0,
            vec![lit("foo~/bin")]
        );
    }

    #[test]
    fn split_double_tilde_invalid_user() {
        assert_eq!(
            split_tildes_in_literal("~~/bin", true, false).0,
            vec![lit("~~/bin")]
        );
    }

    #[test]
    fn split_user_name_with_dot_and_dash() {
        assert_eq!(
            split_tildes_in_literal("~a.b-c/bin", true, false).0,
            vec![WordPart::Tilde(Some("a.b-c".to_string())), lit("/bin")]
        );
    }

    #[test]
    fn split_two_tildes_joined_by_colon_no_slash() {
        assert_eq!(
            split_tildes_in_literal("~:~", true, false).0,
            vec![WordPart::Tilde(None), lit(":"), WordPart::Tilde(None),]
        );
    }

    #[test]
    fn split_not_at_boundary_skips_leading_tilde() {
        assert_eq!(
            split_tildes_in_literal("~/bin", false, false),
            (vec![lit("~/bin")], false)
        );
    }

    #[test]
    fn split_not_at_boundary_then_colon_restarts() {
        assert_eq!(
            split_tildes_in_literal(":~/bin", false, false),
            (vec![lit(":"), WordPart::Tilde(None), lit("/bin")], false)
        );
    }

    #[test]
    fn split_returns_ends_with_colon_flag() {
        assert_eq!(
            split_tildes_in_literal("a:", true, false),
            (vec![lit("a:")], true)
        );
    }

    #[test]
    fn split_tilde_running_into_next_part_stays_literal() {
        // `x=~'root'` / `x=a:~$y`: the tilde-prefix continues into a
        // quoted / expansion part, so it is not a tilde-prefix.
        assert_eq!(split_tildes_in_literal("~", true, true).0, vec![lit("~")]);
        assert_eq!(
            split_tildes_in_literal("a:~", true, true).0,
            vec![lit("a:~")]
        );
        // A `/` or `:` terminates the prefix before the next part.
        assert_eq!(
            split_tildes_in_literal("~/", true, true).0,
            vec![WordPart::Tilde(None), lit("/")]
        );
        assert_eq!(
            split_tildes_in_literal("~:", true, true).0,
            vec![WordPart::Tilde(None), lit(":")]
        );
    }
}
