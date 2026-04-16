# Comprehensive Refactoring v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor the yosh codebase across four phases: small improvements, duplicate code consolidation, file splitting, and error handling migration.

**Architecture:** Risk-ascending order — small safe changes first, large structural changes last. Each phase is independently testable. All existing tests must pass after each phase.

**Tech Stack:** Rust, POSIX shell semantics, crossterm (terminal), owo_colors (CLI colors)

---

## File Map

### Phase 1: Small Improvements
- Modify: `src/interactive/mod.rs:157` — rename env var
- Modify: `docs/superpowers/specs/2026-04-12-path-completion-design.md` — update references
- Modify: `src/main.rs:32-73` — DRY print_help()

### Phase 2: Duplicate Code Consolidation
- Modify: `src/exec/mod.rs:60-85` — keep source_file as single implementation
- Modify: `src/builtin/special.rs:349-396` — delegate to source_file
- Modify: `src/expand/mod.rs` — add expand_tilde_prefix, add skip_balanced_parens helpers
- Modify: `src/interactive/mod.rs:68-93` — use expand_tilde_prefix
- Modify: `src/expand/arith.rs:67-186` — use skip_balanced_parens

### Phase 3: File Splitting
- Create: `src/interactive/command_checker.rs` — extracted from highlight.rs
- Create: `src/interactive/highlight_scanner.rs` — extracted from highlight.rs
- Modify: `src/interactive/highlight.rs` — keep types + apply_style, re-export
- Modify: `src/interactive/mod.rs` — add new module declarations

### Phase 4: Error Handling Migration
- Modify: `src/error.rs` — extend RuntimeErrorKind, add exit_code()
- Modify: `src/builtin/regular.rs` — return Result<i32, ShellError>
- Modify: `src/builtin/special.rs` — return Result<i32, ShellError>
- Modify: `src/builtin/mod.rs` — update dispatchers
- Modify: `src/exec/simple.rs` — return Result<i32, ShellError>
- Modify: `src/exec/compound.rs` — return Result<i32, ShellError>
- Modify: `src/exec/pipeline.rs` — return Result<i32, ShellError>
- Modify: `src/exec/command.rs` — return Result<i32, ShellError>
- Modify: `src/exec/mod.rs` — add wrapper, update internal methods

### Cleanup
- Modify: `TODO.md` — delete completed items

---

## Phase 1: Small Improvements

### Task 1: Rename KISH_SHOW_DOTFILES to YOSH_SHOW_DOTFILES

**Files:**
- Modify: `src/interactive/mod.rs:157`
- Modify: `docs/superpowers/specs/2026-04-12-path-completion-design.md:66,156,178,188`
- Modify: `docs/superpowers/plans/2026-04-12-path-completion.md:1352`

- [ ] **Step 1: Run tests to establish baseline**

Run: `cargo test 2>&1 | tail -5`
Expected: test result: ok

- [ ] **Step 2: Rename in source code**

In `src/interactive/mod.rs`, change line 157:

```rust
// Before
let show_dotfiles = self.executor.env.vars.get("KISH_SHOW_DOTFILES")
// After
let show_dotfiles = self.executor.env.vars.get("YOSH_SHOW_DOTFILES")
```

- [ ] **Step 3: Rename in design spec**

In `docs/superpowers/specs/2026-04-12-path-completion-design.md`, replace all occurrences of `KISH_SHOW_DOTFILES` with `YOSH_SHOW_DOTFILES`.

- [ ] **Step 4: Rename in plan doc**

In `docs/superpowers/plans/2026-04-12-path-completion.md`, replace all occurrences of `KISH_SHOW_DOTFILES` with `YOSH_SHOW_DOTFILES`.

- [ ] **Step 5: Run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: test result: ok

- [ ] **Step 6: Commit**

```bash
git add src/interactive/mod.rs docs/superpowers/specs/2026-04-12-path-completion-design.md docs/superpowers/plans/2026-04-12-path-completion.md
git commit -m "refactor: rename KISH_SHOW_DOTFILES to YOSH_SHOW_DOTFILES"
```

---

### Task 2: DRY print_help()

**Files:**
- Modify: `src/main.rs:32-73`

- [ ] **Step 1: Replace print_help with data-driven implementation**

Replace `fn print_help()` (lines 32–73 in `src/main.rs`) with:

```rust
fn print_help() {
    let color = should_colorize();

    let header = "yosh - A POSIX-compliant shell";
    if color {
        println!("{}", header.bold());
    } else {
        println!("{}", header);
    }
    println!();

    if color {
        println!("{}  yosh [options] [file [argument...]]", "Usage:".yellow().bold());
    } else {
        println!("Usage:  yosh [options] [file [argument...]]");
    }
    println!();

    struct HelpSection {
        heading: &'static str,
        items: &'static [(&'static str, &'static str)],
    }

    const SECTIONS: &[HelpSection] = &[
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

    for section in SECTIONS {
        if color {
            println!("{}", section.heading.yellow().bold());
        } else {
            println!("{}:", section.heading);
        }
        for &(flag, desc) in section.items {
            if color {
                println!("  {:16}{}", flag.green(), desc);
            } else {
                println!("  {:16}{}", flag, desc);
            }
        }
        println!();
    }
}
```

Note: the `HelpSection` heading in SECTIONS does NOT include the `:` suffix — the color branch prints it without `:` (matching current behavior where `"Options:".yellow().bold()` includes the colon in the string). Actually wait, looking at the current code more carefully:

Current color branch: `println!("{}", "Options:".yellow().bold());` — the colon IS part of the string.
Current no-color branch: `println!("Options:");` — same.

So the heading should include the colon. Let me adjust — actually it's cleaner to add `:` only in the non-color branch and keep the heading without it. Looking at the current output:
- Color: prints `Options:` (yellow bold)
- No-color: prints `Options:`

So both include `:`. The data structure should either include `:` in the heading or add it in the print logic. Since the color branch uses `.yellow().bold()` on the whole string, the simplest approach:

```rust
// heading field already includes ":"
HelpSection { heading: "Options:", items: &[ ... ] },
```

But wait, `"Options:".yellow().bold()` means the colon is colored. If we use `section.heading.yellow().bold()` that's the same. Actually there's a problem: `&'static str` doesn't have the `.yellow()` method directly in a const context. Let me check — `owo_colors::OwoColorize` is a trait implemented on all types, but you can only call it at runtime. The `const` just stores the `&str`, and `.yellow().bold()` is called at print time. This is fine.

