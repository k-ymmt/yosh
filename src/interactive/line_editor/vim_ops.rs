//! vim flavor (`set -o vim`): register-kind-aware linewise operations,
//! puts, and VISUAL mode.

use crossterm::event::KeyEvent;

use super::{KeyAction, LineEditor};
use crate::interactive::edit_action::EditAction;
use crate::interactive::vi::{
    self, OpKind, ViMode, ViMotion, VisualCmd, VisualKind, VisualOutcome,
};
use crate::interactive::vim::{self, RegisterKind};

impl LineEditor {
    /// vim `[count]dd` / `cc` / `yy` / `Y`: whole-line operation on
    /// `count` logical lines with linewise register semantics (§8.2).
    pub(super) fn vim_op_line(&mut self, op: OpKind, count: u32) -> KeyAction {
        let (ls, le) = vim::linewise_target(&self.buf, self.pos, count);
        let text = vim::linewise_register_text(&self.buf, ls, le);
        self.record_kill(&text, false, RegisterKind::Linewise);
        match op {
            OpKind::Yank => {}
            OpKind::Delete => {
                let (start, end) = vim::linewise_delete_range(&self.buf, ls, le);
                self.buf.drain(start..end);
                // Cursor: first non-blank of the line now at the deleted
                // position (or the new last line when the tail was
                // deleted; 0 on an empty buffer).
                self.pos = start.min(self.buf.len().saturating_sub(1));
                if let Some(fnb) = vi::motion_move(&self.buf, self.pos, ViMotion::FirstNonBlank, 1)
                {
                    self.pos = fnb;
                }
                self.invalidate_width_cache();
                self.clamp_vi_command_pos();
            }
            OpKind::Change => {
                // The change range consumes no separator: the selected
                // lines collapse to one empty logical line and Insert
                // begins on it (oracle-verified Vim `cc`).
                self.buf.drain(ls..le);
                self.pos = ls;
                self.invalidate_width_cache();
                self.vi.mode = ViMode::Insert;
            }
        }
        KeyAction::Continue
    }

    /// Handle a key while in VISUAL mode (vim flavor).
    pub(super) fn handle_vim_visual_key(&mut self, key: KeyEvent) -> KeyAction {
        match self.vi.resolve_visual_key(key) {
            VisualOutcome::Pending => KeyAction::Continue,
            VisualOutcome::Cmd(cmd, count) => {
                // §7.2: one undo unit per VISUAL operation; an operator
                // that enters Insert (c/s/C/S/R) stages its snapshot
                // instead, spanning the deletion and the typed text.
                let pre = (self.buf.clone(), self.pos);
                let action = self.execute_visual_cmd(cmd, count);
                if self.vi.mode == ViMode::Insert {
                    self.vim_undo_staged = Some(pre);
                } else if self.buf != pre.0 {
                    self.vim_commit_undo(&pre.0, pre.1);
                }
                // Same rationale as command mode: end any emacs-side
                // kill run / yank-pop state.
                self.last_action = EditAction::Noop;
                self.yank_state = None;
                action
            }
        }
    }

    /// Leave VISUAL mode for Command mode.
    fn vim_exit_visual(&mut self) {
        self.vi.mode = ViMode::Command;
        self.clamp_vi_command_pos();
    }

    /// The whole logical lines touched by the current selection, as a
    /// `(line_start, line_end)` char range (no separators consumed).
    fn vim_visual_line_span(&self, anchor: usize) -> (usize, usize) {
        vim::visual_selection(&self.buf, anchor, self.pos, VisualKind::Line)
    }

