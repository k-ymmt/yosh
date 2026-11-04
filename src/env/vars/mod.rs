//! Shell variable storage: the scope chain of name → `Variable` maps and
//! the reads and writes that resolve a name through it.
//!
//! Function-call scopes and the per-scope `getopts` / `OPTIND` state are
//! in `scope`, positional parameters in `positional`, and the cached
//! exported environment handed to child processes in `environ`.

mod environ;
mod positional;
mod scope;

use super::fxhash::FxHashMap;
use environ::ExportCache;

/// A shell variable with its value and attributes.
///
/// `value` is `None` for a name that carries attributes but no value:
/// `export y` / `readonly z` on an unset name (POSIX: the name is marked,
/// but stays unset — `${y-unset}` expands to `unset`, it is not passed
/// to children, and `export -p` lists it as `export y`).
#[derive(Debug, Clone, PartialEq)]
pub struct Variable {
    pub value: Option<String>,
    pub exported: bool,
    pub readonly: bool,
}

impl Variable {
    pub fn new(value: impl Into<String>) -> Self {
        Variable {
            value: Some(value.into()),
            exported: false,
            readonly: false,
        }
    }

    pub fn new_exported(value: impl Into<String>) -> Self {
        Variable {
            value: Some(value.into()),
            exported: true,
            readonly: false,
        }
    }

    /// The variable's value, or `None` when it is unset (attributes only).
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }
}

/// A single scope in the scope chain.
#[derive(Debug, Clone)]
struct Scope {
    vars: FxHashMap<String, Variable>,
    positional_params: Vec<String>,
    /// Number of leading `positional_params` entries consumed by `shift`.
    /// Keeping an offset makes `shift` O(1) instead of re-cloning the
    /// remaining parameters (a `while [ $# -gt 0 ]; do ...; shift; done`
    /// loop over 5000 parameters was O(n²) and 14× slower than dash).
    positional_offset: usize,
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

impl Scope {
    /// A scope holding `vars` and `positional_params`, with its `getopts`
    /// state reset and `saved_optind` to restore when it is popped.
    fn new(
        vars: FxHashMap<String, Variable>,
        positional_params: Vec<String>,
        saved_optind: Option<String>,
    ) -> Self {
        Scope {
            vars,
            positional_params,
            positional_offset: 0,
            getopts_subindex: 0,
            optind_write_generation: 0,
            getopts_observed_optind_generation: 0,
            saved_optind,
        }
    }
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
    export_cache: ExportCache,
}

/// The error a write to a readonly variable fails with.
fn readonly_error(name: &str) -> String {
    format!("{}: readonly variable", name)
}

/// The variable `name` resolves to, walking `scopes` from top to bottom.
fn find_var_mut<'a>(scopes: &'a mut [Scope], name: &str) -> Option<&'a mut Variable> {
    scopes
        .iter_mut()
        .rev()
        .find_map(|scope| scope.vars.get_mut(name))
}

impl VarStore {
    /// Create an empty VarStore with a single global scope.
    pub fn new() -> Self {
        Self::with_globals(FxHashMap::default())
    }

    /// Initialize from the current process environment.
    ///
    /// Uses `vars_os` + the byteenc escape encoding so names and values
    /// that are not valid UTF-8 are imported losslessly instead of being
    /// dropped, and re-exported byte-identically to children.
    pub fn from_environ() -> Self {
        use std::os::unix::ffi::OsStrExt;
        let mut vars = FxHashMap::default();
        for (key, value) in std::env::vars_os() {
            let key = crate::byteenc::encode_bytes(key.as_bytes()).into_owned();
            let value = crate::byteenc::encode_bytes(value.as_bytes()).into_owned();
            vars.insert(key, Variable::new_exported(value));
        }
        Self::with_globals(vars)
    }

    /// A store whose only (global) scope holds `vars`.
    fn with_globals(vars: FxHashMap<String, Variable>) -> Self {
        VarStore {
            scopes: vec![Scope::new(vars, Vec::new(), None)],
            export_cache: ExportCache::default(),
        }
    }

    // ── Variable access ─────────────────────────────────────────────────

    /// Get the string value of a variable, if set.
    /// Walks scopes from top to bottom.
    pub fn get(&self, name: &str) -> Option<&str> {
        // Fast path: single scope (most common — outside function calls)
        if self.scopes.len() == 1 {
            return self.scopes[0].vars.get(name).and_then(|v| v.value());
        }
        for scope in self.scopes.iter().rev() {
            if let Some(var) = scope.vars.get(name) {
                return var.value();
            }
        }
        None
    }

