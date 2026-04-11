# Comprehensive Refactoring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor kish's data structures and module boundaries for future-proofing (fish/zsh-level interactive features) and performance, using E2E tests as the correctness standard.

**Architecture:** Bottom-up approach: establish benchmarks first (Phase 0), redesign core data structures (Phase 1), reorganize module boundaries (Phase 2), then apply cross-cutting improvements (Phase 3). Each phase ends with all tests passing and benchmark comparison.

**Tech Stack:** Rust (edition 2024), criterion 0.5 (benchmarks), nix 0.31, libc 0.2, crossterm 0.29

**Design Spec:** `docs/superpowers/specs/2026-04-12-comprehensive-refactoring-design.md`

---

## Phase 0: Benchmark Infrastructure

### Task 1: Add criterion dependency and benchmark configuration

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add criterion to Cargo.toml**

Add the `[dev-dependencies]` section and benchmark targets to `Cargo.toml`:

```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "lexer_bench"
harness = false

[[bench]]
name = "parser_bench"
harness = false

[[bench]]
name = "expand_bench"
harness = false
```

- [ ] **Step 2: Verify dependency resolves**

Run: `cargo check`
Expected: Compiles successfully with criterion resolved.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "build: add criterion benchmark dependency"
```

### Task 2: Create lexer benchmark

**Files:**
- Create: `benches/lexer_bench.rs`

- [ ] **Step 1: Write lexer benchmark**

```rust
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use kish::lexer::Lexer;

const SMALL_SCRIPT: &str = r#"
echo hello world
FOO=bar
echo "$FOO"
ls -la /tmp
if [ -f /etc/hosts ]; then echo found; fi
cat file.txt | grep pattern | wc -l
A=1; B=2; echo $((A + B))
cd /tmp && pwd
export PATH="/usr/bin:$PATH"
for i in 1 2 3; do echo "$i"; done
"#;

const LARGE_SCRIPT: &str = include_str!("../benches/data/large_script.sh");

