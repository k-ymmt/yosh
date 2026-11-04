//! Function-call scopes, and the `getopts` / `OPTIND` state each scope
//! keeps: `OPTIND` is saved and reset on entry, restored on return, and
//! writes to it are counted so `getopts` can notice a script resetting it.

use super::{Scope, VarStore, Variable};
use crate::env::fxhash::FxHashMap;

impl VarStore {
    // ── Scope management ────────────────────────────────────────────────

    /// Push a new scope with the given positional parameters.
    /// Used for function calls.
    ///
    /// Saves the caller's current `OPTIND` value into the new scope's
    /// `saved_optind` and resets the visible `OPTIND` to `"1"`. The
    /// stacked-options subcursor starts at `0`.
    pub fn push_scope(&mut self, positional_params: Vec<String>) {
        self.export_cache.invalidate();
        // Snapshot caller's OPTIND (may be unset → None).
        let saved_optind = self.get("OPTIND").map(|s| s.to_string());
        self.scopes.push(Scope::new(
            FxHashMap::default(),
            positional_params,
            saved_optind,
        ));
        // Set OPTIND="1" in the new (top) scope so the function body
        // sees a fresh parse position. Direct write into top scope to
        // avoid POSIX "assign in caller" semantics of `set()`.
        self.scopes
            .last_mut()
            .unwrap()
            .vars
            .insert("OPTIND".to_string(), Variable::new("1"));
    }

    /// Pop the current scope, restoring the previous scope's positional
    /// parameters. Panics if only the global scope remains.
    ///
    /// Restores the caller's `OPTIND` from the popped scope's
    /// `saved_optind` snapshot (writing into whichever underlying scope
    /// already holds OPTIND, or creating it in the new top scope).
    pub fn pop_scope(&mut self) {
        self.export_cache.invalidate();
        assert!(self.scopes.len() > 1, "cannot pop the global scope");
        let popped = self.scopes.pop().unwrap();
        if let Some(prev_optind) = popped.saved_optind {
            // Write back into the now-current scope chain. Use `set` so
            // the value lands where OPTIND was originally defined, then
            // mark the internal restore as observed so it does not reset
            // the caller's pending stacked-option cursor.
            if self.set("OPTIND", prev_optind).is_ok() {
                self.mark_getopts_observed_optind();
            }
        }
    }

    /// Return the current scope depth. 1 = global scope only.
    pub fn scope_depth(&self) -> usize {
        self.scopes.len()
    }

    // ── getopts subcursor (top scope) ───────────────────────────────────

    /// Get the current scope's `getopts` stacked-options subcursor.
    pub fn getopts_subindex(&self) -> usize {
        self.scopes.last().unwrap().getopts_subindex
    }

    /// Set the current scope's `getopts` stacked-options subcursor.
    pub fn set_getopts_subindex(&mut self, value: usize) {
        self.scopes.last_mut().unwrap().getopts_subindex = value;
    }

    /// Return true if `OPTIND` has been written since `getopts` last
    /// observed the current scope's write generation.
    pub fn optind_written_since_getopts(&self) -> bool {
        let scope = self.scopes.last().unwrap();
        scope.optind_write_generation != scope.getopts_observed_optind_generation
    }

    /// Mark the current scope's `OPTIND` write generation as observed by
    /// `getopts`.
    pub fn mark_getopts_observed_optind(&mut self) {
        let scope = self.scopes.last_mut().unwrap();
        scope.getopts_observed_optind_generation = scope.optind_write_generation;
    }

