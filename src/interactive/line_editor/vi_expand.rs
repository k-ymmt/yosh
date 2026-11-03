//! vi pathname expansion (`=` `\` `*`) of the bigword at the cursor.

use super::{KeyAction, LineEditor};
use crate::interactive::completion;
use crate::interactive::completion::quoting::{
    completion_replacement, quote_for_insertion, unclosed_quote,
};
use crate::interactive::vi::{self, ViCmd, ViMode};

impl LineEditor {
    /// `=` lists the expansions of the bigword at the cursor; `\`
    /// completes it to the largest unique match; `*` replaces it with
    /// all matches. Bells when nothing matches.
    ///
    /// Matches are inserted quoted for the shell (as Tab completion
    /// does, via [`quote_for_insertion`]), so a pathname containing
    /// blanks or metacharacters (`a b`, `a;b`) is read back as the
    /// single word it names — bash and ksh quote the same way.
    pub(super) fn vi_expand(&mut self, cmd: ViCmd) -> KeyAction {
        let Some(((start, end), open_quote, mut matches)) = self.vi_expand_bigword() else {
            return self.bell();
        };
        match cmd {
            ViCmd::ExpandList => {
                // POSIX: directories are marked with a trailing '/'.
                for m in &mut matches {
                    if !m.ends_with('/') && is_dir(m) {
                        m.push('/');
                    }
                }
                KeyAction::ViListExpansions(matches)
            }
            ViCmd::CompleteUnique => {
                // Largest unique match: the longest common prefix of
                // all matches; a single match additionally gets '/'
                // (directory) or a space (file) appended. A quote the
                // bigword left open stays open for a partial match or
                // a directory and is closed after a completed file,
                // like Tab completion.
                let mut candidate = completion::longest_common_prefix(&matches);
                let finish = matches.len() == 1;
                if finish && is_dir(&candidate) && !candidate.ends_with('/') {
                    candidate.push('/');
                }
                let prefix: String = open_quote.map(String::from).unwrap_or_default();
                let replacement = completion_replacement(&prefix, &candidate, finish);
                self.vi_replace_range(start, end, &replacement);
                self.vi.mode = ViMode::Insert;
                KeyAction::Continue
            }
            _ => {
                // ExpandAll: each match becomes its own word, quoted on
                // its own, and replaces the whole bigword — a quote the
                // bigword left open (`"Cargo.t`) is consumed with it, as
                // bash and ksh do.
                let replacement = matches
                    .iter()
                    .map(|m| quote_for_insertion("", m))
                    .collect::<Vec<_>>()
                    .join(" ");
                self.vi_replace_range(start, end, &replacement);
                self.vi.mode = ViMode::Insert;
                KeyAction::Continue
            }
        }
    }

    /// The bigword containing (or immediately before) the cursor on the
    /// current logical line, as a char range.
    fn vi_bigword_at_cursor(&self) -> Option<(usize, usize)> {
        let ls = vi::line_start(&self.buf, self.pos);
        let le = vi::line_end(&self.buf, self.pos);
        if ls == le {
            return None;
        }
        let mut p = self.pos.min(le - 1);
        while p > ls && vi::is_blank(self.buf[p]) {
            p -= 1;
        }
        if vi::is_blank(self.buf[p]) {
            return None;
        }
        let mut start = p;
        while start > ls && !vi::is_blank(self.buf[start - 1]) {
            start -= 1;
        }
        let mut end = p + 1;
        while end < le && !vi::is_blank(self.buf[end]) {
            end += 1;
        }
        Some((start, end))
    }

    /// Pathname-expand the bigword at the cursor. POSIX: if the bigword
    /// contains none of `* ? [`, a `*` is implicitly appended. Quoting
    /// in the bigword is honoured as the shell would: quoted characters
    /// match literally. Returns the bigword range, the quote the bigword
    /// leaves open (if any), and the (sorted) matches.
    fn vi_expand_bigword(&self) -> Option<ExpandedBigword> {
        let (start, end) = self.vi_bigword_at_cursor()?;
        let word: String = self.buf[start..end].iter().collect();
        let (mut pattern, has_glob) = bigword_glob_pattern(&word);
        if !has_glob {
            pattern.push('*');
        }
        let matches = crate::expand::pathname::glob_match(&pattern);
        if matches.is_empty() {
            None
        } else {
            Some(((start, end), unclosed_quote(&word), matches))
        }
    }

    /// Replace the char range `[start, end)` with `text`, leaving the
    /// cursor after the inserted text.
    fn vi_replace_range(&mut self, start: usize, end: usize, text: &str) {
        self.save_undo_posix();
        self.buf.splice(start..end, text.chars());
        self.pos = start + text.chars().count();
        self.invalidate_width_cache();
    }
}

/// `(bigword char range, quote left open, sorted matches)`.
type ExpandedBigword = ((usize, usize), Option<char>, Vec<String>);

/// Whether the (byteenc-encoded) glob match names a directory.
fn is_dir(path: &str) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let bytes = crate::byteenc::decode_bytes(path);
    std::path::Path::new(std::ffi::OsStr::from_bytes(&bytes)).is_dir()
}

