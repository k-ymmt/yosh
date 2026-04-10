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
}

impl VarStore {
    /// Create an empty VarStore with a single global scope.
    pub fn new() -> Self {
        VarStore {
            scopes: vec![Scope {
                vars: HashMap::new(),
                positional_params: Vec::new(),
            }],
        }
    }

    /// Initialize from the current process environment.
    pub fn from_environ() -> Self {
        let mut vars = HashMap::new();
        for (key, value) in std::env::vars() {
            vars.insert(key, Variable::new_exported(value));
        }
        VarStore {
            scopes: vec![Scope {
                vars,
                positional_params: Vec::new(),
            }],
        }
    }

    // ── Scope management ────────────────────────────────────────────────

    /// Push a new scope with the given positional parameters.
    /// Used for function calls.
    pub fn push_scope(&mut self, positional_params: Vec<String>) {
        self.scopes.push(Scope {
            vars: HashMap::new(),
            positional_params,
        });
    }

    /// Pop the current scope, restoring the previous scope's positional
    /// parameters. Panics if only the global scope remains.
    pub fn pop_scope(&mut self) {
        assert!(self.scopes.len() > 1, "cannot pop the global scope");
        self.scopes.pop();
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
    pub fn set(&mut self, name: &str, value: impl Into<String>) -> Result<(), String> {
        let value = value.into();

        // Search for existing variable in any scope (top to bottom).
        for scope in self.scopes.iter_mut().rev() {
            if let Some(existing) = scope.vars.get(name) {
                if existing.readonly {
                    return Err(format!("{}: readonly variable", name));
                }
                let exported = existing.exported;
                scope.vars.insert(
                    name.to_string(),
                    Variable {
                        value,
                        exported,
                        readonly: false,
                    },
                );
                return Ok(());
            }
        }

        // Not found — create in global scope.
        self.scopes[0].vars.insert(name.to_string(), Variable::new(value));
        Ok(())
    }

    /// Set a variable's value with allexport support.
    pub fn set_with_options(
        &mut self,
        name: &str,
        value: impl Into<String>,
        allexport: bool,
    ) -> Result<(), String> {
        let value = value.into();

        for scope in self.scopes.iter_mut().rev() {
            if let Some(existing) = scope.vars.get(name) {
                if existing.readonly {
                    return Err(format!("{}: readonly variable", name));
                }
                let exported = existing.exported || allexport;
                scope.vars.insert(
                    name.to_string(),
                    Variable {
                        value,
                        exported,
                        readonly: false,
                    },
                );
                return Ok(());
            }
        }

        let mut var = Variable::new(value);
        if allexport {
            var.exported = true;
        }
        self.scopes[0].vars.insert(name.to_string(), var);
        Ok(())
    }

    /// Unset a variable. Returns an error if the variable is readonly.
    /// Removes from whichever scope contains it.
    pub fn unset(&mut self, name: &str) -> Result<(), String> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(existing) = scope.vars.get(name) {
                if existing.readonly {
                    return Err(format!("{}: readonly variable", name));
                }
                scope.vars.remove(name);
                return Ok(());
            }
        }
        Ok(())
    }

    /// Mark a variable as exported. Walks scopes to find it; if not found,
    /// creates in global scope with empty value.
    pub fn export(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(var) = scope.vars.get_mut(name) {
                var.exported = true;
                return;
            }
        }
        self.scopes[0]
            .vars
            .insert(name.to_string(), Variable::new_exported(""));
    }

    /// Mark a variable as readonly. Walks scopes to find it; if not found,
    /// creates in global scope with empty value.
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

    /// Return only exported variables as (name, value) pairs.
    /// Later scopes shadow earlier ones.
    pub fn to_environ(&self) -> Vec<(String, String)> {
        let mut merged: HashMap<String, &Variable> = HashMap::new();
        for scope in &self.scopes {
            for (name, var) in &scope.vars {
                merged.insert(name.clone(), var);
            }
        }
        merged
            .into_iter()
            .filter(|(_, v)| v.exported)
            .map(|(k, v)| (k, v.value.clone()))
            .collect()
    }

    /// Iterate over all variables as (name, &Variable) pairs.
    /// Later scopes shadow earlier ones.
    pub fn vars_iter(&self) -> impl Iterator<Item = (&String, &Variable)> {
        let mut seen: HashMap<&String, &Variable> = HashMap::new();
        for scope in &self.scopes {
            for (name, var) in &scope.vars {
                seen.insert(name, var);
            }
        }
        seen.into_iter()
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
    fn test_export() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        assert!(!store.get_var("FOO").unwrap().exported);
        store.export("FOO");
        assert!(store.get_var("FOO").unwrap().exported);
    }

    #[test]
    fn test_to_environ_excludes_unexported() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        store.set("BAZ", "qux").unwrap();
        store.export("FOO");
        let env = store.to_environ();
        assert_eq!(env.len(), 1);
        assert_eq!(env[0], ("FOO".to_string(), "bar".to_string()));
    }

    #[test]
    fn test_from_environ() {
        let store = VarStore::from_environ();
        // All variables should be marked as exported
        for (_, var) in store.scopes[0].vars.iter() {
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
}
