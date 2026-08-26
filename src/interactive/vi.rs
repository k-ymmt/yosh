// src/interactive/vi.rs

//! POSIX sh vi line editing (POSIX XCU `sh`, "vi Line Editing Command
//! Mode" / "vi Line Editing Insert Mode").
//!
//! This module holds the mode types, the command-mode key state machine
//! ([`ViEngine`]), and the motion-target math as pure functions over
//! `(&[char], pos)` so they can be unit-tested without a terminal.
//! Buffer mutation lives in `LineEditor::execute_vi_cmd`.
//!
//! Word vs bigword follow the vi utility definitions: a *word* is a
//! maximal run of alphanumerics/underscores or a maximal run of other
//! non-blank characters; a *bigword* is a maximal run of non-blanks.
//! Line-scoped motions (`0 ^ $ | f F t T h l`) operate on the current
//! logical line of the (possibly multiline) buffer; word motions may
//! cross newlines, which vi's single-line model never encounters.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Line-editing flavor selected by `set -o emacs` / `set -o vi` /
/// `set -o vim`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EditMode {
    #[default]
    Emacs,
    Vi,
    /// Vim-editor semantics (non-POSIX extension): vi-family dispatch
    /// with Vim editing behavior layered on via [`ViFlavor::Vim`].
    Vim,
}

impl EditMode {
    /// True for the vi-family modes (`Vi` / `Vim`): they share the vi
    /// key dispatch, per-read state reset, undo-save suppression, and
    /// cursor-style handling.
    pub fn is_vi_family(self) -> bool {
        matches!(self, Self::Vi | Self::Vim)
    }
}

/// Editing-semantics flavor of the vi engine: POSIX vi line editing
/// (`set -o vi`) or Vim-editor semantics (`set -o vim`). Shared
/// machinery lives in one code path; behavior differences branch on
/// this at key resolution and command execution.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ViFlavor {
    #[default]
    Posix,
    Vim,
}

/// vi submode. Reads always start in insert mode; ESC enters command
/// mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ViMode {
    #[default]
    Insert,
    Command,
}

/// Direction/kind of a pending `f`/`F`/`t`/`T` character search.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FindKind {
    /// `f` — to the count-th occurrence after the cursor.
    Find,
    /// `F` — to the count-th occurrence before the cursor.
    FindBack,
    /// `t` — to just before the count-th occurrence after the cursor.
    To,
    /// `T` — to just after the count-th occurrence before the cursor.
    ToBack,
}

impl FindKind {
    fn reversed(self) -> Self {
        match self {
            Self::Find => Self::FindBack,
            Self::FindBack => Self::Find,
            Self::To => Self::ToBack,
            Self::ToBack => Self::To,
        }
    }
}

/// A cursor motion (also the object of `d`/`c`/`y` operators).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViMotion {
    /// `l` / `<space>`
    CharForward,
    /// `h`
    CharBack,
    /// `w` / `W`
    WordForward { big: bool },
    /// `e` / `E`
    WordEnd { big: bool },
    /// `b` / `B`
    WordBack { big: bool },
    /// `0`
    LineStart,
    /// `^`
    FirstNonBlank,
    /// `$`
    LineEnd,
    /// `|` — column `count` (1-based)
    Column,
    /// `f c` / `F c` / `t c` / `T c` (and `;` / `,` after the engine
    /// substitutes the remembered kind/char)
    FindChar(FindKind, char),
}

/// Operator commands that pair with a motion (`d` / `c` / `y`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpKind {
    Delete,
    Change,
    Yank,
}

impl OpKind {
    fn cmd_char(self) -> char {
        match self {
            Self::Delete => 'd',
            Self::Change => 'c',
            Self::Yank => 'y',
        }
    }
}

/// Where an insert-entry command places the cursor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InsertAt {
    /// `i`
    Here,
    /// `a`
    AfterChar,
    /// `I`
    FirstNonBlank,
    /// `A`
    LineEnd,
}

/// Direction of a `/` / `?` history search. `/` moves toward older
/// entries, `?` toward newer ones.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SearchDir {
    Older,
    Newer,
}

impl SearchDir {
    pub fn reversed(self) -> Self {
        match self {
            Self::Older => Self::Newer,
            Self::Newer => Self::Older,
        }
    }

    /// The prompt character echoed while the pattern is typed.
    pub fn prompt_char(self) -> char {
        match self {
            Self::Older => '/',
            Self::Newer => '?',
        }
    }
}

