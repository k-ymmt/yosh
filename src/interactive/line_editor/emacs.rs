//! Key dispatch and the keymap-driven (emacs-flavored) edit actions,
//! which vi insert mode shares.

use crossterm::event::KeyEvent;

use super::{KeyAction, LineEditor};
use crate::interactive::edit_action::EditAction;
use crate::interactive::history::History;
use crate::interactive::keymap::BufferState;
use crate::interactive::vi::ViMode;
use crate::interactive::vim::RegisterKind;

impl LineEditor {
    /// Map a single key event to a [`KeyAction`], mutating the buffer as needed.
    pub(super) fn handle_key(&mut self, key: KeyEvent, history: &mut History) -> KeyAction {
        if self.edit_mode.is_vi_family() {
            if self.vi_search_input.is_some() {
                return self.handle_vi_search_key(key, history);
            }
            match self.vi.mode {
                ViMode::Command => return self.handle_vi_command_key(key, history),
                ViMode::Visual { .. } => return self.handle_vim_visual_key(key),
                ViMode::Insert => {
                    if let Some(action) = self.handle_vi_insert_key(key, history) {
                        return action;
                    }
                }
            }
        }
        self.handle_keymap_key(key, history)
    }

    /// Resolve a key through the keymap and run the resulting
    /// [`EditAction`]: emacs mode, and vi insert mode for every key it
    /// does not handle itself.
    fn handle_keymap_key(&mut self, key: KeyEvent, history: &mut History) -> KeyAction {
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

        self.save_undo_before(action);

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

    /// Save the undo snapshot `action` needs before it runs.
    fn save_undo_before(&mut self, action: EditAction) {
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
        let vi_insert = self.edit_mode.is_vi_family();
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
    }

    /// Run a buffer edit `count` times (the numeric argument) and keep
    /// reading.
    fn repeat(&mut self, count: u32, edit: fn(&mut Self)) -> KeyAction {
        for _ in 0..count {
            edit(self);
        }
        KeyAction::Continue
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
            EditAction::MoveBackward => self.repeat(count, Self::move_cursor_left),
            EditAction::MoveForward => self.repeat(count, Self::move_cursor_right),
            EditAction::MoveToStart => {
                self.move_to_start();
                KeyAction::Continue
            }
            EditAction::MoveToEnd => {
                self.move_to_end();
                KeyAction::Continue
            }
            EditAction::MoveBackwardWord => self.repeat(count, Self::move_backward_word),
            EditAction::MoveForwardWord => self.repeat(count, Self::move_forward_word),
            EditAction::DeleteBackward => self.repeat(count, Self::backspace),
            EditAction::DeleteForward => self.repeat(count, Self::delete),
            EditAction::KillToEnd => {
                let killed = self.kill_to_end();
                self.record_kill(&killed, consecutive_kill, RegisterKind::Charwise);
                KeyAction::Continue
            }
            EditAction::KillToStart => {
                let killed = self.kill_to_start();
                self.record_kill_prepend(&killed, consecutive_kill, RegisterKind::Charwise);
                KeyAction::Continue
            }
            // With a numeric argument the words killed by ONE command form
            // one kill-ring entry (readline: `M-3 C-w` then `C-y` restores
            // all three words), so every kill after the first non-empty
            // one merges into it.
            EditAction::KillBackwardWord => {
                let mut merge = consecutive_kill;
                for _ in 0..count {
                    let killed = self.kill_backward_word();
                    self.record_kill_prepend(&killed, merge, RegisterKind::Charwise);
                    merge |= !killed.is_empty();
                }
                KeyAction::Continue
            }
            EditAction::KillForwardWord => {
                let mut merge = consecutive_kill;
                for _ in 0..count {
                    let killed = self.kill_forward_word();
                    self.record_kill(&killed, merge, RegisterKind::Charwise);
                    merge |= !killed.is_empty();
                }
                KeyAction::Continue
            }
            EditAction::Yank => {
                self.yank();
                KeyAction::Continue
            }
            EditAction::YankPop => {
                self.yank_pop();
                KeyAction::Continue
            }
            EditAction::TransposeChars => self.repeat(count, Self::transpose_chars),
            EditAction::TransposeWords => self.repeat(count, Self::transpose_words),
            EditAction::UpcaseWord => self.repeat(count, Self::upcase_word),
            EditAction::DowncaseWord => self.repeat(count, Self::downcase_word),
            EditAction::CapitalizeWord => self.repeat(count, Self::capitalize_word),
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
                        self.replace_buffer(line);
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
                        self.replace_buffer(line);
                    }
                }
                self.suggestion = None;
                KeyAction::Continue
            }
            EditAction::Noop => KeyAction::Continue,
        }
    }
}
