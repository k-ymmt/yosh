# Path Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Tab-based file path completion to kish's interactive mode with single-Tab common prefix completion and double-Tab interactive fuzzy-filter UI.

**Architecture:** New `src/interactive/completion.rs` module handles candidate generation, prefix computation, and the interactive UI. `LineEditor` gains a `tab_count` field and `TabComplete` key action. `Repl` passes a `CompletionContext` (CWD, dotfile setting) to `read_line`.

**Tech Stack:** Rust, crossterm (already in use), std::fs for directory scanning, existing fuzzy_search module for fuzzy matching

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `src/interactive/completion.rs` | Create | Completion word extraction, path decomposition, candidate generation, longest common prefix, `CompletionUI` |
| `src/interactive/line_editor.rs` | Modify | Add `tab_count` field, `TabComplete` action, Tab key handling in `handle_key`, completion logic in `read_line_loop` |
| `src/interactive/mod.rs` | Modify | Add `pub mod completion;`, pass `CompletionContext` from `Repl` to `read_line` |
| `tests/interactive.rs` | Modify | Add Tab completion integration tests using MockTerminal |
| `tests/pty_interactive.rs` | Modify | Add PTY E2E test for Tab completion |

---

### Task 1: Core Completion Logic — Word Extraction and Path Splitting

**Files:**
- Create: `src/interactive/completion.rs`
- Modify: `src/interactive/mod.rs` (add `pub mod completion;`)

- [ ] **Step 1: Write failing tests for `extract_completion_word`**

Add to the bottom of the new `src/interactive/completion.rs` file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_word() {
        assert_eq!(extract_completion_word("ls foo", 6), (3, "foo"));
    }

    #[test]
    fn test_extract_at_start() {
        assert_eq!(extract_completion_word("foo", 3), (0, "foo"));
    }

    #[test]
    fn test_extract_after_pipe() {
        assert_eq!(extract_completion_word("cat foo | grep b", 16), (15, "b"));
    }

    #[test]
    fn test_extract_after_semicolon() {
        assert_eq!(extract_completion_word("echo a; ls sr", 13), (11, "sr"));
    }

    #[test]
    fn test_extract_empty_at_space() {
        assert_eq!(extract_completion_word("ls ", 3), (3, ""));
    }

    #[test]
    fn test_extract_path_with_slash() {
        assert_eq!(extract_completion_word("ls src/int", 10), (3, "src/int"));
    }

    #[test]
    fn test_extract_with_double_quote() {
        assert_eq!(extract_completion_word("ls \"My Doc", 10), (3, "\"My Doc"));
    }

    #[test]
    fn test_extract_with_single_quote() {
        assert_eq!(extract_completion_word("ls 'My Doc", 10), (3, "'My Doc"));
    }
}
```

- [ ] **Step 2: Write failing tests for `split_path`**

Add to the test module:

```rust
    #[test]
    fn test_split_relative_path() {
        assert_eq!(split_path("src/int", "/home/user"), ("src/", "int"));
    }

    #[test]
    fn test_split_no_directory() {
        assert_eq!(split_path("foo", "/home/user"), ("", "foo"));
    }

    #[test]
    fn test_split_absolute_path() {
        assert_eq!(split_path("/usr/lo", "/home/user"), ("/usr/", "lo"));
    }

    #[test]
    fn test_split_tilde_path() {
        assert_eq!(split_path("~/Doc", "/home/user"), ("/home/user/", "Doc"));
    }

    #[test]
    fn test_split_trailing_slash() {
        assert_eq!(split_path("src/", "/home/user"), ("src/", ""));
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib interactive::completion -- --nocapture 2>&1 | head -50`
Expected: compilation errors (functions not defined)

- [ ] **Step 4: Implement `extract_completion_word` and `split_path`**

Write the top of `src/interactive/completion.rs`:

```rust
/// Extract the completion word from the buffer at the given cursor position.
/// Returns (start_index, word) where start_index is the byte offset in the buffer
/// where the completion word begins.
pub fn extract_completion_word(buf: &str, cursor: usize) -> (usize, &str) {
    let bytes = buf.as_bytes();
    let mut i = cursor;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    // Scan leftward from cursor to find word start
    while i > 0 {
        let ch = bytes[i - 1] as char;

        if in_single_quote {
            if ch == '\'' {
                in_single_quote = false;
            }
            i -= 1;
            continue;
        }
        if in_double_quote {
            if ch == '"' {
                in_double_quote = false;
            }
            i -= 1;
            continue;
        }

        match ch {
            '\'' => {
                in_single_quote = true;
                i -= 1;
            }
            '"' => {
                in_double_quote = true;
                i -= 1;
            }
            ' ' | '|' | ';' | '&' | '<' | '>' | '(' | ')' => break,
            _ => {
                i -= 1;
            }
        }
    }

    (i, &buf[i..cursor])
}

/// Split a completion word into (directory, prefix) at the last '/'.
/// Expands `~` to the given home directory.
pub fn split_path<'a>(word: &'a str, home: &str) -> (String, &'a str) {
    // Strip leading quote if present
    let stripped = word.strip_prefix('"')
        .or_else(|| word.strip_prefix('\''))
        .unwrap_or(word);

    if let Some(pos) = stripped.rfind('/') {
        let dir_part = &stripped[..=pos];
        let prefix = &stripped[pos + 1..];
        // Expand ~ to home
        let resolved_dir = if dir_part.starts_with("~/") {
            format!("{}/{}", home.trim_end_matches('/'), &dir_part[1..])
        } else {
            dir_part.to_string()
        };
        (resolved_dir, prefix)
    } else {
        (String::new(), stripped)
    }
}
```

- [ ] **Step 5: Register the module in `mod.rs`**

Add to `src/interactive/mod.rs` after the other `pub mod` lines:

```rust
pub mod completion;
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --lib interactive::completion -- --nocapture`
Expected: all tests pass

- [ ] **Step 7: Commit**

```bash
git add src/interactive/completion.rs src/interactive/mod.rs
git commit -m "feat(completion): add word extraction and path splitting"
```

---

### Task 2: Candidate Generation and Common Prefix

**Files:**
- Modify: `src/interactive/completion.rs`

- [ ] **Step 1: Write failing tests for `longest_common_prefix`**

Add to the test module in `completion.rs`:

```rust
    #[test]
    fn test_lcp_multiple() {
        let candidates = vec!["file_a.rs".to_string(), "file_b.rs".to_string()];
        assert_eq!(longest_common_prefix(&candidates), "file_");
    }

    #[test]
    fn test_lcp_single() {
        let candidates = vec!["unique.txt".to_string()];
        assert_eq!(longest_common_prefix(&candidates), "unique.txt");
    }

    #[test]
    fn test_lcp_empty() {
        let candidates: Vec<String> = vec![];
        assert_eq!(longest_common_prefix(&candidates), "");
    }

    #[test]
    fn test_lcp_no_common() {
        let candidates = vec!["abc".to_string(), "xyz".to_string()];
        assert_eq!(longest_common_prefix(&candidates), "");
    }

    #[test]
    fn test_lcp_all_same() {
        let candidates = vec!["same".to_string(), "same".to_string()];
        assert_eq!(longest_common_prefix(&candidates), "same");
    }
