//! VISUAL-mode key resolution (vim flavor only).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::VisualKind;
use super::command::{VisualCmd, VisualOutcome};
use super::engine::{MotionKey, Pending, ViEngine, count_digit, g_motion, motion_key};
use super::motion::ViMotion;

impl ViEngine {
    /// Resolve one VISUAL-mode key event (vim flavor only). Shares the
    /// count accumulator, find-char pending state, and `;`/`,` find
    /// memory with Command mode.
    pub fn resolve_visual_key(&mut self, key: KeyEvent) -> VisualOutcome {
        use VisualCmd as VC;
        let mods = key.modifiers;
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        let alt = mods.contains(KeyModifiers::ALT);

        if ctrl && !alt {
            self.clear_pending();
            return match key.code {
                // Vim: Ctrl-C exits VISUAL; it does not cancel the line.
                KeyCode::Char('c') => VisualOutcome::Cmd(VC::Exit, 1),
                KeyCode::Char('d') => VisualOutcome::Cmd(VC::Eof, 1),
                KeyCode::Char('l') => VisualOutcome::Cmd(VC::ClearScreen, 1),
                KeyCode::Char('j') => VisualOutcome::Cmd(VC::Submit, 1),
                _ => VisualOutcome::Cmd(VC::Bell, 1),
            };
        }

        match key.code {
            KeyCode::Enter => {
                self.clear_pending();
                return VisualOutcome::Cmd(VC::Submit, 1);
            }
            KeyCode::Esc => {
                // A pending prefix (find char, `r`) is cancelled first;
                // only a bare Esc exits VISUAL.
                return if self.cancel_pending() {
                    VisualOutcome::Pending
                } else {
                    VisualOutcome::Cmd(VC::Exit, 1)
                };
            }
            KeyCode::Left => {
                let n = self.take_count();
                return VisualOutcome::Cmd(VC::Move(ViMotion::CharBack), n);
            }
            KeyCode::Right => {
                let n = self.take_count();
                return VisualOutcome::Cmd(VC::Move(ViMotion::CharForward), n);
            }
            KeyCode::Up => {
                let n = self.take_count();
                return VisualOutcome::Cmd(VC::LineUp, n);
            }
            KeyCode::Down => {
                let n = self.take_count();
                return VisualOutcome::Cmd(VC::LineDown, n);
            }
            _ => {}
        }

        // Fast ESC-then-key (Alt encoding): cancel pending input, then
        // the key acts fresh — but a bare Alt+key exits like Esc would
        // and replays nothing (VISUAL has no Alt chords).
        let Some(ch) = self.key_char(key.code, alt) else {
            return VisualOutcome::Cmd(VC::Bell, 1);
        };

        match self.pending {
            Pending::FindChar(kind, _) => {
                let (motion, n) = self.take_find(kind, ch);
                return VisualOutcome::Cmd(VC::Move(motion), n);
            }
            Pending::ReplaceChar => {
                self.clear_pending();
                return VisualOutcome::Cmd(VC::ReplaceChar(ch), 1);
            }
            Pending::G(_) => {
                self.pending = Pending::None;
                let n = self.take_count();
                let Some(motion) = g_motion(ch) else {
                    return VisualOutcome::Cmd(VC::Bell, 1);
                };
                return VisualOutcome::Cmd(VC::Move(motion), n);
            }
            Pending::VisualObject { around } => {
                self.pending = Pending::None;
                let n = self.take_count();
                return VisualOutcome::Cmd(VC::TextObject { around, obj: ch }, n);
            }
            Pending::None => {}
            // Command-mode-only pendings cannot be active in VISUAL.
            _ => {
                self.clear_pending();
                return VisualOutcome::Cmd(VC::Bell, 1);
            }
        }

        if let Some(count) = count_digit(self.count, ch) {
            self.count = Some(count);
            return VisualOutcome::Pending;
        }

        let n = self.take_count();
        if let Some(mk) = motion_key(ch) {
            return match mk {
                MotionKey::Simple(motion) | MotionKey::CountIgnored(motion) => {
                    VisualOutcome::Cmd(VC::Move(motion), n)
                }
                MotionKey::Find(kind) => {
                    self.await_arg(Pending::FindChar(kind, None), n);
                    VisualOutcome::Pending
                }
                MotionKey::Repeat { rev } => match self.repeat_find(rev) {
                    Some(motion) => VisualOutcome::Cmd(VC::Move(motion), n),
                    None => VisualOutcome::Cmd(VC::Bell, 1),
                },
            };
        }
        let cmd = match ch {
            // j/k are pure buffer motion in VISUAL (no history recall).
            'k' => VC::LineUp,
            'j' => VC::LineDown,
            '%' => return VisualOutcome::Cmd(VC::Move(ViMotion::MatchPair), 1),
            'g' => {
                self.await_arg(Pending::G(None), n);
                return VisualOutcome::Pending;
            }
            'i' | 'a' => {
                self.await_arg(Pending::VisualObject { around: ch == 'a' }, n);
                return VisualOutcome::Pending;
            }
            'd' | 'x' => VC::Delete,
            'c' | 's' => VC::Change,
            'y' => VC::Yank,
            'D' => VC::DeleteLines,
            'C' | 'S' | 'R' => VC::ChangeLines,
            'Y' => VC::YankLines,
            'p' => VC::Put { before: false },
            'P' => VC::Put { before: true },
            'r' => {
                self.pending = Pending::ReplaceChar;
                return VisualOutcome::Pending;
            }
            '~' => VC::ToggleCase,
            'o' => VC::SwapEnds,
            'v' => VC::ToggleKind(VisualKind::Char),
            'V' => VC::ToggleKind(VisualKind::Line),
            _ => VC::Bell,
        };
        VisualOutcome::Cmd(cmd, n)
    }
}
