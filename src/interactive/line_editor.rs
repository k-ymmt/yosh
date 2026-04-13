use std::io;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use super::completion::{self, CompletionContext, CompletionUI};
use super::fuzzy_search::FuzzySearchUI;
use super::highlight::{HighlightScanner, HighlightStyle, ColorSpan, CheckerEnv, apply_style};
use super::history::History;
use super::terminal::Terminal;

/// A minimal line-editing buffer used by the interactive REPL.
///
/// The buffer stores characters as a `Vec<char>` so that cursor
/// movement and insertion work correctly with multi-byte UTF-8
/// characters.
#[derive(Debug, Default)]
pub struct LineEditor {
    buf: Vec<char>,
    pos: usize,
    suggestion: Option<String>,
    tab_count: u8,
}

impl LineEditor {
    /// Create an empty line editor.
    pub fn new() -> Self {
        Self::default()
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
    }

    /// Insert a character at the current cursor position and advance
    /// the cursor by one.
    pub fn insert_char(&mut self, ch: char) {
        self.buf.insert(self.pos, ch);
        self.pos += 1;
    }

    /// Delete the character immediately before the cursor (like the
    /// Backspace key).  Does nothing when the cursor is at position 0.
    pub fn backspace(&mut self) {
        if self.pos > 0 {
            self.pos -= 1;
            self.buf.remove(self.pos);
        }
    }

