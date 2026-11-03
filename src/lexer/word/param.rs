//! `$` expansions: the `$` dispatcher, `$name` / special parameters,
//! and braced `${...}` parameter expansion with its operators.

use super::super::token::Span;
use super::super::{Lexer, is_name_char, is_name_start};
use crate::error::{self, ParseErrorKind, ShellError};
use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};

/// Build the `UnterminatedParamExpansion` parse error shared by the
/// `${...}` scanning paths, anchored at the expansion's opening span.
fn param_err(span: Span, message: impl Into<String>) -> ShellError {
    ShellError::parse(
        ParseErrorKind::UnterminatedParamExpansion,
        span.line,
        span.column,
        message,
    )
}

impl Lexer {
    /// Handles `$`. Dispatches to appropriate expansion method.
    /// `in_double_quote`: POSIX 2024 §2.2.4 — `$'...'` is only recognized
    /// outside double-quotes; inside `"..."` the `$` stays literal.
    pub(super) fn read_dollar(&mut self, in_double_quote: bool) -> error::Result<WordPart> {
        self.advance(); // consume '$'

        if self.at_end() {
            return Ok(WordPart::Literal("$".to_string()));
        }

        let ch = self.current_byte();
        if let Some(sp) = SpecialParam::from_byte(ch) {
            self.advance();
            return Ok(WordPart::Parameter(ParamExpr::Special(sp)));
        }
        match ch {
            b'\'' if in_double_quote => Ok(WordPart::Literal("$".to_string())),
            b'\'' => self.read_dollar_single_quote(),
            b'{' => self.read_param_expansion_braced(in_double_quote),
            b'(' => {
                // check for $(( arithmetic ))
                if self.peek_byte() == b'(' {
                    self.read_arith_expansion()
                } else {
                    self.read_command_sub_dollar()
                }
            }
            b'1'..=b'9' => {
                let digit = ch - b'0';
                self.advance();
                Ok(WordPart::Parameter(ParamExpr::Positional(digit as usize)))
            }
            c if is_name_start(c) => {
                let name = self.read_name();
                Ok(WordPart::Parameter(ParamExpr::Simple(name)))
            }
            _ => Ok(WordPart::Literal("$".to_string())),
        }
    }

    // ---- Identifier/parameter methods ----

    /// Reads [a-zA-Z_][a-zA-Z0-9_]* from current position.
    fn read_name(&mut self) -> String {
        let mut name = String::new();
        while !self.at_end() && is_name_char(self.current_byte()) {
            name.push(self.current_byte() as char);
            self.advance();
        }
        name
    }

    /// Converts a name string to ParamExpr.
    fn classify_param_name(&self, name: &str) -> ParamExpr {
        if let [b] = name.as_bytes()
            && let Some(sp) = SpecialParam::from_byte(*b)
        {
            return ParamExpr::Special(sp);
        }
        match name {
            s if s.chars().all(|c| c.is_ascii_digit()) => {
                // Leading zeros are decimal (`${01}` ≡ `$1`); all-zero
                // digit strings denote `$0`.
                let n: usize = s.parse().unwrap_or(0);
                if n == 0 {
                    ParamExpr::Special(SpecialParam::Zero)
                } else {
                    ParamExpr::Positional(n)
                }
            }
            s => ParamExpr::Simple(s.to_string()),
        }
    }

    /// Reads the parameter name inside braces (after `${`).
    fn read_param_name(&mut self, span: Span) -> error::Result<String> {
        if self.at_end() {
            return Err(param_err(span, "unterminated parameter expansion"));
        }
        let ch = self.current_byte();
        match ch {
            b'@' | b'*' | b'?' | b'-' | b'$' | b'!' | b'#' => {
                self.advance();
                Ok((ch as char).to_string())
            }
            // POSIX §2.5.1: a positional parameter is a decimal digit string;
            // leading zeros are allowed (`${01}` ≡ `$1`).
            b'0'..=b'9' => {
                let mut digits = String::new();
                while !self.at_end() && self.current_byte().is_ascii_digit() {
                    digits.push(self.current_byte() as char);
                    self.advance();
                }
                Ok(digits)
            }
            c if is_name_start(c) => Ok(self.read_name()),
            _ => Err(param_err(
                span,
                format!("invalid parameter name character: '{}'", ch as char),
            )),
        }
    }

    /// Reads word parts until `}`, then consumes `}`.
    fn read_word_in_brace(&mut self, span: Span) -> error::Result<Word> {
        let parts = self.read_word_parts(false, Some(b'}'))?;
        self.expect_byte(b'}', span)?;
        Ok(Word { parts })
    }

    /// Expects the given byte at current position, consuming it on success.
    fn expect_byte(&mut self, expected: u8, span: Span) -> error::Result<()> {
        if self.at_end() || self.current_byte() != expected {
            return Err(param_err(span, format!("expected '{}'", expected as char)));
        }
        self.advance();
        Ok(())
    }

    // ---- Parameter expansion methods ----

    /// Handles `${...}` braced parameter expansion.
    fn read_param_expansion_braced(&mut self, in_dq: bool) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // consume '{'

