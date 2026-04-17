//! Shared "what is this name?" resolver used by the `command -v` / `-V`
//! builtin and (in the future) `type`.

use std::path::PathBuf;

use crate::builtin::{classify_builtin, BuiltinKind};
use crate::env::ShellEnv;
use crate::exec::command::find_in_path;

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

/// POSIX reserved words per IEEE Std 1003.1-2017 §2.4.
const RESERVED_WORDS: &[&str] = &[
    "!", "{", "}", "case", "do", "done", "elif", "else", "esac", "fi",
    "for", "if", "in", "then", "until", "while",
];

fn is_reserved_word(name: &str) -> bool {
    RESERVED_WORDS.contains(&name)
}

/// Walk yosh's name-resolution order and report what `name` would bind to.
///
/// Order (matches bash `command -V` reporting order):
///   1. alias
///   2. reserved word (keyword)
///   3. function
///   4. builtin (Special or Regular)
///   5. PATH search
pub fn resolve_command_kind(env: &ShellEnv, name: &str) -> CommandKind {
    if let Some(val) = env.aliases.get(name) {
        return CommandKind::Alias(val.to_string());
    }
    if is_reserved_word(name) {
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
    if let Some(path_var) = env.vars.get("PATH") {
        if let Some(p) = find_in_path(name, path_var) {
            return CommandKind::External(p);
        }
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
            resolve_command_kind(&env, "ls"),
            CommandKind::Alias("ls -G".to_string())
        );
    }

    #[test]
    fn keyword_detected() {
        let env = env_with_path("/bin:/usr/bin");
        assert_eq!(resolve_command_kind(&env, "if"), CommandKind::Keyword);
        assert_eq!(resolve_command_kind(&env, "for"), CommandKind::Keyword);
        assert_eq!(resolve_command_kind(&env, "done"), CommandKind::Keyword);
    }

    #[test]
    fn function_wins_over_builtin() {
        // FunctionDef fields: { name: String, body: Rc<CompoundCommand>, redirects: Vec<Redirect> }
        // CompoundCommand is a struct wrapping CompoundCommandKind.
        // BraceGroup with an empty body is the minimal valid construction.
        use std::rc::Rc;
        use crate::parser::ast::{FunctionDef, CompoundCommand, CompoundCommandKind};
        let mut env = env_with_path("/bin:/usr/bin");
        env.functions.insert(
            "echo".to_string(),
            FunctionDef {
                name: "echo".to_string(),
                body: Rc::new(CompoundCommand {
                    kind: CompoundCommandKind::BraceGroup { body: Vec::new() },
                }),
                redirects: Vec::new(),
            },
        );
        assert_eq!(resolve_command_kind(&env, "echo"), CommandKind::Function);
    }

    #[test]
    fn special_builtin_detected() {
        let env = env_with_path("/bin:/usr/bin");
        assert_eq!(
            resolve_command_kind(&env, "export"),
            CommandKind::Builtin(BuiltinKind::Special)
        );
    }

    #[test]
    fn regular_builtin_detected() {
        let env = env_with_path("/bin:/usr/bin");
        assert_eq!(
            resolve_command_kind(&env, "cd"),
            CommandKind::Builtin(BuiltinKind::Regular)
        );
    }

    #[test]
    fn external_detected() {
        // /bin/sh is POSIX-mandatory on macOS + Linux.
        let env = env_with_path("/bin:/usr/bin");
        match resolve_command_kind(&env, "sh") {
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
        let env = env_with_path("/bin:/usr/bin");
        assert_eq!(
            resolve_command_kind(&env, "definitely_not_a_real_cmd_xyz"),
            CommandKind::NotFound
        );
    }
}
