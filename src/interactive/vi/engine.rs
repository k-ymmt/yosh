//! [`ViEngine`]: the key-resolution state shared by Command and VISUAL
//! mode (pending count, pending char argument, find and change memory),
//! plus the motion-key table both modes resolve motions through.

use crossterm::event::KeyCode;

use super::command::{ChangeRecord, OpKind, ViCmd};
use super::motion::{FindKind, ViMotion};
use super::{ViFlavor, ViMode};

/// What the previous key(s) left the state machine waiting for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum Pending {
    #[default]
    None,
    /// `f`/`F`/`t`/`T` seen — waiting for the target character.
    /// `op` carries the operator context for `df x`-style sequences.
    FindChar(FindKind, Option<OpKind>),
    /// `r` seen — waiting for the replacement character.
    ReplaceChar,
    /// `@` seen — waiting for the alias letter.
    AliasChar,
    /// `d`/`c`/`y` seen — waiting for the motion (or a doubled operator
    /// char). A count typed after the operator accumulates here and
    /// multiplies the pre-operator count.
    Op(OpKind, Option<u32>),
    /// vim `Ctrl-X` seen — waiting for `Ctrl-E` (edit buffer in editor).
    CtrlX,
    /// vim operator + `i`/`a` seen — waiting for the object char.
    TextObject { op: OpKind, around: bool },
    /// vim `g` prefix — waiting for `e`/`E`; carries the operator
    /// context for `dge`-style sequences (`None` in VISUAL mode).
    G(Option<OpKind>),
    /// vim VISUAL `i`/`a` seen — waiting for the object char.
    VisualObject { around: bool },
}

/// One entry of the motion-key table shared by plain command-mode
/// motions and `d`/`c`/`y` operator motions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum MotionKey {
    /// Plain motion; the accumulated count applies as usual.
    Simple(ViMotion),
    /// `0` / `^` / `$` — POSIX: "a count shall be ignored" for these
    /// motions under an operator (plain moves keep the count, which
    /// `motion_move` ignores anyway for line-scoped targets).
    CountIgnored(ViMotion),
    /// `f` / `F` / `t` / `T` — waits for the target character.
    Find(FindKind),
    /// `;` / `,` — repeat the remembered find (reversed for `,`).
    Repeat { rev: bool },
}

/// The single motion-key table: which character maps to which motion
/// (or find/repeat behavior). Count policy and operator pairing are
/// applied by [`ViEngine::apply_motion_key`].
pub(super) fn motion_key(ch: char) -> Option<MotionKey> {
    use MotionKey::*;
    Some(match ch {
        'l' | ' ' => Simple(ViMotion::CharForward),
        'h' => Simple(ViMotion::CharBack),
        'w' => Simple(ViMotion::WordForward { big: false }),
        'W' => Simple(ViMotion::WordForward { big: true }),
        'e' => Simple(ViMotion::WordEnd { big: false }),
        'E' => Simple(ViMotion::WordEnd { big: true }),
        'b' => Simple(ViMotion::WordBack { big: false }),
        'B' => Simple(ViMotion::WordBack { big: true }),
        '0' => CountIgnored(ViMotion::LineStart),
        '^' => CountIgnored(ViMotion::FirstNonBlank),
        '$' => CountIgnored(ViMotion::LineEnd),
        '|' => Simple(ViMotion::Column),
        'f' => Find(FindKind::Find),
        'F' => Find(FindKind::FindBack),
        't' => Find(FindKind::To),
        'T' => Find(FindKind::ToBack),
        ';' => Repeat { rev: false },
        ',' => Repeat { rev: true },
        _ => return None,
    })
}

/// Upper bound on accumulated counts (readline-style sanity cap): a
/// count beyond this cannot do anything useful on a command line and
/// only serves to stall the editor.
pub(super) const COUNT_CAP: u32 = 1_000_000;

/// The count after typing `ch`, when `ch` continues one: `1`-`9`, or
/// `0` once a count is in progress (a bare `0` is the line-start
/// motion). Capped at [`COUNT_CAP`] so an absurd repeat (e.g.
/// `999999999999p`) cannot stall the editor.
pub(super) fn count_digit(cur: Option<u32>, ch: char) -> Option<u32> {
    let d = ch.to_digit(10)?;
    if d == 0 && cur.is_none() {
        return None;
    }
    Some(
        cur.unwrap_or(0)
            .saturating_mul(10)
            .saturating_add(d)
            .min(COUNT_CAP),
    )
}

