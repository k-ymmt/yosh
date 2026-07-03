use super::Parser;
use super::ast::{self, Assignment, SimpleCommand, Word};
use super::word::{is_valid_name, split_tildes_in_literal};
use crate::error::{self, ParseErrorKind, ShellError};
use crate::lexer::token::Token;

impl Parser {
    pub(super) fn parse_simple_command(&mut self) -> error::Result<SimpleCommand> {
        let line = self.current.span.line;
        let mut assignments = Vec::new();
        let mut words = Vec::new();
        let mut redirects = Vec::new();

        loop {
            // Try redirect first
            if let Some(redirect) = self.try_parse_redirect()? {
                redirects.push(redirect);
                continue;
            }

            // Check for word token
            if let Token::Word(_) = &self.current.token {
                // Move the word out of `current`, leaving a placeholder that is
                // never observed: every branch below calls `advance()?` right
                // after, which overwrites `current` with the next token before
                // any error path could return control to a caller that reads it.
                let Token::Word(word) = std::mem::replace(&mut self.current.token, Token::Eof)
                else {
                    unreachable!("matched Token::Word above")
                };

                // Only try assignments before any command words have been seen
                if words.is_empty()
                    && let Some(assignment) = Self::try_parse_assignment(&word)
                {
                    self.advance()?;
                    assignments.push(assignment);
                    continue;
                }

                // It's a regular word
                self.advance()?;
                words.push(word);
                continue;
            }

            // If we hit a newline and have pending heredocs, process them now
            if self.current.token == Token::Newline && self.lexer.has_pending_heredocs() {
                self.lexer.process_pending_heredocs()?;
            }

            // End of simple command
            break;
        }

        // POSIX §2.9.1: a simple_command derives from at least one of
        // cmd_prefix (assignment/redirect), cmd_name (word), or cmd_word
        // (word). A zero-progress empty return on an operator-like token
        // (DSemi, Pipe in unexpected positions, etc.) lets callers such
        // as parse_compound_list loop forever.
        //
        // Newline and Eof are NOT errors here — they represent lexer
        // boundaries that callers handle via skip_newlines / is_at_end,
        // and an empty return at such a boundary (e.g. a source file
        // line that is only a comment, which the lexer reduces to a
        // bare Newline token) is a legitimate no-op.
        if assignments.is_empty()
            && words.is_empty()
            && redirects.is_empty()
            && !matches!(self.current.token, Token::Newline | Token::Eof)
        {
            let span = self.current_span();
            return Err(ShellError::parse(
                ParseErrorKind::UnexpectedToken,
                span.line,
                span.column,
                "syntax error: unexpected token at start of command",
            ));
        }

        Ok(SimpleCommand {
            assignments,
            words,
            redirects,
            line,
        })
    }

