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

/// Storage for shell variables.
#[derive(Debug, Clone)]
pub struct VarStore {
    vars: HashMap<String, Variable>,
}

impl VarStore {
    /// Create an empty VarStore.
    pub fn new() -> Self {
        VarStore {
            vars: HashMap::new(),
        }
    }

    /// Initialize from the current process environment.
    pub fn from_environ() -> Self {
        let mut store = VarStore::new();
        for (key, value) in std::env::vars() {
            store.vars.insert(key, Variable::new_exported(value));
        }
        store
    }

    /// Get the string value of a variable, if set.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(|v| v.value.as_str())
    }

    /// Get the full Variable struct, if set.
    #[allow(dead_code)]
    pub fn get_var(&self, name: &str) -> Option<&Variable> {
        self.vars.get(name)
    }

    /// Set a variable's value. Returns an error if the variable is readonly.
    pub fn set(&mut self, name: &str, value: impl Into<String>) -> Result<(), String> {
        if let Some(existing) = self.vars.get(name) {
            if existing.readonly {
                return Err(format!("{}: readonly variable", name));
            }
            let exported = existing.exported;
            self.vars.insert(
                name.to_string(),
                Variable {
                    value: value.into(),
                    exported,
                    readonly: false,
                },
            );
        } else {
            self.vars.insert(name.to_string(), Variable::new(value));
        }
        Ok(())
    }

    /// Set a variable's value with allexport support. Returns an error if the variable is readonly.
    pub fn set_with_options(&mut self, name: &str, value: impl Into<String>, allexport: bool) -> Result<(), String> {
        if let Some(existing) = self.vars.get(name) {
            if existing.readonly {
                return Err(format!("{}: readonly variable", name));
            }
            let exported = existing.exported || allexport;
            self.vars.insert(
                name.to_string(),
                Variable {
                    value: value.into(),
                    exported,
                    readonly: false,
                },
            );
        } else {
            let mut var = Variable::new(value);
            if allexport {
                var.exported = true;
            }
            self.vars.insert(name.to_string(), var);
        }
        Ok(())
    }

    /// Unset a variable. Returns an error if the variable is readonly.
    pub fn unset(&mut self, name: &str) -> Result<(), String> {
        if let Some(existing) = self.vars.get(name)
            && existing.readonly
        {
            return Err(format!("{}: readonly variable", name));
        }
        self.vars.remove(name);
        Ok(())
    }

    /// Mark a variable as exported (create it with empty value if it doesn't exist).
    pub fn export(&mut self, name: &str) {
        let entry = self.vars.entry(name.to_string()).or_insert_with(|| Variable::new(""));
        entry.exported = true;
    }

    /// Mark a variable as readonly (create it with empty value if it doesn't exist).
    pub fn set_readonly(&mut self, name: &str) {
        let entry = self.vars.entry(name.to_string()).or_insert_with(|| Variable::new(""));
        entry.readonly = true;
    }

    /// Return only exported variables as (name, value) pairs.
    pub fn to_environ(&self) -> Vec<(String, String)> {
        self.vars
            .iter()
            .filter(|(_, v)| v.exported)
            .map(|(k, v)| (k.clone(), v.value.clone()))
            .collect()
    }

    /// Iterate over all variables as (&name, &Variable) pairs.
    pub fn vars_iter(&self) -> impl Iterator<Item = (&String, &Variable)> {
        self.vars.iter()
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
        // from_environ should include at least PATH or some env vars
        let store = VarStore::from_environ();
        // All variables should be marked as exported
        for (_, var) in store.vars.iter() {
            assert!(var.exported, "Variables from environ should be exported");
        }
    }
}
