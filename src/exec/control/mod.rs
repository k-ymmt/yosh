//! Driving command lists: dispatching one command, AND-OR lists
//! ([`and_or`]), asynchronous lists ([`background`]), complete
//! commands and whole programs, and reporting command-level errors.

mod and_or;
mod background;

use super::Executor;
use crate::error::ShellError;
use crate::parser::ast::{Command, CompleteCommand, FunctionDef, Program, SeparatorOp};

impl Executor {
    /// `set -n` (noexec) is in effect: commands are read but not run.
    /// POSIX XCU set: "-n ... This option is ignored by interactive
    /// shells"; flag_i covers command-substitution children, which run
    /// with is_interactive=false but belong to an interactive shell.
    fn noexec_in_effect(&self) -> bool {
        self.env.mode.options.noexec && !(self.env.mode.is_interactive || self.env.mode.flag_i)
    }

    /// Dispatch a `Command` to the appropriate execution path.
    pub(crate) fn exec_command(&mut self, cmd: &Command) -> i32 {
        // POSIX XCU set: "-n ... This option is ignored by interactive
        // shells" — without the guard, `yosh -n` at a terminal wedges the
        // REPL (every command including `set +n` and `exit` becomes a
        // no-op). flag_i covers command-substitution children, which run
        // with is_interactive=false but belong to an interactive shell;
        // without it, `$(...)` inside an interactive -n shell silently
        // expands to nothing (bash keeps -n ignored there too).
        if self.noexec_in_effect() {
            return 0;
        }
        match cmd {
            Command::Simple(simple) => {
                let status = match self.exec_simple_command(simple) {
                    Ok(status) => status,
                    Err(e) => self.report_command_error(&e),
                };
                // Only compound commands propagate the `set -e` exemption
                // of their last AND-OR list. A simple command that ran
                // shell code — `eval`, `.`, a function — is subject to
                // `set -e` with its own status even when that code ended
                // in an exempt list: `set -e; eval 'false && true'; echo
                // no` exits 1 (dash and bash agree).
                self.clear_errexit_exempt();
                status
            }
            Command::Compound(compound, redirects) => {
                match self.exec_compound_command(compound, redirects) {
                    Ok(status) => status,
                    Err(e) => self.report_command_error(&e),
                }
            }
            Command::FunctionDef(func_def) => self.define_function(func_def),
        }
    }

    /// Run a function definition command: register `func_def` and
    /// return 0, or reject a name that is a special built-in.
    fn define_function(&mut self, func_def: &FunctionDef) -> i32 {
        // POSIX §2.9.5: a function may not be named after a special
        // built-in; the definition is an error, and per §2.8.1 a
        // non-interactive shell exits on it.
        if matches!(
            crate::builtin::classify_builtin(&func_def.name),
            crate::builtin::BuiltinKind::Special
        ) {
            crate::byteenc::write_stderr_decoded_line(&format!(
                "yosh: {}: cannot define a function named after a special builtin",
                func_def.name
            ));
            self.env.exec.last_exit_status = 2;
            if !self.env.mode.is_interactive {
                self.exit_requested = Some(2);
            }
            return 2;
        }
        // One `Rc::new` per definition; every subsequent call is
        // a refcount bump (see `ShellEnv::functions`). The body
        // inside `FunctionDef` is already `Rc<CompoundCommand>`,
        // so this clone is shallow.
        self.env
            .functions
            .insert(func_def.name.clone(), std::rc::Rc::new(func_def.clone()));
        0
    }

