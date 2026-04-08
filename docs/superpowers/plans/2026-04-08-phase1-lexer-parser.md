# Phase 1: Lexer + Parser + AST Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a lexer and parser that correctly converts POSIX shell scripts into an AST.

**Architecture:** Hand-written recursive descent parser fed by a context-aware lexer. The lexer tokenizes input into operators and Word structures (preserving quoting and expansion syntax). The parser classifies words (reserved words, assignments) based on grammar position and builds the AST. No external dependencies needed for this phase.

**Tech Stack:** Rust 2024 edition, no external crates.

**Scope note:** This is Phase 1 of 8. Phases 2-8 (execution, expansion, redirections, control flow, builtins, signals, subshells) will have separate plans. Alias expansion is deferred to Phase 6 (requires shell environment).

---

## File Structure

**Create:**
- `src/error.rs` — Error types for lexer and parser
- `src/lexer/mod.rs` — Lexer implementation (tokenization, word scanning, quoting)
- `src/lexer/token.rs` — Token enum and position tracking
- `src/parser/mod.rs` — Recursive descent parser
- `src/parser/ast.rs` — AST node type definitions

**Modify:**
- `src/main.rs` — Module declarations, basic CLI for parse-and-dump

**Reference:**
- `docs/posix-shell-reference.md` — POSIX shell specification reference
- `docs/superpowers/specs/2026-04-08-posix-shell-design.md` — Design specification

---

### Task 1: Project scaffolding and error types

**Files:**
- Modify: `src/main.rs`
- Create: `src/error.rs`
- Create: `src/lexer/mod.rs` (placeholder)
- Create: `src/lexer/token.rs` (placeholder)
- Create: `src/parser/mod.rs` (placeholder)
- Create: `src/parser/ast.rs` (placeholder)

- [ ] **Step 1: Create module structure**

Create `src/error.rs`:

```rust
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct ShellError {
    pub kind: ShellErrorKind,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShellErrorKind {
    UnterminatedSingleQuote,
    UnterminatedDoubleQuote,
    UnterminatedCommandSub,
    UnterminatedArithSub,
    UnterminatedParamExpansion,
    UnterminatedBacktick,
    UnterminatedDollarSingleQuote,
    UnexpectedToken,
    UnexpectedEof,
    InvalidRedirect,
    InvalidFunctionName,
    InvalidHereDoc,
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "kish: line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ShellError {}

impl ShellError {
    pub fn new(kind: ShellErrorKind, line: usize, column: usize, message: impl Into<String>) -> Self {
        Self { kind, line, column, message: message.into() }
    }
}

pub type Result<T> = std::result::Result<T, ShellError>;
```

- [ ] **Step 2: Create placeholder modules**

Create `src/lexer/token.rs`:

```rust
// Token types — implemented in Task 3
```

Create `src/lexer/mod.rs`:

```rust
pub mod token;
// Lexer implementation — Tasks 4-6
```

Create `src/parser/ast.rs`:

```rust
// AST types — implemented in Task 2
```

Create `src/parser/mod.rs`:

```rust
pub mod ast;
// Parser implementation — Tasks 7-11
```

- [ ] **Step 3: Update main.rs with module declarations**

Replace `src/main.rs`:

```rust
mod error;
mod lexer;
mod parser;

fn main() {
    println!("kish: POSIX shell");
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully.

- [ ] **Step 5: Write error display test**

Add to `src/error.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ShellError::new(
            ShellErrorKind::UnexpectedToken,
            5,
            10,
            "unexpected ')'",
        );
        assert_eq!(err.to_string(), "kish: line 5: unexpected ')'");
    }
}
```

- [ ] **Step 6: Run test**

Run: `cargo test -p kish`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/main.rs src/error.rs src/lexer/ src/parser/
git commit -m "feat: project scaffolding and error types for Phase 1"
```

---

### Task 2: AST type definitions

**Files:**
- Modify: `src/parser/ast.rs`

- [ ] **Step 1: Define core AST nodes**

Write `src/parser/ast.rs`:

```rust
/// Top-level program
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub commands: Vec<CompleteCommand>,
}

/// A complete command: a list of AND-OR lists with separators.
/// The separator after each AND-OR list determines execution mode:
/// - Semi (;) or None: sequential
/// - Amp (&): asynchronous
#[derive(Debug, Clone, PartialEq)]
pub struct CompleteCommand {
    pub items: Vec<(AndOrList, Option<SeparatorOp>)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SeparatorOp {
    Semi,
    Amp,
}

/// AND-OR list: pipelines joined by && or ||
#[derive(Debug, Clone, PartialEq)]
pub struct AndOrList {
    pub first: Pipeline,
    pub rest: Vec<(AndOrOp, Pipeline)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AndOrOp {
    And,
    Or,
}

/// Pipeline: one or more commands connected by |
#[derive(Debug, Clone, PartialEq)]
pub struct Pipeline {
    pub negated: bool,
    pub commands: Vec<Command>,
}

/// A single command
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Simple(SimpleCommand),
    Compound(CompoundCommand, Vec<Redirect>),
    FunctionDef(FunctionDef),
}

/// Simple command: assignments, words, redirections
#[derive(Debug, Clone, PartialEq)]
pub struct SimpleCommand {
    pub assignments: Vec<Assignment>,
    pub words: Vec<Word>,
    pub redirects: Vec<Redirect>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Assignment {
    pub name: String,
    pub value: Option<Word>,
}

/// Compound command kinds
#[derive(Debug, Clone, PartialEq)]
pub struct CompoundCommand {
    pub kind: CompoundCommandKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompoundCommandKind {
    BraceGroup {
        body: Vec<CompleteCommand>,
    },
    Subshell {
        body: Vec<CompleteCommand>,
    },
    If {
        condition: Vec<CompleteCommand>,
        then_part: Vec<CompleteCommand>,
        elif_parts: Vec<(Vec<CompleteCommand>, Vec<CompleteCommand>)>,
        else_part: Option<Vec<CompleteCommand>>,
    },
    For {
        var: String,
        words: Option<Vec<Word>>,
        body: Vec<CompleteCommand>,
    },
    While {
        condition: Vec<CompleteCommand>,
        body: Vec<CompleteCommand>,
    },
    Until {
        condition: Vec<CompleteCommand>,
        body: Vec<CompleteCommand>,
    },
    Case {
        word: Word,
        items: Vec<CaseItem>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaseItem {
    pub patterns: Vec<Word>,
    pub body: Vec<CompleteCommand>,
    pub terminator: CaseTerminator,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CaseTerminator {
    Break,
    FallThrough,
}

/// Function definition
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    pub name: String,
    pub body: CompoundCommand,
    pub redirects: Vec<Redirect>,
}

/// Word: a sequence of literal and expandable parts
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub parts: Vec<WordPart>,
}

impl Word {
    pub fn literal(s: &str) -> Self {
        Word {
            parts: vec![WordPart::Literal(s.to_string())],
        }
    }

    /// Returns the literal string if this word contains only a single literal part.
    pub fn as_literal(&self) -> Option<&str> {
        if self.parts.len() == 1 {
            if let WordPart::Literal(s) = &self.parts[0] {
                return Some(s);
            }
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WordPart {
    /// Unquoted literal text
    Literal(String),
    /// Text inside single quotes
    SingleQuoted(String),
    /// Text inside double quotes (may contain expansions)
    DoubleQuoted(Vec<WordPart>),
    /// $'...' with escape sequences already processed
    DollarSingleQuoted(String),
    /// Parameter expansion: $name, ${name}, ${name:-word}, etc.
    Parameter(ParamExpr),
    /// Command substitution: $(commands) or `commands`
    CommandSub(Program),
    /// Arithmetic expansion: $((expression))
    /// Stored as raw string; parsed during expansion phase.
    ArithSub(String),
    /// Tilde prefix: ~ or ~user
    Tilde(Option<String>),
}

/// Parameter expansion forms
#[derive(Debug, Clone, PartialEq)]
pub enum ParamExpr {
    /// $name or ${name}
    Simple(String),
    /// $1, ${10}, etc.
    Positional(usize),
    /// $@, $*, $#, $?, $-, $$, $!, $0
    Special(SpecialParam),
    /// ${#name}
    Length(String),
    /// ${name:-word}, ${name-word}
    Default {
        name: String,
        word: Option<Word>,
        null_check: bool,
    },
    /// ${name:=word}, ${name=word}
    Assign {
        name: String,
        word: Option<Word>,
        null_check: bool,
    },
    /// ${name:?word}, ${name?word}
    Error {
        name: String,
        word: Option<Word>,
        null_check: bool,
    },
    /// ${name:+word}, ${name+word}
    Alt {
        name: String,
        word: Option<Word>,
        null_check: bool,
    },
    /// ${name%pattern}
    StripShortSuffix(String, Word),
    /// ${name%%pattern}
    StripLongSuffix(String, Word),
    /// ${name#pattern}
    StripShortPrefix(String, Word),
    /// ${name##pattern}
    StripLongPrefix(String, Word),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpecialParam {
    At,
    Star,
    Hash,
    Question,
    Dash,
    Dollar,
    Bang,
    Zero,
}

/// Redirection
#[derive(Debug, Clone, PartialEq)]
pub struct Redirect {
    pub fd: Option<i32>,
    pub kind: RedirectKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RedirectKind {
    /// < word
    Input(Word),
    /// > word
    Output(Word),
    /// >| word
    OutputClobber(Word),
    /// >> word
    Append(Word),
    /// << delimiter (here-document)
    HereDoc(HereDoc),
    /// <& word
    DupInput(Word),
    /// >& word
    DupOutput(Word),
    /// <> word
    ReadWrite(Word),
}

#[derive(Debug, Clone, PartialEq)]
pub struct HereDoc {
    /// The body of the here-document.
    /// Contains Literal parts if the delimiter was quoted (no expansion).
    /// Contains mixed WordParts if the delimiter was unquoted (expansion pending).
    pub body: Vec<WordPart>,
    /// Whether leading tabs should be stripped (<<- form)
    pub strip_tabs: bool,
}
```

- [ ] **Step 2: Write smoke test**

Add at the end of `src/parser/ast.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_literal() {
        let w = Word::literal("hello");
        assert_eq!(w.as_literal(), Some("hello"));
    }

    #[test]
    fn test_word_non_literal() {
        let w = Word {
            parts: vec![
                WordPart::Literal("hello".to_string()),
                WordPart::Parameter(ParamExpr::Simple("x".to_string())),
            ],
        };
        assert_eq!(w.as_literal(), None);
    }

    #[test]
    fn test_simple_command_construction() {
        let cmd = SimpleCommand {
            assignments: vec![],
            words: vec![Word::literal("echo"), Word::literal("hello")],
            redirects: vec![],
        };
        assert_eq!(cmd.words.len(), 2);
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p kish`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add src/parser/ast.rs
git commit -m "feat: define AST types for POSIX shell grammar"
```

---

### Task 3: Token type definitions

**Files:**
- Modify: `src/lexer/token.rs`

- [ ] **Step 1: Define token types**

Write `src/lexer/token.rs`:

```rust
use crate::parser::ast::Word;

/// Source position for error reporting
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Span {
    pub line: usize,
    pub column: usize,
}

/// A token with its source position
#[derive(Debug, Clone, PartialEq)]
pub struct SpannedToken {
    pub token: Token,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// A word (may contain literals, quoted parts, expansions)
    Word(Word),
    /// A number before a redirect operator (e.g., 2>)
    IoNumber(i32),
    /// Newline
    Newline,
    /// End of input
    Eof,

    // --- Operators ---
    /// |
    Pipe,
    /// &&
    AndIf,
    /// ||
    OrIf,
    /// ;
    Semi,
    /// &
    Amp,
    /// ;;
    DSemi,
    /// ;&
    SemiAnd,
    /// <
    Less,
    /// >
    Great,
    /// <<
    DLess,
    /// >>
    DGreat,
    /// <&
    LessAnd,
    /// >&
    GreatAnd,
    /// <>
    LessGreat,
    /// <<-
    DLessDash,
    /// >|
    Clobber,
    /// (
    LParen,
    /// )
    RParen,
}