Let me fix the implementation: heading should NOT include `:` to keep data clean, and we add `:` in the no-color branch:

Actually, looking again at the current code for the color branch:
```rust
println!("{}", "Options:".yellow().bold());
```
The `:` is part of the colored text. So in the data-driven approach:

```rust
if color {
    // Need to format heading + ":" and color the whole thing
    println!("{}", format!("{}:", section.heading).yellow().bold());
} else {
    println!("{}:", section.heading);
}
```

This works. Let me revise.

But actually wait — there's a subtlety with `Subcommands:` in the current code:
```rust
println!("  {}          Manage shell plugins (see '{}')",
    "plugin".green(), "yosh plugin --help".green());
```
The description includes a colored reference `'yosh plugin --help'`. In the data-driven approach, we lose this inner coloring. But this is a reasonable simplification — the description text can be plain. The current colored version only colors `yosh plugin --help` within the description, which is a minor nicety.

Let me keep it simple — no inner coloring in descriptions. The output difference is minimal and the DRY benefit is significant.

Let me finalize the implementation. I'll put the struct and const inside the function to keep scope tight.

- [ ] **Step 2: Build and verify help output**

Run: `cargo build 2>&1 | tail -3 && ./target/debug/yosh --help`
Expected: Compiles successfully; help output shows Options and Subcommands sections with correct formatting.

- [ ] **Step 3: Run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: test result: ok

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "refactor: DRY print_help() with data-driven approach"
```

---

## Phase 2: Duplicate Code Consolidation

### Task 3: Unify sourcing logic

**Files:**
- Modify: `src/builtin/special.rs:349-396`
- Modify: `src/exec/mod.rs:60-85`

- [ ] **Step 1: Run existing source/dot tests to establish baseline**

Run: `cargo test source 2>&1; cargo test dot 2>&1; ./e2e/run_tests.sh --filter=source 2>&1 | tail -5; ./e2e/run_tests.sh --filter=dot 2>&1 | tail -5`
Expected: All pass.

- [ ] **Step 2: Refactor builtin_source to delegate to source_file**

Replace `builtin_source` in `src/builtin/special.rs` (lines 349–396) with:

```rust
fn builtin_source(args: &[String], executor: &mut Executor) -> i32 {
    if args.is_empty() {
        eprintln!("yosh: .: filename argument required");
        return 2;
    }
    let filename = &args[0];
    let path = if filename.contains('/') {
        std::path::PathBuf::from(filename)
    } else {
        if let Some(path_var) = executor.env.vars.get("PATH") {
            let mut found = None;
            for dir in path_var.split(':') {
                let candidate = std::path::PathBuf::from(dir).join(filename);
                if candidate.is_file() {
                    found = Some(candidate);
                    break;
                }
            }
            match found {
                Some(p) => p,
                None => { eprintln!("yosh: .: {}: not found", filename); return 1; }
            }
        } else {
            std::path::PathBuf::from(filename)
        }
    };
    match executor.source_file(&path) {
        Some(status) => status,
        None => {
            eprintln!("yosh: .: {}: No such file or directory", path.display());
            1
        }
    }
}
```

The key change: PATH resolution stays in `builtin_source`, but the actual source execution (read → parse → exec → handle return) delegates to `executor.source_file()`. The old code duplicated the entire read/parse/exec/return-handling logic.

- [ ] **Step 3: Run tests**

Run: `cargo test 2>&1 | tail -5 && ./e2e/run_tests.sh 2>&1 | tail -5`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add src/builtin/special.rs
git commit -m "refactor: unify sourcing logic — builtin_source delegates to source_file"
```

---

### Task 4: Unify tilde expansion logic

**Files:**
- Modify: `src/expand/mod.rs` — add `expand_tilde_prefix`
- Modify: `src/interactive/mod.rs:68-93` — use new helper

- [ ] **Step 1: Write test for expand_tilde_prefix**

