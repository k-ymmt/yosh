use crossterm::event::{Event, KeyEvent};
use std::io;
use unicode_width::UnicodeWidthChar;

use super::command_completion::CommandCompletionContext;
use super::completion::{
    self, CompletionContext, CompletionUI, extract_completion_word, is_command_position,
};
use super::display_width::display_width;
use super::edit_action::EditAction;
use super::fuzzy_search::FuzzySearchUI;
use super::highlight::{CheckerEnv, ColorSpan, HighlightScanner, HighlightStyle, apply_style};
use super::history::History;
use super::keymap::{BufferState, Keymap};
use super::kill_ring::KillRing;
use super::terminal::Terminal;
use super::undo::UndoManager;

/// Return the highlight style covering char index `i`, advancing `span_idx`
/// forward as needed.
///
/// `spans` must be sorted by `start` and non-overlapping (true of every
/// `HighlightScanner::scan` result — spans are pushed in left-to-right scan
/// order). Callers must invoke this with strictly increasing `i` across a
/// single pass and share the same `span_idx` across calls, mirroring how
/// `redraw` walks the buffer once per char. `span_idx` may end up pointing
/// past a span whose range doesn't include `i` (e.g. gaps between spans);
/// that's fine — the `Some(sp) if ...` guard falls through to `Default`
/// without advancing past spans that still might cover a later `i`.
fn style_at_advancing(spans: &[ColorSpan], span_idx: &mut usize, i: usize) -> HighlightStyle {
    while *span_idx < spans.len() && spans[*span_idx].end <= i {
        *span_idx += 1;
    }
    match spans.get(*span_idx) {
        Some(sp) if sp.start <= i && i < sp.end => sp.style,
        _ => HighlightStyle::Default,
    }
}

/// Find the first char index in `[0, limit)` where the highlight style
/// produced by `old_spans` differs from `new_spans`, or `limit` if they
/// agree on the whole range.
///
/// Used to cap the diff-based partial-repaint start column: even when the
/// characters in `[0, limit)` are identical between renders, a scanner
/// re-classification (e.g. a quote becoming closed) can change which style
/// applies to that unchanged text, and that region must still be repainted.
fn style_diff_pos(old_spans: &[ColorSpan], new_spans: &[ColorSpan], limit: usize) -> usize {
    if old_spans.is_empty() && new_spans.is_empty() {
        return limit;
    }
    let mut old_idx = 0;
    let mut new_idx = 0;
    for i in 0..limit {
        let old_style = style_at_advancing(old_spans, &mut old_idx, i);
        let new_style = style_at_advancing(new_spans, &mut new_idx, i);
        if old_style != new_style {
            return i;
        }
    }
    limit
}

/// A minimal line-editing buffer used by the interactive REPL.
///
/// The buffer stores characters as a `Vec<char>` so that cursor
/// movement and insertion work correctly with multi-byte UTF-8
/// characters.
pub struct LineEditor {
    buf: Vec<char>,
    pos: usize,
    suggestion: Option<String>,
    tab_count: u8,
    keymap: Keymap,
    kill_ring: KillRing,
    undo: UndoManager,
    yank_state: Option<YankState>,
    last_action: EditAction,
    last_was_insert: bool,
    prev_total_rows: usize,
    /// Cached total display width of `buf`, invalidated (`None`) on every
    /// buffer mutation and lazily recomputed the next time it's needed.
    /// Avoids re-summing `UnicodeWidthChar::width` over the whole buffer on
    /// every keystroke, including pure cursor-movement keys that don't
    /// touch `buf` at all.
    cached_total_width: Option<usize>,
    /// Bumped every time `buf`'s *content* changes (not on pure cursor
    /// movement). Used to detect whether a previously computed suggestion
    /// is still valid for the current buffer content, so cursor-only
    /// actions (which can still hide/reveal the suggestion depending on
    /// whether the cursor is at the end of the buffer) don't need to
    /// re-run `History::suggest` — only re-derive it from the cache.
    buf_generation: u64,
    /// The `(buf_generation, result)` pair last passed to `History::suggest`,
    /// so `update_suggestion` can reuse the result when `buf_generation`
    /// hasn't changed instead of rebuilding `self.buffer()` and re-querying
    /// history on every keystroke.
    suggestion_cache: Option<(u64, Option<String>)>,
    /// `(buf, spans)` as last painted to the screen by `redraw`, used to
    /// find the first column that actually needs repainting. `None` before
    /// the first `redraw` call (or after anything that invalidates the
    /// on-screen state wholesale, e.g. `clear`).
    prev_render: Option<(Vec<char>, Vec<ColorSpan>)>,
}

#[derive(Debug, Clone)]
struct YankState {
    start: usize,
    len: usize,
}