    /// Try to parse an assignment from a word.
    /// Returns Some(Assignment) if the word contains an `=` and a valid name prefix.
    pub fn try_parse_assignment(word: &Word) -> Option<Assignment> {
        use ast::WordPart;

        // We need the first part to be a Literal containing '='
        // (or the word might be entirely a literal like "FOO=bar")
        if word.parts.is_empty() {
            return None;
        }

        // Collect the full literal text from the first part (if it's a Literal)
        let first_part_text: &str = match &word.parts[0] {
            WordPart::Literal(s) => s,
            _ => return None,
        };

        // Find '=' in the literal
        let eq_pos = first_part_text.find('=')?;

        let name = &first_part_text[..eq_pos];
        if !is_valid_name(name) {
            return None;
        }

        // Value: rest after '=' in the first part + remaining parts
        let after_eq = &first_part_text[eq_pos + 1..];
        let remaining_parts = &word.parts[1..];

        if after_eq.is_empty() && remaining_parts.is_empty() {
            // FOO= with nothing after
            return Some(Assignment {
                name: name.to_string(),
                value: None,
            });
        }

        // Build value word with boundary-aware tilde splitting across all parts.
        //
        // The segment boundary starts true immediately after `=` (we just
        // consumed it). Whenever a Literal part is scanned,
        // split_tildes_in_literal returns whether the last character was an
        // unquoted `:`, which we propagate as the incoming boundary for the
        // next part.
        //
        // A non-Literal part (Parameter, CommandSub, quoted content, Tilde,
        // EscapedLiteral) resets the boundary to false: such parts cannot
        // expose an unquoted trailing `:` to the next segment, and
        // EscapedLiteral specifically carries an explicit "this character
        // was escaped" signal from the lexer — tilde-prefix recognition must
        // not fire immediately after it.
        let mut value_parts = Vec::new();
        let mut at_boundary = true;
        if !after_eq.is_empty() {
            let (parts, ends_colon) = split_tildes_in_literal(after_eq, at_boundary);
            value_parts.extend(parts);
            at_boundary = ends_colon;
        }
        for part in remaining_parts {
            match part {
                WordPart::Literal(s) => {
                    let (parts, ends_colon) = split_tildes_in_literal(s, at_boundary);
                    value_parts.extend(parts);
                    at_boundary = ends_colon;
                }
                other => {
                    // Parameter, CommandSub, SingleQuoted, DoubleQuoted,
                    // DollarSingleQuoted, ArithSub, Tilde, and EscapedLiteral
                    // all hit this arm: emit as-is and close the boundary.
                    value_parts.push(other.clone());
                    at_boundary = false;
                }
            }
        }

        Some(Assignment {
            name: name.to_string(),
            value: Some(Word { parts: value_parts }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::ast::{Command, ParamExpr, WordPart};
    use super::super::tests::parse_first_simple;
    use super::*;

    fn lit(s: &str) -> WordPart {
        WordPart::Literal(s.to_string())
    }

    #[test]
    fn test_simple_command() {
        let sc = parse_first_simple("echo hello world");
        assert_eq!(sc.words.len(), 3);
        assert_eq!(sc.words[0].as_literal(), Some("echo"));
        assert_eq!(sc.words[1].as_literal(), Some("hello"));
        assert_eq!(sc.words[2].as_literal(), Some("world"));
        assert!(sc.assignments.is_empty());
        assert!(sc.redirects.is_empty());
    }

    #[test]
    fn test_assignment_only() {
        let sc = parse_first_simple("FOO=bar");
        assert!(sc.words.is_empty());
        assert_eq!(sc.assignments.len(), 1);
        assert_eq!(sc.assignments[0].name, "FOO");
        assert_eq!(
            sc.assignments[0].value.as_ref().unwrap().as_literal(),
            Some("bar")
        );
    }

    #[test]
    fn test_assignment_with_command() {
        let sc = parse_first_simple("FOO=bar echo hello");
        assert_eq!(sc.assignments.len(), 1);
        assert_eq!(sc.words.len(), 2);
    }

    #[test]
    fn test_assignment_empty_value() {
        let sc = parse_first_simple("FOO=");
        assert_eq!(sc.assignments.len(), 1);
        assert_eq!(sc.assignments[0].name, "FOO");
        assert_eq!(sc.assignments[0].value, None);
    }

    // ── try_parse_assignment integration ────────────────────────

    // AST shape (verified against src/parser/ast.rs):
    //   Program { commands: Vec<CompleteCommand> }
    //   CompleteCommand { items: Vec<(AndOrList, Option<SeparatorOp>)> }
    //   AndOrList { first: Pipeline, rest: ... }
    //   Pipeline { commands: Vec<Command>, negated: bool }
    //   Command::Simple(SimpleCommand)
    //   SimpleCommand { assignments: Vec<Assignment>, words, redirects }
    fn parse_first_assignment(source: &str) -> Option<(String, Vec<WordPart>)> {
        let mut parser = Parser::new(source);
        let program = parser.parse_program().ok()?;
        let cc = program.commands.into_iter().next()?;
        let (aol, _) = cc.items.into_iter().next()?;
        let cmd = aol.first.commands.into_iter().next()?;
        let Command::Simple(sc) = cmd else {
            return None;
        };
        let a = sc.assignments.into_iter().next()?;
        let parts = a.value.map(|w| w.parts).unwrap_or_default();
        Some((a.name, parts))
    }

    #[test]
    fn assignment_rhs_unquoted_tilde_becomes_tilde_part() {
        let (name, parts) = parse_first_assignment("x=~/bin\n").unwrap();
        assert_eq!(name, "x");
        assert_eq!(parts, vec![WordPart::Tilde(None), lit("/bin")]);
    }

    #[test]
    fn assignment_rhs_multi_colon_tildes() {
        let (name, parts) = parse_first_assignment("PATH=~/a:~/b\n").unwrap();
        assert_eq!(name, "PATH");
        assert_eq!(
            parts,
            vec![
                WordPart::Tilde(None),
                lit("/a:"),
                WordPart::Tilde(None),
                lit("/b"),
            ]
        );
    }

    #[test]
    fn assignment_rhs_backslash_tilde_stays_literal() {
        let (_, parts) = parse_first_assignment("x=\\~/bin\n").unwrap();
        let has_tilde = parts.iter().any(|p| matches!(p, WordPart::Tilde(_)));
        assert!(!has_tilde, "parts = {:?}", parts);
    }

    #[test]
    fn assignment_rhs_single_quoted_tilde_stays_quoted() {
        let (_, parts) = parse_first_assignment("x='~'/bin\n").unwrap();
        let has_tilde = parts.iter().any(|p| matches!(p, WordPart::Tilde(_)));
        assert!(!has_tilde, "parts = {:?}", parts);
    }

    #[test]
    fn assignment_rhs_param_then_tilde_expands_after_colon() {
        // POSIX §2.6.1: a tilde-prefix is recognized after `=` and after any
        // unquoted `:` in an assignment value. The colon inside a trailing
        // Literal that follows a Parameter expansion still counts as a
        // segment boundary, so the tilde expands.
        let (_, parts) = parse_first_assignment("x=$var:~/bin\n").unwrap();
        let has_tilde = parts.iter().any(|p| matches!(p, WordPart::Tilde(_)));
        assert!(has_tilde, "parts = {:?}", parts);
    }

    #[test]
    fn assignment_rhs_param_then_colon_tilde_expands() {
        let (name, parts) = parse_first_assignment("x=$var:~/bin\n").unwrap();
        assert_eq!(name, "x");
        assert_eq!(
            parts,
            vec![
                WordPart::Parameter(ParamExpr::Simple("var".to_string())),
                lit(":"),
                WordPart::Tilde(None),
                lit("/bin"),
            ]
        );
    }

    #[test]
    fn assignment_rhs_param_then_tilde_no_colon_stays_literal() {
        let (name, parts) = parse_first_assignment("x=$var~/bin\n").unwrap();
        assert_eq!(name, "x");
        assert_eq!(
            parts,
            vec![
                WordPart::Parameter(ParamExpr::Simple("var".to_string())),
                lit("~/bin"),
            ]
        );
    }

    #[test]
    fn assignment_rhs_backslash_tilde_after_colon_stays_literal() {
        // `x=foo:\~/bin` — the `\~` escape prevents tilde expansion. The
        // lexer emits EscapedLiteral("~"), which the walker treats as a
        // non-Literal segment-boundary closer, preventing tilde expansion.
        let (_, parts) = parse_first_assignment("x=foo:\\~/bin\n").unwrap();
        let has_tilde = parts.iter().any(|p| matches!(p, WordPart::Tilde(_)));
        assert!(!has_tilde, "parts = {:?}", parts);
    }

    #[test]
    fn assignment_rhs_param_then_escaped_tilde_stays_literal() {
        // `x=$var:\~/bin` — the `\~` escape after the `:` prevents tilde
        // expansion at the colon boundary. The lexer emits
        // [Literal("x="), Parameter(var), Literal(":"), EscapedLiteral("~"), Literal("/bin")]
        // (or similar). The walker treats EscapedLiteral as a non-Literal
        // segment-boundary closer, so the following Literal does not re-open
        // tilde recognition.
        let (name, parts) = parse_first_assignment("x=$var:\\~/bin\n").unwrap();
        assert_eq!(name, "x");
        assert_eq!(
            parts,
            vec![
                WordPart::Parameter(ParamExpr::Simple("var".to_string())),
                lit(":"),
                WordPart::EscapedLiteral("~".to_string()),
                lit("/bin"),
            ]
        );
    }

    #[test]
    fn assignment_rhs_line_continuation_tilde_expands() {
        // POSIX §2.2.1: `\<newline>` is removed before tokenization, so
        // `x=foo:\<newline>~/bin` is semantically identical to `x=foo:~/bin`
        // and the tilde MUST expand at the ':' boundary.
        let (_, parts) = parse_first_assignment("x=foo:\\\n~/bin\n").unwrap();
        let has_tilde = parts.iter().any(|p| matches!(p, WordPart::Tilde(_)));
        assert!(has_tilde, "parts = {:?}", parts);
    }

    #[test]
    fn parse_simple_command_captures_line() {
        let cmd = parse_first_simple("echo hi\n");
        assert_eq!(cmd.line, 1);
    }

    #[test]
    fn parse_simple_command_on_third_line() {
        let cmd = parse_first_simple("\n\necho hi\n");
        assert_eq!(cmd.line, 3);
    }

    #[test]
    fn assignment_prefix_before_if_reserved_word_attaches_to_compound() {
        use super::super::ast::{Command, CompoundCommandKind};
        let mut parser = Parser::new("x=1 if true; then echo y; fi\n");
        let prog = parser.parse_program().unwrap();
        let cc = &prog.commands[0];
        let (aol, _) = &cc.items[0];
        let cmd = &aol.first.commands[0];
        let Command::Compound(comp, _redirs) = cmd else {
            panic!("expected Compound, got {:?}", cmd);
        };
        assert!(matches!(comp.kind, CompoundCommandKind::If { .. }));
        assert_eq!(comp.assignments.len(), 1);
        assert_eq!(comp.assignments[0].name, "x");
        assert_eq!(
            comp.assignments[0].value.as_ref().unwrap().as_literal(),
            Some("1")
        );
    }

    #[test]
    fn assignment_prefix_before_while_attaches_to_compound() {
        use super::super::ast::{Command, CompoundCommandKind};
        let mut parser = Parser::new("a=hi while false; do :; done\n");
        let prog = parser.parse_program().unwrap();
        let Command::Compound(comp, _) = &prog.commands[0].items[0].0.first.commands[0] else {
            panic!()
        };
        assert!(matches!(comp.kind, CompoundCommandKind::While { .. }));
        assert_eq!(comp.assignments.len(), 1);
        assert_eq!(comp.assignments[0].name, "a");
    }

    #[test]
    fn no_assignment_prefix_does_not_create_phantom_assignments() {
        use super::super::ast::Command;
        let mut parser = Parser::new("if true; then echo y; fi\n");
        let prog = parser.parse_program().unwrap();
        let Command::Compound(comp, _) = &prog.commands[0].items[0].0.first.commands[0] else {
            panic!()
        };
        assert!(comp.assignments.is_empty());
    }

    #[test]
    fn assignment_then_simple_command_still_lands_in_simple() {
        use super::super::ast::Command;
        let mut parser = Parser::new("x=1 echo y\n");
        let prog = parser.parse_program().unwrap();
        let Command::Simple(sc) = &prog.commands[0].items[0].0.first.commands[0] else {
            panic!("expected Simple, got compound")
        };
        assert_eq!(sc.assignments.len(), 1);
        assert_eq!(sc.assignments[0].name, "x");
        assert_eq!(sc.words.len(), 2);
        assert_eq!(sc.words[0].as_literal(), Some("echo"));
    }
}
