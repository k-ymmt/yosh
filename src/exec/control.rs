use nix::unistd::{ForkResult, fork};

use super::{Executor, exit_child, preview_command};
use crate::error::{RuntimeErrorKind, ShellError};
use crate::parser::ast::{
    AndOrList, AndOrOp, Command, CompleteCommand, Pipeline, Program, SeparatorOp,
};
use crate::signal;

impl Executor {
    /// Dispatch a `Command` to the appropriate execution path.
    pub fn exec_command(&mut self, cmd: &Command) -> i32 {
        // POSIX XCU set: "-n ... This option is ignored by interactive
        // shells" — without the guard, `yosh -n` at a terminal wedges the
        // REPL (every command including `set +n` and `exit` becomes a
        // no-op). flag_i covers command-substitution children, which run
        // with is_interactive=false but belong to an interactive shell;
        // without it, `$(...)` inside an interactive -n shell silently
        // expands to nothing (bash keeps -n ignored there too).
        if self.env.mode.options.noexec && !(self.env.mode.is_interactive || self.env.mode.flag_i) {
            return 0;
        }
        match cmd {
            Command::Simple(simple) => match self.exec_simple_command(simple) {
                Ok(status) => status,
                Err(e) => self.report_command_error(&e),
            },
            Command::Compound(compound, redirects) => {
                match self.exec_compound_command(compound, redirects) {
                    Ok(status) => status,
                    Err(e) => self.report_command_error(&e),
                }
            }
            Command::FunctionDef(func_def) => {
                // POSIX §2.9.5: a function may not be named after a special
                // built-in; the definition is an error, and per §2.8.1 a
                // non-interactive shell exits on it.
                if matches!(
                    crate::builtin::classify_builtin(&func_def.name),
                    crate::builtin::BuiltinKind::Special
                ) {
                    eprintln!(
                        "yosh: {}: cannot define a function named after a special builtin",
                        func_def.name
                    );
                    self.env.exec.last_exit_status = 2;
                    if !self.env.mode.is_interactive {
                        self.exit_requested = Some(2);
                    }
                    return 2;
                }
                self.env
                    .functions
                    .insert(func_def.name.clone(), func_def.clone());
                0
            }
        }
    }

    /// Print a command-level `ShellError` and return its exit code.
    /// Per the POSIX §2.8.1 consequences table, expansion errors and
    /// variable-assignment errors terminate a non-interactive shell —
    /// requested via `exit_requested` so the top-level driver unwinds
    /// (and fires the EXIT trap) instead of `process::exit`ing from
    /// arbitrarily deep in the executor.
    fn report_command_error(&mut self, e: &ShellError) -> i32 {
        eprintln!("{}", e);
        let code = e.exit_code();
        self.env.exec.last_exit_status = code;
        if !self.env.mode.is_interactive && e.requires_noninteractive_exit() {
            self.exit_requested = Some(code);
        }
        code
    }

    /// Execute an AND-OR list.
    pub fn exec_and_or(&mut self, and_or: &AndOrList) -> i32 {
        let has_rest = !and_or.rest.is_empty();

        // POSIX §2.15 set: the final status is exempt from `set -e` when the
        // pipeline that produced it began with `!`, or when it came from a
        // non-final component of the list (short-circuit). The flag is set
        // AFTER each pipeline runs (nested lists inside the pipeline set it
        // for themselves) so the value left when we return describes the
        // status we return.
        let first_exempt = and_or.first.negated || has_rest;
        let mut status = self.exec_pipeline_errexit(&and_or.first, first_exempt);

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

            let exempt = pipeline.negated || !is_last;
            status = self.exec_pipeline_errexit(pipeline, exempt);

            if self.env.exec.flow_control.is_some() || self.exit_requested.is_some() {
                break;
            }
        }