Add to the bottom of `src/expand/mod.rs`, inside a new `#[cfg(test)] mod tests` block (or append to existing tests):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde_prefix_home() {
        assert_eq!(expand_tilde_prefix(Some("/home/user"), "~/docs"), "/home/user/docs");
    }

    #[test]
    fn test_expand_tilde_prefix_home_only() {
        assert_eq!(expand_tilde_prefix(Some("/home/user"), "~"), "/home/user");
    }

    #[test]
    fn test_expand_tilde_prefix_no_home() {
        assert_eq!(expand_tilde_prefix(None, "~/docs"), "~/docs");
    }

    #[test]
    fn test_expand_tilde_prefix_no_tilde() {
        assert_eq!(expand_tilde_prefix(Some("/home/user"), "/abs/path"), "/abs/path");
    }

    #[test]
    fn test_expand_tilde_prefix_empty_home() {
        assert_eq!(expand_tilde_prefix(Some(""), "~/docs"), "~/docs");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test expand::tests::test_expand_tilde_prefix 2>&1 | tail -10`
Expected: FAIL — `expand_tilde_prefix` not found.

- [ ] **Step 3: Implement expand_tilde_prefix**

Add this function in `src/expand/mod.rs`, just above the existing `expand_tilde_user` function (around line 528):

```rust
/// Expand a tilde prefix in a string: `~` uses `home_dir`, `~user` uses getpwnam.
/// Returns the original string unchanged if the prefix doesn't start with `~`
/// or expansion fails.
pub(crate) fn expand_tilde_prefix(home_dir: Option<&str>, s: &str) -> String {
    let rest = match s.strip_prefix('~') {
        Some(r) => r,
        None => return s.to_string(),
    };
    let (user, suffix) = match rest.find('/') {
        Some(pos) => (&rest[..pos], &rest[pos..]),
        None => (rest, ""),
    };
    if user.is_empty() {
        // ~ alone: use provided home directory
        match home_dir {
            Some(home) if !home.is_empty() => format!("{}{}", home, suffix),
            _ => s.to_string(),
        }
    } else {
        // ~user: resolve via getpwnam
        let expanded = expand_tilde_user(user);
        if expanded.starts_with('~') {
            s.to_string() // unknown user, keep original
        } else {
            format!("{}{}", expanded, suffix)
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test expand::tests::test_expand_tilde_prefix 2>&1 | tail -10`
Expected: all 5 tests pass.

- [ ] **Step 5: Replace inline tilde expansion in interactive/mod.rs**

In `src/interactive/mod.rs`, replace lines 70–93 (the inline tilde expansion block inside the `if !env_val.is_empty()` block) with:

```rust
                // POSIX 2.6.1: tilde expansion occurs before parameter expansion
                let home = executor.env.vars.get("HOME").map(|s| s.to_string());
                let after_tilde = crate::expand::expand_tilde_prefix(
                    home.as_deref(),
                    &env_val,
                );
```

This replaces the 22-line inline implementation with a 4-line call.

- [ ] **Step 6: Run full tests**

Run: `cargo test 2>&1 | tail -5 && ./e2e/run_tests.sh 2>&1 | tail -5`
Expected: All pass.

- [ ] **Step 7: Commit**

```bash
git add src/expand/mod.rs src/interactive/mod.rs
git commit -m "refactor: unify tilde expansion via expand_tilde_prefix helper"
```

---

### Task 5: Extract quote-aware balanced parenthesis scanning

**Files:**
- Modify: `src/expand/mod.rs` — add `skip_balanced_parens` and `skip_balanced_double_parens`
- Modify: `src/expand/arith.rs:67-132` — use `skip_balanced_parens`

- [ ] **Step 1: Write tests for skip_balanced_parens**

Add to the `#[cfg(test)] mod tests` in `src/expand/mod.rs`:

```rust
    #[test]
    fn test_skip_balanced_parens_simple() {
        // "echo hello)" — depth starts at 1, finds closing ) at index 10
        let input = b"echo hello)";
        assert_eq!(skip_balanced_parens(input, 0), 10);
    }

    #[test]
    fn test_skip_balanced_parens_nested() {
        // "(inner) outer)" — nested parens
        let input = b"(inner) outer)";
        assert_eq!(skip_balanced_parens(input, 0), 13);
    }

    #[test]
    fn test_skip_balanced_parens_single_quoted() {
        // "')' real)" — ) inside single quotes should be skipped
        let input = b"')' real)";
        assert_eq!(skip_balanced_parens(input, 0), 8);
    }

    #[test]
    fn test_skip_balanced_parens_double_quoted() {
        // '")(" real)'
        let input = b"\")(\" real)";
        assert_eq!(skip_balanced_parens(input, 0), 9);
    }

    #[test]
    fn test_skip_balanced_parens_backslash_escape() {
        // "\) real)" — escaped ) should be skipped
        let input = b"\\) real)";
        assert_eq!(skip_balanced_parens(input, 0), 7);
    }

    #[test]
    fn test_skip_balanced_double_parens_simple() {
        // "1 + 2))" — find )) at depth 1
        let input = b"1 + 2))";
        assert_eq!(skip_balanced_double_parens(input, 0), 5);
    }

    #[test]
    fn test_skip_balanced_double_parens_nested() {
        // "(1 + 2) * 3))" — nested () inside $((...))
        let input = b"(1 + 2) * 3))";
        assert_eq!(skip_balanced_double_parens(input, 0), 11);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test expand::tests::test_skip_balanced 2>&1 | tail -10`
Expected: FAIL — functions not found.

- [ ] **Step 3: Implement skip_balanced_parens and skip_balanced_double_parens**

Add these functions in `src/expand/mod.rs` (above `expand_tilde_prefix`):

```rust
/// Skip forward from `start` in `bytes`, tracking parenthesis depth (starting at 1),
/// while respecting single/double quotes and backslash escapes.
/// Returns the index of the byte where depth reaches 0 (the closing `)`).
/// If no matching `)` is found, returns `bytes.len()`.
pub(crate) fn skip_balanced_parens(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    let mut depth: usize = 1;
    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'\'' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'\'' {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if i < bytes.len() {
                    i += 1;
                }
            }
            b'\\' => {
                if i + 1 < bytes.len() {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                if depth > 0 {
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    i
}

/// Like `skip_balanced_parens`, but terminates when `))` is found at depth 1.
/// Used for `$((...))` arithmetic substitution scanning.
/// Returns the index of the first `)` in the closing `))`.
/// If no matching `))` is found, returns `bytes.len()`.
pub(crate) fn skip_balanced_double_parens(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    let mut depth: usize = 1;
    while i + 1 < bytes.len() && depth > 0 {
        match bytes[i] {
            b'\'' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'\'' {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if i < bytes.len() {
                    i += 1;
                }
            }
            b'\\' => {
                if i + 1 < bytes.len() {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' if bytes[i + 1] == b')' && depth == 1 => {
                break;
            }
            b')' => {
                depth -= 1;
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    i
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test expand::tests::test_skip_balanced 2>&1 | tail -15`
Expected: All 7 tests pass.

- [ ] **Step 5: Replace $(...) scanning in expand_heredoc_string**

In `src/expand/mod.rs`, in the `expand_heredoc_string` function, replace the `$(...) — command substitution` branch (the `else` branch starting around line 264) with:

```rust
                    } else {
                        // $(...) — command substitution
                        i += 1;
                        let start = i;
                        i = skip_balanced_parens(bytes, i);
                        let cmd_str = &s[start..i];
                        if i < bytes.len() { i += 1; } // skip )
                        // Parse and execute
                        if let Ok(program) = crate::parser::Parser::new(cmd_str).parse_program() {
                            result.push_str(&command_sub::execute(env, &program));
                        }
                    }
```

- [ ] **Step 6: Replace $((...)) scanning in expand_heredoc_string**

In the same function, replace the `$((...)) — arithmetic` branch (starting around line 199, after `if i + 1 < bytes.len() && bytes[i + 1] == b'('`) with:

```rust
                    if i + 1 < bytes.len() && bytes[i + 1] == b'(' {
                        // $((...)) — arithmetic
                        i += 2;
                        let start = i;
                        i = skip_balanced_double_parens(bytes, i);
                        let expr = &s[start..i];
                        if i + 1 < bytes.len() { i += 2; } // skip ))
                        match arith::evaluate(env, expr) {
                            Ok(val) => result.push_str(&val),
                            Err(msg) => {
                                eprintln!("yosh: arithmetic: {}", msg);
                                env.exec.last_exit_status = 1;
                                result.push('0');
                            }
                        }
```

- [ ] **Step 7: Replace $(...) scanning in arith.rs expand_vars**

In `src/expand/arith.rs`, in the `expand_vars` function, replace the `$(cmd)` branch (lines 74–139) with:

