use std::collections::HashMap;

/// A shell variable with its value and attributes.
#[derive(Debug, Clone, PartialEq)]
pub struct Variable {
    pub value: String,
    pub exported: bool,
    pub readonly: bool,
}

impl Variable {
    pub fn new(value: impl Into<String>) -> Self {
        Variable {
            value: value.into(),
            exported: false,
            readonly: false,
        }
    }

    pub fn new_exported(value: impl Into<String>) -> Self {
        Variable {
            value: value.into(),
            exported: true,
            readonly: false,
        }
    }
}

/// A single scope in the scope chain.
#[derive(Debug, Clone)]
struct Scope {
    vars: HashMap<String, Variable>,
    positional_params: Vec<String>,
    /// POSIX `getopts` cursor within a stacked argv element (e.g. `-abc`).
    /// `0` means "advance to the next argv element on the next call."
    getopts_subindex: usize,
    /// Successful writes to visible `OPTIND` in this scope.
    optind_write_generation: u64,
    /// Last `OPTIND` write generation observed by `getopts`.
    getopts_observed_optind_generation: u64,
    /// `OPTIND` value snapshot saved on `push_scope`, restored on `pop_scope`.
    /// `None` outside any function call (global scope).
    saved_optind: Option<String>,
}

/// Storage for shell variables with scope chain support.
///
/// Scopes are stacked: `scopes[0]` is global, `scopes.last()` is current.
/// Variable lookups walk from top to bottom. Writes go to the scope that
/// already contains the variable, or to the global scope if the variable
/// is new (POSIX: function assignments affect the caller).
///
/// Positional parameters (`$1`, `$2`, ...) are per-scope — each function
/// invocation gets its own set.
#[derive(Debug, Clone)]
pub struct VarStore {
    scopes: Vec<Scope>,
    environ_cache: Option<Vec<(String, String)>>,
}

impl VarStore {
    /// Create an empty VarStore with a single global scope.
    pub fn new() -> Self {
        VarStore {
            scopes: vec![Scope {
                vars: HashMap::new(),
                positional_params: Vec::new(),
                getopts_subindex: 0,
                optind_write_generation: 0,
                getopts_observed_optind_generation: 0,
                saved_optind: None,
            }],
            environ_cache: None,
        }
    }

    /// Initialize from the current process environment.
    ///
    /// Uses `vars_os` + the byteenc escape encoding so names and values
    /// that are not valid UTF-8 are imported losslessly instead of being
    /// dropped, and re-exported byte-identically to children.
    pub fn from_environ() -> Self {
        use std::os::unix::ffi::OsStrExt;
        let mut vars = HashMap::new();
        for (key, value) in std::env::vars_os() {
            let key = crate::byteenc::encode_bytes(key.as_bytes()).into_owned();
            let value = crate::byteenc::encode_bytes(value.as_bytes()).into_owned();
            vars.insert(key, Variable::new_exported(value));
        }
        VarStore {
            scopes: vec![Scope {
                vars,
                positional_params: Vec::new(),
                getopts_subindex: 0,
                optind_write_generation: 0,
                getopts_observed_optind_generation: 0,
                saved_optind: None,
            }],
            environ_cache: None,
        }
    }

    // ── Scope management ────────────────────────────────────────────────

