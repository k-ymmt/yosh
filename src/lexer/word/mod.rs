//! Word tokenization: the main word-part loop and the dispatch to one
//! submodule per construct (quotes, tilde-prefixes, `$` parameter
//! expansion, command substitution, arithmetic expansion).

mod arith;
mod command_sub;
mod param;
mod quote;
mod tilde;

use super::Lexer;
use super::scanner::{DQUOTED_PLAIN, UNQUOTED_PLAIN};
use super::token::{SpannedToken, Token};
use crate::error::{self, ParseErrorKind, ShellError};
use crate::parser::ast::{Word, WordPart};

/// Flush the accumulated literal run into `parts` (no-op when empty).
/// `read_word_parts` calls this before emitting any non-literal part.
#[inline]
fn flush_literal(parts: &mut Vec<WordPart>, literal: &mut String) {
    if !literal.is_empty() {
        parts.push(WordPart::Literal(std::mem::take(literal)));
    }
}

impl Lexer {
    // ---- Word construction methods ----

    /// Main word-part reader that loops over input, dispatching to quoting methods.
    /// `in_double_quote`: true when inside "..." (or, with `end_byte`, for
    /// the word of a `${x-word}`-style expansion that is itself inside
    /// double quotes / a here-document)
    /// `end_byte`: if Some(b'}'), stop at unquoted `}` (for ${...} parsing)
    pub fn read_word_parts(
        &mut self,
        in_double_quote: bool,
        end_byte: Option<u8>,
    ) -> error::Result<Vec<WordPart>> {
        self.nest_depth += 1;
        let result = match self.check_nest_depth() {
            Ok(_stack) => self.read_word_parts_inner(in_double_quote, end_byte),
            Err(e) => Err(e),
        };
        self.nest_depth -= 1;
        result
    }

    fn read_word_parts_inner(
        &mut self,
        in_double_quote: bool,
        end_byte: Option<u8>,
    ) -> error::Result<Vec<WordPart>> {
        let mut parts: Vec<WordPart> = Vec::new();
        let mut literal = String::new();

        loop {
            if self.at_end() {
                break;
            }

            let ch = self.current_byte();

            // check end_byte (e.g. '}' for ${...})
            if let Some(end) = end_byte
                && ch == end
            {
                break;
            }

            if in_double_quote {
                if ch == b'"' {
                    if end_byte.is_none() {
                        // inside double quotes: stop at closing "
                        break;
                    }
                    // `"${x-"a b"}"`: a double quote inside the braces of
                    // a double-quoted expansion opens a nested quoted
                    // string (bash/dash/ksh agree).
                    flush_literal(&mut parts, &mut literal);
                    let part = self.read_double_quote()?;
                    parts.push(part);
                    continue;
                }
                match ch {
                    b'$' => {
                        flush_literal(&mut parts, &mut literal);
                        let part = self.read_dollar(true)?;
                        parts.push(part);
                    }
                    b'\\' => {
                        flush_literal(&mut parts, &mut literal);
                        let part = self.read_backslash_in_double_quote()?;
                        parts.push(part);
                    }
                    b'`' => {
                        flush_literal(&mut parts, &mut literal);
                        let part = self.read_backtick(true)?;
                        parts.push(part);
                    }
                    _ => {
                        self.push_plain_run(&mut literal, &DQUOTED_PLAIN)?;
                    }
                }
            } else {
                // unquoted context
                // Inside ${...} (end_byte == '}'), spaces are literal, not delimiters
                if end_byte == Some(b'}') {
                    if ch == b'}' {
                        break;
                    }
                } else if Self::is_meta_or_whitespace(ch) {
                    break;
                }
                match ch {
                    b'\'' => {
                        flush_literal(&mut parts, &mut literal);
                        let part = self.read_single_quote()?;
                        parts.push(part);
                    }
                    b'"' => {
                        flush_literal(&mut parts, &mut literal);
                        let part = self.read_double_quote()?;
                        parts.push(part);
                    }
                    b'\\' => {
                        // `\<newline>` line continuations were already
                        // removed by `advance`, so this is a `\<char>`
                        // escape (flush and emit EscapedLiteral).
                        flush_literal(&mut parts, &mut literal);
                        let part = self.read_escape_unquoted()?;
                        parts.push(part);
                    }
                    b'$' => {
                        flush_literal(&mut parts, &mut literal);
                        let part = self.read_dollar(false)?;
                        parts.push(part);
                    }
                    b'`' => {
                        flush_literal(&mut parts, &mut literal);
                        let part = self.read_backtick(false)?;
                        parts.push(part);
                    }
                    b'~' if parts.is_empty() && literal.is_empty() => {
                        match self.read_tilde(end_byte == Some(b'}'))? {
                            Some(part) => parts.push(part),
                            // Not a tilde-prefix: `~` is an ordinary character.
                            None => literal.push('~'),
                        }
                    }
                    _ => {
                        self.push_plain_run(&mut literal, &UNQUOTED_PLAIN)?;
                    }
                }
            }
        }

        if !literal.is_empty() {
            parts.push(WordPart::Literal(literal));
        }

        // Filter out empty Literal("") parts (defensive: a `\<newline>`
        // reaching `read_backslash_in_double_quote` yields one). This is
        // rare, so only pay for the rebuild when such a part actually
        // exists.
        if parts
            .iter()
            .any(|p| matches!(p, WordPart::Literal(s) if s.is_empty()))
        {
            parts.retain(|p| !matches!(p, WordPart::Literal(s) if s.is_empty()));
        }

        Ok(parts)
    }