fn lex_all(input: &str) {
    let mut lexer = Lexer::new(input);
    loop {
        match lexer.next_token() {
            Ok(tok) => {
                if tok.token == kish::lexer::token::Token::Eof {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

fn bench_lexer(c: &mut Criterion) {
    c.bench_function("lex_small", |b| {
        b.iter(|| lex_all(black_box(SMALL_SCRIPT)))
    });
    c.bench_function("lex_large", |b| {
        b.iter(|| lex_all(black_box(LARGE_SCRIPT)))
    });
}

criterion_group!(benches, bench_lexer);
criterion_main!(benches);
```

- [ ] **Step 2: Create large script benchmark data**

Create directory and file `benches/data/large_script.sh` — a ~500-line shell script exercising diverse syntax:

```sh
#!/bin/sh
# Large benchmark script — exercises lexer with mixed syntax

# Variable assignments
A=hello
B="world with spaces"
C='single quoted'
D=$A

# Arithmetic
i=0
while [ $i -lt 100 ]; do
    i=$((i + 1))
    result=$((i * 2 + 3))
done

# Functions
my_func() {
    local_var="$1"
    echo "arg: $local_var"
    return 0
}

# Case statement
case "$A" in
    hello) echo "matched hello" ;;
    world) echo "matched world" ;;
    *) echo "no match" ;;
esac

# Here document
cat <<EOF
This is a heredoc with $A expansion
and multiple lines
of content
EOF

# Nested quoting
echo "double with 'single' inside"
echo 'single with "double" inside'
echo "parameter ${A:-default} expansion"
echo "command $(echo sub) substitution"

# Pipelines and lists
echo one | cat | cat | cat
echo a && echo b || echo c
echo x; echo y; echo z

# Redirects
echo output > /dev/null 2>&1
cat < /dev/null

# For loop with command substitution
for f in a b c d e f g h i j; do
    echo "$f"
done

# Complex parameter expansions
: "${UNSET:-default}"
: "${A:+alternate}"
: "${#A}"
: "${A%l*}"
: "${A%%l*}"
: "${A#h}"
: "${A##h}"
```

Repeat similar blocks to reach ~500 lines total (copy the above patterns with variations 4-5 times).

- [ ] **Step 3: Run benchmark to verify it works**

Run: `cargo bench --bench lexer_bench`
Expected: Benchmark completes and prints timing results for `lex_small` and `lex_large`.

- [ ] **Step 4: Commit**

```bash
git add benches/lexer_bench.rs benches/data/large_script.sh
git commit -m "bench: add lexer tokenization benchmarks"
```

### Task 3: Create parser benchmark

**Files:**
- Create: `benches/parser_bench.rs`

- [ ] **Step 1: Write parser benchmark**

```rust
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use kish::parser::Parser;

const SMALL_SCRIPT: &str = r#"
echo hello world
FOO=bar
echo "$FOO"
ls -la /tmp
if [ -f /etc/hosts ]; then echo found; fi
cat file.txt | grep pattern | wc -l
A=1; B=2; echo $((A + B))
cd /tmp && pwd
export PATH="/usr/bin:$PATH"
for i in 1 2 3; do echo "$i"; done
"#;

const LARGE_SCRIPT: &str = include_str!("../benches/data/large_script.sh");

fn parse_all(input: &str) {
    let mut parser = Parser::new(input);
    let _ = parser.parse_program();
}

fn bench_parser(c: &mut Criterion) {
    c.bench_function("parse_small", |b| {
        b.iter(|| parse_all(black_box(SMALL_SCRIPT)))
    });
    c.bench_function("parse_large", |b| {
        b.iter(|| parse_all(black_box(LARGE_SCRIPT)))
    });
}

criterion_group!(benches, bench_parser);
criterion_main!(benches);
```

- [ ] **Step 2: Run benchmark to verify**

Run: `cargo bench --bench parser_bench`
Expected: Benchmark completes for `parse_small` and `parse_large`.

- [ ] **Step 3: Commit**

```bash
git add benches/parser_bench.rs
git commit -m "bench: add parser benchmarks"
```

### Task 4: Create expansion benchmark

**Files:**
- Create: `benches/expand_bench.rs`

- [ ] **Step 1: Write expansion benchmark**

```rust
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use kish::env::ShellEnv;
use kish::expand::{expand_word, expand_words};
use kish::parser::ast::{ParamExpr, Word, WordPart};

fn bench_expand(c: &mut Criterion) {
    // Benchmark: parameter expansion with default
    c.bench_function("expand_param_default", |b| {
        b.iter(|| {
            let mut env = ShellEnv::new("kish", vec![]);
            env.vars.set("FOO", "hello").unwrap();
            let word = Word {
                parts: vec![WordPart::Parameter(ParamExpr::Default {
                    name: "BAR".to_string(),
                    word: Some(Word::literal("default_value")),
                    null_check: true,
                })],
            };
            for _ in 0..1000 {
                let _ = expand_word(black_box(&mut env), black_box(&word));
            }
        })
    });

    // Benchmark: field splitting with varied IFS
    c.bench_function("expand_field_split", |b| {
        b.iter(|| {
            let mut env = ShellEnv::new("kish", vec![]);
            env.vars.set("IFS", ":").unwrap();
            env.vars.set("PATH", "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/go/bin:/home/user/.cargo/bin").unwrap();
            let word = Word {
                parts: vec![WordPart::Parameter(ParamExpr::Simple("PATH".to_string()))],
            };
            for _ in 0..1000 {
                let _ = expand_word(black_box(&mut env), black_box(&word));
            }
        })
    });

    // Benchmark: simple literal words (baseline)
    c.bench_function("expand_literal_words", |b| {
        b.iter(|| {
            let mut env = ShellEnv::new("kish", vec![]);
            let words: Vec<Word> = (0..100)
                .map(|i| Word::literal(&format!("arg{}", i)))
                .collect();
            let _ = expand_words(black_box(&mut env), black_box(&words));
        })
    });
}

criterion_group!(benches, bench_expand);
criterion_main!(benches);
```

- [ ] **Step 2: Run benchmark to verify**

Run: `cargo bench --bench expand_bench`
Expected: Benchmark completes for all three expansion benchmarks.

- [ ] **Step 3: Commit**

```bash
git add benches/expand_bench.rs
git commit -m "bench: add expansion pipeline benchmarks"
```

### Task 5: Record baseline measurements

- [ ] **Step 1: Run full benchmark suite and save baseline**

Run: `cargo bench 2>&1 | tee benches/baseline.txt`
Expected: All benchmarks complete. File `benches/baseline.txt` contains timing data.

- [ ] **Step 2: Run all tests to confirm no regressions**

Run: `cargo test && sh e2e/run_tests.sh`
Expected: All lib/integration tests pass. All E2E tests pass (except known XFAILs).

- [ ] **Step 3: Commit**

```bash
git add benches/baseline.txt
git commit -m "bench: record Phase 0 baseline measurements"
```

---

## Phase 1: Core Data Structure Redesign

### Task 6: AST — Introduce CommandList type alias

**Files:**
- Modify: `src/parser/ast.rs`

- [ ] **Step 1: Add CommandList type alias and update struct definitions**

In `src/parser/ast.rs`, add after the `Program` struct definition (line 5):

```rust
/// A list of complete commands — used as the body of compound commands.
pub type CommandList = Vec<CompleteCommand>;
```

Then replace all `Vec<CompleteCommand>` occurrences in compound command fields:

In `CompoundCommandKind`:
- `BraceGroup { body: Vec<CompleteCommand> }` → `BraceGroup { body: CommandList }`
- `Subshell { body: Vec<CompleteCommand> }` → `Subshell { body: CommandList }`
- `If { condition: Vec<CompleteCommand>, then_part: Vec<CompleteCommand>, elif_parts: Vec<(Vec<CompleteCommand>, Vec<CompleteCommand>)>, else_part: Option<Vec<CompleteCommand>> }` → `If { condition: CommandList, then_part: CommandList, elif_parts: Vec<(CommandList, CommandList)>, else_part: Option<CommandList> }`
- `For { var: String, words: Option<Vec<Word>>, body: Vec<CompleteCommand> }` → `For { ..., body: CommandList }`
- `While { condition: Vec<CompleteCommand>, body: Vec<CompleteCommand> }` → `While { condition: CommandList, body: CommandList }`
- `Until { condition: Vec<CompleteCommand>, body: Vec<CompleteCommand> }` → `Until { condition: CommandList, body: CommandList }`

In `CaseItem`:
- `body: Vec<CompleteCommand>` → `body: CommandList`

- [ ] **Step 2: Run tests to verify this is a transparent change**

Run: `cargo test`
Expected: All tests pass. `CommandList` is a type alias so no call sites change.

- [ ] **Step 3: Commit**

```bash
git add src/parser/ast.rs
git commit -m "refactor(ast): introduce CommandList type alias for Vec<CompleteCommand>"
```

### Task 7: AST — Rc-wrap FunctionDef body

**Files:**
- Modify: `src/parser/ast.rs`
- Modify: `src/parser/mod.rs` (function definition parsing)
- Modify: `src/exec/mod.rs` (function registration and execution)
- Modify: `src/env/mod.rs` (functions HashMap type)
- Modify: `tests/parser_integration.rs` (if it constructs FunctionDef directly)

- [ ] **Step 1: Add Rc import and update FunctionDef in ast.rs**

At the top of `src/parser/ast.rs`, add:

```rust
use std::rc::Rc;
```

Change `FunctionDef`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    pub name: String,
    pub body: Rc<CompoundCommand>,
    pub redirects: Vec<Redirect>,
}
```

- [ ] **Step 2: Update parser to wrap body in Rc**

In `src/parser/mod.rs`, find the function definition parsing code (where `FunctionDef` is constructed) and wrap the body:

```rust
// Change from:
FunctionDef { name, body: compound, redirects }
// To:
FunctionDef { name, body: Rc::new(compound), redirects }
```

Add `use std::rc::Rc;` to parser imports if not present.

- [ ] **Step 3: Update executor function call to dereference Rc**

In `src/exec/mod.rs`, the `exec_function_call` method at line 370 accesses `func_def.body`:

```rust
// Change from:
let status = self.exec_compound_command(&func_def.body, &func_def.redirects);
// To (Rc auto-derefs, but exec_compound_command takes &CompoundCommand):
let status = self.exec_compound_command(&func_def.body, &func_def.redirects);
```

Since `Rc<CompoundCommand>` auto-derefs to `&CompoundCommand`, most call sites should work unchanged. Find any `.clone()` of `FunctionDef` in exec and verify they now clone the Rc (cheap) rather than the full AST.

- [ ] **Step 4: Update exec_command function definition registration**

In `src/exec/mod.rs`, find where functions are registered into `self.env.functions`:

```rust
// The existing code does something like:
self.env.functions.insert(name.clone(), func_def.clone());
```

This now clones `Rc<CompoundCommand>` (cheap reference count increment) instead of the entire AST tree.

- [ ] **Step 5: Fix any test code that constructs FunctionDef directly**

Search `tests/` for `FunctionDef {` and update to use `Rc::new(...)` for the body field.

- [ ] **Step 6: Run tests**

Run: `cargo test && sh e2e/run_tests.sh`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/parser/ast.rs src/parser/mod.rs src/exec/mod.rs src/env/mod.rs tests/
git commit -m "refactor(ast): Rc-wrap FunctionDef body to eliminate AST clones on function calls"
```

### Task 8: ExpandedField bitset migration

**Files:**
- Modify: `src/expand/mod.rs` (ExpandedField struct + methods + tests)
- Modify: `src/expand/field_split.rs` (lines 75, 90, 167)
- Modify: `src/expand/pathname.rs` (lines 26, 46)

- [ ] **Step 1: Replace ExpandedField internals in expand/mod.rs**

Replace the `ExpandedField` struct and its methods (lines 18-61 of `src/expand/mod.rs`):

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ExpandedField {
    pub value: String,
    /// Packed bitset: 1 bit per byte of `value`. Bit set = quoted (protected).
    quoted_mask: Vec<u64>,
    pub was_quoted: bool,
}

impl ExpandedField {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            quoted_mask: Vec::new(),
            was_quoted: false,
        }
    }

    /// Check if byte at `byte_index` is quoted (protected from splitting/glob).
    pub fn is_quoted(&self, byte_index: usize) -> bool {
        let word = byte_index / 64;
        let bit = byte_index % 64;
        self.quoted_mask
            .get(word)
            .map_or(false, |w| w & (1u64 << bit) != 0)
    }

    /// Append `s` marking each byte as **quoted** (protected).
    pub fn push_quoted(&mut self, s: &str) {
        let start = self.value.len();
        self.value.push_str(s);
        self.set_range(start, s.len(), true);
        self.was_quoted = true;
    }

    /// Append `s` marking each byte as **unquoted** (splittable/globbable).
    pub fn push_unquoted(&mut self, s: &str) {
        let start = self.value.len();
        self.value.push_str(s);
        self.set_range(start, s.len(), false);
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// Create a field with all bytes marked as quoted.
    pub fn all_quoted(value: String) -> Self {
        let len = value.len();
        let needed_words = (len + 63) / 64;
        let mask = vec![u64::MAX; needed_words];
        Self {
            value,
            quoted_mask: mask,
            was_quoted: false,
        }
    }

    fn set_range(&mut self, start: usize, len: usize, quoted: bool) {
        if len == 0 {
            return;
        }
        let end = start + len;
        let needed_words = (end + 63) / 64;
        self.quoted_mask.resize(needed_words, 0);
        if quoted {
            for i in start..end {
                self.quoted_mask[i / 64] |= 1u64 << (i % 64);
            }
        }
        // unquoted: bits are already 0 from resize
    }
}

impl Default for ExpandedField {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 2: Update field_split.rs to use is_quoted()**

In `src/expand/field_split.rs`:

Line 75 — remove the mask variable:
```rust
// Remove: let mask = &field.quoted_mask;
```

Line 90 — change direct indexing:
```rust
// Change from: let quoted = mask[i];
// To:
let quoted = field.is_quoted(i);
```

Line 167 — change direct indexing in `append_byte`:
```rust
// Change from: if source.quoted_mask[i] {
// To:
if source.is_quoted(i) {
```

- [ ] **Step 3: Update pathname.rs to use is_quoted() and all_quoted()**

In `src/expand/pathname.rs`:

Line 26 — change glob match field construction:
```rust
// Change from:
result.push(ExpandedField {
    quoted_mask: vec![true; m.len()],
    value: m,
    was_quoted: false,
});
// To:
result.push(ExpandedField::all_quoted(m));
```

Line 46 — change direct indexing:
```rust
// Change from: if !field.quoted_mask[i] && matches!(b, b'*' | b'?' | b'[') {
// To:
if !field.is_quoted(i) && matches!(b, b'*' | b'?' | b'[') {
```

- [ ] **Step 4: Update tests in expand/mod.rs**

Lines 579-581 — change `.quoted_mask.iter().all(|&q| !q)` assertions:
```rust
// Change from:
assert!(fields[0].quoted_mask.iter().all(|&q| !q));
// To:
assert!((0..fields[0].value.len()).all(|i| !fields[0].is_quoted(i)));
```

Apply the same pattern to all three assertions.

- [ ] **Step 5: Run tests**

Run: `cargo test && sh e2e/run_tests.sh`
Expected: All tests pass.

- [ ] **Step 6: Run benchmarks and compare**

Run: `cargo bench --bench expand_bench`
Expected: No regression; possible improvement in `expand_field_split`.

- [ ] **Step 7: Commit**

```bash
git add src/expand/mod.rs src/expand/field_split.rs src/expand/pathname.rs
git commit -m "refactor(expand): replace Vec<bool> quoted_mask with packed bitset (8x memory reduction)"
```

### Task 9: Unified error type — restructure ShellErrorKind

**Files:**
- Modify: `src/error.rs`
- Modify: `src/lexer/mod.rs` (update ShellErrorKind references)
- Modify: `src/parser/mod.rs` (update ShellErrorKind references)
- Modify: `src/interactive/parse_status.rs` (if it matches on ShellErrorKind)

- [ ] **Step 1: Restructure error.rs**

Replace the content of `src/error.rs`:

```rust
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct ShellError {
    pub kind: ShellErrorKind,
    pub message: String,
    pub location: Option<SourceLocation>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SourceLocation {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShellErrorKind {
    Parse(ParseErrorKind),
    Expansion(ExpansionErrorKind),
    Runtime(RuntimeErrorKind),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParseErrorKind {
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExpansionErrorKind {
    DivisionByZero,
    UnsetVariable,
    ParameterError,
    InvalidArithmetic,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RuntimeErrorKind {
    CommandNotFound,
    PermissionDenied,
    RedirectFailed,
    ReadonlyVariable,
    InvalidOption,
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.location {
            Some(loc) => write!(f, "kish: line {}: {}", loc.line, self.message),
            None => write!(f, "kish: {}", self.message),
        }
    }
}

impl std::error::Error for ShellError {}

impl ShellError {
    pub fn parse(kind: ParseErrorKind, line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            kind: ShellErrorKind::Parse(kind),
            message: message.into(),
            location: Some(SourceLocation { line, column }),
        }
    }

    pub fn expansion(kind: ExpansionErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind: ShellErrorKind::Expansion(kind),
            message: message.into(),
            location: None,
        }
    }

    pub fn runtime(kind: RuntimeErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind: ShellErrorKind::Runtime(kind),
            message: message.into(),
            location: None,
        }
    }
}

pub type Result<T> = std::result::Result<T, ShellError>;
```

- [ ] **Step 2: Update lexer error construction**

In `src/lexer/mod.rs`, find all `ShellError::new(ShellErrorKind::Xxx, ...)` and change to `ShellError::parse(ParseErrorKind::Xxx, ...)`.

Example — find:
```rust
ShellError::new(ShellErrorKind::UnterminatedSingleQuote, line, col, msg)
```
Replace with:
```rust
ShellError::parse(ParseErrorKind::UnterminatedSingleQuote, line, col, msg)
```

Update the import at the top of `src/lexer/mod.rs`:
```rust
// Change from:
use crate::error::{self, ShellError, ShellErrorKind};
// To:
use crate::error::{self, ShellError, ParseErrorKind};
```

- [ ] **Step 3: Update parser error construction**

Same pattern as Step 2 in `src/parser/mod.rs`. Change all `ShellError::new(ShellErrorKind::Xxx, ...)` to `ShellError::parse(ParseErrorKind::Xxx, ...)`.

Update the import similarly.

- [ ] **Step 4: Update interactive/parse_status.rs**

In `src/interactive/parse_status.rs`, find any `match` on `ShellErrorKind` variants and update to match the new nested structure:

```rust
// Change from:
ShellErrorKind::UnterminatedSingleQuote => ...
// To:
ShellErrorKind::Parse(ParseErrorKind::UnterminatedSingleQuote) => ...
```

- [ ] **Step 5: Update error.rs test**

Update the test in `src/error.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_with_location() {
        let err = ShellError::parse(
            ParseErrorKind::UnexpectedToken,
            5,
            10,
            "unexpected ')'",
        );
        assert_eq!(err.to_string(), "kish: line 5: unexpected ')'");
    }

    #[test]
    fn test_error_display_without_location() {
        let err = ShellError::runtime(
            RuntimeErrorKind::CommandNotFound,
            "foo: not found",
        );
        assert_eq!(err.to_string(), "kish: foo: not found");
    }
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test && sh e2e/run_tests.sh`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/error.rs src/lexer/mod.rs src/parser/mod.rs src/interactive/parse_status.rs
git commit -m "refactor(error): restructure ShellErrorKind into Parse/Expansion/Runtime categories"
```

### Task 9b: Expansion error migration (expansion_error flag → Result)

**Files:**
- Modify: `src/expand/mod.rs` (3 sites that set `expansion_error = true`)
- Modify: `src/expand/arith.rs` (error reporting)
- Modify: `src/exec/mod.rs` (expansion_error check in exec_simple_command, lines 404-405)

- [ ] **Step 1: Change expand_word and expand_words to return Result**

In `src/expand/mod.rs`, change the public API signatures:

```rust
// Change from:
pub fn expand_word(env: &mut ShellEnv, word: &Word) -> Vec<String>
pub fn expand_words(env: &mut ShellEnv, words: &[Word]) -> Vec<String>
pub fn expand_word_to_string(env: &mut ShellEnv, word: &Word) -> String

// To:
pub fn expand_word(env: &mut ShellEnv, word: &Word) -> crate::error::Result<Vec<String>>
pub fn expand_words(env: &mut ShellEnv, words: &[Word]) -> crate::error::Result<Vec<String>>
pub fn expand_word_to_string(env: &mut ShellEnv, word: &Word) -> crate::error::Result<String>
```

- [ ] **Step 2: Replace expansion_error flag with Result propagation in expand/mod.rs**

In `expand_part_to_fields` (arithmetic expansion error handling, around line 394):

```rust
// Change from:
Err(_) => {
    env.last_exit_status = 1;
    env.expansion_error = true;
    let zero = "0";
    // ...
}

// To: propagate the error
Err(msg) => {
    return Err(crate::error::ShellError::expansion(
        crate::error::ExpansionErrorKind::InvalidArithmetic,
        msg,
    ));
}
```

Apply same change to `expand_heredoc_string` (line 182) and `expand_heredoc_part` (line 291). Note: heredoc expansion errors should NOT propagate (POSIX: heredoc expansion errors are non-fatal). For those two sites, keep the `eprintln!` + status approach:

```rust
// In expand_heredoc_string and expand_heredoc_part, replace flag with direct status set:
Err(msg) => {
    eprintln!("kish: arithmetic: {}", msg);
    env.exec.last_exit_status = 1;
    out.push_str("0");
}
```

- [ ] **Step 3: Update expand_word_to_fields to return Result**

Change the internal function to propagate errors from `expand_part_to_fields`:

```rust
fn expand_word_to_fields(env: &mut ShellEnv, word: &Word) -> crate::error::Result<Vec<ExpandedField>> {
    let mut fields = vec![ExpandedField::new()];
    for part in &word.parts {
        expand_part_to_fields(env, part, &mut fields, false)?;
    }
    Ok(fields)
}
```

Update `expand_part_to_fields` return type to `crate::error::Result<()>`.

- [ ] **Step 4: Update callers in exec to handle Result**

In `src/exec/mod.rs`, in `exec_simple_command` (around line 404), replace the `expansion_error` flag check with `Result` handling:

```rust
// Change from:
let expanded = expand_words(&mut self.env, &cmd.words);
if self.env.expansion_error {
    self.env.expansion_error = false;
    self.env.last_exit_status = 1;
    return 1;
}

// To:
let expanded = match expand_words(&mut self.env, &cmd.words) {
    Ok(words) => words,
    Err(e) => {
        eprintln!("{}", e);
        self.env.exec.last_exit_status = 1;
        return 1;
    }
};
```

Find all other callers of `expand_word`, `expand_words`, `expand_word_to_string` in exec/, builtin/, and update them to handle `Result`.

- [ ] **Step 5: Remove expansion_error field from ExecState/ShellEnv**

After all callers are migrated, remove the `expansion_error` field from the ShellEnv (or ExecState after Task 10). Also remove the initialization in `ShellEnv::new()` and `command_sub.rs`.

- [ ] **Step 6: Run tests**

Run: `cargo test && sh e2e/run_tests.sh`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/expand/ src/exec/ src/env/ src/builtin/
git commit -m "refactor(expand): replace expansion_error flag with Result propagation"
```

> **Note on runtime error migration (eprintln → Result):** The spec calls for migrating ~90 `eprintln!` call sites in exec/builtin to use `Result<i32, ShellError>`. This is a large change that alters function signatures across the entire executor and builtin modules. It is deferred to a follow-up plan to keep this plan's scope manageable. The new `ShellErrorKind::Runtime` variants are defined and ready for use; the migration can proceed incrementally.

### Task 10: ShellEnv decomposition — introduce sub-structs

**Files:**
- Modify: `src/env/mod.rs`

- [ ] **Step 1: Define ExecState, ProcessState, ShellMode sub-structs**

In `src/env/mod.rs`, add the three sub-structs before `ShellEnv`:

```rust
/// Execution-related state.
#[derive(Debug, Clone)]
pub struct ExecState {
    pub last_exit_status: i32,
    pub flow_control: Option<FlowControl>,
    pub expansion_error: bool,
}

/// Process and job management state.
#[derive(Debug, Clone)]
pub struct ProcessState {
    pub shell_pid: Pid,
    pub shell_pgid: Pid,
    pub jobs: JobTable,
}

/// Shell mode and option flags.
#[derive(Debug, Clone)]
pub struct ShellMode {
    pub options: ShellOptions,
    pub is_interactive: bool,
    pub in_dot_script: bool,
}
```

- [ ] **Step 2: Update ShellEnv to use sub-structs**

```rust
pub struct ShellEnv {
    pub vars: VarStore,
    pub exec: ExecState,
    pub process: ProcessState,
    pub mode: ShellMode,
    pub functions: HashMap<String, FunctionDef>,
    pub traps: TrapStore,
    pub aliases: AliasStore,
    pub shell_name: String,
}
```

- [ ] **Step 3: Update ShellEnv::new()**

```rust
impl ShellEnv {
    pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
        let mut vars = VarStore::from_environ();
        vars.set_positional_params(args);
        ShellEnv {
            vars,
            exec: ExecState {
                last_exit_status: 0,
                flow_control: None,
                expansion_error: false,
            },
            process: ProcessState {
                shell_pid: getpid(),
                shell_pgid: nix::unistd::getpgrp(),
                jobs: JobTable::default(),
            },
            mode: ShellMode {
                options: ShellOptions::default(),
                is_interactive: false,
                in_dot_script: false,
            },
            functions: HashMap::new(),
            traps: TrapStore::default(),
            aliases: AliasStore::default(),
            shell_name: shell_name.into(),
        }
    }
}
```

- [ ] **Step 4: Mechanical replacement across codebase**

This is the largest step. Replace all field accesses throughout the codebase:

| Old | New |
|-----|-----|
| `env.last_exit_status` | `env.exec.last_exit_status` |
| `env.flow_control` | `env.exec.flow_control` |
| `env.expansion_error` | `env.exec.expansion_error` |
| `env.shell_pid` | `env.process.shell_pid` |
| `env.shell_pgid` | `env.process.shell_pgid` |
| `env.jobs` | `env.process.jobs` |
| `env.options` | `env.mode.options` |
| `env.is_interactive` | `env.mode.is_interactive` |
| `env.in_dot_script` | `env.mode.in_dot_script` |

Files to update (use search-and-replace):
- `src/exec/mod.rs` — heaviest usage (27+ `last_exit_status`, 30+ `flow_control`, 9 `options`)
- `src/exec/pipeline.rs`
- `src/exec/redirect.rs`
- `src/expand/mod.rs` — `expansion_error`, `last_exit_status`
- `src/expand/arith.rs` — `expansion_error`, `last_exit_status`
- `src/builtin/mod.rs`
- `src/builtin/special.rs`
- `src/signal.rs`
- `src/interactive/mod.rs`
- `src/main.rs`
- All test files that construct or access ShellEnv fields

For the `Executor` struct: `self.env.last_exit_status` → `self.env.exec.last_exit_status`, etc.

- [ ] **Step 5: Update tests in env/mod.rs**

Update the `test_shell_env_construction` test and others to use the new field paths:

```rust
#[test]
fn test_shell_env_construction() {
    let env = ShellEnv::new("kish", vec!["arg1".to_string(), "arg2".to_string()]);
    assert_eq!(env.shell_name, "kish");
    assert_eq!(env.vars.positional_params(), &["arg1", "arg2"]);
    assert_eq!(env.exec.last_exit_status, 0);
    assert!(env.process.shell_pid.as_raw() > 0);
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test && sh e2e/run_tests.sh`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor(env): decompose ShellEnv into ExecState/ProcessState/ShellMode sub-structs"
```

### Task 11: VarStore optimization — fast path and vars_iter

**Files:**
- Modify: `src/env/vars.rs`

- [ ] **Step 1: Add fast path to get()**

In `src/env/vars.rs`, replace the `get` method (lines 114-121):

```rust
pub fn get(&self, name: &str) -> Option<&str> {
    // Fast path: single scope (most common — outside function calls)
    if self.scopes.len() == 1 {
        return self.scopes[0].vars.get(name).map(|v| v.value.as_str());
    }
    for scope in self.scopes.iter().rev() {
        if let Some(var) = scope.vars.get(name) {
            return Some(var.value.as_str());
        }
    }
    None
}
```

- [ ] **Step 2: Add fast path to set()**

Similarly, add the fast path to `set()` (lines 140-165):

```rust
pub fn set(&mut self, name: &str, value: impl Into<String>) -> Result<(), String> {
    let value = value.into();

    // Fast path: single scope
    if self.scopes.len() == 1 {
        if let Some(existing) = self.scopes[0].vars.get(name) {
            if existing.readonly {
                return Err(format!("{}: readonly variable", name));
            }
            let exported = existing.exported;
            self.scopes[0].vars.insert(
                name.to_string(),
                Variable { value, exported, readonly: false },
            );
        } else {
            self.scopes[0].vars.insert(name.to_string(), Variable::new(value));
        }
        return Ok(());
    }

    // Multi-scope path: walk from top to bottom
    for scope in self.scopes.iter_mut().rev() {
        if let Some(existing) = scope.vars.get(name) {
            if existing.readonly {
                return Err(format!("{}: readonly variable", name));
            }
            let exported = existing.exported;
            scope.vars.insert(
                name.to_string(),
                Variable { value, exported, readonly: false },
            );
            return Ok(());
        }
    }

    self.scopes[0].vars.insert(name.to_string(), Variable::new(value));
    Ok(())
}
```

- [ ] **Step 3: Replace vars_iter() with lazy shadowed iterator**

Replace the `vars_iter` method (lines 263-271):

```rust
pub fn vars_iter(&self) -> impl Iterator<Item = (&str, &Variable)> {
    let mut seen = std::collections::HashSet::new();
    self.scopes
        .iter()
        .rev()
        .flat_map(|s| s.vars.iter())
        .filter_map(move |(k, v)| {
            if seen.insert(k.as_str()) {
                Some((k.as_str(), v))
            } else {
                None
            }
        })
}
```

- [ ] **Step 4: Add environ cache to VarStore**

Add the cache field and update methods:

```rust
pub struct VarStore {
    scopes: Vec<Scope>,
    environ_cache: Option<Vec<(String, String)>>,
}
```

Update `new()` and `from_environ()` to initialize `environ_cache: None`.

Update all mutating methods (`set`, `set_with_options`, `unset`, `export`, `set_readonly`, `push_scope`, `pop_scope`) to invalidate the cache:
```rust
self.environ_cache = None;
```

Replace `to_environ()`:
```rust
pub fn to_environ(&mut self) -> &[(String, String)] {
    if self.environ_cache.is_none() {
        self.environ_cache = Some(self.build_environ());
    }
    self.environ_cache.as_ref().unwrap()
}

fn build_environ(&self) -> Vec<(String, String)> {
    let mut merged: HashMap<String, &Variable> = HashMap::new();
    for scope in &self.scopes {
        for (name, var) in &scope.vars {
            merged.insert(name.clone(), var);
        }
    }
    merged
        .into_iter()
        .filter(|(_, v)| v.exported)
        .map(|(k, v)| (k, v.value.clone()))
        .collect()
}
```

- [ ] **Step 5: Update callers of to_environ()**

Find all callers of `to_environ()` (likely in `src/exec/command.rs` or `src/exec/mod.rs`) and update the call to pass `&mut self.env.vars` instead of `&self.env.vars`, since `to_environ` now takes `&mut self`.

- [ ] **Step 6: Fix test for `from_environ`**

The test `test_from_environ` (line 343) accesses `store.scopes[0]` which is private. Update to use the public API:

```rust
#[test]
fn test_from_environ() {
    let store = VarStore::from_environ();
    // Verify at least some environment variables are present and exported
    // (PATH is almost always set)
    if let Some(var) = store.get_var("PATH") {
        assert!(var.exported, "Variables from environ should be exported");
    }
}
```

- [ ] **Step 7: Run tests**

Run: `cargo test && sh e2e/run_tests.sh`
Expected: All tests pass.

- [ ] **Step 8: Run benchmarks**

Run: `cargo bench`
Expected: Improvement visible in expansion benchmarks (variable lookups are faster).

- [ ] **Step 9: Commit**

```bash
git add src/env/vars.rs src/exec/
git commit -m "perf(vars): add fast path for single-scope lookup, lazy vars_iter, environ cache"
```

---

## Phase 2: Module Boundary Reorganization

### Task 12: Extract env/traps.rs from env/mod.rs

**Files:**
- Create: `src/env/traps.rs`
- Modify: `src/env/mod.rs`

- [ ] **Step 1: Create src/env/traps.rs**

Move `TrapAction` enum and `TrapStore` struct with all its methods (lines 15-155 of `src/env/mod.rs`) to a new file `src/env/traps.rs`:

```rust
use std::collections::HashMap;

/// Action to take when a trap fires.
#[derive(Debug, Clone, PartialEq)]
pub enum TrapAction {
    Default,
    Ignore,
    Command(String),
}

/// Storage for shell trap settings.
#[derive(Debug, Clone, Default)]
pub struct TrapStore {
    pub exit_trap: Option<TrapAction>,
    pub signal_traps: HashMap<i32, TrapAction>,
    saved_traps: Option<Box<(Option<TrapAction>, HashMap<i32, TrapAction>)>>,
}

impl TrapStore {
    // ... all existing methods unchanged
}
```

- [ ] **Step 2: Update env/mod.rs**

Add `pub mod traps;` and replace the removed code with `pub use traps::{TrapAction, TrapStore};`.

Remove the TrapStore tests from `env/mod.rs` and move them to `src/env/traps.rs` as a `#[cfg(test)] mod tests` block.

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/env/traps.rs src/env/mod.rs
git commit -m "refactor(env): extract TrapStore into env/traps.rs"
```

### Task 13: Extract env/exec_state.rs and env/shell_mode.rs

**Files:**
- Create: `src/env/exec_state.rs`
- Create: `src/env/shell_mode.rs`
- Modify: `src/env/mod.rs`

- [ ] **Step 1: Create src/env/exec_state.rs**

```rust
/// Flow control signals for break, continue, and return.
#[derive(Debug, Clone, PartialEq)]
pub enum FlowControl {
    Break(usize),
    Continue(usize),
    Return(i32),
}

/// Execution-related state.
#[derive(Debug, Clone)]
pub struct ExecState {
    pub last_exit_status: i32,
    pub flow_control: Option<FlowControl>,
    pub expansion_error: bool,
}
```

- [ ] **Step 2: Create src/env/shell_mode.rs**

Move `ShellOptions` struct and all its methods, plus `ShellMode`:

```rust
/// POSIX shell option flags (set -o / set +o).
#[derive(Debug, Clone, Default)]
pub struct ShellOptions {
    // ... all existing fields
}

impl ShellOptions {
    // ... all existing methods unchanged
}

/// Shell mode and option flags.
#[derive(Debug, Clone)]
pub struct ShellMode {
    pub options: ShellOptions,
    pub is_interactive: bool,
    pub in_dot_script: bool,
}
```

Move the `ShellOptions` tests into this file.

- [ ] **Step 3: Update env/mod.rs**

```rust
pub mod aliases;
pub mod exec_state;
pub mod jobs;
pub mod shell_mode;
pub mod traps;
pub mod vars;

pub use exec_state::{ExecState, FlowControl};
pub use shell_mode::{ShellMode, ShellOptions};
pub use traps::{TrapAction, TrapStore};

// ShellEnv struct and impl remain here
```

- [ ] **Step 4: Run tests**

Run: `cargo test && sh e2e/run_tests.sh`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/env/exec_state.rs src/env/shell_mode.rs src/env/mod.rs
git commit -m "refactor(env): extract ExecState and ShellMode into separate files"
```

### Task 14: Split lexer/mod.rs — extract scanner.rs

**Files:**
- Create: `src/lexer/scanner.rs`
- Modify: `src/lexer/mod.rs`

- [ ] **Step 1: Create src/lexer/scanner.rs**

Move the following methods from `impl Lexer` in `mod.rs` to `src/lexer/scanner.rs`:

```rust
use crate::error::{self, ShellError, ParseErrorKind};
use crate::lexer::token::{Span, SpannedToken, Token};
use super::Lexer;

impl Lexer {
    pub(crate) fn at_end(&self) -> bool { ... }
    pub(crate) fn current_byte(&self) -> u8 { ... }
    pub(crate) fn peek_byte(&self) -> u8 { ... }
    pub(crate) fn advance(&mut self) -> u8 { ... }
    pub(crate) fn current_span(&self) -> Span { ... }
    pub(crate) fn skip_whitespace_and_comments(&mut self) { ... }
    pub(crate) fn next_token_raw(&mut self) -> error::Result<SpannedToken> { ... }
    pub(crate) fn read_pipe(&mut self) -> error::Result<SpannedToken> { ... }
    pub(crate) fn read_amp(&mut self) -> error::Result<SpannedToken> { ... }
    pub(crate) fn read_semi(&mut self) -> error::Result<SpannedToken> { ... }
    pub(crate) fn read_less(&mut self) -> error::Result<SpannedToken> { ... }
    pub(crate) fn read_great(&mut self) -> error::Result<SpannedToken> { ... }
    pub(crate) fn is_meta_or_whitespace(ch: u8) -> bool { ... }
    pub(crate) fn try_read_io_number(&mut self) -> Option<Token> { ... }
}
```

Change method visibility from `fn` to `pub(crate) fn` so they're accessible from sibling modules within the lexer crate.

- [ ] **Step 2: Add module declaration and verify**

In `src/lexer/mod.rs`, add:
```rust
mod scanner;
```

Remove the moved methods from `mod.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/lexer/scanner.rs src/lexer/mod.rs
git commit -m "refactor(lexer): extract scanner methods into lexer/scanner.rs"
```

### Task 15: Split lexer/mod.rs — extract word.rs

**Files:**
- Create: `src/lexer/word.rs`
- Modify: `src/lexer/mod.rs`

- [ ] **Step 1: Create src/lexer/word.rs**

Move the following methods to `src/lexer/word.rs`:

```rust
use crate::error::{self, ShellError, ParseErrorKind};
use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};
use crate::lexer::token::{Span, SpannedToken, Token};
use super::Lexer;

impl Lexer {
    // Word construction
    pub fn read_word_parts(...) -> error::Result<Vec<WordPart>> { ... }
    pub(crate) fn read_word(&mut self) -> error::Result<SpannedToken> { ... }

    // Quoting
    pub(crate) fn read_single_quote(&mut self) -> error::Result<WordPart> { ... }
    pub(crate) fn read_double_quote(&mut self) -> error::Result<WordPart> { ... }
    pub(crate) fn read_backslash(&mut self) -> error::Result<WordPart> { ... }
    pub(crate) fn read_backslash_in_double_quote(&mut self) -> error::Result<WordPart> { ... }
    pub(crate) fn read_tilde(&mut self) -> WordPart { ... }

    // Identifiers and parameter names
    pub(crate) fn read_name(&mut self) -> String { ... }
    pub(crate) fn classify_param_name(&self, name: &str) -> ParamExpr { ... }
    pub(crate) fn read_param_name(&mut self, span: Span) -> error::Result<String> { ... }
    pub(crate) fn read_word_in_brace(&mut self, span: Span) -> error::Result<Word> { ... }
    pub(crate) fn expect_byte(&mut self, expected: u8, span: Span) -> error::Result<()> { ... }

    // Parameter expansion
    pub(crate) fn read_param_expansion_braced(&mut self) -> error::Result<WordPart> { ... }
    pub(crate) fn read_param_operator(&mut self, span: Span, name: String) -> error::Result<WordPart> { ... }
    pub(crate) fn read_conditional_param(&mut self, span: Span, name: String, null_check: bool) -> error::Result<WordPart> { ... }

    // Dollar expansion & command substitution
    pub(crate) fn read_dollar_single_quote(&mut self) -> error::Result<WordPart> { ... }
    pub(crate) fn read_hex_digits(&mut self, max: usize) -> u8 { ... }
    pub(crate) fn read_command_sub_dollar(&mut self) -> error::Result<WordPart> { ... }
    pub(crate) fn read_arith_expansion(&mut self) -> error::Result<WordPart> { ... }
    pub(crate) fn read_balanced_parens(&mut self, span: Span) -> error::Result<String> { ... }
    pub(crate) fn read_dollar(&mut self) -> error::Result<WordPart> { ... }
    pub(crate) fn read_backtick(&mut self) -> error::Result<WordPart> { ... }
}
```

- [ ] **Step 2: Add module declaration**

In `src/lexer/mod.rs`, add:
```rust
mod word;
```

Remove the moved methods from `mod.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test && sh e2e/run_tests.sh`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/lexer/word.rs src/lexer/mod.rs
git commit -m "refactor(lexer): extract word/quoting/expansion methods into lexer/word.rs"
```

### Task 16: Split lexer/mod.rs — extract heredoc.rs and alias.rs

**Files:**
- Create: `src/lexer/heredoc.rs`
- Create: `src/lexer/alias.rs`
- Modify: `src/lexer/mod.rs`

- [ ] **Step 1: Create src/lexer/heredoc.rs**

Move heredoc methods:

```rust
use crate::error;
use crate::parser::ast::WordPart;
use super::{Lexer, PendingHereDoc};

impl Lexer {
    pub fn register_heredoc(&mut self, delimiter: String, quoted: bool, strip_tabs: bool) { ... }
    pub fn take_heredoc_body(&mut self) -> Option<Vec<WordPart>> { ... }
    pub fn process_pending_heredocs(&mut self) -> error::Result<()> { ... }
    pub fn has_pending_heredocs(&self) -> bool { ... }
    fn read_heredoc_body(&mut self, hd: &PendingHereDoc) -> error::Result<Vec<WordPart>> { ... }
}
```

- [ ] **Step 2: Create src/lexer/alias.rs**

Move alias methods:

```rust
use crate::error;
use crate::lexer::token::{SpannedToken, Token};
use super::Lexer;

impl Lexer {
    pub fn next_token(&mut self) -> error::Result<SpannedToken> { ... }
    fn update_check_alias_after(&mut self, token: &Token) { ... }
}
```

- [ ] **Step 3: Add module declarations**

In `src/lexer/mod.rs`, add:
```rust
mod alias;
mod heredoc;
```

Remove the moved methods.

- [ ] **Step 4: Verify mod.rs is now ~150 lines**

`src/lexer/mod.rs` should now contain only:
- Struct definitions (`Lexer`, `LexerState`, `PendingHereDoc`)
- Free functions (`is_name_start`, `is_name_char`)
- Constructor and state methods (`new`, `new_with_aliases`, `position`, `save_state`, `restore_state`)
- Module declarations
- Tests

- [ ] **Step 5: Run tests**

Run: `cargo test && sh e2e/run_tests.sh`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/lexer/heredoc.rs src/lexer/alias.rs src/lexer/mod.rs
git commit -m "refactor(lexer): extract heredoc and alias methods into separate files"
```

### Task 17: Split exec/mod.rs — extract compound.rs

**Files:**
- Create: `src/exec/compound.rs`
- Modify: `src/exec/mod.rs`

- [ ] **Step 1: Create src/exec/compound.rs**

Move compound command execution methods:

```rust
use crate::env::FlowControl;
use crate::expand::expand_words;
use crate::parser::ast::{CaseItem, CaseTerminator, CompoundCommand, CompoundCommandKind, CompleteCommand, Word};
use super::Executor;

impl Executor {
    pub(crate) fn exec_compound_command(&mut self, compound: &CompoundCommand, redirects: &[crate::parser::ast::Redirect]) -> i32 { ... }
    pub(crate) fn exec_body(&mut self, body: &[CompleteCommand]) -> i32 { ... }
    pub(crate) fn exec_brace_group(&mut self, body: &[CompleteCommand]) -> i32 { ... }
    pub(crate) fn exec_subshell(&mut self, body: &[CompleteCommand]) -> i32 { ... }
    pub(crate) fn exec_if(&mut self, condition: &[CompleteCommand], then_part: &[CompleteCommand], elif_parts: &[(Vec<CompleteCommand>, Vec<CompleteCommand>)], else_part: &Option<Vec<CompleteCommand>>) -> i32 { ... }
    pub(crate) fn exec_loop(&mut self, condition: &[CompleteCommand], body: &[CompleteCommand], until: bool) -> i32 { ... }
    pub(crate) fn exec_for(&mut self, var: &str, words: &Option<Vec<Word>>, body: &[CompleteCommand]) -> i32 { ... }
    pub(crate) fn exec_case(&mut self, word: &Word, items: &[CaseItem]) -> i32 { ... }
}
```

- [ ] **Step 2: Add module declaration**

In `src/exec/mod.rs`, add:
```rust
mod compound;
```

Remove the moved methods.

- [ ] **Step 3: Run tests**

Run: `cargo test && sh e2e/run_tests.sh`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/exec/compound.rs src/exec/mod.rs
git commit -m "refactor(exec): extract compound command execution into exec/compound.rs"
```

### Task 18: Split exec/mod.rs — extract simple.rs and function.rs

**Files:**
- Create: `src/exec/simple.rs`
- Create: `src/exec/function.rs`
- Modify: `src/exec/mod.rs`

- [ ] **Step 1: Create src/exec/function.rs with panic-safe scope guard**

```rust
use crate::env::FlowControl;
use crate::parser::ast::FunctionDef;
use super::Executor;

impl Executor {
    pub(crate) fn exec_function_call(&mut self, func_def: &FunctionDef, args: &[String]) -> i32 {
        self.env.vars.push_scope(args.to_vec());

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.exec_compound_command(&func_def.body, &func_def.redirects)
        }));

        self.env.vars.pop_scope();

        let status = match result {
            Ok(s) => s,
            Err(payload) => std::panic::resume_unwind(payload),
        };

        // Handle return flow control
        let final_status = match self.env.exec.flow_control.take() {
            Some(FlowControl::Return(s)) => s,
            Some(other) => {
                self.env.exec.flow_control = Some(other);
                status
            }
            None => status,
        };

        self.env.exec.last_exit_status = final_status;
        final_status
    }
}
```

- [ ] **Step 2: Create src/exec/simple.rs**

Move simple command execution methods:

```rust
use std::ffi::CString;
use nix::unistd::{execvp, fork, ForkResult};
use crate::builtin::{classify_builtin, exec_regular_builtin, BuiltinKind};
use crate::builtin::special::exec_special_builtin;
use crate::expand::expand_words;
use crate::parser::ast::{Assignment, Redirect, SimpleCommand, Word};
use super::Executor;
use super::command::wait_child;
use super::redirect::RedirectState;

impl Executor {
    pub(crate) fn exec_simple_command(&mut self, cmd: &SimpleCommand) -> i32 { ... }
    pub(crate) fn build_env_vars(&mut self, assignments: &[Assignment]) -> Vec<(String, String)> { ... }
    pub(crate) fn exec_external_with_redirects(&mut self, cmd: &str, args: &[String], env_vars: Vec<(String, String)>, redirects: &[Redirect]) -> i32 { ... }
    pub(crate) fn apply_temp_assignments(&mut self, assignments: &[Assignment]) -> Vec<(String, Option<String>)> { ... }
    pub(crate) fn restore_assignments(&mut self, saved: Vec<(String, Option<String>)>) { ... }
}
```

- [ ] **Step 3: Add module declarations**

In `src/exec/mod.rs`, add:
```rust
mod compound;  // already added
mod function;
mod simple;
```

Remove the moved methods.

- [ ] **Step 4: Verify exec/mod.rs is now ~400 lines**

`src/exec/mod.rs` should contain:
- `Executor` struct definition and `new`/`from_env`
- Errexit methods (`with_errexit_suppressed`, `should_errexit`, `check_errexit`)
- Signal processing (`process_pending_signals`, `handle_default_signal`)
- `eval_string`, `verbose_print`
- `exec_command` (dispatch)
- `exec_complete_command`, `exec_program`
- `exec_and_or`, `exec_async`
- `reap_zombies`, `display_job_notifications`
- Job builtins (`builtin_wait`, `builtin_jobs`, `builtin_fg`, `builtin_bg`, etc.)
- Module declarations
- Tests

- [ ] **Step 5: Run tests**

Run: `cargo test && sh e2e/run_tests.sh`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/exec/simple.rs src/exec/function.rs src/exec/mod.rs
git commit -m "refactor(exec): extract simple command and function execution into separate files

Resolves TODO: exec_function_call now has panic safety via catch_unwind"
```

### Task 19: Extract builtin implementations from builtin/mod.rs

**Files:**
- Create: `src/builtin/regular.rs`
- Modify: `src/builtin/mod.rs`

- [ ] **Step 1: Create src/builtin/regular.rs**

Move all builtin implementation functions from `mod.rs`:

```rust
use crate::env::ShellEnv;

pub fn builtin_cd(args: &[String], env: &mut ShellEnv) -> i32 { ... }
pub fn builtin_echo(args: &[String]) -> i32 { ... }
pub fn builtin_alias(args: &[String], env: &mut ShellEnv) -> i32 { ... }
pub fn builtin_unalias(args: &[String], env: &mut ShellEnv) -> i32 { ... }
pub fn builtin_kill(args: &[String], env: &mut ShellEnv) -> i32 { ... }
fn parse_kill_signal(arg: &str) -> Result<i32, String> { ... }
fn kill_list() { ... }
pub fn builtin_umask(args: &[String]) -> i32 { ... }
fn umask_to_symbolic(mode: u32) -> String { ... }
fn umask_set_octal(s: &str) -> Result<(), String> { ... }
fn umask_set_symbolic(s: &str) -> Result<(), String> { ... }
```

- [ ] **Step 2: Update builtin/mod.rs to dispatch-only**

`src/builtin/mod.rs` should now contain only:

```rust
pub mod regular;
pub mod special;

use crate::env::ShellEnv;

pub enum BuiltinKind {
    Special,
    Regular,
    NotBuiltin,
}

pub fn classify_builtin(name: &str) -> BuiltinKind {
    // ... existing classification logic
}

pub fn exec_regular_builtin(name: &str, args: &[String], env: &mut ShellEnv) -> i32 {
    match name {
        "cd" => regular::builtin_cd(args, env),
        "echo" => regular::builtin_echo(args),
        "alias" => regular::builtin_alias(args, env),
        "unalias" => regular::builtin_unalias(args, env),
        "kill" => regular::builtin_kill(args, env),
        "umask" => regular::builtin_umask(args),
        _ => {
            eprintln!("kish: {}: unknown builtin", name);
            1
        }
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test && sh e2e/run_tests.sh`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/builtin/regular.rs src/builtin/mod.rs
git commit -m "refactor(builtin): extract regular builtin implementations into builtin/regular.rs"
```

### Task 20: Fix TODO.md known issues

**Files:**
- Modify: `src/builtin/regular.rs` (cd OLDPWD fix)
- Modify: `src/exec/redirect.rs` (DupOutput/DupInput guard)
- Modify: `src/builtin/special.rs` (builtin_source FlowControl fix)
- Modify: `TODO.md`

- [ ] **Step 1: Fix cd OLDPWD handling**

In `src/builtin/regular.rs`, in the `builtin_cd` function, move the OLDPWD save to after the successful directory change:

```rust
// Before: OLDPWD was set before set_current_dir
// After: save current dir first, only set OLDPWD after success
let old_cwd = std::env::current_dir().ok();
// ... (resolve target_dir) ...
match std::env::set_current_dir(&target_dir) {
    Ok(()) => {
        // Only update OLDPWD after successful chdir
        if let Some(old) = old_cwd {
            let _ = env.vars.set("OLDPWD", old.to_string_lossy().to_string());
        }
        // Update PWD
        if let Ok(new_pwd) = std::env::current_dir() {
            let _ = env.vars.set("PWD", new_pwd.to_string_lossy().to_string());
        }
        0
    }
    Err(e) => {
        eprintln!("kish: cd: {}: {}", target_dir.display(), e);
        1
    }
}
```

- [ ] **Step 2: Add fd == target_fd guard to DupOutput/DupInput**

In `src/exec/redirect.rs`, add guard to both DupOutput (line ~108) and DupInput (line ~126) handlers, before the `raw_dup2` call:

```rust
// Add after src_fd parsing, before raw_dup2:
if src_fd == target_fd {
    // No-op: fd already points to itself
} else {
    if save {
        self.save_fd(target_fd)?;
    }
    raw_dup2(src_fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
}
```

- [ ] **Step 3: Fix builtin_source FlowControl::Return handling**

In `src/builtin/special.rs`, in the `builtin_source` function (around line 370), move the FlowControl check inside the success path only:

```rust
// Change from:
// (runs unconditionally after both parse success and failure)
if let Some(FlowControl::Return(code)) = executor.env.exec.flow_control {
    executor.env.exec.flow_control = None;
    return code;
}

// To: only check FlowControl after successful exec_program
match crate::parser::Parser::new(&content).parse_program() {
    Ok(program) => {
        executor.exec_program(&program);
        // Only consume Return after successful execution
        if let Some(FlowControl::Return(code)) = executor.env.exec.flow_control {
            executor.env.exec.flow_control = None;
            return code;
        }
        executor.env.exec.last_exit_status
    }
    Err(e) => {
        eprintln!("{}", e);
        1
    }
}
```

- [ ] **Step 4: Update TODO.md — remove resolved items**

Remove the following resolved items from `TODO.md`:
- `cd` overwrites OLDPWD before `set_current_dir`
- `exec_function_call` lacks panic safety (resolved in Task 18)
- `VarStore::vars_iter()` rebuilds HashMap (resolved in Task 11)
- `DupOutput`/`DupInput` redirect kinds lack fd == target_fd guard
- `builtin_source` FlowControl::Return consumption

- [ ] **Step 5: Run tests**

Run: `cargo test && sh e2e/run_tests.sh`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/builtin/regular.rs src/exec/redirect.rs src/builtin/special.rs TODO.md
git commit -m "fix: resolve 5 TODO.md known issues (cd OLDPWD, dup guard, source return, function panic safety, vars_iter)"
```

---

## Phase 3: Cross-Cutting Improvements

### Task 21: Clone reduction — ownership transfer

**Files:**
- Modify: `src/exec/simple.rs`
- Modify: `src/exec/mod.rs`

- [ ] **Step 1: Eliminate command name clone**

In `src/exec/simple.rs`, in `exec_simple_command`, find where the command name is cloned from the expanded words vector:

```rust
// Change from:
let command_name = expanded[0].clone();
// To:
let command_name = expanded.remove(0);
let args = expanded; // remaining elements are arguments
```

Or alternatively:
```rust
let mut expanded = expanded.into_iter();
let command_name = expanded.next().unwrap();
let args: Vec<String> = expanded.collect();
```

- [ ] **Step 2: Audit remaining clones**

Search for `.clone()` across the codebase and verify each remaining clone is necessary:

Run: `grep -rn '\.clone()' src/ | grep -v test | grep -v '#\[' | wc -l`

For each clone, verify it falls into the "retained" category (variable values, trap commands).

- [ ] **Step 3: Run tests**

Run: `cargo test && sh e2e/run_tests.sh`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/exec/
git commit -m "perf: eliminate unnecessary String clones in command execution"
```

### Task 22: Final benchmark comparison and documentation

**Files:**
- Modify: `benches/baseline.txt` (append post-refactoring results)

- [ ] **Step 1: Run full benchmark suite**

Run: `cargo bench 2>&1 | tee benches/post-refactoring.txt`
Expected: All benchmarks complete.

- [ ] **Step 2: Compare results**

Review `benches/baseline.txt` vs `benches/post-refactoring.txt`. Document any significant changes (>5% improvement or regression).

- [ ] **Step 3: Run complete test suite**

Run: `cargo test && sh e2e/run_tests.sh`
Expected: All tests pass — no regressions from the entire refactoring.

- [ ] **Step 4: Commit**

```bash
git add benches/post-refactoring.txt
git commit -m "bench: record post-refactoring measurements

Phase 0-3 refactoring complete. All E2E tests passing."
```
