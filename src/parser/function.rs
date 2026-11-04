use super::Parser;
use super::ast::FunctionDef;
use super::word::is_valid_name;
use crate::error;
use crate::lexer::reserved::is_posix_reserved_word;
use crate::lexer::token::Token;
use std::rc::Rc;

impl Parser {
    /// Try to parse a function definition: NAME ( ) linebreak compound_command [redirect_list]
    pub(super) fn try_parse_function_def(&mut self) -> error::Result<Option<FunctionDef>> {
        // Check if current token is a Word with a valid name.
        // POSIX §2.10.2 rule 8: a reserved word in the fname position is
        // that reserved word, never a NAME, so `esac() { :; }` / `fi() ...`
        // are syntax errors (the simple-command parser reports the
        // dangling word), as in dash and bash.
        fn fname(token: &Token) -> Option<&str> {
            match token {
                Token::Word(word) => word
                    .as_literal()
                    .filter(|lit| is_valid_name(lit) && !is_posix_reserved_word(lit)),
                _ => None,
            }
        }
        if fname(&self.current.token).is_none() {
            return Ok(None);
        }

        // Save state for backtracking and advance past the name (the
        // name token moves into the snapshot; it is copied out only once
        // this is known to be a function definition).
        let (saved, advanced) = self.snapshot_and_advance();
        advanced?;

        // Check for (
        if self.current.token != Token::LParen {
            self.rewind(saved);
            return Ok(None);
        }
        self.advance()?;

        // Check for )
        if self.current.token != Token::RParen {
            self.rewind(saved);
            return Ok(None);
        }
        self.advance()?;
        let Some(name) = fname(&saved.current.token).map(str::to_string) else {
            unreachable!("checked before the look-ahead");
        };

        // Skip newlines (linebreak)
        self.skip_newlines()?;

        // Parse compound command body
        let body = self.parse_compound_command()?;

        // Parse optional redirect list
        let redirects = self.parse_redirect_list()?;

        Ok(Some(FunctionDef {
            name,
            body: Rc::new(body),
            redirects,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::ast::Command;
    use super::super::tests::parse;

    #[test]
    fn test_function_def() {
        let prog = parse("myfunc() { echo hello; }");
        let cmd = &prog.commands[0].items[0].0.first.commands[0];
        match cmd {
            Command::FunctionDef(fd) => assert_eq!(fd.name, "myfunc"),
            _ => panic!(),
        }
    }

    #[test]
    fn reserved_word_is_not_a_function_name() {
        for src in [
            "esac() { :; }",
            "fi() { :; }",
            "done() { :; }",
            "then() { :; }",
        ] {
            let mut parser = super::super::Parser::new(src);
            assert!(parser.parse_program().is_err(), "{src} must not parse");
        }
    }

    #[test]
    fn test_function_def_with_redirect() {
        let prog = parse("myfunc() { echo hello; } > out.txt");
        let cmd = &prog.commands[0].items[0].0.first.commands[0];
        match cmd {
            Command::FunctionDef(fd) => {
                assert_eq!(fd.name, "myfunc");
                assert_eq!(fd.redirects.len(), 1);
            }
            _ => panic!(),
        }
    }
}