```rust
            if bytes[i + 1] == b'(' {
                // $(cmd) — command substitution inside arithmetic
                i += 2; // skip '$('
                let start = i;
                i = crate::expand::skip_balanced_parens(bytes, i);
                let cmd_str = &expr[start..i];
                if i < bytes.len() {
                    i += 1; // skip closing ')'
                }
                if let Ok(program) = crate::parser::Parser::new(cmd_str).parse_program() {
                    let output = crate::expand::command_sub::execute(env, &program);
                    let trimmed = output.trim();
                    // Default to "0" if the output is empty
                    result.push_str(if trimmed.is_empty() { "0" } else { trimmed });
                } else {
                    result.push('0');
                }
```

- [ ] **Step 8: Run full test suite**

Run: `cargo test 2>&1 | tail -5 && ./e2e/run_tests.sh 2>&1 | tail -5`
Expected: All pass.

- [ ] **Step 9: Commit**

```bash
git add src/expand/mod.rs src/expand/arith.rs
git commit -m "refactor: extract skip_balanced_parens helper, deduplicate 3 scanning sites"
```

---

## Phase 3: File Splitting

### Task 6: Split highlight.rs into 3 files

**Files:**
- Create: `src/interactive/command_checker.rs`
- Create: `src/interactive/highlight_scanner.rs`
- Modify: `src/interactive/highlight.rs`
- Modify: `src/interactive/mod.rs`

The current `highlight.rs` (1,756 lines) has three distinct responsibilities:
1. Type definitions + `apply_style` (HighlightStyle, ColorSpan, CheckerEnv, apply_style) ~210 lines
2. CommandChecker + helpers (CommandChecker, CommandExistence, search_path, is_executable) ~90 lines
3. Scanner (ScanMode, ScannerState, HighlightCache, HighlightScanner, keyword tables, char helpers) ~1,450 lines

- [ ] **Step 1: Run tests to establish baseline**

Run: `cargo test highlight 2>&1 | tail -10`
Expected: All pass.

- [ ] **Step 2: Create command_checker.rs**

Create `src/interactive/command_checker.rs` with the CommandChecker, CommandExistence, search_path, and is_executable content extracted from `highlight.rs`:

```rust
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::builtin::{BuiltinKind, classify_builtin};
use crate::env::aliases::AliasStore;

// ---------------------------------------------------------------------------
// CheckerEnv
// ---------------------------------------------------------------------------

/// Lightweight view of the shell environment needed by `CommandChecker`.
pub struct CheckerEnv<'a> {
    /// Value of the PATH variable (may be empty).
    pub path: &'a str,
    /// Alias store for the current shell session.
    pub aliases: &'a AliasStore,
}

// ---------------------------------------------------------------------------
// CommandExistence
// ---------------------------------------------------------------------------

/// Result of a command-existence check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandExistence {
    Valid,
    Invalid,
}

// ---------------------------------------------------------------------------
// CommandChecker
// ---------------------------------------------------------------------------

/// Checks whether a command name exists, with a simple PATH-search cache.
pub struct CommandChecker {
    /// Cache from command name to existence result (`true` = found).
    path_cache: HashMap<String, bool>,
    /// The PATH value used to populate `path_cache`.
    cached_path: String,
}

impl CommandChecker {
    /// Create a new checker with an empty cache.
    pub fn new() -> Self {
        Self {
            path_cache: HashMap::new(),
            cached_path: String::new(),
        }
    }

    /// Check whether `name` is a valid command in the context of `env`.
    pub fn check(&mut self, name: &str, env: &CheckerEnv) -> CommandExistence {
        // 1. Builtins (special or regular) are always valid.
        if classify_builtin(name) != BuiltinKind::NotBuiltin {
            return CommandExistence::Valid;
        }

        // 2. Aliases defined in the current session.
        if env.aliases.get(name).is_some() {
            return CommandExistence::Valid;
        }

        // 3. Name contains a slash — treat as a direct path.
        if name.contains('/') {
            return if is_executable(Path::new(name)) {
                CommandExistence::Valid
            } else {
                CommandExistence::Invalid
            };
        }

        // 4. PATH search, with cache invalidation when PATH changes.
        if env.path != self.cached_path {
            self.path_cache.clear();
            self.cached_path = env.path.to_string();
        }

        let found = self
            .path_cache
            .entry(name.to_string())
            .or_insert_with(|| search_path(name, env.path));

        if *found {
            CommandExistence::Valid
        } else {
            CommandExistence::Invalid
        }
    }
}

/// Search every directory in `path_var` (colon-separated) for `name`.
fn search_path(name: &str, path_var: &str) -> bool {
    for dir in path_var.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = Path::new(dir).join(name);
        if is_executable(&candidate) {
            return true;
        }
    }
    false
}

/// Returns `true` if `path` is a regular file with at least one execute bit set.
fn is_executable(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && (meta.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}
```

- [ ] **Step 3: Create highlight_scanner.rs**

Create `src/interactive/highlight_scanner.rs` with the scanner-related content extracted from `highlight.rs`. This file contains: `ScanMode`, `ScannerState`, keyword tables, character classification helpers, `HighlightCache`, and `HighlightScanner` with its impl block.

The file starts with:

```rust
use super::command_checker::{CheckerEnv, CommandChecker, CommandExistence};
use super::highlight::{ColorSpan, HighlightStyle};
```

Copy the following sections from `highlight.rs` into this new file:
- `ScanMode` enum (around line 216)
- `ScannerState` struct and impl (around line 234)
- `KEYWORDS` and `COMMAND_POSITION_KEYWORDS` consts (around line 272)
- `is_keyword`, `is_operator_char`, `is_redirect_start`, `is_valid_name`, `is_word_break` functions (around line 280)
- `HighlightCache` struct and impl (around line 321)
- `HighlightScanner` struct and impl (around line 368 to end of file, including all scan methods and tests)

Update `HighlightScanner` struct to use the new import paths:

```rust
pub struct HighlightScanner {
    cache: HighlightCache,
    accumulated_state: Option<(String, ScannerState)>,
    checker: CommandChecker,
}
```

- [ ] **Step 4: Reduce highlight.rs to types + apply_style + re-exports**

Replace `src/interactive/highlight.rs` with only the type definitions and `apply_style`, plus re-exports:

