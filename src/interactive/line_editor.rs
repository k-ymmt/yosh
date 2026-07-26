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
use super::spec_completion::{self, SpecStore};
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
    /// Physical row (0-based, relative to the first content row) the cursor
    /// was left on by the previous `redraw`. Both repaint strategies move
    /// relative to this row, so it must be reset together with
    /// `prev_total_rows` whenever the screen is repainted outside `redraw`
    /// (see `invalidate_render_state`).
    prev_cursor_row: usize,
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
    /// Continuation prompt (expanded PS2 last line) resolved lazily on the
    /// first multiline render of the current `read_line` session, so a PS2
    /// containing command substitution is only executed when a continuation
    /// line is actually displayed. Reset by `clear`.
    cont_prompt_cache: Option<String>,
    /// Readline-style "preferred column" for consecutive vertical moves:
    /// captured on the first Up/Down, retained while the cursor passes
    /// through shorter lines, and cleared by any other edit action or
    /// buffer mutation.
    preferred_col: Option<usize>,
    /// Whether the previous `redraw` went through the multiline renderer.
    /// The single-line partial-repaint diff must not run against a
    /// multiline on-screen layout, and buffer content alone can't tell
    /// (an oversized single-logical-line buffer also renders multiline).
    prev_render_multiline: bool,
    /// First visible physical row (into the full row layout) of the
    /// multiline renderer's viewport. 0 while everything fits on screen;
    /// scrolls minimally to keep the cursor row visible when the render
    /// is taller than the terminal.
    viewport_top: usize,
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
            prev_cursor_row: 0,
            cached_total_width: None,
            buf_generation: 0,
            suggestion_cache: None,
            prev_render: None,
            cont_prompt_cache: None,
            preferred_col: None,
            prev_render_multiline: false,
            viewport_top: 0,
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
        self.cont_prompt_cache = None;
        self.invalidate_width_cache();
        self.invalidate_render_state();
    }

    /// Forget everything `redraw` knows about the on-screen state. Must be
    /// called whenever the screen is repainted outside `redraw`'s own
    /// bookkeeping (clear-screen, selector UIs, prompt reprints): those
    /// paths leave the cursor on the freshly printed prompt row, so the
    /// row counters reset to zero along with the render cache.
    fn invalidate_render_state(&mut self) {
        self.prev_render = None;
        self.prev_total_rows = 0;
        self.prev_cursor_row = 0;
        self.prev_render_multiline = false;
        self.viewport_top = 0;
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

    /// Return the char index of the start of the logical line containing
    /// `pos` (the position just after the previous `'\n'`, or 0).
    fn line_start(&self, pos: usize) -> usize {
        self.buf[..pos]
            .iter()
            .rposition(|&c| c == '\n')
            .map(|i| i + 1)
            .unwrap_or(0)
    }

    /// Return the char index of the end of the logical line containing
    /// `pos` (the position of the next `'\n'`, or `buf.len()`).
    fn line_end(&self, pos: usize) -> usize {
        self.buf[pos..]
            .iter()
            .position(|&c| c == '\n')
            .map(|i| pos + i)
            .unwrap_or(self.buf.len())
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
        let cur_start = self.line_start(self.pos);
        if cur_start == 0 {
            return;
        }
        let visual: usize = self.buf[cur_start..self.pos]
            .iter()
            .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
            .sum();
        let target = *self.preferred_col.get_or_insert(visual);
        let prev_end = cur_start - 1; // index of the '\n'
        let prev_start = self.line_start(prev_end);
        self.pos = Self::pos_at_column(&self.buf, prev_start, prev_end, target);
    }

    /// Move the cursor to the next logical line, preserving the display
    /// column as closely as possible. Does nothing on the last line.
    /// Uses the same preferred-column stickiness as [`move_cursor_up`].
    pub fn move_cursor_down(&mut self) {
        let cur_end = self.line_end(self.pos);
        if cur_end == self.buf.len() {
            return;
        }
        let cur_start = self.line_start(self.pos);
        let visual: usize = self.buf[cur_start..self.pos]
            .iter()
            .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
            .sum();
        let target = *self.preferred_col.get_or_insert(visual);
        let next_start = cur_end + 1;
        let next_end = self.line_end(next_start);
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
        self.pos = self.line_start(self.pos);
    }

    /// Move the cursor to the end of the current logical line.
    /// (For a single-line buffer this is the end of the buffer, as before.)
    pub fn move_to_end(&mut self) {
        self.pos = self.line_end(self.pos);
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
        let end = self.line_end(self.pos);
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
        let start = self.line_start(self.pos);
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
    /// A "word" is any leading delimiters (spaces/newlines) + characters up
    /// to the next delimiter. Newlines delimit so one accept never spans two
    /// logical lines: on a multiline suggestion the first accept stops
    /// before the `'\n'`, and the next accept takes the newline together
    /// with the following word — crossing the line boundary only on a
    /// deliberate further press.
    fn accept_word_suggestion(&mut self) {
        let is_delim = |c: char| c == ' ' || c == '\n';
        if let Some(suggestion) = self.suggestion.take() {
            let chars: Vec<char> = suggestion.chars().collect();
            let mut i = 0;
            // Skip leading delimiters
            while i < chars.len() && is_delim(chars[i]) {
                i += 1;
            }
            // Take word characters
            while i < chars.len() && !is_delim(chars[i]) {
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
            // Cache key is buf_generation alone — it does not account for
            // `history` identity/content. This is safe only because the
            // `History` instance is fixed for the duration of a single
            // `read_line` call (the only caller of `update_suggestion`);
            // if that ever changes (e.g. history mutated or swapped
            // mid-call), this cache would need to invalidate on that too.
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

    /// If an un-accepted autosuggestion is on screen, repaint without it so
    /// it doesn't linger in scrollback after the read ends — a multiline
    /// suggestion would otherwise leave dim phantom continuation lines that
    /// were never input above the command's output.
    fn clear_lingering_suggestion<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        prompt_width: usize,
        cont_prompt: &str,
        spans: &[ColorSpan],
    ) -> io::Result<()> {
        if self.suggestion.take().is_some() {
            let (tw, th) = term.size().unwrap_or((80, 24));
            self.redraw(term, prompt, prompt_width, cont_prompt, spans, tw, th)?;
        }
        Ok(())
    }

    /// Move the cursor down past the last rendered row so subsequent
    /// terminal output starts below the full (possibly wrapped) input.
    /// No-op when nothing has been rendered yet.
    fn move_below_render<T: Terminal>(&self, term: &mut T, _prompt_width: usize) -> io::Result<()> {
        if self.prev_total_rows > self.prev_cursor_row {
            term.move_down((self.prev_total_rows - self.prev_cursor_row) as u16)?;
        }
        Ok(())
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
                            self.clear_lingering_suggestion(term, prompt, prompt_width, "> ", &[])?;
                            self.move_below_render(term, prompt_width)?;
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
                            self.clear_lingering_suggestion(term, prompt, prompt_width, "> ", &[])?;
                            self.move_below_render(term, prompt_width)?;
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
                            self.invalidate_render_state();
                        }
                        KeyAction::ClearScreen => {
                            term.clear_all()?;
                            for line in upper_lines {
                                term.write_str(line)?;
                                term.write_str("\r\n")?;
                            }
                            term.write_str(prompt)?;
                            self.invalidate_render_state();
                        }
                        KeyAction::TabComplete | KeyAction::Continue => {}
                    }
                    self.update_suggestion(history);
                    let (tw, th) = term.size().unwrap_or((80, 24));
                    self.redraw(term, prompt, prompt_width, "> ", &[], tw, th)?;
                }
                Event::Resize(_cols, _rows) => {
                    // Terminal dimensions changed, invalidating all cached
                    // row/column math from the previous render; force a full
                    // repaint.
                    self.invalidate_render_state();
                    let (tw, th) = term.size().unwrap_or((80, 24));
                    self.update_suggestion(history);
                    self.redraw(term, prompt, prompt_width, "> ", &[], tw, th)?;
                }
                _ => {}
            }
        }
    }

    /// Redraw the current buffer on screen, positioning the cursor correctly.
    /// Handles input that wraps past the terminal width. Buffers containing
    /// literal newlines are rendered as multiple logical lines, each
    /// continuation line prefixed with `cont_prompt`.
    #[allow(clippy::too_many_arguments)]
    fn redraw<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        prompt_width: usize,
        cont_prompt: &str,
        spans: &[ColorSpan],
        term_width: u16,
        term_height: u16,
    ) -> io::Result<()> {
        let tw = term_width as usize;
        let th = (term_height as usize).max(1);
        let col = |n: usize| -> u16 { n.min(u16::MAX as usize) as u16 };

        // Precompute the width/row info both the dispatch decision and the
        // single-line renderer need.
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

        // Multiline buffers get their own renderer; the soft-wrap row math
        // below assumes a single logical line. The same renderer also
        // handles a suggestion whose remainder spans lines, and — because
        // it is the only path with viewport clamping — any buffer whose
        // soft-wrapped render would be taller than the terminal.
        let suggestion_multiline =
            suggestion_active && self.suggestion.as_deref().is_some_and(|s| s.contains('\n'));
        // Wide (width-2) chars cannot straddle a row boundary, so a real
        // terminal can wrap up to one column early per row and pure width
        // division underestimates physical rows for CJK-heavy content. The
        // height dispatch therefore uses the pessimistic bound (one wasted
        // column per row) so a physically taller-than-screen buffer cannot
        // stay on the auto-wrap single-line path; `total_rows` itself keeps
        // the exact-division model the single-line painter is built on.
        let worst_case_rows = if tw > 1 {
            content_width.div_ceil(tw - 1)
        } else {
            content_width
        };
        if self.buf.contains(&'\n') || suggestion_multiline || (tw > 0 && worst_case_rows > th) {
            return self.redraw_multiline(term, prompt, prompt_width, cont_prompt, spans, tw, th);
        }
        // If the *previous* render went through the multiline renderer, the
        // diff-based partial repaint below would misinterpret its layout;
        // the full clear + repaint path handles it fine (clearing only uses
        // row counts).
        let prev_was_multiline = self.prev_render_multiline;

        // Decide whether a partial repaint (from the first changed cell)
        // is safe, or whether to fall back to the full clear+repaint.
        //
        // The repaint start column is capped by the first position where
        // the *style* differs from the previous render, not just where the
        // *character* differs: highlighting can retroactively recolor
        // already-typed, unchanged characters (e.g. typing the closing
        // quote of `echo 'hello` flips the whole `'hello` run from an
        // unclosed-quote Error span to a normal String span even though
        // none of those characters changed). Using only a character-level
        // diff would miss that recoloring.
        //
        // The multi-row (wrapped) case positions by physical rows: the
        // cursor sits on `prev_cursor_row`, the first changed cell is at
        // `prefix_width / tw` (always within the previously rendered rows
        // because the unchanged prefix has identical layout in both
        // renders), and writing relies on terminal auto-wrap exactly like
        // the full-repaint path.
        let mut partial_repaint_start =
            self.prev_render
                .as_ref()
                .and_then(|(prev_buf, prev_spans)| {
                    if tw == 0 || prev_was_multiline {
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

        // A diff that starts exactly on a wrap boundary would have to write
        // into the terminal's deferred-wrap cell (or onto a physical row
        // that has not been created yet); fall back to the full repaint for
        // that column rather than modeling deferred-wrap state.
        if let Some(start) = partial_repaint_start {
            let w = prompt_width
                + self.buf[..start]
                    .iter()
                    .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
                    .sum::<usize>();
            if tw > 0 && w > 0 && w % tw == 0 {
                partial_repaint_start = None;
            }
        }

        if let Some(start) = partial_repaint_start {
            // ---- Partial repaint: rewrite only from `start` onward ----
            let prefix_width: usize = prompt_width
                + self.buf[..start]
                    .iter()
                    .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
                    .sum::<usize>();
            let diff_row = prefix_width / tw;
            let diff_col = prefix_width % tw;

            // Move from the cursor's row to the first changed row.
            if self.prev_cursor_row > diff_row {
                term.move_up((self.prev_cursor_row - diff_row) as u16)?;
            } else if diff_row > self.prev_cursor_row {
                term.move_down((diff_row - self.prev_cursor_row) as u16)?;
            }
            term.move_to_column(col(diff_col))?;
            term.clear_until_newline()?;

            // Clear any previously rendered rows below the changed row, so
            // shrinking content doesn't leave stale wrapped tails behind.
            if self.prev_total_rows > diff_row {
                for _ in diff_row..self.prev_total_rows {
                    term.move_down(1)?;
                    term.clear_current_line()?;
                }
                term.move_up((self.prev_total_rows - diff_row) as u16)?;
                term.move_to_column(col(diff_col))?;
            }

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
            // Move cursor up to the first content row. The cursor sits on
            // `prev_cursor_row` (not necessarily the last rendered row —
            // e.g. after moving left across a wrap boundary).
            if self.prev_cursor_row > 0 {
                term.move_up(self.prev_cursor_row as u16)?;
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
        self.prev_render_multiline = false;
        self.viewport_top = 0;

        // Position cursor at self.pos
        let cursor_total = prompt_width + buf_pos_width;
        let cursor_row = cursor_total.checked_div(tw).unwrap_or(0);
        let cursor_col = cursor_total.checked_rem(tw).unwrap_or(cursor_total);

        // Move from end-of-content row to cursor row
        let end_row = total_rows;
        if end_row > cursor_row {
            term.move_up((end_row - cursor_row) as u16)?;
        }
        term.move_to_column(col(cursor_col))?;
        // The physical row we just left the cursor on: `cursor_row`, except
        // when the cursor logically sits one past a content-final wrap
        // boundary — we never move down past the last rendered row, so the
        // physical row is clamped to it.
        self.prev_cursor_row = cursor_row.min(total_rows);
        term.flush()?;
        Ok(())
    }

    /// Render a buffer as logical lines: each soft-wraps within the terminal
    /// width, and every continuation line is prefixed with `cont_prompt`. An
    /// autosuggestion remainder is rendered dim after the buffer, its own
    /// newlines starting further continuation lines. Always a full clear +
    /// repaint — the partial-repaint diff only models single-line layouts.
    ///
    /// Unlike the single-line renderer this path does not rely on terminal
    /// auto-wrap: the layout is packed into explicit physical rows first,
    /// and only a viewport of at most `th` rows — always containing the
    /// cursor — is painted. That keeps every relative cursor movement within
    /// the screen, so renders taller than the terminal cannot corrupt the
    /// display (`move_up` past the top row would otherwise clamp and
    /// misalign all subsequent bookkeeping).
    #[allow(clippy::too_many_arguments)]
    fn redraw_multiline<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        prompt_width: usize,
        cont_prompt: &str,
        spans: &[ColorSpan],
        tw: usize,
        th: usize,
    ) -> io::Result<()> {
        let col = |n: usize| -> u16 { n.min(u16::MAX as usize) as u16 };
        let cont_width = display_width(cont_prompt);
        // Column budget per row; tw == 0 means "unknown width" — treat as
        // unbounded so each logical line stays on one row.
        let tw_eff = if tw == 0 { usize::MAX } else { tw };

        /// What a rendered cell's char is styled as.
        #[derive(Clone, Copy, PartialEq)]
        enum Cell {
            /// Buffer char at this index — styled by the highlight spans.
            Buf(usize),
            /// Autosuggestion char — rendered dim, never highlighted.
            Sugg,
        }

        // ---- Build logical display lines (buffer lines + suggestion
        // continuation lines), each a list of (char, cell kind) ----
        let mut dlines: Vec<Vec<(char, Cell)>> = vec![Vec::new()];
        for (i, &c) in self.buf.iter().enumerate() {
            if c == '\n' {
                dlines.push(Vec::new());
            } else {
                dlines.last_mut().unwrap().push((c, Cell::Buf(i)));
            }
        }
        // The cursor's display line/offset must be computed against buffer
        // lines only, before suggestion cells are appended.
        let cursor_line = self.cursor_line_index();
        let cursor_ofs = self.pos - self.line_start(self.pos);

        let suggestion_active = self.suggestion.is_some() && self.pos == self.buf.len();
        if suggestion_active {
            let sugg = self.suggestion.clone().unwrap_or_default();
            for (si, seg) in sugg.split('\n').enumerate() {
                if si > 0 {
                    dlines.push(Vec::new());
                }
                dlines
                    .last_mut()
                    .unwrap()
                    .extend(seg.chars().map(|c| (c, Cell::Sugg)));
            }
        }

        // ---- Pack display lines into physical rows ----
        // `first` marks the row where its logical line's prefix write
        // begins. A prefix wider than the terminal auto-wraps when painted
        // (it is written atomically with `write_str`, since it may contain
        // ANSI escapes that must not be split); `in_prefix` marks the extra
        // physical rows that auto-wrap produces — painting must not emit a
        // row break to enter them. `start_col` is where a row's cells begin
        // (non-zero only on the last prefix-flow row).
        struct Row {
            line: usize,
            first: bool,
            in_prefix: bool,
            start_col: usize,
            cells: Vec<(char, Cell)>,
        }
        let mut rows: Vec<Row> = Vec::new();
        // (row index, column) the cursor lands on, in absolute row terms.
        let mut cursor_at: Option<(usize, usize)> = None;
        for (li, cells) in dlines.iter().enumerate() {
            let prefix_w = if li == 0 { prompt_width } else { cont_width };
            // Physical rows the prefix's characters occupy beyond the
            // first. `(prefix_w - 1) / tw` (not `prefix_w / tw`) because a
            // prefix that exactly fills its last row leaves the cursor in
            // the terminal's deferred-wrap state on that row rather than
            // opening a new one.
            let extra = if tw > 0 && prefix_w > 0 {
                (prefix_w - 1) / tw
            } else {
                0
            };
            let mut cur;
            let mut cur_col;
            if extra == 0 {
                cur_col = prefix_w;
                cur = Row {
                    line: li,
                    first: true,
                    in_prefix: false,
                    start_col: cur_col,
                    cells: Vec::new(),
                };
            } else {
                rows.push(Row {
                    line: li,
                    first: true,
                    in_prefix: false,
                    start_col: 0,
                    cells: Vec::new(),
                });
                for _ in 1..extra {
                    rows.push(Row {
                        line: li,
                        first: false,
                        in_prefix: true,
                        start_col: 0,
                        cells: Vec::new(),
                    });
                }
                cur_col = prefix_w - extra * tw;
                cur = Row {
                    line: li,
                    first: false,
                    in_prefix: true,
                    start_col: cur_col,
                    cells: Vec::new(),
                };
            }
            for (ci, &(ch, kind)) in cells.iter().enumerate() {
                let w = UnicodeWidthChar::width(ch).unwrap_or(0);
                // Wrap before a cell that no longer fits — but always place
                // at least one cell on a continuation row so packing makes
                // progress even for degenerate widths.
                if cur_col + w > tw_eff && (cur.first || cur.in_prefix || !cur.cells.is_empty()) {
                    let done = std::mem::replace(
                        &mut cur,
                        Row {
                            line: li,
                            first: false,
                            in_prefix: false,
                            start_col: 0,
                            cells: Vec::new(),
                        },
                    );
                    rows.push(done);
                    cur_col = 0;
                }
                if li == cursor_line && ci == cursor_ofs {
                    cursor_at = Some((rows.len(), cur_col));
                }
                cur.cells.push((ch, kind));
                cur_col += w;
            }
            if li == cursor_line && cursor_ofs == cells.len() && cursor_at.is_none() {
                cursor_at = Some((rows.len(), cur_col));
            }
            rows.push(cur);
        }
        let total_rows = rows.len();
        let (cursor_row, cursor_col) = cursor_at.unwrap_or((0, prompt_width));

        // ---- Choose the viewport: at most `th` rows, containing the
        // cursor, scrolled minimally from the previous window ----
        let visible = total_rows.min(th);
        let mut vt = self.viewport_top.min(total_rows - visible);
        if cursor_row < vt {
            vt = cursor_row;
        } else if cursor_row >= vt + visible {
            vt = cursor_row + 1 - visible;
        }
        self.viewport_top = vt;

        // ---- Clear every row of the previous render (its bookkeeping is
        // viewport-relative, so all rows are on screen) ----
        if self.prev_cursor_row > 0 {
            term.move_up(self.prev_cursor_row as u16)?;
        }
        term.move_to_column(0)?;
        for i in 0..=self.prev_total_rows {
            if i > 0 {
                term.move_down(1)?;
            }
            term.clear_current_line()?;
        }
        if self.prev_total_rows > 0 {
            term.move_up(self.prev_total_rows as u16)?;
        }
        term.move_to_column(0)?;

        // ---- Paint the visible rows ----
        let mut span_idx = 0;
        let mut current_style = HighlightStyle::Default;
        let mut dim_on = false;
        for (vi, row) in rows[vt..vt + visible].iter().enumerate() {
            if vi > 0 || row.first {
                // Row breaks and prefixes are always unstyled.
                if current_style != HighlightStyle::Default {
                    term.reset_style()?;
                    current_style = HighlightStyle::Default;
                }
                if dim_on {
                    term.set_dim(false)?;
                    dim_on = false;
                }
            }
            // `in_prefix` rows are reached by the prefix write's auto-wrap,
            // not by an explicit row break. (If the viewport ever starts
            // mid-prefix — a prompt wider than a whole screen — the prefix
            // tail is not repainted; cells are positioned explicitly below.)
            if vi > 0 && !row.in_prefix {
                term.write_str("\r\n")?;
            }
            if row.first {
                term.write_str(if row.line == 0 { prompt } else { cont_prompt })?;
            } else if vi == 0 && row.in_prefix && !row.cells.is_empty() {
                term.move_to_column(col(row.start_col))?;
            }
            for &(ch, kind) in &row.cells {
                match kind {
                    Cell::Buf(i) => {
                        if dim_on {
                            term.set_dim(false)?;
                            dim_on = false;
                        }
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
                    }
                    Cell::Sugg => {
                        if current_style != HighlightStyle::Default {
                            term.reset_style()?;
                            current_style = HighlightStyle::Default;
                        }
                        if !dim_on {
                            term.set_dim(true)?;
                            dim_on = true;
                        }
                    }
                }
                term.write_char(ch)?;
            }
        }
        if current_style != HighlightStyle::Default {
            term.reset_style()?;
        }
        if dim_on {
            term.set_dim(false)?;
        }

        // ---- Position the cursor (window-relative) ----
        // The paint left the physical cursor on the last visible row.
        let end_row = vt + visible - 1;
        if end_row > cursor_row {
            term.move_up((end_row - cursor_row) as u16)?;
        }
        // A cursor one past an exactly-full row sits in the terminal's
        // deferred-wrap position; emit the last real column explicitly
        // instead of relying on the terminal clamping an out-of-range CHA.
        let cursor_col = if tw > 0 {
            cursor_col.min(tw - 1)
        } else {
            cursor_col
        };
        term.move_to_column(col(cursor_col))?;

        self.prev_total_rows = visible - 1;
        self.prev_cursor_row = cursor_row - vt;
        self.prev_render_multiline = true;
        self.prev_render = Some((self.buf.clone(), spans.to_vec()));
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
            EditAction::InsertNewline
            | EditAction::KillToEnd
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
            | EditAction::CapitalizeWord
                if !self.last_was_insert =>
            {
                // Not transitioning from insert — save pre-op state directly.
                // (When last_was_insert, the boundary save above already
                // captured the state.)
                self.undo.save(&self.buf, self.pos);
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
        // Preferred-column stickiness only survives an unbroken run of
        // vertical moves (Up/Down map to HistoryPrev/HistoryNext); any
        // other *edit* action ends the run. A numeric-argument prefix
        // (`Alt+3 Up` = "three more lines of this run") edits nothing and
        // must not recapture the clamped column mid-run. Buffer mutations
        // additionally reset it via invalidate_width_cache.
        if !matches!(
            action,
            EditAction::HistoryPrev | EditAction::HistoryNext | EditAction::SetNumericArg(_)
        ) {
            self.preferred_col = None;
        }
        match action {
            EditAction::InsertChar(ch) => {
                for _ in 0..count {
                    self.insert_char(ch);
                }
                KeyAction::Continue
            }
            EditAction::InsertNewline => {
                // Same rationale as the Enter-on-incomplete continuation:
                // a forced newline starts a multiline construct, so drop any
                // in-flight history navigation state.
                history.reset_cursor();
                for _ in 0..count {
                    self.insert_char('\n');
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
                let lines_before = self.line_count();
                self.accept_full_suggestion();
                if self.line_count() > lines_before {
                    // Same rationale as InsertNewline: accepting a multiline
                    // suggestion turns the buffer into a new in-progress
                    // construct; a stale history cursor would let a later Up
                    // replace it with an unrelated older entry.
                    history.reset_cursor();
                }
                KeyAction::Continue
            }
            EditAction::AcceptWordSuggestion => {
                let lines_before = self.line_count();
                self.accept_word_suggestion();
                if self.line_count() > lines_before {
                    history.reset_cursor();
                }
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
                    // In a multiline buffer, Up first moves the cursor across
                    // logical lines; history navigation takes over on the
                    // first line (zsh `up-line-or-history` behavior).
                    if self.cursor_line_index() > 0 {
                        self.move_cursor_up();
                    } else if let Some(line) = history.navigate_up(&self.buffer()) {
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
                    // Mirror of HistoryPrev: Down moves within the buffer
                    // until the last line, then navigates history.
                    if self.cursor_line_index() + 1 < self.line_count() {
                        self.move_cursor_down();
                    } else if let Some(line) = history.navigate_down() {
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
    ///
    /// Multiline editing: when Enter is pressed and `is_incomplete` returns
    /// `true` for the current buffer contents, a literal newline is inserted
    /// at the cursor instead of submitting, and editing continues across
    /// lines. Continuation lines are rendered with the prompt returned by
    /// `cont_prompt` (the caller's expanded `PS2` last line); the callback
    /// is invoked lazily — once per read, on the first multiline render —
    /// so a side-effectful PS2 (command substitution) only runs when a
    /// continuation line is actually displayed.
    #[allow(clippy::too_many_arguments)]
    pub fn read_line_with_completion<T: Terminal>(
        &mut self,
        prompt: &str,
        upper_lines: &[String],
        history: &mut History,
        term: &mut T,
        ctx: &CompletionContext,
        cmd_ctx: &mut CommandCompletionContext<'_>,
        specs: &mut SpecStore,
        scanner: &mut HighlightScanner,
        checker_env: &CheckerEnv<'_>,
        accumulated: &str,
        cont_prompt: &mut dyn FnMut() -> String,
        is_incomplete: &dyn Fn(&str) -> bool,
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
            specs,
            scanner,
            checker_env,
            accumulated,
            cont_prompt,
            is_incomplete,
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
        specs: &mut SpecStore,
        scanner: &mut HighlightScanner,
        checker_env: &CheckerEnv<'_>,
        accumulated: &str,
        cont_prompt: &mut dyn FnMut() -> String,
        is_incomplete: &dyn Fn(&str) -> bool,
    ) -> io::Result<Option<String>> {
        let prompt_width = display_width(prompt);
        loop {
            term.flush()?;
            match term.read_event()? {
                Event::Key(key_event) => {
                    match self.handle_key(key_event, history) {
                        KeyAction::Submit => {
                            if is_incomplete(&self.buffer()) {
                                // Structurally incomplete input (unclosed
                                // if/quote/heredoc, trailing pipe, …): start
                                // a continuation line in-buffer instead of
                                // submitting, keeping the whole construct
                                // editable with cursor movement across lines.
                                // Reset history navigation like the Submit
                                // path does: the buffer is now a new
                                // in-progress construct, and a stale cursor
                                // would let a later Up replace it with an
                                // unrelated older entry.
                                history.reset_cursor();
                                self.insert_char('\n');
                            } else {
                                history.reset_cursor();
                                term.reset_style()?;
                                let spans = scanner.scan(accumulated, &self.buf, checker_env);
                                let cont = self.resolve_cont_prompt(cont_prompt);
                                self.clear_lingering_suggestion(
                                    term,
                                    prompt,
                                    prompt_width,
                                    &cont,
                                    &spans,
                                )?;
                                self.move_below_render(term, prompt_width)?;
                                term.move_to_column(0)?;
                                term.write_str("\r\n")?;
                                term.flush()?;
                                return Ok(Some(self.buffer()));
                            }
                        }
                        KeyAction::Eof => {
                            return Ok(None);
                        }
                        KeyAction::Interrupt => {
                            history.reset_cursor();
                            term.reset_style()?;
                            let spans = scanner.scan(accumulated, &self.buf, checker_env);
                            let cont = self.resolve_cont_prompt(cont_prompt);
                            self.clear_lingering_suggestion(
                                term,
                                prompt,
                                prompt_width,
                                &cont,
                                &spans,
                            )?;
                            self.move_below_render(term, prompt_width)?;
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
                            self.invalidate_render_state();
                        }
                        KeyAction::TabComplete => {
                            term.reset_style()?;
                            self.handle_tab_complete(
                                term,
                                prompt,
                                upper_lines,
                                ctx,
                                cmd_ctx,
                                specs,
                            )?;
                        }
                        KeyAction::ClearScreen => {
                            term.clear_all()?;
                            for line in upper_lines {
                                term.write_str(line)?;
                                term.write_str("\r\n")?;
                            }
                            term.write_str(prompt)?;
                            self.invalidate_render_state();
                        }
                        KeyAction::Continue => {}
                    }
                    self.update_suggestion(history);
                    let spans = scanner.scan(accumulated, &self.buf, checker_env);
                    let (tw, th) = term.size().unwrap_or((80, 24));
                    let cont = self.resolve_cont_prompt(cont_prompt);
                    self.redraw(term, prompt, prompt_width, &cont, &spans, tw, th)?;
                }
                Event::Resize(_cols, _rows) => {
                    self.invalidate_render_state();
                    let (tw, th) = term.size().unwrap_or((80, 24));
                    self.update_suggestion(history);
                    let spans = scanner.scan(accumulated, &self.buf, checker_env);
                    let cont = self.resolve_cont_prompt(cont_prompt);
                    self.redraw(term, prompt, prompt_width, &cont, &spans, tw, th)?;
                }
                _ => {}
            }
        }
    }

    /// Return the continuation prompt for the upcoming redraw, resolving
    /// the lazy callback once per read session and only when the buffer is
    /// actually multiline (single-line redraws never print it).
    fn resolve_cont_prompt(&mut self, cont_prompt: &mut dyn FnMut() -> String) -> String {
        // A multiline autosuggestion renders continuation prompts too, so it
        // needs the resolved PS2 even while the buffer itself is single-line.
        let sugg_multiline = self.pos == self.buf.len()
            && self.suggestion.as_deref().is_some_and(|s| s.contains('\n'));
        if !self.buf.contains(&'\n') && !sugg_multiline {
            return String::new();
        }
        if self.cont_prompt_cache.is_none() {
            self.cont_prompt_cache = Some(cont_prompt());
        }
        self.cont_prompt_cache.clone().unwrap_or_default()
    }

    fn handle_tab_complete<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        upper_lines: &[String],
        ctx: &CompletionContext,
        cmd_ctx: &mut CommandCompletionContext<'_>,
        specs: &mut SpecStore,
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
        } else if let Some(result) =
            spec_completion::try_complete(&self.buffer(), word_start, &word, specs, ctx, cmd_ctx)
        {
            // Spec-based completion (user-defined per-command TOML)
            (result.candidates, result.common_prefix, result.keep_prefix)
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
            self.invalidate_render_state();
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
