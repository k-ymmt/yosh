//! Alias substitution (POSIX §2.3.1).
//!
//! Expansion is lazy: when a word in command-name position names an alias,
//! the alias value is tokenized (without further expansion) and pushed onto
//! the front of `alias_token_queue`. Queued tokens go through the same
//! command-position check as raw input when they are dequeued, so an alias
//! whose value starts with another alias, or contains several commands
//! (`alias a='if true; then e x; fi'`), is substituted exactly as if its
//! text had been typed.
//!
//! The recursion guard (`expanding_aliases`) holds an alias name only while
//! its replacement tokens are still being consumed: an `AliasFrame` records
//! the queue length below which the expansion is finished, and the name is
//! released before the next token is read. `alias e='echo '` therefore turns
//! `e e e` into `echo echo echo`, and `alias e=echo` keeps working for every
//! later command.
//!
//! Command-name position is tracked by a small grammar state machine in
//! `update_check_alias_after`: it is re-enabled after control operators,
//! after reserved words that introduce a command (`if`, `then`, `do`, `{`,
//! `!`, ...), after assignment words in a command prefix, after a redirect's
//! target word when the redirect began in command position, and after the
//! `)` that closes a case pattern list. Case subjects, case patterns, and
//! redirect targets are never alias-substituted.

use std::collections::VecDeque;

use super::Lexer;
use super::reserved::is_posix_reserved_word;
use super::token::{SpannedToken, Token};
use crate::error;
use crate::parser::ast::{Word, WordPart};

/// Alias-substitution bookkeeping beyond the token queue and the guard set.
#[derive(Clone, Debug, Default)]
pub(crate) struct AliasCtx {
    /// One frame per alias expansion whose replacement tokens have not all
    /// been consumed yet (innermost last).
    frames: Vec<AliasFrame>,
    /// Nesting stack of case clauses being lexed (innermost last).
    case_stack: Vec<CaseState>,
    /// `Some(saved)` while the next word is a redirect target: the target
    /// is never alias-substituted, and `check_alias` is restored to `saved`
    /// after it so a command name following a prefix redirect is checked.
    redirect_target: Option<bool>,
}

#[derive(Clone, Debug)]
struct AliasFrame {
    /// The expansion is finished once the queue is no longer than this.
    base_len: usize,
    name: String,
    /// The alias value ends in a blank: check the next word too.
    trailing_blank: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum CaseState {
    /// After `case`, expecting the subject word.
    Subject,
    /// After the subject, expecting `in`.
    In,
    /// Reading a pattern list (`a | b )`), or `esac`.
    Pattern,
    /// Inside a case item's compound list.
    Body,
}

/// True when `word` would be classified as an assignment in a command
/// prefix (`NAME=...` with an unquoted literal `NAME=` lead).
fn is_assignment_word(word: &Word) -> bool {
    let Some(WordPart::Literal(s)) = word.parts.first() else {
        return false;
    };
    let Some(eq) = s.find('=') else {
        return false;
    };
    crate::parser::word::is_valid_name(&s[..eq])
}

impl Lexer {
    pub fn next_token(&mut self) -> error::Result<SpannedToken> {
        loop {
            self.release_finished_aliases();

            let tok = match self.alias_token_queue.pop_front() {
                Some(tok) => tok,
                None => self.next_token_raw()?,
            };

            if self.check_alias
                && let Token::Word(ref word) = tok.token
                && let Some(word_text) = word.as_literal()
                && !is_posix_reserved_word(word_text)
                && !self.expanding_aliases.contains(word_text)
                && let Some(alias_value) = self.aliases.get(word_text).cloned()
            {
                let name = word_text.to_string();
                self.push_alias_expansion(name, &alias_value, tok.span)?;
                // The first replacement token is still in command-name
                // position: loop with `check_alias` unchanged.
                continue;
            }

            self.update_check_alias_after(&tok.token);
            return Ok(tok);
        }
    }

