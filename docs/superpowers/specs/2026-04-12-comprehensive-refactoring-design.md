# Comprehensive Refactoring Design Spec

## Overview

A bottom-up refactoring of the kish POSIX shell focused on future-proofing (fish/zsh-level interactive features, scripting performance) and performance optimization. Breaking changes are allowed; E2E tests serve as the correctness standard.

**Approach**: Bottom-up — redesign data structures first, then rebuild module boundaries on top.

**Phases**:
- Phase 0: Benchmark infrastructure (baseline before changes)
- Phase 1: Core data structure redesign
- Phase 2: Module boundary reorganization
- Phase 3: Cross-cutting improvements (Clone reduction, future extension paths)

---

## Phase 0: Benchmark Infrastructure

Establish `criterion`-based benchmarks before any refactoring to capture baseline measurements and enable before/after comparison at each phase.

### Benchmark Suites

```
benches/
  lexer_bench.rs      Tokenization speed (small/medium/large scripts)
  parser_bench.rs     Parse speed (same scale)
  expand_bench.rs     Expansion pipeline (parameter expansion, field splitting)
  e2e_bench.rs        Representative script execution time
```

### Cargo.toml Addition

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

[[bench]]
name = "e2e_bench"
harness = false
```

### Benchmark Targets

| Benchmark | What It Measures | Input |
|-----------|-----------------|-------|
| `lex_small` | Tokenize ~10 line script | Simple command sequence |
| `lex_large` | Tokenize ~500 line script | Mixed control flow + expansion |
| `parse_small` | Parse ~10 line script | Same as lex |
| `parse_large` | Parse ~500 line script | Same as lex |
| `expand_param` | Parameter expansion throughput | 1000x ${var:-default} |
| `expand_field_split` | IFS splitting throughput | Long string with varied IFS |
| `e2e_loop` | Loop execution overhead | `i=0; while [ $i -lt 1000 ]; do i=$((i+1)); done` |
| `e2e_pipeline` | Pipeline setup cost | `echo x | cat | cat | cat` |

Run benchmarks at each phase boundary to verify no regressions.

---

## Phase 1: Core Data Structure Redesign

### 1.1 AST Improvements

**File**: `src/parser/ast.rs`

**Changes**:

1. **Type alias for command lists** — `Vec<CompleteCommand>` appears 7 times across compound command variants:

```rust
pub type CommandList = Vec<CompleteCommand>;
```

Applied to: `BraceGroup::body`, `Subshell::body`, `If::condition/then_part/else_part`, `For::body`, `While::condition/body`, `Until::condition/body`, `CaseItem::body`, `FunctionDef` body's inner commands.

2. **Rc-wrapped function bodies** — Eliminate full AST clones on every function call:

```rust
pub struct FunctionDef {
    pub name: String,
    pub body: Rc<CompoundCommand>,
    pub redirects: Vec<Redirect>,
}
```

This changes function registration from cloning the entire AST subtree to incrementing a reference count.

3. **No SmallVec/CompactString/interning at this stage** — Defer to post-benchmark evidence. `Vec` and `String` are retained for simplicity.

### 1.2 ExpandedField Bitset

**File**: `src/expand/mod.rs`

Replace `quoted_mask: Vec<bool>` (1 byte per source byte) with a packed bitset (1 bit per source byte = 8x memory reduction).

```rust
pub struct ExpandedField {
    pub value: String,
    quoted_mask: Vec<u64>,  // 64 source bytes tracked per u64 element
    pub was_quoted: bool,
}

impl ExpandedField {
    pub fn is_quoted(&self, byte_index: usize) -> bool {
        let word = byte_index / 64;
        let bit = byte_index % 64;
        self.quoted_mask.get(word).map_or(false, |w| w & (1 << bit) != 0)
    }

    pub fn push_quoted(&mut self, s: &str) {
        let start = self.value.len();
        self.value.push_str(s);
        self.set_range(start, s.len(), true);
        self.was_quoted = true;
    }

    pub fn push_unquoted(&mut self, s: &str) {
        let start = self.value.len();
        self.value.push_str(s);
        self.set_range(start, s.len(), false);
    }

    fn set_range(&mut self, start: usize, len: usize, quoted: bool) {
        let end = start + len;
        let needed_words = (end + 63) / 64;
        self.quoted_mask.resize(needed_words, 0);
        if quoted {
            for i in start..end {
                self.quoted_mask[i / 64] |= 1 << (i % 64);
            }
        }
    }
}
```

**Consumers to update**: `field_split.rs` and `pathname.rs` change `field.quoted_mask[i]` to `field.is_quoted(i)`.

### 1.3 Unified Error Type

**File**: `src/error.rs`

Restructure `ShellError` to cover parse, expansion, and runtime errors.

```rust
pub struct ShellError {
    pub kind: ShellErrorKind,
    pub message: String,
    pub location: Option<SourceLocation>,
}

pub struct SourceLocation {
    pub line: usize,
    pub column: usize,
}

pub enum ShellErrorKind {
    Parse(ParseErrorKind),
    Expansion(ExpansionErrorKind),
    Runtime(RuntimeErrorKind),
}

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

