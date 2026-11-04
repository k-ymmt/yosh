// src/interactive/vi/mod.rs

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
//!
//! Submodules:
//! - `command`: the semantic command vocabulary the key state machines
//!   produce ([`ViCmd`], [`VisualCmd`], operator/insert/search kinds).
//! - `engine`: [`ViEngine`] state, the pending-input model, and the
//!   motion-key table shared by both key state machines.
//! - `command_keys` / `visual_keys`: Command-mode and (vim) VISUAL-mode
//!   key resolution.
//! - `motion`: motion types and the motion-target math.

mod command;
mod command_keys;
mod engine;
mod motion;
mod visual_keys;

pub use command::{InsertAt, OpKind, SearchDir, ViCmd, ViOutcome, VisualCmd, VisualOutcome};
pub use engine::ViEngine;
pub(crate) use motion::char_class;
pub use motion::{ViMotion, is_blank, line_end, line_start, motion_move, motion_range};
// Library-API re-exports that the binary's own copy of this module tree
// never names.
#[allow(unused_imports)]
pub use command::ChangeRecord;
#[allow(unused_imports)]
pub use motion::FindKind;

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
/// mode. `Visual` exists only in the vim flavor.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ViMode {
    #[default]
    Insert,
    /// = Vim "Normal"; name kept for POSIX-vi continuity.
    Command,
    /// vim VISUAL mode. `anchor` is the char index where VISUAL was
    /// entered; the selection is `min(anchor, pos) ..= max(anchor, pos)`
    /// (inclusive, Vim semantics), expanded to logical-line boundaries
    /// for `Line`.
    Visual { kind: VisualKind, anchor: usize },
}

/// Charwise (`v`) vs linewise (`V`) VISUAL selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VisualKind {
    Char,
    Line,
}

#[cfg(test)]
mod tests;