    /// Get the full Variable struct, if set.
    /// Walks scopes from top to bottom.
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
        self.assign(name, value.into(), false)
    }

    /// Set a variable's value with allexport support.
    ///
    /// Cache invalidation mirrors `set`, plus the allexport-specific case:
    /// a write that newly exports a variable (existing var promoted to
    /// exported, or a brand-new var created exported under `set -a`) must
    /// invalidate the cache even though plain `set` would not have
    /// needed to.
    pub fn set_with_options(
        &mut self,
        name: &str,
        value: impl Into<String>,
        allexport: bool,
    ) -> Result<(), String> {
        self.assign(name, value.into(), allexport)
    }

    /// The shared body of `set` / `set_with_options`.
    fn assign(&mut self, name: &str, value: String, allexport: bool) -> Result<(), String> {
        if let Some(existing) = find_var_mut(&mut self.scopes, name) {
            if existing.readonly {
                return Err(readonly_error(name));
            }
            // `exported` covers both cache-relevant cases: the var was
            // already exported (value change alters environ()), or it
            // becomes exported now via allexport.
            let exported = existing.exported || allexport;
            if exported {
                self.export_cache.invalidate();
            }
            existing.value = Some(value);
            existing.exported = exported;
            existing.readonly = false;
        } else {
            // Not found — create in global scope. A brand-new variable
            // changes `environ()` only when allexport exports it.
            let mut var = Variable::new(value);
            if allexport {
                var.exported = true;
                self.export_cache.invalidate();
            }
            self.scopes[0].vars.insert(name.to_string(), var);
        }
        self.note_optind_write(name);
        Ok(())
    }

    /// Unset a variable. Returns an error if the variable is readonly.
    /// Removes from whichever scope contains it. Only invalidates the
    /// environ cache when the removed variable was exported (it cannot
    /// have appeared in `environ()`'s output otherwise).
    pub fn unset(&mut self, name: &str) -> Result<(), String> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(existing) = scope.vars.get(name) {
                if existing.readonly {
                    return Err(readonly_error(name));
                }
                if existing.exported {
                    self.export_cache.invalidate();
                }
                scope.vars.remove(name);
                return Ok(());
            }
        }
        Ok(())
    }

    /// Put a variable back exactly as it was saved (`None`: it did not
    /// exist), attributes included — the undo of a temporary prefix
    /// assignment. Unlike an assignment this never exports through
    /// `set -a` and drops an export attribute the command added (dash
    /// agrees). Returns an error, changing nothing, if the variable is
    /// now readonly. Replaces the variable in the scope that holds it,
    /// else creates it in the global scope.
    pub fn restore(&mut self, name: &str, saved: Option<Variable>) -> Result<(), String> {
        let Some(saved) = saved else {
            return self.unset(name);
        };
        let idx = (0..self.scopes.len())
            .rev()
            .find(|&i| self.scopes[i].vars.contains_key(name))
            .unwrap_or(0);
        let current = self.scopes[idx].vars.get(name);
        if current.is_some_and(|v| v.readonly) {
            return Err(readonly_error(name));
        }
        if saved.exported || current.is_some_and(|v| v.exported) {
            self.export_cache.invalidate();
        }
        self.scopes[idx].vars.insert(name.to_string(), saved);
        self.note_optind_write(name);
        Ok(())
    }

    /// Mark a variable as exported. Walks scopes to find it; if not found,
    /// creates in global scope an exported name *without a value* (it
    /// stays unset until assigned, and is not passed to children until
    /// then). Only invalidates the environ cache when the variable was not
    /// already exported (a no-op re-export cannot change `environ()`'s
    /// output).
    pub fn export(&mut self, name: &str) {
        if let Some(var) = find_var_mut(&mut self.scopes, name) {
            if !var.exported {
                self.export_cache.invalidate();
                var.exported = true;
            }
            return;
        }
        // A valueless name never appears in `environ()`, so the cache
        // stays valid.
        self.scopes[0].vars.insert(
            name.to_string(),
            Variable {
                value: None,
                exported: true,
                readonly: false,
            },
        );
    }

    /// Mark a variable as readonly. Walks scopes to find it; if not found,
    /// creates in global scope a readonly name without a value (it stays
    /// unset, and can never be assigned). `readonly` never changes
    /// `exported`, so this cannot affect `environ()`'s output — except
    /// when it creates a brand-new (non-exported) variable, which also
    /// cannot appear in `environ()`. The cache is therefore never
    /// invalidated here.
    pub fn set_readonly(&mut self, name: &str) {
        if let Some(var) = find_var_mut(&mut self.scopes, name) {
            var.readonly = true;
            return;
        }
        self.scopes[0].vars.insert(
            name.to_string(),
            Variable {
                value: None,
                exported: false,
                readonly: true,
            },
        );
    }

    /// Return true if `name` resolves to a readonly variable in any
    /// scope (rev-walk matches the resolution order of `set` / `unset`).
    /// Allows callers (e.g. `getopts`) to dry-run an assignment without
    /// mutating state.
    pub fn is_readonly(&self, name: &str) -> bool {
        self.get_var(name).is_some_and(|var| var.readonly)
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
    fn test_from_environ() {
        let store = VarStore::from_environ();
        if let Some(var) = store.get_var("PATH") {
            assert!(var.exported, "Variables from environ should be exported");
        }
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
}
