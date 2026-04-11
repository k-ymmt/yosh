use nix::unistd::{fork, ForkResult};

use crate::env::FlowControl;
use crate::expand::expand_words;
use crate::parser::ast::{CaseItem, CaseTerminator, CompoundCommand, CompleteCommand, Redirect, Word};
use crate::signal;

use super::command;
use super::redirect::RedirectState;
use super::Executor;

impl Executor {
    /// Execute a compound command, applying any redirects around it.
    pub(crate) fn exec_compound_command(
        &mut self,
        compound: &CompoundCommand,
        redirects: &[Redirect],
    ) -> i32 {
        let mut redirect_state = RedirectState::new();
        if let Err(e) = redirect_state.apply(redirects, &mut self.env, true) {
            eprintln!("kish: {}", e);
            self.env.exec.last_exit_status = 1;
            return 1;
        }

        let status = match &compound.kind {
            crate::parser::ast::CompoundCommandKind::BraceGroup { body } => {
                self.exec_brace_group(body)
            }
            crate::parser::ast::CompoundCommandKind::Subshell { body } => {
                self.exec_subshell(body)
            }
            crate::parser::ast::CompoundCommandKind::If {
                condition,
                then_part,
                elif_parts,
                else_part,
            } => self.exec_if(condition, then_part, elif_parts, else_part),
            crate::parser::ast::CompoundCommandKind::While { condition, body } => {
                self.exec_loop(condition, body, false)
            }
            crate::parser::ast::CompoundCommandKind::Until { condition, body } => {
                self.exec_loop(condition, body, true)
            }
            crate::parser::ast::CompoundCommandKind::For { var, words, body } => {
                self.exec_for(var, words, body)
            }
            crate::parser::ast::CompoundCommandKind::Case { word, items } => {
                self.exec_case(word, items)
            }
        };

        redirect_state.restore();
        self.env.exec.last_exit_status = status;
        status
    }

    /// Execute a list of complete commands (a compound-list / body).
    /// Checks for flow control signals after each command.
    pub(crate) fn exec_body(&mut self, body: &[CompleteCommand]) -> i32 {
        let mut status = 0;
        for cmd in body {
            status = self.exec_complete_command(cmd);
            if self.env.exec.flow_control.is_some() {
                break;
            }
            self.check_errexit(status);
            self.process_pending_signals();
        }
        status
    }

    fn exec_brace_group(&mut self, body: &[CompleteCommand]) -> i32 {
        self.exec_body(body)
    }

    fn exec_subshell(&mut self, body: &[CompleteCommand]) -> i32 {
        match unsafe { fork() } {
            Err(e) => {
                eprintln!("kish: fork: {}", e);
                1
            }
            Ok(ForkResult::Child) => {
                let ignored = self.env.traps.ignored_signals();
                self.env.traps.reset_non_ignored();
                signal::reset_child_signals(&ignored);
                let status = self.exec_body(body);
                std::process::exit(status);
            }
            Ok(ForkResult::Parent { child }) => command::wait_child(child),
        }
    }

    fn exec_if(
        &mut self,
        condition: &[CompleteCommand],
        then_part: &[CompleteCommand],
        elif_parts: &[(Vec<CompleteCommand>, Vec<CompleteCommand>)],
        else_part: &Option<Vec<CompleteCommand>>,
    ) -> i32 {
        let cond_status = self.with_errexit_suppressed(|e| e.exec_body(condition));
        if self.env.exec.flow_control.is_some() {
            return cond_status;
        }

        if cond_status == 0 {
            return self.exec_body(then_part);
        }

        for (elif_cond, elif_body) in elif_parts {
            let cond_status = self.with_errexit_suppressed(|e| e.exec_body(elif_cond));
            if self.env.exec.flow_control.is_some() {
                return cond_status;
            }
            if cond_status == 0 {
                return self.exec_body(elif_body);
            }
        }

        if let Some(else_body) = else_part {
            return self.exec_body(else_body);
        }

        0
    }

    /// Execute a while or until loop.
    /// `until=false` → while (run while condition succeeds)
    /// `until=true`  → until (run while condition fails)
    fn exec_loop(
        &mut self,
        condition: &[CompleteCommand],
        body: &[CompleteCommand],
        until: bool,
    ) -> i32 {
        let mut status = 0;
        loop {
            let cond_status = self.with_errexit_suppressed(|e| e.exec_body(condition));
            if self.env.exec.flow_control.is_some() {
                return cond_status;
            }
            let should_run = if until {
                cond_status != 0
            } else {
                cond_status == 0
            };
            if !should_run {
                break;
            }

            status = self.exec_body(body);

            match self.env.exec.flow_control.take() {
                Some(FlowControl::Break(n)) => {
                    if n > 1 {
                        self.env.exec.flow_control = Some(FlowControl::Break(n - 1));
                    }
                    break;
                }
                Some(FlowControl::Continue(n)) => {
                    if n > 1 {
                        self.env.exec.flow_control = Some(FlowControl::Continue(n - 1));
                        break;
                    }
                    // n <= 1: continue this loop (re-evaluate condition)
                }
                Some(other) => {
                    self.env.exec.flow_control = Some(other);
                    break;
                }
                None => {}
            }
        }
        status
    }

    fn exec_for(
        &mut self,
        var: &str,
        words: &Option<Vec<Word>>,
        body: &[CompleteCommand],
    ) -> i32 {
        let items: Vec<String> = match words {
            Some(word_list) => match expand_words(&mut self.env, word_list) {
                Ok(words) => words,
                Err(e) => {
                    eprintln!("{}", e);
                    self.env.exec.last_exit_status = 1;
                    return 1;
                }
            },
            None => self.env.vars.positional_params().to_vec(),
        };

        let mut status = 0;
        for item in &items {
            if let Err(e) = self.env.vars.set(var, item.as_str()) {
                eprintln!("kish: {}", e);
                return 1;
            }

            status = self.exec_body(body);

            match self.env.exec.flow_control.take() {
                Some(FlowControl::Break(n)) => {
                    if n > 1 {
                        self.env.exec.flow_control = Some(FlowControl::Break(n - 1));
                    }
                    break;
                }
                Some(FlowControl::Continue(n)) => {
                    if n > 1 {
                        self.env.exec.flow_control = Some(FlowControl::Continue(n - 1));
                        break;
                    }
                    // n <= 1: continue this loop
                }
                Some(other) => {
                    self.env.exec.flow_control = Some(other);
                    break;
                }
                None => {}
            }
        }
        status
    }

    fn exec_case(&mut self, word: &Word, items: &[CaseItem]) -> i32 {
        let case_word = match crate::expand::expand_word_to_string(&mut self.env, word) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("{}", e);
                self.env.exec.last_exit_status = 1;
                return 1;
            }
        };
        let mut status = 0;
        let mut falling_through = false;

        for item in items {
            if !falling_through {
                let mut matched = false;
                for pattern in &item.patterns {
                    let pat = match crate::expand::expand_word_to_string(&mut self.env, pattern) {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("{}", e);
                            self.env.exec.last_exit_status = 1;
                            return 1;
                        }
                    };
                    if crate::expand::pattern::matches(&pat, &case_word) {
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    continue;
                }
            }

            status = self.exec_body(&item.body);
            if self.env.exec.flow_control.is_some() {
                break;
            }

            match item.terminator {
                CaseTerminator::Break => break,
                CaseTerminator::FallThrough => {
                    falling_through = true;
                }
            }
        }

        status
    }
}
