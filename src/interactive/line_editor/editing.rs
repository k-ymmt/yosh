//! Buffer and cursor editing primitives, plus the display-width cache
//! every buffer mutation must invalidate.

use unicode_width::UnicodeWidthChar;

use super::LineEditor;
use crate::interactive::vi;

impl LineEditor {
    /// Insert a character at the current cursor position and advance
    /// the cursor by one.
    pub fn insert_char(&mut self, ch: char) {
        self.buf.insert(self.pos, ch);
        self.pos += 1;
        self.invalidate_width_cache();
    }

    /// Delete the character immediately before the cursor (like the
    /// Backspace key).  Does nothing when the cursor is at position 0.
    pub fn backspace(&mut self) {
        if self.pos > 0 {
            self.pos -= 1;
            self.buf.remove(self.pos);
            self.invalidate_width_cache();
        }
    }

    /// Delete the character at the current cursor position (like the
    /// Delete key).  Does nothing when the cursor is at the end of
    /// the buffer.
    pub fn delete(&mut self) {
        if self.pos < self.buf.len() {
            self.buf.remove(self.pos);
            self.invalidate_width_cache();
        }
    }

    /// Move the cursor one position to the left.  Does nothing when
    /// the cursor is already at position 0.
    pub fn move_cursor_left(&mut self) {
        if self.pos > 0 {
            self.pos -= 1;
        }
    }

    /// Move the cursor one position to the right.  Does nothing when
    /// the cursor is already at the end of the buffer.
    pub fn move_cursor_right(&mut self) {
        if self.pos < self.buf.len() {
            self.pos += 1;
        }
    }

    /// Return the 0-based index of the logical line the cursor is on.
    pub fn cursor_line_index(&self) -> usize {
        self.buf[..self.pos].iter().filter(|&&c| c == '\n').count()
    }

    /// Return the number of logical lines in the buffer (1 when empty).
    pub fn line_count(&self) -> usize {
        self.buf.iter().filter(|&&c| c == '\n').count() + 1
    }

    /// Move the cursor to the previous logical line, preserving the display
    /// column as closely as possible. Does nothing on the first line.
    ///
    /// Consecutive vertical moves keep the column of the *first* move
    /// (readline-style preferred column): passing through a shorter line
    /// clamps the cursor visually but does not shrink the target column
    /// for subsequent moves.
    pub fn move_cursor_up(&mut self) {
        let cur_start = vi::line_start(&self.buf, self.pos);
        if cur_start == 0 {
            return;
        }
        let visual: usize = self.buf[cur_start..self.pos]
            .iter()
            .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
            .sum();
        let target = *self.preferred_col.get_or_insert(visual);
        let prev_end = cur_start - 1; // index of the '\n'
        let prev_start = vi::line_start(&self.buf, prev_end);
        self.pos = Self::pos_at_column(&self.buf, prev_start, prev_end, target);
    }

    /// Move the cursor to the next logical line, preserving the display
    /// column as closely as possible. Does nothing on the last line.
    /// Uses the same preferred-column stickiness as [`move_cursor_up`].
    pub fn move_cursor_down(&mut self) {
        let cur_end = vi::line_end(&self.buf, self.pos);
        if cur_end == self.buf.len() {
            return;
        }
        let cur_start = vi::line_start(&self.buf, self.pos);
        let visual: usize = self.buf[cur_start..self.pos]
            .iter()
            .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
            .sum();
        let target = *self.preferred_col.get_or_insert(visual);
        let next_start = cur_end + 1;
        let next_end = vi::line_end(&self.buf, next_start);
        self.pos = Self::pos_at_column(&self.buf, next_start, next_end, target);
    }

    /// Walk `buf[start..end]` accumulating display width and return the char
    /// index closest to (without exceeding) `target` columns.
    fn pos_at_column(buf: &[char], start: usize, end: usize, target: usize) -> usize {
        let mut w = 0usize;
        let mut p = start;
        while p < end {
            let cw = UnicodeWidthChar::width(buf[p]).unwrap_or(0);
            if w + cw > target {
                break;
            }
            w += cw;
            p += 1;
        }
        p
    }

    /// Move the cursor to the beginning of the current logical line.
    /// (For a single-line buffer this is position 0, as before.)
    pub fn move_to_start(&mut self) {
        self.pos = vi::line_start(&self.buf, self.pos);
    }

    /// Move the cursor to the end of the current logical line.
    /// (For a single-line buffer this is the end of the buffer, as before.)
    pub fn move_to_end(&mut self) {
        self.pos = vi::line_end(&self.buf, self.pos);
    }

    /// Returns true if `ch` is a word character (alphanumeric or underscore).
    fn is_word_char(ch: char) -> bool {
        ch.is_alphanumeric() || ch == '_'
    }