pub enum ExpansionErrorKind {
    DivisionByZero,
    UnsetVariable,
    ParameterError,
    InvalidArithmetic,
}

pub enum RuntimeErrorKind {
    CommandNotFound,
    PermissionDenied,
    RedirectFailed,
    ReadonlyVariable,
    InvalidOption,
}
```

**Migration strategy**:
- Parse errors: Existing `?` propagation preserved, `ShellErrorKind::Xxx` becomes `ShellErrorKind::Parse(ParseErrorKind::Xxx)`
- Runtime errors: Replace `eprintln!("kish: ...")` call sites with `ShellError` construction. Error display moves to the exec_simple_command / exec_pipeline boundary — the outermost execution entry point prints the error and sets exit status. Inner functions return `Result<i32, ShellError>` instead of calling eprintln directly.
- Expansion errors: Replace `env.expansion_error` flag with `Result<_, ShellError>` propagation. `expand_word()` and `expand_words()` return `Result<Vec<String>, ShellError>`. Callers in exec handle the error. This removes `ExecState::expansion_error` field.
- `Display` impl: `kish: line N: message` when location present, `kish: message` otherwise

**Implementation order within Phase 1.3**: Error type definition first, then parse migration (mechanical rename), then expansion migration (flag → Result), then runtime migration (eprintln → Result). Each sub-step keeps E2E tests passing.

### 1.4 ShellEnv Decomposition

**File**: `src/env/mod.rs`

Split 14-field monolith into focused sub-structs:

```rust
pub struct ExecState {
    pub last_exit_status: i32,
    pub flow_control: Option<FlowControl>,
    pub expansion_error: bool,  // removed in Phase 1.3 when expansion errors use Result<_, ShellError>
}

pub struct ProcessState {
    pub shell_pid: Pid,
    pub shell_pgid: Pid,
    pub jobs: JobTable,
}

pub struct ShellMode {
    pub options: ShellOptions,
    pub is_interactive: bool,
    pub in_dot_script: bool,
}

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

**Migration**: Mechanical replacement first (`env.last_exit_status` -> `env.exec.last_exit_status`). Subsequent phases can narrow function signatures from `&mut ShellEnv` to specific sub-structs.

### 1.5 VarStore Optimization

**File**: `src/env/vars.rs`

1. **Fast path for global-only scope**: When `scopes.len() == 1` (most common in scripts outside functions), bypass scope chain walk for O(1) lookup.

2. **`vars_iter()` improvement**: Replace HashMap rebuild with lazy shadowed iterator:

```rust
pub fn vars_iter(&self) -> impl Iterator<Item = (&str, &Variable)> {
    let mut seen = HashSet::new();
    self.scopes.iter().rev()
        .flat_map(|s| s.vars.iter())
        .filter_map(move |(k, v)| {
            if seen.insert(k.as_str()) { Some((k.as_str(), v)) } else { None }
        })
}
```

3. **`to_environ()` caching**: Cache exported environment; invalidate on any variable mutation:

```rust
pub struct VarStore {
    scopes: Vec<Scope>,
    environ_cache: Option<Vec<(String, String)>>,
}

pub fn set(&mut self, ...) -> Result<(), String> {
    self.environ_cache = None;  // invalidate
    // ...existing logic
}

pub fn to_environ(&mut self) -> &[(String, String)] {
    if self.environ_cache.is_none() {
        self.environ_cache = Some(self.build_environ());
    }
    self.environ_cache.as_ref().unwrap()
}
```

4. **No Cow<str>**: Variable values are frequently mutated; `Cow` branch overhead outweighs copy savings. Keep `String`.

---

## Phase 2: Module Boundary Reorganization

### 2.1 Lexer Split (1,812 lines -> 4 files)

```
lexer/
  mod.rs          (~150 lines) Public API, Lexer struct, next_token() dispatch
  token.rs        (existing)   Token enum, Span, SpannedToken
  scanner.rs      (~500 lines) Byte scanning, operator/reserved word/redirect recognition
  word.rs         (~600 lines) Word construction, quoting analysis, parameter expr parsing
  heredoc.rs      (~250 lines) Here-document delimiter and body parsing
  alias.rs        (~200 lines) Alias token queue, recursion prevention
```

- `Lexer` struct stays in `mod.rs`; sub-modules implement methods via `impl Lexer` blocks
- `scanner.rs`: `scan_operator()`, `scan_newline()`, `skip_comment()`, `skip_whitespace()`
- `word.rs`: `scan_word()`, `scan_single_quoted()`, `scan_double_quoted()`, `scan_dollar()`, `scan_param_expr()`
- `heredoc.rs`: `read_heredoc_delimiter()`, `read_heredoc_body()`, `expand_heredoc_body()`
- `alias.rs`: `try_alias_expand()`, `drain_alias_queue()`
- `LexerState` (save/restore) remains in `mod.rs`

### 2.2 Executor Split (1,381 lines -> 5 files)

