# kish — POSIX-Compliant Shell Design Specification

## Overview

kish is a POSIX-compliant Unix shell implemented in Rust, targeting macOS and Linux. The goal is to build a practical, daily-use shell (bash/zsh alternative) with strict POSIX compliance as the foundation, with room for future extensions after compliance is achieved.

### Design Decisions

- **Architecture:** Pipeline-style (Input → Lexer → Parser → AST → Expander → Executor), mapping directly to the POSIX processing pipeline (Section 2.1)
- **Parser:** Hand-written recursive descent (POSIX token classification is context-dependent, making parser combinators unsuitable)
- **Priority:** Non-interactive script execution first; interactive features (line editing, history, job control) added later
- **Platforms:** macOS + Linux
- **Testing:** Self-built test suite based on the POSIX specification

### Dependencies

```toml
[dependencies]
nix = { version = "0.31", features = ["signal", "process", "fs"] }
libc = "0.2"
```

No dev-dependencies — test helpers (temp files, etc.) are implemented in-project.

---

## 1. Module Structure

```
src/
├── main.rs              # Entry point, argument handling
├── shell.rs             # Shell struct (execution environment integration)
├── input.rs             # Input reading (file, string, stdin)
├── lexer/
│   ├── mod.rs           # Lexer core (tokenization)
│   ├── token.rs         # Token type definitions
│   └── quote.rs         # Quote state management
├── parser/
│   ├── mod.rs           # Parser core (recursive descent)
│   └── ast.rs           # AST node definitions
├── expand/
│   ├── mod.rs           # Expansion orchestration
│   ├── tilde.rs         # Tilde expansion
│   ├── param.rs         # Parameter expansion
│   ├── command_sub.rs   # Command substitution
│   ├── arith.rs         # Arithmetic expansion
│   ├── field_split.rs   # Field splitting
│   ├── pathname.rs      # Pathname expansion (glob)
│   └── quote_removal.rs # Quote removal
├── exec/
│   ├── mod.rs           # Command execution engine
│   ├── pipeline.rs      # Pipeline execution
│   ├── redirect.rs      # Redirection handling
│   ├── job.rs           # Job management (for future job control)
│   └── subshell.rs      # Subshell environment
├── builtin/
│   ├── mod.rs           # Builtin dispatch
│   ├── special.rs       # Special builtins (export, set, trap, etc.)
│   └── regular.rs       # Regular builtins (cd, echo, test, etc.)
├── env/
│   ├── mod.rs           # Shell execution environment
│   ├── vars.rs          # Variable management (export, readonly attributes)
│   ├── functions.rs     # Function definition management
│   └── alias.rs         # Alias management
├── signal.rs            # Signal handling, trap
└── error.rs             # Error type definitions
```

---

## 2. AST Design

AST nodes map directly to the POSIX Shell Grammar (Section 2.10).

### Core Nodes

```rust
/// Top-level program
struct Program {
    commands: Vec<CompleteCommand>,
}

/// Complete command (sequence of AND-OR lists)
struct CompleteCommand {
    list: Vec<AndOrList>,
    last_separator: Option<SeparatorOp>,
}

/// AND-OR list (pipelines joined by && / ||)
struct AndOrList {
    first: Pipeline,
    rest: Vec<(AndOrOp, Pipeline)>,
}

enum AndOrOp { And, Or }

enum SeparatorOp { Semi, Amp }

/// Pipeline
struct Pipeline {
    negated: bool,              // ! prefix
    commands: Vec<Command>,
}

/// Command
enum Command {
    Simple(SimpleCommand),
    Compound(CompoundCommand, Vec<Redirect>),
    FunctionDef(FunctionDef),
}
```

### Simple Command

```rust
struct SimpleCommand {
    assignments: Vec<Assignment>,
    words: Vec<Word>,           // command name + arguments
    redirects: Vec<Redirect>,
}

struct Assignment {
    name: String,
    value: Option<Word>,        // None = empty string
}
```

### Compound Commands

```rust
struct CompoundCommand {
    kind: CompoundCommandKind,
}

enum CompoundCommandKind {
    BraceGroup(Vec<AndOrList>),
    Subshell(Vec<AndOrList>),
    ForLoop {
        var: String,
        words: Option<Vec<Word>>,   // None = "$@"
        body: Vec<AndOrList>,
    },
    CaseClause {
        word: Word,
        items: Vec<CaseItem>,
    },
    If {
        condition: Vec<AndOrList>,
        then_part: Vec<AndOrList>,
        elif_parts: Vec<(Vec<AndOrList>, Vec<AndOrList>)>,
        else_part: Option<Vec<AndOrList>>,
    },
    WhileLoop {
        condition: Vec<AndOrList>,
        body: Vec<AndOrList>,
    },
    UntilLoop {
        condition: Vec<AndOrList>,
        body: Vec<AndOrList>,
    },
}

struct CaseItem {
    patterns: Vec<Word>,
    body: Vec<AndOrList>,
    terminator: CaseTerminator,     // ;; or ;&
}

enum CaseTerminator { Break, FallThrough }
```

