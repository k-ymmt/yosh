# Command History Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add command history to kish interactive mode — ↑/↓ navigation, `~/.kish_history` persistence, Ctrl+R fzf-style fuzzy search, and POSIX `fc` built-in.

**Architecture:** New modules `history.rs` (data + persistence) and `fuzzy_search.rs` (Ctrl+R UI + fuzzy match) under `src/interactive/`. `History` struct lives in `ShellEnv` for shared access by `LineEditor` (↑/↓), `FuzzySearchUI` (Ctrl+R), and `fc` (special built-in). LineEditor borrows `&mut History` during `read_line()`.

**Tech Stack:** Rust, crossterm 0.29 (terminal UI), nix 0.31 (file I/O)

**Spec:** `docs/superpowers/specs/2026-04-12-command-history-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `src/interactive/history.rs` | Create | History struct: in-memory list, add/navigate/load/save |
| `src/interactive/fuzzy_search.rs` | Create | FuzzySearchUI: Ctrl+R fzf-style UI + fuzzy match algorithm |
| `src/interactive/mod.rs` | Modify | Add `pub mod history; pub mod fuzzy_search;`, Repl lifecycle changes |
| `src/interactive/line_editor.rs` | Modify | Add ↑/↓/Ctrl+R key handling, `read_line()` takes `&mut History` |
| `src/env/mod.rs` | Modify | Add `history: History` field to `ShellEnv` |
| `src/builtin/special.rs` | Modify | Add `fc` built-in implementation |
| `src/builtin/mod.rs` | Modify | Register `fc` in classify_builtin and dispatch |
| `TODO.md` | Modify | Update completed items |

---

### Task 1: History Core — `add()` with HISTCONTROL

**Files:**
- Create: `src/interactive/history.rs`
- Modify: `src/interactive/mod.rs` (add module declaration)

- [ ] **Step 1: Create history module with struct and `new()`**

Create `src/interactive/history.rs`:

```rust
/// Command history storage with navigation and persistence.
///
/// Stores commands in chronological order (oldest first) and supports
/// cursor-based navigation for ↑/↓ arrow key traversal.
#[derive(Debug, Clone)]
pub struct History {
    entries: Vec<String>,
    cursor: Option<usize>,
    saved_line: String,
}

