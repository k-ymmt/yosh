//! Command-mode key resolution: count prefixes, operators and their
//! motions, char-argument commands, and the vim-flavor additions
//! (`g` prefix, text objects, `%`, `Ctrl-X Ctrl-E`, `Ctrl-R` redo).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::command::{InsertAt, OpKind, SearchDir, ViCmd, ViOutcome};
use super::engine::{COUNT_CAP, MotionKey, Pending, ViEngine, count_digit, g_motion, motion_key};
use super::motion::ViMotion;
use super::{ViFlavor, VisualKind};

impl ViEngine {
    /// Resolve one command-mode key event.
    pub fn resolve_command_key(&mut self, key: KeyEvent) -> ViOutcome {
        let mods = key.modifiers;
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        let alt = mods.contains(KeyModifiers::ALT);

        // Control keys act regardless of pending state (and clear it):
        // interrupt/EOF/redraw mirror their insert-mode meaning. A fast
        // ESC before a control key arrives as Alt+Ctrl+key (terminal
        // ESC-prefix encoding): the ESC cancels pending input first,
        // then the control key acts fresh — same as typing them slowly.
        if ctrl {
            if alt {
                self.clear_pending();
            }
            // A pending vim Ctrl-X consumes the next control key.
            if self.pending == Pending::CtrlX {
                self.clear_pending();
                return match key.code {
                    KeyCode::Char('e') => ViOutcome::Cmd(ViCmd::EditBuffer, 1),
                    _ => ViOutcome::Cmd(ViCmd::Bell, 1),
                };
            }
            let n = self.take_count();
            self.pending = Pending::None;
            return match key.code {
                KeyCode::Char('c') => ViOutcome::Cmd(ViCmd::Interrupt, 1),
                KeyCode::Char('d') => ViOutcome::Cmd(ViCmd::Eof, 1),
                KeyCode::Char('l') => ViOutcome::Cmd(ViCmd::ClearScreen, 1),
                // vim: Ctrl-R is redo in Command mode (fuzzy history
                // search stays on Ctrl-R in Insert mode).
                KeyCode::Char('r') => match self.flavor {
                    ViFlavor::Posix => ViOutcome::Cmd(ViCmd::FuzzySearch, 1),
                    ViFlavor::Vim => ViOutcome::Cmd(ViCmd::Redo, n),
                },
                // Ctrl+J: raw-mode LF, treated as Enter (see Keymap).
                KeyCode::Char('j') => ViOutcome::Cmd(ViCmd::Submit, 1),
                // vim Ctrl-X: prefix for Ctrl-X Ctrl-E (edit buffer).
                // Shadows Vim's number decrement (out of scope, §12).
                KeyCode::Char('x') if self.flavor == ViFlavor::Vim => {
                    self.pending = Pending::CtrlX;
                    ViOutcome::Pending
                }
                _ => ViOutcome::Cmd(ViCmd::Bell, 1),
            };
        }

        match key.code {
            KeyCode::Enter => {
                self.clear_pending();
                return ViOutcome::Cmd(ViCmd::Submit, 1);
            }
            KeyCode::Esc => {
                // Cancel any pending count / char argument.
                return if self.cancel_pending() {
                    ViOutcome::Pending
                } else {
                    ViOutcome::Cmd(ViCmd::Bell, 1)
                };
            }
            // Arrow keys keep their intuitive meaning in command mode.
            KeyCode::Left => {
                let n = self.take_count();
                return ViOutcome::Cmd(ViCmd::Move(ViMotion::CharBack), n);
            }
            KeyCode::Right => {
                let n = self.take_count();
                return ViOutcome::Cmd(ViCmd::Move(ViMotion::CharForward), n);
            }
            KeyCode::Up => {
                let n = self.take_count();
                return ViOutcome::Cmd(ViCmd::HistoryPrev, n);
            }
            KeyCode::Down => {
                let n = self.take_count();
                return ViOutcome::Cmd(ViCmd::HistoryNext, n);
            }
            _ => {}
        }

        // A fast ESC-then-key arrives as Alt+key (terminal ESC prefix
        // encoding). ESC cancels pending input, then the key acts as a
        // fresh command — matching what typing them slowly does.
        let Some(ch) = self.key_char(key.code, alt) else {
            return ViOutcome::Cmd(ViCmd::Bell, 1);
        };

        // A pending char argument consumes the next character verbatim.
        match self.pending {
            Pending::FindChar(kind, op) => {
                let (motion, n) = self.take_find(kind, ch);
                return ViOutcome::Cmd(ViCmd::motion(op, motion), n);
            }
            Pending::ReplaceChar => {
                self.pending = Pending::None;
                let n = self.take_count();
                return ViOutcome::Cmd(ViCmd::ReplaceChar(ch), n);
            }
            Pending::AliasChar => {
                self.clear_pending();
                return ViOutcome::Cmd(ViCmd::AliasMacro(ch), 1);
            }
            Pending::Op(op, count_after) => return self.resolve_op_motion(op, count_after, ch),
            Pending::CtrlX => {
                // Ctrl-X followed by a non-control key: not a chord.
                self.clear_pending();
                return ViOutcome::Cmd(ViCmd::Bell, 1);
            }
            Pending::TextObject { op, around } => {
                self.pending = Pending::None;
                let n = self.take_count();
                return ViOutcome::Cmd(
                    ViCmd::OpObject {
                        op,
                        around,
                        obj: ch,
                    },
                    n,
                );
            }
            Pending::G(op) => {
                self.pending = Pending::None;
                let n = self.take_count();
                // `g` + other → bell.
                let Some(motion) = g_motion(ch) else {
                    return ViOutcome::Cmd(ViCmd::Bell, 1);
                };
                return ViOutcome::Cmd(ViCmd::motion(op, motion), n);
            }
            Pending::VisualObject { .. } => {
                // VISUAL-only pending cannot be active in Command mode.
                self.clear_pending();
                return ViOutcome::Cmd(ViCmd::Bell, 1);
            }
            Pending::None => {}
        }

        // Count digits. `0` is the line-start motion unless a count is
        // already in progress. Counts are capped so an absurd repeat
        // (e.g. `999999999999p`) cannot stall the editor.
        if let Some(count) = count_digit(self.count, ch) {
            self.count = Some(count);
            return ViOutcome::Pending;
        }

        // Commands that need to distinguish "no count given" from an
        // explicit count carry a 0 sentinel: `.` (repeat with recorded
        // count), `G` (oldest entry), `_` (last bigword).
        match ch {
            '.' => {
                let n = self.count.take().unwrap_or(0);
                return ViOutcome::Cmd(ViCmd::Repeat, n);
            }
            'G' => {
                let n = self.count.take().unwrap_or(0);
                return ViOutcome::Cmd(ViCmd::HistoryGoto, n);
            }
            '_' => {
                let n = self.count.take().unwrap_or(0);
                return ViOutcome::Cmd(ViCmd::InsertPrevBigword, n);
            }
            'v' => {
                let n = self.count.take().unwrap_or(0);
                return match self.flavor {
                    ViFlavor::Posix => ViOutcome::Cmd(ViCmd::EditInEditor, n),
                    // Vim: enter VISUAL charwise; count ignored (§12).
                    ViFlavor::Vim => ViOutcome::Cmd(ViCmd::EnterVisual(VisualKind::Char), 1),
                };
            }
            'V' if self.flavor == ViFlavor::Vim => {
                self.count = None;
                return ViOutcome::Cmd(ViCmd::EnterVisual(VisualKind::Line), 1);
            }
            _ => {}
        }

        let n = self.take_count();
        if self.flavor == ViFlavor::Vim {
            match ch {
                // `%`: count ignored (Vim's [count]% is percent-of-file).
                '%' => return ViOutcome::Cmd(ViCmd::Move(ViMotion::MatchPair), 1),
                'g' => {
                    self.await_arg(Pending::G(None), n);
                    return ViOutcome::Pending;
                }
                _ => {}
            }
        }
        if let Some(mk) = motion_key(ch) {
            return self.apply_motion_key(mk, n, None);
        }
        let cmd = match ch {
            'i' => ViCmd::EnterInsert(InsertAt::Here),
            'I' => ViCmd::EnterInsert(InsertAt::FirstNonBlank),
            'a' => ViCmd::EnterInsert(InsertAt::AfterChar),
            'A' => ViCmd::EnterInsert(InsertAt::LineEnd),
            'R' => ViCmd::ReplaceMode,
            'x' => ViCmd::DeleteChar,
            'X' => ViCmd::DeleteCharBack,
            'r' => {
                self.await_arg(Pending::ReplaceChar, n);
                return ViOutcome::Pending;
            }
            '~' => ViCmd::ToggleCase,
            'd' | 'c' | 'y' => {
                let op = match ch {
                    'd' => OpKind::Delete,
                    'c' => OpKind::Change,
                    _ => OpKind::Yank,
                };
                self.await_arg(Pending::Op(op, None), n);
                return ViOutcome::Pending;
            }
            'D' => ViCmd::Op(OpKind::Delete, ViMotion::LineEnd),
            'C' => ViCmd::Op(OpKind::Change, ViMotion::LineEnd),
            // POSIX Y is y$; Vim's default Y is yy (linewise,
            // oracle-verified against vim --clean).
            'Y' => match self.flavor {
                ViFlavor::Posix => ViCmd::Op(OpKind::Yank, ViMotion::LineEnd),
                ViFlavor::Vim => ViCmd::OpLine(OpKind::Yank),
            },
            'S' => ViCmd::OpLine(OpKind::Change),
            's' => ViCmd::SubstChar,
            'p' => ViCmd::PutAfter,
            'P' => ViCmd::PutBefore,
            'u' => ViCmd::Undo,
            'U' => ViCmd::UndoAll,
            'k' | '-' => ViCmd::HistoryPrev,
            'j' | '+' => ViCmd::HistoryNext,
            '/' => ViCmd::SearchStart(SearchDir::Older),
            '?' => ViCmd::SearchStart(SearchDir::Newer),
            'n' => ViCmd::SearchNext,
            'N' => ViCmd::SearchReverse,
            '=' => ViCmd::ExpandList,
            '\\' => ViCmd::CompleteUnique,
            '*' => ViCmd::ExpandAll,
            '#' => ViCmd::CommentSubmit,
            '@' => {
                self.pending = Pending::AliasChar;
                return ViOutcome::Pending;
            }
            _ => ViCmd::Bell,
        };
        ViOutcome::Cmd(cmd, n)
    }

