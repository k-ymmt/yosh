//! Arithmetic expansion `$((...))`: the expression text is kept
//! verbatim (nested substitutions included) for the expander.

use super::super::Lexer;
use super::super::token::Span;
use crate::error::{self, ParseErrorKind, ShellError};
use crate::parser::ast::WordPart;

impl Lexer {
    /// Handles `$((expr))` arithmetic expansion.
    pub(super) fn read_arith_expansion(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // consume first '('
        self.advance(); // consume second '('

        let mut expr = String::new();
        let mut depth: usize = 0;

        loop {
            if self.at_end() {
                return Err(ShellError::parse(
                    ParseErrorKind::UnterminatedArithSub,
                    span.line,
                    span.column,
                    "unterminated arithmetic expansion",
                ));
            }
            let ch = self.current_byte();
            match ch {
                b'$' if self.peek_byte() == b'(' && self.peek_byte2() != b'(' => {
                    // Nested $(cmd) — find its end with the real grammar
                    // and keep its body text verbatim.
                    self.advance(); // consume '$', now at '('
                    let (_, body) = self.parse_command_sub()?;
                    expr.push_str("$(");
                    expr.push_str(&String::from_utf8_lossy(&self.input[body]));
                    expr.push(')');
                }
                b'$' if self.peek_byte() == b'(' => {
                    // Nested $((...)) — balanced, quote-aware scan.
                    let sub_span = self.current_span();
                    expr.push('$');
                    self.advance(); // consume '$', now at '('
                    let content = self.read_balanced_parens(sub_span)?;
                    expr.push('(');
                    expr.push_str(&content);
                    expr.push(')');
                }
                b'`' => {
                    // Backtick command substitution — skip to matching `
                    expr.push('`');
                    self.advance();
                    while !self.at_end() {
                        let bch = self.current_byte();
                        if bch == b'\\' {
                            expr.push('\\');
                            self.advance_raw();
                            if !self.at_end() {
                                self.push_current_char(&mut expr)?;
                            }
                        } else {
                            self.push_current_char(&mut expr)?;
                            if bch == b'`' {
                                break;
                            }
                        }
                    }
                }
                b'(' => {
                    depth += 1;
                    expr.push('(');
                    self.advance();
                }
                b')' => {
                    if depth == 0 {
                        // check for closing '))'
                        self.advance(); // consume first ')'
                        if self.at_end() || self.current_byte() != b')' {
                            return Err(ShellError::parse(
                                ParseErrorKind::UnterminatedArithSub,
                                span.line,
                                span.column,
                                "expected '))'",
                            ));
                        }
                        self.advance(); // consume second ')'
                        break;
                    } else {
                        depth -= 1;
                        expr.push(')');
                        self.advance();
                    }
                }
                _ => {
                    self.push_current_char(&mut expr)?;
                }
            }
        }

        Ok(WordPart::ArithSub(expr.trim().to_string()))
    }

    /// Reads balanced parentheses content for a `$((...))` nested inside
    /// an arithmetic expansion (the parenthesized arithmetic text).
    /// Returns content between outer parens (not including them).
    fn read_balanced_parens(&mut self, span: Span) -> error::Result<String> {
        self.advance(); // consume opening '('
        let mut content = String::new();
        let mut depth: usize = 1;

        loop {
            if self.at_end() {
                return Err(ShellError::parse(
                    ParseErrorKind::UnterminatedCommandSub,
                    span.line,
                    span.column,
                    "unterminated command substitution",
                ));
            }
            let ch = self.current_byte();
            match ch {
                b'$' if self.peek_byte() == b'(' && self.peek_byte2() != b'(' => {
                    self.advance(); // consume '$', now at '('
                    let (_, body) = self.parse_command_sub()?;
                    content.push_str("$(");
                    content.push_str(&String::from_utf8_lossy(&self.input[body]));
                    content.push(')');
                }
                b'\\' => {
                    content.push('\\');
                    self.advance_raw();
                    if !self.at_end() {
                        self.push_current_char(&mut content)?;
                    }
                }
                b'(' => {
                    depth += 1;
                    content.push('(');
                    self.advance();
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        self.advance(); // consume closing ')'
                        break;
                    }
                    content.push(')');
                    self.advance();
                }
                b'\'' => {
                    // single quote: read until closing '
                    content.push('\'');
                    self.advance();
                    loop {
                        if self.at_end() {
                            return Err(ShellError::parse(
                                ParseErrorKind::UnterminatedCommandSub,
                                span.line,
                                span.column,
                                "unterminated single quote in command substitution",
                            ));
                        }
                        let qch = self.current_byte();
                        self.push_current_char(&mut content)?;
                        if qch == b'\'' {
                            break;
                        }
                    }
                }
                b'"' => {
                    // double quote: read until closing ", handling backslash escapes
                    content.push('"');
                    self.advance();
                    loop {
                        if self.at_end() {
                            return Err(ShellError::parse(
                                ParseErrorKind::UnterminatedCommandSub,
                                span.line,
                                span.column,
                                "unterminated double quote in command substitution",
                            ));
                        }
                        let qch = self.current_byte();
                        if qch == b'"' {
                            content.push('"');
                            self.advance();
                            break;
                        }
                        if qch == b'\\' {
                            content.push('\\');
                            self.advance_raw();
                            if !self.at_end() {
                                self.push_current_char(&mut content)?;
                            }
                        } else {
                            self.push_current_char(&mut content)?;
                        }
                    }
                }
                _ => {
                    self.push_current_char(&mut content)?;
                }
            }
        }

        Ok(content)
    }
}