/// vim `g` prefix target: `ge` / `gE` (end of the previous word /
/// WORD). `None` for any other key.
pub(super) fn g_motion(ch: char) -> Option<ViMotion> {
    match ch {
        'e' => Some(ViMotion::WordEndBack { big: false }),
        'E' => Some(ViMotion::WordEndBack { big: true }),
        _ => None,
    }
}

/// Command-mode key state machine. Holds only key-resolution state
/// (pending count / pending char argument / find memory); buffer state
/// stays in `LineEditor`.
#[derive(Debug, Default)]
pub struct ViEngine {
    pub mode: ViMode,
    /// POSIX-vi vs Vim semantics, synced from the edit mode by
    /// `LineEditor::set_edit_mode`.
    pub flavor: ViFlavor,
    /// When true, insert mode overwrites (entered via `R`).
    pub replace_overwrite: bool,
    pub(super) count: Option<u32>,
    pub(super) pending: Pending,
    /// Most recent `f`/`F`/`t`/`T` for `;` / `,`.
    last_find: Option<(FindKind, char)>,
    /// Most recent buffer-modifying command, replayed by `.`.
    last_change: Option<ChangeRecord>,
}

impl ViEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset per-read state: a fresh read starts in insert mode with no
    /// pending input. Find memory (`;`/`,`) survives across reads like
    /// the emacs kill ring does.
    pub fn reset_for_read(&mut self) {
        self.mode = ViMode::Insert;
        self.replace_overwrite = false;
        self.count = None;
        self.pending = Pending::None;
    }

    /// Take the accumulated count (default 1).
    pub(super) fn take_count(&mut self) -> u32 {
        self.count.take().unwrap_or(1).max(1)
    }

    /// Drop any pending count and char argument.
    pub(super) fn clear_pending(&mut self) {
        self.count = None;
        self.pending = Pending::None;
    }

    /// Wait for more input: `pending` names what the next key completes,
    /// and `count` is parked until then.
    pub(super) fn await_arg(&mut self, pending: Pending, count: u32) {
        self.pending = pending;
        self.count = Some(count);
    }

    /// Esc: cancel any pending count / char argument. Returns whether
    /// anything was pending.
    pub(super) fn cancel_pending(&mut self) -> bool {
        let had_pending = self.count.is_some() || self.pending != Pending::None;
        self.clear_pending();
        had_pending
    }

    /// The character a non-control key resolves as: `Backspace` acts as
    /// `h`, and an Alt+char (the terminal's encoding of a fast ESC
    /// followed by the char) first cancels pending input. `None` for
    /// keys with no command meaning.
    pub(super) fn key_char(&mut self, code: KeyCode, alt: bool) -> Option<char> {
        match code {
            KeyCode::Char(c) if !alt => Some(c),
            KeyCode::Char(c) => {
                self.clear_pending();
                Some(c)
            }
            KeyCode::Backspace => Some('h'),
            _ => None,
        }
    }

    /// Complete a pending `f`/`F`/`t`/`T` with its target character:
    /// remember it for `;` / `,` and return the motion together with
    /// the accumulated count.
    pub(super) fn take_find(&mut self, kind: FindKind, ch: char) -> (ViMotion, u32) {
        self.pending = Pending::None;
        let n = self.take_count();
        self.last_find = Some((kind, ch));
        (ViMotion::FindChar(kind, ch), n)
    }

    /// The remembered find, for `;` (`rev == false`) or `,` (reversed).
    pub(super) fn repeat_find(&self, rev: bool) -> Option<ViMotion> {
        self.last_find.map(|(kind, c)| {
            let kind = if rev { kind.reversed() } else { kind };
            ViMotion::FindChar(kind, c)
        })
    }

    /// Record the most recent buffer-modifying command for `.`.
    pub fn record_change(&mut self, cmd: ViCmd, count: u32) {
        self.last_change = Some(ChangeRecord { cmd, count });
    }

    /// The most recent buffer-modifying command, if any.
    pub fn last_change(&self) -> Option<ChangeRecord> {
        self.last_change
    }

    /// An explicit count given to `.` becomes the new default count for
    /// subsequent repeats (POSIX).
    pub fn set_last_change_count(&mut self, count: u32) {
        if let Some(rec) = &mut self.last_change {
            rec.count = count;
        }
    }
}