    /// Resolve the character following a `d`/`c`/`y` operator.
    fn resolve_op_motion(&mut self, op: OpKind, count_after: Option<u32>, ch: char) -> ViOutcome {
        // A count typed between operator and motion accumulates and
        // multiplies the pre-operator count (`2d3w` = 6 words).
        if let Some(count) = count_digit(count_after, ch) {
            self.pending = Pending::Op(op, Some(count));
            return ViOutcome::Pending;
        }
        self.pending = Pending::None;
        let total = self
            .take_count()
            .saturating_mul(count_after.unwrap_or(1).max(1))
            .min(COUNT_CAP);

        // Doubled operator char = whole-line variant.
        if ch == op.cmd_char() {
            return ViOutcome::Cmd(ViCmd::OpLine(op), total);
        }

        if self.flavor == ViFlavor::Vim {
            match ch {
                // Text-object pending: `d`/`c`/`y` + `i`/`a`.
                'i' | 'a' => {
                    let around = ch == 'a';
                    self.await_arg(Pending::TextObject { op, around }, total);
                    return ViOutcome::Pending;
                }
                'g' => {
                    self.await_arg(Pending::G(Some(op)), total);
                    return ViOutcome::Pending;
                }
                // Inclusive motion; count ignored.
                '%' => return ViOutcome::Cmd(ViCmd::Op(op, ViMotion::MatchPair), 1),
                _ => {}
            }
        }

        match motion_key(ch) {
            Some(mk) => self.apply_motion_key(mk, total, Some(op)),
            None => ViOutcome::Cmd(ViCmd::Bell, 1),
        }
    }

    /// Apply one motion-table entry with the caller's count/operator
    /// policy: a plain motion (`op == None`) yields `Move`, an
    /// operator-pending one yields `Op`.
    fn apply_motion_key(&mut self, mk: MotionKey, total: u32, op: Option<OpKind>) -> ViOutcome {
        let wrap = |motion: ViMotion, n: u32| ViOutcome::Cmd(ViCmd::motion(op, motion), n);
        match mk {
            MotionKey::Simple(motion) => wrap(motion, total),
            // POSIX: a count shall be ignored for the motions 0 ^ $
            // under an operator.
            MotionKey::CountIgnored(motion) => wrap(motion, if op.is_some() { 1 } else { total }),
            MotionKey::Find(kind) => {
                self.await_arg(Pending::FindChar(kind, op), total);
                ViOutcome::Pending
            }
            MotionKey::Repeat { rev } => match self.repeat_find(rev) {
                Some(motion) => wrap(motion, total),
                None => ViOutcome::Cmd(ViCmd::Bell, if op.is_some() { 1 } else { total }),
            },
        }
    }
}
