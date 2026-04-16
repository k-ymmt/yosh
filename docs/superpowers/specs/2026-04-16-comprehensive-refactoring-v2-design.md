# Comprehensive Refactoring v2

## Overview

A four-phase refactoring of the yosh codebase, executed in risk-ascending order. Each phase is independently testable — all existing tests must pass after each phase before proceeding.

## Phase 1: Small Improvements

### 1a. Rename `KISH_SHOW_DOTFILES` to `YOSH_SHOW_DOTFILES`

**Files:**
- `src/interactive/mod.rs:157` — rename env var lookup
- `docs/superpowers/specs/2026-04-12-path-completion-design.md` — update references

**Scope:** String replacement only. No logic changes.

### 1b. DRY `print_help()` in `src/main.rs`

**Current:** Lines 32–73 duplicate every help line in color and no-color branches.

**Design:** Replace with a data-driven approach:

```rust
struct HelpSection {
    heading: &'static str,
    items: &'static [(&'static str, &'static str)],
}

const HELP: &[HelpSection] = &[
    HelpSection { heading: "Options", items: &[
        ("-c <command>",    "Read commands from command_string"),
        ("--parse <code>",  "Parse and dump AST (debug)"),
        ("-h, --help",      "Show this help message"),
        ("--version",       "Show version information"),
    ]},
    HelpSection { heading: "Subcommands", items: &[
        ("plugin",          "Manage shell plugins (see 'yosh plugin --help')"),
    ]},
];
```

A single `print_help()` function iterates the data structure, applying color conditionally. Adding a new flag requires editing one place.

## Phase 2: Duplicate Code Consolidation

### 2a. Sourcing Logic Unification

**Current duplication:**
- `builtin_source` (`src/builtin/special.rs:349-396`) — full sourcing with PATH search
- `source_file` (`src/exec/mod.rs:62-85`) — simplified sourcing for startup files

**Core shared logic:** read file → parse → exec with `in_dot_script` → handle `FlowControl::Return`.

**Design:** Keep `Executor::source_file(&mut self, path: &Path) -> Option<i32>` as the single source of truth. Refactor `builtin_source` to:

1. Validate arguments
2. Resolve file path (with PATH search)
3. Delegate to `executor.source_file(&resolved_path)`
4. Convert `None` (file not found) to appropriate error message + exit code

### 2b. Tilde Expansion Unification

**Current duplication:**
- `src/interactive/mod.rs:71-92` — inline `~`/`~user` expansion for ENV preprocessing
- `src/expand/mod.rs:529-542` — `expand_tilde_user()` for `~user` only

**Design:** Add a unified helper in `src/expand/mod.rs`:

```rust
/// Expand a tilde prefix: `~` uses home_dir, `~user` uses getpwnam.
/// Returns the original string unchanged if expansion fails.
pub(crate) fn expand_tilde_prefix(home_dir: Option<&str>, s: &str) -> String
```

- `expand_tilde_user` remains as a lower-level helper called by `expand_tilde_prefix`
- `interactive/mod.rs` calls `expand_tilde_prefix(env.vars.get("HOME"), &env_val)`
- `WordPart::Tilde` expansion in `expand_word_part` also delegates to `expand_tilde_prefix`

### 2c. Quote-Aware Balanced Parenthesis Scanning

**Current duplication (3 locations, ~200 lines total):**
1. `src/expand/mod.rs` — `$((...))` scanning in `expand_heredoc_string`
2. `src/expand/mod.rs` — `$(...)` scanning in `expand_heredoc_string`
3. `src/expand/arith.rs` — `$(...)` scanning in `expand_vars`

All three implement identical logic: track parenthesis depth while skipping single-quoted, double-quoted (with backslash escapes), and backslash-escaped regions.

**Design:** Extract to a shared helper in `src/expand/mod.rs`:

```rust
/// Scan forward from `start` in `bytes`, tracking parenthesis depth (starting at 1),
/// while respecting single/double quotes and backslash escapes.
/// Returns the index where depth reaches 0 (the closing `)`).
pub(crate) fn skip_balanced_parens(bytes: &[u8], start: usize) -> usize
```

