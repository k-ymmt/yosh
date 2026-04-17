pub mod token;
pub mod reserved;
mod alias;
mod heredoc;
mod scanner;
mod word;

use std::collections::{HashMap, HashSet};

use crate::parser::ast::WordPart;
use token::SpannedToken;

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::token::Token;
    use crate::error::ParseErrorKind;
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
    fn test_arith_expansion_with_quoted_paren_in_cmd_sub() {
        // $(echo "3)") inside $((...)) — the ')' in double quotes must not
        // prematurely close the command substitution or arithmetic expansion
        let tokens = tokenize("$(($(echo \"3)\") + 1))");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::ArithSub(
                    "$(echo \"3)\") + 1".to_string()
                )]
            })]
        );
    }

    #[test]
    fn test_arith_expansion_with_single_quoted_paren_in_cmd_sub() {
        let tokens = tokenize("$(($(echo '3)') + 1))");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::ArithSub(
                    "$(echo '3)') + 1".to_string()
                )]
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
