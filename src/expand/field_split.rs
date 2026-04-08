use crate::env::ShellEnv;
use super::ExpandedField;

/// Split fields according to IFS.
/// Phase 3 stub: pass through unchanged.
pub fn split(_env: &ShellEnv, fields: Vec<ExpandedField>) -> Vec<ExpandedField> {
    fields
}
