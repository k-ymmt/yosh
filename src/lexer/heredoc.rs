use super::{Lexer, PendingHereDoc};
use crate::error::{self, ParseErrorKind, ShellError};
use crate::parser::ast::WordPart;

impl Lexer {
    pub fn register_heredoc(&mut self, delimiter: String, strip_tabs: bool, quoted: bool) -> u64 {
        let id = self.next_heredoc_id;
        self.next_heredoc_id += 1;
        self.pending_heredocs.push(PendingHereDoc {
            id,
            delimiter,
            strip_tabs,
            quoted,
        });
        id
    }

    pub fn take_heredoc_body(&mut self, id: u64) -> Option<Vec<WordPart>> {
        let idx = self.heredoc_bodies.iter().position(|(bid, _)| *bid == id)?;
        Some(self.heredoc_bodies.remove(idx).1)
    }

    pub fn process_pending_heredocs(&mut self) -> error::Result<()> {
        let pending = std::mem::take(&mut self.pending_heredocs);
        for hd in pending {
            let body = self.read_heredoc_body(&hd)?;
            self.heredoc_bodies.push((hd.id, body));
        }
        Ok(())
    }

    pub fn has_pending_heredocs(&self) -> bool {
        !self.pending_heredocs.is_empty()
    }

    pub fn has_heredoc_bodies(&self) -> bool {
        !self.heredoc_bodies.is_empty()
    }

    pub(crate) fn read_heredoc_body(
        &mut self,
        hd: &PendingHereDoc,
    ) -> error::Result<Vec<WordPart>> {
        // The body is read raw: backslash-newline is handled per line
        // below (and kept verbatim for a quoted delimiter).
        self.without_join(|lx| lx.read_heredoc_body_raw(hd))
    }

    fn read_heredoc_body_raw(&mut self, hd: &PendingHereDoc) -> error::Result<Vec<WordPart>> {
        let mut body = String::new();
        loop {
            if self.at_end() {
                if self.eof_ends_heredoc {
                    break;
                }
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
            // Read a logical line. With an unquoted delimiter, a physical
            // line ending in an unescaped `\` is joined with the next one
            // (the backslash-newline is removed) BEFORE the delimiter
            // check, so `line \<nl>EOF` is body text, not the delimiter
            // (POSIX §2.7.4; dash/bash agree). `<<-` strips the leading
            // tabs of the logical line: those of a continuation line too
            // while nothing precedes them (`\<nl><tab>EOF` is the
            // delimiter), but not tabs after earlier text (`a\<nl><tab>b`
            // keeps the tab).
            let mut line = String::new();
            let mut had_newline;
            loop {
                let mut phys = String::new();
                while !self.at_end() && self.current_byte() != b'\n' {
                    self.push_current_char(&mut phys)?;
                }
                had_newline = !self.at_end();
                if had_newline {
                    self.advance(); // consume newline
                }
                if hd.strip_tabs && line.is_empty() {
                    line.push_str(phys.trim_start_matches('\t'));
                } else {
                    line.push_str(&phys);
                }
                if !hd.quoted && had_newline && ends_with_unescaped_backslash(&line) {
                    line.pop(); // drop the `\` of the continuation
                    continue;
                }
                break;
            }

            // Check if this is the delimiter line
            if line == hd.delimiter {
                break;
            }

            body.push_str(&line);
            // A last line cut off by end of input (`eof_ends_heredoc`)
            // keeps its missing newline missing, as in dash.
            if had_newline {
                body.push('\n');
            }
        }
        Ok(vec![WordPart::Literal(body)])
    }
}

/// True when `s` ends in an odd-length run of backslashes, i.e. its last
/// `\` is not itself escaped.
fn ends_with_unescaped_backslash(s: &str) -> bool {
    s.bytes().rev().take_while(|&b| b == b'\\').count() % 2 == 1
}