    fn execute_visual_cmd(&mut self, cmd: VisualCmd, count: u32) -> KeyAction {
        let ViMode::Visual { kind, anchor } = self.vi.mode else {
            return self.bell();
        };
        match cmd {
            VisualCmd::Bell => self.bell(),
            VisualCmd::Move(motion) => {
                match vi::motion_move(&self.buf, self.pos, motion, count) {
                    Some(p) => self.pos = p,
                    None => self.pending_bell = true,
                }
                KeyAction::Continue
            }
            VisualCmd::LineUp | VisualCmd::LineDown => {
                // Pure buffer motion: at the buffer edge the cursor
                // simply stops (bell-free, matching Vim).
                for _ in 0..count.max(1) {
                    if cmd == VisualCmd::LineUp {
                        if self.cursor_line_index() == 0 {
                            break;
                        }
                        self.move_cursor_up();
                    } else {
                        if self.cursor_line_index() + 1 >= self.line_count() {
                            break;
                        }
                        self.move_cursor_down();
                    }
                    self.clamp_vi_command_pos();
                }
                KeyAction::Continue
            }
            VisualCmd::Delete | VisualCmd::Change | VisualCmd::Yank => match kind {
                VisualKind::Char => self.vim_visual_charwise_op(cmd, anchor),
                VisualKind::Line => self.vim_visual_linewise_op(cmd, anchor),
            },
            VisualCmd::DeleteLines => self.vim_visual_linewise_op(VisualCmd::Delete, anchor),
            VisualCmd::ChangeLines => self.vim_visual_linewise_op(VisualCmd::Change, anchor),
            VisualCmd::YankLines => self.vim_visual_linewise_op(VisualCmd::Yank, anchor),
            VisualCmd::Put { before } => self.vim_visual_put(kind, anchor, before),
            VisualCmd::ReplaceChar(c) => {
                let (s, e) = vim::visual_selection(&self.buf, anchor, self.pos, kind);
                if s < e {
                    for i in s..e {
                        // '\n' separators are preserved (oracle-verified:
                        // charwise rX over ab\ncd keeps the line break).
                        if self.buf[i] != '\n' {
                            self.buf[i] = c;
                        }
                    }
                    self.invalidate_width_cache();
                }
                self.pos = s;
                self.vim_exit_visual();
                KeyAction::Continue
            }
            VisualCmd::ToggleCase => {
                let (s, e) = vim::visual_selection(&self.buf, anchor, self.pos, kind);
                if s < e {
                    for i in s..e {
                        let ch = self.buf[i];
                        self.buf[i] = if ch.is_uppercase() {
                            ch.to_lowercase().next().unwrap_or(ch)
                        } else if ch.is_lowercase() {
                            ch.to_uppercase().next().unwrap_or(ch)
                        } else {
                            ch
                        };
                    }
                    self.invalidate_width_cache();
                }
                self.pos = s;
                self.vim_exit_visual();
                KeyAction::Continue
            }
            VisualCmd::TextObject { around, obj } => {
                match vim::text_object_range(&self.buf, self.pos, obj, around, count) {
                    Some((s, e)) if s < e => {
                        if anchor == self.pos {
                            // Single-char selection: select the object.
                            self.vi.mode = ViMode::Visual { kind, anchor: s };
                            self.pos = e - 1;
                        } else if self.pos >= anchor {
                            // Larger forward selection: extend its end
                            // (oracle-verified: 0vwiwd on `one two
                            // three` leaves ` three`).
                            self.pos = self.pos.max(e - 1);
                        } else {
                            self.pos = self.pos.min(s);
                        }
                    }
                    // No object / empty inner object: bell, selection
                    // unchanged.
                    _ => self.pending_bell = true,
                }
                KeyAction::Continue
            }
            VisualCmd::SwapEnds => {
                self.vi.mode = ViMode::Visual {
                    kind,
                    anchor: self.pos.min(self.buf.len().saturating_sub(1)),
                };
                self.pos = anchor.min(self.buf.len().saturating_sub(1));
                KeyAction::Continue
            }
            VisualCmd::ToggleKind(k) => {
                if k == kind {
                    self.vim_exit_visual();
                } else {
                    // Kind switch preserves the anchor (oracle-verified).
                    self.vi.mode = ViMode::Visual { kind: k, anchor };
                }
                KeyAction::Continue
            }
            VisualCmd::Exit => {
                self.vim_exit_visual();
                KeyAction::Continue
            }
            VisualCmd::Submit => {
                // Shell semantics: submit the whole line, selection
                // discarded.
                self.vim_exit_visual();
                KeyAction::Submit
            }
            VisualCmd::Eof => {
                if self.is_empty() {
                    KeyAction::Eof
                } else {
                    self.bell()
                }
            }
            VisualCmd::ClearScreen => KeyAction::ClearScreen,
        }
    }

