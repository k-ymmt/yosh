//! POSIX vi command-mode buffer edits (the bodies of the
//! `execute_vi_cmd_arm` arms), plus the small helpers the vim flavor
//! shares with them. An invalid or no-op command requests the bell via
//! `pending_bell`.

use super::LineEditor;
use crate::interactive::history::History;
use crate::interactive::vi::{self, InsertAt, OpKind, ViMode, ViMotion};
use crate::interactive::vim::{self, RegisterKind};

/// vi `~`: swap the case of a cased character; others are unchanged.
pub(super) fn toggle_case(ch: char) -> char {
    if ch.is_uppercase() {
        ch.to_lowercase().next().unwrap_or(ch)
    } else if ch.is_lowercase() {
        ch.to_uppercase().next().unwrap_or(ch)
    } else {
        ch
    }
}

/// Repetitions of a `len`-char put for `count`, bounding the total
/// inserted text (count × length) so a huge count cannot balloon the
/// buffer.
pub(super) fn put_reps(count: u32, len: usize) -> usize {
    (count.max(1) as usize).min(1_000_000 / len.max(1)).max(1)
}

impl LineEditor {
    /// Move the cursor to the first non-blank character of its line.
    pub(super) fn move_to_first_non_blank(&mut self) {
        if let Some(p) = vi::motion_move(&self.buf, self.pos, ViMotion::FirstNonBlank, 1) {
            self.pos = p;
        }
    }

    /// Where a charwise put (or `_`) inserts: after the cursor character
    /// (clamped to the line end) or at the cursor.
    pub(super) fn put_position(&self, after: bool) -> usize {
        let le = vi::line_end(&self.buf, self.pos);
        if after {
            (self.pos + 1).min(le)
        } else {
            self.pos
        }
    }

    /// Insert `chars` `reps` times at `at`, returning the index just
    /// past the inserted text.
    pub(super) fn put_repeated(&mut self, at: usize, chars: &[char], reps: usize) -> usize {
        let mut insert_pos = at;
        for _ in 0..reps {
            for &c in chars {
                self.buf.insert(insert_pos, c);
                insert_pos += 1;
            }
        }
        insert_pos
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

    /// Delete `[start, end)` for a `d`/`c` operator: the cursor lands on
    /// `start`; `c` enters Insert, `d` stays a command-mode cursor.
    fn vi_delete_span(&mut self, op: OpKind, start: usize, end: usize) {
        self.buf.drain(start..end);
        self.pos = start;
        self.invalidate_width_cache();
        if op == OpKind::Change {
            self.vi.mode = ViMode::Insert;
        } else {
            self.clamp_vi_command_pos();
        }
    }

    /// `i` / `a` / `I` / `A`.
    pub(super) fn vi_enter_insert(&mut self, at: InsertAt) {
        // Pre-session state for session-granular vi undo (vim: the
        // wrapper stages the snapshot instead).
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
            InsertAt::FirstNonBlank => self.move_to_first_non_blank(),
            InsertAt::LineEnd => self.pos = le,
        }
    }

    /// `[count]x`.
    pub(super) fn vi_delete_char(&mut self, count: u32) {
        let Some((start, end)) = self.count_range_on_line(count) else {
            return;
        };
        let killed: String = self.buf[start..end].iter().collect();
        self.record_kill(&killed, false, RegisterKind::Charwise);
        self.buf.drain(start..end);
        self.invalidate_width_cache();
        self.clamp_vi_command_pos();
    }

    /// `[count]X`: mirror of `x` toward the line start; the range
    /// preamble differs (bounded by the line start, cursor lands on
    /// `start`), so it does not share count_range_on_line.
    pub(super) fn vi_delete_char_back(&mut self, count: u32) {
        let ls = vi::line_start(&self.buf, self.pos);
        if self.pos <= ls {
            self.pending_bell = true;
            return;
        }
        self.save_undo_posix();
        let start = self.pos.saturating_sub(count as usize).max(ls);
        let killed: String = self.buf[start..self.pos].iter().collect();
        self.record_kill(&killed, false, RegisterKind::Charwise);
        self.buf.drain(start..self.pos);
        self.pos = start;
        self.invalidate_width_cache();
        self.clamp_vi_command_pos();
    }

    /// `[count]r<c>`.
    pub(super) fn vi_replace_char(&mut self, c: char, count: u32) {
        let Some((start, end)) = self.count_range_on_line(count) else {
            return;
        };
        self.buf[start..end].fill(c);
        self.pos = end - 1;
        self.invalidate_width_cache();
    }

