pub mod aliases;
pub mod default_path;
pub mod exec_state;
pub mod jobs;
pub mod shell_mode;
pub mod traps;
pub mod vars;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use aliases::AliasStore;
pub use exec_state::{ExecState, FlowControl};
use jobs::JobTable;
use nix::unistd::{Pid, getpid};
pub use shell_mode::{ShellMode, ShellOptions};
pub use traps::{TrapAction, TrapStore};
use vars::VarStore;

use crate::interactive::history::History;
use crate::parser::ast::FunctionDef;

/// Process and job management state.
#[derive(Debug, Clone)]
pub struct ProcessState {
    pub shell_pid: Pid,
    pub shell_pgid: Pid,
    pub jobs: JobTable,
}

/// The complete shell environment.
#[derive(Debug, Clone)]
pub struct ShellEnv {
    pub vars: VarStore,
    pub exec: ExecState,
    pub process: ProcessState,
    pub mode: ShellMode,
    pub functions: HashMap<String, FunctionDef>,
    pub traps: TrapStore,
    pub aliases: AliasStore,
    pub history: History,
    pub shell_name: String,
    /// Cache of the POSIX default PATH (`confstr(_CS_PATH)`), computed
    /// lazily on first use. See `env::default_path::default_path()`.
    pub default_path_cache: OnceLock<String>,
    /// POSIX hash table: utility name → resolved absolute path.
    /// Auto-populated by `find_in_path` / `lookup_in_path` cache misses
    /// and by explicit `hash utility...` invocations. Cleared by
    /// `hash -r` and on `PATH` reassignment (POSIX §2.5.3).
    pub utility_hash: HashMap<String, PathBuf>,
}

impl ShellEnv {
    /// Create a new ShellEnv, initializing variables from the process environment.
    ///
    /// `shell_name` is $0 (argv[0]), `args` are the positional parameters ($1, $2, ...).
    pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
        let mut vars = VarStore::from_environ();
        vars.set_positional_params(args);
        ShellEnv {
            vars,
            exec: ExecState {
                last_exit_status: 0,
                flow_control: None,
                loop_depth: 0,
            },
            process: ProcessState {
                shell_pid: getpid(),
                shell_pgid: nix::unistd::getpgrp(),
                jobs: JobTable::default(),
            },
            mode: ShellMode {
                options: ShellOptions::default(),
                is_interactive: false,
                in_dot_script: false,
            },
            shell_name: shell_name.into(),
            functions: HashMap::new(),
            traps: TrapStore::default(),
            aliases: AliasStore::default(),
            history: History::new(),
            default_path_cache: OnceLock::new(),
            utility_hash: HashMap::new(),
        }
    }

    /// Set a shell variable. If `name == "PATH"`, the utility hash
    /// table is cleared after the successful assignment (POSIX
    /// §2.5.3). Returns `Err` only if the variable is readonly.
    pub fn assign_var(&mut self, name: &str, value: impl Into<String>) -> Result<(), String> {
        self.vars.set(name, value)?;
        if name == "PATH" {
            self.utility_hash.clear();
        }
        Ok(())
    }

    /// Unset a shell variable. If `name == "PATH"`, the utility hash
    /// table is cleared after the successful unset.
    pub fn unset_var(&mut self, name: &str) -> Result<(), String> {
        self.vars.unset(name)?;
        if name == "PATH" {
            self.utility_hash.clear();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_env_construction() {
        let env = ShellEnv::new("yosh", vec!["arg1".to_string(), "arg2".to_string()]);
        assert_eq!(env.shell_name, "yosh");
        assert_eq!(env.vars.positional_params(), &["arg1", "arg2"]);
        assert_eq!(env.exec.last_exit_status, 0);
        // PID should be a positive number
        assert!(env.process.shell_pid.as_raw() > 0);
    }

    #[test]
    fn test_jobs_table() {
        let env = ShellEnv::new("yosh", vec![]);
        assert!(env.process.jobs.is_empty());
    }

    #[test]
    fn test_shell_pgid() {
        let env = ShellEnv::new("yosh", vec![]);
        assert!(env.process.shell_pgid.as_raw() > 0);
    }

    #[test]
    fn assign_var_clears_utility_hash_on_path_change() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.utility_hash
            .insert("foo".to_string(), std::path::PathBuf::from("/bin/foo"));
        env.assign_var("PATH", "/new").unwrap();
        assert!(env.utility_hash.is_empty());
    }

    #[test]
    fn assign_var_leaves_utility_hash_for_non_path_var() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.utility_hash
            .insert("foo".to_string(), std::path::PathBuf::from("/bin/foo"));
        env.assign_var("OTHER", "x").unwrap();
        assert_eq!(env.utility_hash.len(), 1);
    }

    #[test]
    fn unset_var_clears_utility_hash_on_path_unset() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.assign_var("PATH", "/x").unwrap();
        env.utility_hash
            .insert("foo".to_string(), std::path::PathBuf::from("/bin/foo"));
        env.unset_var("PATH").unwrap();
        assert!(env.utility_hash.is_empty());
    }
}