    /// Push a new scope with the given positional parameters.
    /// Used for function calls.
    ///
    /// Saves the caller's current `OPTIND` value into the new scope's
    /// `saved_optind` and resets the visible `OPTIND` to `"1"`. The
    /// stacked-options subcursor starts at `0`.
    pub fn push_scope(&mut self, positional_params: Vec<String>) {
        self.environ_cache = None;
        // Snapshot caller's OPTIND (may be unset → None).
        let saved_optind = self.get("OPTIND").map(|s| s.to_string());
        self.scopes.push(Scope {
            vars: HashMap::new(),
            positional_params,
            getopts_subindex: 0,
            optind_write_generation: 0,
            getopts_observed_optind_generation: 0,
            saved_optind,
        });
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
        self.environ_cache = None;
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

    /// Return the current scope depth. 1 = global scope only.
    pub fn scope_depth(&self) -> usize {
        self.scopes.len()
    }

    // ── Positional parameters ───────────────────────────────────────────

    /// Get the current scope's positional parameters.
    pub fn positional_params(&self) -> &[String] {
        &self.scopes.last().unwrap().positional_params
    }

    /// Set the current scope's positional parameters.
    pub fn set_positional_params(&mut self, params: Vec<String>) {
        self.scopes.last_mut().unwrap().positional_params = params;
    }

    // ── Variable access ─────────────────────────────────────────────────

    /// Get the string value of a variable, if set.
    /// Walks scopes from top to bottom.
    pub fn get(&self, name: &str) -> Option<&str> {
        // Fast path: single scope (most common — outside function calls)
        if self.scopes.len() == 1 {
            return self.scopes[0].vars.get(name).map(|v| v.value.as_str());
        }
        for scope in self.scopes.iter().rev() {
            if let Some(var) = scope.vars.get(name) {
                return Some(var.value.as_str());
            }
        }
        None
    }

    /// Get the full Variable struct, if set.
    /// Walks scopes from top to bottom.
    #[allow(dead_code)]
    pub fn get_var(&self, name: &str) -> Option<&Variable> {
        for scope in self.scopes.iter().rev() {
            if let Some(var) = scope.vars.get(name) {
                return Some(var);
            }
        }
        None
    }

    /// Set a variable's value. Returns an error if the variable is readonly.
    ///
    /// If the variable already exists in some scope, it is updated in-place
    /// in that scope (POSIX: function assignments affect the caller).
    /// If the variable is new, it is created in the global scope.
    ///
    /// The exported-environ cache is only invalidated when the write can
    /// actually change `environ()`'s output: an update to an already-
    /// exported variable, or creation of a new variable that is exported
    /// (see `set_with_options` for the allexport case — plain `set` never
    /// exports a new variable, so cache-affecting new-var writes cannot
    /// happen here).
    pub fn set(&mut self, name: &str, value: impl Into<String>) -> Result<(), String> {
        let value = value.into();

        // Fast path: single scope (most common — outside function calls)
        if self.scopes.len() == 1 {
            if let Some(existing) = self.scopes[0].vars.get_mut(name) {
                if existing.readonly {
                    return Err(format!("{}: readonly variable", name));
                }
                if existing.exported {
                    self.environ_cache = None;
                }
                existing.value = value;
                existing.readonly = false;
            } else {
                self.scopes[0]
                    .vars
                    .insert(name.to_string(), Variable::new(value));
            }
            self.note_optind_write(name);
            return Ok(());
        }

        // Search for existing variable in any scope (top to bottom).
        for idx in (0..self.scopes.len()).rev() {
            if let Some(existing) = self.scopes[idx].vars.get_mut(name) {
                if existing.readonly {
                    return Err(format!("{}: readonly variable", name));
                }
                if existing.exported {
                    self.environ_cache = None;
                }
                existing.value = value;
                existing.readonly = false;
                self.note_optind_write(name);
                return Ok(());
            }
        }

        // Not found — create in global scope. A brand-new variable from
        // plain `set` is never exported, so it cannot change `environ()`.
        self.scopes[0]
            .vars
            .insert(name.to_string(), Variable::new(value));
        self.note_optind_write(name);
        Ok(())
    }

    /// Set a variable's value with allexport support.
    ///
    /// Cache invalidation mirrors `set`, plus the allexport-specific case:
    /// a write that newly exports a variable (existing var promoted to
    /// exported, or a brand-new var created exported under `set -a`) must
    /// invalidate the cache even though the plain-`set` fast path above
    /// would not have needed to.
    pub fn set_with_options(
        &mut self,
        name: &str,
        value: impl Into<String>,
        allexport: bool,
    ) -> Result<(), String> {
        let value = value.into();

        for idx in (0..self.scopes.len()).rev() {
            if let Some(existing) = self.scopes[idx].vars.get_mut(name) {
                if existing.readonly {
                    return Err(format!("{}: readonly variable", name));
                }
                // `exported` covers both cache-relevant cases: the var
                // was already exported (value change alters environ()),
                // or it becomes exported now via allexport.
                let exported = existing.exported || allexport;
                if exported {
                    self.environ_cache = None;
                }
                existing.value = value;
                existing.exported = exported;
                existing.readonly = false;
                self.note_optind_write(name);
                return Ok(());
            }
        }

        let mut var = Variable::new(value);
        if allexport {
            var.exported = true;
            self.environ_cache = None;
        }
        self.scopes[0].vars.insert(name.to_string(), var);
        self.note_optind_write(name);
        Ok(())
    }

    fn note_optind_write(&mut self, name: &str) {
        if name == "OPTIND" {
            let scope = self.scopes.last_mut().unwrap();
            scope.optind_write_generation = scope.optind_write_generation.wrapping_add(1);
        }
    }

    /// Unset a variable. Returns an error if the variable is readonly.
    /// Removes from whichever scope contains it. Only invalidates the
    /// environ cache when the removed variable was exported (it cannot
    /// have appeared in `environ()`'s output otherwise).
    pub fn unset(&mut self, name: &str) -> Result<(), String> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(existing) = scope.vars.get(name) {
                if existing.readonly {
                    return Err(format!("{}: readonly variable", name));
                }
                if existing.exported {
                    self.environ_cache = None;
                }
                scope.vars.remove(name);
                return Ok(());
            }
        }
        Ok(())
    }

    /// Mark a variable as exported. Walks scopes to find it; if not found,
    /// creates in global scope with empty value. Only invalidates the
    /// environ cache when the variable was not already exported (a no-op
    /// re-export cannot change `environ()`'s output).
    pub fn export(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(var) = scope.vars.get_mut(name) {
                if !var.exported {
                    self.environ_cache = None;
                    var.exported = true;
                }
                return;
            }
        }
        self.environ_cache = None;
        self.scopes[0]
            .vars
            .insert(name.to_string(), Variable::new_exported(""));
    }

    /// Mark a variable as readonly. Walks scopes to find it; if not found,
    /// creates in global scope with empty value. `readonly` never changes
    /// `exported`, so this cannot affect `environ()`'s output — except
    /// when it creates a brand-new (non-exported) variable, which also
    /// cannot appear in `environ()`. The cache is therefore never
    /// invalidated here.
    pub fn set_readonly(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(var) = scope.vars.get_mut(name) {
                var.readonly = true;
                return;
            }
        }
        let mut var = Variable::new("");
        var.readonly = true;
        self.scopes[0].vars.insert(name.to_string(), var);
    }

    /// Return true if `name` resolves to a readonly variable in any
    /// scope (rev-walk matches the resolution order of `set` / `unset`).
    /// Allows callers (e.g. `getopts`) to dry-run an assignment without
    /// mutating state.
    pub fn is_readonly(&self, name: &str) -> bool {
        for scope in self.scopes.iter().rev() {
            if let Some(var) = scope.vars.get(name) {
                return var.readonly;
            }
        }
        false
    }

    /// Return only exported variables as (name, value) pairs.
    /// Later scopes shadow earlier ones. Result is cached until next mutation.
    pub fn environ(&mut self) -> &[(String, String)] {
        if self.environ_cache.is_none() {
            self.environ_cache = Some(self.build_environ());
        }
        self.environ_cache.as_ref().unwrap()
    }

    /// Build the exported-environ snapshot in a single pass: walk scopes
    /// top-down (current scope first) and track already-seen names in a
    /// `HashSet<&str>` so each name is resolved to its shadowing (topmost)
    /// scope without allocating an intermediate `HashMap<String, &Variable>`
    /// covering every variable (exported or not). Only exported entries
    /// are cloned into the result.
    fn build_environ(&self) -> Vec<(String, String)> {
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut result = Vec::new();
        for scope in self.scopes.iter().rev() {
            for (name, var) in &scope.vars {
                if !seen.insert(name.as_str()) {
                    continue;
                }
                if var.exported {
                    result.push((name.clone(), var.value.clone()));
                }
            }
        }
        result
    }

    /// Iterate over all variables as (name, &Variable) pairs.
    /// Later scopes shadow earlier ones (lazy, no intermediate allocation).
    pub fn vars_iter(&self) -> impl Iterator<Item = (&str, &Variable)> {
        let mut seen = std::collections::HashSet::new();
        self.scopes
            .iter()
            .rev()
            .flat_map(|s| s.vars.iter())
            .filter_map(move |(k, v)| {
                if seen.insert(k.as_str()) {
                    Some((k.as_str(), v))
                } else {
                    None
                }
            })
    }
}