```rust
use std::io;

use crossterm::style::Color;

use super::terminal::Terminal;

// Re-export from submodules so existing `use super::highlight::...` imports work.
pub use super::command_checker::{CheckerEnv, CommandChecker, CommandExistence};
pub use super::highlight_scanner::HighlightScanner;

// ---------------------------------------------------------------------------
// HighlightStyle
// ---------------------------------------------------------------------------

/// Visual style applied to a span of characters in the input line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightStyle {
    Default,
    Keyword,
    Operator,
    Redirect,
    String,
    DoubleString,
    Variable,
    CommandSub,
    ArithSub,
    Comment,
    CommandValid,
    CommandInvalid,
    IoNumber,
    Assignment,
    Tilde,
    Error,
}

// ---------------------------------------------------------------------------
// ColorSpan
// ---------------------------------------------------------------------------

/// A half-open byte range [start, end) with an associated style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorSpan {
    pub start: usize,
    pub end: usize,
    pub style: HighlightStyle,
}

// ---------------------------------------------------------------------------
// apply_style
// ---------------------------------------------------------------------------

/// Apply the terminal attributes associated with `style`.
pub fn apply_style<T: Terminal>(term: &mut T, style: HighlightStyle) -> io::Result<()> {
    match style {
        HighlightStyle::Default => {
            // No styling needed.
        }
        HighlightStyle::Keyword => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Magenta)?;
        }
        HighlightStyle::Operator | HighlightStyle::Redirect => {
            term.set_fg_color(Color::Cyan)?;
        }
        HighlightStyle::String | HighlightStyle::DoubleString => {
            term.set_fg_color(Color::Yellow)?;
        }
        HighlightStyle::Variable | HighlightStyle::Tilde => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Green)?;
        }
        HighlightStyle::CommandSub | HighlightStyle::ArithSub => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Yellow)?;
        }
        HighlightStyle::Comment => {
            term.set_dim(true)?;
        }
        HighlightStyle::CommandValid => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Green)?;
        }
        HighlightStyle::CommandInvalid => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Red)?;
        }
        HighlightStyle::IoNumber | HighlightStyle::Assignment => {
            term.set_fg_color(Color::Blue)?;
        }
        HighlightStyle::Error => {
            term.set_fg_color(Color::Red)?;
            term.set_underline(true)?;
        }
    }
    Ok(())
}
```

- [ ] **Step 5: Update module declarations in interactive/mod.rs**

Add the new module declarations near the top of `src/interactive/mod.rs`, alongside the existing module declarations:

```rust
pub mod command_checker;
pub mod highlight_scanner;
```

- [ ] **Step 6: Build to verify compilation**

Run: `cargo build 2>&1 | tail -10`
Expected: Compiles successfully.

- [ ] **Step 7: Run full test suite**

Run: `cargo test 2>&1 | tail -5`
Expected: All pass.

- [ ] **Step 8: Commit**

```bash
git add src/interactive/command_checker.rs src/interactive/highlight_scanner.rs src/interactive/highlight.rs src/interactive/mod.rs
git commit -m "refactor: split highlight.rs into command_checker.rs, highlight_scanner.rs, and highlight.rs"
```

---

## Phase 4: Error Handling Migration

### Task 7: Extend RuntimeErrorKind and add exit_code method

**Files:**
- Modify: `src/error.rs`

- [ ] **Step 1: Write test for new variants and exit_code**

Add to the existing `mod tests` block in `src/error.rs`:

```rust
    #[test]
    fn test_runtime_error_new_variants() {
        let err = ShellError::runtime(RuntimeErrorKind::InvalidArgument, "bad arg");
        assert_eq!(err.to_string(), "yosh: bad arg");

        let err = ShellError::runtime(RuntimeErrorKind::IoError, "read failed");
        assert_eq!(err.to_string(), "yosh: read failed");
    }

    #[test]
    fn test_exit_code_mapping() {
        assert_eq!(ShellError::runtime(RuntimeErrorKind::CommandNotFound, "x").exit_code(), 127);
        assert_eq!(ShellError::runtime(RuntimeErrorKind::PermissionDenied, "x").exit_code(), 126);
        assert_eq!(ShellError::runtime(RuntimeErrorKind::InvalidArgument, "x").exit_code(), 2);
        assert_eq!(ShellError::runtime(RuntimeErrorKind::IoError, "x").exit_code(), 1);
        assert_eq!(ShellError::runtime(RuntimeErrorKind::RedirectFailed, "x").exit_code(), 1);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test error::tests 2>&1 | tail -10`
Expected: FAIL — new variants and `exit_code` not found.

- [ ] **Step 3: Add new variants and exit_code**

In `src/error.rs`, update `RuntimeErrorKind`:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RuntimeErrorKind {
    CommandNotFound,
    PermissionDenied,
    RedirectFailed,
    ReadonlyVariable,
    InvalidOption,
    InvalidArgument,
    IoError,
    ExecFailed,
    TrapError,
    JobControlError,
}
```

Remove the `#[allow(dead_code)]` attributes from `RuntimeErrorKind`, `ExpansionErrorKind`, and `ShellError::runtime()` since they will now be used.

Add an `exit_code` method to `ShellError`:

```rust
impl ShellError {
    // ... existing methods ...

    /// Map this error to an appropriate POSIX exit code.
    pub fn exit_code(&self) -> i32 {
        match &self.kind {
            ShellErrorKind::Parse(_) => 2,
            ShellErrorKind::Expansion(_) => 1,
            ShellErrorKind::Runtime(r) => match r {
                RuntimeErrorKind::CommandNotFound => 127,
                RuntimeErrorKind::PermissionDenied | RuntimeErrorKind::ExecFailed => 126,
                RuntimeErrorKind::InvalidOption | RuntimeErrorKind::InvalidArgument => 2,
                _ => 1,
            },
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test error::tests 2>&1 | tail -10`
Expected: All 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/error.rs
git commit -m "refactor: extend RuntimeErrorKind with new variants, add exit_code()"
```

---

### Task 8: Migrate builtin/regular.rs to Result<i32, ShellError>

**Files:**
- Modify: `src/builtin/regular.rs`
- Modify: `src/builtin/mod.rs:43-67`

This file has 19 `eprintln!("yosh: ...")` sites. The pattern for each migration:

```rust
// Before
eprintln!("yosh: cd: HOME not set");
return 1;