        // Check for ${#name} — length operator
        if !self.at_end() && self.current_byte() == b'#' {
            // peek ahead: if next char is '}' or an operator, this is ${#} (special Hash)
            // if next char starts a name or is a digit/special, it's ${#name}
            let next = self.peek_byte();
            match next {
                b'}' => {
                    // ${#} — special Hash param
                    self.advance(); // consume '#'
                    self.expect_byte(b'}', span)?;
                    return Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::Hash)));
                }
                c if is_name_start(c) || c.is_ascii_digit() => {
                    self.advance(); // consume '#'
                    let name = self.read_param_name(span)?;
                    self.expect_byte(b'}', span)?;
                    // ${#name} where name is a positional or special — still Length
                    // But ${#@}, ${#*} etc. in POSIX are technically invalid; we return Length anyway
                    return Ok(WordPart::Parameter(ParamExpr::Length(name)));
                }
                // ${#X} where X is a special parameter and the brace closes
                // immediately after: string length of $X (`${#?}` is the
                // length of `$?`, not `${#?word}` on `$#` — bash and dash
                // agree). With more content before `}` the '#' stays the
                // parameter (`${#-word}`, `${##pat}`, …).
                c if matches!(c, b'@' | b'*' | b'?' | b'-' | b'$' | b'!' | b'#')
                    && self.peek_byte2() == b'}' =>
                {
                    self.advance(); // consume '#'
                    let name = self.read_param_name(span)?;
                    self.expect_byte(b'}', span)?;
                    return Ok(WordPart::Parameter(ParamExpr::Length(name)));
                }
                _ => {
                    // ${#operator...} — treat '#' as the name (special Hash param), then operator
                    // Actually treat it as param name '#'
                    self.advance(); // consume '#'
                    let name = "#".to_string();
                    // fall through to operator handling below
                    return self.read_param_operator(span, name, in_dq);
                }
            }
        }

        let name = self.read_param_name(span)?;
        self.read_param_operator(span, name, in_dq)
    }

    /// After reading the param name inside `${`, read optional operator and closing `}`.
    fn read_param_operator(
        &mut self,
        span: Span,
        name: String,
        in_dq: bool,
    ) -> error::Result<WordPart> {
        if self.at_end() {
            return Err(param_err(span, "unterminated parameter expansion"));
        }

        let ch = self.current_byte();
        match ch {
            b'}' => {
                self.advance(); // consume '}'
                Ok(WordPart::Parameter(self.classify_param_name(&name)))
            }
            b'%' => {
                self.advance(); // consume '%'
                if !self.at_end() && self.current_byte() == b'%' {
                    self.advance(); // consume second '%'
                    let word = self.read_word_in_brace(span)?;
                    Ok(WordPart::Parameter(ParamExpr::StripLongSuffix(name, word)))
                } else {
                    let word = self.read_word_in_brace(span)?;
                    Ok(WordPart::Parameter(ParamExpr::StripShortSuffix(name, word)))
                }
            }
            b'#' => {
                self.advance(); // consume '#'
                if !self.at_end() && self.current_byte() == b'#' {
                    self.advance(); // consume second '#'
                    let word = self.read_word_in_brace(span)?;
                    Ok(WordPart::Parameter(ParamExpr::StripLongPrefix(name, word)))
                } else {
                    let word = self.read_word_in_brace(span)?;
                    Ok(WordPart::Parameter(ParamExpr::StripShortPrefix(name, word)))
                }
            }
            b':' => {
                self.advance(); // consume ':'
                self.read_conditional_param(span, name, true, in_dq)
            }
            b'-' | b'=' | b'?' | b'+' => self.read_conditional_param(span, name, false, in_dq),
            _ => Err(param_err(
                span,
                format!(
                    "unexpected character in parameter expansion: '{}'",
                    ch as char
                ),
            )),
        }
    }

    /// Reads the conditional operator (`-`, `=`, `?`, `+`) and its word argument.
    ///
    /// `in_dq`: the expansion itself is inside double quotes (or a
    /// here-document). The word is then scanned with double-quote rules —
    /// `'` is literal and `\` only escapes `$` `` ` `` `"` `\` — so
    /// `"${x-'a'}"` yields `'a'` (POSIX §2.6.2; dash/bash/ksh agree). The
    /// pattern operators (`#`, `%`) keep unquoted scanning: quotes in a
    /// pattern are significant even inside double quotes.
    fn read_conditional_param(
        &mut self,
        span: Span,
        name: String,
        null_check: bool,
        in_dq: bool,
    ) -> error::Result<WordPart> {
        if self.at_end() {
            return Err(param_err(span, "unterminated parameter expansion"));
        }
        let op = self.current_byte();
        self.advance(); // consume operator

        // Read optional word (may be empty)
        let parts = self.read_word_parts(in_dq, Some(b'}'))?;
        self.expect_byte(b'}', span)?;
        let word = if parts.is_empty() {
            None
        } else {
            Some(Word { parts })
        };

        let expr = match op {
            b'-' => ParamExpr::Default {
                name,
                word,
                null_check,
            },
            b'=' => ParamExpr::Assign {
                name,
                word,
                null_check,
            },
            b'?' => ParamExpr::Error {
                name,
                word,
                null_check,
            },
            b'+' => ParamExpr::Alt {
                name,
                word,
                null_check,
            },
            _ => {
                return Err(param_err(
                    span,
                    format!("unknown parameter operator: '{}'", op as char),
                ));
            }
        };
        Ok(WordPart::Parameter(expr))
    }
}
