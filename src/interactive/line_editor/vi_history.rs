//! vi history navigation, recall and `/` / `?` search.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{KeyAction, LineEditor};
use crate::interactive::history::History;
use crate::interactive::vi::{SearchDir, ViFlavor};

impl LineEditor {
    /// Recall a history navigation result into the buffer, vi-style:
    /// cursor on the first character (POSIX), baseline updated for `U`.
    fn vi_recall(&mut self, line: String) {
        self.buf = line.chars().collect();
        self.pos = 0;
        self.vi_line_base = self.buf.clone();
        self.suggestion = None;
        self.invalidate_width_cache();
        self.clamp_vi_command_pos();
        // vim §7.1: recalling a history entry resets the undo history —
        // both stacks clear and a fresh base starts (the line-editor
        // analog of Vim's per-buffer undo). `u` right after a recall
        // bells; `U` still restores the as-recalled state.
        if self.vi.flavor == ViFlavor::Vim {
            self.undo.clear();
            self.vim_undo_staged = None;
        }
    }

    /// `[count]G`: no count = oldest entry; count n = history number n
    /// (POSIX vi). Numbers are stable across HISTSIZE trimming, so map
    /// through the history base.
    pub(super) fn vi_history_goto(&mut self, count: u32, history: &mut History) {
        let idx = if count == 0 {
            if history.entries().is_empty() {
                self.pending_bell = true;
                return;
            }
            0
        } else {
            match history.index_of_number(count as usize) {
                Some(idx) => idx,
                None => {
                    self.pending_bell = true;
                    return;
                }
            }
        };
        let current = self.buffer();
        if let Some(line) = history.navigate_to(idx, &current).map(str::to_string) {
            self.vi_recall(line);
        }
    }

    /// `n` / `N`: repeat the last search in its own (or, `reverse`, the
    /// opposite) direction.
    pub(super) fn vi_search_again(&mut self, reverse: bool, history: &mut History) {
        match self.vi_last_search.clone() {
            Some((dir, pattern)) => {
                let dir = if reverse { dir.reversed() } else { dir };
                if !self.vi_history_search(dir, &pattern, history) {
                    self.pending_bell = true;
                }
            }
            None => self.pending_bell = true,
        }
    }

    /// vi `k` / `j`: move within a multiline buffer first (matching the
    /// emacs Up/Down behavior), then navigate history.
    pub(super) fn vi_history_step(&mut self, prev: bool, count: u32, history: &mut History) {
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
    pub(super) fn handle_vi_search_key(
        &mut self,
        key: KeyEvent,
        history: &mut History,
    ) -> KeyAction {
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
}