// After
return Err(ShellError::runtime(RuntimeErrorKind::InvalidArgument, "cd: HOME not set"));
```

- [ ] **Step 1: Change function signatures in regular.rs**

Update the import at the top of `src/builtin/regular.rs`:

```rust
use crate::env::ShellEnv;
use crate::error::{ShellError, RuntimeErrorKind};
use nix::unistd::Pid;
```

Change all public function signatures:
- `pub fn builtin_cd(args: &[String], env: &mut ShellEnv) -> i32` → `pub fn builtin_cd(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError>`
- `pub fn builtin_echo(args: &[String]) -> i32` → `pub fn builtin_echo(args: &[String]) -> Result<i32, ShellError>`
- `pub fn builtin_alias(args: &[String], env: &mut ShellEnv) -> i32` → `pub fn builtin_alias(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError>`
- `pub fn builtin_unalias(args: &[String], env: &mut ShellEnv) -> i32` → `pub fn builtin_unalias(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError>`
- `pub fn builtin_kill(args: &[String], shell_pgid: Pid) -> i32` → `pub fn builtin_kill(args: &[String], shell_pgid: Pid) -> Result<i32, ShellError>`
- `pub fn builtin_umask(args: &[String]) -> i32` → `pub fn builtin_umask(args: &[String]) -> Result<i32, ShellError>`

- [ ] **Step 2: Migrate each eprintln + return site in regular.rs**

For each function, replace `eprintln!("yosh: ...")` + `return N` with `return Err(...)`, and wrap success returns with `Ok(...)`.

Example for `builtin_cd`:

```rust
pub fn builtin_cd(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    let is_dash = !args.is_empty() && args[0] == "-";

    let target = if args.is_empty() {
        match env.vars.get("HOME") {
            Some(h) => h.to_string(),
            None => {
                return Err(ShellError::runtime(RuntimeErrorKind::InvalidArgument, "cd: HOME not set"));
            }
        }
    } else if args[0] == "-" {
        match env.vars.get("OLDPWD") {
            Some(old) => old.to_string(),
            None => {
                return Err(ShellError::runtime(RuntimeErrorKind::InvalidArgument, "cd: OLDPWD not set"));
            }
        }
    } else {
        args[0].clone()
    };

    let old_pwd = std::env::current_dir().ok();

    match std::env::set_current_dir(&target) {
        Ok(_) => {
            if let Some(old) = old_pwd {
                let _ = env.vars.set("OLDPWD", old.to_string_lossy().to_string());
            }
            if is_dash {
                println!("{}", target);
            }
            match std::env::current_dir() {
                Ok(cwd) => {
                    let cwd_str = cwd.to_string_lossy().into_owned();
                    let _ = env.vars.set("PWD", cwd_str);
                }
                Err(e) => {
                    eprintln!("yosh: cd: could not determine new directory: {}", e);
                }
            }
            Ok(0)
        }
        Err(e) => {
            Err(ShellError::runtime(RuntimeErrorKind::IoError, format!("cd: {}: {}", target, e)))
        }
    }
}
```

Apply the same pattern to all other functions. For `builtin_echo`, which has no error paths, simply wrap returns: `Ok(0)`.

For functions that accumulate a status variable (like `builtin_alias`, `builtin_kill`), keep the `eprintln!` for per-item errors that don't abort the whole command, and only return `Ok(status)` at the end. Convert fatal errors (like usage errors) to `Err(...)`.

Note: `builtin_umask` helper functions (`umask_set_octal`, `umask_set_symbolic`) should also return `Result<i32, ShellError>`. `parse_kill_signal` already returns `Result` so keep its existing error handling.

- [ ] **Step 3: Update exec_regular_builtin dispatcher in mod.rs**

In `src/builtin/mod.rs`, change `exec_regular_builtin` to handle `Result`:

```rust
pub fn exec_regular_builtin(name: &str, args: &[String], env: &mut ShellEnv) -> i32 {
    let result = match name {
        "cd" => regular::builtin_cd(args, env),
        "true" => Ok(0),
        "false" => Ok(1),
        "echo" => regular::builtin_echo(args),
        "umask" => regular::builtin_umask(args),
        "alias" => regular::builtin_alias(args, env),
        "unalias" => regular::builtin_unalias(args, env),
        "kill" => regular::builtin_kill(args, env.process.shell_pgid),
        "wait" => {
            eprintln!("yosh: wait: internal error");
            Ok(1)
        }
        "fg" | "bg" | "jobs" => {
            eprintln!("yosh: {}: internal error", name);
            Ok(1)
        }
        _ => {
            eprintln!("yosh: {}: not a regular builtin", name);
            Ok(1)
        }
    };
    match result {
        Ok(status) => status,
        Err(e) => {
            eprintln!("{}", e);
            e.exit_code()
        }
    }
}
```

The dispatcher unwraps `Result` at this boundary, so callers of `exec_regular_builtin` are unchanged.

- [ ] **Step 4: Run full tests**

Run: `cargo test 2>&1 | tail -5 && ./e2e/run_tests.sh 2>&1 | tail -5`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/builtin/regular.rs src/builtin/mod.rs
git commit -m "refactor: migrate builtin/regular.rs to Result<i32, ShellError>"
```

---

### Task 9: Migrate builtin/special.rs to Result<i32, ShellError>

**Files:**
- Modify: `src/builtin/special.rs`

Same pattern as Task 8. This file has 36 `eprintln!("yosh: ...")` sites.

- [ ] **Step 1: Add import and change exec_special_builtin signature**

Add import:
```rust
use crate::error::{ShellError, RuntimeErrorKind};
```

Change `exec_special_builtin` to handle `Result` from the individual functions:

```rust
pub fn exec_special_builtin(name: &str, args: &[String], executor: &mut Executor) -> i32 {
    let result = match name {
        ":" => Ok(0),
        "exit" => builtin_exit(args, executor),
        "export" => builtin_export(args, &mut executor.env),
        // ... all other arms, same as current but each function returns Result<i32, ShellError>
        _ => Err(ShellError::runtime(RuntimeErrorKind::InvalidArgument,
            format!("{}: not a special builtin", name))),
    };
    match result {
        Ok(status) => status,
        Err(e) => {
            eprintln!("{}", e);
            e.exit_code()
        }
    }
}
```

Note: the `"set"` arm that checks monitor mode change needs special handling — the monitor logic runs after the Result is unwrapped:

```rust
        "set" => {
            let was_monitor = executor.env.mode.options.monitor;
            let ret = match builtin_set(args, &mut executor.env) {
                Ok(s) => s,
                Err(e) => { eprintln!("{}", e); return e.exit_code(); }
            };
            let is_monitor = executor.env.mode.options.monitor;
            if was_monitor && !is_monitor {
                crate::signal::reset_job_control_signals();
            } else if !was_monitor && is_monitor {
                crate::signal::init_job_control_signals();
            }
            return ret;
        }
```

- [ ] **Step 2: Migrate each function in special.rs**

Change each function's return type to `Result<i32, ShellError>` and replace `eprintln!("yosh: ...")` + `return N` with `return Err(ShellError::runtime(...))`. Wrap success returns with `Ok(...)`.

Key mappings:
- `"yosh: exit: ... numeric argument required"` → `RuntimeErrorKind::InvalidArgument`
- `"yosh: .: filename argument required"` → `RuntimeErrorKind::InvalidArgument`
- `"yosh: .: ... not found"` → `RuntimeErrorKind::CommandNotFound`
- `"yosh: .: ... {}: {}"` (IO error) → `RuntimeErrorKind::IoError`
- `"yosh: export: ..."` → `RuntimeErrorKind::ReadonlyVariable` or `RuntimeErrorKind::InvalidArgument`
- `"yosh: return: ..."` → `RuntimeErrorKind::InvalidArgument`
- `"yosh: break: ..."` / `"yosh: continue: ..."` → `RuntimeErrorKind::InvalidArgument`
- `"yosh: eval: ..."` → `RuntimeErrorKind::InvalidArgument`
- `"yosh: exec: ... not found"` → `RuntimeErrorKind::CommandNotFound`
- `"yosh: exec: ... permission denied"` → `RuntimeErrorKind::PermissionDenied`
- `"yosh: exec: ... invalid command name"` → `RuntimeErrorKind::ExecFailed`
- `"yosh: {}"` (trap error) → `RuntimeErrorKind::TrapError`
- `"yosh: fc: ..."` → `RuntimeErrorKind::InvalidArgument` or `RuntimeErrorKind::IoError`
- `"yosh: set: ..."` → `RuntimeErrorKind::InvalidOption`
- `"yosh: shift: ..."` → `RuntimeErrorKind::InvalidArgument`
- `"yosh: times: failed"` → `RuntimeErrorKind::IoError`

For functions that accumulate errors per-item (like `builtin_export`, `builtin_unset`, `builtin_trap`), keep `eprintln!` for non-fatal per-item errors and return `Ok(status)`. Only convert fatal errors (argument validation, missing files) to `Err(...)`.

- [ ] **Step 3: Run full tests**

Run: `cargo test 2>&1 | tail -5 && ./e2e/run_tests.sh 2>&1 | tail -5`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add src/builtin/special.rs
git commit -m "refactor: migrate builtin/special.rs to Result<i32, ShellError>"
```

---

### Task 10: Migrate exec/simple.rs to Result<i32, ShellError>

**Files:**
- Modify: `src/exec/simple.rs`

This file has 24 `eprintln!` sites. The main function `exec_simple_command` is the critical path.

- [ ] **Step 1: Change exec_simple_command signature**

Change `pub(crate) fn exec_simple_command(&mut self, cmd: &SimpleCommand) -> i32` to `pub(crate) fn exec_simple_command(&mut self, cmd: &SimpleCommand) -> Result<i32, ShellError>`.

Add import:
```rust
use crate::error::{ShellError, RuntimeErrorKind};
```

- [ ] **Step 2: Migrate error sites in exec_simple_command**

The repeated pattern in this file is:
```rust
// Before
if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
    eprintln!("yosh: {}", e);
    self.env.exec.last_exit_status = 1;
    return 1;
}

// After
if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
    self.env.exec.last_exit_status = 1;
    return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
}
```

And for expansion errors:
```rust
// Before
Err(e) => {
    eprintln!("{}", e);
    self.env.exec.last_exit_status = 1;
    return 1;
}

// After (expansion errors are already ShellError, just propagate)
Err(e) => {
    self.env.exec.last_exit_status = 1;
    return Err(e);
}
```

For assignment errors:
```rust
// Before
if let Err(e) = self.env.vars.set_with_options(&assignment.name, value, self.env.mode.options.allexport) {
    eprintln!("yosh: {}", e);
    self.env.exec.last_exit_status = 1;
    return 1;
}

// After
if let Err(e) = self.env.vars.set_with_options(&assignment.name, value, self.env.mode.options.allexport) {
    self.env.exec.last_exit_status = 1;
    return Err(ShellError::runtime(RuntimeErrorKind::ReadonlyVariable, e));
}
```

Wrap all successful `return status` with `return Ok(status)` and the final return with `Ok(status)`.

- [ ] **Step 3: Update callers**

In `src/exec/mod.rs`, update `exec_command` to handle `Result`:

```rust
pub fn exec_command(&mut self, cmd: &Command) -> i32 {
    if self.env.mode.options.noexec {
        return 0;
    }
    match cmd {
        Command::Simple(simple) => match self.exec_simple_command(simple) {
            Ok(status) => status,
            Err(e) => {
                eprintln!("{}", e);
                self.env.exec.last_exit_status = e.exit_code();
                e.exit_code()
            }
        },
        Command::Compound(compound, redirects) => {
            self.exec_compound_command(compound, redirects)
        }
        Command::FunctionDef(func_def) => {
            self.env
                .functions
                .insert(func_def.name.clone(), func_def.clone());
            0
        }
    }
}
```

- [ ] **Step 4: Run full tests**

Run: `cargo test 2>&1 | tail -5 && ./e2e/run_tests.sh 2>&1 | tail -5`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/exec/simple.rs src/exec/mod.rs
git commit -m "refactor: migrate exec/simple.rs to Result<i32, ShellError>"
```

---

### Task 11: Migrate exec/compound.rs

**Files:**
- Modify: `src/exec/compound.rs`

This file has 6 `eprintln!` sites. All are redirect or expansion errors.

- [ ] **Step 1: Migrate exec_compound_command**

Add import:
```rust
use crate::error::{ShellError, RuntimeErrorKind};
```

Change `exec_compound_command` to return `Result<i32, ShellError>`:

```rust
pub(crate) fn exec_compound_command(
    &mut self,
    compound: &CompoundCommand,
    redirects: &[Redirect],
) -> Result<i32, ShellError> {
    let mut redirect_state = RedirectState::new();
    if let Err(e) = redirect_state.apply(redirects, &mut self.env, true) {
        self.env.exec.last_exit_status = 1;
        return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
    }
    // ... rest unchanged but wrap returns with Ok()
```

`exec_subshell` uses `eprintln!("yosh: fork: {}", e)`:
```rust
Err(e) => {
    return Err(ShellError::runtime(RuntimeErrorKind::IoError, format!("fork: {}", e)));
}
```

For `exec_for` and `exec_case`, expansion errors (`Err(e)`) are already `ShellError` — just propagate with `?` or `return Err(e)`.

For `exec_for` variable set error:
```rust
if let Err(e) = self.env.vars.set(var, item.as_str()) {
    return Err(ShellError::runtime(RuntimeErrorKind::ReadonlyVariable, format!("{}", e)));
}
```

- [ ] **Step 2: Update caller in exec/mod.rs**

In `exec_command`, update the `Compound` arm:

```rust
Command::Compound(compound, redirects) => {
    match self.exec_compound_command(compound, redirects) {
        Ok(status) => status,
        Err(e) => {
            eprintln!("{}", e);
            self.env.exec.last_exit_status = e.exit_code();
            e.exit_code()
        }
    }
}
```

- [ ] **Step 3: Run full tests**

Run: `cargo test 2>&1 | tail -5 && ./e2e/run_tests.sh 2>&1 | tail -5`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add src/exec/compound.rs src/exec/mod.rs
git commit -m "refactor: migrate exec/compound.rs to Result<i32, ShellError>"
```

---

### Task 12: Migrate exec/pipeline.rs, exec/command.rs, exec/mod.rs remaining

**Files:**
- Modify: `src/exec/pipeline.rs`
- Modify: `src/exec/command.rs`
- Modify: `src/exec/mod.rs`
- Modify: `src/builtin/mod.rs`

These files have a combined ~26 `eprintln!` sites. Many are in fork-child contexts where `eprintln!` + `_exit()` is the correct pattern (the child process cannot return errors to the parent). Only parent-side errors should be migrated.

- [ ] **Step 1: Migrate pipeline.rs parent-side errors**

In `src/exec/pipeline.rs`, add import:
```rust
use crate::error::{ShellError, RuntimeErrorKind};
```

In `exec_multi_pipeline`, the pipe creation and fork errors are parent-side:

```rust
// pipe creation error
Err(e) => {
    close_all_pipes(&pipes);
    return Err(ShellError::runtime(RuntimeErrorKind::IoError, format!("pipe: {}", e)));
}

// fork error
Err(e) => {
    close_all_pipes(&pipes);
    return Err(ShellError::runtime(RuntimeErrorKind::IoError, format!("fork: {}", e)));
}
```

Change `exec_multi_pipeline` to return `Result<i32, ShellError>`, and update `exec_pipeline` to handle it:

```rust
pub fn exec_pipeline(&mut self, pipeline: &Pipeline) -> i32 {
    let status = if pipeline.commands.len() == 1 {
        self.exec_command(&pipeline.commands[0])
    } else {
        match self.exec_multi_pipeline(pipeline) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}", e);
                e.exit_code()
            }
        }
    };
    // ... negation logic unchanged