        self.env.exec.last_exit_status = status;
        status
    }

    /// Run one pipeline of an AND-OR list, maintaining
    /// `errexit_exempt_status`: cleared before the run (so a stale value
    /// from a previous list cannot leak in), forced on afterwards when this
    /// pipeline is itself exempt, and otherwise left as the pipeline's body
    /// set it — an in-process compound (brace group, loop, case) whose final
    /// status came from an exempt pipeline propagates that exemption
    /// (matches bash/dash; functions and subshells do not propagate).
    fn exec_pipeline_errexit(&mut self, pipeline: &Pipeline, exempt: bool) -> i32 {
        self.errexit_exempt_status = false;
        let status = if exempt {
            self.with_errexit_suppressed(|e| e.exec_pipeline(pipeline))
        } else {
            self.exec_pipeline(pipeline)
        };
        if exempt {
            self.errexit_exempt_status = true;
        }
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
        // Block signals across the fork: the child inherits the parent's
        // self-pipe handler AND the shared pipe, so a signal delivered to
        // the child before reset_child_signals runs would be written into
        // the pipe and later misread by the parent as its own (observed as
        // `kill -TERM $!` racing the child's signal reset and terminating
        // the parent shell). Both sides restore the mask below.
        let all_signals = nix::sys::signal::SigSet::all();
        let prev_mask = nix::sys::signal::SigSet::empty();
        let mut prev_mask_opt = Some(prev_mask);
        let _ = nix::sys::signal::sigprocmask(
            nix::sys::signal::SigmaskHow::SIG_BLOCK,
            Some(&all_signals),
            prev_mask_opt.as_mut(),
        );
        let prev_mask = prev_mask_opt.unwrap();
        match unsafe { fork() } {
            Err(e) => {
                let _ = nix::sys::signal::sigprocmask(
                    nix::sys::signal::SigmaskHow::SIG_SETMASK,
                    Some(&prev_mask),
                    None,
                );
                Err(ShellError::runtime(
                    RuntimeErrorKind::IoError,
                    format!("fork: {}", e),
                ))
            }
            Ok(ForkResult::Child) => {
                // Set process group BEFORE signal setup to ensure proper isolation.
                let pid = nix::unistd::getpid();
                nix::unistd::setpgid(pid, pid).ok();

                let ignored = self.env.traps.ignored_signals();
                self.env.traps.reset_for_subshell();
                // The async child is a fresh subshell: the parent's
                // remembered reaped statuses and terminal table jobs
                // are not its children.
                self.env.process.jobs.reset_for_subshell();
                if self.env.mode.options.monitor {
                    signal::setup_background_child_signals(&ignored);
                    // A background job is a subshell, not a job-controlling
                    // shell: with monitor left on, a nested external command
                    // would be forked into its own new process group and this
                    // (background) subshell would call tcsetpgrp around it,
                    // stopping the job with SIGTTOU. Nested commands must
                    // stay in this job's process group instead.
                    self.env.mode.options.monitor = false;
                } else {
                    // POSIX §2.9.3.1 / §2.12: with job control disabled,
                    // commands in an asynchronous list ignore SIGINT and
                    // SIGQUIT, and read stdin from /dev/null (before any
                    // explicit redirection, which happens later during
                    // command execution). Record the ignores in the trap
                    // store so nested forks (subshells, exec'd commands)
                    // keep them ignored; a `trap` in the async list may
                    // still override them.
                    let mut ignored = ignored;
                    self.env
                        .traps
                        .signal_traps
                        .insert(libc::SIGINT, crate::env::TrapAction::Ignore);
                    self.env
                        .traps
                        .signal_traps
                        .insert(libc::SIGQUIT, crate::env::TrapAction::Ignore);
                    ignored.push(libc::SIGINT);
                    ignored.push(libc::SIGQUIT);
                    signal::reset_child_signals(&ignored);
                    if let Ok(devnull) = std::fs::File::open("/dev/null") {
                        use std::os::fd::AsRawFd;
                        unsafe {
                            libc::dup2(devnull.as_raw_fd(), libc::STDIN_FILENO);
                        }
                    }
                }
                // Signal dispositions are now the child's own — deliver
                // anything that arrived while the fork window was blocked.
                let _ = nix::sys::signal::sigprocmask(
                    nix::sys::signal::SigmaskHow::SIG_SETMASK,
                    Some(&prev_mask),
                    None,
                );

                let status = self.exec_and_or(and_or);
                exit_child(status);
            }
            Ok(ForkResult::Parent { child }) => {
                let _ = nix::sys::signal::sigprocmask(
                    nix::sys::signal::SigmaskHow::SIG_SETMASK,
                    Some(&prev_mask),
                    None,
                );
                nix::unistd::setpgid(child, child).ok();
                let command_name = preview_command(and_or);
                let job_id = self
                    .env
                    .process
                    .jobs
                    .add_job(child, vec![child], command_name, false);
                // POSIX §2.9.3.1: the "[n] pid" notice belongs to job
                // control; plain non-interactive scripts stay silent
                // (bash/dash agree).
                if self.env.mode.is_interactive || self.env.mode.options.monitor {
                    eprintln!("[{}] {}", job_id, child.as_raw());
                }
                Ok(0)
            }
        }
    }

    /// Execute a complete command (list of AND-OR lists with separators).
    pub fn exec_complete_command(&mut self, cmd: &CompleteCommand) -> i32 {
        // noexec (set -n) stubs the whole complete command here, above
        // the AND-OR machinery: returning 0 from exec_command alone is
        // not enough — a trailing `! cmd` would negate the stub 0 into
        // exit 1, and `cmd &` would fork and print a job line before
        // ever reaching exec_command (bash -n does neither). Interactive
        // exemption mirrors the exec_command guard (POSIX: -n is
        // ignored by interactive shells; flag_i covers command subs).
        if self.env.mode.options.noexec && !(self.env.mode.is_interactive || self.env.mode.flag_i) {
            return 0;
        }
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
        // POSIX §2.12: handle async signals (SIGINT trap etc.) between commands.
        self.process_pending_signals();
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
        assert_eq!(exec.env.exec.lineno, 5);
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
        assert_eq!(exec.env.exec.lineno, 11);
    }

    #[test]
    fn exec_compound_subshell_sets_lineno_on_entry() {
        // yosh forks the subshell body into a child process, so the parent's
        // env.exec.lineno is never modified by the child's execution. After
        // the subshell compound is entered (setting LINENO to 7), the parent
        // waits for the child and its LINENO remains at the compound's line (7).
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
        assert_eq!(exec.env.exec.lineno, 7);
    }
}
