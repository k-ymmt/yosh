pub mod vars;

use nix::unistd::{Pid, getpid};
use vars::VarStore;

/// The complete shell environment.
#[derive(Debug, Clone)]
pub struct ShellEnv {
    pub vars: VarStore,
    pub last_exit_status: i32,
    pub shell_pid: Pid,
    pub shell_name: String,
    pub positional_params: Vec<String>,
}

impl ShellEnv {
    /// Create a new ShellEnv, initializing variables from the process environment.
    ///
    /// `shell_name` is $0 (argv[0]), `args` are the positional parameters ($1, $2, ...).
    pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
        ShellEnv {
            vars: VarStore::from_environ(),
            last_exit_status: 0,
            shell_pid: getpid(),
            shell_name: shell_name.into(),
            positional_params: args,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_env_construction() {
        let env = ShellEnv::new("kish", vec!["arg1".to_string(), "arg2".to_string()]);
        assert_eq!(env.shell_name, "kish");
        assert_eq!(env.positional_params, vec!["arg1", "arg2"]);
        assert_eq!(env.last_exit_status, 0);
        // PID should be a positive number
        assert!(env.shell_pid.as_raw() > 0);
    }
}