    /// Drop from the recursion guard every alias whose replacement tokens
    /// have all been consumed, applying its trailing-blank rule.
    fn release_finished_aliases(&mut self) {
        while let Some(frame) = self.alias_ctx.frames.last()
            && self.alias_token_queue.len() <= frame.base_len
        {
            let frame = self.alias_ctx.frames.pop().expect("checked above");
            self.expanding_aliases.remove(&frame.name);
            if frame.trailing_blank {
                self.check_alias = true;
            }
        }
    }

    /// Tokenize `value` and queue its tokens ahead of everything else.
    fn push_alias_expansion(
        &mut self,
        name: String,
        value: &str,
        span: super::token::Span,
    ) -> error::Result<()> {
        let trailing_blank = value.ends_with(' ') || value.ends_with('\t');

        // The sub-lexer has no alias table: substitution of the replacement
        // tokens happens lazily as they are dequeued (see `next_token`).
        let mut alias_lexer = Lexer::new(value);
        alias_lexer.line = span.line.max(1);
        let mut tokens = Vec::new();
        loop {
            let t = alias_lexer.next_token()?;
            if t.token == Token::Eof {
                break;
            }
            tokens.push(t);
        }

        let base_len = self.alias_token_queue.len();
        let mut queue: VecDeque<SpannedToken> = tokens.into();
        queue.append(&mut self.alias_token_queue);
        self.alias_token_queue = queue;
        self.expanding_aliases.insert(name.clone());
        self.alias_ctx.frames.push(AliasFrame {
            base_len,
            name,
            trailing_blank,
        });
        Ok(())
    }