    /// Move cursor backward to the start of the previous word.
    pub fn move_backward_word(&mut self) {
        while self.pos > 0 && !Self::is_word_char(self.buf[self.pos - 1]) {
            self.pos -= 1;
        }
        while self.pos > 0 && Self::is_word_char(self.buf[self.pos - 1]) {
            self.pos -= 1;
        }
    }

    /// Move cursor forward to the end of the next word.
    pub fn move_forward_word(&mut self) {
        let len = self.buf.len();
        while self.pos < len && !Self::is_word_char(self.buf[self.pos]) {
            self.pos += 1;
        }
        while self.pos < len && Self::is_word_char(self.buf[self.pos]) {
            self.pos += 1;
        }
    }

    /// Kill from cursor to end of the current logical line. When the cursor
    /// is already at the line end and a newline follows, kill the newline
    /// (emacs `C-k` behavior). Returns the killed text.
    pub fn kill_to_end(&mut self) -> String {
        let end = vi::line_end(&self.buf, self.pos);
        if self.pos == end && end < self.buf.len() {
            self.buf.remove(self.pos);
            self.invalidate_width_cache();
            return "\n".to_string();
        }
        let killed: String = self.buf[self.pos..end].iter().collect();
        self.buf.drain(self.pos..end);
        self.invalidate_width_cache();
        killed
    }

    /// Kill from the start of the current logical line to the cursor
    /// (readline `unix-line-discard`). Returns the killed text.
    pub fn kill_to_start(&mut self) -> String {
        let start = vi::line_start(&self.buf, self.pos);
        let killed: String = self.buf[start..self.pos].iter().collect();
        self.buf.drain(start..self.pos);
        self.pos = start;
        self.invalidate_width_cache();
        killed
    }

    /// Kill the word behind the cursor. Returns the killed text.
    pub fn kill_backward_word(&mut self) -> String {
        let old_pos = self.pos;
        self.move_backward_word();
        let killed: String = self.buf[self.pos..old_pos].iter().collect();
        self.buf.drain(self.pos..old_pos);
        self.invalidate_width_cache();
        killed
    }

    /// Kill from cursor to end of the next word. Returns the killed text.
    pub fn kill_forward_word(&mut self) -> String {
        let old_pos = self.pos;
        let len = self.buf.len();
        let mut end = self.pos;
        while end < len && !Self::is_word_char(self.buf[end]) {
            end += 1;
        }
        while end < len && Self::is_word_char(self.buf[end]) {
            end += 1;
        }
        let killed: String = self.buf[old_pos..end].iter().collect();
        self.buf.drain(old_pos..end);
        self.invalidate_width_cache();
        killed
    }

    /// Transpose the two characters around the cursor (Ctrl+T).
    pub fn transpose_chars(&mut self) {
        if self.buf.len() < 2 {
            return;
        }
        if self.pos == 0 {
            return;
        }
        if self.pos == self.buf.len() {
            self.buf.swap(self.pos - 2, self.pos - 1);
        } else {
            self.buf.swap(self.pos - 1, self.pos);
            self.pos += 1;
        }
        // A swap can't change the total display width, but the *content*
        // changed: the generation bump inside invalidate_width_cache is
        // what keeps suggestion_cache from serving a suggestion computed
        // for the pre-swap buffer.
        self.invalidate_width_cache();
    }

    /// Transpose the two words around the cursor (Alt+T).
    pub fn transpose_words(&mut self) {
        let len = self.buf.len();
        if len == 0 {
            return;
        }

        let mut p = self.pos;
        if p == len || !Self::is_word_char(self.buf[p]) {
            while p > 0 && !Self::is_word_char(self.buf[p - 1]) {
                p -= 1;
            }
        }
        if p == 0 {
            return;
        }

        // Find end of word2
        let w2e = if self.pos < len && Self::is_word_char(self.buf[self.pos]) {
            let mut e = self.pos;
            while e < len && Self::is_word_char(self.buf[e]) {
                e += 1;
            }
            e
        } else {
            p
        };

        // Find start of word2
        let mut w2s = w2e;
        while w2s > 0 && Self::is_word_char(self.buf[w2s - 1]) {
            w2s -= 1;
        }
        if w2s == 0 {
            return;
        }

        // Find end of word1
        let mut w1e = w2s;
        while w1e > 0 && !Self::is_word_char(self.buf[w1e - 1]) {
            w1e -= 1;
        }
        if w1e == 0 {
            return;
        }

        // Find start of word1
        let mut w1s = w1e;
        while w1s > 0 && Self::is_word_char(self.buf[w1s - 1]) {
            w1s -= 1;
        }

        let word1: Vec<char> = self.buf[w1s..w1e].to_vec();
        let sep: Vec<char> = self.buf[w1e..w2s].to_vec();
        let word2: Vec<char> = self.buf[w2s..w2e].to_vec();

        let mut replacement = Vec::new();
        replacement.extend_from_slice(&word2);
        replacement.extend_from_slice(&sep);
        replacement.extend_from_slice(&word1);

        self.buf.splice(w1s..w2e, replacement);
        self.pos = w1s + word2.len() + sep.len() + word1.len();
        // Reordering can't change the total display width, but the content
        // generation must advance so suggestion_cache doesn't serve a
        // suggestion computed for the pre-transpose buffer.
        self.invalidate_width_cache();
    }

