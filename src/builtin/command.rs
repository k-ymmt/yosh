//! POSIX `command` builtin.
//!
//! `command [-p] [-v|-V] command_name [argument...]`
//!
//! - `-p`  use the POSIX default PATH for lookup (from `confstr(_CS_PATH)`)
//! - `-v`  concise description of `command_name`
//! - `-V`  verbose description of `command_name`
//! - no flags: execute `command_name`, bypassing shell functions
//!
//! This file holds only the flag parser + description output paths. The
//! actual execution (for `-p` and no-flag forms) is dispatched from
//! `exec/simple.rs` so the `command` invocation has access to the
//! `Executor` for redirects/assignments.

use crate::builtin::BuiltinKind;
use crate::builtin::resolve::{CommandKind, resolve_command_kind};
use crate::env::ShellEnv;

/// Parsed form of a `command [...]` invocation.
#[derive(Debug, PartialEq, Eq)]
pub struct CommandFlags {
    pub use_default_path: bool,
    pub verbose: Verbosity,
    pub name: String,
    pub rest: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verbosity {
    /// No `-v` / `-V` flag: this is an execute invocation.
    Execute,
    /// `-v`: concise description.
    Brief,
    /// `-V`: verbose description.
    Verbose,
}

/// Parse the argument list for `command`. Returns `Err(message)` on invalid
/// flags or missing command name. Messages are already formatted for stderr
/// (e.g., `"command: -x: invalid option"`).
pub fn parse_flags(args: &[String]) -> Result<CommandFlags, String> {
    let mut use_default_path = false;
    let mut verbose = Verbosity::Execute;
    let mut idx = 0;

    while idx < args.len() {
        let a = &args[idx];
        if a == "--" {
            idx += 1;
            break;
        }
        if !a.starts_with('-') || a == "-" {
            break;
        }
        // Parse clustered flags: "-pv" = -p -v, "-Vp" = -V -p.
        for ch in a[1..].chars() {
            match ch {
                'p' => use_default_path = true,
                'v' => verbose = Verbosity::Brief,
                'V' => verbose = Verbosity::Verbose,
                other => return Err(format!("command: -{}: invalid option", other)),
            }
        }
        idx += 1;
    }

    if idx >= args.len() {
        return Err("command: missing command name".to_string());
    }

    let name = args[idx].clone();
    let rest = args[idx + 1..].to_vec();
    Ok(CommandFlags {
        use_default_path,
        verbose,
        name,
        rest,
    })
}

/// Render `-v` concise output. Returns `(stdout, exit_status)`.
/// When `name` is unknown, stdout is empty and exit is 1.
pub fn render_brief(env: &ShellEnv, name: &str) -> (String, i32) {
    match resolve_command_kind(env, name) {
        CommandKind::Alias(val) => {
            let escaped = val.replace('\'', r"'\''");
            (format!("alias {}='{}'", name, escaped), 0)
        }
        CommandKind::Keyword => (name.to_string(), 0),
        CommandKind::Function => (name.to_string(), 0),
        CommandKind::Builtin(_) => (name.to_string(), 0),
        CommandKind::External(p) => (p.to_string_lossy().into_owned(), 0),
        CommandKind::NotFound => (String::new(), 1),
    }
}

/// Render `-V` verbose output. Returns `(stdout_or_empty, stderr_or_empty, exit_status)`.
/// For NotFound, stdout is empty and stderr holds the "not found" message.
pub fn render_verbose(env: &ShellEnv, name: &str) -> (String, String, i32) {
    match resolve_command_kind(env, name) {
        CommandKind::Alias(val) => (
            format!("{} is aliased to '{}'", name, val),
            String::new(),
            0,
        ),
        CommandKind::Keyword => (format!("{} is a shell keyword", name), String::new(), 0),
        CommandKind::Function => (format!("{} is a function", name), String::new(), 0),
        CommandKind::Builtin(BuiltinKind::Special) => (
            format!("{} is a special shell builtin", name),
            String::new(),
            0,
        ),
        CommandKind::Builtin(BuiltinKind::Regular) => {
            (format!("{} is a shell builtin", name), String::new(), 0)
        }
        CommandKind::Builtin(BuiltinKind::NotBuiltin) => {
            // Cannot happen — resolve_command_kind never returns this.
            (
                String::new(),
                format!("yosh: command: {}: not found", name),
                1,
            )
        }
        CommandKind::External(p) => (
            format!("{} is {}", name, p.to_string_lossy()),
            String::new(),
            0,
        ),
        CommandKind::NotFound => (
            String::new(),
            format!("yosh: command: {}: not found", name),
            1,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    fn env_with_path(path: &str) -> ShellEnv {
        let mut env = ShellEnv::new("yosh", vec![]);
        let _ = env.vars.set("PATH", path);
        env
    }

    #[test]
    fn no_flags_execute() {
        let p = parse_flags(&v(&["ls", "-l"])).unwrap();
        assert!(!p.use_default_path);
        assert_eq!(p.verbose, Verbosity::Execute);
        assert_eq!(p.name, "ls");
        assert_eq!(p.rest, v(&["-l"]));
    }

    #[test]
    fn p_flag() {
        let p = parse_flags(&v(&["-p", "ls"])).unwrap();
        assert!(p.use_default_path);
        assert_eq!(p.name, "ls");
    }

    #[test]
    fn v_flag() {
        let p = parse_flags(&v(&["-v", "ls"])).unwrap();
        assert_eq!(p.verbose, Verbosity::Brief);
    }

    #[test]
    fn big_v_flag() {
        let p = parse_flags(&v(&["-V", "ls"])).unwrap();
        assert_eq!(p.verbose, Verbosity::Verbose);
    }

    #[test]
    fn combined_flags() {
        let p = parse_flags(&v(&["-pv", "ls"])).unwrap();
        assert!(p.use_default_path);
        assert_eq!(p.verbose, Verbosity::Brief);
    }

    #[test]
    fn conflicting_v_flags_last_wins() {
        // POSIX does not forbid -vV / -Vv. Lock "last wins" in (matches
        // standard getopt-style behavior).
        let p = parse_flags(&v(&["-vV", "ls"])).unwrap();
        assert_eq!(p.verbose, Verbosity::Verbose);

        let p = parse_flags(&v(&["-Vv", "ls"])).unwrap();
        assert_eq!(p.verbose, Verbosity::Brief);
    }

    #[test]
    fn double_dash_stops_parsing() {
        let p = parse_flags(&v(&["--", "-v", "arg"])).unwrap();
        assert_eq!(p.verbose, Verbosity::Execute);
        assert_eq!(p.name, "-v");
        assert_eq!(p.rest, v(&["arg"]));
    }

    #[test]
    fn single_dash_is_a_name() {
        let p = parse_flags(&v(&["-"])).unwrap();
        assert_eq!(p.name, "-");
    }

    #[test]
    fn invalid_option_errors() {
        let err = parse_flags(&v(&["-x", "ls"])).unwrap_err();
        assert!(err.contains("-x"));
    }

    #[test]
    fn missing_name_errors() {
        let err = parse_flags(&v(&[])).unwrap_err();
        assert!(err.to_lowercase().contains("missing"));

        let err = parse_flags(&v(&["-v"])).unwrap_err();
        assert!(err.to_lowercase().contains("missing"));
    }

    #[test]
    fn brief_alias() {
        let mut env = env_with_path("/bin:/usr/bin");
        env.aliases.set("ll", "ls -l");
        let (out, code) = render_brief(&env, "ll");
        assert_eq!(out, "alias ll='ls -l'");
        assert_eq!(code, 0);
    }

    #[test]
    fn brief_alias_with_single_quote() {
        // An alias value containing ' should be escaped as '\'' so the output
        // round-trips through the shell's quoting rules (matches bash).
        let mut env = env_with_path("/bin:/usr/bin");
        env.aliases.set("weird", r"it's weird");
        let (out, code) = render_brief(&env, "weird");
        assert_eq!(out, r"alias weird='it'\''s weird'");
        assert_eq!(code, 0);
    }

    #[test]
    fn brief_keyword() {
        let env = env_with_path("/bin:/usr/bin");
        assert_eq!(render_brief(&env, "if"), ("if".to_string(), 0));
    }

    #[test]
    fn brief_builtin() {
        let env = env_with_path("/bin:/usr/bin");
        assert_eq!(render_brief(&env, "cd"), ("cd".to_string(), 0));
        assert_eq!(render_brief(&env, "export"), ("export".to_string(), 0));
    }

    #[test]
    fn brief_external() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, code) = render_brief(&env, "sh");
        assert!(
            out.ends_with("/sh"),
            "expected path ending in /sh, got: {out}"
        );
        assert_eq!(code, 0);
    }

    #[test]
    fn brief_not_found() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, code) = render_brief(&env, "definitely_not_a_real_cmd_xyz");
        assert_eq!(out, "");
        assert_eq!(code, 1);
    }

    #[test]
    fn verbose_alias() {
        let mut env = env_with_path("/bin:/usr/bin");
        env.aliases.set("ll", "ls -l");
        let (out, err, code) = render_verbose(&env, "ll");
        assert_eq!(out, "ll is aliased to 'ls -l'");
        assert_eq!(err, "");
        assert_eq!(code, 0);
    }

    #[test]
    fn verbose_keyword() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, _, code) = render_verbose(&env, "if");
        assert_eq!(out, "if is a shell keyword");
        assert_eq!(code, 0);
    }

    #[test]
    fn verbose_special_builtin() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, _, code) = render_verbose(&env, "export");
        assert_eq!(out, "export is a special shell builtin");
        assert_eq!(code, 0);
    }

    #[test]
    fn verbose_regular_builtin() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, _, code) = render_verbose(&env, "cd");
        assert_eq!(out, "cd is a shell builtin");
        assert_eq!(code, 0);
    }

    #[test]
    fn verbose_external() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, _, code) = render_verbose(&env, "sh");
        assert!(out.starts_with("sh is "), "got: {out}");
        assert!(out.contains("/sh"), "got: {out}");
        assert_eq!(code, 0);
    }

    #[test]
    fn verbose_not_found() {
        let env = env_with_path("/bin:/usr/bin");
        let (out, err, code) = render_verbose(&env, "definitely_not_a_real_cmd_xyz");
        assert_eq!(out, "");
        assert!(err.contains("not found"), "got stderr: {err}");
        assert_eq!(code, 1);
    }
}