### Word (Pre-expansion Token)

```rust
/// Word preserves pre-expansion structure.
/// e.g. "hello ${name}!" contains literals and expandable parts.
struct Word {
    parts: Vec<WordPart>,
}

enum WordPart {
    Literal(String),
    SingleQuoted(String),
    DoubleQuoted(Vec<WordPart>),
    DollarSingleQuoted(String),
    Parameter(ParamExpr),
    CommandSub(Vec<AndOrList>),
    ArithSub(ArithExpr),
    Tilde(Option<String>),
    Glob(GlobPattern),
}

/// Glob pattern for pathname expansion
enum GlobPattern {
    Star,                        // *
    Question,                    // ?
    Bracket(BracketExpr),        // [...]
    Sequence(Vec<GlobPattern>),  // concatenation of patterns
}

struct BracketExpr {
    negated: bool,               // [!...]
    elements: Vec<BracketElement>,
}

enum BracketElement {
    Char(char),
    Range(char, char),           // [a-z]
    Class(String),               // [:alpha:]
}
}
```

### Parameter Expansion

```rust
enum ParamExpr {
    Simple(String),                          // $name, ${name}
    Positional(usize),                       // $1, ${10}
    Special(SpecialParam),                   // $@, $*, $#, $?, $$, $!, $0, $-
    Default(String, Option<Word>),           // ${name:-word}
    Assign(String, Option<Word>),            // ${name:=word}
    Error(String, Option<Word>),             // ${name:?word}
    Alt(String, Option<Word>),               // ${name:+word}
    Length(String),                           // ${#name}
    StripShortEnd(String, Word),             // ${name%pattern}
    StripLongEnd(String, Word),              // ${name%%pattern}
    StripShortStart(String, Word),           // ${name#pattern}
    StripLongStart(String, Word),            // ${name##pattern}
    // No-colon variants (test only for unset, not null)
    DefaultNoNull(String, Option<Word>),     // ${name-word}
    AssignNoNull(String, Option<Word>),      // ${name=word}
    ErrorNoNull(String, Option<Word>),       // ${name?word}
    AltNoNull(String, Option<Word>),         // ${name+word}
}
```

### Arithmetic Expression

```rust
struct ArithExpr {
    kind: ArithExprKind,
}

enum ArithExprKind {
    Literal(i64),
    Variable(String),
    BinaryOp(Box<ArithExpr>, ArithOp, Box<ArithExpr>),
    UnaryOp(ArithUnaryOp, Box<ArithExpr>),
    Ternary(Box<ArithExpr>, Box<ArithExpr>, Box<ArithExpr>),  // a?b:c
    Assign(String, Box<ArithExpr>),                           // a=expr
    CompoundAssign(String, ArithOp, Box<ArithExpr>),          // a+=expr
}
```

### Redirection

```rust
struct Redirect {
    fd: Option<i32>,            // default: input=0, output=1
    kind: RedirectKind,
}

enum RedirectKind {
    Input(Word),                // <
    Output(Word),               // >
    OutputClobber(Word),        // >|
    Append(Word),               // >>
    HereDoc(HereDoc),           // << / <<-
    DupInput(Word),             // <&
    DupOutput(Word),            // >&
    ReadWrite(Word),            // <>
}

struct HereDoc {
    content: Vec<WordPart>,     // WordPart for expandable, Literal-only for quoted delimiter
    strip_tabs: bool,           // true for <<-
}
```

### Function Definition

```rust
struct FunctionDef {
    name: String,
    body: CompoundCommand,
    redirects: Vec<Redirect>,
}
```

---

## 3. Lexer Design

### Token Types

```rust
enum Token {
    Word(Word),
    AssignmentWord(Assignment),
    IoNumber(i32),
    Newline,
    // Operators
    Pipe, AndIf, OrIf, Semi, Amp,
    DSemi, SemiAnd,
    Less, Great, DLess, DGreat,
    LessAnd, GreatAnd, LessGreat,
    DLessDash, Clobber,
    LParen, RParen,
    // Reserved words (classified via parser context)
    If, Then, Else, Elif, Fi,
    Do, Done,
    Case, Esac, In,
    While, Until, For,
    Lbrace, Rbrace, Bang,
}
```

