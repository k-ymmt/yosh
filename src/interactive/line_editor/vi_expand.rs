//! vi pathname expansion (`=` `\` `*`) of the bigword at the cursor.

use super::{KeyAction, LineEditor};
use crate::interactive::completion;
use crate::interactive::vi::{self, ViCmd, ViMode};

impl LineEditor {
    /// `=` lists the expansions of the bigword at the cursor; `\`
    /// completes it to the largest unique match; `*` replaces it with
    /// all matches. Bells when nothing matches.
    pub(super) fn vi_expand(&mut self, cmd: ViCmd) -> KeyAction {
        let Some(((start, end), mut matches)) = self.vi_expand_bigword() else {
            return self.bell();
        };
        match cmd {
            ViCmd::ExpandList => {
                // POSIX: directories are marked with a trailing '/'.
                for m in &mut matches {
                    if !m.ends_with('/') && std::path::Path::new(&m).is_dir() {
                        m.push('/');
                    }
                }
                KeyAction::ViListExpansions(matches)
            }
            ViCmd::CompleteUnique => {
                // Largest unique match: the longest common prefix of
                // all matches; a single match additionally gets '/'
                // (directory) or a space (file) appended.
                let mut replacement = completion::longest_common_prefix(&matches);
                if matches.len() == 1 {
                    if std::path::Path::new(&replacement).is_dir() {
                        if !replacement.ends_with('/') {
                            replacement.push('/');
                        }
                    } else {
                        replacement.push(' ');
                    }
                }
                self.vi_replace_range(start, end, &replacement);
                self.vi.mode = ViMode::Insert;
                KeyAction::Continue
            }
            _ => {
                // ExpandAll
                let replacement = matches.join(" ");
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
    /// contains none of `* ? [`, a `*` is implicitly appended. Returns
    /// the bigword range and the (sorted) matches.
    fn vi_expand_bigword(&self) -> Option<((usize, usize), Vec<String>)> {
        let (start, end) = self.vi_bigword_at_cursor()?;
        let word: String = self.buf[start..end].iter().collect();
        let pattern = if word.contains(['*', '?', '[']) {
            word
        } else {
            format!("{}*", word)
        };
        let matches = crate::expand::pathname::glob_match(&pattern);
        if matches.is_empty() {
            None
        } else {
            Some(((start, end), matches))
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
