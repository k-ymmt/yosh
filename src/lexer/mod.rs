mod alias;
mod heredoc;
pub mod reserved;
mod scanner;
pub mod token;
mod word;

use std::collections::HashMap;
use std::rc::Rc;

use crate::parser::ast::WordPart;
use token::SpannedToken;

/// A full look-ahead snapshot: the cursor plus the alias-substitution
/// state (see `Lexer::save_state`).
pub struct LexerState {
    cursor: CursorState,
    alias_state: alias::AliasState,
}

/// A cheap snapshot of only the byte-cursor fields (`pos`/`line`/`column`).
///
/// Unlike `LexerState`, this does NOT capture the alias-substitution state
/// (`alias::AliasState`: the token queue, command-name position, and the
/// recursion guard). It is only safe to use around a scan that provably
/// never calls anything that can mutate that state — i.e. no call to
/// `Lexer::next_token` (the only place that touches it).
/// Plain byte-level helpers (`advance`, `current_byte`, `at_end`, ...) are
/// fine. See `Lexer::try_read_io_number` for the sole caller and its
/// safety argument.
pub(crate) struct CursorState {
    pos: usize,
    line: usize,
    column: usize,
}

pub struct PendingHereDoc {
    /// Parse-time identity linking this pending entry to the AST `HereDoc`
    /// node that registered it, so bodies read later (possibly several
    /// commands after registration) are attached to the right redirect.
    pub id: u64,
    pub delimiter: String,
    pub strip_tabs: bool,
    /// The delimiter was quoted: the body is taken verbatim (no line
    /// continuation joining, no expansion).
    pub quoted: bool,
}

pub struct Lexer {
    /// Shared so a `$(...)` body can be parsed by a sub-lexer over the
    /// same buffer without copying the rest of the input.
    input: Rc<[u8]>,
    pos: usize,
    line: usize,
    column: usize,
    pending_heredocs: Vec<PendingHereDoc>,
    heredoc_bodies: Vec<(u64, Vec<WordPart>)>,
    next_heredoc_id: u64,
    /// Alias table, shared (not copied) with the sub-lexers that parse
    /// `$(...)` and backquoted bodies: as in dash, a substitution's body
    /// is parsed with the aliases in effect when the enclosing command is
    /// parsed.
    aliases: Rc<HashMap<String, String>>,
    /// Alias names never substituted by this lexer or its sub-lexers: the
    /// aliases whose values are being read when this lexer was created
    /// for a command substitution inside an alias value (dash's
    /// "alias in use" guard reaches into `alias a='echo $(a)'`).
    blocked_aliases: Rc<[String]>,
    /// Whether words this lexer reads itself are alias-substituted. Off
    /// for the lexer that tokenizes an alias value: its tokens are
    /// substituted lazily when dequeued, but command substitutions inside
    /// the value still see `aliases`.
    substitute_aliases: bool,
    /// While non-zero, `\<newline>` line continuations are NOT removed by
    /// `advance` (single quotes, `$'...'`, here-document bodies).
    no_join: u32,
    /// Current word-construct nesting depth (see
    /// `parser::nesting::MAX_NEST_DEPTH`).
    nest_depth: usize,
    /// Number of nested command-substitution bodies parsed so far; a
    /// token whose lexing bumped it is expensive and gets cached.
    subparse_count: u64,
    /// Tokens containing command substitutions, keyed by start offset:
    /// start (line, column), the token, and the end cursor. The parser's
    /// save/restore look-ahead re-lexes such a token, and without this
    /// cache each nesting level of `$(echo $(echo ...))` doubled the
    /// parse time.
    token_cache: HashMap<usize, CachedToken>,
    /// Alias-substitution state (token queue, command-name position,
    /// recursion guard); saved and restored with the cursor.
    alias_state: alias::AliasState,
}

struct CachedToken {
    line: usize,
    column: usize,
    token: SpannedToken,
    end: (usize, usize, usize),
}

fn is_name_start(ch: u8) -> bool {
    ch.is_ascii_alphabetic() || ch == b'_'
}

