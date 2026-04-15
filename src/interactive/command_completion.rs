//! Command name completion for interactive tab-completion.
//!
//! Provides `CommandCompleter` which caches PATH executables and generates
//! command name candidates (executables + builtins + aliases).

use std::collections::HashSet;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::env::aliases::AliasStore;
use super::completion::longest_common_prefix;

/// Caches PATH executables and generates command name completion candidates.
pub struct CommandCompleter {
    /// Sorted list of executable names from PATH.
    cached_executables: Vec<String>,
    /// PATH value when cache was built (for invalidation).
    cached_path: String,
}

impl CommandCompleter {
    pub fn new() -> Self {
        Self {
            cached_executables: Vec::new(),
            cached_path: String::new(),
        }
    }

    /// Return command name candidates matching `prefix`.
    ///
    /// Collects from aliases, builtins, and PATH executables (cached).
    /// Results are deduplicated and sorted.
    pub fn complete(
        &mut self,
        prefix: &str,
        path: &str,
        builtins: &[&str],
        aliases: &AliasStore,
    ) -> Vec<String> {
        // Rebuild cache if PATH changed
        if self.cached_path != path {
            self.rebuild_cache(path);
        }

        let mut candidates = Vec::new();

        // Aliases
        for (name, _) in aliases.sorted_iter() {
            if name.starts_with(prefix) {
                candidates.push(name.to_string());
            }
        }

        // Builtins
        for &name in builtins {
            if name.starts_with(prefix) {
                candidates.push(name.to_string());
            }
        }

        // PATH executables (from cache)
        for name in &self.cached_executables {
            if name.starts_with(prefix) {
                candidates.push(name.clone());
            }
        }

        // Deduplicate and sort
        candidates.sort();
        candidates.dedup();
        candidates
    }

    /// Compute the longest common prefix of command candidates.
    pub fn complete_common_prefix(
        &mut self,
        prefix: &str,
        path: &str,
        builtins: &[&str],
        aliases: &AliasStore,
    ) -> (Vec<String>, String) {
        let candidates = self.complete(prefix, path, builtins, aliases);
        let common = longest_common_prefix(&candidates);
        (candidates, common)
    }

    fn rebuild_cache(&mut self, path: &str) {
        let mut seen = HashSet::new();
        let mut executables = Vec::new();

        for dir in path.split(':') {
            if dir.is_empty() {
                continue;
            }
            let entries = match fs::read_dir(dir) {
                Ok(rd) => rd,
                Err(_) => continue,
            };
            for entry in entries.filter_map(|e| e.ok()) {
                let name = match entry.file_name().into_string() {
                    Ok(n) => n,
                    Err(_) => continue,
                };
                // Skip hidden files and already-seen names
                if name.starts_with('.') || seen.contains(&name) {
                    continue;
                }
                // Check if file is executable
                if Self::is_executable(&entry) {
                    seen.insert(name.clone());
                    executables.push(name);
                }
            }
        }

        executables.sort();
        self.cached_executables = executables;
        self.cached_path = path.to_string();
    }