    /// Charwise VISUAL `d`/`c`/`y`.
    fn vim_visual_charwise_op(&mut self, cmd: VisualCmd, anchor: usize) -> KeyAction {
        let (s, e) = vim::visual_selection(&self.buf, anchor, self.pos, VisualKind::Char);
        let text: String = self.buf[s..e].iter().collect();
        if !text.is_empty() {
            self.record_kill(&text, false, RegisterKind::Charwise);
        }
        match cmd {
            VisualCmd::Yank => {
                self.pos = s;
                self.vim_exit_visual();
            }
            VisualCmd::Delete | VisualCmd::Change => {
                if s < e {
                    self.buf.drain(s..e);
                    self.invalidate_width_cache();
                }
                self.pos = s;
                if cmd == VisualCmd::Change {
                    // c/s enter Insert even on an empty selection.
                    self.vi.mode = ViMode::Insert;
                } else {
                    self.vim_exit_visual();
                }
            }
            _ => unreachable!("charwise op is d/c/y"),
        }
        KeyAction::Continue
    }

    /// Linewise VISUAL operation (`d`/`c`/`y` on a linewise selection,
    /// and the `D`/`C`/`S`/`R`/`Y` line variants of any selection).
    fn vim_visual_linewise_op(&mut self, cmd: VisualCmd, anchor: usize) -> KeyAction {
        let (ls, le) = self.vim_visual_line_span(anchor);
        let text = vim::linewise_register_text(&self.buf, ls, le);
        self.record_kill(&text, false, RegisterKind::Linewise);
        match cmd {
            VisualCmd::Yank => {
                self.pos = anchor.min(self.pos).min(self.buf.len().saturating_sub(1));
                self.vim_exit_visual();
            }
            VisualCmd::Delete => {
                let (start, end) = vim::linewise_delete_range(&self.buf, ls, le);
                self.buf.drain(start..end);
                self.pos = start.min(self.buf.len().saturating_sub(1));
                if let Some(fnb) = vi::motion_move(&self.buf, self.pos, ViMotion::FirstNonBlank, 1)
                {
                    self.pos = fnb;
                }
                self.invalidate_width_cache();
                self.vim_exit_visual();
            }
            VisualCmd::Change => {
                // §8.2 change range: the lines collapse to one empty
                // logical line, Insert begins on it.
                self.buf.drain(ls..le);
                self.pos = ls;
                self.invalidate_width_cache();
                self.vi.mode = ViMode::Insert;
            }
            _ => unreachable!("linewise op is d/c/y"),
        }
        KeyAction::Continue
    }