    pub(crate) fn read_word(&mut self) -> error::Result<SpannedToken> {
        let span = self.current_span();
        let parts = self.read_word_parts(false, None)?;
        if parts.is_empty() {
            return Err(ShellError::parse(
                ParseErrorKind::UnexpectedToken,
                span.line,
                span.column,
                format!("unexpected character: '{}'", self.current_byte() as char),
            ));
        }
        Ok(SpannedToken {
            token: Token::Word(Word { parts }),
            span,
        })
    }
}

// ─── Unit tests ──────────────────────────────────────────────────────────────
//
// Characterization tests for `read_word_parts`: they pin the current
// tokenization (part shapes and error line/column) so mechanical refactors
// inside this module have a direct gate. Higher-level behavior is covered by
// the parser integration tests and e2e.

#[cfg(test)]
mod tests {
    use super::super::Lexer;
    use crate::error::{ParseErrorKind, ShellError, ShellErrorKind};
    use crate::parser::ast::{ParamExpr, Word, WordPart};

    fn parts(input: &str) -> Vec<WordPart> {
        Lexer::new(input).read_word_parts(false, None).unwrap()
    }

    fn parse_err(input: &str) -> ShellError {
        Lexer::new(input).read_word_parts(false, None).unwrap_err()
    }

    fn lit(s: &str) -> WordPart {
        WordPart::Literal(s.to_string())
    }

    fn word(parts: Vec<WordPart>) -> Word {
        Word { parts }
    }

    #[test]
    fn plain_word_single_literal() {
        assert_eq!(parts("hello"), vec![lit("hello")]);
    }

    #[test]
    fn word_stops_at_whitespace_and_metachar() {
        assert_eq!(parts("ab cd"), vec![lit("ab")]);
        assert_eq!(parts("ab|cd"), vec![lit("ab")]);
    }

    #[test]
    fn single_quoted() {
        assert_eq!(
            parts("'a b'"),
            vec![WordPart::SingleQuoted("a b".to_string())]
        );
    }

    #[test]
    fn double_quoted_with_param() {
        assert_eq!(
            parts("\"a $x\""),
            vec![WordPart::DoubleQuoted(vec![
                lit("a "),
                WordPart::Parameter(ParamExpr::Simple("x".to_string())),
            ])]
        );
    }

    #[test]
    fn mixed_word_flushes_literal_between_parts() {
        assert_eq!(
            parts("a$x'q'b"),
            vec![
                lit("a"),
                WordPart::Parameter(ParamExpr::Simple("x".to_string())),
                WordPart::SingleQuoted("q".to_string()),
                lit("b"),
            ]
        );
    }

    #[test]
    fn braced_default_expansion() {
        assert_eq!(
            parts("${x:-d}"),
            vec![WordPart::Parameter(ParamExpr::Default {
                name: "x".to_string(),
                word: Some(word(vec![lit("d")])),
                null_check: true,
            })]
        );
    }

    #[test]
    fn braced_length_expansion() {
        assert_eq!(
            parts("${#x}"),
            vec![WordPart::Parameter(ParamExpr::Length("x".to_string()))]
        );
    }

    #[test]
    fn braced_strip_short_suffix() {
        assert_eq!(
            parts("${x%p}"),
            vec![WordPart::Parameter(ParamExpr::StripShortSuffix(
                "x".to_string(),
                word(vec![lit("p")]),
            ))]
        );
    }

