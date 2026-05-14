//! POSIX `type` builtin.
//!
//! `type name...` — for each name, write to stdout how it would be
//! interpreted if used as a command name. Recognizes aliases,
//! reserved words, functions, special/regular builtins, and external
//! commands resolvable via PATH.
//!
//! Output formats match bash/dash conventions:
//! - `<name> is aliased to '<value>'`
//! - `<name> is a shell keyword`
//! - `<name> is a function`
//! - `<name> is a special shell builtin`
//! - `<name> is a shell builtin`
//! - `<name> is <path>`

use crate::builtin::BuiltinKind;
use crate::builtin::resolve::{CommandKind, resolve_command_kind};
use crate::env::ShellEnv;
use crate::error::ShellError;

/// Render the `type` line for a single name.
///
/// Returns `(stdout_line, optional_stderr_line, per_operand_exit)`.
/// `stderr_line` is `Some` only when the name is not found, and
/// `per_operand_exit` is 1 in that case (0 otherwise).
pub(crate) fn format_type_line(env: &mut ShellEnv, name: &str) -> (String, Option<String>, i32) {
    match resolve_command_kind(env, name) {
        CommandKind::Alias(val) => {
            let escaped = val.replace('\'', r"'\''");
            (format!("{} is aliased to '{}'", name, escaped), None, 0)
        }
        CommandKind::Keyword => (format!("{} is a shell keyword", name), None, 0),
        CommandKind::Function => (format!("{} is a function", name), None, 0),
        CommandKind::Builtin(BuiltinKind::Special) => {
            (format!("{} is a special shell builtin", name), None, 0)
        }
        CommandKind::Builtin(BuiltinKind::Regular) => {
            (format!("{} is a shell builtin", name), None, 0)
        }
        CommandKind::Builtin(BuiltinKind::NotBuiltin) => {
            // Cannot happen — resolve_command_kind never returns this.
            (
                String::new(),
                Some(format!("yosh: type: {}: not found", name)),
                1,
            )
        }
        CommandKind::External(p) => (format!("{} is {}", name, p.to_string_lossy()), None, 0),
        CommandKind::NotFound => (
            String::new(),
            Some(format!("yosh: type: {}: not found", name)),
            1,
        ),
    }
}

/// Execute `type` with the given arguments.
pub fn builtin_type(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    if args.is_empty() {
        eprintln!("yosh: type: usage: type name...");
        return Ok(2);
    }

    let mut exit_status = 0;
    for name in args {
        let (stdout_line, stderr_line, per_exit) = format_type_line(env, name);
        if !stdout_line.is_empty() {
            println!("{}", stdout_line);
        }
        if let Some(s) = stderr_line {
            eprintln!("{}", s);
        }
        if per_exit != 0 {
            exit_status = per_exit;
        }
    }
    Ok(exit_status)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_with_path(path: &str) -> ShellEnv {
        let mut env = ShellEnv::new("yosh", vec![]);
        let _ = env.vars.set("PATH", path);
        env
    }

    #[test]
    fn alias_line() {
        let mut env = env_with_path("/bin:/usr/bin");
        env.aliases.set("ll", "ls -l");
        let (out, err, ex) = format_type_line(&mut env, "ll");
        assert_eq!(out, "ll is aliased to 'ls -l'");
        assert!(err.is_none());
        assert_eq!(ex, 0);
    }

    #[test]
    fn alias_single_quote_escaping() {
        let mut env = env_with_path("/bin:/usr/bin");
        env.aliases.set("q", "echo 'hi'");
        let (out, _, _) = format_type_line(&mut env, "q");
        assert_eq!(out, r"q is aliased to 'echo '\''hi'\'''");
    }

    #[test]
    fn keyword_line() {
        let mut env = env_with_path("/bin:/usr/bin");
        let (out, _, ex) = format_type_line(&mut env, "if");
        assert_eq!(out, "if is a shell keyword");
        assert_eq!(ex, 0);
    }

    #[test]
    fn function_line() {
        use crate::parser::ast::{CompoundCommand, CompoundCommandKind, FunctionDef};
        use std::rc::Rc;
        let mut env = env_with_path("/bin:/usr/bin");
        env.functions.insert(
            "myfn".to_string(),
            FunctionDef {
                name: "myfn".to_string(),
                body: Rc::new(CompoundCommand {
                    kind: CompoundCommandKind::BraceGroup { body: Vec::new() },
                    line: 0,
                }),
                redirects: Vec::new(),
            },
        );
        let (out, _, ex) = format_type_line(&mut env, "myfn");
        assert_eq!(out, "myfn is a function");
        assert_eq!(ex, 0);
    }

    #[test]
    fn special_builtin_line() {
        let mut env = env_with_path("/bin:/usr/bin");
        let (out, _, _) = format_type_line(&mut env, "export");
        assert_eq!(out, "export is a special shell builtin");
    }

    #[test]
    fn regular_builtin_line() {
        let mut env = env_with_path("/bin:/usr/bin");
        let (out, _, _) = format_type_line(&mut env, "cd");
        assert_eq!(out, "cd is a shell builtin");
    }

    #[test]
    fn external_line() {
        let mut env = env_with_path("/bin:/usr/bin");
        let (out, _, ex) = format_type_line(&mut env, "sh");
        assert!(out.starts_with("sh is "));
        assert!(out.contains("sh"));
        assert_eq!(ex, 0);
    }

    #[test]
    fn not_found_line() {
        let mut env = env_with_path("/bin:/usr/bin");
        let (out, err, ex) = format_type_line(&mut env, "definitely_no_such_cmd_12345");
        assert_eq!(out, "");
        assert_eq!(
            err.unwrap(),
            "yosh: type: definitely_no_such_cmd_12345: not found"
        );
        assert_eq!(ex, 1);
    }

    #[test]
    fn builtin_type_no_args_returns_usage_error() {
        let mut env = env_with_path("/bin:/usr/bin");
        let r = builtin_type(&[], &mut env).unwrap();
        assert_eq!(r, 2);
    }

    #[test]
    fn builtin_type_multi_operand_mixed_success_and_not_found() {
        let mut env = env_with_path("/bin:/usr/bin");
        let args = vec!["cd".to_string(), "definitely_no_such_cmd_xyz".to_string()];
        let r = builtin_type(&args, &mut env).unwrap();
        assert_eq!(r, 1);
    }

    #[test]
    fn builtin_type_all_found_returns_zero() {
        let mut env = env_with_path("/bin:/usr/bin");
        let args = vec!["cd".to_string(), "export".to_string()];
        let r = builtin_type(&args, &mut env).unwrap();
        assert_eq!(r, 0);
    }
}