```

**Do NOT migrate child-side `eprintln!`** (dup2 errors followed by `libc::_exit`). Those are in a forked child and cannot return errors.

- [ ] **Step 2: Migrate command.rs**

In `src/exec/command.rs`, `wait_child` has one `eprintln!`:
```rust
Err(e) => {
    eprintln!("yosh: waitpid: {}", e);
    1
}
```

Change `wait_child` to return `Result<i32, ShellError>`:
```rust
pub fn wait_child(child: Pid) -> Result<i32, ShellError> {
    match waitpid(child, None) {
        Ok(WaitStatus::Exited(_, code)) => Ok(code),
        Ok(WaitStatus::Signaled(_, sig, _)) => Ok(128 + sig as i32),
        Ok(_) => Ok(0),
        Err(e) => Err(ShellError::runtime(RuntimeErrorKind::IoError, format!("waitpid: {}", e))),
    }
}
```

Update callers of `wait_child` (in `compound.rs` and `simple.rs`) to unwrap the result. In subshell/pipeline child-wait contexts, `wait_child(child).unwrap_or(1)` is appropriate since the parent should not crash if wait fails.

- [ ] **Step 3: Migrate remaining exec/mod.rs errors**

In `exec/mod.rs`, the remaining `eprintln!` sites are:
- `exec_async` fork error → migrate parent-side error
- `builtin_wait` errors → migrate
- `builtin_fg`/`builtin_bg` errors → migrate
- `display_job_notifications` → this is informational output (`eprintln!("{}", line)`) — do NOT migrate

For `exec_async`:
```rust
Err(e) => {
    return Err(ShellError::runtime(RuntimeErrorKind::IoError, format!("fork: {}", e)));
}
```

Change `exec_async` to return `Result<i32, ShellError>`, and update the caller in `exec_complete_command`:
```rust
if separator == &Some(SeparatorOp::Amp) {
    status = match self.exec_async(and_or) {
        Ok(s) => s,
        Err(e) => { eprintln!("{}", e); e.exit_code() }
    };
}
```

For `builtin_wait`, `builtin_fg`, `builtin_bg` — change return types to `Result<i32, ShellError>` and migrate error sites. Update their call sites in `exec/simple.rs` to unwrap results.

For `builtin/mod.rs` — the 3 `eprintln!` sites are internal error messages and the "not a regular builtin" fallback. These can stay as-is since they're already handled by the dispatcher wrapper from Task 8.

- [ ] **Step 4: Run full tests**

Run: `cargo test 2>&1 | tail -5 && ./e2e/run_tests.sh 2>&1 | tail -5`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/exec/pipeline.rs src/exec/command.rs src/exec/mod.rs src/exec/simple.rs src/exec/compound.rs
git commit -m "refactor: migrate exec/pipeline.rs, command.rs, and remaining exec/mod.rs to Result<ShellError>"
```

---

### Task 13: TODO.md cleanup

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Delete completed items from TODO.md**

Delete these items from `TODO.md` (per project convention — delete, don't mark with [x]):
- "Unify `builtin_source` and `source_file`" (line 64)
- "Consolidate tilde expansion logic" (line 65)
- "Rename `KISH_SHOW_DOTFILES` to `YOSH_SHOW_DOTFILES`" (line 66)
- "Extract quote-aware balanced-paren scanning into a shared helper" (line 67)
- "`print_help()` DRY refactor" (line 24)
- "Runtime error migration" (line 68)

If the "Code Quality Improvements" section header becomes empty after deletion, remove the section header too.

- [ ] **Step 2: Run final full test suite**

Run: `cargo test 2>&1 | tail -5 && ./e2e/run_tests.sh 2>&1 | tail -5`
Expected: All pass.

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "docs: remove completed refactoring items from TODO.md"
```
