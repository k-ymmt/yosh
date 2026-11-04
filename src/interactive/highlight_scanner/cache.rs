//! Incremental rescan cache for the highlight scanner.
//!
//! Stores the previous input + spans and a series of checkpoints
//! (state snapshots at fixed positions) so that an unchanged prefix
//! can be reused across keystrokes.

use super::super::highlight::ColorSpan;
use super::state::{ScanMode, ScannerState};

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

    /// Return the nearest usable checkpoint strictly before `pos` (the
    /// first changed char), so that restarting there reproduces exactly
    /// the spans a full scan would emit:
    /// - A checkpoint marks a token boundary of the old input; one AT the
    ///   changed char may no longer be a boundary — an inserted word char
    ///   joins the previous word — and would split that word in two.
    /// - Inside a quote or expansion the scanner re-emits that
    ///   construct's span from its opener when it resumes, duplicating
    ///   the cached span that ends at the checkpoint. Only checkpoints
    ///   in plain command context (`Normal`, possibly nested in `$(`)
    ///   resume cleanly.
    pub(super) fn nearest_checkpoint(&self, pos: usize) -> Option<(usize, ScannerState)> {
        self.checkpoints
            .iter()
            .rev()
            .find(|(cp, st)| {
                *cp < pos
                    && st
                        .mode_stack
                        .iter()
                        .all(|m| matches!(m, ScanMode::Normal | ScanMode::CommandSub { .. }))
            })
            .cloned()
    }

    pub(super) fn clear(&mut self) {
        self.prev_input.clear();
        self.prev_spans.clear();
        self.checkpoints.clear();
    }
}