```

- [ ] **Step 2: Write failing tests for `generate_candidates`**

Add to the test module (uses `tempfile` or manual temp dir):

```rust
    use std::fs;

    fn create_temp_dir() -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("kish-completion-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn test_generate_basic() {
        let dir = create_temp_dir();
        fs::write(dir.join("file_a.rs"), "").unwrap();
        fs::write(dir.join("file_b.txt"), "").unwrap();
        fs::create_dir(dir.join("subdir")).unwrap();

        let candidates = generate_candidates(dir.to_str().unwrap(), "", false);
        assert_eq!(candidates, vec!["file_a.rs", "file_b.txt", "subdir/"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_generate_with_prefix() {
        let dir = create_temp_dir();
        fs::write(dir.join("file_a.rs"), "").unwrap();
        fs::write(dir.join("file_b.txt"), "").unwrap();
        fs::write(dir.join("other.rs"), "").unwrap();

        let candidates = generate_candidates(dir.to_str().unwrap(), "file", false);
        assert_eq!(candidates, vec!["file_a.rs", "file_b.txt"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_generate_hides_dotfiles_by_default() {
        let dir = create_temp_dir();
        fs::write(dir.join("visible.txt"), "").unwrap();
        fs::write(dir.join(".hidden"), "").unwrap();

        let candidates = generate_candidates(dir.to_str().unwrap(), "", false);
        assert_eq!(candidates, vec!["visible.txt"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_generate_shows_dotfiles_with_dot_prefix() {
        let dir = create_temp_dir();
        fs::write(dir.join(".bashrc"), "").unwrap();
        fs::write(dir.join(".profile"), "").unwrap();
        fs::write(dir.join("visible.txt"), "").unwrap();

        let candidates = generate_candidates(dir.to_str().unwrap(), ".", false);
        assert_eq!(candidates, vec![".bashrc", ".profile"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_generate_shows_dotfiles_with_env() {
        let dir = create_temp_dir();
        fs::write(dir.join(".hidden"), "").unwrap();
        fs::write(dir.join("visible.txt"), "").unwrap();

        let candidates = generate_candidates(dir.to_str().unwrap(), "", true);
        assert_eq!(candidates, vec![".hidden", "visible.txt"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_generate_nonexistent_dir() {
        let candidates = generate_candidates("/nonexistent/path/xyz", "", false);
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_generate_directory_gets_slash() {
        let dir = create_temp_dir();
        fs::create_dir(dir.join("mydir")).unwrap();
        fs::write(dir.join("myfile"), "").unwrap();

        let candidates = generate_candidates(dir.to_str().unwrap(), "my", false);
        assert_eq!(candidates, vec!["mydir/", "myfile"]);
        let _ = fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib interactive::completion -- --nocapture 2>&1 | head -30`
Expected: compilation errors (functions not defined)

- [ ] **Step 4: Implement `longest_common_prefix` and `generate_candidates`**

Add to `src/interactive/completion.rs` (after the `split_path` function):

```rust
use std::fs;
use std::path::Path;

/// Compute the longest common prefix of a list of strings.
pub fn longest_common_prefix(candidates: &[String]) -> String {
    if candidates.is_empty() {
        return String::new();
    }
    let first = &candidates[0];
    let mut len = first.len();
    for candidate in &candidates[1..] {
        len = len.min(candidate.len());
        for (i, (a, b)) in first.bytes().zip(candidate.bytes()).enumerate() {
            if a != b {
                len = len.min(i);
                break;
            }
        }
    }
    first[..len].to_string()
}

/// Generate completion candidates by scanning a directory.
///
/// - `dir`: directory to scan (empty string means CWD ".")
/// - `prefix`: filter entries that start with this prefix (case-sensitive)
/// - `show_dotfiles`: if true, include hidden files regardless of prefix
///
/// Returns a sorted list of candidate names. Directories have a trailing `/`.
pub fn generate_candidates(dir: &str, prefix: &str, show_dotfiles: bool) -> Vec<String> {
    let scan_dir = if dir.is_empty() { "." } else { dir };
    let entries = match fs::read_dir(scan_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let include_dotfiles = show_dotfiles || prefix.starts_with('.');

    let mut candidates: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();

            // Filter hidden files
            if name.starts_with('.') && !include_dotfiles {
                return None;
            }

            // Filter by prefix
            if !name.starts_with(prefix) {
                return None;
            }

            // Append '/' for directories
            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            if is_dir {
                Some(format!("{}/", name))
            } else {
                Some(name)
            }
        })
        .collect();

    candidates.sort();
    candidates
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib interactive::completion -- --nocapture`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add src/interactive/completion.rs
git commit -m "feat(completion): add candidate generation and common prefix"
```

---

### Task 3: CompletionContext and High-Level Complete Function

**Files:**
- Modify: `src/interactive/completion.rs`

- [ ] **Step 1: Write failing test for `complete`**

Add to the test module:

```rust
    #[test]
    fn test_complete_single_candidate() {
        let dir = create_temp_dir();
        fs::write(dir.join("unique_file.txt"), "").unwrap();

        let ctx = CompletionContext {
            cwd: dir.to_str().unwrap().to_string(),
            home: "/home/user".to_string(),
            show_dotfiles: false,
        };
        let result = complete("uni", 3, &ctx);
        assert_eq!(result.candidates, vec!["unique_file.txt"]);
        assert_eq!(result.common_prefix, "unique_file.txt");
        assert_eq!(result.word_start, 0);
        assert_eq!(result.dir_prefix, "");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_complete_multiple_candidates() {
        let dir = create_temp_dir();
        fs::write(dir.join("file_a.rs"), "").unwrap();
        fs::write(dir.join("file_b.rs"), "").unwrap();

        let ctx = CompletionContext {
            cwd: dir.to_str().unwrap().to_string(),
            home: "/home/user".to_string(),
            show_dotfiles: false,
        };
        let result = complete("file", 4, &ctx);
        assert_eq!(result.candidates, vec!["file_a.rs", "file_b.rs"]);
        assert_eq!(result.common_prefix, "file_");
        assert_eq!(result.word_start, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_complete_with_directory_prefix() {
        let dir = create_temp_dir();
        let sub = dir.join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("hello.txt"), "").unwrap();

        let input = format!("{}/hel", dir.to_str().unwrap());
        let ctx = CompletionContext {
            cwd: "/tmp".to_string(),
            home: "/home/user".to_string(),
            show_dotfiles: false,
        };
        let result = complete(&input, input.len(), &ctx);
        assert_eq!(result.candidates, vec!["hello.txt"]);
        assert_eq!(result.common_prefix, "hello.txt");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_complete_no_matches() {
        let dir = create_temp_dir();
        fs::write(dir.join("abc.txt"), "").unwrap();

        let ctx = CompletionContext {
            cwd: dir.to_str().unwrap().to_string(),
            home: "/home/user".to_string(),
            show_dotfiles: false,
        };
        let result = complete("xyz", 3, &ctx);
        assert!(result.candidates.is_empty());
        assert_eq!(result.common_prefix, "");

        let _ = fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib interactive::completion -- --nocapture 2>&1 | head -30`
Expected: compilation errors

- [ ] **Step 3: Implement `CompletionContext`, `CompletionResult`, and `complete`**

Add to `src/interactive/completion.rs` (before the test module):

```rust
/// Context needed for path completion.
pub struct CompletionContext {
    pub cwd: String,
    pub home: String,
    pub show_dotfiles: bool,
}

/// Result of a completion operation.
pub struct CompletionResult {
    /// The candidate file/directory names (without the directory prefix).
    pub candidates: Vec<String>,
    /// The longest common prefix of all candidates.
    pub common_prefix: String,
    /// The byte offset in the original buffer where the completion word starts.
    pub word_start: usize,
    /// The directory prefix part of the completion word (e.g., "src/").
    /// Used to reconstruct the full path when inserting a candidate.
    pub dir_prefix: String,
}

/// Perform path completion on the given buffer at the cursor position.
pub fn complete(buf: &str, cursor: usize, ctx: &CompletionContext) -> CompletionResult {
    let (word_start, word) = extract_completion_word(buf, cursor);
    let (dir_part, prefix) = split_path(word, &ctx.home);

    // Resolve the directory to scan
    let scan_dir = if dir_part.is_empty() {
        ctx.cwd.clone()
    } else if dir_part.starts_with('/') {
        dir_part.clone()
    } else {
        format!("{}/{}", ctx.cwd.trim_end_matches('/'), dir_part.trim_end_matches('/'))
    };

    let candidates = generate_candidates(&scan_dir, prefix, ctx.show_dotfiles);
    let common_prefix = longest_common_prefix(&candidates);

    CompletionResult {
        candidates,
        common_prefix,
        word_start,
        dir_prefix: dir_part,
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib interactive::completion -- --nocapture`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add src/interactive/completion.rs
git commit -m "feat(completion): add CompletionContext and complete() function"
```

---

### Task 4: CompletionUI — Interactive Candidate Selection

**Files:**
- Modify: `src/interactive/completion.rs`

- [ ] **Step 1: Write failing tests for `CompletionUI`**

Add to the test module:

```rust
    use super::super::terminal::Terminal;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use std::collections::VecDeque;

    /// Minimal mock terminal for CompletionUI tests.
    struct MockTerm {
        events: VecDeque<Event>,
        cursor_row: i32,
    }

    impl MockTerm {
        fn new(events: Vec<Event>) -> Self {
            Self {
                events: VecDeque::from(events),
                cursor_row: 0,
            }
        }

        fn mk_key(code: KeyCode) -> Event {
            Event::Key(KeyEvent::new(code, KeyModifiers::empty()))
        }
    }

    impl Terminal for MockTerm {
        fn read_event(&mut self) -> std::io::Result<Event> {
            self.events.pop_front().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "no events")
            })
        }
        fn size(&self) -> std::io::Result<(u16, u16)> { Ok((80, 24)) }
        fn enable_raw_mode(&mut self) -> std::io::Result<()> { Ok(()) }
        fn disable_raw_mode(&mut self) -> std::io::Result<()> { Ok(()) }
        fn move_to_column(&mut self, _col: u16) -> std::io::Result<()> { Ok(()) }
        fn move_up(&mut self, n: u16) -> std::io::Result<()> {
            self.cursor_row -= n as i32;
            Ok(())
        }
        fn clear_current_line(&mut self) -> std::io::Result<()> { Ok(()) }
        fn clear_until_newline(&mut self) -> std::io::Result<()> { Ok(()) }
        fn write_str(&mut self, s: &str) -> std::io::Result<()> {
            self.cursor_row += s.chars().filter(|&c| c == '\n').count() as i32;
            Ok(())
        }
        fn set_reverse(&mut self, _on: bool) -> std::io::Result<()> { Ok(()) }
        fn set_dim(&mut self, _on: bool) -> std::io::Result<()> { Ok(()) }
        fn hide_cursor(&mut self) -> std::io::Result<()> { Ok(()) }
        fn show_cursor(&mut self) -> std::io::Result<()> { Ok(()) }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }

    #[test]
    fn test_completion_ui_select_first() {
        let candidates = vec![
            "file_a.rs".to_string(),
            "file_b.rs".to_string(),
            "file_c.rs".to_string(),
        ];
        // Enter selects the first candidate (index 0)
        let events = vec![MockTerm::mk_key(KeyCode::Enter)];
        let mut term = MockTerm::new(events);
        let result = CompletionUI::run(&candidates, &mut term).unwrap();
        assert_eq!(result, Some("file_a.rs".to_string()));
    }

    #[test]
    fn test_completion_ui_navigate_and_select() {
        let candidates = vec![
            "file_a.rs".to_string(),
            "file_b.rs".to_string(),
            "file_c.rs".to_string(),
        ];
        // Up to select file_b.rs (index 1), Enter
        let events = vec![
            MockTerm::mk_key(KeyCode::Up),
            MockTerm::mk_key(KeyCode::Enter),
        ];
        let mut term = MockTerm::new(events);
        let result = CompletionUI::run(&candidates, &mut term).unwrap();
        assert_eq!(result, Some("file_b.rs".to_string()));
    }

    #[test]
    fn test_completion_ui_cancel() {
        let candidates = vec!["file_a.rs".to_string()];
        let events = vec![MockTerm::mk_key(KeyCode::Esc)];
        let mut term = MockTerm::new(events);
        let result = CompletionUI::run(&candidates, &mut term).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_completion_ui_tab_confirms() {
        let candidates = vec![
            "file_a.rs".to_string(),
            "file_b.rs".to_string(),
        ];
        // Tab confirms selection (same as Enter)
        let events = vec![MockTerm::mk_key(KeyCode::Tab)];
        let mut term = MockTerm::new(events);
        let result = CompletionUI::run(&candidates, &mut term).unwrap();
        assert_eq!(result, Some("file_a.rs".to_string()));
    }

    #[test]
    fn test_completion_ui_fuzzy_filter() {
        let candidates = vec![
            "apple.txt".to_string(),
            "banana.txt".to_string(),
            "avocado.txt".to_string(),
        ];
        // Type "ban" to filter, Enter to select
        let events = vec![
            MockTerm::mk_key(KeyCode::Char('b')),
            MockTerm::mk_key(KeyCode::Char('a')),
            MockTerm::mk_key(KeyCode::Char('n')),
            MockTerm::mk_key(KeyCode::Enter),
        ];
        let mut term = MockTerm::new(events);
        let result = CompletionUI::run(&candidates, &mut term).unwrap();
        assert_eq!(result, Some("banana.txt".to_string()));
    }

    #[test]
    fn test_completion_ui_no_cursor_drift() {
        let candidates = vec![
            "file_a.rs".to_string(),
            "file_b.rs".to_string(),
            "file_c.rs".to_string(),
        ];
        let events = vec![
            MockTerm::mk_key(KeyCode::Up),
            MockTerm::mk_key(KeyCode::Up),
            MockTerm::mk_key(KeyCode::Down),
            MockTerm::mk_key(KeyCode::Esc),
        ];
        let mut term = MockTerm::new(events);
        let _ = CompletionUI::run(&candidates, &mut term).unwrap();
        assert_eq!(term.cursor_row, 0, "cursor drifted after CompletionUI");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib interactive::completion -- --nocapture 2>&1 | head -30`
Expected: compilation errors (`CompletionUI` not defined)

- [ ] **Step 3: Implement `CompletionUI`**

Add to `src/interactive/completion.rs` (after the `complete` function, before the test module):

```rust
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use super::fuzzy_search::filter_and_sort;
use super::terminal::Terminal;

pub struct CompletionUI {
    query: Vec<char>,
    selected: usize,
    scroll_offset: usize,
    candidates: Vec<(i64, String)>,
    all_candidates: Vec<String>,
    max_visible: usize,
}

enum CompletionAction {
    Continue,
    Select(String),
    Cancel,
}

impl CompletionUI {
    /// Run the interactive completion UI. Returns `Some(candidate)` on selection,
    /// `None` on cancel.
    pub fn run<T: Terminal>(candidates: &[String], term: &mut T) -> io::Result<Option<String>> {
        if candidates.is_empty() {
            return Ok(None);
        }

        let (_, term_height) = term.size()?;
        let max_visible = ((term_height as f32) * 0.4).max(3.0) as usize;

        let mut ui = CompletionUI {
            query: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            candidates: candidates.iter().cloned().map(|c| (0, c)).collect(),
            all_candidates: candidates.to_vec(),
            max_visible,
        };

        let draw_lines = ui.max_visible + 2; // candidates + separator + query
        term.hide_cursor()?;
        for _ in 0..draw_lines {
            term.write_str("\r\n")?;
        }
        term.move_up(draw_lines as u16)?;
        ui.draw(term)?;

        term.enable_raw_mode()?;
        let result = ui.run_loop(term, draw_lines);
        let _ = term.disable_raw_mode();
        let _ = term.show_cursor();
        result
    }

    fn run_loop<T: Terminal>(
        &mut self,
        term: &mut T,
        draw_lines: usize,
    ) -> io::Result<Option<String>> {
        loop {
            term.flush()?;
            if let Event::Key(key_event) = term.read_event()? {
                match self.handle_key(key_event) {
                    CompletionAction::Continue => {}
                    CompletionAction::Select(name) => {
                        self.clear_ui(term, draw_lines)?;
                        return Ok(Some(name));
                    }
                    CompletionAction::Cancel => {
                        self.clear_ui(term, draw_lines)?;
                        return Ok(None);
                    }
                }
                self.draw(term)?;
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> CompletionAction {
        match (key.code, key.modifiers) {
            (KeyCode::Enter, _) | (KeyCode::Tab, _) => {
                if let Some((_score, name)) = self.candidates.get(self.selected) {
                    CompletionAction::Select(name.clone())
                } else {
                    CompletionAction::Cancel
                }
            }
            (KeyCode::Esc, _) => CompletionAction::Cancel,
            (KeyCode::Char('g'), m) if m.contains(KeyModifiers::CONTROL) => {
                CompletionAction::Cancel
            }
            (KeyCode::Up, _)
            | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                if self.selected + 1 < self.candidates.len() {
                    self.selected += 1;
                    self.adjust_scroll();
                }
                CompletionAction::Continue
            }
            (KeyCode::Down, _)
            | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.adjust_scroll();
                }
                CompletionAction::Continue
            }
            (KeyCode::Backspace, _) => {
                if !self.query.is_empty() {
                    self.query.pop();
                    self.update_candidates();
                }
                CompletionAction::Continue
            }
            (KeyCode::Char(ch), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.query.push(ch);
                self.update_candidates();
                CompletionAction::Continue
            }
            _ => CompletionAction::Continue,
        }
    }

    fn update_candidates(&mut self) {
        let query: String = self.query.iter().collect();
        if query.is_empty() {
            self.candidates = self.all_candidates.iter().cloned().map(|c| (0, c)).collect();
        } else {
            self.candidates = filter_and_sort(&query, &self.all_candidates);
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

    fn draw<T: Terminal>(&self, term: &mut T) -> io::Result<()> {
        let (term_width, _) = term.size()?;
        let width = term_width as usize;

        term.move_to_column(0)?;

        let visible_end = (self.scroll_offset + self.max_visible).min(self.candidates.len());
        let visible_range = self.scroll_offset..visible_end;
        let visible_count = visible_range.len();

        // Fill empty lines
        for _ in 0..(self.max_visible - visible_count) {
            term.clear_current_line()?;
            term.write_str("\r\n")?;
        }

        // Draw candidates (highest index = top)
        for i in (visible_range).rev() {
            term.clear_current_line()?;
            let (_score, ref name) = self.candidates[i];
            let display: String = name.chars().take(width.saturating_sub(2)).collect();
            if i == self.selected {
                term.set_reverse(true)?;
                term.write_str(&format!("> {}", display))?;
                term.set_reverse(false)?;
            } else {
                term.write_str(&format!("  {}", display))?;
            }
            term.write_str("\r\n")?;
        }

        // Separator
        term.clear_current_line()?;
        let sep: String = "\u{2500}".repeat(width.min(40));
        term.write_str(&format!("  {}\r\n", sep))?;

        // Query line
        term.clear_current_line()?;
        let query_str: String = self.query.iter().collect();
        let filtered = self.candidates.len();
        let total = self.all_candidates.len();
        term.write_str(&format!("  {}/{} > {}", filtered, total, query_str))?;

        let total_lines = self.max_visible + 1;
        term.move_up(total_lines as u16)?;
        term.flush()?;
        Ok(())
    }

    fn clear_ui<T: Terminal>(&self, term: &mut T, draw_lines: usize) -> io::Result<()> {
        term.move_to_column(0)?;
        for _ in 0..draw_lines {
            term.clear_current_line()?;
            term.write_str("\r\n")?;
        }
        term.move_up(draw_lines as u16)?;
        term.flush()?;
        Ok(())
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib interactive::completion -- --nocapture`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add src/interactive/completion.rs
git commit -m "feat(completion): add CompletionUI for interactive candidate selection"
```

---

### Task 5: Integrate Tab Completion into LineEditor

**Files:**
- Modify: `src/interactive/line_editor.rs`
- Modify: `src/interactive/mod.rs`

- [ ] **Step 1: Write failing integration test for single-Tab completion**

Add to `tests/interactive.rs`:

```rust
use std::fs;

#[test]
fn test_tab_completes_single_candidate() {
    // Create a temp directory with one matching file
    let dir = std::env::temp_dir().join(format!("kish-tab-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("unique_file.txt"), "").unwrap();

    let ctx = kish::interactive::completion::CompletionContext {
        cwd: dir.to_str().unwrap().to_string(),
        home: "/tmp".to_string(),
        show_dotfiles: false,
    };

    // Type "uni" then Tab, then Enter
    let events = vec![
        key(KeyCode::Char('u')),
        key(KeyCode::Char('n')),
        key(KeyCode::Char('i')),
        key(KeyCode::Tab),
        key(KeyCode::Enter),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line_with_completion("$ ", &mut history, &mut term, &ctx).unwrap();
    assert_eq!(result, Some("unique_file.txt ".to_string()));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_tab_completes_common_prefix() {
    let dir = std::env::temp_dir().join(format!("kish-tab-test2-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("file_alpha.rs"), "").unwrap();
    fs::write(dir.join("file_beta.rs"), "").unwrap();

    let ctx = kish::interactive::completion::CompletionContext {
        cwd: dir.to_str().unwrap().to_string(),
        home: "/tmp".to_string(),
        show_dotfiles: false,
    };

    // Type "file" then Tab (completes to "file_"), then Enter
    let events = vec![
        key(KeyCode::Char('f')),
        key(KeyCode::Char('i')),
        key(KeyCode::Char('l')),
        key(KeyCode::Char('e')),
        key(KeyCode::Tab),
        key(KeyCode::Enter),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line_with_completion("$ ", &mut history, &mut term, &ctx).unwrap();
    assert_eq!(result, Some("file_".to_string()));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_tab_directory_appends_slash() {
    let dir = std::env::temp_dir().join(format!("kish-tab-test3-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir(dir.join("mydir")).unwrap();

    let ctx = kish::interactive::completion::CompletionContext {
        cwd: dir.to_str().unwrap().to_string(),
        home: "/tmp".to_string(),
        show_dotfiles: false,
    };

    // Type "my" then Tab (completes to "mydir/"), then Enter
    let mut events = chars("my");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line_with_completion("$ ", &mut history, &mut term, &ctx).unwrap();
    assert_eq!(result, Some("mydir/".to_string()));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_tab_no_match_does_nothing() {
    let dir = std::env::temp_dir().join(format!("kish-tab-test4-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("abc.txt"), "").unwrap();

    let ctx = kish::interactive::completion::CompletionContext {
        cwd: dir.to_str().unwrap().to_string(),
        home: "/tmp".to_string(),
        show_dotfiles: false,
    };

    // Type "xyz" then Tab (no match), then Enter
    let mut events = chars("xyz");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line_with_completion("$ ", &mut history, &mut term, &ctx).unwrap();
    assert_eq!(result, Some("xyz".to_string()));

    let _ = fs::remove_dir_all(&dir);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test interactive test_tab -- --nocapture 2>&1 | head -30`
Expected: compilation errors (`read_line_with_completion` not defined)

- [ ] **Step 3: Add `tab_count` field and `TabComplete` action to `LineEditor`**

In `src/interactive/line_editor.rs`, modify the struct and `KeyAction`:

Add `tab_count` to the struct fields:
```rust
#[derive(Debug, Default)]
pub struct LineEditor {
    buf: Vec<char>,
    pos: usize,
    suggestion: Option<String>,
    tab_count: u8,
}
```

Add `TabComplete` to `KeyAction`:
```rust
enum KeyAction {
    Continue,
    Submit,
    Eof,
    Interrupt,
    FuzzySearch,
    TabComplete,
}
```

Reset `tab_count = 0` in `clear()`:
```rust
pub fn clear(&mut self) {
    self.buf.clear();
    self.pos = 0;
    self.suggestion = None;
    self.tab_count = 0;
}
```

- [ ] **Step 4: Modify `handle_key` to handle Tab and reset `tab_count`**

In `handle_key`, add the Tab case before the printable character match, and add `self.tab_count = 0;` at the start of every other branch. The cleanest approach is to reset at the top and only skip the reset for Tab:

```rust
fn handle_key(&mut self, key: KeyEvent, history: &mut History) -> KeyAction {
    // Reset tab_count on any key except Tab
    if key.code != KeyCode::Tab {
        self.tab_count = 0;
    }

    match (key.code, key.modifiers) {
        // ... all existing branches unchanged ...

        // Tab — path completion
        (KeyCode::Tab, _) => {
            self.tab_count += 1;
            KeyAction::TabComplete
        }

        // ... rest of existing branches ...
    }
}
```

Insert the Tab branch before the printable character match `(KeyCode::Char(ch), m) if !m.contains(KeyModifiers::CONTROL)`.

- [ ] **Step 5: Add `read_line_with_completion` method**

Add to `src/interactive/line_editor.rs`:

```rust
use super::completion::{self, CompletionContext, CompletionUI};
```

And the new method:

```rust
    /// Read a line with Tab completion support.
    pub fn read_line_with_completion<T: Terminal>(
        &mut self,
        prompt: &str,
        history: &mut History,
        term: &mut T,
        ctx: &CompletionContext,
    ) -> io::Result<Option<String>> {
        self.clear();
        term.enable_raw_mode()?;
        let result = self.read_line_loop_with_completion(prompt, history, term, ctx);
        let _ = term.disable_raw_mode();
        result
    }

    fn read_line_loop_with_completion<T: Terminal>(
        &mut self,
        prompt: &str,
        history: &mut History,
        term: &mut T,
        ctx: &CompletionContext,
    ) -> io::Result<Option<String>> {
        let prompt_width = prompt.chars().count();
        loop {
            term.flush()?;
            if let Event::Key(key_event) = term.read_event()? {
                match self.handle_key(key_event, history) {
                    KeyAction::Submit => {
                        history.reset_cursor();
                        term.move_to_column(0)?;
                        term.write_str("\r\n")?;
                        term.flush()?;
                        return Ok(Some(self.buffer()));
                    }
                    KeyAction::Eof => {
                        return Ok(None);
                    }
                    KeyAction::Interrupt => {
                        history.reset_cursor();
                        term.move_to_column(0)?;
                        term.write_str("\r\n")?;
                        term.flush()?;
                        self.clear();
                        return Ok(Some(String::new()));
                    }
                    KeyAction::FuzzySearch => {
                        self.suggestion = None;
                        term.disable_raw_mode()?;
                        if let Ok(Some(line)) = FuzzySearchUI::run(history, term) {
                            self.buf = line.chars().collect();
                            self.pos = self.buf.len();
                        }
                        term.enable_raw_mode()?;
                        term.move_to_column(0)?;
                        term.clear_current_line()?;
                        term.write_str(prompt)?;
                    }
                    KeyAction::TabComplete => {
                        self.handle_tab_complete(term, prompt, ctx)?;
                    }
                    KeyAction::Continue => {}
                }
                self.update_suggestion(history);
                self.redraw(term, prompt_width)?;
            }
        }
    }

    fn handle_tab_complete<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        ctx: &CompletionContext,
    ) -> io::Result<()> {
        let buf_str = self.buffer();
        let result = completion::complete(&buf_str, self.pos, ctx);

        if result.candidates.is_empty() {
            return Ok(());
        }

        if self.tab_count == 1 {
            // First Tab: insert common prefix
            if result.candidates.len() == 1 {
                // Single candidate: replace word completely
                let candidate = &result.candidates[0];
                let replacement = format!("{}{}", result.dir_prefix, candidate);
                // Add trailing space for files (not directories)
                let replacement = if !candidate.ends_with('/') {
                    format!("{} ", replacement)
                } else {
                    replacement
                };
                self.replace_word(result.word_start, &replacement);
            } else {
                // Multiple candidates: insert common prefix
                let new_word = format!("{}{}", result.dir_prefix, result.common_prefix);
                let current_word = &buf_str[result.word_start..self.pos];
                if new_word != current_word {
                    self.replace_word(result.word_start, &new_word);
                }
            }
        } else {
            // Second+ Tab: open interactive UI
            if result.candidates.len() >= 2 {
                self.suggestion = None;
                term.disable_raw_mode()?;
                if let Ok(Some(selected)) = CompletionUI::run(&result.candidates, term) {
                    let replacement = format!("{}{}", result.dir_prefix, selected);
                    let replacement = if !selected.ends_with('/') {
                        format!("{} ", replacement)
                    } else {
                        replacement
                    };
                    self.replace_word(result.word_start, &replacement);
                }
                term.enable_raw_mode()?;
                term.move_to_column(0)?;
                term.clear_current_line()?;
                term.write_str(prompt)?;
            }
        }
        Ok(())
    }

    /// Replace the completion word in the buffer starting at `word_start`
    /// up to the current cursor position with `replacement`.
    fn replace_word(&mut self, word_start: usize, replacement: &str) {
        // word_start is a byte offset in the buffer string.
        // Convert to char index.
        let buf_str: String = self.buf.iter().collect();
        let char_start = buf_str[..word_start].chars().count();
        let char_end = self.pos;

        // Remove old word
        self.buf.drain(char_start..char_end);
        // Insert replacement
        let replacement_chars: Vec<char> = replacement.chars().collect();
        let new_len = replacement_chars.len();
        for (i, ch) in replacement_chars.into_iter().enumerate() {
            self.buf.insert(char_start + i, ch);
        }
        self.pos = char_start + new_len;
    }
```

- [ ] **Step 6: Update `Repl` to use `read_line_with_completion`**

In `src/interactive/mod.rs`, add the import and modify the `run` method:

Add import at the top:
```rust
use completion::CompletionContext;
```

Replace the `read_line` call in the `run()` method:

```rust
            // Build completion context
            let cwd = std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| self.executor.env.vars.get("PWD").unwrap_or(".").to_string());
            let home = self.executor.env.vars.get("HOME").unwrap_or("").to_string();
            let show_dotfiles = self.executor.env.vars.get("KISH_SHOW_DOTFILES")
                .map(|v| v == "1")
                .unwrap_or(false);
            let comp_ctx = CompletionContext {
                cwd,
                home,
                show_dotfiles,
            };

            // Read a line
            let line = match self.line_editor.read_line_with_completion(
                &prompt,
                &mut self.executor.env.history,
                &mut self.terminal,
                &comp_ctx,
            ) {
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --test interactive test_tab -- --nocapture`
Expected: all 4 tab tests pass

Run: `cargo test --test interactive -- --nocapture`
Expected: all existing tests still pass

- [ ] **Step 8: Commit**

```bash
git add src/interactive/line_editor.rs src/interactive/mod.rs tests/interactive.rs
git commit -m "feat(completion): integrate Tab completion into LineEditor and Repl"
```

---

### Task 6: Double-Tab Integration Test

**Files:**
- Modify: `tests/interactive.rs`

- [ ] **Step 1: Write test for double-Tab interactive UI**

Add to `tests/interactive.rs`:

```rust
#[test]
fn test_double_tab_opens_completion_ui() {
    let dir = std::env::temp_dir().join(format!("kish-tab-test5-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("file_alpha.rs"), "").unwrap();
    fs::write(dir.join("file_beta.rs"), "").unwrap();

    let ctx = kish::interactive::completion::CompletionContext {
        cwd: dir.to_str().unwrap().to_string(),
        home: "/tmp".to_string(),
        show_dotfiles: false,
    };

    // Type "file_", Tab (common prefix already complete, no change),
    // Tab again (opens UI), Up (select file_beta.rs), Enter (confirm), Enter (submit)
    let mut events = chars("file_");
    events.push(key(KeyCode::Tab));     // first tab: completes common prefix (already "file_")
    events.push(key(KeyCode::Tab));     // second tab: opens CompletionUI
    events.push(key(KeyCode::Up));      // select file_beta.rs
    events.push(key(KeyCode::Enter));   // confirm selection in UI
    events.push(key(KeyCode::Enter));   // submit line

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line_with_completion("$ ", &mut history, &mut term, &ctx).unwrap();
    assert_eq!(result, Some("file_beta.rs ".to_string()));

    let _ = fs::remove_dir_all(&dir);
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test --test interactive test_double_tab -- --nocapture`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add tests/interactive.rs
git commit -m "test(completion): add double-Tab interactive UI integration test"
```

---

### Task 7: PTY E2E Test

**Files:**
- Modify: `tests/pty_interactive.rs`

- [ ] **Step 1: Write PTY test for Tab completion**

Add to `tests/pty_interactive.rs`:

```rust
#[test]
fn test_pty_tab_completion() {
    let (mut s, tmpdir) = spawn_kish();
    wait_for_prompt(&mut s);

    // Create a uniquely named file in the temp HOME directory
    let test_file = tmpdir.path().join("kish_tab_test_unique.txt");
    std::fs::write(&test_file, "hello").unwrap();

    // cd to the temp dir
    s.send(&format!("cd {}\r", tmpdir.path().to_str().unwrap())).unwrap();
    wait_for_prompt(&mut s);

    // Type "cat kish_tab" then Tab to complete
    s.send("cat kish_tab").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    s.send("\t").unwrap(); // Tab
    std::thread::sleep(Duration::from_millis(100));

    // Press Enter to execute the completed command
    s.send("\r").unwrap();
    expect_output(&mut s, "hello", "Tab completion failed to complete and execute");
    wait_for_prompt(&mut s);

    exit_shell(&mut s);
}
```

- [ ] **Step 2: Build and run the PTY test**

Run: `cargo build && cargo test --test pty_interactive test_pty_tab_completion -- --nocapture`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add tests/pty_interactive.rs
git commit -m "test(pty): add E2E test for Tab path completion"
```

---

### Task 8: Update TODO.md

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Remove the completed Tab completion item from TODO.md**

Per project convention, delete the "Tab completion" line from `TODO.md` (do not mark with `[x]`).

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: all tests pass (existing + new)

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "docs(TODO): remove completed Tab completion item"
```
