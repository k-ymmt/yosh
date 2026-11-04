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
//! `exec/simple/` so the `command` invocation has access to the
//! `Executor` for redirects/assignments.

use crate::builtin::alias::alias_line;
use crate::builtin::options::{invalid_flag, scan_flag_words};
use crate::builtin::resolve::{CommandKind, describe_command, resolve_command_kind};
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
    // Clustered flags: "-pv" = -p -v, "-Vp" = -V -p.
    let idx = scan_flag_words(args, |ch| {
        match ch {
            'p' => use_default_path = true,
            'v' => verbose = Verbosity::Brief,
            'V' => verbose = Verbosity::Verbose,
            _ => return false,
        }
        true
    })
    .map_err(|e| invalid_flag("command", e.flag))?;

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
pub fn render_brief(env: &mut ShellEnv, name: &str) -> (String, i32) {
    match resolve_command_kind(env, name) {
        CommandKind::Alias(val) => (alias_line(name, &val), 0),
        CommandKind::Keyword => (name.to_string(), 0),
        CommandKind::Function => (name.to_string(), 0),
        CommandKind::Builtin(_) => (name.to_string(), 0),
        CommandKind::External(p) => (crate::builtin::path::encode_path(&p), 0),
        CommandKind::NotFound => (String::new(), 1),
    }
}

/// Render `-V` verbose output. Returns `(stdout_or_empty, stderr_or_empty, exit_status)`.
/// For NotFound, stdout is empty and stderr holds the "not found" message.
pub fn render_verbose(env: &mut ShellEnv, name: &str) -> (String, String, i32) {
    match describe_command(env, name) {
        Some(line) => (line, String::new(), 0),
        None => (
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
        let (out, code) = render_brief(&mut env, "ll");
        assert_eq!(out, "alias ll='ls -l'");
        assert_eq!(code, 0);
    }

    #[test]
    fn brief_alias_with_single_quote() {
        // An alias value containing ' should be escaped as '\'' so the output
        // round-trips through the shell's quoting rules (matches bash).
        let mut env = env_with_path("/bin:/usr/bin");
        env.aliases.set("weird", r"it's weird");
        let (out, code) = render_brief(&mut env, "weird");
        assert_eq!(out, r"alias weird='it'\''s weird'");
        assert_eq!(code, 0);
    }

    #[test]
    fn brief_keyword() {
        let mut env = env_with_path("/bin:/usr/bin");
        assert_eq!(render_brief(&mut env, "if"), ("if".to_string(), 0));
    }

    #[test]
    fn brief_builtin() {
        let mut env = env_with_path("/bin:/usr/bin");
        assert_eq!(render_brief(&mut env, "cd"), ("cd".to_string(), 0));
        assert_eq!(render_brief(&mut env, "export"), ("export".to_string(), 0));
    }

    #[test]
    fn brief_external() {
        let mut env = env_with_path("/bin:/usr/bin");
        let (out, code) = render_brief(&mut env, "sh");
        assert!(
            out.ends_with("/sh"),
            "expected path ending in /sh, got: {out}"
        );
        assert_eq!(code, 0);
    }

    #[test]
    fn brief_not_found() {
        let mut env = env_with_path("/bin:/usr/bin");
        let (out, code) = render_brief(&mut env, "definitely_not_a_real_cmd_xyz");
        assert_eq!(out, "");
        assert_eq!(code, 1);
    }

    #[test]
    fn verbose_alias() {
        let mut env = env_with_path("/bin:/usr/bin");
        env.aliases.set("ll", "ls -l");
        let (out, err, code) = render_verbose(&mut env, "ll");
        assert_eq!(out, "ll is aliased to 'ls -l'");
        assert_eq!(err, "");
        assert_eq!(code, 0);
    }

    #[test]
    fn verbose_alias_with_single_quote() {
        // -V must escape embedded ' the same way `type` does.
        let mut env = env_with_path("/bin:/usr/bin");
        env.aliases.set("q", "echo 'hi'");
        let (out, _, code) = render_verbose(&mut env, "q");
        assert_eq!(out, r"q is aliased to 'echo '\''hi'\'''");
        assert_eq!(code, 0);
    }

    #[test]
    fn verbose_keyword() {
        let mut env = env_with_path("/bin:/usr/bin");
        let (out, _, code) = render_verbose(&mut env, "if");
        assert_eq!(out, "if is a shell keyword");
        assert_eq!(code, 0);
    }

    #[test]
    fn verbose_special_builtin() {
        let mut env = env_with_path("/bin:/usr/bin");
        let (out, _, code) = render_verbose(&mut env, "export");
        assert_eq!(out, "export is a special shell builtin");
        assert_eq!(code, 0);
    }

    #[test]
    fn verbose_regular_builtin() {
        let mut env = env_with_path("/bin:/usr/bin");
        let (out, _, code) = render_verbose(&mut env, "cd");
        assert_eq!(out, "cd is a shell builtin");
        assert_eq!(code, 0);
    }

    #[test]
    fn verbose_function() {
        use crate::parser::ast::{CompoundCommand, CompoundCommandKind, FunctionDef};
        use std::rc::Rc;

        let mut env = env_with_path("/bin:/usr/bin");
        env.functions.insert(
            "myfunc".to_string(),
            Rc::new(FunctionDef {
                name: "myfunc".to_string(),
                body: Rc::new(CompoundCommand {
                    kind: CompoundCommandKind::BraceGroup { body: vec![] },
                    line: 1,
                    assignments: vec![],
                }),
                redirects: vec![],
            }),
        );
        let (out, err, code) = render_verbose(&mut env, "myfunc");
        assert_eq!(out, "myfunc is a function");
        assert_eq!(err, "");
        assert_eq!(code, 0);
    }

    #[test]
    fn verbose_external() {
        let mut env = env_with_path("/bin:/usr/bin");
        let (out, _, code) = render_verbose(&mut env, "sh");
        assert!(out.starts_with("sh is "), "got: {out}");
        assert!(out.contains("/sh"), "got: {out}");
        assert_eq!(code, 0);
    }

    #[test]
    fn verbose_not_found() {
        let mut env = env_with_path("/bin:/usr/bin");
        let (out, err, code) = render_verbose(&mut env, "definitely_not_a_real_cmd_xyz");
        assert_eq!(out, "");
        assert!(err.contains("not found"), "got stderr: {err}");
        assert_eq!(code, 1);
    }
}