    #[test]
    fn arith_expansion_trimmed_expr() {
        assert_eq!(
            parts("$((1+2))"),
            vec![WordPart::ArithSub("1+2".to_string())]
        );
        assert_eq!(
            parts("$(( 1 + 2 ))"),
            vec![WordPart::ArithSub("1 + 2".to_string())]
        );
    }

    #[test]
    fn command_substitution_dollar_form() {
        let p = parts("$(echo hi)");
        assert_eq!(p.len(), 1);
        assert!(
            matches!(&p[0], WordPart::CommandSub(prog) if prog.commands.len() == 1),
            "got: {:?}",
            p
        );
    }

    #[test]
    fn command_substitution_backtick_form() {
        let p = parts("`echo hi`");
        assert_eq!(p.len(), 1);
        assert!(
            matches!(&p[0], WordPart::CommandSub(prog) if prog.commands.len() == 1),
            "got: {:?}",
            p
        );
    }

    #[test]
    fn unquoted_escape_emits_escaped_literal() {
        assert_eq!(
            parts(r"\a"),
            vec![WordPart::EscapedLiteral("a".to_string())]
        );
        assert_eq!(
            parts(r"a\ b"),
            vec![
                lit("a"),
                WordPart::EscapedLiteral(" ".to_string()),
                lit("b")
            ]
        );
    }

    #[test]
    fn line_continuation_is_transparent() {
        assert_eq!(parts("a\\\nb"), vec![lit("ab")]);
    }

    #[test]
    fn unterminated_param_expansion_error_location() {
        let e = parse_err("${x");
        assert_eq!(
            e.kind,
            ShellErrorKind::Parse(ParseErrorKind::UnterminatedParamExpansion)
        );
        let loc = e.location.expect("param error carries a location");
        assert_eq!((loc.line, loc.column), (1, 2));
    }

    #[test]
    fn unterminated_single_quote_error_location() {
        let e = parse_err("'abc");
        assert_eq!(
            e.kind,
            ShellErrorKind::Parse(ParseErrorKind::UnterminatedSingleQuote)
        );
        let loc = e.location.expect("quote error carries a location");
        assert_eq!((loc.line, loc.column), (1, 1));
    }

    #[test]
    fn unterminated_double_quote_error_location() {
        let e = parse_err("\"abc");
        assert_eq!(
            e.kind,
            ShellErrorKind::Parse(ParseErrorKind::UnterminatedDoubleQuote)
        );
        let loc = e.location.expect("quote error carries a location");
        assert_eq!((loc.line, loc.column), (1, 1));
    }

    fn tokens(input: &str) -> Vec<crate::lexer::token::Token> {
        let mut lexer = Lexer::new(input);
        let mut out = Vec::new();
        loop {
            let t = lexer.next_token().unwrap().token;
            if t == crate::lexer::token::Token::Eof {
                break;
            }
            out.push(t);
        }
        out
    }

    #[test]
    fn line_continuation_before_blank_and_inside_operators() {
        use crate::lexer::token::Token;
        assert_eq!(
            tokens("a \\\n  b"),
            vec![
                Token::Word(word(vec![lit("a")])),
                Token::Word(word(vec![lit("b")]))
            ]
        );
        assert_eq!(
            tokens("&\\\n& |\\\n| <\\\n<"),
            vec![Token::AndIf, Token::OrIf, Token::DLess]
        );
        assert_eq!(
            parts("$\\\nx${HO\\\nME}"),
            vec![
                WordPart::Parameter(ParamExpr::Simple("x".to_string())),
                WordPart::Parameter(ParamExpr::Simple("HOME".to_string())),
            ]
        );
    }

    #[test]
    fn line_continuation_kept_in_single_quotes_and_after_escape() {
        assert_eq!(
            parts("'a\\\nb'"),
            vec![WordPart::SingleQuoted("a\\\nb".to_string())]
        );
        // `\\` escapes the backslash; the newline then ends the word.
        assert_eq!(
            parts("a\\\\\nb"),
            vec![lit("a"), WordPart::EscapedLiteral("\\".to_string())]
        );
    }

    #[test]
    fn command_sub_end_found_by_grammar() {
        for src in [
            "$(echo \\))",
            "$(echo \"$(echo \"a)\")\")",
            "$(case a in a) echo A;; esac)",
            "$(cat <<E\nit's\nE\n)",
            "$(\n# it's\necho c\n)",
        ] {
            let p = parts(src);
            assert!(
                matches!(p.as_slice(), [WordPart::CommandSub(_)]),
                "{src:?} -> {p:?}"
            );
        }
        assert_eq!(Lexer::find_command_sub_end("echo \\)) x"), Some(7));
        assert_eq!(Lexer::find_command_sub_end("echo ("), None);
    }