    /// Count a successful write to `name` in the current scope's `OPTIND`
    /// write generation when it is `OPTIND`.
    pub(super) fn note_optind_write(&mut self, name: &str) {
        if name == "OPTIND" {
            let scope = self.scopes.last_mut().unwrap();
            scope.optind_write_generation = scope.optind_write_generation.wrapping_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_pop_scope_positional_params() {
        let mut store = VarStore::new();
        store.set_positional_params(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(store.positional_params(), &["a", "b"]);

        store.push_scope(vec!["x".to_string(), "y".to_string(), "z".to_string()]);
        assert_eq!(store.positional_params(), &["x", "y", "z"]);

        store.pop_scope();
        assert_eq!(store.positional_params(), &["a", "b"]);
    }

    #[test]
    fn test_scope_variable_lookup_walks_chain() {
        let mut store = VarStore::new();
        store.set("FOO", "global").unwrap();

        store.push_scope(vec![]);
        // Variable from global scope is visible
        assert_eq!(store.get("FOO"), Some("global"));

        // Setting FOO in function scope updates the global scope (POSIX)
        store.set("FOO", "updated").unwrap();
        store.pop_scope();
        assert_eq!(store.get("FOO"), Some("updated"));
    }

    #[test]
    fn test_scope_new_variable_goes_to_global() {
        let mut store = VarStore::new();
        store.push_scope(vec![]);
        store.set("NEW_VAR", "value").unwrap();
        store.pop_scope();
        // Variable created inside function scope persists in global
        assert_eq!(store.get("NEW_VAR"), Some("value"));
    }

    #[test]
    fn test_scope_readonly_across_scopes() {
        let mut store = VarStore::new();
        store.set("RO", "immutable").unwrap();
        store.set_readonly("RO");

        store.push_scope(vec![]);
        let result = store.set("RO", "changed");
        assert!(result.is_err());
        assert_eq!(store.get("RO"), Some("immutable"));
        store.pop_scope();
    }

    #[test]
    fn test_scope_export_across_scopes() {
        let mut store = VarStore::new();
        store.set("EX", "value").unwrap();

        store.push_scope(vec![]);
        store.export("EX");
        store.pop_scope();

        assert!(store.get_var("EX").unwrap().exported);
    }

    #[test]
    fn test_scope_unset_across_scopes() {
        let mut store = VarStore::new();
        store.set("DEL", "value").unwrap();

        store.push_scope(vec![]);
        store.unset("DEL").unwrap();
        store.pop_scope();

        assert_eq!(store.get("DEL"), None);
    }

    #[test]
    fn push_scope_snapshots_optind_and_resets_to_one() {
        let mut store = VarStore::new();
        store.set("OPTIND", "5").unwrap();

        store.push_scope(vec!["a".into(), "b".into()]);
        assert_eq!(store.get("OPTIND"), Some("1"));

        store.pop_scope();
        assert_eq!(store.get("OPTIND"), Some("5"));
    }

    #[test]
    fn push_scope_initial_subindex_is_zero() {
        let mut store = VarStore::new();
        store.push_scope(vec![]);
        assert_eq!(store.getopts_subindex(), 0);
    }

    #[test]
    fn set_getopts_subindex_round_trips() {
        let mut store = VarStore::new();
        store.set_getopts_subindex(3);
        assert_eq!(store.getopts_subindex(), 3);
    }

    #[test]
    fn push_scope_resets_subindex_and_pop_restores() {
        let mut store = VarStore::new();
        store.set_getopts_subindex(7);

        store.push_scope(vec![]);
        assert_eq!(store.getopts_subindex(), 0);
        store.set_getopts_subindex(2);

        store.pop_scope();
        assert_eq!(store.getopts_subindex(), 7);
    }

    #[test]
    fn optind_write_since_getopts_detects_same_value_assignment() {
        let mut store = VarStore::new();
        store.set("OPTIND", "1").unwrap();
        store.mark_getopts_observed_optind();
        assert!(!store.optind_written_since_getopts());

        store.set("OPTIND", "1").unwrap();
        assert!(store.optind_written_since_getopts());
    }

    #[test]
    fn optind_write_since_getopts_detects_assignment_with_options() {
        let mut store = VarStore::new();
        store.set("OPTIND", "1").unwrap();
        store.mark_getopts_observed_optind();
        assert!(!store.optind_written_since_getopts());

        store.set_with_options("OPTIND", "1", false).unwrap();
        assert!(store.optind_written_since_getopts());
    }

    #[test]
    fn optind_write_generation_is_scope_local() {
        let mut store = VarStore::new();
        store.set("OPTIND", "1").unwrap();
        store.mark_getopts_observed_optind();

        store.push_scope(vec![]);
        assert!(!store.optind_written_since_getopts());
        store.set("OPTIND", "1").unwrap();
        assert!(store.optind_written_since_getopts());

        store.pop_scope();
        assert!(!store.optind_written_since_getopts());
    }

    #[test]
    fn pop_scope_optind_restore_does_not_trigger_caller_reset() {
        let mut store = VarStore::new();
        store.set("OPTIND", "1").unwrap();
        store.set_getopts_subindex(2);
        store.mark_getopts_observed_optind();

        store.push_scope(vec![]);
        store.set("OPTIND", "1").unwrap();

        store.pop_scope();
        assert_eq!(store.getopts_subindex(), 2);
        assert!(!store.optind_written_since_getopts());
    }
}