impl History {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            cursor: None,
            saved_line: String::new(),
        }
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}
```

Add to `src/interactive/mod.rs` after existing module declarations:

```rust
pub mod history;
```

- [ ] **Step 2: Write failing tests for `add()`**

Add to `src/interactive/history.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_basic() {
        let mut h = History::new();
        h.add("ls", 500, "");
        h.add("pwd", 500, "");
        assert_eq!(h.entries(), &["ls", "pwd"]);
    }

    #[test]
    fn test_add_ignoredups() {
        let mut h = History::new();
        h.add("ls", 500, "ignoredups");
        h.add("ls", 500, "ignoredups");
        h.add("pwd", 500, "ignoredups");
        assert_eq!(h.entries(), &["ls", "pwd"]);
    }

    #[test]
    fn test_add_ignorespace() {
        let mut h = History::new();
        h.add(" secret", 500, "ignorespace");
        h.add("ls", 500, "ignorespace");
        assert_eq!(h.entries(), &["ls"]);
    }

    #[test]
    fn test_add_ignoreboth() {
        let mut h = History::new();
        h.add("ls", 500, "ignoreboth");
        h.add("ls", 500, "ignoreboth");
        h.add(" secret", 500, "ignoreboth");
        h.add("pwd", 500, "ignoreboth");
        assert_eq!(h.entries(), &["ls", "pwd"]);
    }

    #[test]
    fn test_add_histsize_truncation() {
        let mut h = History::new();
        h.add("cmd1", 3, "");
        h.add("cmd2", 3, "");
        h.add("cmd3", 3, "");
        h.add("cmd4", 3, "");
        assert_eq!(h.entries(), &["cmd2", "cmd3", "cmd4"]);
    }

    #[test]
    fn test_add_empty_line_skipped() {
        let mut h = History::new();
        h.add("", 500, "");
        assert_eq!(h.len(), 0);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib interactive::history::tests -q`
Expected: Compilation error — `add` method not found

- [ ] **Step 4: Implement `add()`**

Add to `History` impl block in `src/interactive/history.rs`:

```rust
    pub fn add(&mut self, line: &str, histsize: usize, histcontrol: &str) {
        if line.is_empty() {
            return;
        }

        // ignorespace: skip lines starting with a space
        if (histcontrol == "ignorespace" || histcontrol == "ignoreboth")
            && line.starts_with(' ')
        {
            return;
        }

        // ignoredups: skip if same as last entry
        if (histcontrol == "ignoredups" || histcontrol == "ignoreboth")
            && self.entries.last().map(|s| s.as_str()) == Some(line)
        {
            return;
        }

        self.entries.push(line.to_string());

        // Truncate to histsize (remove oldest entries)
        if histsize > 0 && self.entries.len() > histsize {
            let excess = self.entries.len() - histsize;
            self.entries.drain(..excess);
        }
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib interactive::history::tests -q`
Expected: All 6 tests pass

- [ ] **Step 6: Commit**

```bash
git add src/interactive/history.rs src/interactive/mod.rs
git commit -m "feat(history): add History struct with add() and HISTCONTROL logic"
```

---

### Task 2: History Navigation — `navigate_up()`, `navigate_down()`, `reset_cursor()`

**Files:**
- Modify: `src/interactive/history.rs`

- [ ] **Step 1: Write failing tests for navigation**

Add to the `tests` module in `src/interactive/history.rs`:

```rust
    #[test]
    fn test_navigate_up_basic() {
        let mut h = History::new();
        h.add("first", 500, "");
        h.add("second", 500, "");
        h.add("third", 500, "");

        assert_eq!(h.navigate_up("current"), Some("third"));
        assert_eq!(h.navigate_up("current"), Some("second"));
        assert_eq!(h.navigate_up("current"), Some("first"));
        // At oldest — stays there
        assert_eq!(h.navigate_up("current"), Some("first"));
    }

    #[test]
    fn test_navigate_down_basic() {
        let mut h = History::new();
        h.add("first", 500, "");
        h.add("second", 500, "");

        h.navigate_up("typing");
        h.navigate_up("typing");

        assert_eq!(h.navigate_down(), Some("second"));
        // Past newest — returns saved_line
        assert_eq!(h.navigate_down(), Some("typing"));
        // Already at bottom — stays there
        assert_eq!(h.navigate_down(), Some("typing"));
    }

    #[test]
    fn test_navigate_saves_current_line() {
        let mut h = History::new();
        h.add("old_cmd", 500, "");

        // User was typing "partial" when they pressed ↑
        h.navigate_up("partial");
        assert_eq!(h.navigate_down(), Some("partial"));
    }

    #[test]
    fn test_navigate_empty_history() {
        let mut h = History::new();
        assert_eq!(h.navigate_up("text"), None);
    }

    #[test]
    fn test_reset_cursor() {
        let mut h = History::new();
        h.add("cmd1", 500, "");
        h.add("cmd2", 500, "");

        h.navigate_up("x");
        h.reset_cursor();
        // After reset, navigate_up starts from the end again
        assert_eq!(h.navigate_up("y"), Some("cmd2"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib interactive::history::tests -q`
Expected: Compilation error — `navigate_up`, `navigate_down`, `reset_cursor` not found

- [ ] **Step 3: Implement navigation methods**

Add to `History` impl block in `src/interactive/history.rs`:

```rust
    pub fn navigate_up(&mut self, current_line: &str) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }

        let new_cursor = match self.cursor {
            None => {
                // First ↑ press: save current input and go to last entry
                self.saved_line = current_line.to_string();
                self.entries.len() - 1
            }
            Some(0) => {
                // Already at oldest entry — stay
                0
            }
            Some(pos) => pos - 1,
        };

        self.cursor = Some(new_cursor);
        Some(&self.entries[new_cursor])
    }

    pub fn navigate_down(&mut self) -> Option<&str> {
        let pos = match self.cursor {
            None => return Some(&self.saved_line),
            Some(pos) => pos,
        };

        if pos + 1 >= self.entries.len() {
            // Past the newest entry — restore saved line
            self.cursor = None;
            Some(&self.saved_line)
        } else {
            self.cursor = Some(pos + 1);
            Some(&self.entries[pos + 1])
        }
    }

    pub fn reset_cursor(&mut self) {
        self.cursor = None;
        self.saved_line.clear();
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib interactive::history::tests -q`
Expected: All 11 tests pass

- [ ] **Step 5: Commit**

```bash
git add src/interactive/history.rs
git commit -m "feat(history): add navigate_up/down/reset_cursor for arrow key navigation"
```

---

### Task 3: History Persistence — `load()` and `save()`

**Files:**
- Modify: `src/interactive/history.rs`

- [ ] **Step 1: Write failing tests for persistence**

Add to the `tests` module in `src/interactive/history.rs`:

```rust
    use std::io::Write;

    #[test]
    fn test_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");

        let mut h = History::new();
        h.add("cmd1", 500, "");
        h.add("cmd2", 500, "");
        h.save(&path, 500);

        let mut h2 = History::new();
        h2.load(&path);
        assert_eq!(h2.entries(), &["cmd1", "cmd2"]);
    }

    #[test]
    fn test_load_nonexistent_file() {
        let mut h = History::new();
        h.load(std::path::Path::new("/nonexistent/path/history"));
        assert_eq!(h.len(), 0);
    }

    #[test]
    fn test_save_histfilesize_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");

        let mut h = History::new();
        for i in 0..10 {
            h.add(&format!("cmd{}", i), 500, "");
        }
        h.save(&path, 3);

        let mut h2 = History::new();
        h2.load(&path);
        assert_eq!(h2.entries(), &["cmd7", "cmd8", "cmd9"]);
    }

    #[test]
    fn test_load_trims_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");

        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "cmd1").unwrap();
        writeln!(f, "").unwrap();
        writeln!(f, "cmd2").unwrap();

        let mut h = History::new();
        h.load(&path);
        assert_eq!(h.entries(), &["cmd1", "cmd2"]);
    }
