//! Shared "what is this name?" resolver used by the `command -v` / `-V`
//! builtin and (in the future) `type`.

use std::path::PathBuf;

use crate::builtin::{BuiltinKind, classify_builtin};
use crate::env::ShellEnv;
use crate::exec::command::find_in_path;
use crate::lexer::reserved::is_posix_reserved_word;

/// Classification of a command name against the current shell state.
#[derive(Debug, PartialEq, Eq)]
pub enum CommandKind {
    /// The name is an alias; payload is the alias value (the right-hand side).
    Alias(String),
    /// The name is a POSIX reserved word (e.g. `if`, `while`, `for`).
    Keyword,
    /// The name is a shell function defined in this session.
    Function,
    /// The name is a builtin command; payload distinguishes special vs regular.
    Builtin(BuiltinKind),
    /// The name resolves to an executable file on `PATH`.
    External(PathBuf),
    /// Nothing found.
    NotFound,
}

/// Escape an alias value for display inside single quotes: each embedded
/// `'` becomes `'\''` so the printed form can be pasted back into a shell.
/// Shared by `type` and `command -v` / `-V` output rendering.
pub(crate) fn escape_alias_value(val: &str) -> String {
    val.replace('\'', r"'\''")
}

/// Walk yosh's name-resolution order and report what `name` would bind to.
///
/// Order (matches bash `command -V` reporting order):
///   1. alias
///   2. reserved word (keyword)
///   3. function
///   4. builtin (Special or Regular)
///   5. PATH search
pub fn resolve_command_kind(env: &mut ShellEnv, name: &str) -> CommandKind {
    if let Some(val) = env.aliases.get(name) {
        return CommandKind::Alias(val.to_string());
    }
    if is_posix_reserved_word(name) {
        return CommandKind::Keyword;
    }
    if env.functions.contains_key(name) {
        return CommandKind::Function;
    }
    match classify_builtin(name) {
        BuiltinKind::NotBuiltin => {}
        kind => return CommandKind::Builtin(kind),
    }
    // External: search $PATH.
    // Release the env.vars borrow first so env.utility_hash can be mutably borrowed.
    if let Some(path_var) = env.vars.get("PATH").map(|s| s.to_string())
        && let Some(p) = find_in_path(name, &path_var, &mut env.utility_hash)
    {
        return CommandKind::External(p);
    }
    CommandKind::NotFound
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
    fn alias_wins_over_everything() {
        let mut env = env_with_path("/bin:/usr/bin");
        env.aliases.set("ls", "ls -G");
        // Even though "ls" also exists in PATH, alias takes precedence.
        assert_eq!(
            resolve_command_kind(&mut env, "ls"),
            CommandKind::Alias("ls -G".to_string())
        );
    }

    #[test]
    fn keyword_detected() {
        let mut env = env_with_path("/bin:/usr/bin");
        assert_eq!(resolve_command_kind(&mut env, "if"), CommandKind::Keyword);
        assert_eq!(resolve_command_kind(&mut env, "for"), CommandKind::Keyword);
        assert_eq!(resolve_command_kind(&mut env, "done"), CommandKind::Keyword);
    }

    #[test]
    fn function_wins_over_builtin() {
        // FunctionDef fields: { name: String, body: Rc<CompoundCommand>, redirects: Vec<Redirect> }
        // CompoundCommand is a struct wrapping CompoundCommandKind.
        // BraceGroup with an empty body is the minimal valid construction.
        use crate::parser::ast::{CompoundCommand, CompoundCommandKind, FunctionDef};
        use std::rc::Rc;
        let mut env = env_with_path("/bin:/usr/bin");
        env.functions.insert(
            "echo".to_string(),
            FunctionDef {
                name: "echo".to_string(),
                body: Rc::new(CompoundCommand {
                    kind: CompoundCommandKind::BraceGroup { body: Vec::new() },
                    line: 0,
                    assignments: Vec::new(),
                }),
                redirects: Vec::new(),
            },
        );
        assert_eq!(
            resolve_command_kind(&mut env, "echo"),
            CommandKind::Function
        );
    }

    #[test]
    fn special_builtin_detected() {
        let mut env = env_with_path("/bin:/usr/bin");
        assert_eq!(
            resolve_command_kind(&mut env, "export"),
            CommandKind::Builtin(BuiltinKind::Special)
        );
    }

    #[test]
    fn regular_builtin_detected() {
        let mut env = env_with_path("/bin:/usr/bin");
        assert_eq!(
            resolve_command_kind(&mut env, "cd"),
            CommandKind::Builtin(BuiltinKind::Regular)
        );
    }

    #[test]
    fn external_detected() {
        // /bin/sh is POSIX-mandatory on macOS + Linux.
        let mut env = env_with_path("/bin:/usr/bin");
        match resolve_command_kind(&mut env, "sh") {
            CommandKind::External(p) => {
                assert!(
                    p.ends_with("sh"),
                    "expected path ending in 'sh', got: {}",
                    p.display()
                );
            }
            other => panic!("expected External, got {:?}", other),
        }
    }

    #[test]
    fn not_found_for_unknown_name() {
        let mut env = env_with_path("/bin:/usr/bin");
        assert_eq!(
            resolve_command_kind(&mut env, "definitely_not_a_real_cmd_xyz"),
            CommandKind::NotFound
        );
    }
}