impl Token {
    /// Check if this token is a word that matches the given string exactly.
    /// Used by the parser to recognize reserved words.
    /// Only unquoted single-literal words can be reserved words.
    pub fn is_reserved_word(&self, keyword: &str) -> bool {
        if let Token::Word(w) = self {
            w.as_literal() == Some(keyword)
        } else {
            false
        }
    }
}
```

- [ ] **Step 2: Write test**

Add at the end of `src/lexer/token.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{Word, WordPart};

    #[test]
    fn test_is_reserved_word_literal() {
        let tok = Token::Word(Word::literal("if"));
        assert!(tok.is_reserved_word("if"));
        assert!(!tok.is_reserved_word("then"));
    }

    #[test]
    fn test_is_reserved_word_quoted_not_reserved() {
        let tok = Token::Word(Word {
            parts: vec![WordPart::SingleQuoted("if".to_string())],
        });
        assert!(!tok.is_reserved_word("if"));
    }

    #[test]
    fn test_is_reserved_word_non_word_token() {
        assert!(!Token::Pipe.is_reserved_word("if"));
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p kish`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add src/lexer/token.rs
git commit -m "feat: define token types for shell lexer"
```

---

### Task 4: Lexer — core structure, operators, whitespace, comments

**Files:**
- Modify: `src/lexer/mod.rs`

- [ ] **Step 1: Write tests for operator tokenization**

Write `src/lexer/mod.rs`:

```rust
pub mod token;

use crate::error::{self, ShellError, ShellErrorKind};
use crate::parser::ast::Word;
use token::{Span, SpannedToken, Token};

pub struct Lexer {
    input: Vec<u8>,
    pos: usize,
    line: usize,
    column: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.as_bytes().to_vec(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    pub fn next_token(&mut self) -> error::Result<SpannedToken> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(tokenize("| ; & ( )"), vec![
            Token::Pipe, Token::Semi, Token::Amp, Token::LParen, Token::RParen,
        ]);
    }

    #[test]
    fn test_multi_char_operators() {
        assert_eq!(tokenize("&& || ;; ;&"), vec![
            Token::AndIf, Token::OrIf, Token::DSemi, Token::SemiAnd,
        ]);
    }

    #[test]
    fn test_redirect_operators() {
        assert_eq!(tokenize("< > >> <& >& <> >|"), vec![
            Token::Less, Token::Great, Token::DGreat,
            Token::LessAnd, Token::GreatAnd, Token::LessGreat, Token::Clobber,
        ]);
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
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kish lexer::tests`
Expected: FAIL (not yet implemented)

- [ ] **Step 3: Implement lexer core**

Replace the `Lexer` implementation in `src/lexer/mod.rs` (keep the tests):

```rust
pub struct Lexer {
    input: Vec<u8>,
    pos: usize,
    line: usize,
    column: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.as_bytes().to_vec(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    pub fn next_token(&mut self) -> error::Result<SpannedToken> {
        self.skip_whitespace_and_comments();

        let span = self.current_span();

        if self.at_end() {
            return Ok(SpannedToken { token: Token::Eof, span });
        }

        let ch = self.current_byte();

        let token = match ch {
            b'\n' => {
                self.advance();
                Token::Newline
            }
            b'|' => self.read_pipe()?,
            b'&' => self.read_amp()?,
            b';' => self.read_semi()?,
            b'(' => {
                self.advance();
                Token::LParen
            }
            b')' => {
                self.advance();
                Token::RParen
            }
            b'<' => self.read_less()?,
            b'>' => self.read_great()?,
            _ => {
                return self.read_word();
            }
        };

        Ok(SpannedToken { token, span })
    }

    // --- Character access helpers ---

    fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn current_byte(&self) -> u8 {
        self.input[self.pos]
    }

    fn peek_byte(&self) -> Option<u8> {
        self.input.get(self.pos + 1).copied()
    }

    fn advance(&mut self) {
        if !self.at_end() {
            if self.current_byte() == b'\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
            self.pos += 1;
        }
    }

    fn current_span(&self) -> Span {
        Span {
            line: self.line,
            column: self.column,
        }
    }

    // --- Whitespace and comments ---

    fn skip_whitespace_and_comments(&mut self) {
        while !self.at_end() {
            let ch = self.current_byte();
            if ch == b' ' || ch == b'\t' {
                self.advance();
            } else if ch == b'#' {
                // Skip comment until newline (but don't consume the newline)
                while !self.at_end() && self.current_byte() != b'\n' {
                    self.advance();
                }
            } else {
                break;
            }
        }
    }

    // --- Operator readers ---

    fn read_pipe(&mut self) -> error::Result<Token> {
        self.advance(); // skip |
        if !self.at_end() && self.current_byte() == b'|' {
            self.advance();
            Ok(Token::OrIf)
        } else {
            Ok(Token::Pipe)
        }
    }

    fn read_amp(&mut self) -> error::Result<Token> {
        self.advance(); // skip &
        if !self.at_end() && self.current_byte() == b'&' {
            self.advance();
            Ok(Token::AndIf)
        } else {
            Ok(Token::Amp)
        }
    }

    fn read_semi(&mut self) -> error::Result<Token> {
        self.advance(); // skip ;
        if !self.at_end() {
            match self.current_byte() {
                b';' => {
                    self.advance();
                    Ok(Token::DSemi)
                }
                b'&' => {
                    self.advance();
                    Ok(Token::SemiAnd)
                }
                _ => Ok(Token::Semi),
            }
        } else {
            Ok(Token::Semi)
        }
    }

    fn read_less(&mut self) -> error::Result<Token> {
        self.advance(); // skip <
        if self.at_end() {
            return Ok(Token::Less);
        }
        match self.current_byte() {
            b'<' => {
                self.advance();
                // Check for <<-
                if !self.at_end() && self.current_byte() == b'-' {
                    self.advance();
                    Ok(Token::DLessDash)
                } else {
                    Ok(Token::DLess)
                }
            }
            b'&' => {
                self.advance();
                Ok(Token::LessAnd)
            }
            b'>' => {
                self.advance();
                Ok(Token::LessGreat)
            }
            _ => Ok(Token::Less),
        }
    }

    fn read_great(&mut self) -> error::Result<Token> {
        self.advance(); // skip >
        if self.at_end() {
            return Ok(Token::Great);
        }
        match self.current_byte() {
            b'>' => {
                self.advance();
                Ok(Token::DGreat)
            }
            b'&' => {
                self.advance();
                Ok(Token::GreatAnd)
            }
            b'|' => {
                self.advance();
                Ok(Token::Clobber)
            }
            _ => Ok(Token::Great),
        }
    }

    // --- Word reader (placeholder — implemented in Task 5) ---

    fn read_word(&mut self) -> error::Result<SpannedToken> {
        let span = self.current_span();
        let mut s = String::new();
        while !self.at_end() && !self.is_meta_or_whitespace(self.current_byte()) {
            s.push(self.current_byte() as char);
            self.advance();
        }
        if s.is_empty() {
            return Err(ShellError::new(
                ShellErrorKind::UnexpectedToken,
                span.line,
                span.column,
                format!("unexpected character: '{}'", self.current_byte() as char),
            ));
        }
        Ok(SpannedToken {
            token: Token::Word(Word::literal(&s)),
            span,
        })
    }

    fn is_meta_or_whitespace(&self, ch: u8) -> bool {
        matches!(
            ch,
            b' ' | b'\t' | b'\n' | b'|' | b'&' | b';' | b'(' | b')' | b'<' | b'>'
        )
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kish lexer::tests`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/lexer/mod.rs
git commit -m "feat: lexer core with operators, whitespace, and comments"
```

---

### Task 5: Lexer — word scanning with quoting

**Files:**
- Modify: `src/lexer/mod.rs`

- [ ] **Step 1: Write tests for quoting**

Add to the `tests` module in `src/lexer/mod.rs`:

```rust
    #[test]
    fn test_unquoted_words() {
        let tokens = tokenize("echo hello world");
        assert_eq!(tokens, vec![
            Token::Word(Word::literal("echo")),
            Token::Word(Word::literal("hello")),
            Token::Word(Word::literal("world")),
        ]);
    }

    #[test]
    fn test_single_quoted_word() {
        let tokens = tokenize("echo 'hello world'");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[1], Token::Word(Word {
            parts: vec![WordPart::SingleQuoted("hello world".to_string())],
        }));
    }

    #[test]
    fn test_double_quoted_word() {
        let tokens = tokenize("echo \"hello world\"");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[1], Token::Word(Word {
            parts: vec![WordPart::DoubleQuoted(vec![
                WordPart::Literal("hello world".to_string()),
            ])],
        }));
    }

    #[test]
    fn test_backslash_escape() {
        let tokens = tokenize("echo hello\\ world");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[1], Token::Word(Word {
            parts: vec![
                WordPart::Literal("hello".to_string()),
                WordPart::Literal(" ".to_string()),
                WordPart::Literal("world".to_string()),
            ],
        }));
    }

    #[test]
    fn test_line_continuation() {
        let tokens = tokenize("echo hel\\\nlo");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[1], Token::Word(Word {
            parts: vec![
                WordPart::Literal("hel".to_string()),
                WordPart::Literal("lo".to_string()),
            ],
        }));
    }

    #[test]
    fn test_dollar_single_quote() {
        let tokens = tokenize("echo $'hello\\nworld'");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[1], Token::Word(Word {
            parts: vec![WordPart::DollarSingleQuoted("hello\nworld".to_string())],
        }));
    }

    #[test]
    fn test_dollar_single_quote_escapes() {
        let tokens = tokenize("$'\\t\\r\\a\\b\\\\\\\"\\''");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0], Token::Word(Word {
            parts: vec![WordPart::DollarSingleQuoted("\t\r\x07\x08\\\"'".to_string())],
        }));
    }

    #[test]
    fn test_mixed_quoting_in_word() {
        // echo he"ll"o → one word with 3 parts
        let tokens = tokenize("he\"ll\"o");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0], Token::Word(Word {
            parts: vec![
                WordPart::Literal("he".to_string()),
                WordPart::DoubleQuoted(vec![WordPart::Literal("ll".to_string())]),
                WordPart::Literal("o".to_string()),
            ],
        }));
    }

    #[test]
    fn test_unterminated_single_quote() {
        let mut lexer = Lexer::new("echo 'hello");
        let _ = lexer.next_token().unwrap(); // echo
        let err = lexer.next_token().unwrap_err();
        assert_eq!(err.kind, ShellErrorKind::UnterminatedSingleQuote);
    }

    #[test]
    fn test_unterminated_double_quote() {
        let mut lexer = Lexer::new("echo \"hello");
        let _ = lexer.next_token().unwrap(); // echo
        let err = lexer.next_token().unwrap_err();
        assert_eq!(err.kind, ShellErrorKind::UnterminatedDoubleQuote);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kish lexer::tests`
Expected: FAIL — the current `read_word` treats everything as unquoted literal.

- [ ] **Step 3: Add WordPart import and implement word scanning with quoting**

Add import at top of `src/lexer/mod.rs`:

```rust
use crate::parser::ast::{Word, WordPart};
```

Replace the `read_word` method and add quoting methods:

```rust
    fn read_word(&mut self) -> error::Result<SpannedToken> {
        let span = self.current_span();
        let parts = self.read_word_parts(false, None)?;

        if parts.is_empty() {
            return Err(ShellError::new(
                ShellErrorKind::UnexpectedToken,
                span.line,
                span.column,
                format!("unexpected character: '{}'", self.current_byte() as char),
            ));
        }

        Ok(SpannedToken {
            token: Token::Word(Word { parts }),
            span,
        })
    }

    /// Read word parts until a delimiter is hit.
    /// - `in_double_quote`: true if we're inside double quotes
    /// - `end_byte`: if Some, stop at this unquoted byte (for ${...} parsing)
    fn read_word_parts(
        &mut self,
        in_double_quote: bool,
        end_byte: Option<u8>,
    ) -> error::Result<Vec<WordPart>> {
        let mut parts = Vec::new();
        let mut literal = String::new();

        loop {
            if self.at_end() {
                break;
            }

            let ch = self.current_byte();

            // Check end_byte delimiter
            if let Some(end) = end_byte {
                if ch == end && !in_double_quote {
                    break;
                }
            }

            // In double quotes, only certain chars are special
            if in_double_quote {
                match ch {
                    b'"' => break, // end of double quote
                    b'\\' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        parts.push(self.read_backslash_in_double_quote()?);
                    }
                    b'$' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        parts.push(self.read_dollar()?);
                    }
                    b'`' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        parts.push(self.read_backtick()?);
                    }
                    _ => {
                        literal.push(ch as char);
                        self.advance();
                    }
                }
                continue;
            }

            // Unquoted context
            if self.is_meta_or_whitespace(ch) {
                break;
            }

            match ch {
                b'\'' => {
                    if !literal.is_empty() {
                        parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                    }
                    parts.push(self.read_single_quote()?);
                }
                b'"' => {
                    if !literal.is_empty() {
                        parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                    }
                    parts.push(self.read_double_quote()?);
                }
                b'\\' => {
                    if !literal.is_empty() {
                        parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                    }
                    parts.push(self.read_backslash()?);
                }
                b'$' => {
                    if !literal.is_empty() {
                        parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                    }
                    parts.push(self.read_dollar()?);
                }
                b'`' => {
                    if !literal.is_empty() {
                        parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                    }
                    parts.push(self.read_backtick()?);
                }
                b'~' if parts.is_empty() && literal.is_empty() => {
                    parts.push(self.read_tilde());
                }
                _ => {
                    literal.push(ch as char);
                    self.advance();
                }
            }
        }

        if !literal.is_empty() {
            parts.push(WordPart::Literal(literal));
        }

        Ok(parts)
    }

    // --- Quoting ---

    fn read_single_quote(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // skip opening '
        let mut s = String::new();
        loop {
            if self.at_end() {
                return Err(ShellError::new(
                    ShellErrorKind::UnterminatedSingleQuote,
                    span.line,
                    span.column,
                    "unterminated single quote",
                ));
            }
            if self.current_byte() == b'\'' {
                self.advance(); // skip closing '
                return Ok(WordPart::SingleQuoted(s));
            }
            s.push(self.current_byte() as char);
            self.advance();
        }
    }

    fn read_double_quote(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // skip opening "
        let parts = self.read_word_parts(true, None)?;
        if self.at_end() || self.current_byte() != b'"' {
            return Err(ShellError::new(
                ShellErrorKind::UnterminatedDoubleQuote,
                span.line,
                span.column,
                "unterminated double quote",
            ));
        }
        self.advance(); // skip closing "
        Ok(WordPart::DoubleQuoted(parts))
    }

    fn read_backslash(&mut self) -> error::Result<WordPart> {
        self.advance(); // skip backslash
        if self.at_end() {
            return Ok(WordPart::Literal("\\".to_string()));
        }
        if self.current_byte() == b'\n' {
            // Line continuation: skip backslash and newline
            self.advance();
            // Return empty — the continuation joins the surrounding text
            return Ok(WordPart::Literal(String::new()));
        }
        let ch = self.current_byte() as char;
        self.advance();
        Ok(WordPart::Literal(ch.to_string()))
    }

    fn read_backslash_in_double_quote(&mut self) -> error::Result<WordPart> {
        self.advance(); // skip backslash
        if self.at_end() {
            return Ok(WordPart::Literal("\\".to_string()));
        }
        let ch = self.current_byte();
        match ch {
            b'$' | b'`' | b'"' | b'\\' | b'\n' => {
                self.advance();
                if ch == b'\n' {
                    Ok(WordPart::Literal(String::new()))
                } else {
                    Ok(WordPart::Literal((ch as char).to_string()))
                }
            }
            _ => {
                // Backslash is literal when not before special chars
                Ok(WordPart::Literal("\\".to_string()))
            }
        }
    }

    fn read_dollar_single_quote(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // skip opening '
        let mut s = String::new();
        loop {
            if self.at_end() {
                return Err(ShellError::new(
                    ShellErrorKind::UnterminatedDollarSingleQuote,
                    span.line,
                    span.column,
                    "unterminated $'...' quote",
                ));
            }
            let ch = self.current_byte();
            if ch == b'\'' {
                self.advance();
                return Ok(WordPart::DollarSingleQuoted(s));
            }
            if ch == b'\\' {
                self.advance();
                if self.at_end() {
                    s.push('\\');
                    continue;
                }
                let esc = self.current_byte();
                self.advance();
                match esc {
                    b'a' => s.push('\x07'),
                    b'b' => s.push('\x08'),
                    b'e' => s.push('\x1b'),
                    b'f' => s.push('\x0c'),
                    b'n' => s.push('\n'),
                    b'r' => s.push('\r'),
                    b't' => s.push('\t'),
                    b'v' => s.push('\x0b'),
                    b'\\' => s.push('\\'),
                    b'\'' => s.push('\''),
                    b'"' => s.push('"'),
                    b'x' => {
                        let val = self.read_hex_digits(2);
                        s.push(val as char);
                    }
                    b'0'..=b'7' => {
                        let mut val = (esc - b'0') as u32;
                        for _ in 0..2 {
                            if self.at_end() {
                                break;
                            }
                            let d = self.current_byte();
                            if d >= b'0' && d <= b'7' {
                                val = val * 8 + (d - b'0') as u32;
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        s.push(char::from(val as u8));
                    }
                    b'c' => {
                        // Control character
                        if !self.at_end() {
                            let c = self.current_byte();
                            self.advance();
                            s.push((c & 0x1f) as char);
                        }
                    }
                    _ => {
                        s.push('\\');
                        s.push(esc as char);
                    }
                }
            } else {
                s.push(ch as char);
                self.advance();
            }
        }
    }

    fn read_hex_digits(&mut self, max: usize) -> u8 {
        let mut val: u8 = 0;
        for _ in 0..max {
            if self.at_end() {
                break;
            }
            let d = self.current_byte();
            let nibble = match d {
                b'0'..=b'9' => d - b'0',
                b'a'..=b'f' => d - b'a' + 10,
                b'A'..=b'F' => d - b'A' + 10,
                _ => break,
            };
            val = val * 16 + nibble;
            self.advance();
        }
        val
    }

    fn read_tilde(&mut self) -> WordPart {
        self.advance(); // skip ~
        let mut user = String::new();
        while !self.at_end() {
            let ch = self.current_byte();
            if ch == b'/' || self.is_meta_or_whitespace(ch) {
                break;
            }
            user.push(ch as char);
            self.advance();
        }
        if user.is_empty() {
            WordPart::Tilde(None)
        } else {
            WordPart::Tilde(Some(user))
        }
    }

    // --- Dollar and backtick (placeholder — implemented in Task 6) ---

    fn read_dollar(&mut self) -> error::Result<WordPart> {
        self.advance(); // skip $
        if self.at_end() {
            return Ok(WordPart::Literal("$".to_string()));
        }
        match self.current_byte() {
            b'\'' => self.read_dollar_single_quote(),
            _ => {
                // Simple $literal for now — expanded in Task 6
                Ok(WordPart::Literal("$".to_string()))
            }
        }
    }

    fn read_backtick(&mut self) -> error::Result<WordPart> {
        // Placeholder — implemented in Task 6
        self.advance();
        Ok(WordPart::Literal("`".to_string()))
    }
```

Also remove the empty `read_word_parts` call — the new `read_word` uses `read_word_parts`.

- [ ] **Step 4: Handle empty literal parts from line continuation**

The `read_backslash` returns an empty `Literal("")` for line continuations. Add a post-processing step in `read_word_parts` after collecting all parts to filter out empty literals:

```rust
    // At end of read_word_parts, before returning:
    let parts: Vec<WordPart> = parts
        .into_iter()
        .filter(|p| {
            if let WordPart::Literal(s) = p {
                !s.is_empty()
            } else {
                true
            }
        })
        .collect();

    Ok(parts)
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p kish lexer::tests`
Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add src/lexer/mod.rs
git commit -m "feat: lexer word scanning with single/double/backslash/dollar-single quoting"
```

---

### Task 6: Lexer — dollar expansions and backtick

**Files:**
- Modify: `src/lexer/mod.rs`

- [ ] **Step 1: Write tests for dollar expansions**

Add to the `tests` module in `src/lexer/mod.rs`:

```rust
    use crate::parser::ast::{ParamExpr, SpecialParam};

    #[test]
    fn test_simple_param() {
        let tokens = tokenize("$name");
        assert_eq!(tokens, vec![Token::Word(Word {
            parts: vec![WordPart::Parameter(ParamExpr::Simple("name".to_string()))],
        })]);
    }

    #[test]
    fn test_param_in_word() {
        let tokens = tokenize("hello${x}world");
        assert_eq!(tokens, vec![Token::Word(Word {
            parts: vec![
                WordPart::Literal("hello".to_string()),
                WordPart::Parameter(ParamExpr::Simple("x".to_string())),
                WordPart::Literal("world".to_string()),
            ],
        })]);
    }

    #[test]
    fn test_positional_param() {
        let tokens = tokenize("$1 ${10}");
        assert_eq!(tokens[0], Token::Word(Word {
            parts: vec![WordPart::Parameter(ParamExpr::Positional(1))],
        }));
        assert_eq!(tokens[1], Token::Word(Word {
            parts: vec![WordPart::Parameter(ParamExpr::Positional(10))],
        }));
    }

    #[test]
    fn test_special_params() {
        let tokens = tokenize("$@ $* $# $? $- $$ $! $0");
        let expected_specials = vec![
            SpecialParam::At, SpecialParam::Star, SpecialParam::Hash,
            SpecialParam::Question, SpecialParam::Dash, SpecialParam::Dollar,
            SpecialParam::Bang, SpecialParam::Zero,
        ];
        for (i, sp) in expected_specials.into_iter().enumerate() {
            assert_eq!(tokens[i], Token::Word(Word {
                parts: vec![WordPart::Parameter(ParamExpr::Special(sp))],
            }));
        }
    }

    #[test]
    fn test_param_default() {
        let tokens = tokenize("${x:-default}");
        assert_eq!(tokens, vec![Token::Word(Word {
            parts: vec![WordPart::Parameter(ParamExpr::Default {
                name: "x".to_string(),
                word: Some(Word::literal("default")),
                null_check: true,
            })],
        })]);
    }

    #[test]
    fn test_param_default_no_colon() {
        let tokens = tokenize("${x-default}");
        assert_eq!(tokens, vec![Token::Word(Word {
            parts: vec![WordPart::Parameter(ParamExpr::Default {
                name: "x".to_string(),
                word: Some(Word::literal("default")),
                null_check: false,
            })],
        })]);
    }

    #[test]
    fn test_param_length() {
        let tokens = tokenize("${#name}");
        assert_eq!(tokens, vec![Token::Word(Word {
            parts: vec![WordPart::Parameter(ParamExpr::Length("name".to_string()))],
        })]);
    }

    #[test]
    fn test_param_strip_suffix() {
        let tokens = tokenize("${name%.txt}");
        assert_eq!(tokens, vec![Token::Word(Word {
            parts: vec![WordPart::Parameter(ParamExpr::StripShortSuffix(
                "name".to_string(),
                Word::literal(".txt"),
            ))],
        })]);
    }

    #[test]
    fn test_param_strip_long_prefix() {
        let tokens = tokenize("${name##*/}");
        assert_eq!(tokens, vec![Token::Word(Word {
            parts: vec![WordPart::Parameter(ParamExpr::StripLongPrefix(
                "name".to_string(),
                Word::literal("*/"),
            ))],
        })]);
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
        assert_eq!(tokens, vec![Token::Word(Word {
            parts: vec![WordPart::ArithSub("1 + 2".to_string())],
        })]);
    }

    #[test]
    fn test_backtick_command_sub() {
        let tokens = tokenize("`echo hello`");
        assert_eq!(tokens.len(), 1);
        if let Token::Word(w) = &tokens[0] {
            assert_eq!(w.parts.len(), 1);
            assert!(matches!(&w.parts[0], WordPart::CommandSub(_)));
        } else {
            panic!("expected word");
        }
    }

    #[test]
    fn test_dollar_in_double_quotes() {
        let tokens = tokenize("\"hello $name\"");
        assert_eq!(tokens, vec![Token::Word(Word {
            parts: vec![WordPart::DoubleQuoted(vec![
                WordPart::Literal("hello ".to_string()),
                WordPart::Parameter(ParamExpr::Simple("name".to_string())),
            ])],
        })]);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kish lexer::tests`
Expected: FAIL — dollar expansion not yet implemented.

- [ ] **Step 3: Implement dollar expansion**

Replace `read_dollar` and add new methods in `src/lexer/mod.rs`:

```rust
    fn read_dollar(&mut self) -> error::Result<WordPart> {
        self.advance(); // skip $
        if self.at_end() {
            return Ok(WordPart::Literal("$".to_string()));
        }
        match self.current_byte() {
            b'\'' => self.read_dollar_single_quote(),
            b'{' => self.read_param_expansion_braced(),
            b'(' => {
                if self.peek_byte() == Some(b'(') {
                    self.read_arith_expansion()
                } else {
                    self.read_command_sub_dollar()
                }
            }
            b'@' => { self.advance(); Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::At))) }
            b'*' => { self.advance(); Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::Star))) }
            b'#' => { self.advance(); Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::Hash))) }
            b'?' => { self.advance(); Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::Question))) }
            b'-' => { self.advance(); Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::Dash))) }
            b'$' => { self.advance(); Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::Dollar))) }
            b'!' => { self.advance(); Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::Bang))) }
            b'0' => { self.advance(); Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::Zero))) }
            ch if ch >= b'1' && ch <= b'9' => {
                let n = (ch - b'0') as usize;
                self.advance();
                Ok(WordPart::Parameter(ParamExpr::Positional(n)))
            }
            ch if is_name_start(ch) => {
                let name = self.read_name();
                Ok(WordPart::Parameter(ParamExpr::Simple(name)))
            }
            _ => Ok(WordPart::Literal("$".to_string())),
        }
    }

    fn read_param_expansion_braced(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // skip {

        if self.at_end() {
            return Err(ShellError::new(
                ShellErrorKind::UnterminatedParamExpansion, span.line, span.column,
                "unterminated parameter expansion",
            ));
        }

        // ${#name} — length
        if self.current_byte() == b'#' {
            if let Some(next) = self.peek_byte() {
                if is_name_start(next) {
                    self.advance(); // skip #
                    let name = self.read_name();
                    self.expect_byte(b'}', span)?;
                    return Ok(WordPart::Parameter(ParamExpr::Length(name)));
                }
                // ${#} — special param # (number of positional params)
                if next == b'}' {
                    self.advance(); // skip #
                    self.advance(); // skip }
                    return Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::Hash)));
                }
            }
        }

        // Read parameter name or special char
        let name = self.read_param_name(span)?;

        if self.at_end() {
            return Err(ShellError::new(
                ShellErrorKind::UnterminatedParamExpansion, span.line, span.column,
                "unterminated parameter expansion",
            ));
        }

        // ${name}
        if self.current_byte() == b'}' {
            self.advance();
            return Ok(WordPart::Parameter(self.classify_param_name(&name)));
        }

        // Check for operator
        let ch = self.current_byte();

        // Pattern removal: %, %%, #, ##
        match ch {
            b'%' => {
                self.advance();
                let long = !self.at_end() && self.current_byte() == b'%';
                if long { self.advance(); }
                let pattern = self.read_word_in_brace(span)?;
                if long {
                    return Ok(WordPart::Parameter(ParamExpr::StripLongSuffix(name, pattern)));
                } else {
                    return Ok(WordPart::Parameter(ParamExpr::StripShortSuffix(name, pattern)));
                }
            }
            b'#' => {
                self.advance();
                let long = !self.at_end() && self.current_byte() == b'#';
                if long { self.advance(); }
                let pattern = self.read_word_in_brace(span)?;
                if long {
                    return Ok(WordPart::Parameter(ParamExpr::StripLongPrefix(name, pattern)));
                } else {
                    return Ok(WordPart::Parameter(ParamExpr::StripShortPrefix(name, pattern)));
                }
            }
            _ => {}
        }

        // Conditional forms: check for optional colon
        let null_check = if ch == b':' {
            self.advance();
            if self.at_end() {
                return Err(ShellError::new(
                    ShellErrorKind::UnterminatedParamExpansion, span.line, span.column,
                    "unterminated parameter expansion",
                ));
            }
            true
        } else {
            false
        };

        let op = self.current_byte();
        self.advance();

        let word = if !self.at_end() && self.current_byte() != b'}' {
            Some(self.read_word_in_brace(span)?)
        } else {
            None
        };

        self.expect_byte(b'}', span)?;

        match op {
            b'-' => Ok(WordPart::Parameter(ParamExpr::Default { name, word, null_check })),
            b'=' => Ok(WordPart::Parameter(ParamExpr::Assign { name, word, null_check })),
            b'?' => Ok(WordPart::Parameter(ParamExpr::Error { name, word, null_check })),
            b'+' => Ok(WordPart::Parameter(ParamExpr::Alt { name, word, null_check })),
            _ => Err(ShellError::new(
                ShellErrorKind::UnterminatedParamExpansion, span.line, span.column,
                format!("unexpected operator '{}' in parameter expansion", op as char),
            )),
        }
    }

    fn read_word_in_brace(&mut self, span: Span) -> error::Result<Word> {
        let parts = self.read_word_parts(false, Some(b'}'))?;
        if parts.is_empty() {
            Ok(Word { parts: vec![WordPart::Literal(String::new())] })
        } else {
            Ok(Word { parts })
        }
    }

    fn read_param_name(&mut self, span: Span) -> error::Result<String> {
        let ch = self.current_byte();
        // Special parameters inside braces
        match ch {
            b'@' | b'*' | b'?' | b'-' | b'$' | b'!' | b'0' => {
                self.advance();
                return Ok((ch as char).to_string());
            }
            b'#' => {
                self.advance();
                return Ok("#".to_string());
            }
            _ => {}
        }
        // Positional: digits
        if ch.is_ascii_digit() {
            let mut num = String::new();
            while !self.at_end() && self.current_byte().is_ascii_digit() {
                num.push(self.current_byte() as char);
                self.advance();
            }
            return Ok(num);
        }
        // Name
        if is_name_start(ch) {
            return Ok(self.read_name());
        }
        Err(ShellError::new(
            ShellErrorKind::UnterminatedParamExpansion, span.line, span.column,
            format!("invalid parameter name character: '{}'", ch as char),
        ))
    }

    fn classify_param_name(&self, name: &str) -> ParamExpr {
        match name {
            "@" => ParamExpr::Special(SpecialParam::At),
            "*" => ParamExpr::Special(SpecialParam::Star),
            "#" => ParamExpr::Special(SpecialParam::Hash),
            "?" => ParamExpr::Special(SpecialParam::Question),
            "-" => ParamExpr::Special(SpecialParam::Dash),
            "$" => ParamExpr::Special(SpecialParam::Dollar),
            "!" => ParamExpr::Special(SpecialParam::Bang),
            "0" => ParamExpr::Special(SpecialParam::Zero),
            _ => {
                if let Ok(n) = name.parse::<usize>() {
                    if n > 0 {
                        return ParamExpr::Positional(n);
                    }
                }
                ParamExpr::Simple(name.to_string())
            }
        }
    }

    fn read_command_sub_dollar(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // skip (

        // Collect content between balanced parens
        let content = self.read_balanced_parens(span)?;

        // Parse the content as a shell program
        let mut sub_parser = crate::parser::Parser::new(&content);
        let program = sub_parser.parse_program()?;

        Ok(WordPart::CommandSub(program))
    }

    fn read_arith_expansion(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // skip first (
        self.advance(); // skip second (

        // Read until matching ))
        let mut depth = 1;
        let mut expr = String::new();
        while !self.at_end() {
            let ch = self.current_byte();
            if ch == b'(' {
                depth += 1;
                expr.push('(');
                self.advance();
            } else if ch == b')' {
                if depth == 1 {
                    // Check for ))
                    if self.peek_byte() == Some(b')') {
                        self.advance(); // skip first )
                        self.advance(); // skip second )
                        return Ok(WordPart::ArithSub(expr.trim().to_string()));
                    }
                }
                depth -= 1;
                expr.push(')');
                self.advance();
            } else {
                expr.push(ch as char);
                self.advance();
            }
        }
        Err(ShellError::new(
            ShellErrorKind::UnterminatedArithSub, span.line, span.column,
            "unterminated arithmetic expansion",
        ))
    }

    fn read_balanced_parens(&mut self, span: Span) -> error::Result<String> {
        let mut depth = 1;
        let mut content = String::new();
        while !self.at_end() {
            let ch = self.current_byte();
            match ch {
                b'(' => {
                    depth += 1;
                    content.push('(');
                    self.advance();
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        self.advance(); // skip closing )
                        return Ok(content);
                    }
                    content.push(')');
                    self.advance();
                }
                b'\'' => {
                    content.push('\'');
                    self.advance();
                    while !self.at_end() && self.current_byte() != b'\'' {
                        content.push(self.current_byte() as char);
                        self.advance();
                    }
                    if !self.at_end() {
                        content.push('\'');
                        self.advance();
                    }
                }
                b'"' => {
                    content.push('"');
                    self.advance();
                    while !self.at_end() && self.current_byte() != b'"' {
                        if self.current_byte() == b'\\' {
                            content.push('\\');
                            self.advance();
                            if !self.at_end() {
                                content.push(self.current_byte() as char);
                                self.advance();
                            }
                        } else {
                            content.push(self.current_byte() as char);
                            self.advance();
                        }
                    }
                    if !self.at_end() {
                        content.push('"');
                        self.advance();
                    }
                }
                _ => {
                    content.push(ch as char);
                    self.advance();
                }
            }
        }
        Err(ShellError::new(
            ShellErrorKind::UnterminatedCommandSub, span.line, span.column,
            "unterminated command substitution",
        ))
    }

    fn read_backtick(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // skip opening `
        let mut content = String::new();
        while !self.at_end() {
            let ch = self.current_byte();
            if ch == b'`' {
                self.advance();
                let mut sub_parser = crate::parser::Parser::new(&content);
                let program = sub_parser.parse_program()?;
                return Ok(WordPart::CommandSub(program));
            }
            if ch == b'\\' {
                self.advance();
                if !self.at_end() {
                    let next = self.current_byte();
                    match next {
                        b'$' | b'`' | b'\\' => {
                            content.push(next as char);
                            self.advance();
                        }
                        _ => {
                            content.push('\\');
                            content.push(next as char);
                            self.advance();
                        }
                    }
                }
            } else {
                content.push(ch as char);
                self.advance();
            }
        }
        Err(ShellError::new(
            ShellErrorKind::UnterminatedBacktick, span.line, span.column,
            "unterminated backtick command substitution",
        ))
    }

    // --- Helpers ---

    fn read_name(&mut self) -> String {
        let mut name = String::new();
        while !self.at_end() && is_name_char(self.current_byte()) {
            name.push(self.current_byte() as char);
            self.advance();
        }
        name
    }

    fn expect_byte(&mut self, expected: u8, span: Span) -> error::Result<()> {
        if self.at_end() || self.current_byte() != expected {
            return Err(ShellError::new(
                ShellErrorKind::UnterminatedParamExpansion, span.line, span.column,
                format!("expected '{}' in expansion", expected as char),
            ));
        }
        self.advance();
        Ok(())
    }
```

Also add outside the `impl Lexer` block:

```rust
fn is_name_start(ch: u8) -> bool {
    ch.is_ascii_alphabetic() || ch == b'_'
}

fn is_name_char(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_'
}
```

Also add the `use` for `SpecialParam` and `ParamExpr`:

```rust
use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};
```

- [ ] **Step 4: Add minimal Parser stub for command substitution**

The lexer's `read_command_sub_dollar` and `read_backtick` need `crate::parser::Parser`. Add a minimal stub in `src/parser/mod.rs`:

```rust
pub mod ast;

