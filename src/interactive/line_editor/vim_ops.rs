//! vim flavor (`set -o vim`): register-kind-aware linewise operations
//! and puts, shared by command mode and VISUAL mode.

use super::LineEditor;
use super::vi_edit::put_reps;
use crate::interactive::vi::{self, OpKind, ViMode};
use crate::interactive::vim::{self, RegisterKind};

impl LineEditor {
    /// Delete the whole logical lines `[ls, le)` together with their
    /// separator (§8.2 delete range). Cursor: first non-blank of the
    /// line now at the deleted position (or the new last line when the
    /// tail was deleted; 0 on an empty buffer).
    pub(super) fn vim_delete_lines(&mut self, ls: usize, le: usize) {
        let (start, end) = vim::linewise_delete_range(&self.buf, ls, le);
        self.buf.drain(start..end);
        self.pos = start.min(self.buf.len().saturating_sub(1));
        self.move_to_first_non_blank();
        self.invalidate_width_cache();
    }

    /// Change the whole logical lines `[ls, le)`: the change range
    /// consumes no separator, so the lines collapse to one empty logical
    /// line and Insert begins on it (oracle-verified Vim `cc`).
    pub(super) fn vim_change_lines(&mut self, ls: usize, le: usize) {
        self.buf.drain(ls..le);
        self.pos = ls;
        self.invalidate_width_cache();
        self.vi.mode = ViMode::Insert;
    }

    /// vim `[count]dd` / `cc` / `yy` / `Y`: whole-line operation on
    /// `count` logical lines with linewise register semantics (§8.2).
    pub(super) fn vim_op_line(&mut self, op: OpKind, count: u32) {
        let (ls, le) = vim::linewise_target(&self.buf, self.pos, count);
        let text = vim::linewise_register_text(&self.buf, ls, le);
        self.record_kill(&text, false, RegisterKind::Linewise);
        match op {
            OpKind::Yank => {}
            OpKind::Delete => {
                self.vim_delete_lines(ls, le);
                self.clamp_vi_command_pos();
            }
            OpKind::Change => self.vim_change_lines(ls, le),
        }
    }

    /// vim `p`/`P`: put the typed unnamed register (§8.3). Charwise
    /// puts splice at the cursor like POSIX vi; a linewise register
    /// opens new line(s) below (`p`) or above (`P`) the cursor's
    /// logical line, cursor to the first non-blank of the first pasted
    /// line.
    pub(super) fn vim_put(&mut self, after: bool, count: u32) {
        let reg = self.unnamed_register.clone();
        if reg.text.is_empty() {
            self.pending_bell = true;
            return;
        }
        let chars: Vec<char> = reg.text.chars().collect();
        let reps = put_reps(count, chars.len());
        match reg.kind {
            RegisterKind::Charwise => {
                let at = self.put_position(after);
                self.pos = self.put_repeated(at, &chars, reps) - 1;
            }
            RegisterKind::Linewise => {
                // The pasted block is the register text repeated
                // `count` times, then treated as one linewise text T
                // (always ending in '\n').
                let mut block: Vec<char> = Vec::with_capacity(chars.len() * reps);
                for _ in 0..reps {
                    block.extend_from_slice(&chars);
                }
                if after {
                    // Insert '\n' + T[..len-1] at the cursor line's end.
                    let le = vi::line_end(&self.buf, self.pos);
                    block.pop();
                    block.insert(0, '\n');
                    self.buf.splice(le..le, block);
                    self.pos = le + 1;
                } else {
                    // Insert T at the cursor line's start.
                    let ls = vi::line_start(&self.buf, self.pos);
                    self.buf.splice(ls..ls, block);
                    self.pos = ls;
                }
                self.move_to_first_non_blank();
            }
        }
        self.invalidate_width_cache();
        self.clamp_vi_command_pos();
    }
}