    /// Print a command-level `ShellError` and return its exit code.
    /// Per the POSIX §2.8.1 consequences table, expansion errors and
    /// variable-assignment errors terminate a non-interactive shell —
    /// requested via `exit_requested` so the top-level driver unwinds
    /// (and fires the EXIT trap) instead of `process::exit`ing from
    /// arbitrarily deep in the executor.
    pub(super) fn report_command_error(&mut self, e: &ShellError) -> i32 {
        // Empty message = the diagnostic was already printed at the point
        // of failure (e.g. `param::expansion_error` for a `${x:?}` /
        // nounset failure inside a heredoc body, converted to a
        // redirection error in `redirect::expand_one`). Printing here
        // would duplicate the line; dash/bash print it once.
        if !e.message.is_empty() {
            crate::byteenc::write_stderr_decoded_line(&e.to_string());
        }
        let code = e.exit_code();
        self.env.exec.last_exit_status = code;
        if !self.env.mode.is_interactive && e.requires_noninteractive_exit() {
            self.exit_requested = Some(code);
        }
        code
    }

    /// Execute a complete command (list of AND-OR lists with separators).
    pub(crate) fn exec_complete_command(&mut self, cmd: &CompleteCommand) -> i32 {
        // Taken first so no nested complete command (eval, a function
        // body, a forked subshell's copy) ever sees the hint.
        let shell_tail = std::mem::take(&mut self.shell_tail);
        // noexec (set -n) stubs the whole complete command here, above
        // the AND-OR machinery: returning 0 from exec_command alone is
        // not enough — a trailing `! cmd` would negate the stub 0 into
        // exit 1, and `cmd &` would fork and print a job line before
        // ever reaching exec_command (bash -n does neither). Interactive
        // exemption mirrors the exec_command guard (POSIX: -n is
        // ignored by interactive shells; flag_i covers command subs).
        if self.noexec_in_effect() {
            return 0;
        }
        // Reap any finished background children before forking new ones
        self.reap_zombies();

        // -b flag: immediate job notification
        if self.env.mode.options.notify {
            self.display_job_notifications();
        }

        let mut status = 0;
        let last = cmd.items.len().saturating_sub(1);

        for (i, (and_or, separator)) in cmd.items.iter().enumerate() {
            if separator == &Some(SeparatorOp::Amp) {
                status = match self.exec_async(and_or) {
                    Ok(s) => s,
                    Err(e) => {
                        crate::byteenc::write_stderr_decoded_line(&e.to_string());
                        e.exit_code()
                    }
                };
            } else {
                if shell_tail && i == last {
                    self.arm_exec_in_place_if_shell_tail(and_or);
                }
                // Sequential execution
                status = self.exec_and_or(and_or);
            }
            if self.list_item_stops_list(status) {
                break;
            }
        }

        self.env.exec.last_exit_status = status;
        // Catch signals that arrived during the final item when an early
        // `break` above skipped the in-loop drain.
        self.process_pending_signals();
        status
    }

    /// Bookkeeping after one item of a command list — an AND-OR list of a
    /// complete command, or a complete command of a compound-list body:
    /// unwinding, `set -e`, then pending trap actions. Returns true when
    /// the rest of the list must not run.
    pub(super) fn list_item_stops_list(&mut self, status: i32) -> bool {
        if self.should_unwind() {
            return true;
        }
        self.check_errexit(status);
        if self.exit_requested.is_some() {
            return true;
        }
        // POSIX §2.12: a trapped signal's action runs "after the
        // execution of the current command" — i.e. between the items
        // of this list, not only after the whole list finished
        // (`trap 'echo T' USR1; kill -USR1 $$; echo one` prints T
        // before one; bash/dash agree). Cheap when nothing is
        // pending: gated on an atomic flag, zero syscalls.
        self.process_pending_signals();
        self.exit_requested.is_some()
    }

    /// Execute a program (sequence of complete commands).
    ///
    /// Stays `pub` (unlike the other `exec_*` AST-walking methods, which
    /// are `pub(crate)`): `benches/exec_bench.rs` and `src/bin/yosh-dhat.rs`
    /// are separate crates that drive the executor through this method.
    /// `yosh-dhat` additionally relies on the public lifecycle methods
    /// (`execute_exit_trap` et al.) staying `pub`.
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
    use crate::parser::ast::{
        AndOrList, AndOrOp, CompoundCommand, CompoundCommandKind, Pipeline, SimpleCommand, Word,
    };

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