    /// Shared body of the case-mapping word commands (Alt+U/L/C): skip
    /// to the next word, apply `f(char, is_first_char_of_word)` to each
    /// of its characters, and leave the cursor just past it.
    fn map_word<F: Fn(char, bool) -> char>(&mut self, f: F) {
        let len = self.buf.len();
        while self.pos < len && !Self::is_word_char(self.buf[self.pos]) {
            self.pos += 1;
        }
        let mut first = true;
        while self.pos < len && Self::is_word_char(self.buf[self.pos]) {
            self.buf[self.pos] = f(self.buf[self.pos], first);
            first = false;
            self.pos += 1;
        }
        self.invalidate_width_cache();
    }

    /// Convert the next word to uppercase (Alt+U).
    pub fn upcase_word(&mut self) {
        self.map_word(|c, _| c.to_uppercase().next().unwrap_or(c));
    }

    /// Convert the next word to lowercase (Alt+L).
    pub fn downcase_word(&mut self) {
        self.map_word(|c, _| c.to_lowercase().next().unwrap_or(c));
    }

    /// Capitalize the next word: first char uppercase, rest lowercase (Alt+C).
    pub fn capitalize_word(&mut self) {
        self.map_word(|c, first| {
            if first {
                c.to_uppercase().next().unwrap_or(c)
            } else {
                c.to_lowercase().next().unwrap_or(c)
            }
        });
    }

    /// Insert text at the current cursor position. Returns (start, len) for yank tracking.
    pub fn insert_str(&mut self, text: &str) -> (usize, usize) {
        let start = self.pos;
        let chars: Vec<char> = text.chars().collect();
        let len = chars.len();
        for (i, ch) in chars.into_iter().enumerate() {
            self.buf.insert(self.pos + i, ch);
        }
        self.pos += len;
        self.invalidate_width_cache();
        (start, len)
    }

    /// Remove `len` characters starting at `start`. Used by yank_pop to replace yanked text.
    pub fn remove_range(&mut self, start: usize, len: usize) {
        let end = (start + len).min(self.buf.len());
        self.buf.drain(start..end);
        if self.pos > start {
            self.pos = start;
        }
        self.invalidate_width_cache();
    }

    /// Invalidate width/suggestion caches that depend on buffer *content*.
    /// Must be called by every method that mutates `self.buf`'s contents
    /// (not just cursor position). Bumping `buf_generation` here — rather
    /// than at each call site individually — is what lets `update_suggestion`
    /// tell "buffer changed" apart from "only the cursor moved" without
    /// having to enumerate every `EditAction` that can mutate `buf`.
    pub(super) fn invalidate_width_cache(&mut self) {
        self.cached_total_width = None;
        self.buf_generation += 1;
        // Any content change ends a run of consecutive vertical moves.
        self.preferred_col = None;
    }

    /// Return `(prefix_width, total_width)`: the display width of
    /// `buf[..pos]` and of the whole buffer, computed together in a single
    /// pass instead of the two independent `UnicodeWidthChar::width` scans
    /// `redraw` used to run per keystroke. The total width is cached and
    /// reused across calls while the buffer is unchanged (e.g. pure cursor
    /// movement, which never invalidates the cache), so a pure cursor move
    /// only pays for a fresh prefix-width scan, not a second full-buffer
    /// scan.
    pub(super) fn buf_prefix_and_total_width(&mut self) -> (usize, usize) {
        if let Some(total) = self.cached_total_width {
            let prefix: usize = self.buf[..self.pos]
                .iter()
                .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
                .sum();
            return (prefix, total);
        }
        let mut prefix = 0usize;
        let mut total = 0usize;
        for (i, c) in self.buf.iter().enumerate() {
            let w = UnicodeWidthChar::width(*c).unwrap_or(0);
            total += w;
            if i < self.pos {
                prefix += w;
            }
        }
        self.cached_total_width = Some(total);
        (prefix, total)
    }
}
