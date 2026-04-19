// src/interactive/keymap.rs

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::edit_action::EditAction;

/// Minimal buffer state needed by the keymap to make context-dependent decisions.
pub struct BufferState {
    pub is_empty: bool,
    pub at_end: bool,
    pub has_suggestion: bool,
    pub last_action: EditAction,
}

/// Maps key events to edit actions. Manages numeric argument accumulation.
pub struct Keymap {
    numeric_arg: Option<u32>,
}

impl Keymap {
    pub fn new() -> Self {
        Self { numeric_arg: None }
    }

    /// Return the currently accumulated numeric argument, if any.
    pub fn pending_numeric_arg(&self) -> Option<u32> {
        self.numeric_arg
    }

    /// Resolve a key event into an edit action and repeat count.
    /// Consumes any pending numeric argument as the repeat count.
    pub fn resolve(&mut self, key: KeyEvent, state: &BufferState) -> (EditAction, u32) {
        let mods = key.modifiers;
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        let alt = mods.contains(KeyModifiers::ALT);

        // Alt+digit → accumulate numeric argument
        if alt && !ctrl {
            if let KeyCode::Char(ch) = key.code {
                if let Some(digit) = ch.to_digit(10) {
                    let current = self.numeric_arg.unwrap_or(0);
                    self.numeric_arg = Some(current * 10 + digit);
                    return (EditAction::SetNumericArg(digit as u8), 1);
                }
            }
        }

        // Ctrl+G → cancel (reset numeric arg)
        if ctrl && key.code == KeyCode::Char('g') {
            self.numeric_arg = None;
            return (EditAction::Cancel, 1);
        }

        // Consume numeric arg for repeat count
        let count = self.numeric_arg.take().unwrap_or(1);

        let action = match (key.code, ctrl, alt) {
            // --- Control keybindings ---
            (KeyCode::Char('a'), true, false) => EditAction::MoveToStart,
            (KeyCode::Char('b'), true, false) => EditAction::MoveBackward,
            (KeyCode::Char('c'), true, false) => EditAction::Interrupt,
            (KeyCode::Char('d'), true, false) => {
                if state.is_empty {
                    EditAction::Eof
                } else {
                    EditAction::DeleteForward
                }
            }
            (KeyCode::Char('e'), true, false) => EditAction::MoveToEnd,
            (KeyCode::Char('f'), true, false) => {
                if state.at_end && state.has_suggestion {
                    EditAction::AcceptSuggestion
                } else {
                    EditAction::MoveForward
                }
            }
            // Ctrl+J (LF, 0x0A) is treated as Enter.  When input arrives via
            // the PTY before raw mode is enabled, the kernel ICRNL flag converts
            // CR to LF.  crossterm interprets a raw-mode LF as Ctrl+J rather
            // than Enter, so we map it explicitly to avoid dropped input.
            (KeyCode::Char('j'), true, false) => EditAction::Submit,
            (KeyCode::Char('k'), true, false) => EditAction::KillToEnd,
            (KeyCode::Char('l'), true, false) => EditAction::ClearScreen,
            (KeyCode::Char('r'), true, false) => EditAction::FuzzySearch,
            (KeyCode::Char('t'), true, false) => EditAction::TransposeChars,
            (KeyCode::Char('u'), true, false) => EditAction::KillToStart,
            (KeyCode::Char('w'), true, false) => EditAction::KillBackwardWord,
            (KeyCode::Char('y'), true, false) => EditAction::Yank,
            (KeyCode::Char('_'), true, false) => EditAction::Undo,

            // --- Alt keybindings ---
            (KeyCode::Char('b'), false, true) => EditAction::MoveBackwardWord,
            (KeyCode::Char('c'), false, true) => EditAction::CapitalizeWord,
            (KeyCode::Char('d'), false, true) => EditAction::KillForwardWord,
            (KeyCode::Char('f'), false, true) => {
                if state.has_suggestion {
                    EditAction::AcceptWordSuggestion
                } else {
                    EditAction::MoveForwardWord
                }
            }
            (KeyCode::Char('l'), false, true) => EditAction::DowncaseWord,
            (KeyCode::Char('t'), false, true) => EditAction::TransposeWords,
            (KeyCode::Char('u'), false, true) => EditAction::UpcaseWord,
            (KeyCode::Char('y'), false, true) => {
                if state.last_action == EditAction::Yank || state.last_action == EditAction::YankPop
                {
                    EditAction::YankPop
                } else {
                    EditAction::Noop
                }
            }
            (KeyCode::Backspace, false, true) => EditAction::KillBackwardWord,

            // --- Plain keys ---
            (KeyCode::Enter, false, false) => EditAction::Submit,
            (KeyCode::Backspace, false, false) => EditAction::DeleteBackward,
            (KeyCode::Delete, false, false) => EditAction::DeleteForward,
            (KeyCode::Tab, false, false) => EditAction::TabComplete,
            (KeyCode::Home, false, false) => EditAction::MoveToStart,
            (KeyCode::End, false, false) => EditAction::MoveToEnd,
            (KeyCode::Left, _, _) => EditAction::MoveBackward,
            (KeyCode::Right, _, _) => {
                if state.at_end && state.has_suggestion {
                    EditAction::AcceptSuggestion
                } else {
                    EditAction::MoveForward
                }
            }
            (KeyCode::Up, _, _) => EditAction::HistoryPrev,
            (KeyCode::Down, _, _) => EditAction::HistoryNext,

            // --- Printable character (no Ctrl) ---
            (KeyCode::Char(ch), false, false) => EditAction::InsertChar(ch),

            // --- Everything else ---
            _ => EditAction::Noop,
        };

        (action, count)
    }
}
