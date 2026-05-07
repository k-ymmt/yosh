//! Scanner state machine — modes and the mutable state carried through scan.

use crate::interactive::highlight::{ColorSpan, HighlightStyle};

/// Each mode represents a different parsing context inside the input line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ScanMode {
    Normal,
    SingleQuote { start: usize },
    DoubleQuote { start: usize },
    DollarSingleQuote { start: usize },
    Parameter { start: usize, braced: bool },
    CommandSub { start: usize },
    Backtick { start: usize },
    ArithSub { start: usize },
    Comment { start: usize },
}

/// Mutable state carried through the scan.
#[derive(Debug, Clone)]
pub(super) struct ScannerState {
    pub mode_stack: Vec<ScanMode>,
    /// True when the next non-whitespace character starts a new token at
    /// the beginning of a word (used for `#` comment detection and `~`).
    pub word_start: bool,
    /// True when the next word is in command position (first word of a
    /// simple command, or immediately after `|`, `&&`, `||`, `;`, etc.).
    pub command_position: bool,
}

impl ScannerState {
    pub(super) fn new() -> Self {
        Self {
            mode_stack: vec![ScanMode::Normal],
            word_start: true,
            command_position: true,
        }
    }

    pub(super) fn current_mode(&self) -> &ScanMode {
        self.mode_stack.last().unwrap_or(&ScanMode::Normal)
    }

    pub(super) fn push_mode(&mut self, mode: ScanMode) {
        self.mode_stack.push(mode);
    }

    pub(super) fn pop_mode(&mut self) {
        if self.mode_stack.len() > 1 {
            self.mode_stack.pop();
        }
    }
}

/// Append unclosed-quote / unclosed-expansion error spans to `spans`.
/// Called from `scan_from` after the main scan loop completes; if any
/// non-Normal mode is still on the stack, the corresponding `start`
/// position gets an Error span.
pub(super) fn mark_unclosed_errors(
    state: &ScannerState,
    input_len: usize,
    spans: &mut Vec<ColorSpan>,
) {
    for mode in &state.mode_stack {
        match mode {
            ScanMode::SingleQuote { start }
            | ScanMode::DoubleQuote { start }
            | ScanMode::DollarSingleQuote { start }
            | ScanMode::Backtick { start } => {
                spans.push(ColorSpan {
                    start: *start,
                    end: input_len,
                    style: HighlightStyle::Error,
                });
            }
            ScanMode::CommandSub { start } | ScanMode::Parameter { start, .. } => {
                // Error on opening delimiter only (2 chars for $( or ${)
                spans.push(ColorSpan {
                    start: *start,
                    end: (*start + 2).min(input_len),
                    style: HighlightStyle::Error,
                });
            }
            ScanMode::ArithSub { start } => {
                // Error on opening delimiter only (3 chars for $((  )
                spans.push(ColorSpan {
                    start: *start,
                    end: (*start + 3).min(input_len),
                    style: HighlightStyle::Error,
                });
            }
            ScanMode::Normal | ScanMode::Comment { .. } => {}
        }
    }
}
