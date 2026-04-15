# Command Name Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add tab-completion for command names (PATH executables, builtins, aliases) when cursor is in command position.

**Architecture:** A new `CommandCompleter` struct holds a lazy, session-scoped cache of PATH executables. `is_command_position()` detects whether the cursor is at command position. `handle_tab_complete` in `line_editor.rs` branches between command and path completion based on position. `CompletionContext` is kept unchanged; a new `CommandCompletionContext` struct carries the command-specific data alongside it.

**Tech Stack:** Rust, `std::os::unix::fs::PermissionsExt` for executable detection, existing `completion.rs` infrastructure.

---

### Task 1: Add `BUILTIN_NAMES` constant

**Files:**
- Modify: `src/builtin/mod.rs:19-29` (after `classify_builtin`)

- [ ] **Step 1: Add `BUILTIN_NAMES` constant**

Add a constant slice listing all builtin names at the top of `src/builtin/mod.rs`, after the imports and before `BuiltinKind`:

```rust
/// All builtin command names (special + regular) for tab-completion.
pub const BUILTIN_NAMES: &[&str] = &[
    // Special builtins
    "break", ":", "continue", ".", "eval", "exec", "exit", "export",
    "readonly", "return", "set", "shift", "times", "trap", "unset", "fc",
    // Regular builtins
    "cd", "echo", "true", "false", "alias", "unalias", "kill", "wait",
    "fg", "bg", "jobs", "umask",
];
```

- [ ] **Step 2: Add test verifying consistency with `classify_builtin`**

Add a test at the bottom of the existing `#[cfg(test)] mod tests` block in `src/builtin/mod.rs`:

```rust
#[test]
fn test_builtin_names_consistent_with_classify() {
    for &name in BUILTIN_NAMES {
        assert_ne!(
            classify_builtin(name),
            BuiltinKind::NotBuiltin,
            "{} is in BUILTIN_NAMES but classify_builtin returns NotBuiltin",
            name,
        );
    }
}
```

- [ ] **Step 3: Run test to verify it passes**

Run: `cargo test -p kish test_builtin_names_consistent -- --exact`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/builtin/mod.rs
git commit -m "feat(builtin): add BUILTIN_NAMES constant for tab-completion"
```

---

### Task 2: Add `is_command_position()` with tests

**Files:**
- Modify: `src/interactive/completion.rs:67` (after `is_unquoted_delimiter`)

- [ ] **Step 1: Write failing tests**

Add tests at the bottom of the existing `#[cfg(test)] mod tests` block in `src/interactive/completion.rs`:

```rust
// ── is_command_position ────────────────────────────────────────

#[test]
fn test_command_position_line_start() {
    assert!(is_command_position("", 0));
    assert!(is_command_position("gi", 0));
}

#[test]
fn test_command_position_after_pipe() {
    // "ls | gr" — word_start=5
    assert!(is_command_position("ls | gr", 5));
}

#[test]
fn test_command_position_after_semicolon() {
    // "echo a; ls" — word_start=8
    assert!(is_command_position("echo a; ls", 8));
}

#[test]
fn test_command_position_after_and_and() {
    // "true && ec" — word_start=8
    assert!(is_command_position("true && ec", 8));
}

#[test]
fn test_command_position_after_or_or() {
    // "false || ec" — word_start=9
    assert!(is_command_position("false || ec", 9));
}

#[test]
fn test_command_position_after_open_paren() {
    // "(ls" — word_start=1
    assert!(is_command_position("(ls", 1));
}

#[test]
fn test_command_position_after_bang() {
    // "! cmd" — word_start=2
    assert!(is_command_position("! cmd", 2));
}

#[test]
fn test_not_command_position_argument() {
    // "ls fo" — word_start=3
    assert!(!is_command_position("ls fo", 3));
}

#[test]
fn test_not_command_position_second_arg() {
    // "echo hello wor" — word_start=11
    assert!(!is_command_position("echo hello wor", 11));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kish test_command_position -- 2>&1 | head -20`
Expected: compilation error — `is_command_position` not found

- [ ] **Step 3: Implement `is_command_position`**

Add the function in `src/interactive/completion.rs` after the `is_unquoted_delimiter` function (after line 71):

