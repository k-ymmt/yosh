//! Key dispatch and the keymap-driven (emacs-flavored) edit actions,
//! which vi insert mode shares.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{KeyAction, LineEditor, YankState};
use crate::interactive::edit_action::EditAction;
use crate::interactive::history::History;
use crate::interactive::keymap::BufferState;
use crate::interactive::vi::{self, ViMode};
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
                    // correctness wins; the plain Ctrl bindings all
                    // remain. Alt+Ctrl+key is a fast ESC before a
                    // control key (e.g. ESC then Ctrl-X): the control
                    // modifier is preserved into command mode.
                    if let KeyCode::Char(c) = key.code
                        && key.modifiers.contains(KeyModifiers::ALT)
                    {
                        self.vi_leave_insert_mode();
                        return self.handle_vi_command_key(
                            KeyEvent::new(
                                KeyCode::Char(c),
                                key.modifiers.difference(KeyModifiers::ALT),
                            ),
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
                self.record_kill(&killed, consecutive_kill, RegisterKind::Charwise);
                KeyAction::Continue
            }
            EditAction::KillToStart => {
                let killed = self.kill_to_start();
                self.record_kill_prepend(&killed, consecutive_kill, RegisterKind::Charwise);
                KeyAction::Continue
            }
            EditAction::KillBackwardWord => {
                for _ in 0..count {
                    let killed = self.kill_backward_word();
                    self.record_kill_prepend(&killed, consecutive_kill, RegisterKind::Charwise);
                }
                KeyAction::Continue
            }
            EditAction::KillForwardWord => {
                for _ in 0..count {
                    let killed = self.kill_forward_word();
                    self.record_kill(&killed, consecutive_kill, RegisterKind::Charwise);
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
}