```

- [ ] **Step 2: Add `tempfile` dev-dependency**

Add to `Cargo.toml` under `[dev-dependencies]`:

```toml
tempfile = "3"
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib interactive::history::tests -q`
Expected: Compilation error — `load`, `save` not found

- [ ] **Step 4: Implement `load()` and `save()`**

Add imports at the top of `src/interactive/history.rs`:

```rust
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
```

Add to `History` impl block:

```rust
    pub fn load(&mut self, path: &Path) {
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return,
        };
        let reader = BufReader::new(file);
        for line in reader.lines() {
            if let Ok(line) = line {
                if !line.is_empty() {
                    self.entries.push(line);
                }
            }
        }
    }

    pub fn save(&self, path: &Path, histfilesize: usize) {
        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut file = match fs::File::create(path) {
            Ok(f) => f,
            Err(_) => return,
        };
        let start = if histfilesize > 0 && self.entries.len() > histfilesize {
            self.entries.len() - histfilesize
        } else {
            0
        };
        for entry in &self.entries[start..] {
            let _ = writeln!(file, "{}", entry);
        }
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib interactive::history::tests -q`
Expected: All 15 tests pass

- [ ] **Step 6: Commit**

```bash
git add src/interactive/history.rs Cargo.toml
git commit -m "feat(history): add load/save for ~/.kish_history persistence"
```

---

### Task 4: ShellEnv Integration — Add `history` Field

**Files:**
- Modify: `src/env/mod.rs`

- [ ] **Step 1: Add `history` field to `ShellEnv`**

In `src/env/mod.rs`, add the import at the top (after existing use statements):

```rust
use crate::interactive::history::History;
```

Add `history` field to `ShellEnv` struct:

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
    pub history: History,
}
```

Add `history: History::new()` to the `ShellEnv::new()` constructor:

```rust
        ShellEnv {
            vars,
            exec: ExecState {
                last_exit_status: 0,
                flow_control: None,
            },
            // ... existing fields ...
            aliases: AliasStore::default(),
            history: History::new(),
        }
```

- [ ] **Step 2: Verify full test suite passes**

Run: `cargo test -q`
Expected: All existing tests pass (history field added with default value)

- [ ] **Step 3: Commit**

```bash
git add src/env/mod.rs
git commit -m "feat(env): add history field to ShellEnv"
```

---

### Task 5: LineEditor ↑/↓ Integration

**Files:**
- Modify: `src/interactive/line_editor.rs`
- Modify: `src/interactive/mod.rs` (update `read_line` call)

- [ ] **Step 1: Change `read_line()` signature to accept `&mut History`**

In `src/interactive/line_editor.rs`, add the import at the top:

```rust
use super::history::History;
```

Change the `read_line()` signature:

```rust
    pub fn read_line(&mut self, prompt_width: usize, history: &mut History) -> io::Result<Option<String>> {
```

Change `handle_key()` signature to accept history:

```rust
    fn handle_key(&mut self, key: KeyEvent, history: &mut History) -> KeyAction {
```

Update the `read_line()` method's call to `handle_key()`:

```rust
                match self.handle_key(key_event, history) {
```

Add `history.reset_cursor()` in the `Submit` and `Interrupt` arms of `read_line()`:

In the `Submit` arm, before the `return`:
```rust
                    KeyAction::Submit => {
                        history.reset_cursor();
                        stdout.execute(cursor::MoveToColumn(0))?;
                        write!(stdout, "\r\n")?;
                        stdout.flush()?;
                        return Ok(Some(self.buffer()));
                    }
```

In the `Interrupt` arm, before the `return`:
```rust
                    KeyAction::Interrupt => {
                        history.reset_cursor();
                        stdout.execute(cursor::MoveToColumn(0))?;
                        write!(stdout, "\r\n")?;
                        stdout.flush()?;
                        self.clear();
                        return Ok(Some(String::new()));
                    }
```

- [ ] **Step 2: Add ↑/↓ key handlers in `handle_key()`**

Add these arms in the `handle_key()` match, before the final catch-all `_ => KeyAction::Continue`:

```rust
            // Up — navigate history backward
            (KeyCode::Up, _) => {
                if let Some(line) = history.navigate_up(&self.buffer()) {
                    self.buf = line.chars().collect();
                    self.pos = self.buf.len();
                }
                KeyAction::Continue
            }

            // Down — navigate history forward
            (KeyCode::Down, _) => {
                if let Some(line) = history.navigate_down() {
                    self.buf = line.chars().collect();
                    self.pos = self.buf.len();
                }
                KeyAction::Continue
            }
```

- [ ] **Step 3: Update Repl to pass history to `read_line()`**

In `src/interactive/mod.rs`, change the `read_line` call in `Repl::run()`:

```rust
            let line = match self.line_editor.read_line(prompt_width, &mut self.executor.env.history) {
```

- [ ] **Step 4: Verify compilation and existing tests pass**

Run: `cargo test -q`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add src/interactive/line_editor.rs src/interactive/mod.rs
git commit -m "feat(line_editor): add ↑/↓ arrow key history navigation"
```

---

### Task 6: Repl Lifecycle — Init, Add, Save

**Files:**
- Modify: `src/interactive/mod.rs`

- [ ] **Step 1: Add history initialization in `Repl::new()`**

In `src/interactive/mod.rs`, add at the end of `Repl::new()`, before the `Self { ... }` return, after the `take_terminal` call:

```rust
        // Set history variable defaults
        let home = executor.env.vars.get("HOME").unwrap_or("").to_string();
        let histfile = format!("{}/.kish_history", home);
        let _ = executor.env.vars.set("HISTFILE", &histfile);
        let _ = executor.env.vars.set("HISTSIZE", "500");
        let _ = executor.env.vars.set("HISTFILESIZE", "500");
        let _ = executor.env.vars.set("HISTCONTROL", "ignoreboth");

        // Load history from file
        executor.env.history.load(std::path::Path::new(&histfile));
```

- [ ] **Step 2: Add `history.add()` after successful command parse**

In `Repl::run()`, inside the `ParseStatus::Complete` arm, add `history.add()` before executing commands. The trimmed input_buffer (without trailing newline) is added:

```rust
                ParseStatus::Complete(commands) => {
                    // Add to history before executing
                    let histsize: usize = self.executor.env.vars.get("HISTSIZE")
                        .and_then(|s| s.parse().ok()).unwrap_or(500);
                    let histcontrol = self.executor.env.vars.get("HISTCONTROL")
                        .unwrap_or("ignoreboth").to_string();
                    let cmd_text = input_buffer.trim_end().to_string();
                    self.executor.env.history.add(&cmd_text, histsize, &histcontrol);

                    for cmd in &commands {
                        let status = self.executor.exec_complete_command(cmd);
                        self.executor.env.exec.last_exit_status = status;
                    }
                    input_buffer.clear();
                }
```

- [ ] **Step 3: Add history save before exit**

In `Repl::run()`, add history save before the final `self.executor.env.exec.last_exit_status` return at the end, after `execute_exit_trap()`:

```rust
        self.executor.process_pending_signals();
        self.executor.execute_exit_trap();

        // Save history to file
        let histfile = self.executor.env.vars.get("HISTFILE").unwrap_or("").to_string();
        let histfilesize: usize = self.executor.env.vars.get("HISTFILESIZE")
            .and_then(|s| s.parse().ok()).unwrap_or(500);
        if !histfile.is_empty() {
            self.executor.env.history.save(std::path::Path::new(&histfile), histfilesize);
        }

        self.executor.env.exec.last_exit_status
```

- [ ] **Step 4: Add history save on SIGHUP**

In `src/exec/mod.rs`, find the `handle_default_signal()` method (the method that handles signals like SIGHUP by terminating). Before the process exits on SIGHUP, save the history. The exact integration depends on the signal handling flow — if `handle_default_signal` calls `std::process::exit()`, add a history save before it. If the signal is processed in `Repl::run()` via `process_pending_signals()`, the save at the end of `run()` will cover it.

Check the current signal flow: if SIGHUP causes `Repl::run()` to exit normally via its `break` path, the existing save-on-exit code (Step 3) already covers this. If SIGHUP causes an immediate exit via `std::process::exit()`, add a save call there. Verify the behavior and add save if needed.

- [ ] **Step 5: Verify compilation and existing tests pass**

Run: `cargo test -q`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add src/interactive/mod.rs
git commit -m "feat(repl): integrate history lifecycle — init, add on command, save on exit"
```

---

### Task 7: Fuzzy Match Algorithm

**Files:**
- Create: `src/interactive/fuzzy_search.rs`
- Modify: `src/interactive/mod.rs` (add module declaration)

- [ ] **Step 1: Write failing tests for fuzzy matching**

Create `src/interactive/fuzzy_search.rs`:

```rust
/// Result of a fuzzy match: the score and matched character positions.
#[derive(Debug)]
pub struct FuzzyMatch {
    pub score: i64,
    pub positions: Vec<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let m = fuzzy_match("ls", "ls").unwrap();
        assert!(m.score > 0);
    }

    #[test]
    fn test_substring_match() {
        let m = fuzzy_match("check", "git checkout").unwrap();
        assert!(m.score > 0);
    }

    #[test]
    fn test_fuzzy_order_preserving() {
        let m = fuzzy_match("gco", "git checkout").unwrap();
        assert!(m.score > 0);
        // Positions must be in ascending order
        for w in m.positions.windows(2) {
            assert!(w[0] < w[1]);
        }
    }

    #[test]
    fn test_no_match() {
        assert!(fuzzy_match("xyz", "git checkout").is_none());
    }

    #[test]
    fn test_empty_query_matches_all() {
        let m = fuzzy_match("", "anything").unwrap();
        assert_eq!(m.score, 0);
    }

    #[test]
    fn test_consecutive_bonus() {
        // "che" in "checkout" (consecutive) should score higher than "c_h_e" spread out
        let consecutive = fuzzy_match("che", "checkout").unwrap();
        let spread = fuzzy_match("che", "c-h-e-ckout").unwrap();
        assert!(consecutive.score > spread.score);
    }

    #[test]
    fn test_word_boundary_bonus() {
        // "gc" matching at word boundaries "git checkout" should score higher
        // than "gc" matching inside a word "agcdef"
        let boundary = fuzzy_match("gc", "git checkout").unwrap();
        let inside = fuzzy_match("gc", "xgcdef").unwrap();
        assert!(boundary.score > inside.score);
    }

    #[test]
    fn test_case_sensitive_bonus() {
        let exact = fuzzy_match("Make", "Makefile").unwrap();
        let wrong_case = fuzzy_match("Make", "makefile").unwrap();
        assert!(exact.score > wrong_case.score);
    }

    #[test]
    fn test_filter_and_sort() {
        let entries = vec![
            "git checkout main".to_string(),
            "git commit -m 'fix'".to_string(),
            "ls -la".to_string(),
            "grep pattern file".to_string(),
        ];
        let results = filter_and_sort("gco", &entries);
        assert!(!results.is_empty());
        assert!(results[0].1.contains("checkout"));
        // Scores should be in descending order
        for w in results.windows(2) {
            assert!(w[0].0 >= w[1].0);
        }
    }
}
```

Add to `src/interactive/mod.rs`:

```rust
pub mod fuzzy_search;
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib interactive::fuzzy_search::tests -q`
Expected: Compilation error — `fuzzy_match`, `filter_and_sort` not found

- [ ] **Step 3: Implement `fuzzy_match()`**

Add to `src/interactive/fuzzy_search.rs`:

```rust
const SCORE_MATCH: i64 = 16;
const SCORE_CONSECUTIVE: i64 = 24;
const SCORE_WORD_BOUNDARY: i64 = 32;
const SCORE_EXACT_CASE: i64 = 4;

/// Perform a fuzzy match of `query` against `target`.
///
/// Returns `None` if the query characters don't appear in order in the target.
/// Returns `Some(FuzzyMatch)` with a score and the matched positions.
pub fn fuzzy_match(query: &str, target: &str) -> Option<FuzzyMatch> {
    if query.is_empty() {
        return Some(FuzzyMatch { score: 0, positions: vec![] });
    }

    let query_chars: Vec<char> = query.chars().collect();
    let target_chars: Vec<char> = target.chars().collect();

    // First pass: check if all query chars exist in order (case-insensitive)
    let mut qi = 0;
    for &tc in &target_chars {
        if qi < query_chars.len()
            && tc.to_ascii_lowercase() == query_chars[qi].to_ascii_lowercase()
        {
            qi += 1;
        }
    }
    if qi < query_chars.len() {
        return None;
    }

    // Second pass: find the best matching positions using greedy scoring
    let mut positions = Vec::with_capacity(query_chars.len());
    let mut score: i64 = 0;
    let mut qi = 0;
    let mut prev_match_idx: Option<usize> = None;

    for (ti, &tc) in target_chars.iter().enumerate() {
        if qi >= query_chars.len() {
            break;
        }
        if tc.to_ascii_lowercase() == query_chars[qi].to_ascii_lowercase() {
            positions.push(ti);
            score += SCORE_MATCH;

            // Exact case bonus
            if tc == query_chars[qi] {
                score += SCORE_EXACT_CASE;
            }

            // Consecutive bonus
            if let Some(prev) = prev_match_idx {
                if ti == prev + 1 {
                    score += SCORE_CONSECUTIVE;
                }
            }

            // Word boundary bonus
            if ti == 0 || matches!(target_chars[ti - 1], ' ' | '/' | '-' | '_' | '.') {
                score += SCORE_WORD_BOUNDARY;
            }

            prev_match_idx = Some(ti);
            qi += 1;
        }
    }

    Some(FuzzyMatch { score, positions })
}

/// Filter entries by fuzzy match and return sorted by score descending.
///
/// Returns a Vec of (score, entry_string) pairs.
pub fn filter_and_sort(query: &str, entries: &[String]) -> Vec<(i64, String)> {
    let mut results: Vec<(i64, String)> = entries
        .iter()
        .filter_map(|entry| {
            fuzzy_match(query, entry).map(|m| (m.score, entry.clone()))
        })
        .collect();
    results.sort_by(|a, b| b.0.cmp(&a.0));
    results
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib interactive::fuzzy_search::tests -q`
Expected: All 9 tests pass

- [ ] **Step 5: Commit**

```bash
git add src/interactive/fuzzy_search.rs src/interactive/mod.rs
git commit -m "feat(fuzzy): add fuzzy match algorithm with scoring bonuses"
```

---

### Task 8: Fuzzy Search UI — Ctrl+R

**Files:**
- Modify: `src/interactive/fuzzy_search.rs`
- Modify: `src/interactive/line_editor.rs`

- [ ] **Step 1: Implement `FuzzySearchUI` struct and `run()`**

Add to `src/interactive/fuzzy_search.rs`, after the existing code:

```rust
use std::io::{self, Write, Stdout, stdout};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    style::{Attribute, SetAttribute},
    terminal::{self, ClearType},
    ExecutableCommand,
};

use super::history::History;

pub struct FuzzySearchUI {
    query: Vec<char>,
    selected: usize,
    scroll_offset: usize,
    candidates: Vec<(i64, String)>,
    max_visible: usize,
}

impl FuzzySearchUI {
    pub fn run(history: &History) -> io::Result<Option<String>> {
        let entries = history.entries();
        if entries.is_empty() {
            return Ok(None);
        }

        let (_, term_height) = terminal::size()?;
        let max_visible = ((term_height as f32) * 0.4).max(3.0) as usize;

        let mut ui = FuzzySearchUI {
            query: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            candidates: entries.iter().cloned().map(|e| (0, e)).collect(),
            max_visible,
        };

        // Reverse so newest is first in candidates
        ui.candidates.reverse();

        let mut stdout = stdout();

        // Reserve space: print empty lines for the UI area
        let draw_lines = ui.max_visible + 2; // candidates + separator + query line
        for _ in 0..draw_lines {
            write!(stdout, "\r\n")?;
        }
        // Move cursor back up to the top of our drawing area
        stdout.execute(cursor::MoveUp(draw_lines as u16))?;

        ui.draw(&mut stdout)?;

        loop {
            stdout.flush()?;
            if let Event::Key(key_event) = event::read()? {
                match ui.handle_key(key_event, entries) {
                    SearchAction::Continue => {}
                    SearchAction::Select(line) => {
                        ui.clear_ui(&mut stdout, draw_lines)?;
                        return Ok(Some(line));
                    }
                    SearchAction::Cancel => {
                        ui.clear_ui(&mut stdout, draw_lines)?;
                        return Ok(None);
                    }
                }
                ui.draw(&mut stdout)?;
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent, entries: &[String]) -> SearchAction {
        match (key.code, key.modifiers) {
            // Enter — select current candidate
            (KeyCode::Enter, _) => {
                if let Some((_score, line)) = self.candidates.get(self.selected) {
                    SearchAction::Select(line.clone())
                } else {
                    SearchAction::Cancel
                }
            }

            // Esc / Ctrl+G — cancel
            (KeyCode::Esc, _) => SearchAction::Cancel,
            (KeyCode::Char('g'), m) if m.contains(KeyModifiers::CONTROL) => SearchAction::Cancel,

            // Up — move selection up
            (KeyCode::Up, _) => {
                if self.selected + 1 < self.candidates.len() {
                    self.selected += 1;
                    self.adjust_scroll();
                }
                SearchAction::Continue
            }

            // Ctrl+P — move selection up
            (KeyCode::Char('p'), m) if m.contains(KeyModifiers::CONTROL) => {
                if self.selected + 1 < self.candidates.len() {
                    self.selected += 1;
                    self.adjust_scroll();
                }
                SearchAction::Continue
            }

            // Ctrl+R — move selection up (same as ↑)
            (KeyCode::Char('r'), m) if m.contains(KeyModifiers::CONTROL) => {
                if self.selected + 1 < self.candidates.len() {
                    self.selected += 1;
                    self.adjust_scroll();
                }
                SearchAction::Continue
            }

            // Down / Ctrl+N — move selection down
            (KeyCode::Down, _) => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.adjust_scroll();
                }
                SearchAction::Continue
            }
            (KeyCode::Char('n'), m) if m.contains(KeyModifiers::CONTROL) => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.adjust_scroll();
                }
                SearchAction::Continue
            }

            // Backspace — delete last query char
            (KeyCode::Backspace, _) => {
                if !self.query.is_empty() {
                    self.query.pop();
                    self.update_candidates(entries);
                }
                SearchAction::Continue
            }

            // Printable character — append to query
            (KeyCode::Char(ch), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.query.push(ch);
                self.update_candidates(entries);
                SearchAction::Continue
            }

            _ => SearchAction::Continue,
        }
    }

    fn update_candidates(&mut self, entries: &[String]) {
        let query: String = self.query.iter().collect();
        if query.is_empty() {
            self.candidates = entries.iter().cloned().map(|e| (0, e)).collect();
            self.candidates.reverse();
        } else {
            self.candidates = filter_and_sort(&query, entries);
        }
        self.selected = 0;
        self.scroll_offset = 0;
    }

    fn adjust_scroll(&mut self) {
        if self.selected >= self.scroll_offset + self.max_visible {
            self.scroll_offset = self.selected - self.max_visible + 1;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
    }

    fn draw(&self, stdout: &mut Stdout) -> io::Result<()> {
        let (term_width, _) = terminal::size()?;
        let width = term_width as usize;

        // Move to start of our drawing area
        stdout.execute(cursor::MoveToColumn(0))?;

        // Draw candidates (bottom-to-top: highest index at top)
        let visible_end = (self.scroll_offset + self.max_visible).min(self.candidates.len());
        let visible_range = self.scroll_offset..visible_end;

        // We draw from top of area to bottom. Top = higher indices, bottom = lower indices.
        // Fill empty lines first if we have fewer candidates than max_visible
        let visible_count = visible_range.len();
        for _ in 0..(self.max_visible - visible_count) {
            stdout.execute(terminal::Clear(ClearType::CurrentLine))?;
            write!(stdout, "\r\n")?;
        }

        // Draw candidates in reverse order (highest index = top)
        for i in (visible_range).rev() {
            stdout.execute(terminal::Clear(ClearType::CurrentLine))?;
            let (_score, ref line) = self.candidates[i];
            let display: String = line.chars().take(width.saturating_sub(2)).collect();
            if i == self.selected {
                stdout.execute(SetAttribute(Attribute::Reverse))?;
                write!(stdout, "> {}", display)?;
                stdout.execute(SetAttribute(Attribute::Reset))?;
            } else {
                write!(stdout, "  {}", display)?;
            }
            write!(stdout, "\r\n")?;
        }

        // Draw separator
        stdout.execute(terminal::Clear(ClearType::CurrentLine))?;
        let sep: String = "\u{2500}".repeat(width.min(40));
        write!(stdout, "  {}\r\n", sep)?;

        // Draw query line
        stdout.execute(terminal::Clear(ClearType::CurrentLine))?;
        let query_str: String = self.query.iter().collect();
        write!(stdout, "  {}/{} > {}", self.candidates.len(),
            self.candidates.len() + (if self.query.is_empty() { 0 } else { 0 }),
            query_str)?;

        // Move cursor back to top of drawing area for next redraw
        let total_lines = self.max_visible + 2;
        stdout.execute(cursor::MoveUp(total_lines as u16))?;
        stdout.flush()?;
        Ok(())
    }

    fn clear_ui(&self, stdout: &mut Stdout, draw_lines: usize) -> io::Result<()> {
        stdout.execute(cursor::MoveToColumn(0))?;
        for _ in 0..draw_lines {
            stdout.execute(terminal::Clear(ClearType::CurrentLine))?;
            write!(stdout, "\r\n")?;
        }
        stdout.execute(cursor::MoveUp(draw_lines as u16))?;
        stdout.flush()?;
        Ok(())
    }
}

enum SearchAction {
    Continue,
    Select(String),
    Cancel,
}
```

- [ ] **Step 2: Add Ctrl+R key handler in LineEditor**

In `src/interactive/line_editor.rs`, add import:

```rust
use super::fuzzy_search::FuzzySearchUI;
```

Add this arm in `handle_key()`, before the catch-all:

```rust
            // Ctrl+R — fuzzy history search
            (KeyCode::Char('r'), m) if m.contains(KeyModifiers::CONTROL) => {
                KeyAction::FuzzySearch
            }
```

Add `FuzzySearch` variant to `KeyAction`:

```rust
enum KeyAction {
    Continue,
    Submit,
    Eof,
    Interrupt,
    FuzzySearch,
}
```

Handle `FuzzySearch` in the `read_line()` loop. Add this arm after `Interrupt` handling:

```rust
                    KeyAction::FuzzySearch => {
                        // Drop raw mode temporarily (FuzzySearchUI manages its own)
                        drop(_guard);
                        match FuzzySearchUI::run(history) {
                            Ok(Some(line)) => {
                                self.buf = line.chars().collect();
                                self.pos = self.buf.len();
                            }
                            _ => {}
                        }
                        _guard = RawModeGuard::new()?;
                        self.redraw(&mut stdout, prompt_width)?;
                    }
```

Note: `_guard` must be changed to a mutable binding. Change:

```rust
        let _guard = RawModeGuard::new()?;
```

to:

```rust
        let mut _guard = RawModeGuard::new()?;
```

- [ ] **Step 3: Verify compilation passes**

Run: `cargo test -q`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add src/interactive/fuzzy_search.rs src/interactive/line_editor.rs
git commit -m "feat(fuzzy): add Ctrl+R fzf-style fuzzy search UI"
```

---

### Task 9: `fc` Built-in — List Mode

**Files:**
- Modify: `src/builtin/special.rs`
- Modify: `src/builtin/mod.rs`

- [ ] **Step 1: Write failing tests for `fc -l`**

Add tests at the bottom of `src/builtin/mod.rs` inside the existing `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn test_classify_fc() {
        assert!(matches!(classify_builtin("fc"), BuiltinKind::Special));
    }
```

- [ ] **Step 2: Register `fc` as a special builtin**

In `src/builtin/mod.rs`, add `"fc"` to the special builtins in `classify_builtin()`:

```rust
        "break" | ":" | "continue" | "." | "eval" | "exec" | "exit" | "export"
        | "readonly" | "return" | "set" | "shift" | "times" | "trap" | "unset"
        | "fc" => {
            BuiltinKind::Special
        }
```

In `src/builtin/special.rs`, add `"fc"` to the dispatch in `exec_special_builtin()`:

```rust
        "fc" => builtin_fc(args, executor),
```

- [ ] **Step 3: Implement `fc` built-in**

Add `use std::io::Write;` to the imports at the top of `src/builtin/special.rs`.

Add to `src/builtin/special.rs`:

```rust
fn builtin_fc(args: &[String], executor: &mut Executor) -> i32 {
    let entries = executor.env.history.entries();
    if entries.is_empty() {
        eprintln!("kish: fc: history is empty");
        return 1;
    }

    let mut list_mode = false;
    let mut suppress_numbers = false;
    let mut reverse = false;
    let mut substitute_mode = false;
    let mut editor: Option<String> = None;
    let mut operands: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-e" {
            i += 1;
            if i >= args.len() {
                eprintln!("kish: fc: -e: option requires an argument");
                return 1;
            }
            editor = Some(args[i].clone());
        } else if arg.starts_with('-') && arg.len() > 1 && arg.chars().nth(1).map_or(false, |c| c.is_ascii_alphabetic()) {
            for ch in arg[1..].chars() {
                match ch {
                    'l' => list_mode = true,
                    'n' => suppress_numbers = true,
                    'r' => reverse = true,
                    's' => substitute_mode = true,
                    _ => {
                        eprintln!("kish: fc: -{}: invalid option", ch);
                        return 2;
                    }
                }
            }
        } else {
            operands.push(arg.clone());
        }
        i += 1;
    }

    if substitute_mode {
        return fc_substitute(&operands, executor);
    }

    let hist_len = entries.len();
    let (start, end) = fc_resolve_range(&operands, hist_len, list_mode, entries);

    if list_mode {
        fc_list(entries, start, end, suppress_numbers, reverse);
        0
    } else {
        fc_edit(entries, start, end, reverse, editor, executor)
    }
}

