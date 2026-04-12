use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

/// Command history storage with navigation and persistence.
///
/// Stores commands in chronological order (oldest first) and supports
/// cursor-based navigation for ↑/↓ arrow key traversal.
#[derive(Debug, Clone, Default)]
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

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return the suffix of the most recent history entry that starts with `prefix`.
    /// Returns `None` if `prefix` is empty, no entry matches, or only exact matches exist.
    pub fn suggest(&self, prefix: &str) -> Option<String> {
        if prefix.is_empty() {
            return None;
        }
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.starts_with(prefix) && entry.as_str() != prefix)
            .map(|entry| entry[prefix.len()..].to_string())
    }

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

    pub fn navigate_up(&mut self, current_line: &str) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }

        let new_cursor = match self.cursor {
            None => {
                self.saved_line = current_line.to_string();
                self.entries.len() - 1
            }
            Some(0) => 0,
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

    pub fn load(&mut self, path: &Path) {
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return,
        };
        let reader = BufReader::new(file);
        for line in reader.lines().flatten() {
            if !line.is_empty() {
                self.entries.push(line);
            }
        }
    }

    pub fn save(&self, path: &Path, histfilesize: usize) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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

    #[test]
    fn test_navigate_up_basic() {
        let mut h = History::new();
        h.add("first", 500, "");
        h.add("second", 500, "");
        h.add("third", 500, "");
        assert_eq!(h.navigate_up("current"), Some("third"));
        assert_eq!(h.navigate_up("current"), Some("second"));
        assert_eq!(h.navigate_up("current"), Some("first"));
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
        assert_eq!(h.navigate_down(), Some("typing"));
        assert_eq!(h.navigate_down(), Some("typing"));
    }

    #[test]
    fn test_navigate_saves_current_line() {
        let mut h = History::new();
        h.add("old_cmd", 500, "");
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
        assert_eq!(h.navigate_up("y"), Some("cmd2"));
    }

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
    fn test_load_skips_empty_lines() {
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

    #[test]
    fn test_suggest_prefix_match() {
        let mut h = History::new();
        h.add("git commit -m 'fix'", 500, "");
        h.add("git push origin main", 500, "");
        // "git c" matches "git commit -m 'fix'" — returns the suffix
        assert_eq!(h.suggest("git c"), Some("ommit -m 'fix'".to_string()));
    }

    #[test]
    fn test_suggest_most_recent_wins() {
        let mut h = History::new();
        h.add("echo first", 500, "");
        h.add("echo second", 500, "");
        // Both match "echo ", but most recent ("echo second") wins
        assert_eq!(h.suggest("echo "), Some("second".to_string()));
    }

    #[test]
    fn test_suggest_exact_match_excluded() {
        let mut h = History::new();
        h.add("ls -la", 500, "");
        // Exact match returns None (nothing to suggest)
        assert_eq!(h.suggest("ls -la"), None);
    }

    #[test]
    fn test_suggest_empty_prefix_returns_none() {
        let mut h = History::new();
        h.add("some command", 500, "");
        assert_eq!(h.suggest(""), None);
    }

    #[test]
    fn test_suggest_no_match_returns_none() {
        let mut h = History::new();
        h.add("git commit", 500, "");
        assert_eq!(h.suggest("cargo"), None);
    }

    #[test]
    fn test_suggest_empty_history_returns_none() {
        let h = History::new();
        assert_eq!(h.suggest("git"), None);
    }
}
