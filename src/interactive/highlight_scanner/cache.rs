//! Incremental rescan cache for the highlight scanner.
//!
//! Stores the previous input + spans and a series of checkpoints
//! (state snapshots at fixed positions) so that an unchanged prefix
//! can be reused across keystrokes.

use super::super::highlight::ColorSpan;
use super::state::ScannerState;

pub(super) struct HighlightCache {
    pub prev_input: Vec<char>,
    pub prev_spans: Vec<ColorSpan>,
    pub checkpoints: Vec<(usize, ScannerState)>,
    pub checkpoint_interval: usize,
}

impl HighlightCache {
    pub(super) fn new() -> Self {
        Self {
            prev_input: Vec::new(),
            prev_spans: Vec::new(),
            checkpoints: Vec::new(),
            checkpoint_interval: 32,
        }
    }

    /// Find the first position where `input` differs from the cached input.
    pub(super) fn diff_pos(&self, input: &[char]) -> usize {
        self.prev_input
            .iter()
            .zip(input.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(self.prev_input.len().min(input.len()))
    }

    /// Return the nearest checkpoint at or before `pos`.
    pub(super) fn nearest_checkpoint(&self, pos: usize) -> Option<(usize, ScannerState)> {
        self.checkpoints
            .iter()
            .rev()
            .find(|(cp, _)| *cp <= pos)
            .cloned()
    }

    pub(super) fn clear(&mut self) {
        self.prev_input.clear();
        self.prev_spans.clear();
        self.checkpoints.clear();
    }
}