fn fc_resolve_one(spec: &str, default: usize, entries: &[String]) -> usize {
    if let Ok(n) = spec.parse::<i64>() {
        if n > 0 {
            ((n - 1) as usize).min(entries.len().saturating_sub(1))
        } else {
            entries.len().saturating_sub((-n) as usize)
        }
    } else {
        // String prefix match — most recent entry
        (0..entries.len()).rev()
            .find(|&i| entries[i].starts_with(spec))
            .unwrap_or(default)
    }
}

fn fc_resolve_range(operands: &[String], hist_len: usize, is_list: bool, entries: &[String]) -> (usize, usize) {
    match operands.len() {
        0 => {
            if is_list {
                (hist_len.saturating_sub(16), hist_len.saturating_sub(1))
            } else {
                let last = hist_len.saturating_sub(1);
                (last, last)
            }
        }
        1 => {
            let idx = fc_resolve_one(&operands[0], hist_len.saturating_sub(1), entries);
            if is_list {
                (idx, hist_len.saturating_sub(1))
            } else {
                (idx, idx)
            }
        }
        _ => {
            let s = fc_resolve_one(&operands[0], hist_len.saturating_sub(1), entries);
            let e = fc_resolve_one(&operands[1], hist_len.saturating_sub(1), entries);
            (s, e)
        }
    }
}

