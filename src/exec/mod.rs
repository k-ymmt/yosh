pub mod command;
pub mod pipeline;
pub mod redirect;

use std::ffi::CString;

use nix::unistd::{execvp, fork, ForkResult};

use crate::builtin::{exec_builtin, is_builtin};
use crate::env::ShellEnv;
use crate::expand::expand_words;
use crate::parser::ast::{
    AndOrList, AndOrOp, Assignment, Command, CompleteCommand, Program, SeparatorOp, SimpleCommand,
};

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

    /// Execute an AND-OR list.
    pub fn exec_and_or(&mut self, and_or: &AndOrList) -> i32 {
        let mut status = self.exec_pipeline(&and_or.first);

        for (op, pipeline) in &and_or.rest {
            match op {
                AndOrOp::And => {
                    if status == 0 {
                        status = self.exec_pipeline(pipeline);
                    }
                }
                AndOrOp::Or => {
                    if status != 0 {
                        status = self.exec_pipeline(pipeline);
                    }
                }
            }
        }

        self.env.last_exit_status = status;
        status
    }

    /// Reap any zombie background children without blocking.
    fn reap_zombies(&self) {
        loop {
            match nix::sys::wait::waitpid(
                nix::unistd::Pid::from_raw(-1),
                Some(nix::sys::wait::WaitPidFlag::WNOHANG),
            ) {
                Ok(nix::sys::wait::WaitStatus::StillAlive) => break,
                Ok(_) => continue, // Reaped a zombie, check for more
                Err(_) => break,   // No children or error
            }
        }
    }

    /// Execute a complete command (list of AND-OR lists with separators).
    pub fn exec_complete_command(&mut self, cmd: &CompleteCommand) -> i32 {
        // Reap any finished background children before forking new ones
        self.reap_zombies();

        let mut status = 0;

        for (and_or, separator) in &cmd.items {
            if separator == &Some(SeparatorOp::Amp) {
                // Background: fork child to execute, parent continues with status 0
                match unsafe { fork() } {
                    Err(e) => {
                        eprintln!("kish: fork: {}", e);
                        status = 1;
                    }
                    Ok(ForkResult::Child) => {
                        let s = self.exec_and_or(and_or);
                        std::process::exit(s);
                    }
                    Ok(ForkResult::Parent { child }) => {
                        // Track last background PID ($!)
                        self.env.last_bg_pid = Some(child.as_raw());
                        // Parent continues; background job status = 0
                        status = 0;
                    }
                }
            } else {
                // Sequential execution
                status = self.exec_and_or(and_or);
            }
        }

        self.env.last_exit_status = status;
        status
    }

    /// Execute a program (sequence of complete commands).
    pub fn exec_program(&mut self, program: &Program) -> i32 {
        let mut status = 0;
        for cmd in &program.commands {
            status = self.exec_complete_command(cmd);
        }
        self.env.last_exit_status = status;
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{
        AndOrList, AndOrOp, Command, CompleteCommand, Pipeline, Program, SeparatorOp,
        SimpleCommand, Word,
    };

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

    #[test]
    fn test_single_command_pipeline() {
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let pipeline = Pipeline {
            negated: false,
            commands: vec![Command::Simple(SimpleCommand {
                assignments: vec![],
                words: vec![Word::literal("true")],
                redirects: vec![],
            })],
        };
        assert_eq!(exec.exec_pipeline(&pipeline), 0);
    }

    #[test]
    fn test_negated_pipeline() {
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let pipeline = Pipeline {
            negated: true,
            commands: vec![Command::Simple(SimpleCommand {
                assignments: vec![],
                words: vec![Word::literal("true")],
                redirects: vec![],
            })],
        };
        assert_eq!(exec.exec_pipeline(&pipeline), 1);
    }

    fn make_pipeline(word: &str) -> Pipeline {
        Pipeline {
            negated: false,
            commands: vec![Command::Simple(SimpleCommand {
                assignments: vec![],
                words: vec![Word::literal(word)],
                redirects: vec![],
            })],
        }
    }

    #[test]
    fn test_and_list_all_succeed() {
        // true && true → 0
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let and_or = AndOrList {
            first: make_pipeline("true"),
            rest: vec![(AndOrOp::And, make_pipeline("true"))],
        };
        assert_eq!(exec.exec_and_or(&and_or), 0);
    }

    #[test]
    fn test_and_list_first_fails() {
        // false && true → 1 (second not executed)
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let and_or = AndOrList {
            first: make_pipeline("false"),
            rest: vec![(AndOrOp::And, make_pipeline("true"))],
        };
        assert_eq!(exec.exec_and_or(&and_or), 1);
    }

    #[test]
    fn test_or_list_first_fails() {
        // false || true → 0
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let and_or = AndOrList {
            first: make_pipeline("false"),
            rest: vec![(AndOrOp::Or, make_pipeline("true"))],
        };
        assert_eq!(exec.exec_and_or(&and_or), 0);
    }

    #[test]
    fn test_or_list_first_succeeds() {
        // true || false → 0 (second not executed)
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let and_or = AndOrList {
            first: make_pipeline("true"),
            rest: vec![(AndOrOp::Or, make_pipeline("false"))],
        };
        assert_eq!(exec.exec_and_or(&and_or), 0);
    }

    #[test]
    fn test_exec_program_sequential() {
        // true; false → 1 (last command status)
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let program = Program {
            commands: vec![
                CompleteCommand {
                    items: vec![(
                        AndOrList {
                            first: make_pipeline("true"),
                            rest: vec![],
                        },
                        Some(SeparatorOp::Semi),
                    )],
                },
                CompleteCommand {
                    items: vec![(
                        AndOrList {
                            first: make_pipeline("false"),
                            rest: vec![],
                        },
                        None,
                    )],
                },
            ],
        };
        assert_eq!(exec.exec_program(&program), 1);
    }
}
