//! vi command mode: key resolution, the `.`/undo wrapper and the
//! per-command dispatch. The buffer edits themselves live in `vi_edit`
//! (POSIX vi) and `vim_ops` (vim flavor).

use crossterm::event::KeyEvent;

use super::{KeyAction, LineEditor};
use crate::interactive::edit_action::EditAction;
use crate::interactive::history::History;
use crate::interactive::vi::{self, OpKind, ViCmd, ViFlavor, ViMode, ViOutcome};

impl LineEditor {
    /// Clamp the cursor onto a character of its logical line: command-mode
    /// cursors rest *on* a character, never past the line end (except on
    /// an empty line).
    pub(super) fn clamp_vi_command_pos(&mut self) {
        let ls = vi::line_start(&self.buf, self.pos);
        let le = vi::line_end(&self.buf, self.pos);
        if self.pos >= le && le > ls {
            self.pos = le - 1;
        }
    }

    pub(super) fn handle_vi_command_key(
        &mut self,
        key: KeyEvent,
        history: &mut History,
    ) -> KeyAction {
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
                | ViCmd::OpObject {
                    op: OpKind::Delete | OpKind::Change,
                    ..
                }
                | ViCmd::InsertPrevBigword
        )
    }

    /// vim: commands the §7.2 wrapper must not treat as changes —
    /// `u`/`Ctrl-R` manipulate the stacks themselves, and history
    /// navigation/recall triggers the history-reset in `vi_recall`
    /// instead. `U` is deliberately NOT excluded: its restore-to-base
    /// commits as a regular unit.
    fn vim_undo_excluded(cmd: ViCmd) -> bool {
        matches!(
            cmd,
            ViCmd::Undo
                | ViCmd::Redo
                | ViCmd::HistoryPrev
                | ViCmd::HistoryNext
                | ViCmd::HistoryGoto
                | ViCmd::SearchNext
                | ViCmd::SearchReverse
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
        // §7.2 compare-and-commit wrapper (vim flavor): snapshot before
        // dispatch, commit one unit after iff the buffer changed. Only
        // at the outermost depth — a `.` replay must yield exactly one
        // unit, committed by the outer Repeat invocation.
        let pre =
            (!replaying && self.vi.flavor == ViFlavor::Vim).then(|| (self.buf.clone(), self.pos));
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
            if let Some((pre_buf, pre_pos)) = pre
                && !Self::vim_undo_excluded(cmd)
            {
                if mode_before != ViMode::Insert && self.vi.mode == ViMode::Insert {
                    // A command that enters Insert transfers its
                    // snapshot into the staging slot: the deletion and
                    // the inserted text form one unit, committed when
                    // Insert ends.
                    self.vim_undo_staged = Some((pre_buf, pre_pos));
                } else if self.buf != pre_buf {
                    self.vim_commit_undo(&pre_buf, pre_pos);
                }
            }
        }
        action
    }

    /// Request a terminal alert and continue the read loop — the shared
    /// tail of every invalid/no-op vi command arm.
    pub(super) fn bell(&mut self) -> KeyAction {
        self.pending_bell = true;
        KeyAction::Continue
    }

    /// Run a buffer edit that cannot change the read loop's course, then
    /// continue reading.
    fn vi_edit(&mut self, edit: impl FnOnce(&mut Self)) -> KeyAction {
        edit(self);
        KeyAction::Continue
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
            ViCmd::EnterInsert(at) => self.vi_edit(|ed| ed.vi_enter_insert(at)),
            ViCmd::ReplaceMode => self.vi_edit(|ed| {
                ed.save_undo_posix();
                ed.vi.mode = ViMode::Insert;
                ed.vi.replace_overwrite = true;
            }),
            ViCmd::DeleteChar => self.vi_edit(|ed| ed.vi_delete_char(count)),
            ViCmd::DeleteCharBack => self.vi_edit(|ed| ed.vi_delete_char_back(count)),
            ViCmd::ReplaceChar(c) => self.vi_edit(|ed| ed.vi_replace_char(c, count)),
            ViCmd::ToggleCase => self.vi_edit(|ed| ed.vi_toggle_case(count)),
            ViCmd::Op(op, motion) => self.vi_edit(|ed| ed.vi_op_motion(op, motion, count)),
            ViCmd::OpObject { op, around, obj } => {
                self.vi_edit(|ed| ed.vi_op_object(op, around, obj, count))
            }
            ViCmd::OpLine(op) => {
                if self.vi.flavor == ViFlavor::Vim {
                    self.vi_edit(|ed| ed.vim_op_line(op, count))
                } else {
                    self.vi_edit(|ed| ed.vi_op_line(op))
                }
            }
            ViCmd::SubstChar => self.vi_edit(|ed| ed.vi_subst_char(count)),
            ViCmd::PutAfter | ViCmd::PutBefore => {
                let after = cmd == ViCmd::PutAfter;
                if self.vi.flavor == ViFlavor::Vim {
                    self.vi_edit(|ed| ed.vim_put(after, count))
                } else {
                    self.vi_edit(|ed| ed.vi_put(after, count))
                }
            }
            ViCmd::Undo => {
                if self.vi.flavor == ViFlavor::Vim {
                    // Multi-level undo with counts; the current state
                    // moves to the redo stack.
                    self.vi_edit(|ed| ed.vim_undo_steps(count, false))
                } else {
                    self.vi_edit(Self::vi_undo)
                }
            }
            // vim flavor only (POSIX resolution never produces it).
            ViCmd::Redo => self.vi_edit(|ed| ed.vim_undo_steps(count, true)),
            ViCmd::UndoAll => self.vi_edit(Self::vi_undo_all),
            ViCmd::HistoryPrev => self.vi_edit(|ed| ed.vi_history_step(true, count, history)),
            ViCmd::HistoryNext => self.vi_edit(|ed| ed.vi_history_step(false, count, history)),
            ViCmd::HistoryGoto => self.vi_edit(|ed| ed.vi_history_goto(count, history)),
            ViCmd::SearchStart(dir) => self.vi_edit(|ed| {
                ed.vi_search_input = Some((dir, String::new()));
                ed.suggestion = None;
            }),
            ViCmd::SearchNext | ViCmd::SearchReverse => {
                let reverse = cmd == ViCmd::SearchReverse;
                self.vi_edit(|ed| ed.vi_search_again(reverse, history))
            }
            ViCmd::InsertPrevBigword => {
                self.vi_edit(|ed| ed.vi_insert_prev_bigword(count, history))
            }
            ViCmd::ExpandList | ViCmd::CompleteUnique | ViCmd::ExpandAll => self.vi_expand(cmd),
            ViCmd::CommentSubmit => {
                self.vi_comment_out_lines();
                KeyAction::Submit
            }
            ViCmd::AliasMacro(c) => KeyAction::ViAliasMacro(c),
            ViCmd::EditInEditor => KeyAction::ViEditInEditor(count),
            ViCmd::EnterVisual(kind) => self.vi_edit(|ed| {
                ed.vi.mode = ViMode::Visual {
                    kind,
                    anchor: ed.pos,
                };
            }),
            ViCmd::EditBuffer => KeyAction::ViEditBuffer,
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
}
