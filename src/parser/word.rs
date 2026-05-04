use super::Parser;
use super::ast::{self, Word};
use crate::error::{self, ParseErrorKind, ShellError};
use crate::lexer::token::Token;

impl Parser {
    pub(super) fn expect_word(&mut self, context: &str) -> error::Result<Word> {
        if let Token::Word(word) = &self.current.token.clone() {
            let word = word.clone();
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

pub(super) fn is_valid_name(s: &str) -> bool {
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
pub(super) fn split_tildes_in_literal(
    s: &str,
    start_at_boundary: bool,
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

    for (i, segment) in s.split(':').enumerate() {
        if i > 0 {
            push_literal(&mut out, ":");
        }
        let eligible = if i == 0 { start_at_boundary } else { true };
        if eligible && let Some(rest_after_tilde) = segment.strip_prefix('~') {
            let (user, tail) = match rest_after_tilde.find('/') {
                Some(p) => (&rest_after_tilde[..p], &rest_after_tilde[p..]),
                None => (rest_after_tilde, ""),
            };
            if user.is_empty() || user.chars().all(is_name_safe) {
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
            split_tildes_in_literal("foo/bar", true).0,
            vec![lit("foo/bar")]
        );
    }

    #[test]
    fn split_leading_tilde_only() {
        assert_eq!(
            split_tildes_in_literal("~", true).0,
            vec![WordPart::Tilde(None)]
        );
    }

    #[test]
    fn split_leading_tilde_slash() {
        assert_eq!(
            split_tildes_in_literal("~/bin", true).0,
            vec![WordPart::Tilde(None), lit("/bin")]
        );
    }

    #[test]
    fn split_leading_tilde_user() {
        assert_eq!(
            split_tildes_in_literal("~user/bin", true).0,
            vec![WordPart::Tilde(Some("user".to_string())), lit("/bin")]
        );
    }

    #[test]
    fn split_colon_separated_tildes() {
        assert_eq!(
            split_tildes_in_literal("~/a:~/b", true).0,
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
            split_tildes_in_literal("/usr:~/bin", true).0,
            vec![lit("/usr:"), WordPart::Tilde(None), lit("/bin")]
        );
    }

    #[test]
    fn split_trailing_colon() {
        assert_eq!(
            split_tildes_in_literal("~/a:", true).0,
            vec![WordPart::Tilde(None), lit("/a:")]
        );
    }

    #[test]
    fn split_leading_colon() {
        assert_eq!(
            split_tildes_in_literal(":~/a", true).0,
            vec![lit(":"), WordPart::Tilde(None), lit("/a")]
        );
    }

    #[test]
    fn split_consecutive_colons() {
        assert_eq!(
            split_tildes_in_literal("::~/a", true).0,
            vec![lit("::"), WordPart::Tilde(None), lit("/a")]
        );
    }

    #[test]
    fn split_mid_word_tilde_stays_literal() {
        assert_eq!(
            split_tildes_in_literal("foo~/bin", true).0,
            vec![lit("foo~/bin")]
        );
    }

    #[test]
    fn split_double_tilde_invalid_user() {
        assert_eq!(
            split_tildes_in_literal("~~/bin", true).0,
            vec![lit("~~/bin")]
        );
    }

    #[test]
    fn split_user_name_with_dot_and_dash() {
        assert_eq!(
            split_tildes_in_literal("~a.b-c/bin", true).0,
            vec![WordPart::Tilde(Some("a.b-c".to_string())), lit("/bin")]
        );
    }

    #[test]
    fn split_two_tildes_joined_by_colon_no_slash() {
        assert_eq!(
            split_tildes_in_literal("~:~", true).0,
            vec![WordPart::Tilde(None), lit(":"), WordPart::Tilde(None),]
        );
    }

    #[test]
    fn split_not_at_boundary_skips_leading_tilde() {
        assert_eq!(
            split_tildes_in_literal("~/bin", false),
            (vec![lit("~/bin")], false)
        );
    }

    #[test]
    fn split_not_at_boundary_then_colon_restarts() {
        assert_eq!(
            split_tildes_in_literal(":~/bin", false),
            (vec![lit(":"), WordPart::Tilde(None), lit("/bin")], false)
        );
    }

    #[test]
    fn split_returns_ends_with_colon_flag() {
        assert_eq!(split_tildes_in_literal("a:", true), (vec![lit("a:")], true));
    }
}
