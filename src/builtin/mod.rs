pub mod alias;
pub mod cd;
pub mod command;
pub mod echo;
pub mod fc;
pub mod getopts;
pub mod hash;
pub mod kill;
pub(crate) mod output;
pub(crate) mod path;
pub mod printf;
pub(crate) mod quote;
pub mod read;
pub mod regular;
pub mod resolve;
pub mod special;
pub mod test;
pub mod r#type;
pub mod ulimit;
pub mod umask;

pub use regular::exec_regular_builtin;

/// All builtin command names (special + regular) for tab-completion.
pub const BUILTIN_NAMES: &[&str] = &[
    // Special builtins
    "break", ":", "continue", ".", "eval", "exec", "exit", "export", "readonly", "return", "set",
    "shift", "times", "trap", "unset", // Regular builtins
    "fc", "cd", "command", "echo", "true", "false", "alias", "unalias", "kill", "wait", "fg", "bg",
    "jobs", "umask", "ulimit", "test", "[", "type", "hash", "read", "getopts", "printf",
];

/// Classification of a command name as a POSIX builtin kind.
#[derive(Debug, PartialEq, Eq)]
pub enum BuiltinKind {
    /// POSIX special builtin: prefix assignments persist in current env,
    /// errors in assignments are fatal.
    Special,
    /// Regular builtin: prefix assignments are temporary.
    Regular,
    /// Not a builtin: execute as external command.
    NotBuiltin,
}

/// Classify a command name into its builtin kind.
pub fn classify_builtin(name: &str) -> BuiltinKind {
    match name {
        "break" | ":" | "continue" | "." | "eval" | "exec" | "exit" | "export" | "readonly"
        | "return" | "set" | "shift" | "times" | "trap" | "unset" => BuiltinKind::Special,
        "fc" | "cd" | "command" | "echo" | "true" | "false" | "alias" | "unalias" | "kill"
        | "wait" | "fg" | "bg" | "jobs" | "umask" | "ulimit" | "test" | "[" | "type" | "hash"
        | "read" | "getopts" | "printf" => BuiltinKind::Regular,
        _ => BuiltinKind::NotBuiltin,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_builtin() {
        assert!(matches!(classify_builtin(":"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("break"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("continue"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("return"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("exit"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("export"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("readonly"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("unset"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("set"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("eval"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("exec"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("trap"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("."), BuiltinKind::Special));
        assert!(matches!(classify_builtin("shift"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("times"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("cd"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("echo"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("true"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("false"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("alias"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("unalias"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("umask"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("read"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("getopts"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("ls"), BuiltinKind::NotBuiltin));
    }

    #[test]
    fn test_classify_fc() {
        // POSIX: fc is a regular built-in, not a special one.
        assert!(matches!(classify_builtin("fc"), BuiltinKind::Regular));
    }

    #[test]
    fn test_classify_ulimit() {
        assert!(matches!(classify_builtin("ulimit"), BuiltinKind::Regular));
    }

    #[test]
    fn test_classify_fg_bg_jobs() {
        assert!(matches!(classify_builtin("fg"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("bg"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("jobs"), BuiltinKind::Regular));
    }

    #[test]
    fn test_builtin_names_consistent_with_classify() {
        for &name in BUILTIN_NAMES {
            assert_ne!(
                classify_builtin(name),
                BuiltinKind::NotBuiltin,
                "{} is in BUILTIN_NAMES but classify_builtin returns NotBuiltin",
                name,
            );
        }
    }
}