### Context-Dependent Tokenization

The parser sets context on the lexer before requesting the next token, enabling correct application of POSIX token classification rules 1-9.

```rust
struct Lexer<'a> {
    input: &'a [u8],
    pos: usize,
    context: LexerContext,
    pending_heredocs: Vec<PendingHereDoc>,
    expanding_aliases: HashSet<String>,
    alias_input_stack: Vec<String>,
}

enum LexerContext {
    CommandPosition,    // Recognize reserved words
    WordPosition,       // Do not recognize reserved words
    CaseHead,           // Recognize in / esac
    CasePattern,        // Case pattern context
    ForHead,            // Recognize in / do
    HereDocBody {       // Reading here-document content
        delimiter: String,
        quoted: bool,
        strip_tabs: bool,
    },
}
```

### Quote State Management

```rust
struct QuoteState {
    stack: Vec<QuoteKind>,
}

enum QuoteKind {
    SingleQuote,
    DoubleQuote,
    CommandSub(usize),   // $() nesting depth
    ArithSub,            // $(())
    Backtick,
}
```

### Here-Document Processing

Here-document body is read after the command line containing `<<` is fully parsed. The lexer maintains a queue of pending here-documents and consumes body lines after each newline token.

```rust
struct PendingHereDoc {
    delimiter: String,
    quoted: bool,
    strip_tabs: bool,
}
```

### Alias Expansion

Handled at the lexer level. When a word in `CommandPosition` matches an alias, its value is inserted into the input buffer and re-scanned. A set of currently-expanding alias names prevents infinite recursion. If an alias value ends with whitespace, the next token is also subject to alias expansion.

### Parser-Lexer Feedback

```rust
impl Parser<'_> {
    fn next_token(&mut self, ctx: LexerContext) -> Result<Token> {
        self.lexer.context = ctx;
        self.lexer.next_token()
    }
}
```

---

## 4. Parser Design

Hand-written recursive descent parser with methods corresponding 1:1 to the POSIX BNF grammar (Section 2.10).

```rust
struct Parser<'a> {
    lexer: Lexer<'a>,
    current: Token,
}

impl Parser<'_> {
    fn parse_program(&mut self) -> Result<Program>;
    fn parse_complete_command(&mut self) -> Result<CompleteCommand>;
    fn parse_list(&mut self) -> Result<Vec<AndOrList>>;
    fn parse_and_or(&mut self) -> Result<AndOrList>;
    fn parse_pipeline(&mut self) -> Result<Pipeline>;
    fn parse_command(&mut self) -> Result<Command>;
    fn parse_simple_command(&mut self) -> Result<SimpleCommand>;
    fn parse_compound_command(&mut self) -> Result<CompoundCommand>;
    fn parse_if_clause(&mut self) -> Result<CompoundCommandKind>;
    fn parse_for_clause(&mut self) -> Result<CompoundCommandKind>;
    fn parse_case_clause(&mut self) -> Result<CompoundCommandKind>;
    fn parse_while_clause(&mut self) -> Result<CompoundCommandKind>;
    fn parse_until_clause(&mut self) -> Result<CompoundCommandKind>;
    fn parse_brace_group(&mut self) -> Result<CompoundCommandKind>;
    fn parse_subshell(&mut self) -> Result<CompoundCommandKind>;
    fn parse_function_def(&mut self) -> Result<FunctionDef>;
    fn parse_compound_list(&mut self) -> Result<Vec<AndOrList>>;
    fn parse_redirect(&mut self) -> Result<Redirect>;
    fn parse_io_here(&mut self) -> Result<Redirect>;
}
```

### Error Reporting

```rust
struct ParseError {
    kind: ParseErrorKind,
    line: usize,
    column: usize,
    message: String,
}

enum ParseErrorKind {
    UnexpectedToken,
    UnexpectedEof,
    UnmatchedQuote,
    InvalidRedirect,
    InvalidFunctionName,
}
```

---

## 5. Expansion Design

Strictly follows the POSIX expansion order (Section 2.6):

```
Word
 │
 ├─ 1. Tilde expansion
 ├─ 2. Parameter expansion     ┐
 ├─ 3. Command substitution    ├─ Processed left-to-right simultaneously
 ├─ 4. Arithmetic expansion    ┘
 │
 ├─ 5. Field splitting (outside double-quotes only)
 ├─ 6. Pathname expansion (unless set -f)
 └─ 7. Quote removal
         │
         ▼
    Vec<String>  (expanded field list)
```