fn fc_list(entries: &[String], start: usize, end: usize, suppress_numbers: bool, reverse: bool) {
    let (lo, hi) = if start <= end { (start, end) } else { (end, start) };
    let range: Vec<usize> = if reverse ^ (start > end) {
        (lo..=hi).rev().collect()
    } else {
        (lo..=hi).collect()
    };
    for i in range {
        if suppress_numbers {
            println!("\t{}", entries[i]);
        } else {
            // History numbers are 1-based
            println!("{}\t{}", i + 1, entries[i]);
        }
    }
}

fn fc_edit(
    entries: &[String],
    start: usize,
    end: usize,
    reverse: bool,
    editor: Option<String>,
    executor: &mut Executor,
) -> i32 {
    let editor_cmd = editor
        .or_else(|| executor.env.vars.get("FCEDIT").map(|s| s.to_string()))
        .or_else(|| executor.env.vars.get("EDITOR").map(|s| s.to_string()))
        .unwrap_or_else(|| "/bin/ed".to_string());

    let (lo, hi) = if start <= end { (start, end) } else { (end, start) };
    let mut commands: Vec<&str> = (lo..=hi).map(|i| entries[i].as_str()).collect();
    if reverse {
        commands.reverse();
    }

    // Write commands to temp file
    let tmp_path = format!("/tmp/kish_fc_{}", std::process::id());
    {
        let mut file = match std::fs::File::create(&tmp_path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("kish: fc: cannot create temp file: {}", e);
                return 1;
            }
        };
        for cmd in &commands {
            let _ = writeln!(file, "{}", cmd);
        }
    }

    // Run editor
    use std::process::Command;
    let status = Command::new(&editor_cmd).arg(&tmp_path).status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            let _ = std::fs::remove_file(&tmp_path);
            return s.code().unwrap_or(1);
        }
        Err(e) => {
            eprintln!("kish: fc: {}: {}", editor_cmd, e);
            let _ = std::fs::remove_file(&tmp_path);
            return 127;
        }
    }

    // Read back and execute
    let content = match std::fs::read_to_string(&tmp_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("kish: fc: cannot read temp file: {}", e);
            let _ = std::fs::remove_file(&tmp_path);
            return 1;
        }
    };
    let _ = std::fs::remove_file(&tmp_path);

    if content.trim().is_empty() {
        return 0;
    }

    // Execute the edited content
    executor.eval_string(&content);
    executor.env.exec.last_exit_status
}

