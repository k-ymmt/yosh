use nix::unistd::{ForkResult, fork};

use super::{Executor, exit_child, preview_command};
use crate::error::{RuntimeErrorKind, ShellError};
use crate::parser::ast::{AndOrList, AndOrOp, Command, CompleteCommand, Program, SeparatorOp};
use crate::signal;

impl Executor {
    /// Dispatch a `Command` to the appropriate execution path.
    pub fn exec_command(&mut self, cmd: &Command) -> i32 {
        if self.env.mode.options.noexec {
            return 0;
        }
        match cmd {
            Command::Simple(simple) => match self.exec_simple_command(simple) {
                Ok(status) => status,
                Err(e) => {
                    eprintln!("{}", e);
                    let code = e.exit_code();
                    self.env.exec.last_exit_status = code;
                    code
                }
            },
            Command::Compound(compound, redirects) => {
                match self.exec_compound_command(compound, redirects) {
                    Ok(status) => status,
                    Err(e) => {
                        eprintln!("{}", e);
                        self.env.exec.last_exit_status = e.exit_code();
                        e.exit_code()
                    }
                }
            }
            Command::FunctionDef(func_def) => {
                self.env
                    .functions
                    .insert(func_def.name.clone(), func_def.clone());
                0
            }
        }
    }

    /// Execute an AND-OR list.
    pub fn exec_and_or(&mut self, and_or: &AndOrList) -> i32 {
        let has_rest = !and_or.rest.is_empty();

        let mut status = if and_or.first.negated || has_rest {
            self.with_errexit_suppressed(|e| e.exec_pipeline(&and_or.first))
        } else {
            self.exec_pipeline(&and_or.first)
        };

        if self.env.exec.flow_control.is_some() || self.exit_requested.is_some() {
            return status;
        }

        for (i, (op, pipeline)) in and_or.rest.iter().enumerate() {
            let is_last = i == and_or.rest.len() - 1;
            let should_run = match op {
                AndOrOp::And => status == 0,
                AndOrOp::Or => status != 0,
            };
            if !should_run {
                continue;
            }

            status = if pipeline.negated || !is_last {
                self.with_errexit_suppressed(|e| e.exec_pipeline(pipeline))
            } else {
                self.exec_pipeline(pipeline)
            };

            if self.env.exec.flow_control.is_some() || self.exit_requested.is_some() {
                break;
            }
        }

        self.env.exec.last_exit_status = status;
        status
    }

    /// Reap any zombie background children without blocking.
    pub(crate) fn reap_zombies(&mut self) {
        use crate::env::jobs::JobStatus;
        loop {
            match nix::sys::wait::waitpid(
                nix::unistd::Pid::from_raw(-1),
                Some(nix::sys::wait::WaitPidFlag::WNOHANG | nix::sys::wait::WaitPidFlag::WUNTRACED),
            ) {
                Ok(nix::sys::wait::WaitStatus::Exited(pid, code)) => {
                    self.env
                        .process
                        .jobs
                        .update_status(pid, JobStatus::Done(code));
                }
                Ok(nix::sys::wait::WaitStatus::Signaled(pid, sig, _)) => {
                    self.env
                        .process
                        .jobs
                        .update_status(pid, JobStatus::Terminated(sig as i32));
                }
                Ok(nix::sys::wait::WaitStatus::Stopped(pid, sig)) => {
                    self.env
                        .process
                        .jobs
                        .update_status(pid, JobStatus::Stopped(sig as i32));
                }
                Ok(nix::sys::wait::WaitStatus::StillAlive) => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    }

    /// Execute a command asynchronously (background with &).
    fn exec_async(&mut self, and_or: &AndOrList) -> Result<i32, ShellError> {
        match unsafe { fork() } {
            Err(e) => Err(ShellError::runtime(
                RuntimeErrorKind::IoError,
                format!("fork: {}", e),
            )),
            Ok(ForkResult::Child) => {
                // Set process group BEFORE signal setup to ensure proper isolation.
                let pid = nix::unistd::getpid();
                nix::unistd::setpgid(pid, pid).ok();

                let ignored = self.env.traps.ignored_signals();
                self.env.traps.reset_non_ignored();
                if self.env.mode.options.monitor {
                    signal::setup_background_child_signals(&ignored);
                } else {
                    signal::reset_child_signals(&ignored);
                }

                // Note: we do NOT call ignore_signal(SIGINT/SIGQUIT) here.
                // setpgid already isolates this process from keyboard signals,
                // and reset_child_signals would undo the ignore anyway.
                let status = self.exec_and_or(and_or);
                exit_child(status);
            }
            Ok(ForkResult::Parent { child }) => {
                nix::unistd::setpgid(child, child).ok();
                let command_name = preview_command(and_or);
                let job_id = self
                    .env
                    .process
                    .jobs
                    .add_job(child, vec![child], command_name, false);
                eprintln!("[{}] {}", job_id, child.as_raw());
                Ok(0)
            }
        }
    }

    /// Execute a complete command (list of AND-OR lists with separators).
    pub fn exec_complete_command(&mut self, cmd: &CompleteCommand) -> i32 {
        // Reap any finished background children before forking new ones
        self.reap_zombies();

        // -b flag: immediate job notification
        if self.env.mode.options.notify {
            self.display_job_notifications();
        }

        let mut status = 0;

        for (and_or, separator) in &cmd.items {
            if separator == &Some(SeparatorOp::Amp) {
                status = match self.exec_async(and_or) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("{}", e);
                        e.exit_code()
                    }
                };
            } else {
                // Sequential execution
                status = self.exec_and_or(and_or);
            }
            if self.env.exec.flow_control.is_some() {
                break;
            }
            self.check_errexit(status);
            if self.exit_requested.is_some() {
                break;
            }
        }

        self.env.exec.last_exit_status = status;
        status
    }