### Expansion Interface

```rust
struct Expander<'a> {
    env: &'a mut ShellEnv,
}

/// Expanded field with per-byte quote tracking
struct ExpandedField {
    value: String,
    quoted_mask: Vec<bool>,
}

impl Expander<'_> {
    /// Full word expansion (all stages)
    fn expand_word(&mut self, word: &Word) -> Result<Vec<String>>;
    /// Assignment value expansion (no field splitting, no pathname expansion)
    fn expand_assignment(&mut self, word: &Word) -> Result<String>;
    /// Redirect target expansion (must produce exactly 1 field)
    fn expand_redirect(&mut self, word: &Word) -> Result<String>;
    /// Here-document body expansion
    fn expand_heredoc(&mut self, parts: &[WordPart]) -> Result<String>;
}
```

### Key Expansion Behaviors

- **`"$@"` in double-quotes:** Each positional parameter becomes a separate field. Zero parameters produce zero fields.
- **IFS field splitting:** Distinguishes IFS whitespace from IFS non-whitespace characters. Only bytes from expansion results are treated as delimiters; literal bytes are always ordinary.
- **Tilde expansion:** At word start or after `=`/`:` in assignments. Result is protected from field splitting and pathname expansion.
- **Command substitution:** `fork` + subshell execution, capture stdout via pipe, strip trailing newlines.
- **Arithmetic:** Signed `i64` arithmetic with C-style operators, decimal/octal/hexadecimal constants, nested parameter expansion.

---

## 6. Execution Engine

### Shell Execution Environment

```rust
struct ShellEnv {
    vars: VarStore,
    functions: HashMap<String, FunctionDef>,
    aliases: HashMap<String, String>,
    options: ShellOptions,
    traps: HashMap<TrapCondition, TrapAction>,  // See definitions below
    positional_params: Vec<String>,
    last_exit_status: i32,       // $?
    last_bg_pid: Option<pid_t>,  // $!
    shell_pid: pid_t,            // $$ (unchanged in subshells)
    shell_name: String,          // $0
    lineno: usize,               // $LINENO
    jobs: Vec<Job>,
}
```

### Variable Store

```rust
struct VarStore {
    scopes: Vec<Scope>,
}

struct Scope {
    vars: HashMap<String, Variable>,
}

struct Variable {
    value: String,
    exported: bool,
    readonly: bool,
}
```

### Trap Types

```rust
enum TrapCondition {
    Exit,       // EXIT or 0
    Signal(i32), // HUP, INT, QUIT, TERM, etc. (signal number)
}

enum TrapAction {
    Default,           // Reset to default
    Ignore,            // Ignore the signal (action = "")
    Command(String),   // Execute via eval
}
```

### Job Management

```rust
struct Job {
    pgid: pid_t,                 // Process group ID
    pids: Vec<pid_t>,            // Process IDs in the job
    status: JobStatus,
    command: String,             // Original command string (for display)
}

enum JobStatus {
    Running,
    Stopped,
    Done(i32),                   // Exit status
}
```

### Shell Options

```rust
struct ShellOptions {
    allexport: bool,     // -a
    notify: bool,        // -b
    noclobber: bool,     // -C
    errexit: bool,       // -e
    noglob: bool,        // -f
    noexec: bool,        // -n
    monitor: bool,       // -m
    nounset: bool,       // -u
    verbose: bool,       // -v
    xtrace: bool,        // -x
    ignoreeof: bool,
    pipefail: bool,
}
```

### Simple Command Execution Flow

Per POSIX Section 2.9.1:

```
SimpleCommand
 │
 ├─ 1. Classify words: separate assignments and redirections
 ├─ 2. Expand command name (first non-assignment, non-redirect word)
 │     Expand remaining words for argument list
 ├─ 3. Apply redirections
 ├─ 4. Expand and apply variable assignments
 │
 └─ Branch on command name presence:
      │
      ├─ No command name → apply assignments to current env, exit 0
      │
      └─ Command name present → search and execute:
           ├─ 1. Special builtin → current process, assignments persist
           ├─ 2. Shell function   → current process, replace positional params
           ├─ 3. Regular builtin  → current process, assignments temporary
           └─ 4. External command → fork + execve
```

### Pipeline Execution

