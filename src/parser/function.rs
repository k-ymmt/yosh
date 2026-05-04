use super::Parser;
use super::ast::FunctionDef;
use super::word::is_valid_name;
use crate::error;
use crate::lexer::token::Token;
use std::rc::Rc;

impl Parser {
    /// Try to parse a function definition: NAME ( ) linebreak compound_command [redirect_list]
    pub(super) fn try_parse_function_def(&mut self) -> error::Result<Option<FunctionDef>> {
        // Check if current token is a Word with a valid name
        let name = match &self.current.token {
            Token::Word(word) => {
                if let Some(lit) = word.as_literal() {
                    if is_valid_name(lit) {
                        lit.to_string()
                    } else {
                        return Ok(None);
                    }
                } else {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        };

        // Save state for backtracking
        let saved_lexer_state = self.lexer.save_state();
        let saved_current = self.current.clone();

        // Advance past the name
        self.advance()?;

        // Check for (
        if self.current.token != Token::LParen {
            // Restore state
            self.lexer.restore_state(saved_lexer_state);
            self.current = saved_current;
            return Ok(None);
        }
        self.advance()?;

        // Check for )
        if self.current.token != Token::RParen {
            // Restore state
            self.lexer.restore_state(saved_lexer_state);
            self.current = saved_current;
            return Ok(None);
        }
        self.advance()?;

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