    /// Execute a program (sequence of complete commands).
    pub fn exec_program(&mut self, program: &Program) -> i32 {
        let mut status = 0;
        for cmd in &program.commands {
            status = self.exec_complete_command(cmd);
            if self.exit_requested.is_some() {
                break;
            }
        }
        self.env.exec.last_exit_status = status;
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{CompoundCommand, CompoundCommandKind, Pipeline, SimpleCommand, Word};

    fn make_simple_cmd(words: &[&str]) -> SimpleCommand {
        SimpleCommand {
            assignments: vec![],
            words: words.iter().map(|s| Word::literal(s)).collect(),
            redirects: vec![],
            line: 0,
        }
    }

    #[test]
    fn exec_builtin_true_returns_0() {
        let mut exec = Executor::new("yosh", vec![]);
        let cmd = make_simple_cmd(&["true"]);
        assert_eq!(exec.exec_simple_command(&cmd), Ok(0));
        assert_eq!(exec.env.exec.last_exit_status, 0);
    }

    #[test]
    fn exec_builtin_false_returns_1() {
        let mut exec = Executor::new("yosh", vec![]);
        let cmd = make_simple_cmd(&["false"]);
        assert_eq!(exec.exec_simple_command(&cmd), Ok(1));
        assert_eq!(exec.env.exec.last_exit_status, 1);
    }

    #[test]
    fn exec_external_true_returns_0() {
        let mut exec = Executor::new("yosh", vec![]);
        let cmd = make_simple_cmd(&["/usr/bin/true"]);
        assert_eq!(exec.exec_simple_command(&cmd), Ok(0));
    }

    #[test]
    fn assignment_only_sets_var() {
        use crate::parser::ast::Assignment;
        let mut exec = Executor::new("yosh", vec![]);
        let cmd = SimpleCommand {
            assignments: vec![Assignment {
                name: "MYVAR".to_string(),
                value: Some(Word::literal("hello")),
            }],
            words: vec![],
            redirects: vec![],
            line: 0,
        };
        let status = exec.exec_simple_command(&cmd).unwrap();
        assert_eq!(status, 0);
        assert_eq!(exec.env.vars.get("MYVAR"), Some("hello"));
    }

    #[test]
    fn exit_status_tracked() {
        let mut exec = Executor::new("yosh", vec![]);
        // false sets last_exit_status to 1
        let false_cmd = make_simple_cmd(&["false"]);
        let _ = exec.exec_simple_command(&false_cmd);
        assert_eq!(exec.env.exec.last_exit_status, 1);

        // true resets it to 0
        let true_cmd = make_simple_cmd(&["true"]);
        let _ = exec.exec_simple_command(&true_cmd);
        assert_eq!(exec.env.exec.last_exit_status, 0);
    }

    #[test]
    fn test_single_command_pipeline() {
        let mut exec = Executor::new("yosh".to_string(), vec![]);
        let pipeline = Pipeline {
            negated: false,
            commands: vec![Command::Simple(SimpleCommand {
                assignments: vec![],
                words: vec![Word::literal("true")],
                redirects: vec![],
                line: 0,
            })],
        };
        assert_eq!(exec.exec_pipeline(&pipeline), 0);
    }

    #[test]
    fn test_negated_pipeline() {
        let mut exec = Executor::new("yosh".to_string(), vec![]);
        let pipeline = Pipeline {
            negated: true,
            commands: vec![Command::Simple(SimpleCommand {
                assignments: vec![],
                words: vec![Word::literal("true")],
                redirects: vec![],
                line: 0,
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
                line: 0,
            })],
        }
    }

    #[test]
    fn test_and_list_all_succeed() {
        // true && true → 0
        let mut exec = Executor::new("yosh".to_string(), vec![]);
        let and_or = AndOrList {
            first: make_pipeline("true"),
            rest: vec![(AndOrOp::And, make_pipeline("true"))],
        };
        assert_eq!(exec.exec_and_or(&and_or), 0);
    }

    #[test]
    fn test_and_list_first_fails() {
        // false && true → 1 (second not executed)
        let mut exec = Executor::new("yosh".to_string(), vec![]);
        let and_or = AndOrList {
            first: make_pipeline("false"),
            rest: vec![(AndOrOp::And, make_pipeline("true"))],
        };
        assert_eq!(exec.exec_and_or(&and_or), 1);
    }

    #[test]
    fn test_or_list_first_fails() {
        // false || true → 0
        let mut exec = Executor::new("yosh".to_string(), vec![]);
        let and_or = AndOrList {
            first: make_pipeline("false"),
            rest: vec![(AndOrOp::Or, make_pipeline("true"))],
        };
        assert_eq!(exec.exec_and_or(&and_or), 0);
    }

    #[test]
    fn test_or_list_first_succeeds() {
        // true || false → 0 (second not executed)
        let mut exec = Executor::new("yosh".to_string(), vec![]);
        let and_or = AndOrList {
            first: make_pipeline("true"),
            rest: vec![(AndOrOp::Or, make_pipeline("false"))],
        };
        assert_eq!(exec.exec_and_or(&and_or), 0);
    }

    #[test]
    fn test_exec_program_sequential() {
        // true; false → 1 (last command status)
        let mut exec = Executor::new("yosh".to_string(), vec![]);
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

    #[test]
    fn exec_and_or_stops_after_first_pipeline_when_exit_requested() {
        // Simulates: exit 0 && echo X — the && branch should not execute
        let mut exec = Executor::new("yosh".to_string(), vec![]);
        exec.exit_requested = Some(0);
        let and_or = AndOrList {
            first: make_pipeline("true"),
            rest: vec![(AndOrOp::And, make_pipeline("false"))],
        };
        let status = exec.exec_and_or(&and_or);
        assert_eq!(status, 0);
        assert_eq!(exec.exit_requested, Some(0));
    }

    #[test]
    fn exec_and_or_stops_after_rest_pipeline_when_exit_requested() {
        // Simulates: false || exit 0 && echo X — after exit sets exit_requested,
        // the && branch should not execute
        let mut exec = Executor::new("yosh".to_string(), vec![]);
        let and_or = AndOrList {
            first: make_pipeline("false"),
            rest: vec![
                (AndOrOp::Or, make_pipeline("true")),
                (AndOrOp::And, make_pipeline("false")),
            ],
        };
        // Set exit_requested after first rest pipeline would execute
        // To test the loop check, we pre-set exit_requested; the second rest
        // pipeline ("false") should be skipped.
        exec.exit_requested = Some(0);
        let status = exec.exec_and_or(&and_or);
        // First pipeline returns 1 (false), but exit_requested stops before it runs
        assert_eq!(status, 1);
        assert_eq!(exec.exit_requested, Some(0));
    }

    // ── LINENO update tests ─────────────────────────────────────

    #[test]
    fn exec_simple_command_sets_lineno() {
        let mut exec = Executor::new("yosh", vec![]);
        let cmd = SimpleCommand {
            assignments: vec![],
            words: vec![Word::literal("true")],
            redirects: vec![],
            line: 5,
        };
        let _ = exec.exec_simple_command(&cmd);
        assert_eq!(exec.env.vars.get("LINENO"), Some("5"));
    }

    #[test]
    fn exec_compound_command_sets_lineno() {
        let mut exec = Executor::new("yosh", vec![]);
        let cmd = CompoundCommand {
            kind: CompoundCommandKind::BraceGroup {
                body: vec![CompleteCommand {
                    items: vec![(
                        AndOrList {
                            first: Pipeline {
                                negated: false,
                                commands: vec![Command::Simple(SimpleCommand {
                                    assignments: vec![],
                                    words: vec![Word::literal("true")],
                                    redirects: vec![],
                                    line: 11,
                                })],
                            },
                            rest: vec![],
                        },
                        None,
                    )],
                }],
            },
            line: 10,
            assignments: vec![],
        };
        let _ = exec.exec_compound_command(&cmd, &[]);
        // Inner SimpleCommand (line 11) runs last, so LINENO ends at 11.
        assert_eq!(exec.env.vars.get("LINENO"), Some("11"));
    }

    #[test]
    fn exec_compound_subshell_sets_lineno_on_entry() {
        // yosh forks the subshell body into a child process, so the parent's
        // env.vars is never modified by the child's execution. After the
        // subshell compound is entered (setting LINENO to 7), the parent waits
        // for the child and its LINENO remains at the compound's line (7).
        let mut exec = Executor::new("yosh", vec![]);
        let cmd = CompoundCommand {
            kind: CompoundCommandKind::Subshell {
                body: vec![CompleteCommand {
                    items: vec![(
                        AndOrList {
                            first: Pipeline {
                                negated: false,
                                commands: vec![Command::Simple(SimpleCommand {
                                    assignments: vec![],
                                    words: vec![Word::literal(":")],
                                    redirects: vec![],
                                    line: 22,
                                })],
                            },
                            rest: vec![],
                        },
                        None,
                    )],
                }],
            },
            line: 7,
            assignments: vec![],
        };
        let _ = exec.exec_compound_command(&cmd, &[]);
        assert_eq!(exec.env.vars.get("LINENO"), Some("7"));
    }
}