fn is_name_char(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_'
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self::over(Rc::from(input.as_bytes()))
    }

    /// A lexer at the start of `input`, with no aliases, no pending
    /// here-documents and no nesting.
    #[inline]
    fn over(input: Rc<[u8]>) -> Self {
        Lexer {
            input,
            pos: 0,
            line: 1,
            column: 1,
            pending_heredocs: Vec::new(),
            heredoc_bodies: Vec::new(),
            next_heredoc_id: 0,
            aliases: Rc::default(),
            blocked_aliases: Rc::new([]),
            substitute_aliases: true,
            no_join: 0,
            nest_depth: 0,
            subparse_count: 0,
            token_cache: HashMap::new(),
            alias_state: alias::AliasState::default(),
        }
    }

    /// A lexer over the same input, starting at the current position, for
    /// parsing a nested `$(...)` body with the real grammar. It shares this
    /// lexer's alias table but starts with fresh alias-substitution state
    /// and no pending here-documents (the body is a separate program), one
    /// nesting level deeper.
    pub(crate) fn sub_lexer(&self) -> Lexer {
        let mut lexer = Lexer::over(Rc::clone(&self.input));
        lexer.restore_cursor(self.save_cursor());
        lexer.inherit_aliases(self);
        lexer
    }

    /// A fresh lexer over `input` that inherits this lexer's nesting depth
    /// and alias table (for re-parsing a backquoted command body).
    pub(crate) fn nested_lexer(&self, input: &str) -> Lexer {
        let mut lexer = Lexer::new(input);
        lexer.inherit_aliases(self);
        lexer
    }

    /// Set up a lexer for a command substitution body found by `parent`:
    /// one nesting level deeper, with the parent's alias table and alias
    /// recursion guard.
    fn inherit_aliases(&mut self, parent: &Lexer) {
        self.nest_depth = parent.nest_depth + crate::parser::SUBPARSE_NEST_COST;
        self.aliases = Rc::clone(&parent.aliases);
        self.blocked_aliases = Rc::clone(&parent.blocked_aliases);
    }

    /// Error out once word-level nesting gets too deep (see
    /// `parser::nesting`). The returned guard keeps this level counted
    /// against the shared stack budget while it is alive.
    pub(crate) fn check_nest_depth(&self) -> crate::error::Result<crate::parser::StackBudget> {
        crate::parser::enter_word_level(self.nest_depth, self.line, self.column)
    }

    pub fn new_with_aliases(input: &str, aliases: &crate::env::aliases::AliasStore) -> Self {
        let mut lexer = Self::new(input);
        lexer.set_aliases(aliases);
        lexer
    }

    /// Returns the current byte position in the input.
    /// Since alias expansion uses a token queue instead of rewriting the buffer,
    /// this position always maps to the original input.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// The raw source text between two byte positions (as returned by
    /// `position`). Used to recover the literal text of a here-document
    /// delimiter word.
    pub(crate) fn source_text(&self, start: usize, end: usize) -> String {
        String::from_utf8_lossy(&self.input[start.min(end)..end.min(self.input.len())]).into_owned()
    }

    /// Replace the alias table (used when a persistent parser reads a
    /// script command by command and earlier commands changed aliases).
    pub fn set_aliases(&mut self, aliases: &crate::env::aliases::AliasStore) {
        self.aliases = Rc::new(
            aliases
                .sorted_iter()
                .into_iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
        );
    }

    /// Drop cached command-substitution tokens (see `token_cache`); the
    /// parser calls this where look-ahead can no longer rewind to them.
    pub(crate) fn clear_token_cache(&mut self) {
        if !self.token_cache.is_empty() {
            self.token_cache.clear();
        }
    }

    pub fn save_state(&self) -> LexerState {
        LexerState {
            cursor: self.save_cursor(),
            alias_state: self.alias_state.clone(),
        }
    }

    pub fn restore_state(&mut self, state: LexerState) {
        self.restore_cursor(state.cursor);
        self.alias_state = state.alias_state;
    }

    /// When the token most recently returned by `next_token` was produced
    /// by alias substitution, its raw spelling in the alias value (leading
    /// blanks included) — the input text at the lexer's position spells
    /// the alias name, not the token. `None` for a token read directly
    /// from the input.
    pub(crate) fn last_token_alias_source(&self) -> Option<&str> {
        self.alias_state.last_source()
    }

    /// Cheap cursor-only snapshot; see `CursorState` for the safety contract.
    pub(crate) fn save_cursor(&self) -> CursorState {
        CursorState {
            pos: self.pos,
            line: self.line,
            column: self.column,
        }
    }

    pub(crate) fn restore_cursor(&mut self, state: CursorState) {
        self.pos = state.pos;
        self.line = state.line;
        self.column = state.column;
    }
}