    #[test]
    fn tilde_prefix_with_quote_or_expansion_is_literal() {
        assert_eq!(
            parts("~'root'"),
            vec![lit("~"), WordPart::SingleQuoted("root".to_string())]
        );
        assert_eq!(
            parts("~$u"),
            vec![
                lit("~"),
                WordPart::Parameter(ParamExpr::Simple("u".to_string()))
            ]
        );
        assert_eq!(
            parts("${x:-~}"),
            vec![WordPart::Parameter(ParamExpr::Default {
                name: "x".to_string(),
                word: Some(word(vec![WordPart::Tilde(None)])),
                null_check: true,
            })]
        );
    }

    #[test]
    fn dquoted_conditional_word_uses_dquote_rules() {
        assert_eq!(
            parts("\"${x-'a'}\""),
            vec![WordPart::DoubleQuoted(vec![WordPart::Parameter(
                ParamExpr::Default {
                    name: "x".to_string(),
                    word: Some(word(vec![lit("'a'")])),
                    null_check: false,
                }
            )])]
        );
    }

    /// `(line, column)` of every token of `input`.
    fn token_spans(input: &str) -> Vec<(usize, usize)> {
        let mut lexer = Lexer::new(input);
        let mut out = Vec::new();
        loop {
            let t = lexer.next_token().unwrap();
            if t.token == crate::lexer::token::Token::Eof {
                break;
            }
            out.push((t.span.line, t.span.column));
        }
        out
    }

    // Boundaries of the bulk copy of plain byte runs (`push_plain_run`).
    #[test]
    fn plain_runs_end_at_every_special_byte() {
        assert_eq!(parts("ab\\\ncd~e"), vec![lit("abcd~e")]);
        assert_eq!(parts("a}b{c=d#e!f*?[g]"), vec![lit("a}b{c=d#e!f*?[g]")]);
        assert_eq!(
            parts("\"a}b c|d;\tx\""),
            vec![WordPart::DoubleQuoted(vec![lit("a}b c|d;\tx")])]
        );
        assert_eq!(
            parts("\"ab\\\ncd\""),
            vec![WordPart::DoubleQuoted(vec![lit("abcd")])]
        );
        assert_eq!(
            parts("'a\\b c\\\nd'"),
            vec![WordPart::SingleQuoted("a\\b c\\\nd".to_string())]
        );
        assert_eq!(parts("aé日b c"), vec![lit("aé日b")]);
        assert_eq!(
            parts("${x:-a b|c}"),
            vec![WordPart::Parameter(ParamExpr::Default {
                name: "x".to_string(),
                word: Some(word(vec![lit("a b|c")])),
                null_check: true,
            })]
        );
        assert_eq!(
            parts("ab$x\"c\"d"),
            vec![
                lit("ab"),
                WordPart::Parameter(ParamExpr::Simple("x".to_string())),
                WordPart::DoubleQuoted(vec![lit("c")]),
                lit("d"),
            ]
        );
    }

    #[test]
    fn plain_runs_keep_line_and_column() {
        assert_eq!(
            token_spans("hello   world \"a b\" 'c d' e"),
            vec![(1, 1), (1, 9), (1, 15), (1, 21), (1, 27)]
        );
        assert_eq!(token_spans("'a\nbc' d"), vec![(1, 1), (2, 5)]);
        assert_eq!(token_spans("日本 x"), vec![(1, 1), (1, 4)]);
        assert_eq!(token_spans("ab\\\ncd e"), vec![(1, 1), (2, 4)]);
        let e = parse_err("abc${x");
        let loc = e.location.expect("param error carries a location");
        assert_eq!((loc.line, loc.column), (1, 5));
    }

    #[test]
    fn deep_nesting_is_a_syntax_error() {
        // Run on a big stack: the limit is sized for the 8 MiB main
        // thread, larger than libtest's default thread stack.
        let handle = std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(|| {
                let deep = format!("{}z{}", "${x:-".repeat(5000), "}".repeat(5000));
                let e = parse_err(&deep);
                assert!(e.message.contains("nesting too deep"), "{e:?}");
                let ok = format!("{}z{}", "${x:-".repeat(100), "}".repeat(100));
                assert_eq!(parts(&ok).len(), 1);
            })
            .unwrap();
        handle.join().unwrap();
    }
}
