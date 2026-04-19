use super::{Lexer, PendingHereDoc};
use crate::error::{self, ParseErrorKind, ShellError};
use crate::parser::ast::WordPart;

impl Lexer {
    pub fn register_heredoc(&mut self, delimiter: String, quoted: bool, strip_tabs: bool) {
        self.pending_heredocs.push(PendingHereDoc {
            delimiter,
            quoted,
            strip_tabs,
        });
    }

    pub fn take_heredoc_body(&mut self) -> Option<Vec<WordPart>> {
        if self.heredoc_bodies.is_empty() {
            None
        } else {
            Some(self.heredoc_bodies.remove(0))
        }
    }

    pub fn process_pending_heredocs(&mut self) -> error::Result<()> {
        let pending: Vec<PendingHereDoc> = self.pending_heredocs.drain(..).collect();
        for hd in pending {
            let body = self.read_heredoc_body(&hd)?;
            self.heredoc_bodies.push(body);
        }
        Ok(())
    }

    pub fn has_pending_heredocs(&self) -> bool {
        !self.pending_heredocs.is_empty()
    }

    pub(crate) fn read_heredoc_body(
        &mut self,
        hd: &PendingHereDoc,
    ) -> error::Result<Vec<WordPart>> {
        let mut body = String::new();
        loop {
            if self.at_end() {
                return Err(ShellError::parse(
                    ParseErrorKind::InvalidHereDoc,
                    self.line,
                    self.column,
                    format!(
                        "here-document delimited by '{}' was not closed",
                        hd.delimiter
                    ),
                ));
            }
            // Read a line
            let mut line = String::new();
            while !self.at_end() && self.current_byte() != b'\n' {
                line.push(self.current_byte() as char);
                self.advance();
            }
            if !self.at_end() {
                self.advance(); // consume newline
            }

            // Strip leading tabs if <<-
            let check_line = if hd.strip_tabs {
                line.trim_start_matches('\t').to_string()
            } else {
                line.clone()
            };

            // Check if this is the delimiter line
            if check_line == hd.delimiter {
                break;
            }

            // Add line to body (with tab stripping if applicable)
            if hd.strip_tabs {
                body.push_str(line.trim_start_matches('\t'));
            } else {
                body.push_str(&line);
            }
            body.push('\n');
        }
        Ok(vec![WordPart::Literal(body)])
    }
}