impl Default for LineEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl LineEditor {
    /// Create an empty line editor.
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            pos: 0,
            suggestion: None,
            tab_count: 0,
            keymap: Keymap::new(),
            kill_ring: KillRing::new(60),
            undo: UndoManager::new(256),
            yank_state: None,
            last_action: EditAction::Noop,
            last_was_insert: false,
            prev_total_rows: 0,
            cached_total_width: None,
            buf_generation: 0,
            suggestion_cache: None,
            prev_render: None,
        }
    }

    /// Return the current buffer contents as a `String`.
    pub fn buffer(&self) -> String {
        self.buf.iter().collect()
    }

    /// Return the current cursor position (0-based character index).
    #[allow(dead_code)] // public API for interactive mode enhancements
    pub fn cursor(&self) -> usize {
        self.pos
    }

    /// Return `true` if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Clear the buffer and reset the cursor to 0.
    pub fn clear(&mut self) {
        self.buf.clear();
        self.pos = 0;
        self.suggestion = None;
        self.tab_count = 0;
        self.yank_state = None;
        self.last_action = EditAction::Noop;
        self.last_was_insert = false;
        self.undo.clear();
        self.prev_total_rows = 0;
        self.invalidate_width_cache();
        self.prev_render = None;
    }

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

    /// Move the cursor to the beginning of the buffer (position 0).
    pub fn move_to_start(&mut self) {
        self.pos = 0;
    }

    /// Move the cursor to the end of the buffer.
    pub fn move_to_end(&mut self) {
        self.pos = self.buf.len();
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

    /// Kill from cursor to end of line. Returns the killed text.
    pub fn kill_to_end(&mut self) -> String {
        let killed: String = self.buf[self.pos..].iter().collect();
        self.buf.truncate(self.pos);
        self.invalidate_width_cache();
        killed
    }

    /// Kill from start of line to cursor. Returns the killed text.
    pub fn kill_to_start(&mut self) -> String {
        let killed: String = self.buf[..self.pos].iter().collect();
        self.buf.drain(..self.pos);
        self.pos = 0;
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
        // A swap can't change the total display width (same multiset of
        // chars), so the cache stays valid — no invalidation needed.
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
        // Reordering existing chars can't change the total display width.
    }

    /// Convert the next word to uppercase (Alt+U).
    pub fn upcase_word(&mut self) {
        let len = self.buf.len();
        while self.pos < len && !Self::is_word_char(self.buf[self.pos]) {
            self.pos += 1;
        }
        while self.pos < len && Self::is_word_char(self.buf[self.pos]) {
            self.buf[self.pos] = self.buf[self.pos]
                .to_uppercase()
                .next()
                .unwrap_or(self.buf[self.pos]);
            self.pos += 1;
        }
        self.invalidate_width_cache();
    }

    /// Convert the next word to lowercase (Alt+L).
    pub fn downcase_word(&mut self) {
        let len = self.buf.len();
        while self.pos < len && !Self::is_word_char(self.buf[self.pos]) {
            self.pos += 1;
        }
        while self.pos < len && Self::is_word_char(self.buf[self.pos]) {
            self.buf[self.pos] = self.buf[self.pos]
                .to_lowercase()
                .next()
                .unwrap_or(self.buf[self.pos]);
            self.pos += 1;
        }
        self.invalidate_width_cache();
    }

    /// Capitalize the next word: first char uppercase, rest lowercase (Alt+C).
    pub fn capitalize_word(&mut self) {
        let len = self.buf.len();
        while self.pos < len && !Self::is_word_char(self.buf[self.pos]) {
            self.pos += 1;
        }
        let mut first = true;
        while self.pos < len && Self::is_word_char(self.buf[self.pos]) {
            if first {
                self.buf[self.pos] = self.buf[self.pos]
                    .to_uppercase()
                    .next()
                    .unwrap_or(self.buf[self.pos]);
                first = false;
            } else {
                self.buf[self.pos] = self.buf[self.pos]
                    .to_lowercase()
                    .next()
                    .unwrap_or(self.buf[self.pos]);
            }
            self.pos += 1;
        }
        self.invalidate_width_cache();
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

    /// Return the current suggestion text, if any.
    #[allow(dead_code)]
    pub fn suggestion(&self) -> Option<&str> {
        self.suggestion.as_deref()
    }

    /// Accept the full autosuggestion, appending it to the buffer.
    fn accept_full_suggestion(&mut self) {
        if let Some(suggestion) = self.suggestion.take() {
            self.buf.extend(suggestion.chars());
            self.pos = self.buf.len();
            self.invalidate_width_cache();
        }
    }

    /// Accept the next word from the autosuggestion.
    /// A "word" is defined as: any leading spaces + non-space characters up to the next space.
    fn accept_word_suggestion(&mut self) {
        if let Some(suggestion) = self.suggestion.take() {
            let chars: Vec<char> = suggestion.chars().collect();
            let mut i = 0;
            // Skip leading spaces
            while i < chars.len() && chars[i] == ' ' {
                i += 1;
            }
            // Take non-space characters
            while i < chars.len() && chars[i] != ' ' {
                i += 1;
            }
            // Append the accepted portion to the buffer
            self.buf.extend(&chars[..i]);
            self.pos = self.buf.len();
            self.invalidate_width_cache();
            // Keep remaining suggestion, if any
            if i < chars.len() {
                self.suggestion = Some(chars[i..].iter().collect());
            }
        }
    }

    /// Invalidate width/suggestion caches that depend on buffer *content*.
    /// Must be called by every method that mutates `self.buf`'s contents
    /// (not just cursor position). Bumping `buf_generation` here — rather
    /// than at each call site individually — is what lets `update_suggestion`
    /// tell "buffer changed" apart from "only the cursor moved" without
    /// having to enumerate every `EditAction` that can mutate `buf`.
    fn invalidate_width_cache(&mut self) {
        self.cached_total_width = None;
        self.buf_generation += 1;
    }

    /// Return `(prefix_width, total_width)`: the display width of
    /// `buf[..pos]` and of the whole buffer, computed together in a single
    /// pass instead of the two independent `UnicodeWidthChar::width` scans
    /// `redraw` used to run per keystroke. The total width is cached and
    /// reused across calls while the buffer is unchanged (e.g. pure cursor
    /// movement, which never invalidates the cache), so a pure cursor move
    /// only pays for a fresh prefix-width scan, not a second full-buffer
    /// scan.
    fn buf_prefix_and_total_width(&mut self) -> (usize, usize) {
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

    /// Update the autosuggestion based on the current buffer state.
    /// Only suggests when the cursor is at the end of a non-empty buffer.
    ///
    /// Pure cursor movement (and any other action that leaves `buf`'s
    /// content unchanged) never bumps `buf_generation`, so once a
    /// suggestion has been computed for the current content it's reused
    /// from `suggestion_cache` instead of rebuilding `self.buffer()` and
    /// re-running `History::suggest` on every keystroke — including keys
    /// that can't possibly change what the suggestion *would* be, even
    /// though they can still change whether it's *shown* (moving the
    /// cursor away from the end of the buffer hides it, exactly as before).
    fn update_suggestion(&mut self, history: &History) {
        if self.pos == self.buf.len() && !self.buf.is_empty() {
            if let Some((cached_gen, cached)) = &self.suggestion_cache
                && *cached_gen == self.buf_generation
            {
                self.suggestion = cached.clone();
                return;
            }
            let result = history.suggest(&self.buffer());
            self.suggestion_cache = Some((self.buf_generation, result.clone()));
            self.suggestion = result;
        } else {
            self.suggestion = None;
        }
    }
}

impl std::fmt::Display for LineEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.buffer())
    }
}