fn fc_substitute(operands: &[String], executor: &mut Executor) -> i32 {
    let entries = executor.env.history.entries();
    if entries.is_empty() {
        eprintln!("kish: fc: history is empty");
        return 1;
    }

    let mut replacement: Option<(&str, &str)> = None;
    let mut target_spec: Option<&str> = None;

    for op in operands {
        if let Some(eq_pos) = op.find('=') {
            replacement = Some((&op[..eq_pos], &op[eq_pos + 1..]));
        } else {
            target_spec = Some(op.as_str());
        }
    }

    // Find the target entry
    let idx = if let Some(spec) = target_spec {
        fc_resolve_one(spec, entries.len().saturating_sub(1), entries)
    } else {
        entries.len().saturating_sub(1)
    };

    let mut cmd = entries[idx].clone();
    if let Some((old, new)) = replacement {
        cmd = cmd.replacen(old, new, 1);
    }

    // Print the command being executed
    eprintln!("{}", cmd);

    // Add the substituted command to history
    let histsize: usize = executor.env.vars.get("HISTSIZE")
        .and_then(|s| s.parse().ok()).unwrap_or(500);
    let histcontrol = executor.env.vars.get("HISTCONTROL")
        .unwrap_or("ignoreboth").to_string();
    executor.env.history.add(&cmd, histsize, &histcontrol);

    executor.eval_string(&cmd);
    executor.env.exec.last_exit_status
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -q`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add src/builtin/special.rs src/builtin/mod.rs
git commit -m "feat(fc): add POSIX fc built-in — list, edit, substitute modes"
```

---

### Task 10: E2E Tests for History and `fc`

**Files:**
- Create: `tests/history.rs`

- [ ] **Step 1: Write E2E tests**

Note: `fc` operates on the history list. In non-interactive `-c` mode, the history starts empty. The Repl adds commands to history in interactive mode. These tests verify `fc` behavior with empty history and basic non-interactive execution.

Create `tests/history.rs`:

```rust
use std::process::Command;

fn kish_exec(input: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kish"))
        .args(["-c", input])
        .output()
        .expect("failed to execute kish")
}

#[test]
fn test_fc_empty_history_error() {
    // In non-interactive mode, history is empty — fc should report an error
    let out = kish_exec("fc -l");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("history is empty"));
}

#[test]
fn test_fc_is_special_builtin() {
    // fc should be recognized as a command (no "not found" error)
    let out = kish_exec("fc -l 2>/dev/null; echo $?");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Should print exit status 1 (empty history), not 127 (not found)
    assert!(stdout.trim().ends_with('1'));
}
```

- [ ] **Step 2: Run E2E tests**

Run: `cargo test --test history -q`
Expected: All tests pass

- [ ] **Step 3: Commit**

```bash
git add tests/history.rs
git commit -m "test(e2e): add history and fc E2E tests"
```

---

### Task 11: TODO.md Update and Cleanup

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Update TODO.md**

Remove the completed items from the "Future: Interactive Mode Enhancements" section:

Remove: `- [ ] History — ↑/↓ for history navigation, ~/.kish_history persistence, Ctrl+R reverse search`

Since Ctrl+R is now fzf-style fuzzy search (not traditional reverse-i-search), and `fc` is implemented, these are done.

Keep all other items unchanged.

- [ ] **Step 2: Run full test suite**

Run: `cargo test -q`
Expected: All tests pass

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "docs: update TODO.md — mark history feature as complete"
```

---

## Summary

| Task | Description | New/Modified Files |
|------|-------------|-------------------|
| 1 | History core + `add()` | `history.rs` (create), `mod.rs` |
| 2 | Navigation (↑/↓) | `history.rs` |
| 3 | Persistence (load/save) | `history.rs`, `Cargo.toml` |
| 4 | ShellEnv integration | `env/mod.rs` |
| 5 | LineEditor ↑/↓ | `line_editor.rs`, `interactive/mod.rs` |
| 6 | Repl lifecycle | `interactive/mod.rs` |
| 7 | Fuzzy match algorithm | `fuzzy_search.rs` (create), `mod.rs` |
| 8 | Ctrl+R UI | `fuzzy_search.rs`, `line_editor.rs` |
| 9 | `fc` built-in | `special.rs`, `builtin/mod.rs` |
| 10 | E2E tests | `tests/history.rs` (create) |
| 11 | TODO.md cleanup | `TODO.md` |
