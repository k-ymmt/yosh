use crate::env::ShellEnv;
use super::ExpandedField;

/// Perform pathname expansion (glob).
/// Phase 3 stub: pass through unchanged.
pub fn expand(_env: &ShellEnv, fields: Vec<ExpandedField>) -> Vec<ExpandedField> {
    fields
}