    /// VISUAL `p`/`P` (§8.3): the selection is deleted, the register
    /// contents replace it. With `p` the deleted text swaps into the
    /// unnamed register (selection's kind); with `P` the register — and
    /// the kill ring, to keep the front-entry invariant — are left
    /// unchanged.
    fn vim_visual_put(&mut self, kind: VisualKind, anchor: usize, before: bool) -> KeyAction {
        let reg = self.unnamed_register.clone();
        if reg.text.is_empty() {
            return self.bell();
        }
        // Delete the selection. Linewise selections use the change
        // range (no separator consumed) so the pasted block lands on
        // its own line slot between the neighbors.
        let (s, e) = vim::visual_selection(&self.buf, anchor, self.pos, kind);
        let deleted: String = self.buf[s..e].iter().collect();
        if !before && !deleted.is_empty() {
            let deleted_kind = match kind {
                VisualKind::Char => RegisterKind::Charwise,
                VisualKind::Line => RegisterKind::Linewise,
            };
            let reg_text = match kind {
                VisualKind::Char => deleted.clone(),
                VisualKind::Line => format!("{}\n", deleted),
            };
            self.record_kill(&reg_text, false, deleted_kind);
        }
        self.buf.drain(s..e);
        let reg_chars: Vec<char> = reg.text.chars().collect();
        match (reg.kind, kind) {
            (RegisterKind::Charwise, VisualKind::Char) => {
                // Splice in place; cursor on the last pasted char.
                self.buf.splice(s..s, reg_chars.iter().copied());
                self.pos = s + reg_chars.len() - 1;
            }
            (RegisterKind::Charwise, VisualKind::Line) => {
                // Charwise text becomes its own new line, occupying the
                // emptied line slot.
                self.buf.splice(s..s, reg_chars.iter().copied());
                self.pos = s;
                if let Some(fnb) = vi::motion_move(&self.buf, self.pos, ViMotion::FirstNonBlank, 1)
                {
                    self.pos = fnb;
                }
            }
            (RegisterKind::Linewise, VisualKind::Char) => {
                // Complete line(s) at the deletion point, splitting the
                // surrounding line: '\n' + T (T ends in '\n' already
                // separating the following text... the trailing '\n'
                // of T becomes the break before the split-off tail).
                let mut ins: Vec<char> = Vec::with_capacity(reg_chars.len() + 1);
                ins.push('\n');
                ins.extend(&reg_chars);
                // Drop the trailing separator when the deletion point
                // is already at a line boundary end (nothing follows on
                // the line) to avoid a dangling empty line? Vim keeps
                // the split; keep it simple and keep the split.
                let first = s + 1;
                self.buf.splice(s..s, ins);
                self.pos = first;
                if let Some(fnb) = vi::motion_move(&self.buf, self.pos, ViMotion::FirstNonBlank, 1)
                {
                    self.pos = fnb;
                }
            }
            (RegisterKind::Linewise, VisualKind::Line) => {
                // Replace the lines: T minus its trailing '\n' fills
                // the emptied line slot.
                let ins = &reg_chars[..reg_chars.len().saturating_sub(1)];
                self.buf.splice(s..s, ins.iter().copied());
                self.pos = s;
                if let Some(fnb) = vi::motion_move(&self.buf, self.pos, ViMotion::FirstNonBlank, 1)
                {
                    self.pos = fnb;
                }
            }
        }
        self.invalidate_width_cache();
        self.vim_exit_visual();
        KeyAction::Continue
    }

    /// vim `p`/`P`: put the typed unnamed register (§8.3). Charwise
    /// puts splice at the cursor like POSIX vi; a linewise register
    /// opens new line(s) below (`p`) or above (`P`) the cursor's
    /// logical line, cursor to the first non-blank of the first pasted
    /// line.
    pub(super) fn vim_put(&mut self, after: bool, count: u32) -> KeyAction {
        let reg = self.unnamed_register.clone();
        if reg.text.is_empty() {
            return self.bell();
        }
        let chars: Vec<char> = reg.text.chars().collect();
        // Bound the total inserted text (count × register length) so a
        // huge count cannot balloon the buffer.
        let reps = (count.max(1) as usize)
            .min(1_000_000 / chars.len().max(1))
            .max(1);
        match reg.kind {
            RegisterKind::Charwise => {
                let le = vi::line_end(&self.buf, self.pos);
                let at = if after {
                    (self.pos + 1).min(le)
                } else {
                    self.pos
                };
                let mut insert_pos = at;
                for _ in 0..reps {
                    for &c in &chars {
                        self.buf.insert(insert_pos, c);
                        insert_pos += 1;
                    }
                }
                self.pos = insert_pos - 1;
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
                if let Some(fnb) = vi::motion_move(&self.buf, self.pos, ViMotion::FirstNonBlank, 1)
                {
                    self.pos = fnb;
                }
            }
        }
        self.invalidate_width_cache();
        self.clamp_vi_command_pos();
        KeyAction::Continue
    }
}
