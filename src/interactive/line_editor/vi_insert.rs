//! vi insert-mode helpers: word erase, leaving insert mode, `.`
//! insert replay and Ctrl+V literal input.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::LineEditor;
use crate::interactive::vi::{self, ViFlavor, ViMode};
use crate::interactive::vim::RegisterKind;

impl LineEditor {
    /// POSIX vi insert-mode Ctrl+W: delete back to the preceding word
    /// boundary — "the closer of the beginning of the line or the first
    /// non-blank after a blank" — never crossing the logical line start.
    pub(super) fn vi_insert_werase(&mut self) {
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

    /// Leave vi insert mode (ESC): capture the session's inserted text
    /// for `.` (approximation: from the insert entry point to the
    /// cursor; cursor movement inside the session degrades it
    /// gracefully), then enter command mode. vi leaves the cursor on
    /// the last inserted character (one left of the insert point),
    /// clamped within the logical line. No undo boundary save here: vi
    /// undo is session-granular and the pre-session state was saved on
    /// entry.
    pub(super) fn vi_leave_insert_mode(&mut self) {
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

    /// The literal character a key event inserts under Ctrl+V.
    pub(super) fn literal_char_for(key: KeyEvent) -> Option<char> {
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

    /// Replay the last insert session's text (for `.`), honoring `R`
    /// overwrite semantics, then leave insert mode like ESC does.
    pub(super) fn vi_replay_insert(&mut self) {
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
}