/// Semantic command produced by the command-mode key state machine.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViCmd {
    Move(ViMotion),
    EnterInsert(InsertAt),
    /// `R` — insert mode with overwrite until ESC.
    ReplaceMode,
    /// `x`
    DeleteChar,
    /// `X`
    DeleteCharBack,
    /// `r c`
    ReplaceChar(char),
    /// `~`
    ToggleCase,
    /// `d motion` / `c motion` / `y motion` (and the `D` `C` `Y`
    /// shorthands, which resolve to `LineEnd` motions).
    Op(OpKind, ViMotion),
    /// `dd` / `cc` (also `S`) / `yy` — operate on the whole logical line.
    OpLine(OpKind),
    /// `s` — delete count chars and enter insert mode.
    SubstChar,
    /// `p`
    PutAfter,
    /// `P`
    PutBefore,
    /// `u`
    Undo,
    /// `U`
    UndoAll,
    /// `.` — count 0 means "no explicit count given".
    Repeat,
    /// `k` / `-` (and Up)
    HistoryPrev,
    /// `j` / `+` (and Down)
    HistoryNext,
    /// `G` — count 0 means "no number given" (oldest entry).
    HistoryGoto,
    /// `/` / `?` — begin pattern input for a history search.
    SearchStart(SearchDir),
    /// `n`
    SearchNext,
    /// `N`
    SearchReverse,
    /// `_` — append the count-th (default last) bigword of the previous
    /// input line and enter insert mode. Count 0 = "no count given".
    InsertPrevBigword,
    /// `=` — list the pathname expansions of the current bigword.
    ExpandList,
    /// `\` — complete the current bigword to the largest unique match
    /// and enter insert mode.
    CompleteUnique,
    /// `*` — replace the current bigword with all its pathname
    /// expansions and enter insert mode.
    ExpandAll,
    /// `#` — comment the line out and submit it (into history).
    CommentSubmit,
    /// `@ c` — run the value of alias `_c` as editor input.
    AliasMacro(char),
    /// `v` — edit the line (or history entry `count`; 0 = current
    /// line) in the vi editor, then execute the result.
    EditInEditor,
    Submit,
    Eof,
    Interrupt,
    ClearScreen,
    FuzzySearch,
    /// Invalid or unsupported input — alert (terminal bell), no change.
    Bell,
}

/// Resolution result: either a command with its effective count, or
/// "waiting for more input" (count digits, char argument).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViOutcome {
    Cmd(ViCmd, u32),
    Pending,
}

/// What the previous key(s) left the state machine waiting for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Pending {
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
}

/// The most recent buffer-modifying command, for `.`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChangeRecord {
    pub cmd: ViCmd,
    pub count: u32,
}