    /// Decide whether the next word is in command-name position (and so
    /// subject to alias substitution) after producing `token`.
    pub(crate) fn update_check_alias_after(&mut self, token: &Token) {
        let cmd_pos = self.check_alias;
        let ctx = &mut self.alias_ctx;
        let case_top = ctx.case_stack.last().copied();
        self.check_alias = match token {
            Token::Word(word) => {
                if let Some(saved) = ctx.redirect_target.take() {
                    saved
                } else {
                    let lit = word.as_literal();
                    match case_top {
                        Some(CaseState::Subject) => {
                            *ctx.case_stack.last_mut().expect("non-empty") = CaseState::In;
                            false
                        }
                        Some(CaseState::In) => {
                            if lit == Some("in") {
                                *ctx.case_stack.last_mut().expect("non-empty") = CaseState::Pattern;
                            }
                            false
                        }
                        Some(CaseState::Pattern) => {
                            if lit == Some("esac") {
                                ctx.case_stack.pop();
                            }
                            false
                        }
                        _ if !cmd_pos => false,
                        _ => match lit {
                            Some("if" | "then" | "else" | "elif" | "do" | "while" | "until")
                            | Some("{" | "!") => true,
                            Some("case") => {
                                ctx.case_stack.push(CaseState::Subject);
                                false
                            }
                            Some("esac") => {
                                if case_top == Some(CaseState::Body) {
                                    ctx.case_stack.pop();
                                }
                                false
                            }
                            // An assignment in a command prefix keeps the
                            // following word in command-name position.
                            _ => is_assignment_word(word),
                        },
                    }
                }
            }
            Token::Newline => !matches!(case_top, Some(CaseState::In | CaseState::Pattern)),
            Token::LParen | Token::Pipe => case_top != Some(CaseState::Pattern),
            Token::RParen => {
                if case_top == Some(CaseState::Pattern) {
                    *ctx.case_stack.last_mut().expect("non-empty") = CaseState::Body;
                }
                true
            }
            Token::DSemi | Token::SemiAnd => {
                if case_top == Some(CaseState::Body) {
                    *ctx.case_stack.last_mut().expect("non-empty") = CaseState::Pattern;
                }
                false
            }
            Token::Semi | Token::AndIf | Token::OrIf | Token::Amp => true,
            Token::Less
            | Token::Great
            | Token::DLess
            | Token::DGreat
            | Token::LessAnd
            | Token::GreatAnd
            | Token::LessGreat
            | Token::DLessDash
            | Token::Clobber => {
                ctx.redirect_target = Some(cmd_pos);
                false
            }
            Token::IoNumber(_) | Token::Eof => cmd_pos,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::super::Lexer;
    use super::super::token::Token;
    use crate::env::aliases::AliasStore;

    /// Tokenize `input` with `aliases`, rendering words as their literal
    /// text and operators as their debug name.
    fn words(input: &str, aliases: &[(&str, &str)]) -> Vec<String> {
        let mut store = AliasStore::default();
        for (name, value) in aliases {
            store.set(name, value);
        }
        let mut lexer = Lexer::new_with_aliases(input, &store);
        let mut out = Vec::new();
        loop {
            match lexer.next_token().unwrap().token {
                Token::Eof => break,
                Token::Word(w) => out.push(w.as_literal().unwrap_or("?").to_string()),
                Token::Newline => out.push("NL".to_string()),
                other => out.push(format!("{other:?}")),
            }
        }
        out
    }

    #[test]
    fn guard_is_released_after_the_alias_text_is_consumed() {
        assert_eq!(
            words("e a; e b", &[("e", "echo")]),
            ["echo", "a", "Semi", "echo", "b"]
        );
        // Trailing blank: each `e` is a fresh substitution.
        assert_eq!(words("e e e", &[("e", "echo ")]), ["echo", "echo", "echo"]);
    }

    #[test]
    fn mutual_recursion_terminates() {
        assert_eq!(words("a x", &[("a", "b "), ("b", "a ")]), ["a", "x"]);
        assert_eq!(words("ls", &[("ls", "ls -l")]), ["ls", "-l"]);
    }

    #[test]
    fn substituted_after_command_introducing_reserved_words() {
        let e = [("e", "echo")];
        assert_eq!(
            words("if e; then e; else e; fi", &e),
            [
                "if", "echo", "Semi", "then", "echo", "Semi", "else", "echo", "Semi", "fi"
            ]
        );
        assert_eq!(words("{ e; }", &e), ["{", "echo", "Semi", "}"]);
        assert_eq!(words("! e", &e), ["!", "echo"]);
        let w = words("while e; do e; done", &e);
        assert_eq!((w[1].as_str(), w[4].as_str()), ("echo", "echo"));
    }

    #[test]
    fn substituted_after_assignments_and_prefix_redirects() {
        let e = [("e", "echo")];
        assert_eq!(words("x=1 y=2 e", &e), ["x=1", "y=2", "echo"]);
        assert_eq!(words(">f e", &e), ["Great", "f", "echo"]);
        // ... but never for the redirect target itself.
        assert_eq!(words("echo >e", &e), ["echo", "Great", "e"]);
    }

    #[test]
    fn case_subject_and_patterns_are_not_substituted() {
        let e = [("e", "echo")];
        assert_eq!(
            words("case e in e) e;; (e|e) e;; esac", &e),
            [
                "case", "e", "in", "e", "RParen", "echo", "DSemi", "LParen", "e", "Pipe", "e",
                "RParen", "echo", "DSemi", "esac"
            ]
        );
        assert_eq!(
            words("case e in\ne) e\n;;\nesac; e", &e),
            [
                "case", "e", "in", "NL", "e", "RParen", "echo", "NL", "DSemi", "NL", "esac",
                "Semi", "echo"
            ]
        );
    }

    #[test]
    fn reserved_words_are_never_substituted() {
        assert_eq!(words("if :; then :; fi", &[("if", "echo")])[0], "if");
    }

    #[test]
    fn multi_line_alias_value_is_queued_in_full() {
        assert_eq!(
            words("ml; e", &[("ml", "echo 1\necho 2"), ("e", "echo")]),
            ["echo", "1", "NL", "echo", "2", "Semi", "echo"]
        );
    }
}