impl Default for VarStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_set() {
        let mut store = VarStore::new();
        assert_eq!(store.get("FOO"), None);
        store.set("FOO", "bar").unwrap();
        assert_eq!(store.get("FOO"), Some("bar"));
    }

    #[test]
    fn test_unset() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        assert_eq!(store.get("FOO"), Some("bar"));
        store.unset("FOO").unwrap();
        assert_eq!(store.get("FOO"), None);
    }

    #[test]
    fn test_readonly_prevents_set() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        store.set_readonly("FOO");
        let result = store.set("FOO", "baz");
        assert!(result.is_err());
        assert_eq!(store.get("FOO"), Some("bar"));
    }

    #[test]
    fn test_readonly_prevents_unset() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        store.set_readonly("FOO");
        let result = store.unset("FOO");
        assert!(result.is_err());
        assert_eq!(store.get("FOO"), Some("bar"));
    }

    #[test]
    fn is_readonly_reports_readonly_state_without_mutation() {
        let mut store = VarStore::new();
        assert!(!store.is_readonly("FOO"));
        store.set("FOO", "bar").unwrap();
        assert!(!store.is_readonly("FOO"));
        store.set_readonly("FOO");
        assert!(store.is_readonly("FOO"));
        assert_eq!(store.get("FOO"), Some("bar"));
    }

    #[test]
    fn test_export() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        assert!(!store.get_var("FOO").unwrap().exported);
        store.export("FOO");
        assert!(store.get_var("FOO").unwrap().exported);
    }

    #[test]
    fn test_environ_excludes_unexported() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        store.set("BAZ", "qux").unwrap();
        store.export("FOO");
        let env = store.environ();
        assert_eq!(env.len(), 1);
        assert_eq!(env[0], ("FOO".to_string(), "bar".to_string()));
    }

    #[test]
    fn test_from_environ() {
        let store = VarStore::from_environ();
        if let Some(var) = store.get_var("PATH") {
            assert!(var.exported, "Variables from environ should be exported");
        }
    }

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

    // ── environ_cache invalidation gating (TODO PERF item 2) ────────────

    #[test]
    fn set_on_non_exported_var_does_not_invalidate_cache() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        let _ = store.environ(); // populate cache
        assert!(store.environ_cache.is_some());

        store.set("FOO", "baz").unwrap();
        assert!(
            store.environ_cache.is_some(),
            "writing a non-exported var must not clear the environ cache"
        );
        assert_eq!(store.get("FOO"), Some("baz"));
    }

    #[test]
    fn set_on_exported_var_invalidates_cache() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        store.export("FOO");
        let _ = store.environ(); // populate cache
        assert!(store.environ_cache.is_some());

        store.set("FOO", "baz").unwrap();
        assert!(
            store.environ_cache.is_none(),
            "writing an exported var must clear the environ cache"
        );
        assert_eq!(store.environ(), &[("FOO".to_string(), "baz".to_string())]);
    }

    #[test]
    fn set_creating_new_non_exported_var_does_not_invalidate_cache() {
        let mut store = VarStore::new();
        store.set("EXPORTED", "1").unwrap();
        store.export("EXPORTED");
        let _ = store.environ();
        assert!(store.environ_cache.is_some());

        // Creating a brand-new, non-exported variable must not disturb the
        // cached exported-environ snapshot.
        store.set("NEWVAR", "value").unwrap();
        assert!(store.environ_cache.is_some());
        assert_eq!(store.get("NEWVAR"), Some("value"));
    }

    #[test]
    fn set_with_options_allexport_invalidates_cache_for_new_var() {
        let mut store = VarStore::new();
        store.set("EXPORTED", "1").unwrap();
        store.export("EXPORTED");
        let _ = store.environ();
        assert!(store.environ_cache.is_some());

        // Under `set -a` (allexport), a newly created variable becomes
        // exported, so the cache must be invalidated.
        store.set_with_options("NEWVAR", "value", true).unwrap();
        assert!(
            store.environ_cache.is_none(),
            "allexport-created var must invalidate the cache"
        );
        let env = store.environ();
        assert!(env.contains(&("NEWVAR".to_string(), "value".to_string())));
    }

    #[test]
    fn set_with_options_non_allexport_on_non_exported_var_does_not_invalidate_cache() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        let _ = store.environ();
        assert!(store.environ_cache.is_some());

        store.set_with_options("FOO", "baz", false).unwrap();
        assert!(
            store.environ_cache.is_some(),
            "non-allexport write to a non-exported var must not invalidate the cache"
        );
        assert_eq!(store.get("FOO"), Some("baz"));
    }

    #[test]
    fn unset_non_exported_var_does_not_invalidate_cache() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        let _ = store.environ();
        assert!(store.environ_cache.is_some());

        store.unset("FOO").unwrap();
        assert!(
            store.environ_cache.is_some(),
            "unsetting a non-exported var need not invalidate the cache"
        );
        assert_eq!(store.get("FOO"), None);
    }

    #[test]
    fn unset_exported_var_invalidates_cache() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        store.export("FOO");
        let _ = store.environ();
        assert!(store.environ_cache.is_some());

        store.unset("FOO").unwrap();
        assert!(
            store.environ_cache.is_none(),
            "unsetting an exported var must invalidate the cache"
        );
        assert!(store.environ().is_empty());
    }

    #[test]
    fn export_invalidates_cache() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        let _ = store.environ();
        assert!(store.environ_cache.is_some());

        store.export("FOO");
        assert!(store.environ_cache.is_none());
        assert_eq!(store.environ(), &[("FOO".to_string(), "bar".to_string())]);
    }

    // ── in-place update preserves attributes (TODO PERF item 3) ─────────

    #[test]
    fn set_in_place_preserves_exported_flag() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        store.export("FOO");
        store.set("FOO", "baz").unwrap();
        assert!(store.get_var("FOO").unwrap().exported);
        assert_eq!(store.get("FOO"), Some("baz"));
    }

    #[test]
    fn set_in_place_resets_readonly_to_false_when_not_readonly() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        store.set("FOO", "baz").unwrap();
        assert!(!store.get_var("FOO").unwrap().readonly);
    }

    #[test]
    fn set_in_place_multiscope_preserves_exported_flag() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        store.export("FOO");

        store.push_scope(vec![]);
        store.set("FOO", "updated").unwrap();
        assert!(store.get_var("FOO").unwrap().exported);
        assert_eq!(store.get("FOO"), Some("updated"));
        store.pop_scope();
    }

    #[test]
    fn set_with_options_in_place_preserves_or_adds_exported() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        // allexport=true on an existing non-exported var must export it.
        store.set_with_options("FOO", "baz", true).unwrap();
        assert!(store.get_var("FOO").unwrap().exported);
        assert_eq!(store.get("FOO"), Some("baz"));
    }

    // ── build_environ scope shadowing (TODO PERF item 4) ─────────────────

    #[test]
    fn build_environ_later_scope_shadows_earlier() {
        let mut store = VarStore::new();
        store.set("FOO", "global").unwrap();
        store.export("FOO");

        store.push_scope(vec![]);
        // New scope shadows FOO with its own exported value.
        store.set_with_options("FOO", "local", true).unwrap();
        let env = store.environ();
        assert_eq!(env, &[("FOO".to_string(), "local".to_string())]);
        store.pop_scope();

        // After popping, the global FOO value (mutated in place by
        // `set_with_options`, since FOO already existed there... but here
        // FOO only exists in global at this point since the function-scope
        // write updates whichever scope currently holds the var. Since FOO
        // pre-existed in global before push_scope, the write landed in the
        // global scope too (POSIX: existing var updates affect caller).
        assert_eq!(store.get("FOO"), Some("local"));
    }

    #[test]
    fn build_environ_excludes_unexported_across_scopes() {
        let mut store = VarStore::new();
        store.set("GLOBAL_EXPORTED", "g").unwrap();
        store.export("GLOBAL_EXPORTED");
        store.set("GLOBAL_PLAIN", "p").unwrap();

        store.push_scope(vec![]);
        store.set("NEW_IN_SCOPE", "new").unwrap();
        // NEW_IN_SCOPE is new (didn't exist in any scope), so it lands in
        // the global scope per existing `set` semantics — not exported.

        let env = store.environ();
        assert_eq!(env.len(), 1);
        assert_eq!(env[0], ("GLOBAL_EXPORTED".to_string(), "g".to_string()));
        store.pop_scope();
    }
}
