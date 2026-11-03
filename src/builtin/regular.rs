//! Dispatch of the POSIX regular builtins (see
//! [`super::classify_builtin`]). Each utility is implemented in its own
//! module; `wait`, `fg`, `bg`, `jobs`, `fc` and `command` need the
//! executor and are dispatched from `exec/simple/` instead.

use super::{alias, cd, echo, getopts, hash, kill, printf, read, test, r#type, ulimit, umask};
use crate::env::ShellEnv;

/// Execute a regular builtin command, returning its exit status.
pub fn exec_regular_builtin(name: &str, args: &[String], env: &mut ShellEnv) -> i32 {
    let result = match name {
        "cd" => cd::builtin_cd(args, env),
        "true" => Ok(0),
        "false" => Ok(1),
        "echo" => echo::builtin_echo(args),
        "printf" => printf::builtin_printf(args),
        "umask" => umask::builtin_umask(args),
        "ulimit" => ulimit::builtin_ulimit(args),
        "alias" => alias::builtin_alias(args, env),
        "unalias" => alias::builtin_unalias(args, env),
        "kill" => kill::builtin_kill(args, env),
        "wait" => {
            // Handled in Executor::exec_simple_command — should not reach here
            eprintln!("yosh: wait: internal error");
            Ok(1)
        }
        "fg" | "bg" | "jobs" | "fc" => {
            // Handled in Executor::exec_simple_command
            eprintln!("yosh: {}: internal error", name);
            Ok(1)
        }
        "command" => {
            // Dispatched from exec/simple/ — should not reach here.
            eprintln!("yosh: command: internal error");
            Ok(1)
        }
        "test" | "[" => Ok(test::builtin_test(name, args)),
        "type" => r#type::builtin_type(args, env),
        "hash" => hash::builtin_hash(args, env),
        "read" => read::builtin_read(args, env),
        "getopts" => getopts::builtin_getopts(args, env),
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

    fn make_env() -> ShellEnv {
        ShellEnv::new("yosh", vec![])
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
    fn test_echo_dash_n() {
        // -n flag should suppress trailing newline.
        // We can't easily capture stdout in unit tests, so verify
        // the function returns 0 (behavior tested via E2E).
        let args = vec!["-n".to_string(), "hello".to_string()];
        assert_eq!(echo::builtin_echo(&args).unwrap(), 0);
    }
}