/// Translate a typed (possibly quoted) bigword into a glob pattern for
/// [`crate::expand::pathname::glob_match`]: quote removal as the lexer
/// performs it, with every quoted pattern character (`* ? [ ] \`)
/// backslash-escaped so it matches literally. Returns the pattern and
/// whether it contains an unquoted `*`, `?` or `[`. Unterminated quotes
/// and a trailing `\` are tolerated (the word is still being typed).
fn bigword_glob_pattern(word: &str) -> (String, bool) {
    fn push_literal(out: &mut String, c: char) {
        if matches!(c, '*' | '?' | '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    let mut out = String::with_capacity(word.len() + 1);
    let mut has_glob = false;
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = word.chars();
    while let Some(c) = chars.next() {
        if in_single {
            if c == '\'' {
                in_single = false;
            } else {
                push_literal(&mut out, c);
            }
            continue;
        }
        match c {
            '\'' if !in_double => in_single = true,
            '"' => in_double = !in_double,
            '\\' => match chars.next() {
                None => push_literal(&mut out, '\\'),
                Some(n) if in_double && !matches!(n, '$' | '`' | '"' | '\\' | '\n') => {
                    push_literal(&mut out, '\\');
                    push_literal(&mut out, n);
                }
                Some(n) => push_literal(&mut out, n),
            },
            c if in_double => push_literal(&mut out, c),
            '*' | '?' | '[' => {
                has_glob = true;
                out.push(c);
            }
            c => out.push(c),
        }
    }
    (out, has_glob)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor_with(line: &str) -> LineEditor {
        let mut ed = LineEditor::new();
        ed.buf = line.chars().collect();
        ed.pos = ed.buf.len().saturating_sub(1);
        ed
    }

    fn line_of(ed: &LineEditor) -> String {
        ed.buf.iter().collect()
    }

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a;b", "a b", "a$c"] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        dir
    }

    #[test]
    fn glob_pattern_honours_quoting() {
        assert_eq!(bigword_glob_pattern("ab"), ("ab".into(), false));
        assert_eq!(bigword_glob_pattern("a*"), ("a*".into(), true));
        assert_eq!(bigword_glob_pattern("'a*'"), ("a\\*".into(), false));
        assert_eq!(bigword_glob_pattern("\"a?\""), ("a\\?".into(), false));
        assert_eq!(bigword_glob_pattern("a\\;"), ("a;".into(), false));
        assert_eq!(bigword_glob_pattern("a\\["), ("a\\[".into(), false));
        assert_eq!(bigword_glob_pattern("\"a\\b\""), ("a\\\\b".into(), false));
        assert_eq!(bigword_glob_pattern("\"ab"), ("ab".into(), false));
    }

    /// `*` must quote matches containing metacharacters, or `a;b`
    /// becomes two commands when the line is run.
    #[test]
    fn expand_all_quotes_metacharacters() {
        let dir = fixture();
        let base = dir.path().to_str().unwrap();
        let mut ed = editor_with(&format!("ls {base}/a"));
        ed.vi_expand(ViCmd::ExpandAll);
        assert_eq!(
            line_of(&ed),
            format!("ls {base}/a\\ b {base}/a\\$c {base}/a\\;b")
        );
    }

    #[test]
    fn complete_unique_quotes_single_match() {
        let dir = fixture();
        let base = dir.path().to_str().unwrap();
        let mut ed = editor_with(&format!("ls {base}/a\\;"));
        ed.vi_expand(ViCmd::CompleteUnique);
        assert_eq!(line_of(&ed), format!("ls {base}/a\\;b "));
    }

    #[test]
    fn complete_unique_keeps_and_closes_open_quote() {
        let dir = fixture();
        let base = dir.path().to_str().unwrap();
        let mut ed = editor_with(&format!("ls \"{base}/a$"));
        ed.vi_expand(ViCmd::CompleteUnique);
        assert_eq!(line_of(&ed), format!("ls \"{base}/a\\$c\" "));
    }

    /// An open quote in the bigword must not stop `*`: the matches
    /// replace the whole bigword, quote included.
    #[test]
    fn expand_all_consumes_open_quote() {
        let dir = fixture();
        let base = dir.path().to_str().unwrap();
        let mut ed = editor_with(&format!("ls \"{base}/a$"));
        ed.vi_expand(ViCmd::ExpandAll);
        assert_eq!(line_of(&ed), format!("ls {base}/a\\$c"));
        assert_eq!(ed.vi.mode, ViMode::Insert);
    }

    #[test]
    fn expand_all_with_quoted_glob_char_matches_literally() {
        let dir = fixture();
        std::fs::write(dir.path().join("x*y"), "").unwrap();
        std::fs::write(dir.path().join("xzy"), "").unwrap();
        let base = dir.path().to_str().unwrap();
        let mut ed = editor_with(&format!("ls {base}/x\\*"));
        ed.vi_expand(ViCmd::ExpandAll);
        assert_eq!(line_of(&ed), format!("ls {base}/x\\*y"));
    }
}