use crate::error;
use ast::Program;

pub struct Parser {
    input: String,
}

impl Parser {
    pub fn new(input: &str) -> Self {
        Self { input: input.to_string() }
    }

    pub fn parse_program(&mut self) -> error::Result<Program> {
        // Stub — returns empty program for now
        // Full implementation in Tasks 7-11
        Ok(Program { commands: vec![] })
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p kish lexer::tests`
Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add src/lexer/mod.rs src/parser/mod.rs
git commit -m "feat: lexer dollar expansions, parameter expansion, command substitution, arithmetic"
```

---

### Task 7: Lexer — IO_NUMBER detection

**Files:**
- Modify: `src/lexer/mod.rs`

- [ ] **Step 1: Write tests**

Add to the `tests` module:

```rust
    #[test]
    fn test_io_number_redirect() {
        let tokens = tokenize("2>/dev/null");
        assert_eq!(tokens, vec![
            Token::IoNumber(2),
            Token::Great,
            Token::Word(Word::literal("/dev/null")),
        ]);
    }

    #[test]
    fn test_io_number_input() {
        let tokens = tokenize("0<input.txt");
        assert_eq!(tokens, vec![
            Token::IoNumber(0),
            Token::Less,
            Token::Word(Word::literal("input.txt")),
        ]);
    }

    #[test]
    fn test_digits_not_followed_by_redirect() {
        let tokens = tokenize("123 abc");
        assert_eq!(tokens, vec![
            Token::Word(Word::literal("123")),
            Token::Word(Word::literal("abc")),
        ]);
    }

    #[test]
    fn test_fd_dup() {
        let tokens = tokenize("2>&1");
        assert_eq!(tokens, vec![
            Token::IoNumber(2),
            Token::GreatAnd,
            Token::Word(Word::literal("1")),
        ]);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kish lexer::tests::test_io_number`
Expected: FAIL — digits are currently tokenized as words.

- [ ] **Step 3: Implement IO_NUMBER detection**

IO_NUMBER is recognized when a token is all digits and is immediately followed by `<` or `>` (no whitespace). Modify `next_token` in the default match arm:

```rust
            _ => {
                // Check for IO_NUMBER: digits followed immediately by < or >
                if ch.is_ascii_digit() {
                    if let Some(io_num) = self.try_read_io_number() {
                        return Ok(SpannedToken { token: io_num, span });
                    }
                }
                return self.read_word();
            }
```

Add the method:

```rust
    /// Try to read an IO_NUMBER. Returns None if the digits are not followed
    /// by a redirect operator (in which case, the position is restored).
    fn try_read_io_number(&mut self) -> Option<Token> {
        let saved_pos = self.pos;
        let saved_line = self.line;
        let saved_col = self.column;

        let mut num_str = String::new();
        while !self.at_end() && self.current_byte().is_ascii_digit() {
            num_str.push(self.current_byte() as char);
            self.advance();
        }

        if !self.at_end() && (self.current_byte() == b'<' || self.current_byte() == b'>') {
            if let Ok(n) = num_str.parse::<i32>() {
                return Some(Token::IoNumber(n));
            }
        }

        // Not an IO_NUMBER — restore position
        self.pos = saved_pos;
        self.line = saved_line;
        self.column = saved_col;
        None
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kish lexer::tests`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/lexer/mod.rs
git commit -m "feat: lexer IO_NUMBER detection for redirections"
```

---

### Task 8: Parser — core structure and simple commands

**Files:**
- Modify: `src/parser/mod.rs`

- [ ] **Step 1: Write tests**

Replace `src/parser/mod.rs`:

```rust
pub mod ast;

use crate::error::{self, ShellError, ShellErrorKind};
use crate::lexer::token::{Span, SpannedToken, Token};
use crate::lexer::Lexer;
use ast::*;

pub struct Parser {
    lexer: Lexer,
    current: SpannedToken,
}

impl Parser {
    pub fn new(input: &str) -> Self {
        let mut lexer = Lexer::new(input);
        let current = lexer.next_token().unwrap_or(SpannedToken {
            token: Token::Eof,
            span: Span::default(),
        });
        Self { lexer, current }
    }

    pub fn parse_program(&mut self) -> error::Result<Program> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Program {
        let mut parser = Parser::new(input);
        parser.parse_program().unwrap()
    }

    fn parse_first_simple(input: &str) -> SimpleCommand {
        let prog = parse(input);
        let cmd = &prog.commands[0].items[0].0.first.commands[0];
        match cmd {
            Command::Simple(sc) => sc.clone(),
            _ => panic!("expected simple command"),
        }
    }

    #[test]
    fn test_empty_program() {
        let prog = parse("");
        assert!(prog.commands.is_empty());
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
        assert_eq!(sc.assignments[0].value.as_ref().unwrap().as_literal(), Some("bar"));
    }

    #[test]
    fn test_assignment_with_command() {
        let sc = parse_first_simple("FOO=bar echo hello");
        assert_eq!(sc.assignments.len(), 1);
        assert_eq!(sc.assignments[0].name, "FOO");
        assert_eq!(sc.words.len(), 2);
        assert_eq!(sc.words[0].as_literal(), Some("echo"));
    }

    #[test]
    fn test_assignment_empty_value() {
        let sc = parse_first_simple("FOO=");
        assert_eq!(sc.assignments.len(), 1);
        assert_eq!(sc.assignments[0].name, "FOO");
        assert_eq!(sc.assignments[0].value, None);
    }

    #[test]
    fn test_multiple_newlines() {
        let prog = parse("\n\necho hello\n\n");
        assert_eq!(prog.commands.len(), 1);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kish parser::tests`
Expected: FAIL — parse_program not implemented.

- [ ] **Step 3: Implement parser core**

Add methods to `Parser` in `src/parser/mod.rs`:

```rust
impl Parser {
    pub fn new(input: &str) -> Self {
        let mut lexer = Lexer::new(input);
        let current = lexer.next_token().unwrap_or(SpannedToken {
            token: Token::Eof,
            span: Span::default(),
        });
        Self { lexer, current }
    }

    // --- Token access ---

    fn current_token(&self) -> &Token {
        &self.current.token
    }

    fn current_span(&self) -> Span {
        self.current.span
    }

    fn advance(&mut self) -> error::Result<()> {
        self.current = self.lexer.next_token()?;
        Ok(())
    }

    fn eat(&mut self, expected: &Token) -> error::Result<bool> {
        if self.current_token() == expected {
            self.advance()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn expect_reserved(&mut self, keyword: &str) -> error::Result<()> {
        if self.current_token().is_reserved_word(keyword) {
            self.advance()?;
            Ok(())
        } else {
            Err(ShellError::new(
                ShellErrorKind::UnexpectedToken,
                self.current_span().line,
                self.current_span().column,
                format!("expected '{}', got {:?}", keyword, self.current_token()),
            ))
        }
    }

    fn skip_newlines(&mut self) -> error::Result<()> {
        while *self.current_token() == Token::Newline {
            self.advance()?;
        }
        Ok(())
    }

    fn is_at_end(&self) -> bool {
        *self.current_token() == Token::Eof
    }

    /// Check if current token is a word that matches a reserved word
    fn is_reserved(&self, keyword: &str) -> bool {
        self.current_token().is_reserved_word(keyword)
    }

    // --- Grammar productions ---

    pub fn parse_program(&mut self) -> error::Result<Program> {
        self.skip_newlines()?;
        let mut commands = Vec::new();
        while !self.is_at_end() {
            commands.push(self.parse_complete_command()?);
            self.skip_newlines()?;
        }
        Ok(Program { commands })
    }

    fn parse_complete_command(&mut self) -> error::Result<CompleteCommand> {
        let mut items = Vec::new();
        let first = self.parse_and_or()?;

        // Check for separator
        let sep = self.parse_separator_op()?;
        items.push((first, sep));

        // Continue if more AND-OR lists follow
        while sep.is_some() && !self.is_at_end() {
            self.skip_newlines()?;
            if self.is_at_end() || self.is_complete_command_end() {
                break;
            }
            let aol = self.parse_and_or()?;
            let sep = self.parse_separator_op()?;
            items.push((aol, sep));
        }

        Ok(CompleteCommand { items })
    }

    fn parse_separator_op(&mut self) -> error::Result<Option<SeparatorOp>> {
        match self.current_token() {
            Token::Semi => {
                self.advance()?;
                Ok(Some(SeparatorOp::Semi))
            }
            Token::Amp => {
                self.advance()?;
                Ok(Some(SeparatorOp::Amp))
            }
            Token::Newline => {
                self.advance()?;
                Ok(Some(SeparatorOp::Semi))
            }
            _ => Ok(None),
        }
    }

    fn is_complete_command_end(&self) -> bool {
        matches!(
            self.current_token(),
            Token::Eof | Token::RParen
        ) || self.is_reserved("}")
            || self.is_reserved("fi")
            || self.is_reserved("done")
            || self.is_reserved("esac")
            || self.is_reserved("then")
            || self.is_reserved("else")
            || self.is_reserved("elif")
            || self.is_reserved("do")
    }

    fn parse_and_or(&mut self) -> error::Result<AndOrList> {
        let first = self.parse_pipeline()?;
        let mut rest = Vec::new();

        loop {
            let op = match self.current_token() {
                Token::AndIf => AndOrOp::And,
                Token::OrIf => AndOrOp::Or,
                _ => break,
            };
            self.advance()?;
            self.skip_newlines()?;
            let pipeline = self.parse_pipeline()?;
            rest.push((op, pipeline));
        }

        Ok(AndOrList { first, rest })
    }

    fn parse_pipeline(&mut self) -> error::Result<Pipeline> {
        let negated = if self.is_reserved("!") {
            self.advance()?;
            true
        } else {
            false
        };

        let first = self.parse_command()?;
        let mut commands = vec![first];

        while *self.current_token() == Token::Pipe {
            self.advance()?;
            self.skip_newlines()?;
            commands.push(self.parse_command()?);
        }

        Ok(Pipeline { negated, commands })
    }

    fn parse_command(&mut self) -> error::Result<Command> {
        // Check for compound commands or function definitions
        if self.is_compound_command_start() {
            let compound = self.parse_compound_command()?;
            let redirects = self.parse_redirect_list()?;
            return Ok(Command::Compound(compound, redirects));
        }

        // Try function definition: NAME ( )
        if let Some(func) = self.try_parse_function_def()? {
            return Ok(Command::FunctionDef(func));
        }

        // Simple command
        let sc = self.parse_simple_command()?;
        Ok(Command::Simple(sc))
    }

    fn parse_simple_command(&mut self) -> error::Result<SimpleCommand> {
        let mut assignments = Vec::new();
        let mut words = Vec::new();
        let mut redirects = Vec::new();
        let mut found_command_name = false;

        loop {
            // Try redirect first
            if let Some(redir) = self.try_parse_redirect()? {
                redirects.push(redir);
                continue;
            }

            match self.current_token().clone() {
                Token::Word(w) => {
                    if !found_command_name {
                        // Before command name: check for assignment
                        if let Some(assignment) = self.try_parse_assignment(&w) {
                            assignments.push(assignment);
                            self.advance()?;
                            continue;
                        }
                    }
                    found_command_name = true;
                    words.push(w.clone());
                    self.advance()?;
                }
                _ => break,
            }
        }

        Ok(SimpleCommand { assignments, words, redirects })
    }

    fn try_parse_assignment(&self, word: &Word) -> Option<Assignment> {
        // Check if word is NAME=VALUE form
        if word.parts.len() >= 1 {
            if let WordPart::Literal(s) = &word.parts[0] {
                if let Some(eq_pos) = s.find('=') {
                    let name = &s[..eq_pos];
                    if !name.is_empty() && is_valid_name(name) {
                        let value_str = &s[eq_pos + 1..];
                        let mut value_parts = Vec::new();
                        if !value_str.is_empty() {
                            value_parts.push(WordPart::Literal(value_str.to_string()));
                        }
                        // Append remaining parts
                        for part in &word.parts[1..] {
                            value_parts.push(part.clone());
                        }
                        let value = if value_parts.is_empty() {
                            None
                        } else {
                            Some(Word { parts: value_parts })
                        };
                        return Some(Assignment {
                            name: name.to_string(),
                            value,
                        });
                    }
                }
            }
        }
        None
    }

    // --- Stubs for compound commands, redirects, functions (Tasks 9-11) ---

    fn is_compound_command_start(&self) -> bool {
        self.is_reserved("if")
            || self.is_reserved("for")
            || self.is_reserved("while")
            || self.is_reserved("until")
            || self.is_reserved("case")
            || self.is_reserved("{")
            || *self.current_token() == Token::LParen
    }

    fn parse_compound_command(&mut self) -> error::Result<CompoundCommand> {
        // Implemented in Tasks 10-11
        Err(ShellError::new(
            ShellErrorKind::UnexpectedToken,
            self.current_span().line,
            self.current_span().column,
            "compound commands not yet implemented",
        ))
    }

    fn try_parse_function_def(&mut self) -> error::Result<Option<FunctionDef>> {
        // Implemented in Task 11
        Ok(None)
    }

    fn try_parse_redirect(&mut self) -> error::Result<Option<Redirect>> {
        // Implemented in Task 9
        Ok(None)
    }

    fn parse_redirect_list(&mut self) -> error::Result<Vec<Redirect>> {
        // Implemented in Task 9
        Ok(vec![])
    }
}

fn is_valid_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kish parser::tests`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/parser/mod.rs
git commit -m "feat: parser core with simple command, pipeline, AND-OR list parsing"
```

---

### Task 9: Parser — redirections

**Files:**
- Modify: `src/parser/mod.rs`

- [ ] **Step 1: Write tests**

Add to the `tests` module:

```rust
    #[test]
    fn test_output_redirect() {
        let sc = parse_first_simple("echo hello > out.txt");
        assert_eq!(sc.words.len(), 2);
        assert_eq!(sc.redirects.len(), 1);
        assert_eq!(sc.redirects[0].fd, None);
        assert!(matches!(&sc.redirects[0].kind, RedirectKind::Output(w) if w.as_literal() == Some("out.txt")));
    }

    #[test]
    fn test_input_redirect() {
        let sc = parse_first_simple("cat < input.txt");
        assert_eq!(sc.redirects.len(), 1);
        assert_eq!(sc.redirects[0].fd, None);
        assert!(matches!(&sc.redirects[0].kind, RedirectKind::Input(w) if w.as_literal() == Some("input.txt")));
    }

    #[test]
    fn test_append_redirect() {
        let sc = parse_first_simple("echo hello >> log.txt");
        assert!(matches!(&sc.redirects[0].kind, RedirectKind::Append(w) if w.as_literal() == Some("log.txt")));
    }

    #[test]
    fn test_fd_redirect() {
        let sc = parse_first_simple("cmd 2>/dev/null");
        assert_eq!(sc.redirects.len(), 1);
        assert_eq!(sc.redirects[0].fd, Some(2));
        assert!(matches!(&sc.redirects[0].kind, RedirectKind::Output(w) if w.as_literal() == Some("/dev/null")));
    }

    #[test]
    fn test_dup_output() {
        let sc = parse_first_simple("cmd 2>&1");
        assert_eq!(sc.redirects[0].fd, Some(2));
        assert!(matches!(&sc.redirects[0].kind, RedirectKind::DupOutput(w) if w.as_literal() == Some("1")));
    }

    #[test]
    fn test_heredoc_redirect() {
        let sc = parse_first_simple("cat <<EOF");
        assert_eq!(sc.redirects.len(), 1);
        assert!(matches!(&sc.redirects[0].kind, RedirectKind::HereDoc(_)));
    }

    #[test]
    fn test_clobber_redirect() {
        let sc = parse_first_simple("echo hello >| out.txt");
        assert!(matches!(&sc.redirects[0].kind, RedirectKind::OutputClobber(w) if w.as_literal() == Some("out.txt")));
    }

    #[test]
    fn test_read_write_redirect() {
        let sc = parse_first_simple("cmd 3<>file");
        assert_eq!(sc.redirects[0].fd, Some(3));
        assert!(matches!(&sc.redirects[0].kind, RedirectKind::ReadWrite(w) if w.as_literal() == Some("file")));
    }

    #[test]
    fn test_multiple_redirects() {
        let sc = parse_first_simple("cmd < in > out 2>&1");
        assert_eq!(sc.redirects.len(), 3);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kish parser::tests::test_output_redirect`
Expected: FAIL — redirects not parsed yet.

- [ ] **Step 3: Implement redirect parsing**

Replace the `try_parse_redirect` and `parse_redirect_list` stubs:

```rust
    fn try_parse_redirect(&mut self) -> error::Result<Option<Redirect>> {
        let fd = match self.current_token() {
            Token::IoNumber(n) => {
                let n = *n;
                self.advance()?;
                Some(n)
            }
            _ => None,
        };

        let kind = match self.current_token().clone() {
            Token::Less => {
                self.advance()?;
                let target = self.expect_word("redirect target")?;
                RedirectKind::Input(target)
            }
            Token::Great => {
                self.advance()?;
                let target = self.expect_word("redirect target")?;
                RedirectKind::Output(target)
            }
            Token::DGreat => {
                self.advance()?;
                let target = self.expect_word("redirect target")?;
                RedirectKind::Append(target)
            }
            Token::Clobber => {
                self.advance()?;
                let target = self.expect_word("redirect target")?;
                RedirectKind::OutputClobber(target)
            }
            Token::LessAnd => {
                self.advance()?;
                let target = self.expect_word("dup target")?;
                RedirectKind::DupInput(target)
            }
            Token::GreatAnd => {
                self.advance()?;
                let target = self.expect_word("dup target")?;
                RedirectKind::DupOutput(target)
            }
            Token::LessGreat => {
                self.advance()?;
                let target = self.expect_word("redirect target")?;
                RedirectKind::ReadWrite(target)
            }
            Token::DLess => {
                self.advance()?;
                let delimiter = self.expect_word("here-document delimiter")?;
                RedirectKind::HereDoc(HereDoc {
                    body: vec![],
                    strip_tabs: false,
                })
            }
            Token::DLessDash => {
                self.advance()?;
                let delimiter = self.expect_word("here-document delimiter")?;
                RedirectKind::HereDoc(HereDoc {
                    body: vec![],
                    strip_tabs: true,
                })
            }
            _ => {
                // Not a redirect — if we consumed an IoNumber, that's an error
                if fd.is_some() {
                    return Err(ShellError::new(
                        ShellErrorKind::InvalidRedirect,
                        self.current_span().line,
                        self.current_span().column,
                        "expected redirect operator after file descriptor number",
                    ));
                }
                return Ok(None);
            }
        };

        Ok(Some(Redirect { fd, kind }))
    }

    fn parse_redirect_list(&mut self) -> error::Result<Vec<Redirect>> {
        let mut redirects = Vec::new();
        while let Some(redir) = self.try_parse_redirect()? {
            redirects.push(redir);
        }
        Ok(redirects)
    }

    fn expect_word(&mut self, context: &str) -> error::Result<Word> {
        match self.current_token().clone() {
            Token::Word(w) => {
                self.advance()?;
                Ok(w)
            }
            _ => Err(ShellError::new(
                ShellErrorKind::UnexpectedToken,
                self.current_span().line,
                self.current_span().column,
                format!("expected word for {}, got {:?}", context, self.current_token()),
            )),
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kish parser::tests`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/parser/mod.rs
git commit -m "feat: parser redirect handling for all POSIX redirect forms"
```

---

### Task 10: Parser — compound commands (if, for, while, until)

**Files:**
- Modify: `src/parser/mod.rs`

- [ ] **Step 1: Write tests**

Add to the `tests` module:

```rust
    fn parse_first_compound(input: &str) -> CompoundCommandKind {
        let prog = parse(input);
        let cmd = &prog.commands[0].items[0].0.first.commands[0];
        match cmd {
            Command::Compound(cc, _) => cc.kind.clone(),
            _ => panic!("expected compound command"),
        }
    }

    #[test]
    fn test_if_then_fi() {
        let kind = parse_first_compound("if true; then echo yes; fi");
        match kind {
            CompoundCommandKind::If { condition, then_part, elif_parts, else_part } => {
                assert!(!condition.is_empty());
                assert!(!then_part.is_empty());
                assert!(elif_parts.is_empty());
                assert!(else_part.is_none());
            }
            _ => panic!("expected if"),
        }
    }

    #[test]
    fn test_if_else() {
        let kind = parse_first_compound("if true; then echo yes; else echo no; fi");
        match kind {
            CompoundCommandKind::If { else_part, .. } => {
                assert!(else_part.is_some());
            }
            _ => panic!("expected if"),
        }
    }

    #[test]
    fn test_if_elif() {
        let kind = parse_first_compound("if false; then echo 1; elif true; then echo 2; else echo 3; fi");
        match kind {
            CompoundCommandKind::If { elif_parts, else_part, .. } => {
                assert_eq!(elif_parts.len(), 1);
                assert!(else_part.is_some());
            }
            _ => panic!("expected if"),
        }
    }

    #[test]
    fn test_for_loop_with_words() {
        let kind = parse_first_compound("for i in a b c; do echo $i; done");
        match kind {
            CompoundCommandKind::For { var, words, body } => {
                assert_eq!(var, "i");
                let words = words.unwrap();
                assert_eq!(words.len(), 3);
                assert!(!body.is_empty());
            }
            _ => panic!("expected for"),
        }
    }

    #[test]
    fn test_for_loop_without_in() {
        let kind = parse_first_compound("for i; do echo $i; done");
        match kind {
            CompoundCommandKind::For { var, words, .. } => {
                assert_eq!(var, "i");
                assert!(words.is_none());
            }
            _ => panic!("expected for"),
        }
    }

    #[test]
    fn test_for_loop_with_do_on_newline() {
        let kind = parse_first_compound("for i in a b c\ndo\necho $i\ndone");
        match kind {
            CompoundCommandKind::For { var, words, .. } => {
                assert_eq!(var, "i");
                assert!(words.is_some());
            }
            _ => panic!("expected for"),
        }
    }

    #[test]
    fn test_while_loop() {
        let kind = parse_first_compound("while true; do echo loop; done");
        assert!(matches!(kind, CompoundCommandKind::While { .. }));
    }

    #[test]
    fn test_until_loop() {
        let kind = parse_first_compound("until false; do echo loop; done");
        assert!(matches!(kind, CompoundCommandKind::Until { .. }));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kish parser::tests::test_if`
Expected: FAIL — compound commands not implemented.

- [ ] **Step 3: Implement compound command parsing**

Replace the `parse_compound_command` stub:

```rust
    fn parse_compound_command(&mut self) -> error::Result<CompoundCommand> {
        let kind = if self.is_reserved("if") {
            self.parse_if_clause()?
        } else if self.is_reserved("for") {
            self.parse_for_clause()?
        } else if self.is_reserved("while") {
            self.parse_while_clause()?
        } else if self.is_reserved("until") {
            self.parse_until_clause()?
        } else if self.is_reserved("case") {
            self.parse_case_clause()?
        } else if self.is_reserved("{") {
            self.parse_brace_group()?
        } else if *self.current_token() == Token::LParen {
            self.parse_subshell()?
        } else {
            return Err(ShellError::new(
                ShellErrorKind::UnexpectedToken,
                self.current_span().line,
                self.current_span().column,
                format!("expected compound command, got {:?}", self.current_token()),
            ));
        };
        Ok(CompoundCommand { kind })
    }

    fn parse_compound_list(&mut self) -> error::Result<Vec<CompleteCommand>> {
        self.skip_newlines()?;
        let mut commands = Vec::new();
        while !self.is_at_end() && !self.is_complete_command_end() {
            commands.push(self.parse_complete_command()?);
            self.skip_newlines()?;
        }
        Ok(commands)
    }

    fn parse_if_clause(&mut self) -> error::Result<CompoundCommandKind> {
        self.expect_reserved("if")?;
        let condition = self.parse_compound_list()?;
        self.expect_reserved("then")?;
        let then_part = self.parse_compound_list()?;

        let mut elif_parts = Vec::new();
        while self.is_reserved("elif") {
            self.advance()?;
            let elif_cond = self.parse_compound_list()?;
            self.expect_reserved("then")?;
            let elif_body = self.parse_compound_list()?;
            elif_parts.push((elif_cond, elif_body));
        }

        let else_part = if self.is_reserved("else") {
            self.advance()?;
            Some(self.parse_compound_list()?)
        } else {
            None
        };

        self.expect_reserved("fi")?;

        Ok(CompoundCommandKind::If {
            condition,
            then_part,
            elif_parts,
            else_part,
        })
    }

    fn parse_for_clause(&mut self) -> error::Result<CompoundCommandKind> {
        self.expect_reserved("for")?;
        let var_word = self.expect_word("for variable")?;
        let var = var_word.as_literal()
            .ok_or_else(|| ShellError::new(
                ShellErrorKind::UnexpectedToken,
                self.current_span().line,
                self.current_span().column,
                "for variable must be a name",
            ))?
            .to_string();

        // Check for 'in' wordlist
        // Between 'for name' and 'in'/'do', newlines and ; are allowed
        self.skip_newlines()?;

        let words = if self.is_reserved("in") {
            self.advance()?;
            let mut words = Vec::new();
            while !self.is_at_end() {
                // Stop at ;, newline, or do
                match self.current_token() {
                    Token::Semi | Token::Newline => {
                        self.advance()?;
                        break;
                    }
                    _ if self.is_reserved("do") => break,
                    Token::Word(w) => {
                        words.push(w.clone());
                        self.advance()?;
                    }
                    _ => break,
                }
            }
            Some(words)
        } else {
            // ; or newline before do (no 'in' clause)
            match self.current_token() {
                Token::Semi => { self.advance()?; }
                _ => {}
            }
            None
        };

        self.skip_newlines()?;
        let body = self.parse_do_group()?;

        Ok(CompoundCommandKind::For { var, words, body })
    }

    fn parse_do_group(&mut self) -> error::Result<Vec<CompleteCommand>> {
        self.expect_reserved("do")?;
        let body = self.parse_compound_list()?;
        self.expect_reserved("done")?;
        Ok(body)
    }

    fn parse_while_clause(&mut self) -> error::Result<CompoundCommandKind> {
        self.expect_reserved("while")?;
        let condition = self.parse_compound_list()?;
        let body = self.parse_do_group()?;
        Ok(CompoundCommandKind::While { condition, body })
    }

    fn parse_until_clause(&mut self) -> error::Result<CompoundCommandKind> {
        self.expect_reserved("until")?;
        let condition = self.parse_compound_list()?;
        let body = self.parse_do_group()?;
        Ok(CompoundCommandKind::Until { condition, body })
    }

    // Stubs for case, brace, subshell — Task 11
    fn parse_case_clause(&mut self) -> error::Result<CompoundCommandKind> {
        Err(ShellError::new(
            ShellErrorKind::UnexpectedToken,
            self.current_span().line, self.current_span().column,
            "case not yet implemented",
        ))
    }

    fn parse_brace_group(&mut self) -> error::Result<CompoundCommandKind> {
        Err(ShellError::new(
            ShellErrorKind::UnexpectedToken,
            self.current_span().line, self.current_span().column,
            "brace group not yet implemented",
        ))
    }

    fn parse_subshell(&mut self) -> error::Result<CompoundCommandKind> {
        Err(ShellError::new(
            ShellErrorKind::UnexpectedToken,
            self.current_span().line, self.current_span().column,
            "subshell not yet implemented",
        ))
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kish parser::tests`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/parser/mod.rs
git commit -m "feat: parser if/elif/else/fi, for, while, until compound commands"
```

---

### Task 11: Parser — case, brace groups, subshells, function definitions

**Files:**
- Modify: `src/parser/mod.rs`

- [ ] **Step 1: Write tests**

Add to the `tests` module:

```rust
    #[test]
    fn test_case_basic() {
        let kind = parse_first_compound("case $x in\na) echo a;;\nb) echo b;;\nesac");
        match kind {
            CompoundCommandKind::Case { items, .. } => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].terminator, CaseTerminator::Break);
            }
            _ => panic!("expected case"),
        }
    }

    #[test]
    fn test_case_fallthrough() {
        let kind = parse_first_compound("case $x in\na) echo a;&\nb) echo b;;\nesac");
        match kind {
            CompoundCommandKind::Case { items, .. } => {
                assert_eq!(items[0].terminator, CaseTerminator::FallThrough);
                assert_eq!(items[1].terminator, CaseTerminator::Break);
            }
            _ => panic!("expected case"),
        }
    }

    #[test]
    fn test_case_multiple_patterns() {
        let kind = parse_first_compound("case $x in\na|b|c) echo match;;\nesac");
        match kind {
            CompoundCommandKind::Case { items, .. } => {
                assert_eq!(items[0].patterns.len(), 3);
            }
            _ => panic!("expected case"),
        }
    }

    #[test]
    fn test_case_empty() {
        let kind = parse_first_compound("case $x in\nesac");
        match kind {
            CompoundCommandKind::Case { items, .. } => {
                assert!(items.is_empty());
            }
            _ => panic!("expected case"),
        }
    }

    #[test]
    fn test_brace_group() {
        let kind = parse_first_compound("{ echo hello; }");
        assert!(matches!(kind, CompoundCommandKind::BraceGroup { .. }));
    }

    #[test]
    fn test_subshell() {
        let kind = parse_first_compound("(echo hello)");
        assert!(matches!(kind, CompoundCommandKind::Subshell { .. }));
    }

    #[test]
    fn test_function_def() {
        let prog = parse("myfunc() { echo hello; }");
        let cmd = &prog.commands[0].items[0].0.first.commands[0];
        match cmd {
            Command::FunctionDef(fd) => {
                assert_eq!(fd.name, "myfunc");
            }
            _ => panic!("expected function definition"),
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
            _ => panic!("expected function definition"),
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kish parser::tests::test_case`
Expected: FAIL — case/brace/subshell/function not implemented.

- [ ] **Step 3: Implement case clause**

Replace the `parse_case_clause` stub:

```rust
    fn parse_case_clause(&mut self) -> error::Result<CompoundCommandKind> {
        self.expect_reserved("case")?;
        let word = self.expect_word("case word")?;
        self.skip_newlines()?;
        self.expect_reserved("in")?;
        self.skip_newlines()?;

        let mut items = Vec::new();

        while !self.is_reserved("esac") && !self.is_at_end() {
            // Optional leading (
            let _ = self.eat(&Token::LParen)?;

            // Read patterns separated by |
            let mut patterns = Vec::new();
            patterns.push(self.expect_word("case pattern")?);
            while *self.current_token() == Token::Pipe {
                self.advance()?;
                patterns.push(self.expect_word("case pattern")?);
            }

            // Expect )
            if !self.eat(&Token::RParen)? {
                return Err(ShellError::new(
                    ShellErrorKind::UnexpectedToken,
                    self.current_span().line,
                    self.current_span().column,
                    format!("expected ')' in case item, got {:?}", self.current_token()),
                ));
            }

            self.skip_newlines()?;

            // Parse body (may be empty)
            let mut body = Vec::new();
            while !self.is_at_end()
                && !matches!(self.current_token(), Token::DSemi | Token::SemiAnd)
                && !self.is_reserved("esac")
            {
                body.push(self.parse_complete_command()?);
                self.skip_newlines()?;
            }

            // Terminator
            let terminator = match self.current_token() {
                Token::DSemi => {
                    self.advance()?;
                    CaseTerminator::Break
                }
                Token::SemiAnd => {
                    self.advance()?;
                    CaseTerminator::FallThrough
                }
                _ => CaseTerminator::Break, // last item before esac
            };

            self.skip_newlines()?;
            items.push(CaseItem { patterns, body, terminator });
        }

        self.expect_reserved("esac")?;

        Ok(CompoundCommandKind::Case { word, items })
    }
```

- [ ] **Step 4: Implement brace group and subshell**

Replace the `parse_brace_group` and `parse_subshell` stubs:

```rust
    fn parse_brace_group(&mut self) -> error::Result<CompoundCommandKind> {
        self.expect_reserved("{")?;
        let body = self.parse_compound_list()?;
        self.expect_reserved("}")?;
        Ok(CompoundCommandKind::BraceGroup { body })
    }

    fn parse_subshell(&mut self) -> error::Result<CompoundCommandKind> {
        if !self.eat(&Token::LParen)? {
            return Err(ShellError::new(
                ShellErrorKind::UnexpectedToken,
                self.current_span().line,
                self.current_span().column,
                "expected '('",
            ));
        }
        let body = self.parse_compound_list()?;
        if !self.eat(&Token::RParen)? {
            return Err(ShellError::new(
                ShellErrorKind::UnexpectedToken,
                self.current_span().line,
                self.current_span().column,
                "expected ')'",
            ));
        }
        Ok(CompoundCommandKind::Subshell { body })
    }
```

- [ ] **Step 5: Implement function definition**

Replace the `try_parse_function_def` stub:

```rust
    fn try_parse_function_def(&mut self) -> error::Result<Option<FunctionDef>> {
        // Function definition: NAME ( ) linebreak function_body
        // We need to look ahead for ( ) after the name
        if let Token::Word(w) = self.current_token().clone() {
            if let Some(name) = w.as_literal() {
                if !is_valid_name(name) {
                    return Ok(None);
                }
                // We can only detect function def by looking ahead for ( )
                // Save state and try
                let name = name.to_string();

                // Peek: is the next token LParen?
                // We need to save the lexer state to do lookahead
                let saved_current = self.current.clone();
                let saved_lexer_pos = self.lexer.save_state();

                self.advance()?;
                if *self.current_token() == Token::LParen {
                    self.advance()?;
                    if *self.current_token() == Token::RParen {
                        self.advance()?;
                        self.skip_newlines()?;

                        // Parse function body (must be a compound command)
                        let body = self.parse_compound_command()?;
                        let redirects = self.parse_redirect_list()?;

                        return Ok(Some(FunctionDef { name, body, redirects }));
                    }
                }

                // Not a function definition — restore state
                self.current = saved_current;
                self.lexer.restore_state(saved_lexer_pos);
                return Ok(None);
            }
        }
        Ok(None)
    }
```

- [ ] **Step 6: Add lexer state save/restore for function definition lookahead**

Add to `Lexer` in `src/lexer/mod.rs`:

```rust
    pub fn save_state(&self) -> LexerState {
        LexerState {
            pos: self.pos,
            line: self.line,
            column: self.column,
        }
    }

    pub fn restore_state(&mut self, state: LexerState) {
        self.pos = state.pos;
        self.line = state.line;
        self.column = state.column;
    }
```

Add the state struct (outside `impl Lexer`):

```rust
pub struct LexerState {
    pos: usize,
    line: usize,
    column: usize,
}
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p kish parser::tests`
Expected: All pass.

- [ ] **Step 8: Commit**

```bash
git add src/lexer/mod.rs src/parser/mod.rs
git commit -m "feat: parser case/esac, brace groups, subshells, function definitions"
```

---

### Task 12: Parser — here-document body reading

**Files:**
- Modify: `src/lexer/mod.rs`
- Modify: `src/parser/mod.rs`

- [ ] **Step 1: Write tests**

Add to `parser::tests`:

```rust
    #[test]
    fn test_heredoc_body() {
        let sc = parse_first_simple("cat <<EOF\nhello world\nEOF");
        assert_eq!(sc.redirects.len(), 1);
        match &sc.redirects[0].kind {
            RedirectKind::HereDoc(hd) => {
                assert_eq!(hd.body, vec![WordPart::Literal("hello world\n".to_string())]);
                assert!(!hd.strip_tabs);
            }
            _ => panic!("expected heredoc"),
        }
    }

    #[test]
    fn test_heredoc_strip_tabs() {
        let sc = parse_first_simple("cat <<-EOF\n\thello\n\tworld\n\tEOF");
        match &sc.redirects[0].kind {
            RedirectKind::HereDoc(hd) => {
                assert!(hd.strip_tabs);
                assert_eq!(hd.body, vec![WordPart::Literal("hello\nworld\n".to_string())]);
            }
            _ => panic!("expected heredoc"),
        }
    }

    #[test]
    fn test_heredoc_quoted_delimiter() {
        let sc = parse_first_simple("cat <<'EOF'\nhello $name\nEOF");
        match &sc.redirects[0].kind {
            RedirectKind::HereDoc(hd) => {
                // Quoted delimiter: body is literal (no expansion)
                assert_eq!(hd.body, vec![WordPart::Literal("hello $name\n".to_string())]);
            }
            _ => panic!("expected heredoc"),
        }
    }

    #[test]
    fn test_heredoc_with_command_after() {
        let prog = parse("cat <<EOF\nhello\nEOF\necho done");
        assert_eq!(prog.commands.len(), 2);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kish parser::tests::test_heredoc_body`
Expected: FAIL — heredoc body not read.

- [ ] **Step 3: Add here-document tracking to the lexer**

Add to `Lexer` struct:

```rust
pub struct Lexer {
    input: Vec<u8>,
    pos: usize,
    line: usize,
    column: usize,
    pending_heredocs: Vec<PendingHereDoc>,
    heredoc_bodies: Vec<Vec<WordPart>>,
}

pub struct PendingHereDoc {
    pub delimiter: String,
    pub quoted: bool,
    pub strip_tabs: bool,
}
```

Update `Lexer::new` to initialize the new fields:

```rust
    pub fn new(input: &str) -> Self {
        Self {
            input: input.as_bytes().to_vec(),
            pos: 0,
            line: 1,
            column: 1,
            pending_heredocs: Vec::new(),
            heredoc_bodies: Vec::new(),
        }
    }
```

Add to `save_state` / `restore_state`: also save/restore the pending_heredocs.

Add method to register and process heredocs:

```rust
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

    fn process_pending_heredocs(&mut self) -> error::Result<()> {
        let pending: Vec<PendingHereDoc> = self.pending_heredocs.drain(..).collect();
        for hd in pending {
            let body = self.read_heredoc_body(&hd)?;
            self.heredoc_bodies.push(body);
        }
        Ok(())
    }

    fn read_heredoc_body(&mut self, hd: &PendingHereDoc) -> error::Result<Vec<WordPart>> {
        let mut body = String::new();

        loop {
            if self.at_end() {
                return Err(ShellError::new(
                    ShellErrorKind::InvalidHereDoc,
                    self.line, self.column,
                    format!("here-document delimited by '{}' was not closed", hd.delimiter),
                ));
            }

            // Read a line
            let mut line = String::new();
            while !self.at_end() && self.current_byte() != b'\n' {
                line.push(self.current_byte() as char);
                self.advance();
            }
            // Consume the newline
            if !self.at_end() {
                self.advance();
            }

            // Strip leading tabs if <<-
            let check_line = if hd.strip_tabs {
                line.trim_start_matches('\t').to_string()
            } else {
                line.clone()
            };

            // Check if this line is the delimiter
            if check_line == hd.delimiter {
                break;
            }

            // Add the line to body (with the tab stripping applied)
            if hd.strip_tabs {
                body.push_str(&line.trim_start_matches('\t'));
            } else {
                body.push_str(&line);
            }
            body.push('\n');
        }

        // If delimiter was quoted, body is all literal (no expansion)
        // If unquoted, body would need expansion — but we store as literal for now
        // and let the expander handle it in Phase 3
        Ok(vec![WordPart::Literal(body)])
    }
```

- [ ] **Step 4: Modify parser to handle heredoc registration and body retrieval**

In the parser, modify `try_parse_redirect` for `DLess` and `DLessDash`:

```rust
            Token::DLess => {
                self.advance()?;
                let delimiter_word = self.expect_word("here-document delimiter")?;
                let (delimiter, quoted) = self.extract_heredoc_delimiter(&delimiter_word);
                self.lexer.register_heredoc(delimiter, quoted, false);
                RedirectKind::HereDoc(HereDoc {
                    body: vec![], // Filled in after newline processing
                    strip_tabs: false,
                })
            }
            Token::DLessDash => {
                self.advance()?;
                let delimiter_word = self.expect_word("here-document delimiter")?;
                let (delimiter, quoted) = self.extract_heredoc_delimiter(&delimiter_word);
                self.lexer.register_heredoc(delimiter, quoted, true);
                RedirectKind::HereDoc(HereDoc {
                    body: vec![],
                    strip_tabs: true,
                })
            }
```

Add the helper:

```rust
    fn extract_heredoc_delimiter(&self, word: &Word) -> (String, bool) {
        // Check if delimiter is quoted
        let mut delimiter = String::new();
        let mut quoted = false;
        for part in &word.parts {
            match part {
                WordPart::Literal(s) => delimiter.push_str(s),
                WordPart::SingleQuoted(s) => { delimiter.push_str(s); quoted = true; }
                WordPart::DoubleQuoted(parts) => {
                    quoted = true;
                    for p in parts {
                        if let WordPart::Literal(s) = p {
                            delimiter.push_str(s);
                        }
                    }
                }
                _ => {
                    // Other parts just contribute their literal representation
                    if let WordPart::DollarSingleQuoted(s) = part {
                        delimiter.push_str(s);
                        quoted = true;
                    }
                }
            }
        }
        (delimiter, quoted)
    }
```

Modify `parse_separator_op` to process heredocs on newline:

```rust
    fn parse_separator_op(&mut self) -> error::Result<Option<SeparatorOp>> {
        match self.current_token() {
            Token::Semi => {
                self.advance()?;
                Ok(Some(SeparatorOp::Semi))
            }
            Token::Amp => {
                self.advance()?;
                Ok(Some(SeparatorOp::Amp))
            }
            Token::Newline => {
                self.advance()?;
                self.process_heredocs()?;
                Ok(Some(SeparatorOp::Semi))
            }
            _ => Ok(None),
        }
    }

    fn process_heredocs(&mut self) -> error::Result<()> {
        if !self.lexer.pending_heredocs.is_empty() {
            self.lexer.process_pending_heredocs()?;
            // Now fill in the heredoc bodies in the most recently parsed redirects
            // This is handled by the caller retrieving bodies via take_heredoc_body
        }
        Ok(())
    }
```

To properly link heredoc bodies to redirects, after parsing a complete command line, walk the redirects and fill in any empty heredoc bodies:

Add a method to `Parser`:

```rust
    fn fill_heredoc_bodies(&mut self, redirects: &mut Vec<Redirect>) {
        for redir in redirects {
            if let RedirectKind::HereDoc(ref mut hd) = redir.kind {
                if hd.body.is_empty() {
                    if let Some(body) = self.lexer.take_heredoc_body() {
                        hd.body = body;
                    }
                }
            }
        }
    }
```

Call `fill_heredoc_bodies` at the end of `parse_simple_command` and after `parse_redirect_list` in `parse_command`:

In `parse_simple_command`, before returning:

```rust
        // Fill heredoc bodies if any were registered
        self.fill_heredoc_bodies(&mut redirects);

        Ok(SimpleCommand { assignments, words, redirects })
```

In `parse_command`, for compound commands:

```rust
        if self.is_compound_command_start() {
            let compound = self.parse_compound_command()?;
            let mut redirects = self.parse_redirect_list()?;
            self.fill_heredoc_bodies(&mut redirects);
            return Ok(Command::Compound(compound, redirects));
        }
```

- [ ] **Step 5: Handle newline processing for heredoc in skip_newlines**

Modify `skip_newlines` to also process pending heredocs:

```rust
    fn skip_newlines(&mut self) -> error::Result<()> {
        while *self.current_token() == Token::Newline {
            self.advance()?;
            if !self.lexer.pending_heredocs.is_empty() {
                self.lexer.process_pending_heredocs()?;
            }
        }
        Ok(())
    }
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p kish parser::tests`
Expected: All pass.

- [ ] **Step 7: Commit**

```bash
git add src/lexer/mod.rs src/parser/mod.rs
git commit -m "feat: here-document body reading with quoted/unquoted delimiter support"
```

---

### Task 13: Entry point and integration tests

**Files:**
- Modify: `src/main.rs`
- Create: `tests/helpers/mod.rs`
- Create: `tests/parser_integration.rs`

- [ ] **Step 1: Update main.rs with basic CLI**

Replace `src/main.rs`:

```rust
mod error;
mod lexer;
mod parser;

use std::env;
use std::fs;
use std::io::{self, Read};
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    match args.len() {
        1 => {
            // No arguments: interactive mode (not yet implemented)
            eprintln!("kish: interactive mode not yet implemented");
            process::exit(1);
        }
        _ => {
            if args[1] == "-c" {
                // -c command
                if args.len() < 3 {
                    eprintln!("kish: -c requires an argument");
                    process::exit(2);
                }
                run_string(&args[2]);
            } else if args[1] == "--parse" {
                // Debug: parse and dump AST
                if args.len() < 3 {
                    eprintln!("kish: --parse requires an argument");
                    process::exit(2);
                }
                let input = if args[2] == "-" {
                    let mut buf = String::new();
                    io::stdin().read_to_string(&mut buf).unwrap();
                    buf
                } else {
                    args[2].clone()
                };
                match parser::Parser::new(&input).parse_program() {
                    Ok(ast) => println!("{:#?}", ast),
                    Err(e) => {
                        eprintln!("{}", e);
                        process::exit(2);
                    }
                }
            } else {
                // Script file
                run_file(&args[1]);
            }
        }
    }
}

fn run_string(input: &str) {
    match parser::Parser::new(input).parse_program() {
        Ok(ast) => {
            // Execution engine not yet implemented — just confirm parsing succeeds
            println!("{:#?}", ast);
        }
        Err(e) => {
            eprintln!("{}", e);
            process::exit(2);
        }
    }
}

fn run_file(path: &str) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("kish: {}: {}", path, e);
            process::exit(127);
        }
    };
    run_string(&content);
}
```

- [ ] **Step 2: Create test helpers**

Create `tests/helpers/mod.rs`:

```rust
use std::path::PathBuf;
use std::process::Command;

pub struct ShellTest {
    pub input: String,
    pub expected_exit_code: i32,
}

impl ShellTest {
    /// Run kish with --parse and return whether parsing succeeded.
    pub fn parse_succeeds(&self) -> bool {
        let output = Command::new(kish_binary())
            .args(["--parse", &self.input])
            .output()
            .expect("failed to execute kish");
        output.status.code() == Some(self.expected_exit_code)
    }
}

pub fn kish_binary() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_BIN_EXE_kish"));
    path
}

/// Create a temporary directory and return its path.
/// The directory is deleted when the returned TempDir is dropped.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        let mut path = std::env::temp_dir();
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("kish-test-{}", id));
        std::fs::create_dir_all(&path).unwrap();
        TempDir { path }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn write_file(&self, name: &str, content: &str) -> PathBuf {
        let file_path = self.path.join(name);
        std::fs::write(&file_path, content).unwrap();
        file_path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
```

- [ ] **Step 3: Create integration tests**

Create `tests/parser_integration.rs`:

```rust
mod helpers;

use std::process::Command;

fn kish_parse(input: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kish"))
        .args(["--parse", input])
        .output()
        .expect("failed to execute kish")
}

#[test]
fn test_parse_simple_pipeline() {
    let out = kish_parse("echo hello | grep h");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn test_parse_and_or_list() {
    let out = kish_parse("true && echo yes || echo no");
    assert!(out.status.success());
}

#[test]
fn test_parse_if_statement() {
    let out = kish_parse("if true; then echo yes; elif false; then echo maybe; else echo no; fi");
    assert!(out.status.success());
}

#[test]
fn test_parse_for_loop() {
    let out = kish_parse("for i in a b c; do echo $i; done");
    assert!(out.status.success());
}

#[test]
fn test_parse_while_loop() {
    let out = kish_parse("while true; do echo loop; break; done");
    assert!(out.status.success());
}

#[test]
fn test_parse_case() {
    let out = kish_parse("case $x in\na) echo a;;\nb|c) echo bc;;\nesac");
    assert!(out.status.success());
}

#[test]
fn test_parse_function_def() {
    let out = kish_parse("myfunc() { echo hello; }");
    assert!(out.status.success());
}

#[test]
fn test_parse_subshell() {
    let out = kish_parse("(echo hello; echo world)");
    assert!(out.status.success());
}

#[test]
fn test_parse_brace_group() {
    let out = kish_parse("{ echo hello; echo world; }");
    assert!(out.status.success());
}

#[test]
fn test_parse_complex_redirects() {
    let out = kish_parse("cmd < in > out 2>&1 >>log");
    assert!(out.status.success());
}

#[test]
fn test_parse_assignments_and_command() {
    let out = kish_parse("FOO=bar BAZ=qux echo hello");
    assert!(out.status.success());
}

#[test]
fn test_parse_command_substitution() {
    let out = kish_parse("echo $(echo hello)");
    assert!(out.status.success());
}

#[test]
fn test_parse_arithmetic_expansion() {
    let out = kish_parse("echo $((1 + 2 * 3))");
    assert!(out.status.success());
}

#[test]
fn test_parse_parameter_expansion() {
    let out = kish_parse("echo ${name:-default} ${#name} ${path%%/*}");
    assert!(out.status.success());
}

#[test]
fn test_parse_nested_structures() {
    let out = kish_parse("if true; then for i in a b; do case $i in a) echo yes;; esac; done; fi");
    assert!(out.status.success());
}

#[test]
fn test_parse_semicolons_and_async() {
    let out = kish_parse("cmd1; cmd2 & cmd3");
    assert!(out.status.success());
}

#[test]
fn test_parse_error_unmatched_quote() {
    let out = kish_parse("echo 'hello");
    assert!(!out.status.success());
}

#[test]
fn test_parse_error_unexpected_token() {
    let out = kish_parse("if; then echo; fi");
    // This should fail because 'if' expects a compound list, not ';'
    assert!(!out.status.success());
}

#[test]
fn test_parse_script_file() {
    let tmp = helpers::TempDir::new();
    let script = tmp.write_file("test.sh", "#!/bin/kish\necho hello\nfor i in 1 2 3; do\n  echo $i\ndone\n");

    let output = Command::new(env!("CARGO_BIN_EXE_kish"))
        .arg(script.to_str().unwrap())
        .output()
        .expect("failed to execute kish");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
}
```

- [ ] **Step 4: Run all tests**

Run: `cargo test -p kish`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs tests/
git commit -m "feat: CLI entry point with --parse mode and integration test suite"
```

---

## Subsequent Phases

This plan covers **Phase 1 only** (Lexer + Parser + AST). After this phase is complete and all tests pass, create separate plans for:

- **Phase 2:** Basic execution engine (fork/exec, pipelines, lists)
- **Phase 3:** Word expansion (tilde, parameter, command sub, arithmetic, field splitting, pathname, quote removal)
- **Phase 4:** Redirections and here-document I/O
- **Phase 5:** Control structure execution (if, for, while, until, case, functions)
- **Phase 6:** Special builtins (set, export, trap, eval, exec, etc.) + alias expansion
- **Phase 7:** Signals and errexit
- **Phase 8:** Subshell environment isolation
