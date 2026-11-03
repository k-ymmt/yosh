//! Command substitution: `$(...)` (parsed with the real grammar on a
//! sub-lexer) and backquoted `` `...` `` bodies.

use super::super::Lexer;
use crate::error::{self, ParseErrorKind, ShellError};
use crate::parser::ast::WordPart;

impl Lexer {
    /// Handles `$(...)` command substitution.
    pub(super) fn read_command_sub_dollar(&mut self) -> error::Result<WordPart> {
        let (program, _) = self.parse_command_sub()?;
        Ok(WordPart::CommandSub(std::rc::Rc::new(program)))
    }

    /// Parse the body of `$(...)` with the real grammar, as dash and ksh
    /// do: the closing `)` is the first `)` the shell grammar does not
    /// consume itself, so escapes (`\)`), quotes, nested substitutions,
    /// comments (`# it's`), here-document bodies and `case` patterns
    /// (`a) ...;;`) are all handled exactly as at top level.
    ///
    /// Called with the current byte at the opening `(`; on return the
    /// closing `)` has been consumed. Also returns the byte range of the
    /// body text (between the parentheses) for callers that need it.
    pub(crate) fn parse_command_sub(
        &mut self,
    ) -> error::Result<(crate::parser::ast::Program, std::ops::Range<usize>)> {
        let span = self.current_span();
        self.advance(); // consume '('
        self.subparse_count += 1;
        let body_start = self.pos;
        let sub = self.sub_lexer();
        let (program, sub) = match crate::parser::Parser::parse_command_sub_body(sub, span) {
            Ok(parsed) => parsed,
            // A here-document delimiter only undergoes quote removal
            // (POSIX §2.7.4): its `$(...)` is never run, so a body that is
            // not a valid program (`cat <<"$(if)"`) must not be a syntax
            // error (dash, bash). The grammar still finds the end of a
            // valid body (`case` patterns); otherwise match parentheses.
            Err(_) if self.alias_state.reading_heredoc_delimiter() => {
                let body_end = self.skip_unparsed_command_sub(span)?;
                return Ok((
                    crate::parser::ast::Program {
                        commands: Vec::new(),
                    },
                    body_start..body_end,
                ));
            }
            Err(e) => return Err(e),
        };
        // The sub-lexer stops right after the `)` (read raw, see
        // `next_token_raw`).
        let body_end = sub.pos - 1;
        self.pos = sub.pos;
        self.line = sub.line;
        self.column = sub.column;
        self.skip_line_continuations();
        Ok((program, body_start..body_end))
    }

    /// Skip a `$(...)` body that is not parsed as a program (current byte
    /// just after the opening `(`), up to and including the matching `)`:
    /// parentheses are counted outside single quotes, double quotes and
    /// backslash escapes. Returns the position of the closing `)`.
    fn skip_unparsed_command_sub(
        &mut self,
        span: crate::lexer::token::Span,
    ) -> error::Result<usize> {
        let unterminated = || {
            ShellError::parse(
                ParseErrorKind::UnterminatedCommandSub,
                span.line,
                span.column,
                "unterminated command substitution",
            )
        };
        let mut depth = 1usize;
        let mut quote: Option<u8> = None;
        loop {
            if self.at_end() {
                return Err(unterminated());
            }
            let ch = self.current_byte();
            match (quote, ch) {
                (Some(b'\''), b'\'') | (Some(b'"'), b'"') => quote = None,
                (Some(b'\''), _) => {}
                (_, b'\\') => {
                    self.advance_raw();
                    if self.at_end() {
                        return Err(unterminated());
                    }
                }
                (Some(_), _) => {}
                (None, b'\'' | b'"') => quote = Some(ch),
                (None, b'(') => depth += 1,
                (None, b')') => {
                    depth -= 1;
                    if depth == 0 {
                        let close = self.pos;
                        self.advance();
                        return Ok(close);
                    }
                }
                _ => {}
            }
            if self.current_byte() < 0x80 {
                self.advance_raw();
            } else {
                self.advance_char()?;
            }
        }
    }

    /// Byte-offset of the `)` that closes a `$(` whose body starts at the
    /// beginning of `body`, found by parsing it with the real grammar.
    /// `None` if the body does not parse or is unterminated.
    pub fn find_command_sub_end(body: &str) -> Option<usize> {
        let lexer = Lexer::new(body);
        let span = lexer.current_span();
        let (_, sub) = crate::parser::Parser::parse_command_sub_body(lexer, span).ok()?;
        Some(sub.pos - 1)
    }

    /// Handles backtick command substitution `` `...` ``.
    ///
    /// `in_double_quote`: the backquotes are inside double quotes, where
    /// `\"` also loses its backslash (POSIX §2.6.3; dash/bash/ksh:
    /// ``"`echo \"hi\"`"`` prints `hi`).
    pub(super) fn read_backtick(&mut self, in_double_quote: bool) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // consume opening '`'
        let mut content = String::new();

        loop {
            if self.at_end() {
                return Err(ShellError::parse(
                    ParseErrorKind::UnterminatedBacktick,
                    span.line,
                    span.column,
                    "unterminated backtick command substitution",
                ));
            }
            let ch = self.current_byte();
            match ch {
                b'`' => {
                    self.advance(); // consume closing '`'
                    break;
                }
                b'\\' => {
                    self.advance_raw(); // consume '\' — the escaped byte is raw
                    if self.at_end() {
                        content.push('\\');
                        break;
                    }
                    let esc = self.current_byte();
                    match esc {
                        b'$' | b'`' | b'\\' => {
                            content.push(esc as char);
                            self.advance();
                        }
                        b'"' if in_double_quote => {
                            content.push('"');
                            self.advance();
                        }
                        _ => {
                            // keep backslash literally
                            content.push('\\');
                            self.push_current_char(&mut content)?;
                        }
                    }
                }
                _ => {
                    self.push_current_char(&mut content)?;
                }
            }
        }

        self.subparse_count += 1;
        let lexer = self.nested_lexer(&content);
        let program = match crate::parser::Parser::parse_nested_program(lexer) {
            Ok(program) => program,
            // Never run in a here-document delimiter: see
            // `parse_command_sub`.
            Err(_) if self.alias_state.reading_heredoc_delimiter() => crate::parser::ast::Program {
                commands: Vec::new(),
            },
            Err(e) => return Err(e),
        };
        Ok(WordPart::CommandSub(std::rc::Rc::new(program)))
    }
}