The `$((...))` case (terminated by `))` at depth 1) needs special handling. Two options:
- **Option A:** A separate `skip_balanced_double_parens` function
- **Option B:** A parameter `double_close: bool` on the shared function

Recommend **Option A** for clarity — the two functions share the quote-skipping logic via an inner helper, but have distinct termination conditions.

## Phase 3: File Splitting

### 3a. Split `highlight.rs` (1,756 lines → 3 files)

| New file | Responsibility | Key types |
|---|---|---|
| `highlight.rs` | Style definitions, `apply_style`, public re-exports | `HighlightStyle`, `ColorSpan`, `CheckerEnv`, `apply_style()` |
| `highlight_scanner.rs` | Lexical scanning for syntax highlighting | `HighlightScanner` impl |
| `command_checker.rs` | Command existence checking with PATH cache | `CommandChecker`, `CommandExistence`, `search_path()`, `is_executable()` |

**Public API is unchanged.** `highlight.rs` re-exports via `pub use` so downstream `use super::highlight::...` imports remain valid.

**Not splitting:**
- `line_editor.rs` (1,063 lines) — methods are tightly coupled (buffer, cursor, rendering interdependent)
- `completion.rs` (1,012 lines) — `CompletionUI` and completion logic are tightly coupled; `command_completion.rs` already extracted

## Phase 4: Error Handling Migration

### Overview

Migrate ~112 `eprintln!("yosh: ...")` call sites in `exec/` and `builtin/` to `Result<i32, ShellError>` using `RuntimeErrorKind` variants.

### RuntimeErrorKind Extension

Add variants to cover all error categories currently expressed as format strings:

```rust
pub enum RuntimeErrorKind {
    // Existing
    CommandNotFound,
    PermissionDenied,
    RedirectFailed,
    ReadonlyVariable,
    InvalidOption,
    // New
    InvalidArgument,
    IoError,
    ExecFailed,
    TrapError,
    JobControlError,
}
```

### Migration Order (by file)

1. `builtin/regular.rs` (~19 sites) — simplest builtins, lowest risk
2. `builtin/special.rs` (~36 sites) — after Phase 2a sourcing unification
3. `exec/simple.rs` (~24 sites) — command execution errors
4. `exec/compound.rs` (~6 sites) — few sites, quick
5. `exec/mod.rs` + remaining — top-level dispatcher

### Error Output Centralization

`ShellError::Display` already formats as `"yosh: ..."`. Error printing is centralized to the top-level dispatch point (`exec_complete_command` or similar), which catches `Err(e)` and calls `eprintln!("{}", e)`.

### Type Signature Migration

Internal methods change signature from `-> i32` to `-> Result<i32, ShellError>` progressively. Public API (`exec_complete_command`, `exec_program`) retains `-> i32` with a thin wrapper that handles `Err`:

```rust
pub fn exec_complete_command(&mut self, cmd: &CompleteCommand) -> i32 {
    match self.exec_complete_command_inner(cmd) {
        Ok(status) => status,
        Err(e) => {
            eprintln!("{}", e);
            e.exit_code()
        }
    }
}
```

This minimizes impact on `main.rs`, `interactive/mod.rs`, and test code.

## Testing Strategy

- **After each phase:** Run full `cargo test` + `./e2e/run_tests.sh`
- **Phase 1:** Verify help output manually (`yosh --help`)
- **Phase 2:** Existing tests cover sourcing, tilde expansion; add unit test for `skip_balanced_parens`
- **Phase 3:** No behavior changes — existing tests sufficient
- **Phase 4:** Existing tests verify error messages; add unit tests for new `RuntimeErrorKind` variants

## TODO.md Cleanup

After completion, delete the following items from TODO.md:
- "Unify `builtin_source` and `source_file`"
- "Consolidate tilde expansion logic"
- "Rename `KISH_SHOW_DOTFILES` to `YOSH_SHOW_DOTFILES`"
- "Extract quote-aware balanced-paren scanning into a shared helper"
- "`print_help()` DRY refactor"
- "Runtime error migration"
