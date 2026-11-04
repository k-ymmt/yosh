//! Semantic commands produced by the vi key state machines: what a
//! resolved key sequence *means*, independent of the buffer.

use super::VisualKind;
use super::motion::ViMotion;

/// Operator commands that pair with a motion (`d` / `c` / `y`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpKind {
    Delete,
    Change,
    Yank,
}

impl OpKind {
    pub(super) fn cmd_char(self) -> char {
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
    /// vim `d`/`c`/`y` + `i`/`a` + object char (`diw`, `ci"`, …).
    OpObject {
        op: OpKind,
        around: bool,
        obj: char,
    },
    /// `s` — delete count chars and enter insert mode.
    SubstChar,
    /// `p`
    PutAfter,
    /// `P`
    PutBefore,
    /// `u`
    Undo,
    /// vim `Ctrl-R` — redo (`[count]` supported).
    Redo,
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
    /// line) in the vi editor, then execute the result. POSIX flavor
    /// only; the vim flavor binds `v` to VISUAL.
    EditInEditor,
    /// vim `v` / `V` — enter VISUAL mode (charwise / linewise).
    EnterVisual(VisualKind),
    /// vim `Ctrl-X Ctrl-E` — edit the buffer in the external editor;
    /// the result replaces the buffer without submitting.
    EditBuffer,
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

/// Semantic command produced by the VISUAL-mode key state machine
/// (vim flavor only).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VisualCmd {
    /// Motion: moves `pos`, extending the selection.
    Move(ViMotion),
    /// `j` / Down — pure buffer line motion (stops at the last line).
    LineDown,
    /// `k` / Up — pure buffer line motion (stops at the first line).
    LineUp,
    /// `d` / `x` — delete the selection.
    Delete,
    /// `c` / `s` — delete the selection and enter Insert.
    Change,
    /// `y` — yank the selection.
    Yank,
    /// `D` — delete the whole logical lines touched by the selection.
    DeleteLines,
    /// `C` / `S` / `R` — linewise change of the touched lines.
    ChangeLines,
    /// `Y` — linewise yank of the touched lines.
    YankLines,
    /// `p` / `P` — replace the selection with the register contents.
    Put {
        /// `P`: the unnamed register is left unchanged.
        before: bool,
    },
    /// `r c` — replace every selected character (except `'\n'`).
    ReplaceChar(char),
    /// `~` — toggle case of the selection.
    ToggleCase,
    /// `i`/`a` + object char — select or extend by the text object.
    TextObject {
        around: bool,
        obj: char,
    },
    /// `o` — swap cursor and anchor.
    SwapEnds,
    /// `v` / `V` — same kind exits, other kind switches (§2.3).
    ToggleKind(VisualKind),
    /// Esc (no pending) / Ctrl-C — exit to Command mode.
    Exit,
    /// Enter — submit the whole line (selection discarded).
    Submit,
    Eof,
    ClearScreen,
    Bell,
}

/// Resolution result for VISUAL-mode keys.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VisualOutcome {
    Cmd(VisualCmd, u32),
    Pending,
}

/// The most recent buffer-modifying command, for `.`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChangeRecord {
    pub cmd: ViCmd,
    pub count: u32,
}
