pub mod token;
mod scanner;
mod word;

use std::collections::{HashMap, HashSet};

use crate::error::{self, ShellError, ParseErrorKind};
use crate::parser::ast::WordPart;
use token::{SpannedToken, Token};

pub struct LexerState {
    pub pos: usize,
    pub line: usize,
    pub column: usize,
    alias_token_queue: Vec<SpannedToken>,
    check_alias: bool,
    expanding_aliases: HashSet<String>,
}

pub struct PendingHereDoc {
    pub delimiter: String,
    #[allow(dead_code)]
    pub quoted: bool,
    pub strip_tabs: bool,
}

pub struct Lexer {
    input: Vec<u8>,
    pos: usize,
    line: usize,
    column: usize,
    pending_heredocs: Vec<PendingHereDoc>,
    heredoc_bodies: Vec<Vec<WordPart>>,
    aliases: HashMap<String, String>,
    expanding_aliases: HashSet<String>,
    check_alias: bool,
    /// Queue of tokens produced by alias expansion, to be returned before reading more input.
    alias_token_queue: Vec<SpannedToken>,
}

fn is_name_start(ch: u8) -> bool {
    ch.is_ascii_alphabetic() || ch == b'_'
}

fn is_name_char(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_'
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            input: input.as_bytes().to_vec(),
            pos: 0,
            line: 1,
            column: 1,
            pending_heredocs: Vec::new(),
            heredoc_bodies: Vec::new(),
            aliases: HashMap::new(),
            expanding_aliases: HashSet::new(),
            check_alias: true,
            alias_token_queue: Vec::new(),
        }
    }

    pub fn new_with_aliases(input: &str, aliases: &crate::env::aliases::AliasStore) -> Self {
        let mut lexer = Self::new(input);
        for (name, value) in aliases.sorted_iter() {
            lexer.aliases.insert(name.to_string(), value.to_string());
        }
        lexer
    }

    /// Returns the current byte position in the input.
    /// Since alias expansion uses a token queue instead of rewriting the buffer,
    /// this position always maps to the original input.
    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn save_state(&self) -> LexerState {
        LexerState {
            pos: self.pos,
            line: self.line,
            column: self.column,
            alias_token_queue: self.alias_token_queue.clone(),
            check_alias: self.check_alias,
            expanding_aliases: self.expanding_aliases.clone(),
        }
    }

    pub fn restore_state(&mut self, state: LexerState) {
        self.pos = state.pos;
        self.line = state.line;
        self.column = state.column;
        self.alias_token_queue = state.alias_token_queue;
        self.check_alias = state.check_alias;
        self.expanding_aliases = state.expanding_aliases;
    }

    pub fn register_heredoc(&mut self, delimiter: String, quoted: bool, strip_tabs: bool) {
        self.pending_heredocs.push(PendingHereDoc { delimiter, quoted, strip_tabs });
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

    fn read_heredoc_body(&mut self, hd: &PendingHereDoc) -> error::Result<Vec<WordPart>> {
        let mut body = String::new();
        loop {
            if self.at_end() {
                return Err(ShellError::parse(
                    ParseErrorKind::InvalidHereDoc,
                    self.line,
                    self.column,
                    format!("here-document delimited by '{}' was not closed", hd.delimiter),
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
            let trailing_space = alias_value.ends_with(' ')
                || alias_value.ends_with('\t');

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

    fn update_check_alias_after(&mut self, token: &Token) {
        match token {
            Token::Semi | Token::Newline | Token::Pipe | Token::AndIf | Token::OrIf
            | Token::LParen | Token::Amp | Token::DSemi => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};

    fn tokenize(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let st = lexer.next_token().unwrap();
            if st.token == Token::Eof {
                break;
            }
            tokens.push(st.token);
        }
        tokens
    }

    // ---- Task 4 tests ----

    #[test]
    fn test_empty_input() {
        assert_eq!(tokenize(""), vec![]);
    }

    #[test]
    fn test_newline() {
        assert_eq!(tokenize("\n"), vec![Token::Newline]);
    }

    #[test]
    fn test_single_char_operators() {
        assert_eq!(
            tokenize("| ; & ( )"),
            vec![Token::Pipe, Token::Semi, Token::Amp, Token::LParen, Token::RParen]
        );
    }

    #[test]
    fn test_multi_char_operators() {
        assert_eq!(
            tokenize("&& || ;; ;&"),
            vec![Token::AndIf, Token::OrIf, Token::DSemi, Token::SemiAnd]
        );
    }

    #[test]
    fn test_redirect_operators() {
        assert_eq!(
            tokenize("< > >> <& >& <> >|"),
            vec![
                Token::Less,
                Token::Great,
                Token::DGreat,
                Token::LessAnd,
                Token::GreatAnd,
                Token::LessGreat,
                Token::Clobber
            ]
        );
    }

    #[test]
    fn test_heredoc_operators() {
        assert_eq!(tokenize("<< <<-"), vec![Token::DLess, Token::DLessDash]);
    }

    #[test]
    fn test_comment_ignored() {
        assert_eq!(tokenize("# this is a comment\n"), vec![Token::Newline]);
    }

    #[test]
    fn test_whitespace_only() {
        assert_eq!(tokenize("   \t  "), vec![]);
    }

    // ---- Task 5 tests ----

    #[test]
    fn test_unquoted_words() {
        let tokens = tokenize("echo hello world");
        assert_eq!(
            tokens,
            vec![
                Token::Word(Word::literal("echo")),
                Token::Word(Word::literal("hello")),
                Token::Word(Word::literal("world")),
            ]
        );
    }

    #[test]
    fn test_single_quoted_word() {
        let tokens = tokenize("echo 'hello world'");
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![WordPart::SingleQuoted("hello world".to_string())]
            })
        );
    }

    #[test]
    fn test_double_quoted_word() {
        let tokens = tokenize("echo \"hello world\"");
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![WordPart::DoubleQuoted(vec![WordPart::Literal(
                    "hello world".to_string()
                )])],
            })
        );
    }

    #[test]
    fn test_backslash_escape() {
        let tokens = tokenize("echo hello\\ world");
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![
                    WordPart::Literal("hello".to_string()),
                    WordPart::Literal(" ".to_string()),
                    WordPart::Literal("world".to_string()),
                ],
            })
        );
    }

    #[test]
    fn test_line_continuation() {
        let tokens = tokenize("echo hel\\\nlo");
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![
                    WordPart::Literal("hel".to_string()),
                    WordPart::Literal("lo".to_string())
                ],
            })
        );
    }

    #[test]
    fn test_dollar_single_quote() {
        let tokens = tokenize("echo $'hello\\nworld'");
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![WordPart::DollarSingleQuoted("hello\nworld".to_string())],
            })
        );
    }

    #[test]
    fn test_dollar_single_quote_escapes() {
        let tokens = tokenize("$'\\t\\r\\a\\b\\\\\\\"\\''");
        assert_eq!(tokens.len(), 1);
        assert_eq!(
            tokens[0],
            Token::Word(Word {
                parts: vec![WordPart::DollarSingleQuoted(
                    "\t\r\x07\x08\\\"'".to_string()
                )],
            })
        );
    }

    #[test]
    fn test_mixed_quoting_in_word() {
        let tokens = tokenize("he\"ll\"o");
        assert_eq!(tokens.len(), 1);
        assert_eq!(
            tokens[0],
            Token::Word(Word {
                parts: vec![
                    WordPart::Literal("he".to_string()),
                    WordPart::DoubleQuoted(vec![WordPart::Literal("ll".to_string())]),
                    WordPart::Literal("o".to_string()),
                ],
            })
        );
    }

    #[test]
    fn test_unterminated_single_quote() {
        let mut lexer = Lexer::new("echo 'hello");
        let _ = lexer.next_token().unwrap();
        let err = lexer.next_token().unwrap_err();
        assert_eq!(err.kind, crate::error::ShellErrorKind::Parse(ParseErrorKind::UnterminatedSingleQuote));
    }

    #[test]
    fn test_unterminated_double_quote() {
        let mut lexer = Lexer::new("echo \"hello");
        let _ = lexer.next_token().unwrap();
        let err = lexer.next_token().unwrap_err();
        assert_eq!(err.kind, crate::error::ShellErrorKind::Parse(ParseErrorKind::UnterminatedDoubleQuote));
    }

    // ---- Task 6 tests ----

    #[test]
    fn test_simple_param() {
        let tokens = tokenize("$name");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::Parameter(ParamExpr::Simple("name".to_string()))]
            })]
        );
    }

    #[test]
    fn test_param_in_word() {
        let tokens = tokenize("hello${x}world");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![
                    WordPart::Literal("hello".to_string()),
                    WordPart::Parameter(ParamExpr::Simple("x".to_string())),
                    WordPart::Literal("world".to_string()),
                ]
            })]
        );
    }

    #[test]
    fn test_positional_param() {
        let tokens = tokenize("$1 ${10}");
        assert_eq!(
            tokens[0],
            Token::Word(Word {
                parts: vec![WordPart::Parameter(ParamExpr::Positional(1))]
            })
        );
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![WordPart::Parameter(ParamExpr::Positional(10))]
            })
        );
    }

    #[test]
    fn test_special_params() {
        let tokens = tokenize("$@ $* $# $? $- $$ $! $0");
        let expected = vec![
            SpecialParam::At,
            SpecialParam::Star,
            SpecialParam::Hash,
            SpecialParam::Question,
            SpecialParam::Dash,
            SpecialParam::Dollar,
            SpecialParam::Bang,
            SpecialParam::Zero,
        ];
        for (i, sp) in expected.into_iter().enumerate() {
            assert_eq!(
                tokens[i],
                Token::Word(Word {
                    parts: vec![WordPart::Parameter(ParamExpr::Special(sp))]
                })
            );
        }
    }

    #[test]
    fn test_param_default() {
        let tokens = tokenize("${x:-default}");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::Parameter(ParamExpr::Default {
                    name: "x".to_string(),
                    word: Some(Word::literal("default")),
                    null_check: true,
                })]
            })]
        );
    }

    #[test]
    fn test_param_default_no_colon() {
        let tokens = tokenize("${x-default}");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::Parameter(ParamExpr::Default {
                    name: "x".to_string(),
                    word: Some(Word::literal("default")),
                    null_check: false,
                })]
            })]
        );
    }

    #[test]
    fn test_param_length() {
        let tokens = tokenize("${#name}");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::Parameter(ParamExpr::Length("name".to_string()))]
            })]
        );
    }

    #[test]
    fn test_param_strip_suffix() {
        let tokens = tokenize("${name%.txt}");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::Parameter(ParamExpr::StripShortSuffix(
                    "name".to_string(),
                    Word::literal(".txt")
                ))]
            })]
        );
    }

    #[test]
    fn test_param_strip_long_prefix() {
        let tokens = tokenize("${name##*/}");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::Parameter(ParamExpr::StripLongPrefix(
                    "name".to_string(),
                    Word::literal("*/")
                ))]
            })]
        );
    }

    #[test]
    fn test_command_sub_dollar_paren() {
        let tokens = tokenize("$(echo hello)");
        assert_eq!(tokens.len(), 1);
        if let Token::Word(w) = &tokens[0] {
            assert_eq!(w.parts.len(), 1);
            assert!(matches!(&w.parts[0], WordPart::CommandSub(_)));
        } else {
            panic!("expected word");
        }
    }

    #[test]
    fn test_arith_expansion() {
        let tokens = tokenize("$((1 + 2))");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::ArithSub("1 + 2".to_string())]
            })]
        );
    }

    #[test]
    fn test_backtick_command_sub() {
        let tokens = tokenize("`echo hello`");
        assert_eq!(tokens.len(), 1);
        if let Token::Word(w) = &tokens[0] {
            assert!(matches!(&w.parts[0], WordPart::CommandSub(_)));
        } else {
            panic!("expected word");
        }
    }

    #[test]
    fn test_dollar_in_double_quotes() {
        let tokens = tokenize("\"hello $name\"");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::DoubleQuoted(vec![
                    WordPart::Literal("hello ".to_string()),
                    WordPart::Parameter(ParamExpr::Simple("name".to_string())),
                ])]
            })]
        );
    }

    // ---- Task 7 tests ----

    #[test]
    fn test_io_number_redirect() {
        let tokens = tokenize("2>/dev/null");
        assert_eq!(
            tokens,
            vec![
                Token::IoNumber(2),
                Token::Great,
                Token::Word(Word::literal("/dev/null"))
            ]
        );
    }

    #[test]
    fn test_io_number_input() {
        let tokens = tokenize("0<input.txt");
        assert_eq!(
            tokens,
            vec![
                Token::IoNumber(0),
                Token::Less,
                Token::Word(Word::literal("input.txt"))
            ]
        );
    }

    #[test]
    fn test_digits_not_followed_by_redirect() {
        let tokens = tokenize("123 abc");
        assert_eq!(
            tokens,
            vec![
                Token::Word(Word::literal("123")),
                Token::Word(Word::literal("abc"))
            ]
        );
    }

    #[test]
    fn test_fd_dup() {
        let tokens = tokenize("2>&1");
        assert_eq!(
            tokens,
            vec![
                Token::IoNumber(2),
                Token::GreatAnd,
                Token::Word(Word::literal("1"))
            ]
        );
    }
}