    /// `[count]~`.
    pub(super) fn vi_toggle_case(&mut self, count: u32) {
        let Some((start, end)) = self.count_range_on_line(count) else {
            return;
        };
        for ch in &mut self.buf[start..end] {
            *ch = toggle_case(*ch);
        }
        // Cursor advances past the last toggled character, capped
        // at the line's last character (the buffer length is
        // unchanged, so the line end is where it was).
        let le = vi::line_end(&self.buf, self.pos);
        self.pos = end.min(le - 1);
        self.invalidate_width_cache();
    }

    /// Operator + motion (`dw`, `c$`, `y0`, …).
    pub(super) fn vi_op_motion(&mut self, op: OpKind, motion: ViMotion, count: u32) {
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
                        self.vi_delete_span(op, start, end);
                    }
                }
            }
            _ => self.pending_bell = true,
        }
    }

    /// Operator + text object (`diw`, `ca(`, `yi"`, …; vim flavor).
    pub(super) fn vi_op_object(&mut self, op: OpKind, around: bool, obj: char, count: u32) {
        match vim::text_object_range(&self.buf, self.pos, obj, around, count) {
            None => self.pending_bell = true,
            Some((s, e)) if s == e => {
                // Empty inner object: d/y are bell-free no-ops;
                // c enters Insert inside the pair
                // (oracle-verified: ci( on () yields (X)).
                if op == OpKind::Change {
                    self.pos = s;
                    self.vi.mode = ViMode::Insert;
                }
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
                    OpKind::Delete | OpKind::Change => self.vi_delete_span(op, s, e),
                }
            }
        }
    }

    /// POSIX `dd` / `cc` / `yy`: the current logical line's text (the
    /// vim flavor uses `vim_op_line` instead).
    pub(super) fn vi_op_line(&mut self, op: OpKind) {
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
    }

    /// `[count]s`.
    pub(super) fn vi_subst_char(&mut self, count: u32) {
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
    }

    /// POSIX `[count]p` / `[count]P`: put the kill ring's front entry
    /// (the vim flavor uses `vim_put` instead).
    pub(super) fn vi_put(&mut self, after: bool, count: u32) {
        let text = self.kill_ring.yank().map(str::to_string);
        let Some(text) = text.filter(|t| !t.is_empty()) else {
            self.pending_bell = true;
            return;
        };
        self.undo.save(&self.buf, self.pos);
        let at = self.put_position(after);
        let chars: Vec<char> = text.chars().collect();
        let end = self.put_repeated(at, &chars, put_reps(count, chars.len()));
        // Cursor lands on the last put character, clamped onto an
        // editable cell (put text ending in '\n' would otherwise
        // park the cursor on the separator).
        self.pos = end - 1;
        self.invalidate_width_cache();
        self.clamp_vi_command_pos();
    }

    /// POSIX `u`: single-level undo.
    pub(super) fn vi_undo(&mut self) {
        match self.undo.undo() {
            Some((buf, pos)) => {
                self.buf = buf;
                self.pos = pos;
                self.invalidate_width_cache();
                self.clamp_vi_command_pos();
            }
            None => self.pending_bell = true,
        }
    }

    /// vim `[count]u` / `[count]Ctrl-R`: step through the undo (or redo)
    /// stack, the current state moving onto the opposite stack.
    pub(super) fn vim_undo_steps(&mut self, count: u32, redo: bool) {
        for _ in 0..count.max(1) {
            let step = if redo {
                self.undo.redo_swap(&self.buf, self.pos)
            } else {
                self.undo.undo_swap(&self.buf, self.pos)
            };
            match step {
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
    }

    /// `U`: restore the line the edit started from (empty, or the
    /// recalled history entry); `u` can undo the `U` itself (vim: via
    /// the wrapper commit).
    pub(super) fn vi_undo_all(&mut self) {
        self.save_undo_posix();
        self.buf = self.vi_line_base.clone();
        self.pos = 0;
        self.invalidate_width_cache();
        self.clamp_vi_command_pos();
    }

    /// `_`: append a space plus the count-th (default last) bigword of
    /// the previous input line, then enter insert mode after it.
    pub(super) fn vi_insert_prev_bigword(&mut self, count: u32, history: &History) {
        let Some(prev) = history.entries().last() else {
            self.pending_bell = true;
            return;
        };
        let bigwords: Vec<&str> = prev.split_whitespace().collect();
        let word = if count == 0 {
            bigwords.last().copied()
        } else {
            bigwords.get(count as usize - 1).copied()
        };
        let Some(word) = word.map(str::to_string) else {
            self.pending_bell = true;
            return;
        };
        self.save_undo_posix();
        let mut at = self.put_position(true);
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
    }

    /// `#`: insert '#' at the start of every logical line (the caller
    /// submits): the input is recorded in history but executes as
    /// comments. (POSIX describes the single-line case; commenting each
    /// line keeps multiline buffers inert.)
    pub(super) fn vi_comment_out_lines(&mut self) {
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
    }
}