    #[cfg(unix)]
    fn is_executable(entry: &fs::DirEntry) -> bool {
        entry
            .file_type()
            .map(|ft| ft.is_file() || ft.is_symlink())
            .unwrap_or(false)
            && entry
                .metadata()
                .map(|m| m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
    }

    #[cfg(not(unix))]
    fn is_executable(entry: &fs::DirEntry) -> bool {
        entry
            .file_type()
            .map(|ft| ft.is_file())
            .unwrap_or(false)
    }
}

/// Context for command-name completion, passed alongside `CompletionContext`.
pub struct CommandCompletionContext<'a> {
    pub completer: &'a mut CommandCompleter,
    pub path: &'a str,
    pub builtins: &'a [&'static str],
    pub aliases: &'a AliasStore,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use tempfile::TempDir;

    fn make_executable(path: &std::path::Path) {
        File::create(path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    fn make_non_executable(path: &std::path::Path) {
        File::create(path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
        }
    }

    #[test]
    fn test_complete_prefix_match() {
        let tmp = TempDir::new().unwrap();
        make_executable(&tmp.path().join("git"));
        make_executable(&tmp.path().join("grep"));
        make_executable(&tmp.path().join("ls"));

        let mut completer = CommandCompleter::new();
        let aliases = AliasStore::default();
        let path = tmp.path().to_str().unwrap();
        let candidates = completer.complete("g", path, &[], &aliases);
        assert_eq!(candidates, vec!["git", "grep"]);
    }

    #[test]
    fn test_complete_includes_builtins() {
        let tmp = TempDir::new().unwrap();
        let mut completer = CommandCompleter::new();
        let aliases = AliasStore::default();
        let path = tmp.path().to_str().unwrap();
        let builtins = &["echo", "eval", "exec"][..];
        let candidates = completer.complete("e", path, builtins, &aliases);
        assert_eq!(candidates, vec!["echo", "eval", "exec"]);
    }

    #[test]
    fn test_complete_includes_aliases() {
        let tmp = TempDir::new().unwrap();
        let mut completer = CommandCompleter::new();
        let mut aliases = AliasStore::default();
        aliases.set("ll", "ls -l");
        aliases.set("la", "ls -a");
        let path = tmp.path().to_str().unwrap();
        let candidates = completer.complete("l", path, &[], &aliases);
        assert_eq!(candidates, vec!["la", "ll"]);
    }

    #[test]
    fn test_complete_deduplicates() {
        let tmp = TempDir::new().unwrap();
        make_executable(&tmp.path().join("echo"));

        let mut completer = CommandCompleter::new();
        let aliases = AliasStore::default();
        let path = tmp.path().to_str().unwrap();
        let builtins = &["echo"][..];
        let candidates = completer.complete("echo", path, builtins, &aliases);
        // "echo" appears in both builtins and PATH but should appear only once
        assert_eq!(candidates, vec!["echo"]);
    }

    #[test]
    fn test_complete_empty_prefix_returns_all() {
        let tmp = TempDir::new().unwrap();
        make_executable(&tmp.path().join("git"));
        make_executable(&tmp.path().join("ls"));

        let mut completer = CommandCompleter::new();
        let mut aliases = AliasStore::default();
        aliases.set("ll", "ls -l");
        let path = tmp.path().to_str().unwrap();
        let builtins = &["cd"][..];
        let candidates = completer.complete("", path, builtins, &aliases);
        assert!(candidates.contains(&"git".to_string()));
        assert!(candidates.contains(&"ls".to_string()));
        assert!(candidates.contains(&"ll".to_string()));
        assert!(candidates.contains(&"cd".to_string()));
    }

    #[test]
    fn test_skips_non_executable_files() {
        let tmp = TempDir::new().unwrap();
        make_executable(&tmp.path().join("runnable"));
        make_non_executable(&tmp.path().join("readme.txt"));

        let mut completer = CommandCompleter::new();
        let aliases = AliasStore::default();
        let path = tmp.path().to_str().unwrap();
        let candidates = completer.complete("r", path, &[], &aliases);
        assert_eq!(candidates, vec!["runnable"]);
    }

    #[test]
    fn test_cache_invalidation_on_path_change() {
        let tmp1 = TempDir::new().unwrap();
        let tmp2 = TempDir::new().unwrap();
        make_executable(&tmp1.path().join("alpha"));
        make_executable(&tmp2.path().join("beta"));

        let mut completer = CommandCompleter::new();
        let aliases = AliasStore::default();

        // First completion with tmp1
        let path1 = tmp1.path().to_str().unwrap();
        let c1 = completer.complete("", path1, &[], &aliases);
        assert!(c1.contains(&"alpha".to_string()));
        assert!(!c1.contains(&"beta".to_string()));

        // Change PATH to tmp2 — cache should rebuild
        let path2 = tmp2.path().to_str().unwrap();
        let c2 = completer.complete("", path2, &[], &aliases);
        assert!(!c2.contains(&"alpha".to_string()));
        assert!(c2.contains(&"beta".to_string()));
    }

    #[test]
    fn test_path_priority_first_dir_wins() {
        let tmp1 = TempDir::new().unwrap();
        let tmp2 = TempDir::new().unwrap();
        make_executable(&tmp1.path().join("mycmd"));
        make_executable(&tmp2.path().join("mycmd"));

        let mut completer = CommandCompleter::new();
        let aliases = AliasStore::default();
        let path = format!(
            "{}:{}",
            tmp1.path().to_str().unwrap(),
            tmp2.path().to_str().unwrap()
        );
        let candidates = completer.complete("mycmd", &path, &[], &aliases);
        // Should appear only once despite being in both dirs
        assert_eq!(candidates, vec!["mycmd"]);
    }

    #[test]
    fn test_complete_common_prefix() {
        let tmp = TempDir::new().unwrap();
        make_executable(&tmp.path().join("git"));
        make_executable(&tmp.path().join("grep"));

        let mut completer = CommandCompleter::new();
        let aliases = AliasStore::default();
        let path = tmp.path().to_str().unwrap();
        let (candidates, common) =
            completer.complete_common_prefix("g", path, &[], &aliases);
        assert_eq!(candidates, vec!["git", "grep"]);
        assert_eq!(common, "g");
    }
}
