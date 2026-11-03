//! History autosuggestion: computing, caching and accepting it.

use super::LineEditor;
use crate::interactive::history::History;

impl LineEditor {
    /// Accept the full autosuggestion, appending it to the buffer.
    pub(super) fn accept_full_suggestion(&mut self) {
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
    pub(super) fn accept_word_suggestion(&mut self) {
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
    pub(super) fn update_suggestion(&mut self, history: &History) {
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
