//! vi command mode: key resolution, the `.`/undo wrapper and the
//! per-command dispatch.

use crossterm::event::KeyEvent;

use super::{KeyAction, LineEditor};
use crate::interactive::completion;
use crate::interactive::edit_action::EditAction;
use crate::interactive::history::History;
use crate::interactive::vi::{
    self, InsertAt, OpKind, ViCmd, ViFlavor, ViMode, ViMotion, ViOutcome,
};
use crate::interactive::vim::{self, RegisterKind};

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

    /// Shared preamble of the count-ranged single-line commands (`x`,
    /// `r`, `~`): alert when the cursor is at/past the line end,
    /// otherwise save the undo state and return the `[pos, end)` range
    /// the count covers, clipped to the line end.
    fn count_range_on_line(&mut self, count: u32) -> Option<(usize, usize)> {
        let le = vi::line_end(&self.buf, self.pos);
        if self.pos >= le {
            self.pending_bell = true;
            return None;
        }
        self.save_undo_posix();
        Some((self.pos, (self.pos + count as usize).min(le)))
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
            ViCmd::EnterInsert(at) => {
                // Pre-session state for session-granular vi undo (vim:
                // the wrapper stages the snapshot instead).
                self.save_undo_posix();
                self.vi.mode = ViMode::Insert;
                let le = vi::line_end(&self.buf, self.pos);
                match at {
                    InsertAt::Here => {}
                    InsertAt::AfterChar => {
                        if self.pos < le {
                            self.pos += 1;
                        }
                    }
                    InsertAt::FirstNonBlank => {
                        if let Some(p) =
                            vi::motion_move(&self.buf, self.pos, ViMotion::FirstNonBlank, 1)
                        {
                            self.pos = p;
                        }
                    }
                    InsertAt::LineEnd => self.pos = le,
                }
                KeyAction::Continue
            }
            ViCmd::ReplaceMode => {
                self.save_undo_posix();
                self.vi.mode = ViMode::Insert;
                self.vi.replace_overwrite = true;
                KeyAction::Continue
            }
            ViCmd::DeleteChar => {
                let Some((start, end)) = self.count_range_on_line(count) else {
                    return KeyAction::Continue;
                };
                let killed: String = self.buf[start..end].iter().collect();
                self.record_kill(&killed, false, RegisterKind::Charwise);
                self.buf.drain(start..end);
                self.invalidate_width_cache();
                self.clamp_vi_command_pos();
                KeyAction::Continue
            }
            ViCmd::DeleteCharBack => {
                // Mirror of DeleteChar toward the line start; the range
                // preamble differs (bounded by `ls`, cursor lands on
                // `start`), so it does not share count_range_on_line.
                let ls = vi::line_start(&self.buf, self.pos);
                if self.pos <= ls {
                    return self.bell();
                }
                self.save_undo_posix();
                let start = self.pos.saturating_sub(count as usize).max(ls);
                let killed: String = self.buf[start..self.pos].iter().collect();
                self.record_kill(&killed, false, RegisterKind::Charwise);
                self.buf.drain(start..self.pos);
                self.pos = start;
                self.invalidate_width_cache();
                self.clamp_vi_command_pos();
                KeyAction::Continue
            }
            ViCmd::ReplaceChar(c) => {
                let Some((start, end)) = self.count_range_on_line(count) else {
                    return KeyAction::Continue;
                };
                for i in start..end {
                    self.buf[i] = c;
                }
                self.pos = end - 1;
                self.invalidate_width_cache();
                KeyAction::Continue
            }
            ViCmd::ToggleCase => {
                let Some((start, end)) = self.count_range_on_line(count) else {
                    return KeyAction::Continue;
                };
                for i in start..end {
                    let ch = self.buf[i];
                    self.buf[i] = if ch.is_uppercase() {
                        ch.to_lowercase().next().unwrap_or(ch)
                    } else if ch.is_lowercase() {
                        ch.to_uppercase().next().unwrap_or(ch)
                    } else {
                        ch
                    };
                }
                // Cursor advances past the last toggled character, capped
                // at the line's last character (the buffer length is
                // unchanged, so the line end is where it was).
                let le = vi::line_end(&self.buf, self.pos);
                self.pos = end.min(le - 1);
                self.invalidate_width_cache();
                KeyAction::Continue
            }
            ViCmd::Op(op, motion) => {
                // cw / cW act like ce / cE on a non-blank character
                // (vi/readline tradition; POSIX is silent on it).
                let motion = match (op, motion) {
                    (OpKind::Change, ViMotion::WordForward { big })
                        if self.pos < self.buf.len()
                            && !matches!(self.buf[self.pos], ' ' | '\t' | '\n') =>
                    {
                        ViMotion::WordEnd { big }
                    }
                    _ => motion,
                };
                match vi::motion_range(&self.buf, self.pos, motion, count) {
                    Some((start, end)) if start < end => {
                        let text: String = self.buf[start..end].iter().collect();
                        self.record_kill(&text, false, RegisterKind::Charwise);
                        match op {
                            OpKind::Yank => {
                                // POSIX: the cursor does not move.
                            }
                            OpKind::Delete | OpKind::Change => {
                                self.save_undo_posix();
                                self.buf.drain(start..end);
                                self.pos = start;
                                self.invalidate_width_cache();
                                if op == OpKind::Change {
                                    self.vi.mode = ViMode::Insert;
                                } else {
                                    self.clamp_vi_command_pos();
                                }
                            }
                        }
                        KeyAction::Continue
                    }
                    _ => self.bell(),
                }
            }
            ViCmd::OpObject { op, around, obj } => {
                match vim::text_object_range(&self.buf, self.pos, obj, around, count) {
                    None => self.bell(),
                    Some((s, e)) if s == e => {
                        // Empty inner object: d/y are bell-free no-ops;
                        // c enters Insert inside the pair
                        // (oracle-verified: ci( on () yields (X)).
                        if op == OpKind::Change {
                            self.pos = s;
                            self.vi.mode = ViMode::Insert;
                        }
                        KeyAction::Continue
                    }
                    Some((s, e)) => {
                        let text: String = self.buf[s..e].iter().collect();
                        // Text-object deletes/yanks are charwise (§6.2).
                        self.record_kill(&text, false, RegisterKind::Charwise);
                        match op {
                            OpKind::Yank => {
                                // Vim: the cursor moves to the object start.
                                self.pos = s;
                            }
                            OpKind::Delete | OpKind::Change => {
                                self.buf.drain(s..e);
                                self.pos = s;
                                self.invalidate_width_cache();
                                if op == OpKind::Change {
                                    self.vi.mode = ViMode::Insert;
                                } else {
                                    self.clamp_vi_command_pos();
                                }
                            }
                        }
                        KeyAction::Continue
                    }
                }
            }
            ViCmd::OpLine(op) => {
                if self.vi.flavor == ViFlavor::Vim {
                    return self.vim_op_line(op, count);
                }
                let ls = vi::line_start(&self.buf, self.pos);
                let le = vi::line_end(&self.buf, self.pos);
                let text: String = self.buf[ls..le].iter().collect();
                if !text.is_empty() {
                    self.record_kill(&text, false, RegisterKind::Charwise);
                }
                match op {
                    OpKind::Yank => {}
                    OpKind::Delete | OpKind::Change => {
                        self.save_undo_posix();
                        self.buf.drain(ls..le);
                        self.pos = ls;
                        self.invalidate_width_cache();
                        if op == OpKind::Change {
                            self.vi.mode = ViMode::Insert;
                        }
                    }
                }
                KeyAction::Continue
            }
            ViCmd::SubstChar => {
                self.save_undo_posix();
                let le = vi::line_end(&self.buf, self.pos);
                if self.pos < le {
                    let end = (self.pos + count as usize).min(le);
                    let text: String = self.buf[self.pos..end].iter().collect();
                    self.record_kill(&text, false, RegisterKind::Charwise);
                    self.buf.drain(self.pos..end);
                    self.invalidate_width_cache();
                }
                self.vi.mode = ViMode::Insert;
                KeyAction::Continue
            }
            ViCmd::PutAfter | ViCmd::PutBefore => {
                if self.vi.flavor == ViFlavor::Vim {
                    return self.vim_put(cmd == ViCmd::PutAfter, count);
                }
                let text = self.kill_ring.yank().map(str::to_string);
                let Some(text) = text.filter(|t| !t.is_empty()) else {
                    return self.bell();
                };
                self.undo.save(&self.buf, self.pos);
                let le = vi::line_end(&self.buf, self.pos);
                let at = if cmd == ViCmd::PutAfter {
                    (self.pos + 1).min(le)
                } else {
                    self.pos
                };
                let chars: Vec<char> = text.chars().collect();
                // Bound the total inserted text (count × kill length) so a
                // huge count cannot balloon the buffer.
                let reps = (count.max(1) as usize)
                    .min(1_000_000 / chars.len().max(1))
                    .max(1);
                let mut insert_pos = at;
                for _ in 0..reps {
                    for &c in &chars {
                        self.buf.insert(insert_pos, c);
                        insert_pos += 1;
                    }
                }
                // Cursor lands on the last put character, clamped onto an
                // editable cell (put text ending in '\n' would otherwise
                // park the cursor on the separator).
                self.pos = insert_pos - 1;
                self.invalidate_width_cache();
                self.clamp_vi_command_pos();
                KeyAction::Continue
            }
            ViCmd::Undo => {
                if self.vi.flavor == ViFlavor::Vim {
                    // Multi-level undo with counts; the current state
                    // moves to the redo stack.
                    for _ in 0..count.max(1) {
                        match self.undo.undo_swap(&self.buf, self.pos) {
                            Some((buf, pos)) => {
                                self.buf = buf;
                                self.pos = pos;
                            }
                            None => {
                                self.pending_bell = true;
                                break;
                            }
                        }
                    }
                    self.invalidate_width_cache();
                    self.clamp_vi_command_pos();
                    return KeyAction::Continue;
                }
                match self.undo.undo() {
                    Some((buf, pos)) => {
                        self.buf = buf;
                        self.pos = pos;
                        self.invalidate_width_cache();
                        self.clamp_vi_command_pos();
                    }
                    None => self.pending_bell = true,
                }
                KeyAction::Continue
            }
            ViCmd::Redo => {
                // vim flavor only (POSIX resolution never produces it).
                for _ in 0..count.max(1) {
                    match self.undo.redo_swap(&self.buf, self.pos) {
                        Some((buf, pos)) => {
                            self.buf = buf;
                            self.pos = pos;
                        }
                        None => {
                            self.pending_bell = true;
                            break;
                        }
                    }
                }
                self.invalidate_width_cache();
                self.clamp_vi_command_pos();
                KeyAction::Continue
            }
            ViCmd::UndoAll => {
                // U restores the line the edit started from (empty, or
                // the recalled history entry); u can undo the U itself
                // (vim: via the wrapper commit).
                self.save_undo_posix();
                self.buf = self.vi_line_base.clone();
                self.pos = 0;
                self.invalidate_width_cache();
                self.clamp_vi_command_pos();
                KeyAction::Continue
            }
            ViCmd::HistoryPrev => {
                self.vi_history_step(true, count, history);
                KeyAction::Continue
            }
            ViCmd::HistoryNext => {
                self.vi_history_step(false, count, history);
                KeyAction::Continue
            }
            ViCmd::HistoryGoto => {
                // G: no count = oldest entry; count n = history number n
                // (POSIX vi `[count]G`). Numbers are stable across
                // HISTSIZE trimming, so map through the history base.
                let idx = if count == 0 {
                    if history.entries().is_empty() {
                        return self.bell();
                    }
                    0
                } else {
                    match history.index_of_number(count as usize) {
                        Some(idx) => idx,
                        None => return self.bell(),
                    }
                };
                let current = self.buffer();
                if let Some(line) = history.navigate_to(idx, &current).map(str::to_string) {
                    self.vi_recall(line);
                }
                KeyAction::Continue
            }
            ViCmd::SearchStart(dir) => {
                self.vi_search_input = Some((dir, String::new()));
                self.suggestion = None;
                KeyAction::Continue
            }
            ViCmd::SearchNext | ViCmd::SearchReverse => {
                match self.vi_last_search.clone() {
                    Some((dir, pattern)) => {
                        let dir = if cmd == ViCmd::SearchReverse {
                            dir.reversed()
                        } else {
                            dir
                        };
                        if !self.vi_history_search(dir, &pattern, history) {
                            self.pending_bell = true;
                        }
                    }
                    None => self.pending_bell = true,
                }
                KeyAction::Continue
            }
            ViCmd::InsertPrevBigword => {
                // `_`: append a space plus the count-th (default last)
                // bigword of the previous input line, then enter insert
                // mode after it.
                let Some(prev) = history.entries().last() else {
                    return self.bell();
                };
                let bigwords: Vec<&str> = prev.split_whitespace().collect();
                let word = if count == 0 {
                    bigwords.last().copied()
                } else {
                    bigwords.get(count as usize - 1).copied()
                };
                let Some(word) = word.map(str::to_string) else {
                    return self.bell();
                };
                self.save_undo_posix();
                let le = vi::line_end(&self.buf, self.pos);
                let mut at = (self.pos + 1).min(le);
                // POSIX: "Append a <space> after the current character
                // position" — unconditionally, empty line included.
                self.buf.insert(at, ' ');
                at += 1;
                for c in word.chars() {
                    self.buf.insert(at, c);
                    at += 1;
                }
                self.pos = at;
                self.invalidate_width_cache();
                self.vi.mode = ViMode::Insert;
                KeyAction::Continue
            }
            ViCmd::ExpandList | ViCmd::CompleteUnique | ViCmd::ExpandAll => {
                let Some(((start, end), mut matches)) = self.vi_expand_bigword() else {
                    return self.bell();
                };
                match cmd {
                    ViCmd::ExpandList => {
                        // POSIX: directories are marked with a trailing '/'.
                        for m in &mut matches {
                            if !m.ends_with('/') && std::path::Path::new(&m).is_dir() {
                                m.push('/');
                            }
                        }
                        KeyAction::ViListExpansions(matches)
                    }
                    ViCmd::CompleteUnique => {
                        // Largest unique match: the longest common prefix of
                        // all matches; a single match additionally gets '/'
                        // (directory) or a space (file) appended.
                        let mut replacement = completion::longest_common_prefix(&matches);
                        if matches.len() == 1 {
                            if std::path::Path::new(&replacement).is_dir() {
                                if !replacement.ends_with('/') {
                                    replacement.push('/');
                                }
                            } else {
                                replacement.push(' ');
                            }
                        }
                        self.vi_replace_range(start, end, &replacement);
                        self.vi.mode = ViMode::Insert;
                        KeyAction::Continue
                    }
                    _ => {
                        // ExpandAll
                        let replacement = matches.join(" ");
                        self.vi_replace_range(start, end, &replacement);
                        self.vi.mode = ViMode::Insert;
                        KeyAction::Continue
                    }
                }
            }
            ViCmd::CommentSubmit => {
                // Insert '#' at the start of every logical line and
                // submit: the input is recorded in history but executes
                // as comments. (POSIX describes the single-line case;
                // commenting each line keeps multiline buffers inert.)
                self.save_undo_posix();
                self.buf.insert(0, '#');
                let mut i = 1;
                while i < self.buf.len() {
                    if self.buf[i] == '\n' {
                        self.buf.insert(i + 1, '#');
                        i += 1;
                    }
                    i += 1;
                }
                self.pos = self.buf.len();
                self.invalidate_width_cache();
                KeyAction::Submit
            }
            ViCmd::AliasMacro(c) => KeyAction::ViAliasMacro(c),
            ViCmd::EditInEditor => KeyAction::ViEditInEditor(count),
            ViCmd::EnterVisual(kind) => {
                self.vi.mode = ViMode::Visual {
                    kind,
                    anchor: self.pos,
                };
                KeyAction::Continue
            }
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
