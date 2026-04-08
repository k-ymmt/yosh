pub mod command;
pub mod redirect;

use std::ffi::CString;

use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{execvp, fork, ForkResult};

use crate::builtin::{exec_builtin, is_builtin};
use crate::env::ShellEnv;
use crate::expand::expand_words;
use crate::parser::ast::{Assignment, Command, SimpleCommand, Word};

use command::wait_child;
use redirect::RedirectState;

pub struct Executor {
    pub env: ShellEnv,
}

impl Executor {
    pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
        Executor {
            env: ShellEnv::new(shell_name, args),
        }
    }

    /// Dispatch a `Command` to the appropriate execution path.
    pub fn exec_command(&mut self, cmd: &Command) -> i32 {
        match cmd {
            Command::Simple(simple) => self.exec_simple_command(simple),
            Command::Compound(_, _) => {
                eprintln!("kish: compound commands not yet implemented");
                1
            }
            Command::FunctionDef(_) => {
                eprintln!("kish: function definitions not yet implemented");
                1
            }
        }
    }

    /// Execute a simple command (assignments, builtins, or external programs).
    pub fn exec_simple_command(&mut self, cmd: &SimpleCommand) -> i32 {
        // Expand all words
        let expanded = expand_words(&self.env, &cmd.words);

        // Assignment-only command (no words)
        if expanded.is_empty() {
            for assignment in &cmd.assignments {
                let value = assignment
                    .value
                    .as_ref()
                    .map(|w| crate::expand::expand_word_to_string(&self.env, w))
                    .unwrap_or_default();
                if let Err(e) = self.env.vars.set(&assignment.name, value) {
                    eprintln!("kish: {}", e);
                    self.env.last_exit_status = 1;
                    return 1;
                }
            }
            self.env.last_exit_status = 0;
            return 0;
        }

        let command_name = &expanded[0];
        let args = &expanded[1..];

        if is_builtin(command_name) {
            // For builtins: apply redirects with save=true, run, restore
            let mut redirect_state = RedirectState::new();
            if let Err(e) = redirect_state.apply(&cmd.redirects, &self.env, true) {
                eprintln!("kish: {}", e);
                self.env.last_exit_status = 1;
                return 1;
            }
            let status = exec_builtin(command_name, args, &mut self.env);
            redirect_state.restore();
            self.env.last_exit_status = status;
            status
        } else {
            // External command: fork, child applies redirects + env + exec, parent waits
            let env_vars = self.build_env_vars(&cmd.assignments);
            let status = self.exec_external_with_redirects(
                command_name,
                args,
                &env_vars,
                &cmd.redirects,
            );
            self.env.last_exit_status = status;
            status
        }
    }

    /// Merge exported shell variables with command-specific assignments.
    pub fn build_env_vars(&self, assignments: &[Assignment]) -> Vec<(String, String)> {
        let mut vars = self.env.vars.to_environ();
        for assign in assignments {
            let value = assign
                .value
                .as_ref()
                .map(|w| crate::expand::expand_word_to_string(&self.env, w))
                .unwrap_or_default();
            // Replace existing or push new
            if let Some(entry) = vars.iter_mut().find(|(k, _)| k == &assign.name) {
                entry.1 = value;
            } else {
                vars.push((assign.name.clone(), value));
            }
        }
        vars
    }

    /// Fork, apply redirects in child, exec the command, wait in parent.
    pub fn exec_external_with_redirects(
        &self,
        cmd: &str,
        args: &[String],
        env_vars: &[(String, String)],
        redirects: &[crate::parser::ast::Redirect],
    ) -> i32 {
        // Build argv CStrings
        let c_cmd = match CString::new(cmd) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("kish: {}: invalid command name", cmd);
                return 127;
            }
        };
        let mut c_args: Vec<CString> = Vec::with_capacity(args.len() + 1);
        c_args.push(c_cmd.clone());
        for a in args {
            match CString::new(a.as_str()) {
                Ok(s) => c_args.push(s),
                Err(_) => {
                    eprintln!("kish: {}: invalid argument", a);
                    return 1;
                }
            }
        }

        match unsafe { fork() } {
            Err(e) => {
                eprintln!("kish: fork: {}", e);
                1
            }
            Ok(ForkResult::Child) => {
                // Apply redirects (no need to save, we're in the child)
                let mut redir_state = RedirectState::new();
                if let Err(e) = redir_state.apply(redirects, &self.env, false) {
                    eprintln!("kish: {}", e);
                    std::process::exit(1);
                }

                // Set environment variables
                for (k, v) in env_vars {
                    // SAFETY: single-threaded child after fork
                    unsafe { std::env::set_var(k, v) };
                }

                let err = execvp(&c_cmd, &c_args).unwrap_err();
                use nix::errno::Errno;
                let exit_code = match err {
                    Errno::ENOENT => {
                        eprintln!("kish: {}: command not found", cmd);
                        127
                    }
                    Errno::EACCES => {
                        eprintln!("kish: {}: permission denied", cmd);
                        126
                    }
                    _ => {
                        eprintln!("kish: {}: {}", cmd, err);
                        127
                    }
                };
                std::process::exit(exit_code);
            }
            Ok(ForkResult::Parent { child }) => wait_child(child),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{Redirect, SimpleCommand, Word};

    fn make_simple_cmd(words: &[&str]) -> SimpleCommand {
        SimpleCommand {
            assignments: vec![],
            words: words.iter().map(|s| Word::literal(s)).collect(),
            redirects: vec![],
        }
    }

    #[test]
    fn exec_builtin_true_returns_0() {
        let mut exec = Executor::new("kish", vec![]);
        let cmd = make_simple_cmd(&["true"]);
        assert_eq!(exec.exec_simple_command(&cmd), 0);
        assert_eq!(exec.env.last_exit_status, 0);
    }

    #[test]
    fn exec_builtin_false_returns_1() {
        let mut exec = Executor::new("kish", vec![]);
        let cmd = make_simple_cmd(&["false"]);
        assert_eq!(exec.exec_simple_command(&cmd), 1);
        assert_eq!(exec.env.last_exit_status, 1);
    }

    #[test]
    fn exec_external_true_returns_0() {
        let mut exec = Executor::new("kish", vec![]);
        let cmd = make_simple_cmd(&["/usr/bin/true"]);
        assert_eq!(exec.exec_simple_command(&cmd), 0);
    }

    #[test]
    fn assignment_only_sets_var() {
        use crate::parser::ast::Assignment;
        let mut exec = Executor::new("kish", vec![]);
        let cmd = SimpleCommand {
            assignments: vec![Assignment {
                name: "MYVAR".to_string(),
                value: Some(Word::literal("hello")),
            }],
            words: vec![],
            redirects: vec![],
        };
        let status = exec.exec_simple_command(&cmd);
        assert_eq!(status, 0);
        assert_eq!(exec.env.vars.get("MYVAR"), Some("hello"));
    }

    #[test]
    fn exit_status_tracked() {
        let mut exec = Executor::new("kish", vec![]);
        // false sets last_exit_status to 1
        let false_cmd = make_simple_cmd(&["false"]);
        exec.exec_simple_command(&false_cmd);
        assert_eq!(exec.env.last_exit_status, 1);

        // true resets it to 0
        let true_cmd = make_simple_cmd(&["true"]);
        exec.exec_simple_command(&true_cmd);
        assert_eq!(exec.env.last_exit_status, 0);
    }
}
