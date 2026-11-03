use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

/// First line of history files written in the v2 (newline-escaped) format.
/// Multiline entries are stored on one physical line with `\` and newline
/// escaped; files without this header are read as the legacy one-entry-
/// per-line format (under which multiline entries shattered into fragments
/// on reload).
const FILE_HEADER_V2: &str = "#yosh-history-v2";

/// Escape one entry for v2 persistence: `\` -> `\\`, newline -> `\n`.
fn escape_entry(entry: &str) -> String {
    let mut out = String::with_capacity(entry.len());
    for c in entry.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

/// Inverse of [`escape_entry`]. Lenient: an unknown escape or a trailing
/// lone backslash is kept literally.
fn unescape_entry(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The `(histsize, histcontrol)` pair [`History::add`] and
/// [`History::trim_to`] want, read from `$HISTSIZE` / `$HISTCONTROL` with
/// the REPL defaults (500, `ignoreboth`) when unset or unparsable.
/// HISTSIZE=0 keeps no entries and a negative HISTSIZE means unlimited
/// (bash semantics). Every place that adds to or trims the history list
/// must use this so they agree on what HISTSIZE means.
pub(crate) fn history_settings(vars: &crate::env::vars::VarStore) -> (usize, String) {
    let histsize: usize = vars
        .get("HISTSIZE")
        .and_then(|s| s.trim().parse::<i64>().ok())
        .map(|n| usize::try_from(n).unwrap_or(usize::MAX))
        .unwrap_or(500);
    let histcontrol = vars.get("HISTCONTROL").unwrap_or("ignoreboth").to_string();
    (histsize, histcontrol)
}

/// Command history storage with navigation and persistence.
///
/// Stores commands in chronological order (oldest first) and supports
/// cursor-based navigation for ↑/↓ arrow key traversal.
///
/// Every entry has a stable *history number* (POSIX `fc`: "the number
/// of a command shall not change"): `entries()[i]` is number
/// `first_number() + i`. Trimming the oldest entries to honor
/// `HISTSIZE` advances `first_number()` instead of renumbering.
#[derive(Debug, Clone, Default)]
pub struct History {
    entries: Vec<String>,
    /// Number of entries ever dropped from the front, so the history
    /// number of `entries[i]` is `base + i + 1`.
    base: usize,
    cursor: Option<usize>,
    saved_line: String,
}

impl History {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            base: 0,
            cursor: None,
            saved_line: String::new(),
        }
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// History number of the oldest retained entry (`entries()[0]`).
    /// Starts at 1 and only grows as old entries are trimmed. When the
    /// list is empty this is the number the next added entry will get.
    pub fn first_number(&self) -> usize {
        self.base + 1
    }

    /// Index into `entries()` of history number `number`, or `None`
    /// when that command was trimmed or does not exist yet.
    pub fn index_of_number(&self, number: usize) -> Option<usize> {
        number
            .checked_sub(self.first_number())
            .filter(|&idx| idx < self.entries.len())
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

        let tokens: HashSet<&str> = histcontrol.split(':').collect();
        let ignore_space = tokens.contains("ignorespace") || tokens.contains("ignoreboth");
        let ignore_dups = tokens.contains("ignoredups") || tokens.contains("ignoreboth");

        if ignore_space && line.starts_with(' ') {
            return;
        }

        if ignore_dups && self.entries.last().map(|s| s.as_str()) == Some(line) {
            return;
        }

        self.entries.push(line.to_string());
        self.truncate_to(histsize);
    }

    /// Drop the oldest entries beyond `histsize` (see [`history_settings`]
    /// for the mapping from `$HISTSIZE`). Used after loading the history
    /// file, which must honor an inherited HISTSIZE as `add` does.
    pub fn trim_to(&mut self, histsize: usize) {
        self.truncate_to(histsize);
    }

    /// Keep at most `histsize` most-recent entries (`0` keeps none,
    /// bash's HISTSIZE=0; callers map "unlimited" to `usize::MAX`).
    /// Dropped entries advance the numbering base so the numbers of the
    /// survivors do not change.
    fn truncate_to(&mut self, histsize: usize) {
        if self.entries.len() > histsize {
            let excess = self.entries.len() - histsize;
            self.entries.drain(..excess);
            self.base += excess;
            // Keep a recalled entry's cursor pointing at the same entry;
            // one that was itself dropped ends navigation.
            if let Some(c) = self.cursor {
                self.cursor = c.checked_sub(excess);
            }
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
            // Not navigating: returning the (empty) saved line here would
            // let the caller wipe a freshly typed buffer on a stray Down.
            None => return None,
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

    /// Current navigation cursor: `Some(idx)` while a history entry is
    /// recalled, `None` at the (saved) edit line.
    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    /// Jump the navigation cursor straight to `idx` (vi `G` and `/`/`?`
    /// searches). Saves the in-progress line the first time navigation
    /// leaves the edit line, like `navigate_up` does.
    pub fn navigate_to(&mut self, idx: usize, current_line: &str) -> Option<&str> {
        if idx >= self.entries.len() {
            return None;
        }
        if self.cursor.is_none() {
            self.saved_line = current_line.to_string();
        }
        self.cursor = Some(idx);
        Some(&self.entries[idx])
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
        // Read raw byte lines and byteenc-encode them: a line that is not
        // valid UTF-8 (written by another tool, or a command containing
        // raw bytes) must neither end the load — `BufRead::lines` would
        // stop there, and the next `save` would then truncate every
        // later entry out of the file — nor lose its bytes.
        let reader = BufReader::new(file);
        let mut lines = reader
            .split(b'\n')
            .map_while(Result::ok)
            .map(|raw| crate::byteenc::encode_bytes(&raw).into_owned())
            .peekable();
        let v2 = lines.peek().map(|l| l.as_str()) == Some(FILE_HEADER_V2);
        if v2 {
            lines.next();
        }
        for line in lines {
            if line.is_empty() {
                continue;
            }
            self.entries
                .push(if v2 { unescape_entry(&line) } else { line });
        }
    }

    /// Persist history entries to `path` in the v2 (newline-escaped) format,
    /// keeping at most `histfilesize` most-recent entries (`0` means keep
    /// all). One physical line per entry, so multiline commands survive a
    /// save/load round trip instead of shattering into per-line fragments.
    ///
    /// Errors from `create_dir_all`, `File::create`, and per-entry `writeln!` are
    /// propagated. On a partial-write failure the file is left truncated to
    /// whatever entries were written before the error; the caller surfaces the
    /// error (currently as a `yosh: warning:` at shell exit, without affecting
    /// the exit code).
    pub fn save(&self, path: &Path, histfilesize: usize) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // History can hold secrets typed at the prompt: create the file
        // owner-only (bash uses 0600). An existing file keeps its mode.
        let mut file = {
            use std::os::unix::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(path)?
        };
        writeln!(file, "{}", FILE_HEADER_V2)?;
        let start = if histfilesize > 0 && self.entries.len() > histfilesize {
            self.entries.len() - histfilesize
        } else {
            0
        };
        // Write entries as their original bytes (the inverse of `load`'s
        // byteenc encoding), buffered: one syscall per entry is slow for
        // a large HISTFILESIZE.
        let mut out = io::BufWriter::new(&mut file);
        for entry in &self.entries[start..] {
            out.write_all(&crate::byteenc::decode_bytes(&escape_entry(entry)))?;
            out.write_all(b"\n")?;
        }
        out.flush()?;
        Ok(())
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
    fn test_add_histcontrol_colon_separated_dups_and_space() {
        let mut h = History::new();
        h.add("ls", 500, "ignoredups:ignorespace");
        h.add("ls", 500, "ignoredups:ignorespace");
        h.add(" secret", 500, "ignoredups:ignorespace");
        h.add("pwd", 500, "ignoredups:ignorespace");
        assert_eq!(h.entries(), &["ls", "pwd"]);
    }

    #[test]
    fn test_add_histcontrol_colon_separated_reverse_order() {
        let mut h = History::new();
        h.add("ls", 500, "ignorespace:ignoredups");
        h.add("ls", 500, "ignorespace:ignoredups");
        h.add(" secret", 500, "ignorespace:ignoredups");
        h.add("pwd", 500, "ignorespace:ignoredups");
        assert_eq!(h.entries(), &["ls", "pwd"]);
    }

    #[test]
    fn test_add_histcontrol_unknown_token_ignored() {
        let mut h = History::new();
        h.add("ls", 500, "foo:ignoredups");
        h.add("ls", 500, "foo:ignoredups");
        h.add(" visible", 500, "foo:ignoredups");
        assert_eq!(h.entries(), &["ls", " visible"]);
    }

    #[test]
    fn test_add_histcontrol_only_unknown_tokens() {
        let mut h = History::new();
        h.add("ls", 500, "foo:bar");
        h.add("ls", 500, "foo:bar");
        h.add(" leading_space", 500, "foo:bar");
        assert_eq!(h.entries(), &["ls", "ls", " leading_space"]);
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
    fn test_numbers_stable_across_truncation() {
        // POSIX fc: a command's number shall not change.
        let mut h = History::new();
        assert_eq!(h.first_number(), 1);
        h.add("cmd1", 3, "");
        h.add("cmd2", 3, "");
        h.add("cmd3", 3, "");
        assert_eq!(h.index_of_number(2), Some(1));
        h.add("cmd4", 3, "");
        h.add("cmd5", 3, "");
        assert_eq!(h.first_number(), 3);
        assert_eq!(h.index_of_number(2), None);
        assert_eq!(h.index_of_number(3), Some(0));
        assert_eq!(h.entries()[h.index_of_number(5).unwrap()], "cmd5");
        assert_eq!(h.index_of_number(6), None);
        assert_eq!(h.index_of_number(0), None);
        assert_eq!(h.first_number(), 3);
        assert_eq!(h.first_number() + 2, 5);
    }

    #[test]
    fn test_histsize_zero_keeps_nothing() {
        let mut h = History::new();
        h.add("cmd1", 0, "");
        h.add("cmd2", 0, "");
        assert!(h.entries().is_empty());
        // Numbering still advances: the next kept command is number 3.
        h.add("cmd3", 500, "");
        assert_eq!(h.first_number(), 3);
        assert_eq!(h.index_of_number(3), Some(0));
    }

    #[test]
    fn test_save_creates_owner_only_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("hist");
        let mut h = History::new();
        h.add("secret", 500, "");
        h.save(&path, 500).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn test_add_empty_line_skipped() {
        let mut h = History::new();
        h.add("", 500, "");
        assert_eq!(h.entries().len(), 0);
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
        // Cursor is cleared after returning the saved line; a further Down
        // is not navigating and must not offer the saved line again.
        assert_eq!(h.navigate_down(), None);
    }

    #[test]
    fn test_navigate_down_without_navigation_returns_none() {
        let mut h = History::new();
        h.add("first", 500, "");
        assert_eq!(h.navigate_down(), None);
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
        h.save(&path, 500).unwrap();
        let mut h2 = History::new();
        h2.load(&path);
        assert_eq!(h2.entries(), &["cmd1", "cmd2"]);
    }

    #[test]
    fn test_load_nonexistent_file() {
        let mut h = History::new();
        h.load(std::path::Path::new("/nonexistent/path/history"));
        assert_eq!(h.entries().len(), 0);
    }

    #[test]
    fn test_save_histfilesize_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        let mut h = History::new();
        for i in 0..10 {
            h.add(&format!("cmd{}", i), 500, "");
        }
        h.save(&path, 3).unwrap();
        let mut h2 = History::new();
        h2.load(&path);
        assert_eq!(h2.entries(), &["cmd7", "cmd8", "cmd9"]);
    }

    #[test]
    fn test_save_returns_err_on_unwritable_parent() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let readonly_parent = dir.path().join("readonly");
        std::fs::create_dir(&readonly_parent).unwrap();
        let mut perms = std::fs::metadata(&readonly_parent).unwrap().permissions();
        perms.set_mode(0o555);
        std::fs::set_permissions(&readonly_parent, perms).unwrap();

        let path = readonly_parent.join("history");
        let mut h = History::new();
        h.add("cmd1", 500, "");

        let result = h.save(&path, 500);

        // Restore writable mode so tempdir cleanup can succeed
        let mut restore = std::fs::metadata(&readonly_parent).unwrap().permissions();
        restore.set_mode(0o755);
        std::fs::set_permissions(&readonly_parent, restore).ok();

        // On macOS/Linux as non-root, File::create inside a 0o555 dir fails.
        // If running as root (e.g., some CI containers), root bypasses mode bits and the
        // create succeeds; in that case we skip the assertion since the bug isn't
        // reproducible and the codepath is still exercised.
        if nix::unistd::geteuid().is_root() {
            eprintln!("test_save_returns_err_on_unwritable_parent: skipped (running as root)");
            return;
        }
        assert!(
            result.is_err(),
            "expected Err when parent dir is read-only, got {:?}",
            result
        );
    }

    #[test]
    fn test_load_skips_empty_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "cmd1").unwrap();
        writeln!(f).unwrap();
        writeln!(f, "cmd2").unwrap();
        let mut h = History::new();
        h.load(&path);
        assert_eq!(h.entries(), &["cmd1", "cmd2"]);
    }

    #[test]
    fn test_save_and_load_multiline_entry_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        let mut h = History::new();
        h.add("if true\nthen echo hi\nfi", 500, "");
        h.add("echo plain", 500, "");
        h.save(&path, 500).unwrap();
        let mut h2 = History::new();
        h2.load(&path);
        assert_eq!(h2.entries(), &["if true\nthen echo hi\nfi", "echo plain"]);
    }

    #[test]
    fn test_save_and_load_backslash_round_trip() {
        // A literal backslash-n typed by the user must stay two characters,
        // not become a real newline on reload.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        let mut h = History::new();
        h.add(r"printf 'a\nb'", 500, "");
        h.add(r"echo \\", 500, "");
        h.save(&path, 500).unwrap();
        let mut h2 = History::new();
        h2.load(&path);
        assert_eq!(h2.entries(), &[r"printf 'a\nb'", r"echo \\"]);
    }

    /// A line that is not valid UTF-8 used to end the load (and the next
    /// save then truncated every later entry out of the file).
    #[test]
    fn test_load_and_save_keep_invalid_utf8_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        let raw: &[u8] = b"#yosh-history-v2\necho one\necho \xff bad\necho three\n";
        std::fs::write(&path, raw).unwrap();
        let mut h = History::new();
        h.load(&path);
        assert_eq!(h.entries().len(), 3, "entries: {:?}", h.entries());
        assert_eq!(h.entries()[0], "echo one");
        assert_eq!(h.entries()[2], "echo three");
        assert_eq!(
            &*crate::byteenc::decode_bytes(&h.entries()[1]),
            b"echo \xff bad"
        );
        // Saved back as the original bytes.
        h.save(&path, 500).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), raw);
    }

    #[test]
    fn test_load_legacy_file_without_header_is_literal() {
        // Pre-v2 files have no header and no escaping: a literal `\n`
        // two-character sequence must load unchanged.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r"printf 'a\nb'").unwrap();
        writeln!(f, "cmd2").unwrap();
        let mut h = History::new();
        h.load(&path);
        assert_eq!(h.entries(), &[r"printf 'a\nb'", "cmd2"]);
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
