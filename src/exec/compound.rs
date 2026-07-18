use nix::unistd::{ForkResult, fork};

use crate::env::FlowControl;
use crate::error::{RuntimeErrorKind, ShellError};
use crate::expand::expand_words;
use crate::parser::ast::{
    CaseItem, CaseTerminator, CompleteCommand, CompoundCommand, Redirect, Word,
};
use crate::signal;

use super::Executor;
use super::command;
use super::redirect::RedirectState;

impl Executor {
    /// Execute a compound command, applying any redirects around it.
    pub(crate) fn exec_compound_command(
        &mut self,
        compound: &CompoundCommand,
        redirects: &[Redirect],
    ) -> Result<i32, ShellError> {
        self.env.exec.lineno = compound.line;

        let saved = self
            .apply_temp_assignments(&compound.assignments)
            .inspect_err(|_| {
                self.env.exec.last_exit_status = 1;
            })?;

        let mut redirect_state = RedirectState::new();
        if let Err(e) = redirect_state.apply(redirects, &mut self.env, true) {
            self.restore_assignments(saved);
            self.env.exec.last_exit_status = 1;
            return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
        }

        let status = match &compound.kind {
            crate::parser::ast::CompoundCommandKind::BraceGroup { body } => {
                self.exec_brace_group(body)
            }
            crate::parser::ast::CompoundCommandKind::Subshell { body } => {
                self.exec_subshell(body)?
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
                self.exec_for(var, words, body)?
            }
            crate::parser::ast::CompoundCommandKind::Case { word, items } => {
                self.exec_case(word, items)?
            }
        };

        redirect_state.restore();
        self.restore_assignments(saved);
        self.env.exec.last_exit_status = status;
        Ok(status)
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
            if self.exit_requested.is_some() {
                break;
            }
            self.process_pending_signals();
            if self.exit_requested.is_some() {
                break;
            }
        }
        status
    }

    fn exec_brace_group(&mut self, body: &[CompleteCommand]) -> i32 {
        self.exec_body(body)
    }

    fn exec_subshell(&mut self, body: &[CompleteCommand]) -> Result<i32, ShellError> {
        match unsafe { fork() } {
            Err(e) => Err(ShellError::runtime(
                RuntimeErrorKind::IoError,
                format!("fork: {}", e),
            )),
            Ok(ForkResult::Child) => {
                let ignored = self.env.traps.ignored_signals();
                self.env.traps.reset_for_subshell();
                signal::reset_child_signals(&ignored);
                let status = self.exec_body(body);
                // POSIX §2.12: EXIT pseudo-signal handler runs on shell exit,
                // including subshell exit. Fire BEFORE _exit so the action runs
                // in the child's environment.
                self.execute_exit_trap();
                super::exit_child(status);
            }
            Ok(ForkResult::Parent { child }) => Ok(command::wait_child(child).unwrap_or(1)),
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
    /// Run `f` with `loop_depth` incremented, restoring the counter on
    /// exit even if `f` unwinds (panics inside a function body are caught
    /// by `exec_function_call`'s `catch_unwind`, so a skipped decrement
    /// would otherwise leak into subsequent commands).
    fn with_loop_depth<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        struct Guard<'a>(&'a mut Executor);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.env.exec.loop_depth -= 1;
            }
        }
        self.env.exec.loop_depth += 1;
        let guard = Guard(self);
        f(&mut *guard.0)
    }

    fn exec_loop(
        &mut self,
        condition: &[CompleteCommand],
        body: &[CompleteCommand],
        until: bool,
    ) -> i32 {
        self.with_loop_depth(|e| e.exec_loop_inner(condition, body, until))
    }

    fn exec_loop_inner(
        &mut self,
        condition: &[CompleteCommand],
        body: &[CompleteCommand],
        until: bool,
    ) -> i32 {
        let mut status = 0;
        loop {
            if self.exit_requested.is_some() {
                break;
            }
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
    ) -> Result<i32, ShellError> {
        self.with_loop_depth(|e| e.exec_for_inner(var, words, body))
    }

    fn exec_for_inner(
        &mut self,
        var: &str,
        words: &Option<Vec<Word>>,
        body: &[CompleteCommand],
    ) -> Result<i32, ShellError> {
        let items: Vec<String> = match words {
            Some(word_list) => match expand_words(&mut self.env, word_list) {
                Ok(words) => words,
                Err(e) => {
                    self.env.exec.last_exit_status = 1;
                    return Err(e);
                }
            },
            None => self.env.vars.positional_params().to_vec(),
        };

        let mut status = 0;
        for item in &items {
            if self.exit_requested.is_some() {
                break;
            }
            // assign_var (not vars.set): a `for PATH in ...` loop must
            // invalidate the utility hash on each iteration.
            if let Err(e) = self.env.assign_var(var, item.as_str()) {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::ReadonlyVariable,
                    e.to_string(),
                ));
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
        Ok(status)
    }

    fn exec_case(&mut self, word: &Word, items: &[CaseItem]) -> Result<i32, ShellError> {
        let case_word = match crate::expand::expand_word_to_string(&mut self.env, word) {
            Ok(w) => w,
            Err(e) => {
                self.env.exec.last_exit_status = 1;
                return Err(e);
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
                            self.env.exec.last_exit_status = 1;
                            return Err(e);
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

        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use crate::exec::Executor;
    use crate::parser::Parser;

    #[test]
    fn compound_with_assignment_prefix_runs_inside_temp_scope() {
        let source = "y=initial\nx=replaced if true; then echo $x; fi\necho post=$x";
        let prog = Parser::new(source).parse_program().unwrap();
        let mut exec = Executor::new("yosh", vec![]);
        // Capture stdout by running in-process; for simplicity rely on
        // env after execution to verify scope behavior.
        exec.exec_program(&prog);
        // The temp assignment must NOT persist past the compound.
        assert_eq!(
            exec.env.vars.get("x"),
            None,
            "x must not leak past compound"
        );
        // The earlier permanent assignment must remain.
        assert_eq!(exec.env.vars.get("y"), Some("initial"));
    }

    #[test]
    fn for_loop_variable_named_path_clears_utility_hash() {
        let prog = Parser::new("for PATH in /only_entry; do :; done")
            .parse_program()
            .unwrap();
        let mut exec = Executor::new("yosh", vec![]);
        exec.env.utility_hash.insert(
            "foo".to_string(),
            crate::env::HashEntry::new(std::path::PathBuf::from("/bin/foo")),
        );
        exec.exec_program(&prog);
        assert!(
            exec.env.utility_hash.is_empty(),
            "`for PATH in ...` must invalidate the utility hash"
        );
        assert_eq!(exec.env.vars.get("PATH"), Some("/only_entry"));
    }
}
