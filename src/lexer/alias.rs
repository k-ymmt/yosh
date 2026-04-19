use super::Lexer;
use super::token::{SpannedToken, Token};
use crate::error;

impl Lexer {
    pub fn next_token(&mut self) -> error::Result<SpannedToken> {
        // Return queued tokens from alias expansion first
        if let Some(tok) = self.alias_token_queue.first().cloned() {
            self.alias_token_queue.remove(0);
            // Update check_alias based on this queued token
            self.update_check_alias_after(&tok.token);
            return Ok(tok);
        }

        let tok = self.next_token_raw()?;

        // Check for alias expansion on Word tokens when check_alias is set
        if self.check_alias
            && let Token::Word(ref word) = tok.token
            && let Some(word_text) = word.as_literal()
            && !self.expanding_aliases.contains(word_text)
            && let Some(alias_value) = self.aliases.get(word_text).cloned()
        {
            let word_text = word_text.to_string();
            // Mark as expanding to prevent recursion
            self.expanding_aliases.insert(word_text);

            // Check if alias value ends with whitespace —
            // if so, the next word after the expansion should also be alias-checked
            let trailing_space = alias_value.ends_with(' ') || alias_value.ends_with('\t');

            // Tokenize the alias value into a separate token stream
            let mut alias_lexer = Lexer::new(&alias_value);
            // Copy aliases and expanding_aliases to the sub-lexer
            alias_lexer.aliases = self.aliases.clone();
            alias_lexer.expanding_aliases = self.expanding_aliases.clone();
            alias_lexer.check_alias = true;

            let mut tokens = Vec::new();
            loop {
                let t = alias_lexer.next_token()?;
                if t.token == Token::Eof {
                    break;
                }
                tokens.push(t);
            }

            // Merge back any recursion-prevention state
            self.expanding_aliases = alias_lexer.expanding_aliases;

            if tokens.is_empty() {
                // Alias expanded to nothing, get next token
                if trailing_space {
                    self.check_alias = true;
                }
                return self.next_token();
            }

            // Return the first token, queue the rest
            let first = tokens.remove(0);
            self.alias_token_queue = tokens;
            self.update_check_alias_after(&first.token);

            // If alias value ends with space/tab, force next
            // word to be alias-checked (overrides normal behavior)
            if trailing_space {
                self.check_alias = true;
            }

            return Ok(first);
        }

        // After producing a token, decide if next word should be alias-checked
        self.update_check_alias_after(&tok.token);

        Ok(tok)
    }

    pub(crate) fn update_check_alias_after(&mut self, token: &Token) {
        match token {
            Token::Semi
            | Token::Newline
            | Token::Pipe
            | Token::AndIf
            | Token::OrIf
            | Token::LParen
            | Token::Amp
            | Token::DSemi => {
                self.check_alias = true;
            }
            Token::Word(_) => {
                // After first word in command position, stop alias-expanding
                // (unless a previous alias ended with space, already handled above)
                if self.check_alias {
                    self.check_alias = false;
                }
            }
            _ => {
                // For other tokens (redirects, etc.), don't change check_alias
            }
        }
    }
}
