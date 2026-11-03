//! vi insert mode: the keys handled before the shared keymap (ESC,
//! Ctrl+V literal input, Ctrl+W word erase, `R` overwrite), leaving
//! insert mode, and the `.` insert replay.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{KeyAction, LineEditor};
use crate::interactive::history::History;
use crate::interactive::vi::{self, ViFlavor, ViMode};
use crate::interactive::vim::RegisterKind;

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

impl LineEditor {
    /// Handle the vi insert-mode keys that differ from the shared
    /// (emacs-flavored) keymap. Returns `None` to let the key fall
    /// through to the keymap — everything else keeps the regular
    /// insert-mode bindings.
    pub(super) fn handle_vi_insert_key(
        &mut self,
        key: KeyEvent,
        history: &mut History,
    ) -> Option<KeyAction> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        // Ctrl+V literal-next (POSIX vi insert mode): the next key is
        // inserted verbatim, ESC and control keys included — so it must
        // run before the ESC handling.
        if self.vi_literal_next {
            self.vi_literal_next = false;
            if let Some(ch) = literal_char_for(key) {
                self.vi_insert_or_overwrite(ch);
            }
            return Some(KeyAction::Continue);
        }
        if ctrl && key.code == KeyCode::Char('v') {
            self.vi_literal_next = true;
            return Some(KeyAction::Continue);
        }
        if key.code == KeyCode::Esc {
            self.vi_leave_insert_mode();
            return Some(KeyAction::Continue);
        }
        // POSIX vi insert-mode Ctrl+W: word-erase bounded by the current
        // logical line's start (the emacs backward-kill crosses '\n' and
        // would join lines).
        if ctrl && key.code == KeyCode::Char('w') {
            self.vi_insert_werase();
            return Some(KeyAction::Continue);
        }
        // A fast ESC-then-key sequence arrives as Alt+key (terminal
        // ESC-prefix encoding): leave insert mode and run the key as a
        // command-mode command, exactly like typing them slowly. This
        // shadows the emacs Alt-chords (Alt+f/b/d…) in vi insert mode —
        // vi ESC correctness wins; the plain Ctrl bindings all remain.
        // Alt+Ctrl+key is a fast ESC before a control key (e.g. ESC then
        // Ctrl-X): the control modifier is preserved into command mode.
        if let KeyCode::Char(c) = key.code
            && key.modifiers.contains(KeyModifiers::ALT)
        {
            self.vi_leave_insert_mode();
            return Some(self.handle_vi_command_key(
                KeyEvent::new(
                    KeyCode::Char(c),
                    key.modifiers.difference(KeyModifiers::ALT),
                ),
                history,
            ));
        }
        // `R` replace mode: a plain character overwrites the character
        // under the cursor instead of shifting it right. Delete first,
        // then fall through to the normal insert path (which handles
        // undo bookkeeping).
        if self.vi.replace_overwrite
            && let KeyCode::Char(_) = key.code
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            && self.pos < vi::line_end(&self.buf, self.pos)
        {
            self.delete();
        }
        None
    }

    /// Insert `ch` at the cursor; in `R` replace mode it overwrites the
    /// character under the cursor (never the line's '\n').
    fn vi_insert_or_overwrite(&mut self, ch: char) {
        if self.vi.replace_overwrite && self.pos < vi::line_end(&self.buf, self.pos) {
            self.delete();
        }
        self.insert_char(ch);
    }

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
        self.record_kill(&killed, false, RegisterKind::Charwise);
        self.buf.drain(start..self.pos);
        self.pos = start;
        self.invalidate_width_cache();
    }

    /// Switch from insert to command mode: end `R` overwrite and leave
    /// the cursor on the last inserted character (one left of the
    /// insert point), clamped within the logical line.
    fn vi_return_to_command(&mut self) {
        self.vi.mode = ViMode::Command;
        self.vi.replace_overwrite = false;
        let ls = vi::line_start(&self.buf, self.pos);
        if self.pos > ls {
            self.pos -= 1;
        }
        self.clamp_vi_command_pos();
    }

    /// Leave vi insert mode (ESC): capture the session's inserted text
    /// for `.` (approximation: from the insert entry point to the
    /// cursor; cursor movement inside the session degrades it
    /// gracefully), then enter command mode. No undo boundary save
    /// here: vi undo is session-granular and the pre-session state was
    /// saved on entry.
    fn vi_leave_insert_mode(&mut self) {
        if let Some(start) = self.vi_insert_start.take() {
            if start <= self.pos {
                self.vi_last_insert = self.buf[start..self.pos].iter().collect();
            } else {
                self.vi_last_insert.clear();
            }
        }
        // vim §7.2: leaving Insert commits the staged session as one
        // unit iff the buffer changed; a no-op session commits nothing
        // and preserves redo.
        if self.vi.flavor == ViFlavor::Vim
            && let Some((staged_buf, staged_pos)) = self.vim_undo_staged.take()
            && self.buf != staged_buf
        {
            self.vim_commit_undo(&staged_buf, staged_pos);
        }
        self.last_was_insert = false;
        self.vi_literal_next = false;
        self.vi_return_to_command();
        self.suggestion = None;
    }

    /// Replay the last insert session's text (for `.`), honoring `R`
    /// overwrite semantics, then leave insert mode like ESC does.
    pub(super) fn vi_replay_insert(&mut self) {
        let text: Vec<char> = self.vi_last_insert.chars().collect();
        for ch in text {
            self.vi_insert_or_overwrite(ch);
        }
        self.vi_return_to_command();
    }
}