    /// Delete the character at the current cursor position (like the
    /// Delete key).  Does nothing when the cursor is at the end of
    /// the buffer.
    pub fn delete(&mut self) {
        if self.pos < self.buf.len() {
            self.buf.remove(self.pos);
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
        killed
    }

    /// Kill from start of line to cursor. Returns the killed text.
    pub fn kill_to_start(&mut self) -> String {
        let killed: String = self.buf[..self.pos].iter().collect();
        self.buf.drain(..self.pos);
        self.pos = 0;
        killed
    }

    /// Kill the word behind the cursor. Returns the killed text.
    pub fn kill_backward_word(&mut self) -> String {
        let old_pos = self.pos;
        self.move_backward_word();
        let killed: String = self.buf[self.pos..old_pos].iter().collect();
        self.buf.drain(self.pos..old_pos);
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
    }

    /// Transpose the two words around the cursor (Alt+T).
    pub fn transpose_words(&mut self) {
        let len = self.buf.len();
        if len == 0 { return; }

        let mut p = self.pos;
        if p == len || !Self::is_word_char(self.buf[p]) {
            while p > 0 && !Self::is_word_char(self.buf[p - 1]) {
                p -= 1;
            }
        }
        if p == 0 { return; }

        // Find end of word2
        let w2e = if self.pos < len && Self::is_word_char(self.buf[self.pos]) {
            let mut e = self.pos;
            while e < len && Self::is_word_char(self.buf[e]) { e += 1; }
            e
        } else {
            p
        };

        // Find start of word2
        let mut w2s = w2e;
        while w2s > 0 && Self::is_word_char(self.buf[w2s - 1]) {
            w2s -= 1;
        }
        if w2s == 0 { return; }

        // Find end of word1
        let mut w1e = w2s;
        while w1e > 0 && !Self::is_word_char(self.buf[w1e - 1]) {
            w1e -= 1;
        }
        if w1e == 0 { return; }

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
    }

    /// Convert the next word to uppercase (Alt+U).
    pub fn upcase_word(&mut self) {
        let len = self.buf.len();
        while self.pos < len && !Self::is_word_char(self.buf[self.pos]) {
            self.pos += 1;
        }
        while self.pos < len && Self::is_word_char(self.buf[self.pos]) {
            self.buf[self.pos] = self.buf[self.pos].to_uppercase().next().unwrap_or(self.buf[self.pos]);
            self.pos += 1;
        }
    }

    /// Convert the next word to lowercase (Alt+L).
    pub fn downcase_word(&mut self) {
        let len = self.buf.len();
        while self.pos < len && !Self::is_word_char(self.buf[self.pos]) {
            self.pos += 1;
        }
        while self.pos < len && Self::is_word_char(self.buf[self.pos]) {
            self.buf[self.pos] = self.buf[self.pos].to_lowercase().next().unwrap_or(self.buf[self.pos]);
            self.pos += 1;
        }
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
                self.buf[self.pos] = self.buf[self.pos].to_uppercase().next().unwrap_or(self.buf[self.pos]);
                first = false;
            } else {
                self.buf[self.pos] = self.buf[self.pos].to_lowercase().next().unwrap_or(self.buf[self.pos]);
            }
            self.pos += 1;
        }
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
        (start, len)
    }

    /// Remove `len` characters starting at `start`. Used by yank_pop to replace yanked text.
    pub fn remove_range(&mut self, start: usize, len: usize) {
        let end = (start + len).min(self.buf.len());
        self.buf.drain(start..end);
        if self.pos > start {
            self.pos = start;
        }
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
            // Keep remaining suggestion, if any
            if i < chars.len() {
                self.suggestion = Some(chars[i..].iter().collect());
            }
        }
    }

    /// Update the autosuggestion based on the current buffer state.
    /// Only suggests when the cursor is at the end of a non-empty buffer.
    fn update_suggestion(&mut self, history: &History) {
        if self.pos == self.buf.len() && !self.buf.is_empty() {
            self.suggestion = history.suggest(&self.buffer());
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
}

impl LineEditor {
    /// Read a line of input from the terminal, handling cursor movement and
    /// editing keys.  Returns `Ok(Some(line))` on Enter, `Ok(None)` on
    /// Ctrl-D with an empty buffer (EOF), or `Ok(Some(""))` on Ctrl-C.
    #[allow(dead_code)] // Used by tests; production code uses read_line_with_completion
    pub fn read_line<T: Terminal>(&mut self, prompt: &str, history: &mut History, term: &mut T) -> io::Result<Option<String>> {
        self.clear();
        term.enable_raw_mode()?;
        let result = self.read_line_loop(prompt, history, term);
        let _ = term.disable_raw_mode();
        result
    }

    fn read_line_loop<T: Terminal>(&mut self, prompt: &str, history: &mut History, term: &mut T) -> io::Result<Option<String>> {
        let prompt_width = prompt.chars().count();
        loop {
            term.flush()?;
            if let Event::Key(key_event) = term.read_event()? {
                match self.handle_key(key_event, history) {
                    KeyAction::Submit => {
                        history.reset_cursor();
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
                        }
                        term.enable_raw_mode()?;
                        term.move_to_column(0)?;
                        term.clear_current_line()?;
                        term.write_str(prompt)?;
                    }
                    KeyAction::TabComplete | KeyAction::Continue => {}
                }
                self.update_suggestion(history);
                self.redraw(term, prompt_width, &[])?;
            }
        }
    }

    /// Redraw the current buffer on screen, positioning the cursor correctly.
    fn redraw<T: Terminal>(&self, term: &mut T, prompt_width: usize, spans: &[ColorSpan]) -> io::Result<()> {
        let col = |n: usize| -> u16 { n.min(u16::MAX as usize) as u16 };
        term.move_to_column(col(prompt_width))?;
        term.clear_until_newline()?;
        if spans.is_empty() {
            // No highlighting: plain write
            term.write_str(&self.buffer())?;
        } else {
            // Highlighted write: iterate chars and apply styles
            let mut current_style = HighlightStyle::Default;
            for (i, ch) in self.buf.iter().enumerate() {
                // Find the style for char at position i
                let new_style = spans.iter()
                    .find(|sp| sp.start <= i && i < sp.end)
                    .map(|sp| sp.style)
                    .unwrap_or(HighlightStyle::Default);
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
        // Draw suggestion in dim text when cursor is at end of buffer
        if let Some(ref suggestion) = self.suggestion
            && self.pos == self.buf.len()
        {
            term.set_dim(true)?;
            term.write_str(suggestion)?;
            term.set_dim(false)?;
        }
        term.move_to_column(col(prompt_width + self.pos))?;
        term.flush()?;
        Ok(())
    }

    /// Map a single key event to a [`KeyAction`], mutating the buffer as needed.
    fn handle_key(&mut self, key: KeyEvent, history: &mut History) -> KeyAction {
        if key.code != KeyCode::Tab {
            self.tab_count = 0;
        }
        match (key.code, key.modifiers) {
            // Ctrl+D — EOF when empty, otherwise delete char at cursor
            (KeyCode::Char('d'), m) if m.contains(KeyModifiers::CONTROL) => {
                if self.is_empty() {
                    KeyAction::Eof
                } else {
                    self.delete();
                    KeyAction::Continue
                }
            }

            // Ctrl+C — interrupt
            (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => KeyAction::Interrupt,

            // Ctrl+B / Left — move cursor left
            (KeyCode::Char('b'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.move_cursor_left();
                KeyAction::Continue
            }
            (KeyCode::Left, _) => {
                self.move_cursor_left();
                KeyAction::Continue
            }

            // Ctrl+F / Right — move cursor right, or accept suggestion at end
            (KeyCode::Char('f'), m) if m.contains(KeyModifiers::CONTROL) => {
                if self.pos == self.buf.len() && self.suggestion.is_some() {
                    self.accept_full_suggestion();
                } else {
                    self.move_cursor_right();
                }
                KeyAction::Continue
            }
            (KeyCode::Right, _) => {
                if self.pos == self.buf.len() && self.suggestion.is_some() {
                    self.accept_full_suggestion();
                } else {
                    self.move_cursor_right();
                }
                KeyAction::Continue
            }

            // Ctrl+A / Home — move to start
            (KeyCode::Char('a'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.move_to_start();
                KeyAction::Continue
            }
            (KeyCode::Home, _) => {
                self.move_to_start();
                KeyAction::Continue
            }

            // Ctrl+E / End — move to end
            (KeyCode::Char('e'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.move_to_end();
                KeyAction::Continue
            }
            (KeyCode::End, _) => {
                self.move_to_end();
                KeyAction::Continue
            }

            // Enter — submit
            (KeyCode::Enter, _) => KeyAction::Submit,

            // Backspace — delete char before cursor
            (KeyCode::Backspace, _) => {
                self.backspace();
                KeyAction::Continue
            }

            // Delete — delete char at cursor
            (KeyCode::Delete, _) => {
                self.delete();
                KeyAction::Continue
            }

            // Tab — trigger completion
            (KeyCode::Tab, _) => {
                self.tab_count += 1;
                KeyAction::TabComplete
            }

            // Alt+F — accept next word from suggestion
            (KeyCode::Char('f'), m) if m.contains(KeyModifiers::ALT) => {
                if self.suggestion.is_some() {
                    self.accept_word_suggestion();
                }
                KeyAction::Continue
            }

            // Printable character (without Ctrl modifier)
            (KeyCode::Char(ch), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.insert_char(ch);
                KeyAction::Continue
            }

            // Ctrl+R — fuzzy history search
            (KeyCode::Char('r'), m) if m.contains(KeyModifiers::CONTROL) => {
                KeyAction::FuzzySearch
            }

            // Up — navigate history backward
            (KeyCode::Up, _) => {
                if let Some(line) = history.navigate_up(&self.buffer()) {
                    self.buf = line.chars().collect();
                    self.pos = self.buf.len();
                }
                self.suggestion = None;
                KeyAction::Continue
            }

            // Down — navigate history forward
            (KeyCode::Down, _) => {
                if let Some(line) = history.navigate_down() {
                    self.buf = line.chars().collect();
                    self.pos = self.buf.len();
                }
                self.suggestion = None;
                KeyAction::Continue
            }

            // Everything else — ignore
            _ => KeyAction::Continue,
        }
    }

    // ── Tab completion support ─────────────────────────────────────────

    /// Read a line of input with Tab completion support.
    ///
    /// Behaves identically to [`read_line`] but also handles Tab key events
    /// by invoking the completion engine.
    pub fn read_line_with_completion<T: Terminal>(
        &mut self,
        prompt: &str,
        history: &mut History,
        term: &mut T,
        ctx: &CompletionContext,
        scanner: &mut HighlightScanner,
        checker_env: &CheckerEnv<'_>,
        accumulated: &str,
    ) -> io::Result<Option<String>> {
        self.clear();
        term.enable_raw_mode()?;
        let result = self.read_line_loop_with_completion(prompt, history, term, ctx, scanner, checker_env, accumulated);
        let _ = term.disable_raw_mode();
        result
    }

    fn read_line_loop_with_completion<T: Terminal>(
        &mut self,
        prompt: &str,
        history: &mut History,
        term: &mut T,
        ctx: &CompletionContext,
        scanner: &mut HighlightScanner,
        checker_env: &CheckerEnv<'_>,
        accumulated: &str,
    ) -> io::Result<Option<String>> {
        let prompt_width = prompt.chars().count();
        loop {
            term.flush()?;
            if let Event::Key(key_event) = term.read_event()? {
                match self.handle_key(key_event, history) {
                    KeyAction::Submit => {
                        history.reset_cursor();
                        term.reset_style()?;
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
                        }
                        term.enable_raw_mode()?;
                        term.move_to_column(0)?;
                        term.clear_current_line()?;
                        term.write_str(prompt)?;
                    }
                    KeyAction::TabComplete => {
                        term.reset_style()?;
                        self.handle_tab_complete(term, prompt, ctx)?;
                    }
                    KeyAction::Continue => {}
                }
                self.update_suggestion(history);
                let spans = scanner.scan(accumulated, &self.buf, checker_env);
                self.redraw(term, prompt_width, &spans)?;
            }
        }
    }

    fn handle_tab_complete<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        ctx: &CompletionContext,
    ) -> io::Result<()> {
        let result = completion::complete(&self.buffer(), self.pos, ctx);

        if result.candidates.is_empty() {
            return Ok(());
        }

        if self.tab_count == 1 {
            if result.candidates.len() == 1 {
                // Single candidate: replace word with dir_prefix + candidate
                let candidate = &result.candidates[0];
                let is_dir = candidate.ends_with('/');
                let mut replacement = format!("{}{}", result.dir_prefix, candidate);
                if !is_dir {
                    replacement.push(' ');
                }
                self.replace_word(result.word_start, &replacement);
            } else {
                // Multiple candidates: replace with common prefix if longer
                let current_word = &self.buffer()[result.word_start..self.pos];
                let new_word = format!("{}{}", result.dir_prefix, result.common_prefix);
                if new_word.len() > current_word.len() {
                    self.replace_word(result.word_start, &new_word);
                }
            }
        } else if self.tab_count >= 2 && result.candidates.len() >= 2 {
            // Show interactive completion UI
            self.suggestion = None;
            term.disable_raw_mode()?;
            let selected = CompletionUI::run(&result.candidates, term)?;
            if let Some(sel) = selected {
                let is_dir = sel.ends_with('/');
                let mut replacement = format!("{}{}", result.dir_prefix, sel);
                if !is_dir {
                    replacement.push(' ');
                }
                self.replace_word(result.word_start, &replacement);
            }
            term.enable_raw_mode()?;
            term.move_to_column(0)?;
            term.clear_current_line()?;
            term.write_str(prompt)?;
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
    }
}