/// One entry of the motion-key table shared by plain command-mode
/// motions and `d`/`c`/`y` operator motions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MotionKey {
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
fn motion_key(ch: char) -> Option<MotionKey> {
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
const COUNT_CAP: u32 = 1_000_000;

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
    count: Option<u32>,
    pending: Pending,
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
    fn take_count(&mut self) -> u32 {
        self.count.take().unwrap_or(1).max(1)
    }

    /// Resolve one command-mode key event.
    pub fn resolve_command_key(&mut self, key: KeyEvent) -> ViOutcome {
        let mods = key.modifiers;
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        let alt = mods.contains(KeyModifiers::ALT);

        // Control keys act regardless of pending state (and clear it):
        // interrupt/EOF/redraw mirror their insert-mode meaning.
        if ctrl && !alt {
            self.count = None;
            self.pending = Pending::None;
            return match key.code {
                KeyCode::Char('c') => ViOutcome::Cmd(ViCmd::Interrupt, 1),
                KeyCode::Char('d') => ViOutcome::Cmd(ViCmd::Eof, 1),
                KeyCode::Char('l') => ViOutcome::Cmd(ViCmd::ClearScreen, 1),
                KeyCode::Char('r') => ViOutcome::Cmd(ViCmd::FuzzySearch, 1),
                // Ctrl+J: raw-mode LF, treated as Enter (see Keymap).
                KeyCode::Char('j') => ViOutcome::Cmd(ViCmd::Submit, 1),
                _ => ViOutcome::Cmd(ViCmd::Bell, 1),
            };
        }

        match key.code {
            KeyCode::Enter => {
                self.count = None;
                self.pending = Pending::None;
                return ViOutcome::Cmd(ViCmd::Submit, 1);
            }
            KeyCode::Esc => {
                // Cancel any pending count / char argument.
                let had_pending = self.count.is_some() || self.pending != Pending::None;
                self.count = None;
                self.pending = Pending::None;
                return if had_pending {
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

        let ch = match key.code {
            KeyCode::Char(c) if !alt => c,
            // A fast ESC-then-key arrives as Alt+key (terminal ESC
            // prefix encoding). ESC cancels pending input, then the key
            // acts as a fresh command — matching what typing them
            // slowly does.
            KeyCode::Char(c) => {
                self.count = None;
                self.pending = Pending::None;
                c
            }
            KeyCode::Backspace => 'h',
            _ => return ViOutcome::Cmd(ViCmd::Bell, 1),
        };

        // A pending char argument consumes the next character verbatim.
        match self.pending {
            Pending::FindChar(kind, op) => {
                self.pending = Pending::None;
                let n = self.take_count();
                self.last_find = Some((kind, ch));
                let motion = ViMotion::FindChar(kind, ch);
                return ViOutcome::Cmd(
                    match op {
                        Some(op) => ViCmd::Op(op, motion),
                        None => ViCmd::Move(motion),
                    },
                    n,
                );
            }
            Pending::ReplaceChar => {
                self.pending = Pending::None;
                let n = self.take_count();
                return ViOutcome::Cmd(ViCmd::ReplaceChar(ch), n);
            }
            Pending::AliasChar => {
                self.pending = Pending::None;
                self.count = None;
                return ViOutcome::Cmd(ViCmd::AliasMacro(ch), 1);
            }
            Pending::Op(op, count_after) => return self.resolve_op_motion(op, count_after, ch),
            Pending::None => {}
        }

        // Count digits. `0` is the line-start motion unless a count is
        // already in progress. Counts are capped so an absurd repeat
        // (e.g. `999999999999p`) cannot stall the editor.
        if let Some(d) = ch.to_digit(10)
            && (d != 0 || self.count.is_some())
        {
            let cur = self.count.unwrap_or(0);
            self.count = Some(cur.saturating_mul(10).saturating_add(d).min(COUNT_CAP));
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
                return ViOutcome::Cmd(ViCmd::EditInEditor, n);
            }
            _ => {}
        }

        let n = self.take_count();
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
                self.pending = Pending::ReplaceChar;
                self.count = Some(n);
                return ViOutcome::Pending;
            }
            '~' => ViCmd::ToggleCase,
            'd' | 'c' | 'y' => {
                let op = match ch {
                    'd' => OpKind::Delete,
                    'c' => OpKind::Change,
                    _ => OpKind::Yank,
                };
                self.pending = Pending::Op(op, None);
                self.count = Some(n);
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
        if let Some(d) = ch.to_digit(10)
            && (d != 0 || count_after.is_some())
        {
            let cur = count_after.unwrap_or(0);
            self.pending = Pending::Op(
                op,
                Some(cur.saturating_mul(10).saturating_add(d).min(COUNT_CAP)),
            );
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

        match motion_key(ch) {
            Some(mk) => self.apply_motion_key(mk, total, Some(op)),
            None => ViOutcome::Cmd(ViCmd::Bell, 1),
        }
    }

    /// Apply one motion-table entry with the caller's count/operator
    /// policy: a plain motion (`op == None`) yields `Move`, an
    /// operator-pending one yields `Op`.
    fn apply_motion_key(&mut self, mk: MotionKey, total: u32, op: Option<OpKind>) -> ViOutcome {
        let wrap = |motion: ViMotion, n: u32| match op {
            Some(op) => ViOutcome::Cmd(ViCmd::Op(op, motion), n),
            None => ViOutcome::Cmd(ViCmd::Move(motion), n),
        };
        match mk {
            MotionKey::Simple(motion) => wrap(motion, total),
            // POSIX: a count shall be ignored for the motions 0 ^ $
            // under an operator.
            MotionKey::CountIgnored(motion) => wrap(motion, if op.is_some() { 1 } else { total }),
            MotionKey::Find(kind) => {
                self.pending = Pending::FindChar(kind, op);
                self.count = Some(total);
                ViOutcome::Pending
            }
            MotionKey::Repeat { rev } => match self.last_find {
                Some((kind, c)) => {
                    let kind = if rev { kind.reversed() } else { kind };
                    wrap(ViMotion::FindChar(kind, c), total)
                }
                None => ViOutcome::Cmd(ViCmd::Bell, if op.is_some() { 1 } else { total }),
            },
        }
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

// ---------------------------------------------------------------------------
// Motion-target math (pure functions over the buffer)
// ---------------------------------------------------------------------------

/// Start of the logical line containing `pos`.
pub fn line_start(buf: &[char], pos: usize) -> usize {
    buf[..pos]
        .iter()
        .rposition(|&c| c == '\n')
        .map(|i| i + 1)
        .unwrap_or(0)
}

/// End (exclusive; index of `'\n'` or `buf.len()`) of the logical line
/// containing `pos`.
pub fn line_end(buf: &[char], pos: usize) -> usize {
    buf[pos..]
        .iter()
        .position(|&c| c == '\n')
        .map(|i| pos + i)
        .unwrap_or(buf.len())
}

/// vi blank: space or tab (newline is a line separator, not a blank).
pub fn is_blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// vi character class: 0 = blank (incl. newline), 1 = word char
/// (alphanumeric or underscore), 2 = other punctuation. Bigwords
/// collapse classes 1 and 2.
fn char_class(c: char, big: bool) -> u8 {
    if is_blank(c) || c == '\n' {
        0
    } else if big || c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

/// One `w` step: start of the next word. `None` when the cursor cannot
/// advance (already in the last word's trailing position / at end).
fn next_word_start(buf: &[char], pos: usize, big: bool) -> Option<usize> {
    let len = buf.len();
    if pos >= len {
        return None;
    }
    let mut i = pos;
    let c0 = char_class(buf[i], big);
    if c0 != 0 {
        while i < len && char_class(buf[i], big) == c0 {
            i += 1;
        }
    }
    while i < len && char_class(buf[i], big) == 0 {
        i += 1;
    }
    if i == pos { None } else { Some(i) }
}

/// One `e` step: end of the current word, or of the next word when the
/// cursor already sits on a word end.
fn word_end_after(buf: &[char], pos: usize, big: bool) -> Option<usize> {
    let len = buf.len();
    let mut i = pos.checked_add(1)?;
    if i >= len {
        return None;
    }
    while i < len && char_class(buf[i], big) == 0 {
        i += 1;
    }
    if i >= len {
        return None;
    }
    let c = char_class(buf[i], big);
    while i + 1 < len && char_class(buf[i + 1], big) == c {
        i += 1;
    }
    Some(i)
}

/// One `b` step: start of the current word, or of the previous word
/// when the cursor already sits on a word start.
fn prev_word_start(buf: &[char], pos: usize, big: bool) -> Option<usize> {
    if pos == 0 {
        return None;
    }
    let mut i = pos - 1;
    while char_class(buf[i], big) == 0 {
        if i == 0 {
            return None;
        }
        i -= 1;
    }
    let c = char_class(buf[i], big);
    while i > 0 && char_class(buf[i - 1], big) == c {
        i -= 1;
    }
    Some(i)
}

/// Index of the first non-blank character of the cursor's logical line
/// (line start when the line is all blank).
fn first_non_blank(buf: &[char], pos: usize) -> usize {
    let ls = line_start(buf, pos);
    let le = line_end(buf, pos);
    buf[ls..le]
        .iter()
        .position(|&c| !is_blank(c))
        .map(|i| ls + i)
        .unwrap_or(ls)
}

/// Index of the count-th occurrence of `target` for `f`/`F`/`t`/`T`,
/// scoped to the cursor's logical line.
fn find_char_index(
    buf: &[char],
    pos: usize,
    kind: FindKind,
    target: char,
    count: u32,
) -> Option<usize> {
    let ls = line_start(buf, pos);
    let le = line_end(buf, pos);
    match kind {
        FindKind::Find | FindKind::To => {
            let mut i = pos;
            for _ in 0..count.max(1) {
                // An empty line (or cursor at/after line end) has no
                // "after the cursor" region to search.
                if i + 1 > le {
                    return None;
                }
                i = buf[i + 1..le].iter().position(|&c| c == target)? + i + 1;
            }
            Some(i)
        }
        FindKind::FindBack | FindKind::ToBack => {
            let mut i = pos;
            for _ in 0..count.max(1) {
                if i <= ls {
                    return None;
                }
                i = buf[ls..i].iter().rposition(|&c| c == target)? + ls;
            }
            Some(i)
        }
    }
}

/// Cursor destination for a plain (non-operator) motion. `None` = the
/// motion cannot move the cursor (alert). Forward motions cap at the
/// last character; command-mode cursors rest *on* a character, so the
/// destination never equals a non-empty buffer's length.
pub fn motion_move(buf: &[char], pos: usize, motion: ViMotion, count: u32) -> Option<usize> {
    let count = count.max(1) as usize;
    let ls = line_start(buf, pos);
    let le = line_end(buf, pos);
    let line_last = if le > ls { le - 1 } else { ls };
    match motion {
        ViMotion::CharForward => {
            if pos >= line_last {
                return None;
            }
            Some((pos + count).min(line_last))
        }
        ViMotion::CharBack => {
            if pos <= ls {
                return None;
            }
            Some(pos.saturating_sub(count).max(ls))
        }
        ViMotion::WordForward { big } => {
            let mut p = pos;
            for step in 0..count {
                match next_word_start(buf, p, big) {
                    Some(np) if np < buf.len() => p = np,
                    // Landing exactly at (or past) buffer end caps the
                    // cursor at the last character.
                    _ => {
                        if step == 0 && pos + 1 >= buf.len() {
                            return None;
                        }
                        p = buf.len().saturating_sub(1);
                        break;
                    }
                }
            }
            if p == pos { None } else { Some(p) }
        }
        ViMotion::WordEnd { big } => {
            let mut p = pos;
            for step in 0..count {
                match word_end_after(buf, p, big) {
                    Some(np) => p = np,
                    None => {
                        if step == 0 {
                            return None;
                        }
                        break;
                    }
                }
            }
            Some(p)
        }
        ViMotion::WordBack { big } => {
            let mut p = pos;
            for step in 0..count {
                match prev_word_start(buf, p, big) {
                    Some(np) => p = np,
                    None => {
                        if step == 0 {
                            return None;
                        }
                        break;
                    }
                }
            }
            Some(p)
        }
        ViMotion::LineStart => Some(ls),
        ViMotion::FirstNonBlank => Some(first_non_blank(buf, pos)),
        ViMotion::LineEnd => Some(line_last),
        ViMotion::Column => Some((ls + count - 1).min(line_last)),
        ViMotion::FindChar(kind, target) => {
            let idx = find_char_index(buf, pos, kind, target, count as u32)?;
            match kind {
                FindKind::Find | FindKind::FindBack => Some(idx),
                FindKind::To => Some(idx.saturating_sub(1)),
                FindKind::ToBack => Some(idx + 1),
            }
        }
    }
}

/// Character range `[start, end)` a `d`/`c`/`y` operator applies to for
/// a motion. `None` = the motion cannot produce a range (alert); an
/// *empty* range is returned as-is (callers treat it as an alert too).
///
/// Encodes the vi/POSIX rules directly: forward-inclusive motions
/// (`e E f t $`) include their target character; forward-exclusive
/// motions (`l w |`) stop before it; backward motions never include the
/// character under the cursor.
pub fn motion_range(
    buf: &[char],
    pos: usize,
    motion: ViMotion,
    count: u32,
) -> Option<(usize, usize)> {
    let count = count.max(1) as usize;
    let ls = line_start(buf, pos);
    let le = line_end(buf, pos);
    match motion {
        ViMotion::CharForward => {
            if pos >= le {
                return None;
            }
            Some((pos, (pos + count).min(le)))
        }
        ViMotion::CharBack => {
            if pos <= ls {
                return None;
            }
            Some((pos.saturating_sub(count).max(ls), pos))
        }
        ViMotion::WordForward { big } => {
            if pos >= buf.len() {
                return None;
            }
            let mut p = pos;
            for _ in 0..count {
                match next_word_start(buf, p, big) {
                    Some(np) if np < buf.len() => p = np,
                    // No further word: the operator consumes the rest of
                    // the buffer (`dw` on the last word deletes to the
                    // end).
                    _ => {
                        p = buf.len();
                        break;
                    }
                }
            }
            if p == pos { None } else { Some((pos, p)) }
        }
        ViMotion::WordEnd { big } => {
            let mut p = pos;
            let mut moved = false;
            for _ in 0..count {
                match word_end_after(buf, p, big) {
                    Some(np) => {
                        p = np;
                        moved = true;
                    }
                    None => break,
                }
            }
            if moved { Some((pos, p + 1)) } else { None }
        }
        ViMotion::WordBack { big } => {
            let mut p = pos;
            let mut moved = false;
            for _ in 0..count {
                match prev_word_start(buf, p, big) {
                    Some(np) => {
                        p = np;
                        moved = true;
                    }
                    None => break,
                }
            }
            if moved { Some((p, pos)) } else { None }
        }
        ViMotion::LineStart => {
            if pos > ls {
                Some((ls, pos))
            } else {
                None
            }
        }
        ViMotion::FirstNonBlank => {
            let f = first_non_blank(buf, pos);
            match f.cmp(&pos) {
                std::cmp::Ordering::Less => Some((f, pos)),
                std::cmp::Ordering::Greater => Some((pos, f)),
                std::cmp::Ordering::Equal => None,
            }
        }
        ViMotion::LineEnd => {
            if pos < le {
                Some((pos, le))
            } else {
                None
            }
        }
        ViMotion::Column => {
            let line_last = if le > ls { le - 1 } else { ls };
            let t = (ls + count - 1).min(line_last);
            match t.cmp(&pos) {
                std::cmp::Ordering::Less => Some((t, pos)),
                std::cmp::Ordering::Greater => Some((pos, t)),
                std::cmp::Ordering::Equal => None,
            }
        }
        ViMotion::FindChar(kind, target) => {
            let idx = find_char_index(buf, pos, kind, target, count as u32)?;
            match kind {
                FindKind::Find => Some((pos, idx + 1)),
                FindKind::To => Some((pos, idx)),
                FindKind::FindBack => Some((idx, pos)),
                FindKind::ToBack => Some((idx + 1, pos)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn word_motions_basic() {
        let b = chars("echo hello world");
        assert_eq!(
            motion_move(&b, 0, ViMotion::WordForward { big: false }, 1),
            Some(5)
        );
        assert_eq!(
            motion_move(&b, 0, ViMotion::WordForward { big: false }, 2),
            Some(11)
        );
        assert_eq!(
            motion_move(&b, 5, ViMotion::WordBack { big: false }, 1),
            Some(0)
        );
        assert_eq!(
            motion_move(&b, 0, ViMotion::WordEnd { big: false }, 1),
            Some(3)
        );
        // e at word end jumps to the next word's end.
        assert_eq!(
            motion_move(&b, 3, ViMotion::WordEnd { big: false }, 1),
            Some(9)
        );
    }

    #[test]
    fn word_vs_bigword_punctuation() {
        let b = chars("a=b c");
        // word: 'a' / '=' / 'b' are three words.
        assert_eq!(
            motion_move(&b, 0, ViMotion::WordForward { big: false }, 1),
            Some(1)
        );
        // bigword: 'a=b' is one bigword.
        assert_eq!(
            motion_move(&b, 0, ViMotion::WordForward { big: true }, 1),
            Some(4)
        );
    }

    #[test]
    fn word_forward_caps_at_last_char() {
        let b = chars("echo hi");
        // w from inside the last word: caps at last character.
        assert_eq!(
            motion_move(&b, 5, ViMotion::WordForward { big: false }, 1),
            Some(6)
        );
        // Already at the last character: alert.
        assert_eq!(
            motion_move(&b, 6, ViMotion::WordForward { big: false }, 1),
            None
        );
    }

    #[test]
    fn line_scoped_motions() {
        let b = chars("  echo hi");
        assert_eq!(motion_move(&b, 5, ViMotion::LineStart, 1), Some(0));
        assert_eq!(motion_move(&b, 5, ViMotion::FirstNonBlank, 1), Some(2));
        assert_eq!(motion_move(&b, 0, ViMotion::LineEnd, 1), Some(8));
        assert_eq!(motion_move(&b, 5, ViMotion::Column, 3), Some(2));
        assert_eq!(motion_move(&b, 0, ViMotion::Column, 100), Some(8));
    }

    #[test]
    fn char_motions_stay_in_logical_line() {
        let b = chars("ab\ncd");
        // l on 'b' (index 1): line end, alert.
        assert_eq!(motion_move(&b, 1, ViMotion::CharForward, 1), None);
        // h on 'c' (index 3): line start, alert.
        assert_eq!(motion_move(&b, 3, ViMotion::CharBack, 1), None);
        assert_eq!(motion_move(&b, 3, ViMotion::LineEnd, 1), Some(4));
        assert_eq!(motion_move(&b, 3, ViMotion::LineStart, 1), Some(3));
    }

    #[test]
    fn find_char_motions() {
        let b = chars("echo hello");
        let f = |pos, kind, c, n| motion_move(&b, pos, ViMotion::FindChar(kind, c), n);
        assert_eq!(f(0, FindKind::Find, 'l', 1), Some(7));
        assert_eq!(f(0, FindKind::Find, 'l', 2), Some(8));
        assert_eq!(f(0, FindKind::To, 'l', 1), Some(6));
        assert_eq!(f(9, FindKind::FindBack, 'e', 1), Some(6));
        assert_eq!(f(9, FindKind::ToBack, 'e', 1), Some(7));
        assert_eq!(f(0, FindKind::Find, 'z', 1), None);
    }

    #[test]
    fn engine_counts_and_zero() {
        let mut e = ViEngine::new();
        e.mode = ViMode::Command;
        assert_eq!(e.resolve_command_key(key('2')), ViOutcome::Pending);
        assert_eq!(e.resolve_command_key(key('3')), ViOutcome::Pending);
        assert_eq!(
            e.resolve_command_key(key('l')),
            ViOutcome::Cmd(ViCmd::Move(ViMotion::CharForward), 23)
        );
        // Bare 0 is the line-start motion...
        assert_eq!(
            e.resolve_command_key(key('0')),
            ViOutcome::Cmd(ViCmd::Move(ViMotion::LineStart), 1)
        );
        // ...but 0 after a digit is part of the count.
        assert_eq!(e.resolve_command_key(key('1')), ViOutcome::Pending);
        assert_eq!(e.resolve_command_key(key('0')), ViOutcome::Pending);
        assert_eq!(
            e.resolve_command_key(key('h')),
            ViOutcome::Cmd(ViCmd::Move(ViMotion::CharBack), 10)
        );
    }

    #[test]
    fn engine_find_char_and_semicolon_memory() {
        let mut e = ViEngine::new();
        e.mode = ViMode::Command;
        assert_eq!(e.resolve_command_key(key('f')), ViOutcome::Pending);
        assert_eq!(
            e.resolve_command_key(key('x')),
            ViOutcome::Cmd(ViCmd::Move(ViMotion::FindChar(FindKind::Find, 'x')), 1)
        );
        assert_eq!(
            e.resolve_command_key(key(';')),
            ViOutcome::Cmd(ViCmd::Move(ViMotion::FindChar(FindKind::Find, 'x')), 1)
        );
        assert_eq!(
            e.resolve_command_key(key(',')),
            ViOutcome::Cmd(ViCmd::Move(ViMotion::FindChar(FindKind::FindBack, 'x')), 1)
        );
        // Count before f applies to the find.
        assert_eq!(e.resolve_command_key(key('2')), ViOutcome::Pending);
        assert_eq!(e.resolve_command_key(key('t')), ViOutcome::Pending);
        assert_eq!(
            e.resolve_command_key(key('y')),
            ViOutcome::Cmd(ViCmd::Move(ViMotion::FindChar(FindKind::To, 'y')), 2)
        );
    }

    #[test]
    fn engine_replace_char_pending() {
        let mut e = ViEngine::new();
        e.mode = ViMode::Command;
        assert_eq!(e.resolve_command_key(key('r')), ViOutcome::Pending);
        assert_eq!(
            e.resolve_command_key(key('Z')),
            ViOutcome::Cmd(ViCmd::ReplaceChar('Z'), 1)
        );
    }

    #[test]
    fn engine_esc_cancels_pending() {
        let mut e = ViEngine::new();
        e.mode = ViMode::Command;
        assert_eq!(e.resolve_command_key(key('3')), ViOutcome::Pending);
        assert_eq!(
            e.resolve_command_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            ViOutcome::Pending
        );
        // Count was discarded.
        assert_eq!(
            e.resolve_command_key(key('l')),
            ViOutcome::Cmd(ViCmd::Move(ViMotion::CharForward), 1)
        );
    }

    #[test]
    fn motion_range_word_forward() {
        let b = chars("echo hello world");
        // dw from 0: up to start of "hello" (exclusive).
        assert_eq!(
            motion_range(&b, 0, ViMotion::WordForward { big: false }, 1),
            Some((0, 5))
        );
        // dw on the last word: consumes to the end.
        assert_eq!(
            motion_range(&b, 11, ViMotion::WordForward { big: false }, 1),
            Some((11, 16))
        );
    }

    #[test]
    fn motion_range_inclusive_motions() {
        let b = chars("echo hello");
        // de: includes the word's last char.
        assert_eq!(
            motion_range(&b, 0, ViMotion::WordEnd { big: false }, 1),
            Some((0, 4))
        );
        // d$: includes the line's last char.
        assert_eq!(motion_range(&b, 5, ViMotion::LineEnd, 1), Some((5, 10)));
        // dfl: includes the found char.
        assert_eq!(
            motion_range(&b, 0, ViMotion::FindChar(FindKind::Find, 'l'), 1),
            Some((0, 8))
        );
        // dtl: excludes the found char.
        assert_eq!(
            motion_range(&b, 0, ViMotion::FindChar(FindKind::To, 'l'), 1),
            Some((0, 7))
        );
    }

    #[test]
    fn motion_range_backward_excludes_cursor() {
        let b = chars("echo hello");
        // db from 'h' of hello: deletes "echo " but not 'h'.
        assert_eq!(
            motion_range(&b, 5, ViMotion::WordBack { big: false }, 1),
            Some((0, 5))
        );
        assert_eq!(motion_range(&b, 5, ViMotion::LineStart, 1), Some((0, 5)));
        // d0 at line start: nothing to do.
        assert_eq!(motion_range(&b, 0, ViMotion::LineStart, 1), None);
    }

    #[test]
    fn engine_operator_sequences() {
        let mut e = ViEngine::new();
        e.mode = ViMode::Command;
        // dw
        assert_eq!(e.resolve_command_key(key('d')), ViOutcome::Pending);
        assert_eq!(
            e.resolve_command_key(key('w')),
            ViOutcome::Cmd(
                ViCmd::Op(OpKind::Delete, ViMotion::WordForward { big: false }),
                1
            )
        );
        // 2d3w = 6 words
        for k in ['2', 'd', '3'] {
            assert_eq!(e.resolve_command_key(key(k)), ViOutcome::Pending);
        }
        assert_eq!(
            e.resolve_command_key(key('w')),
            ViOutcome::Cmd(
                ViCmd::Op(OpKind::Delete, ViMotion::WordForward { big: false }),
                6
            )
        );
        // dd
        assert_eq!(e.resolve_command_key(key('d')), ViOutcome::Pending);
        assert_eq!(
            e.resolve_command_key(key('d')),
            ViOutcome::Cmd(ViCmd::OpLine(OpKind::Delete), 1)
        );
        // count ignored for $ under an operator
        assert_eq!(e.resolve_command_key(key('5')), ViOutcome::Pending);
        assert_eq!(e.resolve_command_key(key('c')), ViOutcome::Pending);
        assert_eq!(
            e.resolve_command_key(key('$')),
            ViOutcome::Cmd(ViCmd::Op(OpKind::Change, ViMotion::LineEnd), 1)
        );
        // df x
        assert_eq!(e.resolve_command_key(key('d')), ViOutcome::Pending);
        assert_eq!(e.resolve_command_key(key('f')), ViOutcome::Pending);
        assert_eq!(
            e.resolve_command_key(key('x')),
            ViOutcome::Cmd(
                ViCmd::Op(OpKind::Delete, ViMotion::FindChar(FindKind::Find, 'x')),
                1
            )
        );
    }

    #[test]
    fn engine_repeat_count_sentinel() {
        let mut e = ViEngine::new();
        e.mode = ViMode::Command;
        // Bare `.` carries the "no explicit count" sentinel 0.
        assert_eq!(
            e.resolve_command_key(key('.')),
            ViOutcome::Cmd(ViCmd::Repeat, 0)
        );
        assert_eq!(e.resolve_command_key(key('3')), ViOutcome::Pending);
        assert_eq!(
            e.resolve_command_key(key('.')),
            ViOutcome::Cmd(ViCmd::Repeat, 3)
        );
    }

    #[test]
    fn engine_alias_macro_pending() {
        let mut e = ViEngine::new();
        e.mode = ViMode::Command;
        assert_eq!(e.resolve_command_key(key('@')), ViOutcome::Pending);
        assert_eq!(
            e.resolve_command_key(key('z')),
            ViOutcome::Cmd(ViCmd::AliasMacro('z'), 1)
        );
        // Expansion commands resolve directly.
        assert_eq!(
            e.resolve_command_key(key('=')),
            ViOutcome::Cmd(ViCmd::ExpandList, 1)
        );
        assert_eq!(
            e.resolve_command_key(key('\\')),
            ViOutcome::Cmd(ViCmd::CompleteUnique, 1)
        );
        assert_eq!(
            e.resolve_command_key(key('#')),
            ViOutcome::Cmd(ViCmd::CommentSubmit, 1)
        );
        // v with and without a number (0 = current line sentinel).
        assert_eq!(
            e.resolve_command_key(key('v')),
            ViOutcome::Cmd(ViCmd::EditInEditor, 0)
        );
        assert_eq!(e.resolve_command_key(key('2')), ViOutcome::Pending);
        assert_eq!(
            e.resolve_command_key(key('v')),
            ViOutcome::Cmd(ViCmd::EditInEditor, 2)
        );
    }

    #[test]
    fn engine_count_is_capped() {
        let mut e = ViEngine::new();
        e.mode = ViMode::Command;
        for _ in 0..13 {
            assert_eq!(e.resolve_command_key(key('9')), ViOutcome::Pending);
        }
        assert_eq!(
            e.resolve_command_key(key('l')),
            ViOutcome::Cmd(ViCmd::Move(ViMotion::CharForward), 1_000_000)
        );
        // Operator-side accumulation and the multiplied total cap too.
        assert_eq!(e.resolve_command_key(key('9')), ViOutcome::Pending);
        assert_eq!(e.resolve_command_key(key('9')), ViOutcome::Pending);
        assert_eq!(e.resolve_command_key(key('d')), ViOutcome::Pending);
        for _ in 0..13 {
            assert_eq!(e.resolve_command_key(key('9')), ViOutcome::Pending);
        }
        assert_eq!(
            e.resolve_command_key(key('w')),
            ViOutcome::Cmd(
                ViCmd::Op(OpKind::Delete, ViMotion::WordForward { big: false }),
                1_000_000
            )
        );
    }

    #[test]
    fn engine_semicolon_without_find_bells() {
        let mut e = ViEngine::new();
        e.mode = ViMode::Command;
        assert_eq!(
            e.resolve_command_key(key(';')),
            ViOutcome::Cmd(ViCmd::Bell, 1)
        );
    }
}