```rust
/// Returns `true` if `word_start` is at command position in `buf`.
///
/// Command position means the word is the first token after:
/// - line start (nothing before it)
/// - `|`, `;`, `&`, `(`, `!`
///
/// Scans backward from `word_start`, skipping whitespace, and checks
/// the last non-whitespace character.
pub fn is_command_position(buf: &str, word_start: usize) -> bool {
    let before = buf[..word_start].trim_end();
    if before.is_empty() {
        return true;
    }
    matches!(before.as_bytes().last(), Some(b'|' | b';' | b'&' | b'(' | b'!'))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kish test_command_position`
Expected: all 9 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/interactive/completion.rs
git commit -m "feat(completion): add is_command_position() for command name completion"
```

---

### Task 3: Create `CommandCompleter` with tests

**Files:**
- Create: `src/interactive/command_completion.rs`
- Modify: `src/interactive/mod.rs:1` (add `pub mod command_completion;`)

- [ ] **Step 1: Create the module file with struct and constructor**

Create `src/interactive/command_completion.rs`:

```rust
//! Command name completion for interactive tab-completion.
//!
//! Provides `CommandCompleter` which caches PATH executables and generates
//! command name candidates (executables + builtins + aliases).

use std::collections::HashSet;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::env::aliases::AliasStore;
use super::completion::longest_common_prefix;

/// Caches PATH executables and generates command name completion candidates.
pub struct CommandCompleter {
    /// Sorted list of executable names from PATH.
    cached_executables: Vec<String>,
    /// PATH value when cache was built (for invalidation).
    cached_path: String,
}

impl CommandCompleter {
    pub fn new() -> Self {
        Self {
            cached_executables: Vec::new(),
            cached_path: String::new(),
        }
    }

    /// Return command name candidates matching `prefix`.
    ///
    /// Collects from aliases, builtins, and PATH executables (cached).
    /// Results are deduplicated and sorted.
    pub fn complete(
        &mut self,
        prefix: &str,
        path: &str,
        builtins: &[&str],
        aliases: &AliasStore,
    ) -> Vec<String> {
        // Rebuild cache if PATH changed
        if self.cached_path != path {
            self.rebuild_cache(path);
        }

        let mut candidates = Vec::new();

        // Aliases
        for (name, _) in aliases.sorted_iter() {
            if name.starts_with(prefix) {
                candidates.push(name.to_string());
            }
        }

        // Builtins
        for &name in builtins {
            if name.starts_with(prefix) {
                candidates.push(name.to_string());
            }
        }

        // PATH executables (from cache)
        for name in &self.cached_executables {
            if name.starts_with(prefix) {
                candidates.push(name.clone());
            }
        }

        // Deduplicate and sort
        candidates.sort();
        candidates.dedup();
        candidates
    }

    /// Compute the longest common prefix of command candidates.
    pub fn complete_common_prefix(
        &mut self,
        prefix: &str,
        path: &str,
        builtins: &[&str],
        aliases: &AliasStore,
    ) -> (Vec<String>, String) {
        let candidates = self.complete(prefix, path, builtins, aliases);
        let common = longest_common_prefix(&candidates);
        (candidates, common)
    }

    fn rebuild_cache(&mut self, path: &str) {
        let mut seen = HashSet::new();
        let mut executables = Vec::new();

        for dir in path.split(':') {
            if dir.is_empty() {
                continue;
            }
            let entries = match fs::read_dir(dir) {
                Ok(rd) => rd,
                Err(_) => continue,
            };
            for entry in entries.filter_map(|e| e.ok()) {
                let name = match entry.file_name().into_string() {
                    Ok(n) => n,
                    Err(_) => continue,
                };
                // Skip hidden files and already-seen names
                if name.starts_with('.') || seen.contains(&name) {
                    continue;
                }
                // Check if file is executable
                if Self::is_executable(&entry) {
                    seen.insert(name.clone());
                    executables.push(name);
                }
            }
        }

        executables.sort();
        self.cached_executables = executables;
        self.cached_path = path.to_string();
    }

    #[cfg(unix)]
    fn is_executable(entry: &fs::DirEntry) -> bool {
        entry
            .file_type()
            .map(|ft| ft.is_file() || ft.is_symlink())
            .unwrap_or(false)
            && entry
                .metadata()
                .map(|m| m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
    }

    #[cfg(not(unix))]
    fn is_executable(entry: &fs::DirEntry) -> bool {
        entry
            .file_type()
            .map(|ft| ft.is_file())
            .unwrap_or(false)
    }
}

/// Context for command-name completion, passed alongside `CompletionContext`.
pub struct CommandCompletionContext<'a> {
    pub completer: &'a mut CommandCompleter,
    pub path: &'a str,
    pub builtins: &'a [&'static str],
    pub aliases: &'a AliasStore,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use tempfile::TempDir;

    fn make_executable(path: &std::path::Path) {
        File::create(path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    fn make_non_executable(path: &std::path::Path) {
        File::create(path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
        }
    }

    #[test]
    fn test_complete_prefix_match() {
        let tmp = TempDir::new().unwrap();
        make_executable(&tmp.path().join("git"));
        make_executable(&tmp.path().join("grep"));
        make_executable(&tmp.path().join("ls"));

        let mut completer = CommandCompleter::new();
        let aliases = AliasStore::default();
        let path = tmp.path().to_str().unwrap();
        let candidates = completer.complete("g", path, &[], &aliases);
        assert_eq!(candidates, vec!["git", "grep"]);
    }

    #[test]
    fn test_complete_includes_builtins() {
        let tmp = TempDir::new().unwrap();
        let mut completer = CommandCompleter::new();
        let aliases = AliasStore::default();
        let path = tmp.path().to_str().unwrap();
        let builtins = &["echo", "eval", "exec"][..];
        let candidates = completer.complete("e", path, builtins, &aliases);
        assert_eq!(candidates, vec!["echo", "eval", "exec"]);
    }

    #[test]
    fn test_complete_includes_aliases() {
        let tmp = TempDir::new().unwrap();
        let mut completer = CommandCompleter::new();
        let mut aliases = AliasStore::default();
        aliases.set("ll", "ls -l");
        aliases.set("la", "ls -a");
        let path = tmp.path().to_str().unwrap();
        let candidates = completer.complete("l", path, &[], &aliases);
        assert_eq!(candidates, vec!["la", "ll"]);
    }

    #[test]
    fn test_complete_deduplicates() {
        let tmp = TempDir::new().unwrap();
        make_executable(&tmp.path().join("echo"));

        let mut completer = CommandCompleter::new();
        let aliases = AliasStore::default();
        let path = tmp.path().to_str().unwrap();
        let builtins = &["echo"][..];
        let candidates = completer.complete("echo", path, builtins, &aliases);
        // "echo" appears in both builtins and PATH but should appear only once
        assert_eq!(candidates, vec!["echo"]);
    }

    #[test]
    fn test_complete_empty_prefix_returns_all() {
        let tmp = TempDir::new().unwrap();
        make_executable(&tmp.path().join("git"));
        make_executable(&tmp.path().join("ls"));

        let mut completer = CommandCompleter::new();
        let mut aliases = AliasStore::default();
        aliases.set("ll", "ls -l");
        let path = tmp.path().to_str().unwrap();
        let builtins = &["cd"][..];
        let candidates = completer.complete("", path, builtins, &aliases);
        assert!(candidates.contains(&"git".to_string()));
        assert!(candidates.contains(&"ls".to_string()));
        assert!(candidates.contains(&"ll".to_string()));
        assert!(candidates.contains(&"cd".to_string()));
    }

    #[test]
    fn test_skips_non_executable_files() {
        let tmp = TempDir::new().unwrap();
        make_executable(&tmp.path().join("runnable"));
        make_non_executable(&tmp.path().join("readme.txt"));

        let mut completer = CommandCompleter::new();
        let aliases = AliasStore::default();
        let path = tmp.path().to_str().unwrap();
        let candidates = completer.complete("r", path, &[], &aliases);
        assert_eq!(candidates, vec!["runnable"]);
    }

    #[test]
    fn test_cache_invalidation_on_path_change() {
        let tmp1 = TempDir::new().unwrap();
        let tmp2 = TempDir::new().unwrap();
        make_executable(&tmp1.path().join("alpha"));
        make_executable(&tmp2.path().join("beta"));

        let mut completer = CommandCompleter::new();
        let aliases = AliasStore::default();

        // First completion with tmp1
        let path1 = tmp1.path().to_str().unwrap();
        let c1 = completer.complete("", path1, &[], &aliases);
        assert!(c1.contains(&"alpha".to_string()));
        assert!(!c1.contains(&"beta".to_string()));

        // Change PATH to tmp2 — cache should rebuild
        let path2 = tmp2.path().to_str().unwrap();
        let c2 = completer.complete("", path2, &[], &aliases);
        assert!(!c2.contains(&"alpha".to_string()));
        assert!(c2.contains(&"beta".to_string()));
    }

    #[test]
    fn test_path_priority_first_dir_wins() {
        let tmp1 = TempDir::new().unwrap();
        let tmp2 = TempDir::new().unwrap();
        make_executable(&tmp1.path().join("mycmd"));
        make_executable(&tmp2.path().join("mycmd"));

        let mut completer = CommandCompleter::new();
        let aliases = AliasStore::default();
        let path = format!(
            "{}:{}",
            tmp1.path().to_str().unwrap(),
            tmp2.path().to_str().unwrap()
        );
        let candidates = completer.complete("mycmd", &path, &[], &aliases);
        // Should appear only once despite being in both dirs
        assert_eq!(candidates, vec!["mycmd"]);
    }

    #[test]
    fn test_complete_common_prefix() {
        let tmp = TempDir::new().unwrap();
        make_executable(&tmp.path().join("git"));
        make_executable(&tmp.path().join("grep"));

        let mut completer = CommandCompleter::new();
        let aliases = AliasStore::default();
        let path = tmp.path().to_str().unwrap();
        let (candidates, common) =
            completer.complete_common_prefix("g", path, &[], &aliases);
        assert_eq!(candidates, vec!["git", "grep"]);
        assert_eq!(common, "g");
    }
}
```

- [ ] **Step 2: Register the module in `src/interactive/mod.rs`**

Add `pub mod command_completion;` after the existing module declarations (after `pub mod completion;`, line 1):

```rust
pub mod command_completion;
pub mod completion;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p kish command_completion`
Expected: all 9 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/interactive/command_completion.rs src/interactive/mod.rs
git commit -m "feat(completion): add CommandCompleter with PATH caching"
```

---

### Task 4: Integrate command completion into `handle_tab_complete`

**Files:**
- Modify: `src/interactive/line_editor.rs:1-15` (imports)
- Modify: `src/interactive/line_editor.rs:838-854` (`read_line_with_completion` signature)
- Modify: `src/interactive/line_editor.rs:857-960` (`read_line_loop_with_completion` signature + tab branch)
- Modify: `src/interactive/line_editor.rs:963-1018` (`handle_tab_complete` logic)

- [ ] **Step 1: Add import for `command_completion` and `is_command_position`**

In `src/interactive/line_editor.rs`, update the import on line 5:

Change:
```rust
use super::completion::{self, CompletionContext, CompletionUI};
```
To:
```rust
use super::completion::{self, CompletionContext, CompletionUI, is_command_position, extract_completion_word, longest_common_prefix};
use super::command_completion::CommandCompletionContext;
```

- [ ] **Step 2: Add `cmd_ctx` parameter to `read_line_with_completion`**

In `src/interactive/line_editor.rs`, update the `read_line_with_completion` method signature (around line 838). Add `cmd_ctx: &mut CommandCompletionContext<'_>` parameter:

Change:
```rust
    pub fn read_line_with_completion<T: Terminal>(
        &mut self,
        prompt: &str,
        upper_lines: &[String],
        history: &mut History,
        term: &mut T,
        ctx: &CompletionContext,
        scanner: &mut HighlightScanner,
        checker_env: &CheckerEnv<'_>,
        accumulated: &str,
    ) -> io::Result<Option<String>> {
        self.clear();
        term.enable_raw_mode()?;
        let result = self.read_line_loop_with_completion(prompt, upper_lines, history, term, ctx, scanner, checker_env, accumulated);
        let _ = term.disable_raw_mode();
        result
    }
```
To:
```rust
    pub fn read_line_with_completion<T: Terminal>(
        &mut self,
        prompt: &str,
        upper_lines: &[String],
        history: &mut History,
        term: &mut T,
        ctx: &CompletionContext,
        cmd_ctx: &mut CommandCompletionContext<'_>,
        scanner: &mut HighlightScanner,
        checker_env: &CheckerEnv<'_>,
        accumulated: &str,
    ) -> io::Result<Option<String>> {
        self.clear();
        term.enable_raw_mode()?;
        let result = self.read_line_loop_with_completion(prompt, upper_lines, history, term, ctx, cmd_ctx, scanner, checker_env, accumulated);
        let _ = term.disable_raw_mode();
        result
    }
```

- [ ] **Step 3: Add `cmd_ctx` parameter to `read_line_loop_with_completion`**

Update the `read_line_loop_with_completion` method signature (around line 857). Add `cmd_ctx: &mut CommandCompletionContext<'_>` parameter:

Change:
```rust
    fn read_line_loop_with_completion<T: Terminal>(
        &mut self,
        prompt: &str,
        upper_lines: &[String],
        history: &mut History,
        term: &mut T,
        ctx: &CompletionContext,
        scanner: &mut HighlightScanner,
        checker_env: &CheckerEnv<'_>,
        accumulated: &str,
    ) -> io::Result<Option<String>> {
```
To:
```rust
    fn read_line_loop_with_completion<T: Terminal>(
        &mut self,
        prompt: &str,
        upper_lines: &[String],
        history: &mut History,
        term: &mut T,
        ctx: &CompletionContext,
        cmd_ctx: &mut CommandCompletionContext<'_>,
        scanner: &mut HighlightScanner,
        checker_env: &CheckerEnv<'_>,
        accumulated: &str,
    ) -> io::Result<Option<String>> {
```

And update the `TabComplete` branch inside this method (around line 933-935):

Change:
```rust
                        KeyAction::TabComplete => {
                            // Tab completion
                            self.handle_tab_complete(term, prompt, upper_lines, ctx)?;
```
To:
```rust
                        KeyAction::TabComplete => {
                            // Tab completion
                            self.handle_tab_complete(term, prompt, upper_lines, ctx, cmd_ctx)?;
```

- [ ] **Step 4: Rewrite `handle_tab_complete` to branch between command and path completion**

Replace the `handle_tab_complete` method (lines 963-1018) with:

```rust
    fn handle_tab_complete<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        upper_lines: &[String],
        ctx: &CompletionContext,
        cmd_ctx: &mut CommandCompletionContext<'_>,
    ) -> io::Result<()> {
        let (word_start, word) = extract_completion_word(&self.buffer(), self.pos);

        let (candidates, common_prefix, dir_prefix) =
            if is_command_position(&self.buffer(), word_start) && !word.contains('/') {
                // Command name completion
                let (cands, common) = cmd_ctx.completer.complete_common_prefix(
                    word,
                    cmd_ctx.path,
                    cmd_ctx.builtins,
                    cmd_ctx.aliases,
                );
                (cands, common, String::new())
            } else {
                // Path completion (existing)
                let result = completion::complete(&self.buffer(), self.pos, ctx);
                (result.candidates, result.common_prefix, result.dir_prefix)
            };

        if candidates.is_empty() {
            return Ok(());
        }

        if self.tab_count == 1 {
            if candidates.len() == 1 {
                // Single candidate: replace word
                let candidate = &candidates[0];
                let is_dir = candidate.ends_with('/');
                let mut replacement = format!("{}{}", dir_prefix, candidate);
                if !is_dir {
                    replacement.push(' ');
                }
                self.replace_word(word_start, &replacement);
            } else {
                // Multiple candidates: replace with common prefix if longer
                let current_word = &self.buffer()[word_start..self.pos];
                let new_word = format!("{}{}", dir_prefix, common_prefix);
                if new_word.len() > current_word.len() {
                    self.replace_word(word_start, &new_word);
                }
            }
        } else if self.tab_count >= 2 && candidates.len() >= 2 {
            // Show interactive completion UI
            self.suggestion = None;
            term.disable_raw_mode()?;
            let selected = CompletionUI::run(&candidates, term)?;
            if let Some(sel) = selected {
                let is_dir = sel.ends_with('/');
                let mut replacement = format!("{}{}", dir_prefix, sel);
                if !is_dir {
                    replacement.push(' ');
                }
                self.replace_word(word_start, &replacement);
            }
            term.enable_raw_mode()?;
            term.move_to_column(0)?;
            term.clear_current_line()?;
            for line in upper_lines {
                term.write_str(line)?;
                term.write_str("\r\n")?;
            }
            term.write_str(prompt)?;
        }

        Ok(())
    }
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p kish 2>&1 | head -30`
Expected: errors only from call sites not yet updated (mod.rs and tests)

- [ ] **Step 6: Commit**

```bash
git add src/interactive/line_editor.rs
git commit -m "feat(completion): branch handle_tab_complete for command vs path completion"
```

---

### Task 5: Wire up `CommandCompleter` in `Repl`

**Files:**
- Modify: `src/interactive/mod.rs:1-14` (imports)
- Modify: `src/interactive/mod.rs:27-32` (`Repl` struct)
- Modify: `src/interactive/mod.rs:35-64` (`Repl::new`)
- Modify: `src/interactive/mod.rs:92-118` (completion context + `read_line_with_completion` call)

- [ ] **Step 1: Add import and field to `Repl`**

In `src/interactive/mod.rs`, add the import after `use completion::CompletionContext;` (line 20):

```rust
use command_completion::{CommandCompleter, CommandCompletionContext};
```

Add `command_completer: CommandCompleter,` to the `Repl` struct (after `scanner: HighlightScanner,`):

```rust
pub struct Repl {
    executor: Executor,
    line_editor: LineEditor,
    terminal: CrosstermTerminal,
    scanner: HighlightScanner,
    command_completer: CommandCompleter,
}
```

- [ ] **Step 2: Initialize in `Repl::new`**

Add `command_completer: CommandCompleter::new(),` to the `Self { ... }` block in `Repl::new()`:

```rust
        Self {
            executor,
            line_editor: LineEditor::new(),
            terminal: CrosstermTerminal::new(),
            scanner: HighlightScanner::new(),
            command_completer: CommandCompleter::new(),
        }
```

- [ ] **Step 3: Build `CommandCompletionContext` and pass to `read_line_with_completion`**

In the `run()` method, after building `comp_ctx` (around line 100), build the command completion context and pass it. Update the section from `let comp_ctx` through the `read_line_with_completion` call:

After line 100 (`let comp_ctx = CompletionContext { cwd, home, show_dotfiles };`), add:

```rust
            let mut cmd_ctx = CommandCompletionContext {
                completer: &mut self.command_completer,
                path: &path_val,
                builtins: crate::builtin::BUILTIN_NAMES,
                aliases: &self.executor.env.aliases,
            };
```

Update the `read_line_with_completion` call to include `&mut cmd_ctx`:

Change:
```rust
            let line = match self.line_editor.read_line_with_completion(
                &prompt_info.last_line,
                &prompt_info.upper_lines,
                &mut self.executor.env.history,
                &mut self.terminal,
                &comp_ctx,
                &mut self.scanner,
                &checker_env,
                &input_buffer,
            ) {
```
To:
```rust
            let line = match self.line_editor.read_line_with_completion(
                &prompt_info.last_line,
                &prompt_info.upper_lines,
                &mut self.executor.env.history,
                &mut self.terminal,
                &comp_ctx,
                &mut cmd_ctx,
                &mut self.scanner,
                &checker_env,
                &input_buffer,
            ) {
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p kish 2>&1 | head -30`
Expected: errors only from test files (tests/interactive.rs)

- [ ] **Step 5: Commit**

```bash
git add src/interactive/mod.rs
git commit -m "feat(completion): wire CommandCompleter into Repl"
```

---

### Task 6: Update existing tests for new `read_line_with_completion` signature

**Files:**
- Modify: `tests/interactive.rs:1-10` (imports)
- Modify: `tests/interactive.rs` (all `read_line_with_completion` call sites — lines 987, 1016, 1044, 1072, 1106)

- [ ] **Step 1: Add imports**

In `tests/interactive.rs`, add the command completion import alongside the existing completion import (around line 5):

```rust
use kish::interactive::command_completion::{CommandCompleter, CommandCompletionContext};
```

- [ ] **Step 2: Update each test to create and pass `CommandCompletionContext`**

In each of the 5 test functions that call `read_line_with_completion`, add a `CommandCompleter` and `CommandCompletionContext` before the call, and pass `&mut cmd_ctx` as the new parameter.

For each test, add these lines before the `read_line_with_completion` call:

```rust
    let mut command_completer = CommandCompleter::new();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };
```

And update each `read_line_with_completion` call to include `&mut cmd_ctx` after `&ctx`:

Change (example from `test_tab_completes_single_candidate`):
```rust
    let result = editor
        .read_line_with_completion("$ ", &[], &mut history, &mut term, &ctx, &mut scanner, &checker_env, "")
        .unwrap();
```
To:
```rust
    let result = editor
        .read_line_with_completion("$ ", &[], &mut history, &mut term, &ctx, &mut cmd_ctx, &mut scanner, &checker_env, "")
        .unwrap();
```

Apply this same pattern to all 5 tests:
- `test_tab_completes_single_candidate` (line 987)
- `test_tab_completes_common_prefix` (line 1016)
- `test_tab_directory_appends_slash` (line 1044)
- `test_tab_no_match_does_nothing` (line 1072)
- `test_double_tab_opens_completion_ui` (line 1106)

- [ ] **Step 3: Run all tests to verify everything compiles and passes**

Run: `cargo test -p kish 2>&1 | tail -5`
Expected: all tests PASS (including pre-existing tests)

- [ ] **Step 4: Commit**

```bash
git add tests/interactive.rs
git commit -m "test: update existing tab completion tests for CommandCompletionContext"
```

---

### Task 7: Add integration tests for command name completion

**Files:**
- Modify: `tests/interactive.rs` (add new test functions after existing tab completion tests)

- [ ] **Step 1: Add test for command completion at line start**

Add the following test after `test_double_tab_opens_completion_ui` in `tests/interactive.rs`:

```rust
#[test]
fn test_tab_command_completion_at_line_start() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Create an executable in a temp PATH directory
    let bin_dir = tempfile::TempDir::new().unwrap();
    let cmd_path = bin_dir.path().join("kish_test_mycmd");
    fs::File::create(&cmd_path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&cmd_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/tmp".to_string(),
        show_dotfiles: false,
    };

    let mut command_completer = CommandCompleter::new();
    let aliases = AliasStore::default();
    let path_str = bin_dir.path().to_str().unwrap().to_string();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: &path_str,
        builtins: &[],
        aliases: &aliases,
    };

    // Type "kish_test_my" + Tab + Enter — should complete to "kish_test_mycmd "
    let mut events = chars("kish_test_my");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv { path: "", aliases: &aliases };
    let result = editor
        .read_line_with_completion("$ ", &[], &mut history, &mut term, &ctx, &mut cmd_ctx, &mut scanner, &checker_env, "")
        .unwrap();
    assert_eq!(result, Some("kish_test_mycmd ".to_string()));
}
```

- [ ] **Step 2: Add test for path fallback in command position**

```rust
#[test]
fn test_tab_command_position_path_fallback() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Create a file in cwd starting with "./"
    fs::File::create(tmp.path().join("myscript.sh")).unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/tmp".to_string(),
        show_dotfiles: false,
    };

    let mut command_completer = CommandCompleter::new();
    let aliases = AliasStore::default();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };

    // Type "./my" + Tab + Enter — should fall back to path completion
    let mut events = chars("./my");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv { path: "", aliases: &aliases };
    let result = editor
        .read_line_with_completion("$ ", &[], &mut history, &mut term, &ctx, &mut cmd_ctx, &mut scanner, &checker_env, "")
        .unwrap();
    assert_eq!(result, Some("./myscript.sh ".to_string()));
}
```

- [ ] **Step 3: Add test for argument position path completion (regression)**

```rust
#[test]
fn test_tab_argument_position_uses_path_completion() {
    let tmp = tempfile::TempDir::new().unwrap();
    fs::File::create(tmp.path().join("testfile.txt")).unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/tmp".to_string(),
        show_dotfiles: false,
    };

    let mut command_completer = CommandCompleter::new();
    let aliases = AliasStore::default();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };

    // Type "cat test" + Tab + Enter — argument position should use path completion
    let mut events = chars("cat test");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv { path: "", aliases: &aliases };
    let result = editor
        .read_line_with_completion("$ ", &[], &mut history, &mut term, &ctx, &mut cmd_ctx, &mut scanner, &checker_env, "")
        .unwrap();
    assert_eq!(result, Some("cat testfile.txt ".to_string()));
}
```

- [ ] **Step 4: Add test for builtin completion**

```rust
#[test]
fn test_tab_completes_builtin() {
    let tmp = tempfile::TempDir::new().unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/tmp".to_string(),
        show_dotfiles: false,
    };

    let mut command_completer = CommandCompleter::new();
    let aliases = AliasStore::default();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &["export", "exec", "exit"],
        aliases: &aliases,
    };

    // Type "expo" + Tab + Enter — should complete to "export "
    let mut events = chars("expo");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv { path: "", aliases: &aliases };
    let result = editor
        .read_line_with_completion("$ ", &[], &mut history, &mut term, &ctx, &mut cmd_ctx, &mut scanner, &checker_env, "")
        .unwrap();
    assert_eq!(result, Some("export ".to_string()));
}
```

- [ ] **Step 5: Run all tests**

Run: `cargo test -p kish 2>&1 | tail -10`
Expected: all tests PASS

- [ ] **Step 6: Commit**

```bash
git add tests/interactive.rs
git commit -m "test: add command name completion integration tests"
```

---

### Task 8: Add PTY tests

**Files:**
- Modify: `tests/pty_interactive.rs` (add new tests after existing `test_pty_tab_completion`)

- [ ] **Step 1: Add PTY test for command completion at line start**

Add after `test_pty_tab_completion` (around line 286) in `tests/pty_interactive.rs`:

```rust
#[test]
fn test_pty_command_completion() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    // "ech" + Tab should complete to "echo" (builtin)
    s.send("ech").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    s.send("\t").unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // Add " hello" and press Enter to execute "echo hello"
    s.send(" hello\r").unwrap();
    expect_output(&mut s, "hello", "Command completion for 'echo' failed");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}
```

- [ ] **Step 2: Add PTY test for command completion after pipe**

```rust
#[test]
fn test_pty_command_completion_after_pipe() {
    let (mut s, _tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    // "echo hello | ca" + Tab should complete to "cat" (from PATH)
    // Then press Enter to execute
    s.send("echo hello | ca").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    s.send("\t").unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // Press Enter — "echo hello | cat" should output "hello"
    s.send("\r").unwrap();
    expect_output(
        &mut s,
        "hello",
        "Command completion after pipe failed",
    );
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}
```

- [ ] **Step 3: Add PTY test for path completion in argument position (regression)**

```rust
#[test]
fn test_pty_path_completion_in_argument_position() {
    let (mut s, tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    // Create a uniquely named file
    let test_file = tmpdir.path().join("kish_argcomp_unique.txt");
    std::fs::write(&test_file, "content").unwrap();

    // cd to HOME
    s.send("cd\r").unwrap();
    wait_for_prompt(&mut s);

    // "cat kish_argcomp" + Tab should path-complete to "kish_argcomp_unique.txt"
    s.send("cat kish_argcomp").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    s.send("\t").unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // Press Enter — should print the file content
    s.send("\r").unwrap();
    expect_output(
        &mut s,
        "content",
        "Path completion in argument position failed",
    );
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}
```

- [ ] **Step 4: Build and run PTY tests**

Run: `cargo build -p kish && cargo test -p kish --test pty_interactive test_pty_command_completion test_pty_path_completion_in_argument -- --test-threads=1`
Expected: all 3 new PTY tests PASS

- [ ] **Step 5: Commit**

```bash
git add tests/pty_interactive.rs
git commit -m "test(pty): add command name completion PTY tests"
```

---

### Task 9: Update TODO.md

**Files:**
- Modify: `TODO.md:33`

- [ ] **Step 1: Remove the completed item**

Delete this line from `TODO.md`:

```
- [ ] Tab completion: command name completion — complete executable names from PATH in command position (`src/interactive/completion.rs`)
```

- [ ] **Step 2: Run full test suite**

Run: `cargo test -p kish 2>&1 | tail -5`
Expected: all tests PASS

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "docs(TODO): remove completed command name completion item"
```
