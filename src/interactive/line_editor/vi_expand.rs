//! vi pathname expansion (`=` `\` `*`) of the bigword at the cursor.

use super::LineEditor;
use crate::interactive::vi;

impl LineEditor {
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
    pub(super) fn vi_expand_bigword(&self) -> Option<((usize, usize), Vec<String>)> {
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
    pub(super) fn vi_replace_range(&mut self, start: usize, end: usize, text: &str) {
        self.save_undo_posix();
        self.buf.splice(start..end, text.chars());
        self.pos = start + text.chars().count();
        self.invalidate_width_cache();
    }
}
