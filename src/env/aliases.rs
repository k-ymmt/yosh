use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct AliasStore {
    aliases: HashMap<String, String>,
}

impl AliasStore {
    /// Define or update an alias.
    pub fn set(&mut self, name: &str, value: &str) {
        self.aliases.insert(name.to_string(), value.to_string());
    }

    /// Get the value of an alias, or None if not defined.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.aliases.get(name).map(|s| s.as_str())
    }

    /// Remove an alias. Returns true if the alias existed, false otherwise.
    pub fn remove(&mut self, name: &str) -> bool {
        self.aliases.remove(name).is_some()
    }

    /// Remove all aliases.
    pub fn clear(&mut self) {
        self.aliases.clear();
    }

    /// Return all aliases sorted by name.
    pub fn sorted_iter(&self) -> Vec<(&str, &str)> {
        let mut pairs: Vec<(&str, &str)> = self
            .aliases
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        pairs.sort_by_key(|(name, _)| *name);
        pairs
    }

    /// Returns true if no aliases are defined.
    pub fn is_empty(&self) -> bool {
        self.aliases.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alias_set_get() {
        let mut store = AliasStore::default();
        store.set("ll", "ls -l");
        assert_eq!(store.get("ll"), Some("ls -l"));
    }

    #[test]
    fn test_alias_remove() {
        let mut store = AliasStore::default();
        store.set("ll", "ls -l");
        assert!(store.remove("ll"));
        assert_eq!(store.get("ll"), None);
        assert!(!store.remove("ll"));
    }

    #[test]
    fn test_alias_clear() {
        let mut store = AliasStore::default();
        store.set("ll", "ls -l");
        store.set("la", "ls -a");
        store.clear();
        assert!(store.is_empty());
    }

    #[test]
    fn test_alias_sorted_iter() {
        let mut store = AliasStore::default();
        store.set("ll", "ls -l");
        store.set("aa", "echo a");
        let sorted: Vec<_> = store.sorted_iter();
        assert_eq!(sorted, vec![("aa", "echo a"), ("ll", "ls -l")]);
    }

    #[test]
    fn test_alias_overwrite() {
        let mut store = AliasStore::default();
        store.set("ll", "ls -l");
        store.set("ll", "ls -la");
        assert_eq!(store.get("ll"), Some("ls -la"));
    }
}