```
exec/
  mod.rs          (~200 lines) Executor struct, exec_program(), errexit, signal processing
  simple.rs       (~350 lines) exec_simple_command(), builtin dispatch, external command fork/exec
  compound.rs     (~400 lines) exec_if(), exec_for(), exec_while(), exec_until(), exec_case(), exec_brace_group(), exec_subshell()
  function.rs     (~150 lines) exec_function_call(), scope push/pop with Drop guard
  pipeline.rs     (existing)   Pipeline execution
  command.rs      (existing)   execvp, PATH search, wait
  redirect.rs     (existing)   RedirectState, fd operations
```

**function.rs Drop guard** (resolves TODO.md `exec_function_call lacks panic safety`):

Note: A simple `&mut VarStore` guard creates a borrow conflict with `&mut self` in `exec_compound_command()`. Instead, use a boolean flag pattern:

```rust
fn exec_function_call(&mut self, func: &FunctionDef, args: Vec<String>) -> i32 {
    self.env.vars.push_scope(args);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        self.exec_compound_command(&func.body, &func.redirects)
    }));
    self.env.vars.pop_scope();
    match result {
        Ok(status) => status,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}
```

Alternative: Extract the function body execution into a closure that captures only what it needs, avoiding the aliased mutable borrow entirely. The exact pattern will be determined during implementation.

### 2.3 env/ Sub-contextualization

```
env/
  mod.rs          (~150 lines) ShellEnv struct (aggregate), new(), Clone
  exec_state.rs   (~50 lines)  ExecState, FlowControl
  process.rs      (~30 lines)  ProcessState
  shell_mode.rs   (~200 lines) ShellMode, ShellOptions (moved from mod.rs)
  vars.rs         (existing + optimizations) VarStore, Variable
  traps.rs        (~160 lines) TrapStore, TrapAction (extracted from mod.rs)
  jobs.rs         (existing)   JobTable, Job, JobStatus
  aliases.rs      (existing)   AliasStore
```

### 2.4 expand/ — Minimal Changes

Structure is already well-organized. Changes limited to:
- `ExpandedField` bitset migration (Phase 1.2)
- `field_split.rs` / `pathname.rs`: `quoted_mask[i]` -> `is_quoted(i)` method calls
- Function signatures narrowed from `&mut ShellEnv` to sub-structs (deferred)

### 2.5 builtin/ — Classification Cleanup

```
builtin/
  mod.rs          (~100 lines) classify_builtin(), BuiltinKind enum, dispatch only
  regular.rs      (~400 lines) cd, echo, alias, unalias, read, test, umask, fg, bg, jobs, kill, wait
  special.rs      (existing)   eval, exec, export, set, trap, source, shift, times, unset, return, exit, break, continue
```

Move builtin implementations out of `mod.rs` into `regular.rs`. `mod.rs` becomes dispatch-only.

---

## Phase 3: Cross-Cutting Improvements

### 3.1 Clone Reduction

**Rc-based elimination (highest impact)**:
- `FunctionDef` clones in exec/mod.rs -> resolved by `Rc<CompoundCommand>` from Phase 1.1

**Ownership transfer**:
- `expanded[0].clone()` for command name -> `Vec::remove(0)` or `into_iter().next()`
- Return value clones -> move ownership directly where possible

**Retained**:
- Variable value `String` clones (mutation requires owned values)
- `TrapAction::Command` clones (low frequency)

### 3.2 Future Extension Paths

Not implemented in this refactoring, but design ensures clean extension points:

| Future Feature | Extension Point | Prerequisite |
|---------------|----------------|-------------|
| History / completion | `InteractiveState` inside Repl (not ShellEnv) | ShellEnv decomposition |
| Syntax highlighting | Lexer token stream reuse | scanner.rs separation |
| Structured data (arrays, assoc arrays) | `Variable::value` -> `enum Value` | VarStore isolation |
| Process substitution `<(cmd)` | New `RedirectKind` variant | redirect.rs isolation |
| Parallel pipelines | pipeline.rs improvements | ProcessState separation |

---

## TODO.md Issues Resolved

This refactoring addresses the following known issues:

- [x] `exec_function_call` lacks panic safety -> Drop guard in `exec/function.rs` (Phase 2.2)
- [x] `VarStore::vars_iter()` rebuilds HashMap on every call -> Lazy shadowed iterator (Phase 1.5)
- [x] `cd` overwrites OLDPWD before checking success -> Fix during builtin reorganization (Phase 2.5)
- [x] `DupOutput`/`DupInput` redirect kinds lack fd == target_fd guard -> Fix during redirect.rs review (Phase 2.2)
- [x] `builtin_source` FlowControl::Return consumption fragility -> Fix during special.rs review (Phase 2.5)

---

## Execution Order Summary

1. **Phase 0**: Benchmark infrastructure -> baseline measurement
2. **Phase 1**: Core data structures (AST, ExpandedField, Error, ShellEnv, VarStore)
3. **Phase 2**: Module splits (lexer, exec, env, builtin)
4. **Phase 3**: Clone reduction, future extension path documentation

Each phase ends with:
- All E2E tests passing
- Benchmark comparison against baseline
- Commit with phase summary
