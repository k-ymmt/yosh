use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::builtin::{BuiltinKind, classify_builtin};
use crate::env::aliases::AliasStore;

// ---------------------------------------------------------------------------
// CheckerEnv
// ---------------------------------------------------------------------------

/// Lightweight view of the shell environment needed by `CommandChecker`.
pub struct CheckerEnv<'a> {
    /// Value of the PATH variable (may be empty).
    pub path: &'a str,
    /// Alias store for the current shell session.
    pub aliases: &'a AliasStore,
}

// ---------------------------------------------------------------------------
// CommandExistence
// ---------------------------------------------------------------------------

/// Result of a command-existence check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandExistence {
    Valid,
    Invalid,
}

// ---------------------------------------------------------------------------
// CommandChecker
// ---------------------------------------------------------------------------

/// Checks whether a command name exists, with a simple PATH-search cache.
pub struct CommandChecker {
    /// Cache from command name to existence result (`true` = found).
    path_cache: HashMap<String, bool>,
    /// The PATH value used to populate `path_cache`.
    cached_path: String,
}

impl CommandChecker {
    /// Create a new checker with an empty cache.
    pub fn new() -> Self {
        Self {
            path_cache: HashMap::new(),
            cached_path: String::new(),
        }
    }

    /// Check whether `name` is a valid command in the context of `env`.
    pub fn check(&mut self, name: &str, env: &CheckerEnv) -> CommandExistence {
        // 1. Builtins (special or regular) are always valid.
        if classify_builtin(name) != BuiltinKind::NotBuiltin {
            return CommandExistence::Valid;
        }

        // 2. Aliases defined in the current session.
        if env.aliases.get(name).is_some() {
            return CommandExistence::Valid;
        }

        // 3. Name contains a slash — treat as a direct path.
        if name.contains('/') {
            return if is_executable(Path::new(name)) {
                CommandExistence::Valid
            } else {
                CommandExistence::Invalid
            };
        }

        // 4. PATH search, with cache invalidation when PATH changes.
        if env.path != self.cached_path {
            self.path_cache.clear();
            self.cached_path = env.path.to_string();
        }

        let found = self
            .path_cache
            .entry(name.to_string())
            .or_insert_with(|| search_path(name, env.path));

        if *found {
            CommandExistence::Valid
        } else {
            CommandExistence::Invalid
        }
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Search every directory in `path_var` (colon-separated) for `name`.
fn search_path(name: &str, path_var: &str) -> bool {
    for dir in path_var.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = Path::new(dir).join(name);
        if is_executable(&candidate) {
            return true;
        }
    }
    false
}

/// Returns `true` if `path` is a regular file with at least one execute bit set.
fn is_executable(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && (meta.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}
