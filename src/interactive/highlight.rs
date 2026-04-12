use std::collections::HashMap;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crossterm::style::Color;

use crate::builtin::{BuiltinKind, classify_builtin};
use crate::env::aliases::AliasStore;

use super::terminal::Terminal;

// ---------------------------------------------------------------------------
// HighlightStyle
// ---------------------------------------------------------------------------

/// Visual style applied to a span of characters in the input line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightStyle {
    Default,
    Keyword,
    Operator,
    Redirect,
    String,
    DoubleString,
    Variable,
    CommandSub,
    ArithSub,
    Comment,
    CommandValid,
    CommandInvalid,
    IoNumber,
    Assignment,
    Tilde,
    Error,
}

// ---------------------------------------------------------------------------
// ColorSpan
// ---------------------------------------------------------------------------

/// A half-open byte range [start, end) with an associated style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorSpan {
    pub start: usize,
    pub end: usize,
    pub style: HighlightStyle,
}

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

// ---------------------------------------------------------------------------
// apply_style
// ---------------------------------------------------------------------------

/// Apply the terminal attributes associated with `style`.
pub fn apply_style<T: Terminal>(term: &mut T, style: HighlightStyle) -> io::Result<()> {
    match style {
        HighlightStyle::Default => {
            // No styling needed.
        }
        HighlightStyle::Keyword => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Magenta)?;
        }
        HighlightStyle::Operator | HighlightStyle::Redirect => {
            term.set_fg_color(Color::Cyan)?;
        }
        HighlightStyle::String | HighlightStyle::DoubleString => {
            term.set_fg_color(Color::Yellow)?;
        }
        HighlightStyle::Variable | HighlightStyle::Tilde => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Green)?;
        }
        HighlightStyle::CommandSub | HighlightStyle::ArithSub => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Yellow)?;
        }
        HighlightStyle::Comment => {
            term.set_dim(true)?;
        }
        HighlightStyle::CommandValid => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Green)?;
        }
        HighlightStyle::CommandInvalid => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Red)?;
        }
        HighlightStyle::IoNumber | HighlightStyle::Assignment => {
            term.set_fg_color(Color::Blue)?;
        }
        HighlightStyle::Error => {
            term.set_fg_color(Color::Red)?;
            term.set_underline(true)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_aliases() -> AliasStore {
        AliasStore::default()
    }

    // Helpers -----------------------------------------------------------------

    fn checker_env<'a>(path: &'a str, aliases: &'a AliasStore) -> CheckerEnv<'a> {
        CheckerEnv { path, aliases }
    }

    // Tests -------------------------------------------------------------------

    #[test]
    fn test_checker_builtin_special() {
        let mut checker = CommandChecker::new();
        let aliases = make_aliases();
        let env = checker_env("", &aliases);

        // Special builtins
        assert_eq!(checker.check("export", &env), CommandExistence::Valid);
        assert_eq!(checker.check("cd", &env), CommandExistence::Valid);
        // Regular builtins
        assert_eq!(checker.check("echo", &env), CommandExistence::Valid);
        assert_eq!(checker.check("true", &env), CommandExistence::Valid);
    }

    #[test]
    fn test_checker_alias() {
        let mut checker = CommandChecker::new();
        let mut aliases = make_aliases();
        aliases.set("ll", "ls -l");

        let env = checker_env("", &aliases);
        assert_eq!(checker.check("ll", &env), CommandExistence::Valid);
        assert_eq!(checker.check("zz", &env), CommandExistence::Invalid);
    }

    #[test]
    fn test_checker_path_search() {
        let mut checker = CommandChecker::new();
        let aliases = make_aliases();
        let path = "/usr/bin:/bin";
        let env = checker_env(path, &aliases);

        assert_eq!(checker.check("ls", &env), CommandExistence::Valid);
        assert_eq!(
            checker.check("xyzzy_nonexistent", &env),
            CommandExistence::Invalid
        );
    }

    #[test]
    fn test_checker_path_cache_invalidation() {
        let mut checker = CommandChecker::new();
        let aliases = make_aliases();

        // First check with real PATH — ls should be found.
        let env1 = checker_env("/usr/bin:/bin", &aliases);
        assert_eq!(checker.check("ls", &env1), CommandExistence::Valid);

        // Now check with empty PATH — cache must be invalidated.
        let env2 = checker_env("", &aliases);
        assert_eq!(checker.check("ls", &env2), CommandExistence::Invalid);
    }

    #[test]
    fn test_checker_direct_path() {
        let mut checker = CommandChecker::new();
        let aliases = make_aliases();
        let env = checker_env("", &aliases);

        assert_eq!(checker.check("/bin/sh", &env), CommandExistence::Valid);
        assert_eq!(
            checker.check("./nonexistent_script_xyz", &env),
            CommandExistence::Invalid
        );
    }

    #[test]
    fn test_checker_path_with_tempfile() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("create tempdir");
        let bin_path = dir.path().join("my_test_cmd");

        // Write a minimal shell script and make it executable.
        fs::write(&bin_path, "#!/bin/sh\n").expect("write temp executable");
        let mut perms = fs::metadata(&bin_path)
            .expect("metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&bin_path, perms).expect("set permissions");

        let mut checker = CommandChecker::new();
        let aliases = make_aliases();
        let path_val = dir.path().to_str().unwrap().to_string();
        let env = checker_env(&path_val, &aliases);

        assert_eq!(checker.check("my_test_cmd", &env), CommandExistence::Valid);
        assert_eq!(
            checker.check("nosuchthing", &env),
            CommandExistence::Invalid
        );
    }
}
