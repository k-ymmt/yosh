/// All editing operations that the line editor can perform.
/// Serves as the contract between Keymap (key → action) and LineEditor (action → mutation).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EditAction {
    // Character input
    InsertChar(char),

    // Cursor movement
    MoveBackward,
    MoveForward,
    MoveToStart,
    MoveToEnd,
    MoveBackwardWord,
    MoveForwardWord,

    // Delete (does NOT enter kill ring)
    DeleteBackward,
    DeleteForward,

    // Kill (enters kill ring)
    KillToEnd,
    KillToStart,
    KillBackwardWord,
    KillForwardWord,

    // Yank
    Yank,
    YankPop,

    // Editing
    TransposeChars,
    TransposeWords,
    UpcaseWord,
    DowncaseWord,
    CapitalizeWord,

    // Undo
    Undo,

    // Other
    ClearScreen,
    Cancel,
    AcceptSuggestion,
    AcceptWordSuggestion,
    SetNumericArg(u8),

    // Control (maps to KeyAction for REPL loop)
    Submit,
    Eof,
    Interrupt,
    FuzzySearch,
    TabComplete,
    HistoryPrev,
    HistoryNext,
    Noop,
}

impl EditAction {
    /// Returns true if this action is a kill operation (text goes to kill ring).
    pub fn is_kill(&self) -> bool {
        matches!(
            self,
            Self::KillToEnd | Self::KillToStart | Self::KillBackwardWord | Self::KillForwardWord
        )
    }
}
