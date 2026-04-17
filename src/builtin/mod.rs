pub mod command;
pub mod regular;
pub mod resolve;
pub mod special;

use crate::env::ShellEnv;

/// All builtin command names (special + regular) for tab-completion.
pub const BUILTIN_NAMES: &[&str] = &[
    // Special builtins
    "break", ":", "continue", ".", "eval", "exec", "exit", "export",
    "readonly", "return", "set", "shift", "times", "trap", "unset", "fc",
    // Regular builtins
    "cd", "command", "echo", "true", "false", "alias", "unalias", "kill",
    "wait", "fg", "bg", "jobs", "umask",
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
        "break" | ":" | "continue" | "." | "eval" | "exec" | "exit" | "export"
        | "readonly" | "return" | "set" | "shift" | "times" | "trap" | "unset"
        | "fc" => {
            BuiltinKind::Special
        }
        "cd" | "command" | "echo" | "true" | "false" | "alias" | "unalias"
        | "kill" | "wait" | "fg" | "bg" | "jobs" | "umask" => BuiltinKind::Regular,
        _ => BuiltinKind::NotBuiltin,
    }
}

/// Execute a regular builtin command, returning its exit status.
pub fn exec_regular_builtin(name: &str, args: &[String], env: &mut ShellEnv) -> i32 {
    let result = match name {
        "cd" => regular::builtin_cd(args, env),
        "true" => Ok(0),
        "false" => Ok(1),
        "echo" => regular::builtin_echo(args),
        "umask" => regular::builtin_umask(args),
        "alias" => regular::builtin_alias(args, env),
        "unalias" => regular::builtin_unalias(args, env),
        "kill" => regular::builtin_kill(args, env.process.shell_pgid),
        "wait" => {
            // Handled in Executor::exec_simple_command — should not reach here
            eprintln!("yosh: wait: internal error");
            Ok(1)
        }
        "fg" | "bg" | "jobs" => {
            // Handled in Executor::exec_simple_command
            eprintln!("yosh: {}: internal error", name);
            Ok(1)
        }
        _ => {
            eprintln!("yosh: {}: not a regular builtin", name);
            Ok(1)
        }
    };
    match result {
        Ok(status) => status,
        Err(e) => {
            eprintln!("{}", e);
            e.exit_code()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;

    fn make_env() -> ShellEnv {
        ShellEnv::new("yosh", vec![])
    }

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
        assert!(matches!(classify_builtin("ls"), BuiltinKind::NotBuiltin));
    }

    #[test]
    fn test_true_false() {
        let mut env = make_env();
        assert_eq!(exec_regular_builtin("true", &[], &mut env), 0);
        assert_eq!(exec_regular_builtin("false", &[], &mut env), 1);
    }

    #[test]
    fn test_alias_unalias() {
        let mut env = make_env();
        let args = vec!["ll=ls -l".to_string()];
        assert_eq!(exec_regular_builtin("alias", &args, &mut env), 0);
        assert_eq!(env.aliases.get("ll"), Some("ls -l"));
        let args = vec!["ll".to_string()];
        assert_eq!(exec_regular_builtin("unalias", &args, &mut env), 0);
        assert_eq!(env.aliases.get("ll"), None);
    }

    #[test]
    fn test_unalias_all() {
        let mut env = make_env();
        env.aliases.set("ll", "ls -l");
        env.aliases.set("la", "ls -a");
        let args = vec!["-a".to_string()];
        assert_eq!(exec_regular_builtin("unalias", &args, &mut env), 0);
        assert!(env.aliases.is_empty());
    }

    #[test]
    fn test_classify_fc() {
        assert!(matches!(classify_builtin("fc"), BuiltinKind::Special));
    }

    #[test]
    fn test_classify_fg_bg_jobs() {
        assert!(matches!(classify_builtin("fg"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("bg"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("jobs"), BuiltinKind::Regular));
    }

    #[test]
    fn test_echo_dash_n() {
        // -n flag should suppress trailing newline.
        // We can't easily capture stdout in unit tests, so verify
        // the function returns 0 (behavior tested via E2E).
        let args = vec!["-n".to_string(), "hello".to_string()];
        assert_eq!(regular::builtin_echo(&args).unwrap(), 0);
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