```
cmd1 | cmd2 | cmd3

pipe0: cmd1.stdout → cmd2.stdin
pipe1: cmd2.stdout → cmd3.stdin

1. Create all pipes
2. Fork each command
   - Child: dup2 stdin/stdout to pipe ends, close unused fds, execute
3. Parent: waitpid for all children
4. Exit status:
   - pipefail off: last command's status
   - pipefail on: last non-zero status (0 if all zero)
5. ! prefix: logically negate (0↔1)
```

### Subshell Execution

```
1. fork()
2. Child:
   a. Reset traps (non-ignored → default)
   b. Execute commands
   c. exit(last_status)
3. Parent: waitpid → return exit status
```

### Redirection Handling

```rust
struct RedirectState {
    saved_fds: Vec<(i32, i32)>,  // (original_fd, saved_copy)
}

impl RedirectState {
    fn apply(&mut self, redirects: &[Redirect], env: &mut ShellEnv) -> Result<()>;
    fn restore(&mut self) -> Result<()>;  // Restore for builtins/functions
}
```

External commands apply redirections in the child process (no restore needed). Builtins and shell functions apply/restore in the current process.

### errexit (set -e) Control

```rust
struct Errexit {
    suppressed_depth: usize,
}
```

Suppressed during: if/while/until condition lists, `!` pipelines, non-final commands in AND-OR lists.

### Signal Handling

- Use `nix::sys::signal` to set up signal handlers that set flags
- Check flags at appropriate points in the main loop
- Async lists with job control disabled: set SIGINT/SIGQUIT to SIG_IGN
- Trap actions are executed via `eval` of the action string
- Signals during `wait`: return immediately with status > 128, then execute trap
- Signals during foreground command: deferred until command completion

---

## 7. Testing Strategy

Three-layer test suite built on the POSIX specification.

### Layer 1: Unit Tests

In-module `#[cfg(test)]` tests for each component:

- **lexer/** — tokenization accuracy, quote state transitions, here-doc delimiter recognition
- **parser/** — AST generation for each syntax form, error cases, edge cases
- **expand/** — individual expansion step tests
- **env/** — variable store attribute management, scope operations
- **builtin/** — argument processing and return values

### Layer 2: Integration Tests (POSIX Section-Based)

```
tests/
├── quoting.rs           # Section 2.2
├── token_recognition.rs # Section 2.3
├── reserved_words.rs    # Section 2.4
├── parameters.rs        # Section 2.5
├── expansions.rs        # Section 2.6
├── redirections.rs      # Section 2.7
├── exit_status.rs       # Section 2.8
├── commands.rs          # Section 2.9
├── signals.rs           # Section 2.12
├── pattern_matching.rs  # Section 2.14
├── special_builtins.rs  # Section 2.15
└── helpers/
    └── mod.rs           # Test helpers (temp file management, etc.)
```

Test helper launches `kish` as a process and verifies stdout, stderr, and exit code:

```rust
struct ShellTest {
    input: String,
    stdin: Option<String>,
    env: Vec<(String, String)>,
    expected_stdout: String,
    expected_stderr: Option<String>,
    expected_exit_code: i32,
}
```

Temp file/directory management is implemented in-project (no external crate).

### Layer 3: Script-Based Tests

```
tests/scripts/
├── pipeline_basic.sh
├── subshell_env_isolation.sh
├── errexit_exceptions.sh
├── heredoc_expansion.sh
├── case_fallthrough.sh
└── ...
```

Self-verifying scripts that exit non-zero on failure. A Rust test runner auto-discovers and executes them.

---

## 8. Build Configuration

### Cargo.toml

```toml
[package]
name = "kish"
version = "0.1.0"
edition = "2024"

[dependencies]
nix = { version = "0.31", features = ["signal", "process", "fs"] }
libc = "0.2"
```

### Entry Point

```
kish                         → Interactive mode (future)
kish script.sh [args...]     → Script execution
kish -c 'command' [args...]  → Command string execution
```

---

## 9. Implementation Phases

| Phase | Content | Goal |
|-------|---------|------|
| **1** | Lexer + Parser + AST | Correctly convert shell scripts to AST |
| **2** | Basic execution engine | Execute simple commands, pipelines, lists (`;`, `&&`, `\|\|`, `&`) |
| **3** | Expansion | Tilde → Parameter → Command sub → Arithmetic → Field split → Pathname → Quote removal |
| **4** | Redirection + here-document | All redirection forms |
| **5** | Control structures | if, for, while, until, case, function definitions |
| **6** | Special builtins | set, export, trap, eval, exec, etc. |
| **7** | Signals + errexit | trap, set -e exception handling |
| **8** | Subshell environment | `()`, pipeline subshells, environment isolation |
