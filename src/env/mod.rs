pub mod aliases;
pub mod default_path;
pub mod exec_state;
pub mod jobs;
pub mod shell_mode;
pub mod traps;
pub mod vars;

use std::collections::HashMap;

use nix::unistd::{Pid, getpid};
use jobs::JobTable;
use aliases::AliasStore;
use vars::VarStore;
pub use exec_state::{ExecState, FlowControl};
pub use shell_mode::{ShellMode, ShellOptions};
pub use traps::{TrapAction, TrapStore};

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
        }
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
}
