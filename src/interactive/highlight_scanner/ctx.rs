//! Per-scan context — the bundle of mutable state every scanner needs.
//!
//! Built fresh by `scan_from` each loop iteration and passed to the
//! per-mode scanner functions. Holds:
//! - `input` — the input character slice (read-only).
//! - `state` — the mode stack and word/command-position flags.
//! - `spans` — the output span vector under construction.
//! - `checker` — `CommandChecker`. Only `scan_word` reads it (to
//!   determine if a word is a known command), but it's bundled here
//!   to keep every scanner's signature uniform.

use super::super::command_checker::CommandChecker;
use super::super::highlight::ColorSpan;
use super::state::ScannerState;

#[allow(dead_code)] // PR-B converts scanners to consume this; unused until then.
pub(super) struct ScanCtx<'a> {
    pub input: &'a [char],
    pub state: &'a mut ScannerState,
    pub spans: &'a mut Vec<ColorSpan>,
    pub checker: &'a mut CommandChecker,
}