#[cfg(test)]
mod tests {
    use super::token::Token;
    use super::*;
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
            vec![
                Token::Pipe,
                Token::Semi,
                Token::Amp,
                Token::LParen,
                Token::RParen
            ]
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
    fn test_utf8_literals_preserved() {
        let tokens = tokenize("printf 日本語 '単引用' \"二重引用\"");
        assert_eq!(
            tokens,
            vec![
                Token::Word(Word::literal("printf")),
                Token::Word(Word::literal("日本語")),
                Token::Word(Word {
                    parts: vec![WordPart::SingleQuoted("単引用".to_string())],
                }),
                Token::Word(Word {
                    parts: vec![WordPart::DoubleQuoted(vec![WordPart::Literal(
                        "二重引用".to_string()
                    )])],
                }),
            ]
        );
    }

    #[test]
    fn test_backslash_escape() {
        // `\<char>` unquoted escape now emits EscapedLiteral to preserve the
        // escape metadata for downstream tilde-prefix recognition (POSIX §2.6.1).
        let tokens = tokenize("echo hello\\ world");
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![
                    WordPart::Literal("hello".to_string()),
                    WordPart::EscapedLiteral(" ".to_string()),
                    WordPart::Literal("world".to_string()),
                ],
            })
        );
    }

    #[test]
    fn test_line_continuation() {
        // POSIX §2.2.1: `\<newline>` is removed before tokenization. The lexer
        // now accumulates the two literal chunks into a SINGLE merged Literal
        // (no split, no empty entry).
        let tokens = tokenize("echo hel\\\nlo");
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![WordPart::Literal("hello".to_string())],
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
        assert_eq!(
            err.kind,
            crate::error::ShellErrorKind::Parse(ParseErrorKind::UnterminatedSingleQuote)
        );
    }

    #[test]
    fn test_unterminated_double_quote() {
        let mut lexer = Lexer::new("echo \"hello");
        let _ = lexer.next_token().unwrap();
        let err = lexer.next_token().unwrap_err();
        assert_eq!(
            err.kind,
            crate::error::ShellErrorKind::Parse(ParseErrorKind::UnterminatedDoubleQuote)
        );
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
                parts: vec![WordPart::ArithSub("$(echo \"3)\") + 1".to_string())]
            })]
        );
    }

    #[test]
    fn test_arith_expansion_with_single_quoted_paren_in_cmd_sub() {
        let tokens = tokenize("$(($(echo '3)') + 1))");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::ArithSub("$(echo '3)') + 1".to_string())]
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

    #[test]
    fn lexer_backslash_escape_emits_escaped_literal() {
        let mut lexer = Lexer::new("x=\\~/bin");
        let tok = lexer.next_token().expect("token");
        let parts = match &tok.token {
            Token::Word(w) => &w.parts,
            other => panic!("expected Word, got {:?}", other),
        };
        let has_escaped = parts
            .iter()
            .any(|p| matches!(p, WordPart::EscapedLiteral(s) if s == "~"));
        assert!(
            has_escaped,
            "expected EscapedLiteral(~) in parts, got {:?}",
            parts
        );
    }

    #[test]
    fn lexer_line_continuation_merges_literals() {
        let mut lexer = Lexer::new("x=foo\\\nbar");
        let tok = lexer.next_token().expect("token");
        let parts = match &tok.token {
            Token::Word(w) => &w.parts,
            other => panic!("expected Word, got {:?}", other),
        };
        assert_eq!(
            parts.len(),
            1,
            "expected single merged Literal, got {:?}",
            parts
        );
        match &parts[0] {
            WordPart::Literal(s) => assert_eq!(s, "x=foobar"),
            other => panic!("expected Literal, got {:?}", other),
        }
    }

    // ---- Task 4 item 17 locking tests ----
    //
    // These lock in behavior around `try_read_io_number`'s `save_state`/
    // `restore_state` pair (src/lexer/scanner.rs) while it interacts with
    // alias-expansion state (the token queue, command-name position, and
    // the recursion guard). They must stay green whether
    // `save_state` does a full clone or a light pos/line/column-only
    // snapshot, because nothing between save and restore in
    // `try_read_io_number` can mutate the alias-related fields (only the
    // byte-level `advance`/`current_byte` are called).

    #[test]
    fn io_number_lookahead_after_alias_expanded_word_still_tokenizes_correctly() {
        // `ll` expands to `ls -l`; the token queue holds `-l` when the lexer
        // resumes scanning raw input at `2>err`. The io-number lookahead
        // inside that raw scan must not disturb the still-pending queue.
        use crate::env::aliases::AliasStore;
        let mut aliases = AliasStore::default();
        aliases.set("ll", "ls -l");
        let mut lexer = Lexer::new_with_aliases("ll 2>err", &aliases);

        let t1 = lexer.next_token().unwrap().token;
        assert_eq!(t1, Token::Word(Word::literal("ls")));
        let t2 = lexer.next_token().unwrap().token;
        assert_eq!(t2, Token::Word(Word::literal("-l")));
        let t3 = lexer.next_token().unwrap().token;
        assert_eq!(t3, Token::IoNumber(2));
        let t4 = lexer.next_token().unwrap().token;
        assert_eq!(t4, Token::Great);
        let t5 = lexer.next_token().unwrap().token;
        assert_eq!(t5, Token::Word(Word::literal("err")));
    }

    #[test]
    fn io_number_lookahead_during_recursive_alias_expansion_preserves_guard() {
        // Alias `a` expands to `2x a` (self-referential) with a digit-led
        // word (`2x`) that is NOT a redirect, so the io-number lookahead
        // inside the sub-lexer used for `a`'s expansion must backtrack. The
        // second `a` in the expansion is scanned by that SAME sub-lexer
        // (still inside `next_token`'s alias-expansion block), while `a` is
        // already being expanded — the recursion guard must still
        // correctly block it from expanding again, regardless of the
        // intervening io-number backtrack.
        use crate::env::aliases::AliasStore;
        let mut aliases = AliasStore::default();
        aliases.set("a", "2x a");
        let mut lexer = Lexer::new_with_aliases("a", &aliases);

        // "2x" is not a valid io-number (not followed by < or >), so it must
        // come back as a literal word, not silently swallowed or corrupted.
        let t1 = lexer.next_token().unwrap().token;
        assert_eq!(t1, Token::Word(Word::literal("2x")));

        // The second "a" must come back as a literal word (recursion
        // blocked), not loop or panic.
        let t2 = lexer.next_token().unwrap().token;
        assert_eq!(t2, Token::Word(Word::literal("a")));

        let t3 = lexer.next_token().unwrap().token;
        assert_eq!(t3, Token::Eof);
    }

    #[test]
    fn io_number_lookahead_after_trailing_space_alias_chain() {
        // Alias `a` expands to `2x ` (trailing space), which forces the NEXT
        // raw-scanned word to also be alias-checked. The next raw input is
        // `4>out`, exercising the io-number lookahead on a freshly scanned
        // (non-queued) token immediately after an alias-chain continuation
        // — the scenario the light `CursorState` snapshot must handle
        // identically to the full `LexerState` snapshot.
        use crate::env::aliases::AliasStore;
        let mut aliases = AliasStore::default();
        aliases.set("a", "2x ");
        let mut lexer = Lexer::new_with_aliases("a 4>out", &aliases);

        let t1 = lexer.next_token().unwrap().token;
        assert_eq!(t1, Token::Word(Word::literal("2x")));
        let t2 = lexer.next_token().unwrap().token;
        assert_eq!(t2, Token::IoNumber(4));
        let t3 = lexer.next_token().unwrap().token;
        assert_eq!(t3, Token::Great);
        let t4 = lexer.next_token().unwrap().token;
        assert_eq!(t4, Token::Word(Word::literal("out")));
    }

    #[test]
    fn io_number_lookahead_digit_redirect_inside_alias_expansion() {
        // Alias value itself contains a digit-then-redirect sequence, so
        // try_read_io_number's save/restore runs inside the sub-lexer used
        // for alias expansion (whose recursion guard is non-empty).
        use crate::env::aliases::AliasStore;
        let mut aliases = AliasStore::default();
        aliases.set("redir", "cmd 2>/dev/null");
        let mut lexer = Lexer::new_with_aliases("redir", &aliases);

        let t1 = lexer.next_token().unwrap().token;
        assert_eq!(t1, Token::Word(Word::literal("cmd")));
        let t2 = lexer.next_token().unwrap().token;
        assert_eq!(t2, Token::IoNumber(2));
        let t3 = lexer.next_token().unwrap().token;
        assert_eq!(t3, Token::Great);
        let t4 = lexer.next_token().unwrap().token;
        assert_eq!(t4, Token::Word(Word::literal("/dev/null")));
    }
}