// ---------------------------------------------------------------------------
// Terminal I/O support (crossterm)
// ---------------------------------------------------------------------------

/// Result of processing a single key event.
enum KeyAction {
    Continue,
    Submit,
    Eof,
    Interrupt,
    FuzzySearch,
    TabComplete,
    ClearScreen,
}

impl LineEditor {
    /// Read a line of input from the terminal, handling cursor movement and
    /// editing keys.  Returns `Ok(Some(line))` on Enter, `Ok(None)` on
    /// Ctrl-D with an empty buffer (EOF), or `Ok(Some(""))` on Ctrl-C.
    #[allow(dead_code)] // Used by tests; production code uses read_line_with_completion
    pub fn read_line<T: Terminal>(
        &mut self,
        prompt: &str,
        upper_lines: &[String],
        history: &mut History,
        term: &mut T,
    ) -> io::Result<Option<String>> {
        self.clear();
        term.enable_raw_mode()?;
        let result = self.read_line_loop(prompt, upper_lines, history, term);
        let _ = term.disable_raw_mode();
        result
    }

    fn read_line_loop<T: Terminal>(
        &mut self,
        prompt: &str,
        upper_lines: &[String],
        history: &mut History,
        term: &mut T,
    ) -> io::Result<Option<String>> {
        let prompt_width = display_width(prompt);
        loop {
            term.flush()?;
            match term.read_event()? {
                Event::Key(key_event) => {
                    match self.handle_key(key_event, history) {
                        KeyAction::Submit => {
                            history.reset_cursor();
                            if self.prev_total_rows > 0 {
                                let buf_pos_width: usize = self.buf[..self.pos]
                                    .iter()
                                    .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
                                    .sum();
                                let (tw, _) = term.size().unwrap_or((80, 24));
                                let tw = tw as usize;
                                let cursor_row = if tw > 0 {
                                    (prompt_width + buf_pos_width) / tw
                                } else {
                                    0
                                };
                                if self.prev_total_rows > cursor_row {
                                    term.move_down((self.prev_total_rows - cursor_row) as u16)?;
                                }
                            }
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
                            if self.prev_total_rows > 0 {
                                let buf_pos_width: usize = self.buf[..self.pos]
                                    .iter()
                                    .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
                                    .sum();
                                let (tw, _) = term.size().unwrap_or((80, 24));
                                let tw = tw as usize;
                                let cursor_row = if tw > 0 {
                                    (prompt_width + buf_pos_width) / tw
                                } else {
                                    0
                                };
                                if self.prev_total_rows > cursor_row {
                                    term.move_down((self.prev_total_rows - cursor_row) as u16)?;
                                }
                            }
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
                                self.invalidate_width_cache();
                            }
                            term.enable_raw_mode()?;
                            term.move_to_column(0)?;
                            term.clear_current_line()?;
                            for line in upper_lines {
                                term.write_str(line)?;
                                term.write_str("\r\n")?;
                            }
                            term.write_str(prompt)?;
                            // The screen was repainted outside of redraw's own
                            // bookkeeping; force a full repaint next time so
                            // the diff-based partial repaint doesn't assume a
                            // stale on-screen state.
                            self.prev_render = None;
                        }
                        KeyAction::ClearScreen => {
                            term.clear_all()?;
                            for line in upper_lines {
                                term.write_str(line)?;
                                term.write_str("\r\n")?;
                            }
                            term.write_str(prompt)?;
                            self.prev_render = None;
                        }
                        KeyAction::TabComplete | KeyAction::Continue => {}
                    }
                    self.update_suggestion(history);
                    let (tw, _) = term.size().unwrap_or((80, 24));
                    self.redraw(term, prompt, prompt_width, &[], tw)?;
                }
                Event::Resize(_cols, _rows) => {
                    // Terminal dimensions changed, invalidating all cached
                    // row/column math from the previous render; force a full
                    // repaint.
                    self.prev_render = None;
                    let (tw, _) = term.size().unwrap_or((80, 24));
                    self.update_suggestion(history);
                    self.redraw(term, prompt, prompt_width, &[], tw)?;
                }
                _ => {}
            }
        }
    }

    /// Redraw the current buffer on screen, positioning the cursor correctly.
    /// Handles input that wraps past the terminal width.
    fn redraw<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        prompt_width: usize,
        spans: &[ColorSpan],
        term_width: u16,
    ) -> io::Result<()> {
        let tw = term_width as usize;
        let col = |n: usize| -> u16 { n.min(u16::MAX as usize) as u16 };

        // Precompute the width/row info the new-render needs regardless of
        // which repaint strategy is chosen below.
        let (buf_pos_width, buf_total_width) = self.buf_prefix_and_total_width();
        let suggestion_active = self.suggestion.is_some() && self.pos == self.buf.len();
        let suggestion_width: usize = if suggestion_active {
            self.suggestion
                .as_ref()
                .unwrap()
                .chars()
                .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
                .sum()
        } else {
            0
        };
        let content_width = prompt_width + buf_total_width + suggestion_width;
        let total_rows = if tw > 0 && content_width > 0 {
            (content_width.saturating_sub(1)) / tw
        } else {
            0
        };

        // Decide whether a partial repaint (from the first changed column)
        // is safe, or whether to fall back to the full clear+repaint.
        //
        // Partial repaint is restricted to the single-row, no-wrap case:
        // both the previous and the new render must fit on one row. This
        // sidesteps the multi-row cursor-positioning math (move_up/move_down
        // per wrapped row) entirely, which is where a partial-repaint bug
        // would be both easiest to introduce and hardest to notice. Content
        // that wraps keeps the previous (correctness-proven) full-repaint
        // behavior.
        //
        // The repaint start column is also capped by the first position
        // where the *style* differs from the previous render, not just
        // where the *character* differs: highlighting can retroactively
        // recolor already-typed, unchanged characters (e.g. typing the
        // closing quote of `echo 'hello` flips the whole `'hello` run from
        // an unclosed-quote Error span to a normal String span even though
        // none of those characters changed). Using only a character-level
        // diff would miss that recoloring.
        let partial_repaint_start = self
            .prev_render
            .as_ref()
            .and_then(|(prev_buf, prev_spans)| {
                if self.prev_total_rows != 0 || total_rows != 0 {
                    return None;
                }
                let char_diff = prev_buf
                    .iter()
                    .zip(self.buf.iter())
                    .position(|(a, b)| a != b)
                    .unwrap_or_else(|| prev_buf.len().min(self.buf.len()));
                let style_diff = style_diff_pos(prev_spans, spans, char_diff);
                Some(style_diff)
            });

        if let Some(start) = partial_repaint_start {
            // ---- Partial repaint: rewrite only from `start` onward ----
            let prefix_width: usize = self.buf[..start]
                .iter()
                .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
                .sum();
            term.move_to_column(col(prompt_width + prefix_width))?;
            term.clear_until_newline()?;

            let mut span_idx = 0;
            let mut current_style = HighlightStyle::Default;
            if !spans.is_empty() {
                // Seed span_idx/current_style as if we'd walked from 0, so
                // the first char written at `start` gets the right style
                // without re-walking (and re-emitting escapes for) the
                // unchanged prefix.
                current_style = style_at_advancing(spans, &mut span_idx, start);
                if current_style != HighlightStyle::Default {
                    apply_style(term, current_style)?;
                }
            }
            for (i, ch) in self.buf.iter().enumerate().skip(start) {
                if !spans.is_empty() {
                    let new_style = style_at_advancing(spans, &mut span_idx, i);
                    if new_style != current_style {
                        if current_style != HighlightStyle::Default {
                            term.reset_style()?;
                        }
                        apply_style(term, new_style)?;
                        current_style = new_style;
                    }
                }
                term.write_char(*ch)?;
            }
            if current_style != HighlightStyle::Default {
                term.reset_style()?;
            }

            if suggestion_active {
                term.set_dim(true)?;
                term.write_str(self.suggestion.as_deref().unwrap_or(""))?;
                term.set_dim(false)?;
            }
        } else {
            // ---- Full clear + repaint (original behavior) ----
            // Move cursor up to the prompt's last_line row (start of content)
            if self.prev_total_rows > 0 {
                term.move_up(self.prev_total_rows as u16)?;
            }
            term.move_to_column(0)?;

            // Clear all rows from previous render
            for i in 0..=self.prev_total_rows {
                if i > 0 {
                    term.move_down(1)?;
                }
                term.clear_current_line()?;
            }
            // Move back up to start
            if self.prev_total_rows > 0 {
                term.move_up(self.prev_total_rows as u16)?;
            }

            // Repaint the prompt
            term.move_to_column(0)?;
            term.write_str(prompt)?;

            // Write the buffer with or without highlighting
            if spans.is_empty() {
                term.write_str(&self.buffer())?;
            } else {
                // Spans are produced in left-to-right scan order (sorted,
                // non-overlapping — see HighlightScanner::scan_from), so a
                // single advancing cursor into `spans` (see
                // `style_at_advancing`) replaces the previous per-char
                // `spans.iter().find(...)` linear search, turning
                // classification from O(n*spans) into O(n + spans).
                let mut current_style = HighlightStyle::Default;
                let mut span_idx = 0;
                for (i, ch) in self.buf.iter().enumerate() {
                    let new_style = style_at_advancing(spans, &mut span_idx, i);
                    if new_style != current_style {
                        if current_style != HighlightStyle::Default {
                            term.reset_style()?;
                        }
                        apply_style(term, new_style)?;
                        current_style = new_style;
                    }
                    term.write_char(*ch)?;
                }
                if current_style != HighlightStyle::Default {
                    term.reset_style()?;
                }
            }

            // Draw suggestion
            if suggestion_active {
                term.set_dim(true)?;
                term.write_str(self.suggestion.as_deref().unwrap_or(""))?;
                term.set_dim(false)?;
            }
        }

        self.prev_total_rows = total_rows;
        self.prev_render = Some((self.buf.clone(), spans.to_vec()));

        // Position cursor at self.pos
        let cursor_total = prompt_width + buf_pos_width;
        let cursor_row = if tw > 0 { cursor_total / tw } else { 0 };
        let cursor_col = if tw > 0 {
            cursor_total % tw
        } else {
            cursor_total
        };

        // Move from end-of-content row to cursor row
        let end_row = total_rows;
        if end_row > cursor_row {
            term.move_up((end_row - cursor_row) as u16)?;
        }
        term.move_to_column(col(cursor_col))?;
        term.flush()?;
        Ok(())
    }

    /// Map a single key event to a [`KeyAction`], mutating the buffer as needed.
    fn handle_key(&mut self, key: KeyEvent, history: &mut History) -> KeyAction {
        let state = BufferState {
            is_empty: self.is_empty(),
            at_end: self.pos == self.buf.len(),
            has_suggestion: self.suggestion.is_some(),
            last_action: self.last_action,
        };

        let (action, count) = self.keymap.resolve(key, &state);

        if !matches!(action, EditAction::TabComplete) {
            self.tab_count = 0;
        }

        // Undo snapshot management
        // - Insert group boundary: when transitioning from insert to non-insert, save once
        // - Destructive ops: always save pre-op state (once)
        // - Insert start: save when first insert after non-insert
        if self.last_was_insert && !matches!(action, EditAction::InsertChar(_)) {
            // Finalize the insert group — save current state as group boundary
            self.undo.save(&self.buf, self.pos);
        }
        match action {
            EditAction::InsertChar(_) => {
                if !self.last_was_insert {
                    self.undo.save(&self.buf, self.pos);
                }
            }
            EditAction::KillToEnd
            | EditAction::KillToStart
            | EditAction::KillBackwardWord
            | EditAction::KillForwardWord
            | EditAction::DeleteBackward
            | EditAction::DeleteForward
            | EditAction::Yank
            | EditAction::YankPop
            | EditAction::TransposeChars
            | EditAction::TransposeWords
            | EditAction::UpcaseWord
            | EditAction::DowncaseWord
            | EditAction::CapitalizeWord => {
                if !self.last_was_insert {
                    // Not transitioning from insert — save pre-op state directly
                    self.undo.save(&self.buf, self.pos);
                }
                // If last_was_insert, boundary save above already captured the state
            }
            _ => {}
        }

        // Determine if consecutive kill for append
        let is_consecutive_kill = action.is_kill() && self.last_action.is_kill();

        // Execute action
        let key_action = self.execute_action(action, count, history, is_consecutive_kill);

        // Update tracking state
        self.last_was_insert = matches!(action, EditAction::InsertChar(ch) if ch != ' ');
        if !matches!(action, EditAction::Yank | EditAction::YankPop) {
            self.yank_state = None;
        }
        self.last_action = action;

        key_action
    }

    fn execute_action(
        &mut self,
        action: EditAction,
        count: u32,
        history: &mut History,
        consecutive_kill: bool,
    ) -> KeyAction {
        match action {
            EditAction::InsertChar(ch) => {
                for _ in 0..count {
                    self.insert_char(ch);
                }
                KeyAction::Continue
            }
            EditAction::MoveBackward => {
                for _ in 0..count {
                    self.move_cursor_left();
                }
                KeyAction::Continue
            }
            EditAction::MoveForward => {
                for _ in 0..count {
                    self.move_cursor_right();
                }
                KeyAction::Continue
            }
            EditAction::MoveToStart => {
                self.move_to_start();
                KeyAction::Continue
            }
            EditAction::MoveToEnd => {
                self.move_to_end();
                KeyAction::Continue
            }
            EditAction::MoveBackwardWord => {
                for _ in 0..count {
                    self.move_backward_word();
                }
                KeyAction::Continue
            }
            EditAction::MoveForwardWord => {
                for _ in 0..count {
                    self.move_forward_word();
                }
                KeyAction::Continue
            }
            EditAction::DeleteBackward => {
                for _ in 0..count {
                    self.backspace();
                }
                KeyAction::Continue
            }
            EditAction::DeleteForward => {
                for _ in 0..count {
                    self.delete();
                }
                KeyAction::Continue
            }
            EditAction::KillToEnd => {
                let killed = self.kill_to_end();
                self.kill_ring.kill(&killed, consecutive_kill);
                KeyAction::Continue
            }
            EditAction::KillToStart => {
                let killed = self.kill_to_start();
                self.kill_ring.prepend(&killed, consecutive_kill);
                KeyAction::Continue
            }
            EditAction::KillBackwardWord => {
                for _ in 0..count {
                    let killed = self.kill_backward_word();
                    self.kill_ring.prepend(&killed, consecutive_kill);
                }
                KeyAction::Continue
            }
            EditAction::KillForwardWord => {
                for _ in 0..count {
                    let killed = self.kill_forward_word();
                    self.kill_ring.kill(&killed, consecutive_kill);
                }
                KeyAction::Continue
            }
            EditAction::Yank => {
                if let Some(text) = self.kill_ring.yank().map(|s| s.to_string()) {
                    let (start, len) = self.insert_str(&text);
                    self.yank_state = Some(YankState { start, len });
                }
                KeyAction::Continue
            }
            EditAction::YankPop => {
                if let Some(ys) = self.yank_state.clone() {
                    self.remove_range(ys.start, ys.len);
                    if let Some(text) = self.kill_ring.yank_pop().map(|s| s.to_string()) {
                        let (start, len) = self.insert_str(&text);
                        self.yank_state = Some(YankState { start, len });
                    }
                }
                KeyAction::Continue
            }
            EditAction::TransposeChars => {
                for _ in 0..count {
                    self.transpose_chars();
                }
                KeyAction::Continue
            }
            EditAction::TransposeWords => {
                for _ in 0..count {
                    self.transpose_words();
                }
                KeyAction::Continue
            }
            EditAction::UpcaseWord => {
                for _ in 0..count {
                    self.upcase_word();
                }
                KeyAction::Continue
            }
            EditAction::DowncaseWord => {
                for _ in 0..count {
                    self.downcase_word();
                }
                KeyAction::Continue
            }
            EditAction::CapitalizeWord => {
                for _ in 0..count {
                    self.capitalize_word();
                }
                KeyAction::Continue
            }
            EditAction::Undo => {
                for _ in 0..count {
                    if let Some((buf, pos)) = self.undo.undo() {
                        self.buf = buf;
                        self.pos = pos;
                        self.invalidate_width_cache();
                    }
                }
                KeyAction::Continue
            }
            EditAction::ClearScreen => KeyAction::ClearScreen,
            EditAction::Cancel => KeyAction::Continue,
            EditAction::AcceptSuggestion => {
                self.accept_full_suggestion();
                KeyAction::Continue
            }
            EditAction::AcceptWordSuggestion => {
                self.accept_word_suggestion();
                KeyAction::Continue
            }
            EditAction::SetNumericArg(_) => KeyAction::Continue,
            EditAction::Submit => KeyAction::Submit,
            EditAction::Eof => KeyAction::Eof,
            EditAction::Interrupt => KeyAction::Interrupt,
            EditAction::FuzzySearch => KeyAction::FuzzySearch,
            EditAction::TabComplete => {
                self.tab_count += 1;
                KeyAction::TabComplete
            }
            EditAction::HistoryPrev => {
                for _ in 0..count {
                    if let Some(line) = history.navigate_up(&self.buffer()) {
                        self.buf = line.chars().collect();
                        self.pos = self.buf.len();
                        self.invalidate_width_cache();
                    }
                }
                self.suggestion = None;
                KeyAction::Continue
            }
            EditAction::HistoryNext => {
                for _ in 0..count {
                    if let Some(line) = history.navigate_down() {
                        self.buf = line.chars().collect();
                        self.pos = self.buf.len();
                        self.invalidate_width_cache();
                    }
                }
                self.suggestion = None;
                KeyAction::Continue
            }
            EditAction::Noop => KeyAction::Continue,
        }
    }

    // ── Tab completion support ─────────────────────────────────────────

    /// Read a line of input with Tab completion support.
    ///
    /// Behaves identically to [`read_line`] but also handles Tab key events
    /// by invoking the completion engine.
    #[allow(clippy::too_many_arguments)]
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
        let result = self.read_line_loop_with_completion(
            prompt,
            upper_lines,
            history,
            term,
            ctx,
            cmd_ctx,
            scanner,
            checker_env,
            accumulated,
        );
        let _ = term.disable_raw_mode();
        result
    }

    #[allow(clippy::too_many_arguments)]
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
        let prompt_width = display_width(prompt);
        loop {
            term.flush()?;
            match term.read_event()? {
                Event::Key(key_event) => {
                    match self.handle_key(key_event, history) {
                        KeyAction::Submit => {
                            history.reset_cursor();
                            term.reset_style()?;
                            if self.prev_total_rows > 0 {
                                let buf_pos_width: usize = self.buf[..self.pos]
                                    .iter()
                                    .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
                                    .sum();
                                let (tw, _) = term.size().unwrap_or((80, 24));
                                let tw = tw as usize;
                                let cursor_row = if tw > 0 {
                                    (prompt_width + buf_pos_width) / tw
                                } else {
                                    0
                                };
                                if self.prev_total_rows > cursor_row {
                                    term.move_down((self.prev_total_rows - cursor_row) as u16)?;
                                }
                            }
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
                            term.reset_style()?;
                            if self.prev_total_rows > 0 {
                                let buf_pos_width: usize = self.buf[..self.pos]
                                    .iter()
                                    .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
                                    .sum();
                                let (tw, _) = term.size().unwrap_or((80, 24));
                                let tw = tw as usize;
                                let cursor_row = if tw > 0 {
                                    (prompt_width + buf_pos_width) / tw
                                } else {
                                    0
                                };
                                if self.prev_total_rows > cursor_row {
                                    term.move_down((self.prev_total_rows - cursor_row) as u16)?;
                                }
                            }
                            term.move_to_column(0)?;
                            term.write_str("\r\n")?;
                            term.flush()?;
                            self.clear();
                            return Ok(Some(String::new()));
                        }
                        KeyAction::FuzzySearch => {
                            self.suggestion = None;
                            term.reset_style()?;
                            term.disable_raw_mode()?;
                            if let Ok(Some(line)) = FuzzySearchUI::run(history, term) {
                                self.buf = line.chars().collect();
                                self.pos = self.buf.len();
                                self.invalidate_width_cache();
                            }
                            term.enable_raw_mode()?;
                            term.move_to_column(0)?;
                            term.clear_current_line()?;
                            for line in upper_lines {
                                term.write_str(line)?;
                                term.write_str("\r\n")?;
                            }
                            term.write_str(prompt)?;
                            self.prev_render = None;
                        }
                        KeyAction::TabComplete => {
                            term.reset_style()?;
                            self.handle_tab_complete(term, prompt, upper_lines, ctx, cmd_ctx)?;
                        }
                        KeyAction::ClearScreen => {
                            term.clear_all()?;
                            for line in upper_lines {
                                term.write_str(line)?;
                                term.write_str("\r\n")?;
                            }
                            term.write_str(prompt)?;
                            self.prev_render = None;
                        }
                        KeyAction::Continue => {}
                    }
                    self.update_suggestion(history);
                    let spans = scanner.scan(accumulated, &self.buf, checker_env);
                    let (tw, _) = term.size().unwrap_or((80, 24));
                    self.redraw(term, prompt, prompt_width, &spans, tw)?;
                }
                Event::Resize(_cols, _rows) => {
                    self.prev_render = None;
                    let (tw, _) = term.size().unwrap_or((80, 24));
                    self.update_suggestion(history);
                    let spans = scanner.scan(accumulated, &self.buf, checker_env);
                    self.redraw(term, prompt, prompt_width, &spans, tw)?;
                }
                _ => {}
            }
        }
    }

    fn handle_tab_complete<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        upper_lines: &[String],
        ctx: &CompletionContext,
        cmd_ctx: &mut CommandCompletionContext<'_>,
    ) -> io::Result<()> {
        let (word_start, word) = {
            let buf = self.buffer();
            let (ws, w) = extract_completion_word(&buf, self.pos);
            (ws, w.to_owned())
        };
        let is_cmd_pos = {
            let buf = self.buffer();
            is_command_position(&buf, word_start)
        };

        let (candidates, common_prefix, dir_prefix) = if is_cmd_pos && !word.contains('/') {
            // Command name completion
            let (cands, common) = cmd_ctx.completer.complete_common_prefix(
                &word,
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
                let current_word_len = self.buffer()[word_start..self.pos].len();
                let new_word = format!("{}{}", dir_prefix, common_prefix);
                if new_word.len() > current_word_len {
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
            self.prev_render = None;
        }

        Ok(())
    }

    /// Replace the word starting at byte offset `word_start` with `replacement`.
    fn replace_word(&mut self, word_start: usize, replacement: &str) {
        // Convert byte offset to char index
        let char_start = self.buffer()[..word_start].chars().count();
        // Drain chars from char_start to current pos
        let drain_end = self.pos;
        self.buf.drain(char_start..drain_end);
        // Insert replacement chars at char_start
        let rep_chars: Vec<char> = replacement.chars().collect();
        let rep_len = rep_chars.len();
        for (i, ch) in rep_chars.into_iter().enumerate() {
            self.buf.insert(char_start + i, ch);
        }
        self.pos = char_start + rep_len;
        self.invalidate_width_cache();
    }
}

#[cfg(test)]
mod redraw_helper_tests {
    use super::*;

    /// Reference implementation matching the pre-optimization behavior
    /// (`spans.iter().find(...)` per char): O(n*spans) but trivially
    /// correct, used as an oracle for `style_at_advancing`.
    fn naive_style_at(spans: &[ColorSpan], i: usize) -> HighlightStyle {
        spans
            .iter()
            .find(|sp| sp.start <= i && i < sp.end)
            .map(|sp| sp.style)
            .unwrap_or(HighlightStyle::Default)
    }

    fn span(start: usize, end: usize, style: HighlightStyle) -> ColorSpan {
        ColorSpan { start, end, style }
    }

    /// Assert that walking `len` chars with the advancing cursor produces
    /// the same style sequence as the naive per-char `find`.
    fn assert_equivalent(spans: &[ColorSpan], len: usize) {
        let mut span_idx = 0;
        for i in 0..len {
            let got = style_at_advancing(spans, &mut span_idx, i);
            let want = naive_style_at(spans, i);
            assert_eq!(got, want, "mismatch at i={i} for spans={spans:?}");
        }
    }

    #[test]
    fn empty_spans_all_default() {
        assert_equivalent(&[], 10);
    }

    #[test]
    fn single_span_covering_prefix() {
        let spans = vec![span(0, 4, HighlightStyle::CommandValid)];
        assert_equivalent(&spans, 10);
    }

    #[test]
    fn single_span_in_middle_with_gaps() {
        let spans = vec![span(3, 6, HighlightStyle::Variable)];
        assert_equivalent(&spans, 10);
    }

    #[test]
    fn adjacent_spans_no_gap() {
        let spans = vec![
            span(0, 4, HighlightStyle::CommandValid),
            span(4, 9, HighlightStyle::Default),
            span(9, 13, HighlightStyle::Operator),
        ];
        assert_equivalent(&spans, 15);
    }

    #[test]
    fn spans_with_gaps_between() {
        let spans = vec![
            span(0, 2, HighlightStyle::Keyword),
            span(5, 7, HighlightStyle::String),
            span(10, 12, HighlightStyle::Error),
        ];
        assert_equivalent(&spans, 15);
    }

    #[test]
    fn span_at_very_end() {
        let spans = vec![span(8, 10, HighlightStyle::Comment)];
        assert_equivalent(&spans, 10);
    }

    #[test]
    fn many_small_spans_pseudo_random_layout() {
        // Deterministic pseudo-random-ish layout: alternating short spans
        // and gaps of varying length, covering a range of styles.
        let styles = [
            HighlightStyle::CommandValid,
            HighlightStyle::CommandInvalid,
            HighlightStyle::Keyword,
            HighlightStyle::Operator,
            HighlightStyle::Redirect,
            HighlightStyle::String,
            HighlightStyle::DoubleString,
            HighlightStyle::Variable,
            HighlightStyle::CommandSub,
            HighlightStyle::ArithSub,
            HighlightStyle::Comment,
            HighlightStyle::Error,
            HighlightStyle::Assignment,
            HighlightStyle::Tilde,
        ];
        let mut spans = Vec::new();
        let mut pos = 0usize;
        for (idx, style) in styles.iter().enumerate() {
            let span_len = (idx % 3) + 1; // 1..=3
            let gap = idx % 4; // 0..=3
            pos += gap;
            spans.push(span(pos, pos + span_len, *style));
            pos += span_len;
        }
        assert_equivalent(&spans, pos + 10);
    }

    #[test]
    fn single_char_spans_tightly_packed() {
        let spans: Vec<ColorSpan> = (0..20)
            .map(|i| span(i, i + 1, HighlightStyle::Default))
            .collect();
        assert_equivalent(&spans, 25);
    }
}
