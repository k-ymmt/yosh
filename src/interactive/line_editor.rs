use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::collections::VecDeque;
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
use super::terminal::{CursorStyle, Terminal};
use super::undo::UndoManager;
use super::vi::{
    self, EditMode, InsertAt, OpKind, SearchDir, ViCmd, ViEngine, ViMode, ViMotion, ViOutcome,
};

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
    /// Active editing flavor (`set -o emacs` / `set -o vi`), synced by
    /// the REPL before each read.
    edit_mode: EditMode,
    /// vi command-mode key state machine (unused while `edit_mode` is
    /// `Emacs`).
    vi: ViEngine,
    /// Terminal alert requested by an invalid vi command; emitted (as
    /// BEL) and cleared by the read loop before the next redraw.
    pending_bell: bool,
    /// Cursor shape last emitted to the terminal, so the read loop only
    /// writes DECSCUSR on actual mode transitions. `None` = never set
    /// (or already reset to the terminal default).
    last_cursor_style: Option<CursorStyle>,
    /// vi `U` baseline: the buffer content the current line started
    /// from (empty at read start; updated on history recall).
    vi_line_base: Vec<char>,
    /// Cursor position when the current vi insert session began via a
    /// command-mode command; used to capture the inserted text for `.`.
    vi_insert_start: Option<usize>,
    /// Text entered during the most recent vi insert session, replayed
    /// by `.` for insert-entering commands.
    vi_last_insert: String,
    /// Active `/` / `?` pattern input: keystrokes go to the pattern and
    /// the render shows `/pattern` in place of the buffer.
    vi_search_input: Option<(SearchDir, String)>,
    /// Most recent executed search, for `n` / `N` and the empty-pattern
    /// reuse rule.
    vi_last_search: Option<(SearchDir, String)>,
    /// Synthetic key events queued by `@letter` alias macros, consumed
    /// before reading from the terminal.
    pending_events: VecDeque<KeyEvent>,
    /// Remaining `@letter` expansions allowed before the next real
    /// terminal read. A recursive macro (`alias _a='@a'`) would
    /// otherwise refill the queue forever without ever reaching the
    /// terminal (and its Ctrl+C).
    vi_macro_budget: u32,
    /// vi insert-mode Ctrl+V: the next key is inserted literally.
    vi_literal_next: bool,
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
            edit_mode: EditMode::Emacs,
            vi: ViEngine::new(),
            pending_bell: false,
            last_cursor_style: None,
            vi_line_base: Vec::new(),
            vi_insert_start: None,
            vi_last_insert: String::new(),
            vi_search_input: None,
            vi_last_search: None,
            pending_events: VecDeque::new(),
            vi_macro_budget: 0,
            vi_literal_next: false,
        }
    }

    /// Select the editing flavor for subsequent reads (`set -o emacs` /
    /// `set -o vi`). Takes effect at the start of the next read.
    pub fn set_edit_mode(&mut self, mode: EditMode) {
        self.edit_mode = mode;
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
        self.vi.reset_for_read();
        self.pending_bell = false;
        self.vi_line_base.clear();
        self.vi_insert_start = None;
        self.vi_search_input = None;
        self.pending_events.clear();
        self.vi_literal_next = false;
        if self.edit_mode == EditMode::Vi {
            // Base undo entry so `u` after the first insert session can
            // restore the empty line (vi undo is session-granular; the
            // per-run emacs saves are suppressed in vi insert mode).
            self.undo.save(&[], 0);
        }
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
    /// vi `=`: list these pathname expansions below the line.
    ViListExpansions(Vec<String>),
    /// vi `@letter`: feed the value of alias `_letter` as editor input.
    ViAliasMacro(char),
    /// vi `v`: edit the line (or history entry `n`; 0 = current line)
    /// in an external editor, then execute the result.
    ViEditInEditor(u32),
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
        self.reset_cursor_style(term);
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
            let (tw, th) = Self::term_size(term);
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

    /// Terminal size with the conventional 80x24 fallback.
    fn term_size<T: Terminal>(term: &T) -> (u16, u16) {
        term.size().unwrap_or((80, 24))
    }

    /// End-of-read screen sequence shared by Submit/Interrupt (and the
    /// like): move below the rendered input, return to column 0, restore
    /// the cursor shape, and start a fresh output line.
    fn finish_line<T: Terminal>(&mut self, term: &mut T, prompt_width: usize) -> io::Result<()> {
        self.move_below_render(term, prompt_width)?;
        term.move_to_column(0)?;
        self.reset_cursor_style(term);
        term.write_str("\r\n")?;
        term.flush()?;
        Ok(())
    }

    /// Repaint the prompt block (upper display lines plus the last prompt
    /// line) after a full-screen UI or external program disturbed the
    /// display, then reset the render bookkeeping so the next redraw does
    /// a full repaint. `clear_screen` clears the whole screen (Ctrl+L);
    /// otherwise only the current line is cleared.
    fn repaint_prompt<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        upper_lines: &[String],
        clear_screen: bool,
    ) -> io::Result<()> {
        if clear_screen {
            term.clear_all()?;
        } else {
            term.move_to_column(0)?;
            term.clear_current_line()?;
        }
        for line in upper_lines {
            term.write_str(line)?;
            term.write_str("\r\n")?;
        }
        term.write_str(prompt)?;
        self.invalidate_render_state();
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
            self.sync_cursor_style(term)?;
            term.flush()?;
            match self.next_event(term)? {
                Event::Key(key_event) => {
                    match self.handle_key(key_event, history) {
                        KeyAction::Submit => {
                            history.reset_cursor();
                            self.clear_lingering_suggestion(term, prompt, prompt_width, "> ", &[])?;
                            self.finish_line(term, prompt_width)?;
                            return Ok(Some(self.buffer()));
                        }
                        KeyAction::Eof => {
                            return Ok(None);
                        }
                        KeyAction::Interrupt => {
                            history.reset_cursor();
                            self.clear_lingering_suggestion(term, prompt, prompt_width, "> ", &[])?;
                            self.finish_line(term, prompt_width)?;
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
                            // The screen was repainted outside of redraw's own
                            // bookkeeping; force a full repaint next time so
                            // the diff-based partial repaint doesn't assume a
                            // stale on-screen state.
                            self.repaint_prompt(term, prompt, upper_lines, false)?;
                        }
                        KeyAction::ClearScreen => {
                            self.repaint_prompt(term, prompt, upper_lines, true)?;
                        }
                        KeyAction::ViListExpansions(items) => {
                            self.print_vi_expansions(term, prompt, upper_lines, &items)?;
                        }
                        // The plain read path has no alias store or
                        // editor integration (test-only entry point).
                        KeyAction::ViAliasMacro(_) => {}
                        KeyAction::ViEditInEditor(_) => {
                            self.pending_bell = true;
                        }
                        KeyAction::TabComplete | KeyAction::Continue => {}
                    }
                    self.flush_pending_bell(term)?;
                    self.update_suggestion(history);
                    let (tw, th) = Self::term_size(term);
                    self.redraw(term, prompt, prompt_width, "> ", &[], tw, th)?;
                }
                Event::Resize(_cols, _rows) => {
                    // Terminal dimensions changed, invalidating all cached
                    // row/column math from the previous render; force a full
                    // repaint.
                    self.invalidate_render_state();
                    let (tw, th) = Self::term_size(term);
                    self.update_suggestion(history);
                    self.redraw(term, prompt, prompt_width, "> ", &[], tw, th)?;
                }
                _ => {}
            }
        }
    }

    /// Move the cursor up to the first content row and clear every row of
    /// the previous render, leaving the cursor at column 0 of the first
    /// row. The cursor sits on `prev_cursor_row` (not necessarily the
    /// last rendered row — e.g. after moving left across a wrap
    /// boundary). Shared by both full-repaint paths.
    fn clear_prev_render<T: Terminal>(&mut self, term: &mut T) -> io::Result<()> {
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
        Ok(())
    }

    /// Redraw dispatcher: while vi `/` / `?` pattern input is active the
    /// input line displays the search prompt and pattern in place of the
    /// buffer (the buffer itself is untouched and reappears when the
    /// search resolves or is cancelled).
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
        if let Some((dir, pat)) = &self.vi_search_input {
            let display: Vec<char> = std::iter::once(dir.prompt_char())
                .chain(pat.chars())
                .collect();
            let saved_buf = std::mem::replace(&mut self.buf, display);
            let saved_pos = self.pos;
            let saved_suggestion = self.suggestion.take();
            self.pos = self.buf.len();
            self.invalidate_width_cache();
            let result = self.redraw_content(
                term,
                prompt,
                prompt_width,
                cont_prompt,
                &[],
                term_width,
                term_height,
            );
            self.buf = saved_buf;
            self.pos = saved_pos;
            self.suggestion = saved_suggestion;
            self.invalidate_width_cache();
            return result;
        }
        self.redraw_content(
            term,
            prompt,
            prompt_width,
            cont_prompt,
            spans,
            term_width,
            term_height,
        )
    }

    /// Redraw the current buffer on screen, positioning the cursor correctly.
    /// Handles input that wraps past the terminal width. Buffers containing
    /// literal newlines are rendered as multiple logical lines, each
    /// continuation line prefixed with `cont_prompt`.
    #[allow(clippy::too_many_arguments)]
    fn redraw_content<T: Terminal>(
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
            self.clear_prev_render(term)?;

            // Repaint the prompt
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
        let cursor_ofs = self.pos - vi::line_start(&self.buf, self.pos);

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
        self.clear_prev_render(term)?;

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
        if self.edit_mode == EditMode::Vi {
            if self.vi_search_input.is_some() {
                return self.handle_vi_search_key(key, history);
            }
            match self.vi.mode {
                ViMode::Command => return self.handle_vi_command_key(key, history),
                ViMode::Insert => {
                    // Ctrl+V literal-next (POSIX vi insert mode): the next
                    // key is inserted verbatim, ESC and control keys
                    // included — so it must run before the ESC handling.
                    if self.vi_literal_next {
                        self.vi_literal_next = false;
                        if let Some(ch) = Self::literal_char_for(key) {
                            if self.vi.replace_overwrite
                                && self.pos < vi::line_end(&self.buf, self.pos)
                            {
                                self.delete();
                            }
                            self.insert_char(ch);
                        }
                        return KeyAction::Continue;
                    }
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('v')
                    {
                        self.vi_literal_next = true;
                        return KeyAction::Continue;
                    }
                    if key.code == KeyCode::Esc {
                        self.vi_leave_insert_mode();
                        return KeyAction::Continue;
                    }
                    // POSIX vi insert-mode Ctrl+W: word-erase bounded by
                    // the current logical line's start (the emacs
                    // backward-kill crosses '\n' and would join lines).
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('w')
                    {
                        self.vi_insert_werase();
                        return KeyAction::Continue;
                    }
                    // A fast ESC-then-key sequence arrives as Alt+key
                    // (terminal ESC-prefix encoding): leave insert mode
                    // and run the key as a command-mode command, exactly
                    // like typing them slowly. This shadows the emacs
                    // Alt-chords (Alt+f/b/d…) in vi insert mode — vi ESC
                    // correctness wins; the Ctrl bindings all remain.
                    if let KeyCode::Char(c) = key.code
                        && key.modifiers.contains(KeyModifiers::ALT)
                        && !key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        self.vi_leave_insert_mode();
                        return self.handle_vi_command_key(
                            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
                            history,
                        );
                    }
                    // `R` replace mode: a plain character overwrites the
                    // character under the cursor instead of shifting it
                    // right. Delete first, then fall through to the normal
                    // insert path (which handles undo bookkeeping).
                    if self.vi.replace_overwrite
                        && let KeyCode::Char(_) = key.code
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && self.pos < vi::line_end(&self.buf, self.pos)
                    {
                        self.delete();
                    }
                    // Everything else keeps the regular (emacs-flavored)
                    // insert-mode bindings: fall through.
                }
            }
        }
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
        //
        // vi insert mode suppresses ALL of these saves: vi undo is
        // session-granular — the pre-session state was saved when insert
        // mode was entered (command-mode entry or the read-start base) —
        // so nothing typed or edited inside the session (plain chars,
        // Backspace, the Ctrl kills) may create intermediate entries;
        // `u` after ESC must restore the whole pre-session state.
        let vi_insert = self.edit_mode == EditMode::Vi;
        if self.last_was_insert && !matches!(action, EditAction::InsertChar(_)) && !vi_insert {
            // Finalize the insert group — save current state as group boundary
            self.undo.save(&self.buf, self.pos);
        }
        match action {
            EditAction::InsertChar(_) => {
                if !self.last_was_insert && !vi_insert {
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
                if !self.last_was_insert && !vi_insert =>
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

    // ── vi command mode ────────────────────────────────────────────────

    /// POSIX vi insert-mode Ctrl+W: delete back to the preceding word
    /// boundary — "the closer of the beginning of the line or the first
    /// non-blank after a blank" — never crossing the logical line start.
    fn vi_insert_werase(&mut self) {
        let ls = vi::line_start(&self.buf, self.pos);
        let mut start = self.pos;
        while start > ls && vi::is_blank(self.buf[start - 1]) {
            start -= 1;
        }
        while start > ls && !vi::is_blank(self.buf[start - 1]) {
            start -= 1;
        }
        if start == self.pos {
            return;
        }
        let killed: String = self.buf[start..self.pos].iter().collect();
        self.kill_ring.kill(&killed, false);
        self.buf.drain(start..self.pos);
        self.pos = start;
        self.invalidate_width_cache();
    }

    /// Leave vi insert mode (ESC): capture the session's inserted text
    /// for `.` (approximation: from the insert entry point to the
    /// cursor; cursor movement inside the session degrades it
    /// gracefully), then enter command mode. vi leaves the cursor on
    /// the last inserted character (one left of the insert point),
    /// clamped within the logical line. No undo boundary save here: vi
    /// undo is session-granular and the pre-session state was saved on
    /// entry.
    fn vi_leave_insert_mode(&mut self) {
        if let Some(start) = self.vi_insert_start.take() {
            if start <= self.pos {
                self.vi_last_insert = self.buf[start..self.pos].iter().collect();
            } else {
                self.vi_last_insert.clear();
            }
        }
        self.last_was_insert = false;
        self.vi.mode = ViMode::Command;
        self.vi.replace_overwrite = false;
        self.vi_literal_next = false;
        let ls = vi::line_start(&self.buf, self.pos);
        if self.pos > ls {
            self.pos -= 1;
        }
        self.clamp_vi_command_pos();
        self.suggestion = None;
    }

    /// Clamp the cursor onto a character of its logical line: command-mode
    /// cursors rest *on* a character, never past the line end (except on
    /// an empty line).
    fn clamp_vi_command_pos(&mut self) {
        let ls = vi::line_start(&self.buf, self.pos);
        let le = vi::line_end(&self.buf, self.pos);
        if self.pos >= le && le > ls {
            self.pos = le - 1;
        }
    }

    fn handle_vi_command_key(&mut self, key: KeyEvent, history: &mut History) -> KeyAction {
        match self.vi.resolve_command_key(key) {
            ViOutcome::Pending => KeyAction::Continue,
            ViOutcome::Cmd(cmd, count) => {
                let action = self.execute_vi_cmd(cmd, count, history);
                // vi commands end any emacs-side action run: a Ctrl kill
                // after a vi command must not append to the previous
                // kill, and stale yank-pop state must not survive.
                self.last_action = EditAction::Noop;
                self.yank_state = None;
                action
            }
        }
    }

    fn execute_vi_cmd(&mut self, cmd: ViCmd, count: u32, history: &mut History) -> KeyAction {
        self.execute_vi_cmd_inner(cmd, count, history, false)
    }

    /// True for commands `.` repeats (buffer-modifying, non-motion).
    fn vi_cmd_recordable(cmd: ViCmd) -> bool {
        matches!(
            cmd,
            ViCmd::EnterInsert(_)
                | ViCmd::ReplaceMode
                | ViCmd::DeleteChar
                | ViCmd::DeleteCharBack
                | ViCmd::ReplaceChar(_)
                | ViCmd::ToggleCase
                | ViCmd::SubstChar
                | ViCmd::PutAfter
                | ViCmd::PutBefore
                | ViCmd::Op(OpKind::Delete | OpKind::Change, _)
                | ViCmd::OpLine(OpKind::Delete | OpKind::Change)
                | ViCmd::InsertPrevBigword
        )
    }

    fn execute_vi_cmd_inner(
        &mut self,
        cmd: ViCmd,
        count: u32,
        history: &mut History,
        replaying: bool,
    ) -> KeyAction {
        let mode_before = self.vi.mode;
        let gen_before = self.buf_generation;
        let action = self.execute_vi_cmd_arm(cmd, count, history);
        if !replaying {
            // Entering insert mode starts a new insert session (for the
            // `.` text capture).
            if mode_before == ViMode::Command && self.vi.mode == ViMode::Insert {
                self.vi_insert_start = Some(self.pos);
            }
            // Record buffer-modifying commands for `.` — only when they
            // actually did something (a belled no-op must not overwrite
            // the last change).
            if Self::vi_cmd_recordable(cmd)
                && (self.buf_generation != gen_before || self.vi.mode == ViMode::Insert)
            {
                self.vi.record_change(cmd, count);
            }
        }
        action
    }

    /// Request a terminal alert and continue the read loop — the shared
    /// tail of every invalid/no-op vi command arm.
    fn bell(&mut self) -> KeyAction {
        self.pending_bell = true;
        KeyAction::Continue
    }

    /// Shared preamble of the count-ranged single-line commands (`x`,
    /// `r`, `~`): alert when the cursor is at/past the line end,
    /// otherwise save the undo state and return the `[pos, end)` range
    /// the count covers, clipped to the line end.
    fn count_range_on_line(&mut self, count: u32) -> Option<(usize, usize)> {
        let le = vi::line_end(&self.buf, self.pos);
        if self.pos >= le {
            self.pending_bell = true;
            return None;
        }
        self.undo.save(&self.buf, self.pos);
        Some((self.pos, (self.pos + count as usize).min(le)))
    }

    fn execute_vi_cmd_arm(&mut self, cmd: ViCmd, count: u32, history: &mut History) -> KeyAction {
        match cmd {
            ViCmd::Bell => self.bell(),
            ViCmd::Submit => KeyAction::Submit,
            ViCmd::Eof => {
                // Like the insert-mode Ctrl+D: EOF only on an empty line.
                if self.is_empty() {
                    KeyAction::Eof
                } else {
                    self.bell()
                }
            }
            ViCmd::Interrupt => KeyAction::Interrupt,
            ViCmd::ClearScreen => KeyAction::ClearScreen,
            ViCmd::FuzzySearch => KeyAction::FuzzySearch,
            ViCmd::Move(motion) => {
                match vi::motion_move(&self.buf, self.pos, motion, count) {
                    Some(p) => self.pos = p,
                    None => self.pending_bell = true,
                }
                KeyAction::Continue
            }
            ViCmd::EnterInsert(at) => {
                // Pre-session state for session-granular vi undo.
                self.undo.save(&self.buf, self.pos);
                self.vi.mode = ViMode::Insert;
                let le = vi::line_end(&self.buf, self.pos);
                match at {
                    InsertAt::Here => {}
                    InsertAt::AfterChar => {
                        if self.pos < le {
                            self.pos += 1;
                        }
                    }
                    InsertAt::FirstNonBlank => {
                        if let Some(p) =
                            vi::motion_move(&self.buf, self.pos, ViMotion::FirstNonBlank, 1)
                        {
                            self.pos = p;
                        }
                    }
                    InsertAt::LineEnd => self.pos = le,
                }
                KeyAction::Continue
            }
            ViCmd::ReplaceMode => {
                self.undo.save(&self.buf, self.pos);
                self.vi.mode = ViMode::Insert;
                self.vi.replace_overwrite = true;
                KeyAction::Continue
            }
            ViCmd::DeleteChar => {
                let Some((start, end)) = self.count_range_on_line(count) else {
                    return KeyAction::Continue;
                };
                let killed: String = self.buf[start..end].iter().collect();
                self.kill_ring.kill(&killed, false);
                self.buf.drain(start..end);
                self.invalidate_width_cache();
                self.clamp_vi_command_pos();
                KeyAction::Continue
            }
            ViCmd::DeleteCharBack => {
                // Mirror of DeleteChar toward the line start; the range
                // preamble differs (bounded by `ls`, cursor lands on
                // `start`), so it does not share count_range_on_line.
                let ls = vi::line_start(&self.buf, self.pos);
                if self.pos <= ls {
                    return self.bell();
                }
                self.undo.save(&self.buf, self.pos);
                let start = self.pos.saturating_sub(count as usize).max(ls);
                let killed: String = self.buf[start..self.pos].iter().collect();
                self.kill_ring.kill(&killed, false);
                self.buf.drain(start..self.pos);
                self.pos = start;
                self.invalidate_width_cache();
                self.clamp_vi_command_pos();
                KeyAction::Continue
            }
            ViCmd::ReplaceChar(c) => {
                let Some((start, end)) = self.count_range_on_line(count) else {
                    return KeyAction::Continue;
                };
                for i in start..end {
                    self.buf[i] = c;
                }
                self.pos = end - 1;
                self.invalidate_width_cache();
                KeyAction::Continue
            }
            ViCmd::ToggleCase => {
                let Some((start, end)) = self.count_range_on_line(count) else {
                    return KeyAction::Continue;
                };
                for i in start..end {
                    let ch = self.buf[i];
                    self.buf[i] = if ch.is_uppercase() {
                        ch.to_lowercase().next().unwrap_or(ch)
                    } else if ch.is_lowercase() {
                        ch.to_uppercase().next().unwrap_or(ch)
                    } else {
                        ch
                    };
                }
                // Cursor advances past the last toggled character, capped
                // at the line's last character (the buffer length is
                // unchanged, so the line end is where it was).
                let le = vi::line_end(&self.buf, self.pos);
                self.pos = end.min(le - 1);
                self.invalidate_width_cache();
                KeyAction::Continue
            }
            ViCmd::Op(op, motion) => {
                // cw / cW act like ce / cE on a non-blank character
                // (vi/readline tradition; POSIX is silent on it).
                let motion = match (op, motion) {
                    (OpKind::Change, ViMotion::WordForward { big })
                        if self.pos < self.buf.len()
                            && !matches!(self.buf[self.pos], ' ' | '\t' | '\n') =>
                    {
                        ViMotion::WordEnd { big }
                    }
                    _ => motion,
                };
                match vi::motion_range(&self.buf, self.pos, motion, count) {
                    Some((start, end)) if start < end => {
                        let text: String = self.buf[start..end].iter().collect();
                        self.kill_ring.kill(&text, false);
                        match op {
                            OpKind::Yank => {
                                // POSIX: the cursor does not move.
                            }
                            OpKind::Delete | OpKind::Change => {
                                self.undo.save(&self.buf, self.pos);
                                self.buf.drain(start..end);
                                self.pos = start;
                                self.invalidate_width_cache();
                                if op == OpKind::Change {
                                    self.vi.mode = ViMode::Insert;
                                } else {
                                    self.clamp_vi_command_pos();
                                }
                            }
                        }
                        KeyAction::Continue
                    }
                    _ => self.bell(),
                }
            }
            ViCmd::OpLine(op) => {
                let ls = vi::line_start(&self.buf, self.pos);
                let le = vi::line_end(&self.buf, self.pos);
                let text: String = self.buf[ls..le].iter().collect();
                if !text.is_empty() {
                    self.kill_ring.kill(&text, false);
                }
                match op {
                    OpKind::Yank => {}
                    OpKind::Delete | OpKind::Change => {
                        self.undo.save(&self.buf, self.pos);
                        self.buf.drain(ls..le);
                        self.pos = ls;
                        self.invalidate_width_cache();
                        if op == OpKind::Change {
                            self.vi.mode = ViMode::Insert;
                        }
                    }
                }
                KeyAction::Continue
            }
            ViCmd::SubstChar => {
                self.undo.save(&self.buf, self.pos);
                let le = vi::line_end(&self.buf, self.pos);
                if self.pos < le {
                    let end = (self.pos + count as usize).min(le);
                    let text: String = self.buf[self.pos..end].iter().collect();
                    self.kill_ring.kill(&text, false);
                    self.buf.drain(self.pos..end);
                    self.invalidate_width_cache();
                }
                self.vi.mode = ViMode::Insert;
                KeyAction::Continue
            }
            ViCmd::PutAfter | ViCmd::PutBefore => {
                let text = self.kill_ring.yank().map(str::to_string);
                let Some(text) = text.filter(|t| !t.is_empty()) else {
                    return self.bell();
                };
                self.undo.save(&self.buf, self.pos);
                let le = vi::line_end(&self.buf, self.pos);
                let at = if cmd == ViCmd::PutAfter {
                    (self.pos + 1).min(le)
                } else {
                    self.pos
                };
                let chars: Vec<char> = text.chars().collect();
                // Bound the total inserted text (count × kill length) so a
                // huge count cannot balloon the buffer.
                let reps = (count.max(1) as usize)
                    .min(1_000_000 / chars.len().max(1))
                    .max(1);
                let mut insert_pos = at;
                for _ in 0..reps {
                    for &c in &chars {
                        self.buf.insert(insert_pos, c);
                        insert_pos += 1;
                    }
                }
                // Cursor lands on the last put character, clamped onto an
                // editable cell (put text ending in '\n' would otherwise
                // park the cursor on the separator).
                self.pos = insert_pos - 1;
                self.invalidate_width_cache();
                self.clamp_vi_command_pos();
                KeyAction::Continue
            }
            ViCmd::Undo => {
                match self.undo.undo() {
                    Some((buf, pos)) => {
                        self.buf = buf;
                        self.pos = pos;
                        self.invalidate_width_cache();
                        self.clamp_vi_command_pos();
                    }
                    None => self.pending_bell = true,
                }
                KeyAction::Continue
            }
            ViCmd::UndoAll => {
                // U restores the line the edit started from (empty, or
                // the recalled history entry); u can undo the U itself.
                self.undo.save(&self.buf, self.pos);
                self.buf = self.vi_line_base.clone();
                self.pos = 0;
                self.invalidate_width_cache();
                self.clamp_vi_command_pos();
                KeyAction::Continue
            }
            ViCmd::HistoryPrev => {
                self.vi_history_step(true, count, history);
                KeyAction::Continue
            }
            ViCmd::HistoryNext => {
                self.vi_history_step(false, count, history);
                KeyAction::Continue
            }
            ViCmd::HistoryGoto => {
                // G: count 0 = oldest entry; count n = entry n (1-based,
                // oldest first).
                let len = history.entries().len();
                let idx = if count == 0 {
                    if len == 0 {
                        return self.bell();
                    }
                    0
                } else {
                    let idx = count as usize - 1;
                    if idx >= len {
                        return self.bell();
                    }
                    idx
                };
                let current = self.buffer();
                if let Some(line) = history.navigate_to(idx, &current).map(str::to_string) {
                    self.vi_recall(line);
                }
                KeyAction::Continue
            }
            ViCmd::SearchStart(dir) => {
                self.vi_search_input = Some((dir, String::new()));
                self.suggestion = None;
                KeyAction::Continue
            }
            ViCmd::SearchNext | ViCmd::SearchReverse => {
                match self.vi_last_search.clone() {
                    Some((dir, pattern)) => {
                        let dir = if cmd == ViCmd::SearchReverse {
                            dir.reversed()
                        } else {
                            dir
                        };
                        if !self.vi_history_search(dir, &pattern, history) {
                            self.pending_bell = true;
                        }
                    }
                    None => self.pending_bell = true,
                }
                KeyAction::Continue
            }
            ViCmd::InsertPrevBigword => {
                // `_`: append a space plus the count-th (default last)
                // bigword of the previous input line, then enter insert
                // mode after it.
                let Some(prev) = history.entries().last() else {
                    return self.bell();
                };
                let bigwords: Vec<&str> = prev.split_whitespace().collect();
                let word = if count == 0 {
                    bigwords.last().copied()
                } else {
                    bigwords.get(count as usize - 1).copied()
                };
                let Some(word) = word.map(str::to_string) else {
                    return self.bell();
                };
                self.undo.save(&self.buf, self.pos);
                let le = vi::line_end(&self.buf, self.pos);
                let mut at = (self.pos + 1).min(le);
                // POSIX: "Append a <space> after the current character
                // position" — unconditionally, empty line included.
                self.buf.insert(at, ' ');
                at += 1;
                for c in word.chars() {
                    self.buf.insert(at, c);
                    at += 1;
                }
                self.pos = at;
                self.invalidate_width_cache();
                self.vi.mode = ViMode::Insert;
                KeyAction::Continue
            }
            ViCmd::ExpandList | ViCmd::CompleteUnique | ViCmd::ExpandAll => {
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
            ViCmd::CommentSubmit => {
                // Insert '#' at the start of every logical line and
                // submit: the input is recorded in history but executes
                // as comments. (POSIX describes the single-line case;
                // commenting each line keeps multiline buffers inert.)
                self.undo.save(&self.buf, self.pos);
                self.buf.insert(0, '#');
                let mut i = 1;
                while i < self.buf.len() {
                    if self.buf[i] == '\n' {
                        self.buf.insert(i + 1, '#');
                        i += 1;
                    }
                    i += 1;
                }
                self.pos = self.buf.len();
                self.invalidate_width_cache();
                KeyAction::Submit
            }
            ViCmd::AliasMacro(c) => KeyAction::ViAliasMacro(c),
            ViCmd::EditInEditor => KeyAction::ViEditInEditor(count),
            ViCmd::Repeat => {
                let Some(rec) = self.vi.last_change() else {
                    return self.bell();
                };
                // count 0 = bare `.`: reuse the recorded count. An
                // explicit count becomes the new default (POSIX).
                let effective = if count > 0 {
                    self.vi.set_last_change_count(count);
                    count
                } else {
                    rec.count
                };
                let action = self.execute_vi_cmd_inner(rec.cmd, effective, history, true);
                // Insert-entering commands replay the recorded text and
                // return to command mode, as if the user retyped it and
                // pressed ESC.
                if self.vi.mode == ViMode::Insert {
                    self.vi_replay_insert();
                }
                action
            }
        }
    }

    // ── vi history navigation and search ───────────────────────────────

    /// Recall a history navigation result into the buffer, vi-style:
    /// cursor on the first character (POSIX), baseline updated for `U`.
    fn vi_recall(&mut self, line: String) {
        self.buf = line.chars().collect();
        self.pos = 0;
        self.vi_line_base = self.buf.clone();
        self.suggestion = None;
        self.invalidate_width_cache();
        self.clamp_vi_command_pos();
    }

    /// vi `k` / `j`: move within a multiline buffer first (matching the
    /// emacs Up/Down behavior), then navigate history.
    fn vi_history_step(&mut self, prev: bool, count: u32, history: &mut History) {
        for _ in 0..count.max(1) {
            if prev {
                if self.cursor_line_index() > 0 {
                    self.move_cursor_up();
                    // The column-preserving move may land on the '\n' of
                    // a shorter line; command-mode cursors rest on a
                    // character.
                    self.clamp_vi_command_pos();
                } else if let Some(line) = history.navigate_up(&self.buffer()).map(str::to_string) {
                    self.vi_recall(line);
                } else {
                    self.pending_bell = true;
                    break;
                }
            } else if self.cursor_line_index() + 1 < self.line_count() {
                self.move_cursor_down();
                self.clamp_vi_command_pos();
            } else if let Some(line) = history.navigate_down().map(str::to_string) {
                self.vi_recall(line);
            } else {
                self.pending_bell = true;
                break;
            }
        }
    }

    /// Execute a `/` / `?` history search. Returns whether a matching
    /// entry was recalled.
    fn vi_history_search(&mut self, dir: SearchDir, pattern: &str, history: &mut History) -> bool {
        // POSIX: sh patterns; a leading `^` anchors at the start of the
        // line, otherwise the pattern matches anywhere. A trailing odd
        // backslash would escape the appended `*`; double it so it stays
        // a literal backslash.
        let escape_tail = |p: &str| {
            let trailing = p.chars().rev().take_while(|&c| c == '\\').count();
            if trailing % 2 == 1 {
                format!("{}\\", p)
            } else {
                p.to_string()
            }
        };
        let full_pattern = match pattern.strip_prefix('^') {
            Some(rest) => format!("{}*", escape_tail(rest)),
            None => format!("*{}*", escape_tail(pattern)),
        };
        let len = history.entries().len();
        let found = match dir {
            SearchDir::Older => {
                let upper = history.cursor().unwrap_or(len);
                history.entries()[..upper]
                    .iter()
                    .rposition(|e| crate::expand::pattern::matches(&full_pattern, e))
            }
            SearchDir::Newer => match history.cursor() {
                None => None,
                Some(cur) => history.entries()[cur + 1..]
                    .iter()
                    .position(|e| crate::expand::pattern::matches(&full_pattern, e))
                    .map(|i| cur + 1 + i),
            },
        };
        match found {
            Some(idx) => {
                let current = self.buffer();
                if let Some(line) = history.navigate_to(idx, &current).map(str::to_string) {
                    self.vi_recall(line);
                    true
                } else {
                    false
                }
            }
            None => false,
        }
    }

    /// Handle a key while `/` / `?` pattern input is active.
    fn handle_vi_search_key(&mut self, key: KeyEvent, history: &mut History) -> KeyAction {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c') if ctrl => {
                self.vi_search_input = None;
                return KeyAction::Interrupt;
            }
            KeyCode::Esc => {
                self.vi_search_input = None;
            }
            KeyCode::Backspace => {
                if let Some((_, pat)) = &mut self.vi_search_input
                    && pat.pop().is_none()
                {
                    // Backspacing past the start cancels the search.
                    self.vi_search_input = None;
                }
            }
            KeyCode::Enter => {
                let (dir, pattern) = self.vi_search_input.take().expect("search input active");
                // Empty pattern reuses the previous one (POSIX); the
                // direction comes from the just-typed `/` or `?`.
                let pattern = if pattern.is_empty() {
                    match &self.vi_last_search {
                        Some((_, p)) => p.clone(),
                        None => {
                            self.pending_bell = true;
                            return KeyAction::Continue;
                        }
                    }
                } else {
                    pattern
                };
                if !self.vi_history_search(dir, &pattern, history) {
                    self.pending_bell = true;
                }
                self.vi_last_search = Some((dir, pattern));
            }
            KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
                if let Some((_, pat)) = &mut self.vi_search_input {
                    pat.push(c);
                }
            }
            _ => {}
        }
        KeyAction::Continue
    }

    /// The literal character a key event inserts under Ctrl+V.
    fn literal_char_for(key: KeyEvent) -> Option<char> {
        match key.code {
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let up = c.to_ascii_uppercase();
                if up.is_ascii_uppercase() || ('@'..='_').contains(&up) {
                    Some(((up as u8) ^ 0x40) as char)
                } else {
                    None
                }
            }
            KeyCode::Char(c) => Some(c),
            KeyCode::Esc => Some('\x1b'),
            KeyCode::Tab => Some('\t'),
            KeyCode::Enter => Some('\n'),
            KeyCode::Backspace => Some('\x7f'),
            _ => None,
        }
    }

    // ── vi pathname expansion (`=` `\` `*`) ───────────────────────────

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
        self.undo.save(&self.buf, self.pos);
        self.buf.splice(start..end, text.chars());
        self.pos = start + text.chars().count();
        self.invalidate_width_cache();
    }

    /// Replay the last insert session's text (for `.`), honoring `R`
    /// overwrite semantics, then leave insert mode like ESC does.
    fn vi_replay_insert(&mut self) {
        let text: Vec<char> = self.vi_last_insert.chars().collect();
        for ch in text {
            if self.vi.replace_overwrite && self.pos < vi::line_end(&self.buf, self.pos) {
                self.delete();
            }
            self.insert_char(ch);
        }
        self.vi.mode = ViMode::Command;
        self.vi.replace_overwrite = false;
        let ls = vi::line_start(&self.buf, self.pos);
        if self.pos > ls {
            self.pos -= 1;
        }
        self.clamp_vi_command_pos();
    }

    /// Emit the cursor shape matching the current vi submode when it
    /// changed since the last emit. No-op in emacs mode (the user's
    /// terminal default is left untouched).
    fn sync_cursor_style<T: Terminal>(&mut self, term: &mut T) -> io::Result<()> {
        if self.edit_mode != EditMode::Vi {
            return Ok(());
        }
        let want = match self.vi.mode {
            ViMode::Insert => CursorStyle::Bar,
            ViMode::Command => CursorStyle::Block,
        };
        if self.last_cursor_style != Some(want) {
            term.set_cursor_style(want)?;
            self.last_cursor_style = Some(want);
        }
        Ok(())
    }

    /// Restore the terminal-default cursor shape if a vi read changed it.
    fn reset_cursor_style<T: Terminal>(&mut self, term: &mut T) {
        if self.last_cursor_style.take().is_some() {
            let _ = term.set_cursor_style(CursorStyle::Default);
            let _ = term.flush();
        }
    }

    /// Pop a queued synthetic event (`@letter` macro input) or read one
    /// from the terminal.
    fn next_event<T: Terminal>(&mut self, term: &mut T) -> io::Result<Event> {
        if let Some(k) = self.pending_events.pop_front() {
            return Ok(Event::Key(k));
        }
        // Reading from the terminal replenishes the alias-macro budget:
        // the recursion cap is per keystroke, not per read session.
        self.vi_macro_budget = 64;
        term.read_event()
    }

    /// Convert alias-macro text into key events: ESC, Enter, Tab, and
    /// control characters map to their key equivalents so an alias value
    /// can contain editing commands (POSIX `@letter`).
    fn key_events_for_text(text: &str) -> Vec<KeyEvent> {
        text.chars()
            .map(|c| match c {
                '\x1b' => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                '\n' | '\r' => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                '\t' => KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
                c if (c as u32) < 0x20 => KeyEvent::new(
                    KeyCode::Char((((c as u8) | 0x60) as char).to_ascii_lowercase()),
                    KeyModifiers::CONTROL,
                ),
                c => KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            })
            .collect()
    }

    /// Print the vi `=` expansion list below the current render, then
    /// reprint the prompt for the follow-up redraw.
    fn print_vi_expansions<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        upper_lines: &[String],
        items: &[String],
    ) -> io::Result<()> {
        self.move_below_render(term, 0)?;
        term.move_to_column(0)?;
        term.write_str("\r\n")?;
        term.write_str(&items.join("  "))?;
        term.write_str("\r\n")?;
        for line in upper_lines {
            term.write_str(line)?;
            term.write_str("\r\n")?;
        }
        term.write_str(prompt)?;
        self.invalidate_render_state();
        Ok(())
    }

    /// vi `v`: run the external editor on the current line (or history
    /// entry `entry`; 0 = current line). On success the edited text
    /// replaces the buffer and `Ok(true)` asks the caller to submit it.
    fn vi_edit_in_editor<T: Terminal>(
        &mut self,
        term: &mut T,
        history: &History,
        entry: u32,
    ) -> io::Result<bool> {
        let content = if entry == 0 {
            self.buffer()
        } else {
            match history.entries().get(entry as usize - 1) {
                Some(e) => e.clone(),
                None => {
                    self.pending_bell = true;
                    return Ok(false);
                }
            }
        };
        // O_EXCL via create_new: refuses to follow a pre-planted symlink
        // at a predictable path and never overwrites an existing file.
        let mut path = std::env::temp_dir();
        let mut created = None;
        for attempt in 0..64u32 {
            let mut candidate = path.clone();
            let uniq = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            candidate.push(format!(
                "yosh-vi-{}-{}-{}.sh",
                std::process::id(),
                uniq,
                attempt
            ));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(f) => {
                    created = Some((candidate, f));
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        let Some((tmp_path, mut file)) = created else {
            self.pending_bell = true;
            return Ok(false);
        };
        path = tmp_path;
        {
            use std::io::Write as _;
            writeln!(file, "{}", content)?;
        }
        drop(file);

        term.reset_style()?;
        term.disable_raw_mode()?;
        // POSIX prescribes vi; honor the conventional VISUAL/EDITOR
        // overrides first. Shell variables set inside yosh are not
        // process-environment exports, so this reads the inherited
        // environment only.
        let editor = std::env::var("VISUAL")
            .unwrap_or_else(|_| std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string()));
        let mut parts = editor.split_whitespace();
        let status = parts.next().map(|prog| {
            std::process::Command::new(prog)
                .args(parts)
                .arg(&path)
                .status()
        });
        term.enable_raw_mode()?;

        let ok = matches!(status, Some(Ok(st)) if st.success());
        let result = if ok {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let text = text.strip_suffix('\n').unwrap_or(&text).to_string();
            self.buf = text.chars().collect();
            self.pos = self.buf.len();
            self.invalidate_width_cache();
            true
        } else {
            self.pending_bell = true;
            self.invalidate_render_state();
            false
        };
        let _ = std::fs::remove_file(&path);
        Ok(result)
    }

    /// Emit (and clear) a pending vi alert as a terminal BEL.
    fn flush_pending_bell<T: Terminal>(&mut self, term: &mut T) -> io::Result<()> {
        if self.pending_bell {
            self.pending_bell = false;
            term.write_str("\x07")?;
        }
        Ok(())
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
        self.reset_cursor_style(term);
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
            self.sync_cursor_style(term)?;
            term.flush()?;
            match self.next_event(term)? {
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
                                // vi Enter executes the whole buffer
                                // regardless of cursor position, so the
                                // continuation line starts after the end
                                // of what was submitted — not at a
                                // mid-buffer command-mode cursor. The
                                // continuation conventionally starts in
                                // insert mode, whichever submode Enter
                                // was pressed in.
                                if self.edit_mode == EditMode::Vi {
                                    // The continuation is a fresh insert
                                    // session: save the pre-continuation
                                    // state for `u` and start the `.`
                                    // text capture at the new line.
                                    self.undo.save(&self.buf, self.pos);
                                    self.pos = self.buf.len();
                                    self.vi.mode = ViMode::Insert;
                                    self.vi.replace_overwrite = false;
                                    self.insert_char('\n');
                                    self.vi_insert_start = Some(self.pos);
                                } else {
                                    self.insert_char('\n');
                                }
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
                                self.finish_line(term, prompt_width)?;
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
                            self.finish_line(term, prompt_width)?;
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
                            self.repaint_prompt(term, prompt, upper_lines, false)?;
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
                            self.repaint_prompt(term, prompt, upper_lines, true)?;
                        }
                        KeyAction::ViListExpansions(items) => {
                            term.reset_style()?;
                            self.print_vi_expansions(term, prompt, upper_lines, &items)?;
                        }
                        KeyAction::ViAliasMacro(c) => {
                            // POSIX @letter: run alias `_letter`'s value as
                            // editor input; no effect if unset. The
                            // per-keystroke expansion budget (see
                            // next_event) breaks recursive macros like
                            // `alias _a='@a'`; the size cap bounds a
                            // single expansion burst.
                            let name = format!("_{}", c);
                            if let Some(value) = cmd_ctx.aliases.get(&name)
                                && self.vi_macro_budget > 0
                                && self.pending_events.len() + value.chars().count() <= 4096
                            {
                                self.vi_macro_budget -= 1;
                                for ev in Self::key_events_for_text(value) {
                                    self.pending_events.push_back(ev);
                                }
                            }
                        }
                        KeyAction::ViEditInEditor(entry) => {
                            term.reset_style()?;
                            if self.vi_edit_in_editor(term, history, entry)? {
                                // Execute the edited line (POSIX v).
                                history.reset_cursor();
                                term.move_to_column(0)?;
                                self.reset_cursor_style(term);
                                term.write_str("\r\n")?;
                                term.flush()?;
                                return Ok(Some(self.buffer()));
                            }
                            // Editor failed or was aborted: repaint the
                            // prompt line and continue editing.
                            self.repaint_prompt(term, prompt, upper_lines, false)?;
                        }
                        KeyAction::Continue => {}
                    }
                    self.flush_pending_bell(term)?;
                    self.update_suggestion(history);
                    let spans = scanner.scan(accumulated, &self.buf, checker_env);
                    let (tw, th) = Self::term_size(term);
                    let cont = self.resolve_cont_prompt(cont_prompt);
                    self.redraw(term, prompt, prompt_width, &cont, &spans, tw, th)?;
                }
                Event::Resize(_cols, _rows) => {
                    self.invalidate_render_state();
                    let (tw, th) = Self::term_size(term);
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
            self.repaint_prompt(term, prompt, upper_lines, false)?;
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
