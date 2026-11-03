//! Prefix assignments (`name=value cmd`): expansion, persistent and
//! temporary application, and restoration.

use crate::env::vars::Variable;
use crate::error::{RuntimeErrorKind, ShellError};
use crate::parser::ast::Assignment;

use super::super::Executor;
use super::super::redirect::RedirectState;
use super::Invocation;

/// Variables saved before temporary prefix assignments, as `(name, the
/// whole variable or None if it did not exist)`, for
/// [`Executor::restore_assignments`].
pub(crate) type SavedVars = Vec<(String, Option<Variable>)>;

impl Executor {
    /// Expand one prefix assignment's value (empty for `name=`). A
    /// braced-parameter failure (`${x:?}`, `set -u`) only raises
    /// `FlowControl::ExpansionError` while returning `Ok`; turn it into
    /// an error so the command is not run (POSIX §2.8.1 expansion error).
    pub(super) fn expand_assignment(
        &mut self,
        assignment: &Assignment,
    ) -> Result<String, ShellError> {
        let Some(w) = &assignment.value else {
            return Ok(String::new());
        };
        let value = crate::expand::expand_word_to_string(&mut self.env, w)?;
        if self.expansion_error_pending() {
            return Err(ShellError::expansion_already_reported());
        }
        Ok(value)
    }

    /// Assign `name=value` persistently, as an assignment-only command or
    /// a special built-in's prefix assignment does (POSIX §2.9.1).
    /// `assign_var_with_options` (not `vars.set_with_options`): a `PATH`
    /// assignment must invalidate the utility hash (POSIX §2.5.3). A
    /// readonly variable makes this a variable-assignment error.
    pub(super) fn assign_persistent(
        &mut self,
        name: &str,
        value: String,
    ) -> Result<(), ShellError> {
        let allexport = self.env.mode.options.allexport;
        self.env
            .assign_var_with_options(name, value, allexport)
            .map_err(|e| ShellError::runtime(RuntimeErrorKind::ReadonlyVariable, e.to_string()))
    }

    /// Run `run` with the command's prefix assignments applied
    /// temporarily and its (already expanded) redirections applied,
    /// restoring both afterwards. Shared by the function, regular
    /// built-in and Executor-hosted built-in dispatches.
    pub(super) fn run_with_temp_assignments(
        &mut self,
        inv: &Invocation,
        run: impl FnOnce(&mut Self) -> i32,
    ) -> Result<i32, ShellError> {
        let (saved, assigned) = self.apply_temp_assignments_with_values(inv.assignments)?;
        self.xtrace_simple(&assigned, inv.name, inv.args);
        let mut redirect_state = RedirectState::new();
        if let Err(e) = redirect_state.apply_expanded(inv.redirs, true) {
            self.restore_assignments(saved);
            return Err(e);
        }
        let status = run(self);
        redirect_state.restore();
        self.restore_assignments(saved);
        Ok(status)
    }

    /// Expand command-specific prefix assignments to (name, value) pairs.
    ///
    /// This does NOT merge with the exported environ — that merge happens
    /// just before `execv` in `exec_external_with_redirects`, so the
    /// common case (no prefix assignments on the command) never clones
    /// `environ()` at all. When there are assignments, they are applied
    /// as overrides on top of `environ()` (last-wins, matching the
    /// previous `iter_mut().find()` replace-or-push behavior).
    ///
    /// A readonly variable cannot be overridden even for one command:
    /// that is a variable-assignment error (POSIX §2.8.1; dash refuses
    /// `readonly r=1; r=2 cmd` and exits a non-interactive shell).
    pub(super) fn build_env_vars(
        &mut self,
        assignments: &[Assignment],
    ) -> crate::error::Result<Vec<(String, String)>> {
        let mut vars = Vec::with_capacity(assignments.len());
        for assign in assignments {
            let value = self.expand_assignment(assign)?;
            if self.env.vars.is_readonly(&assign.name) {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::ReadonlyVariable,
                    format!("{}: readonly variable", assign.name),
                ));
            }
            vars.push((assign.name.clone(), value));
        }
        Ok(vars)
    }

    /// Apply prefix assignments temporarily, returning saved values for later restoration.
    pub(crate) fn apply_temp_assignments(
        &mut self,
        assignments: &[Assignment],
    ) -> crate::error::Result<SavedVars> {
        self.apply_temp_assignments_with_values(assignments)
            .map(|(saved, _)| saved)
    }

    /// [`Self::apply_temp_assignments`], also returning the assigned
    /// (name, value) pairs (for the `set -x` trace).
    ///
    /// On error every assignment applied so far is rolled back. A
    /// readonly variable rejecting the temporary assignment is a
    /// variable-assignment error (POSIX §2.8.1: the command is not run
    /// and a non-interactive shell exits — dash agrees).
    fn apply_temp_assignments_with_values(
        &mut self,
        assignments: &[Assignment],
    ) -> crate::error::Result<(SavedVars, Vec<(String, String)>)> {
        let mut saved = Vec::with_capacity(assignments.len());
        let mut assigned = Vec::with_capacity(assignments.len());
        for assignment in assignments {
            if let Err(e) = self.apply_temp_assignment(assignment, &mut saved, &mut assigned) {
                self.restore_assignments(saved);
                return Err(e);
            }
        }
        Ok((saved, assigned))
    }

    /// Apply one temporary prefix assignment, recording the variable's
    /// previous state in `saved` and the assigned pair in `assigned`.
    fn apply_temp_assignment(
        &mut self,
        assignment: &Assignment,
        saved: &mut SavedVars,
        assigned: &mut Vec<(String, String)>,
    ) -> Result<(), ShellError> {
        let value = self.expand_assignment(assignment)?;
        // The whole variable, not just its value: an exported name
        // without a value (`export x` while unset) must get its
        // export attribute back (dash/bash agree).
        let old_val = self.env.vars.get_var(&assignment.name).cloned();
        self.env
            .assign_var(&assignment.name, value.clone())
            .map_err(|e| ShellError::runtime(RuntimeErrorKind::ReadonlyVariable, e.to_string()))?;
        saved.push((assignment.name.clone(), old_val));
        assigned.push((assignment.name.clone(), value));
        Ok(())
    }

    /// Restore variables to their pre-assignment values. Restores in
    /// reverse order so `a=1 a=2 cmd` ends with `a`'s original value.
    pub(crate) fn restore_assignments(&mut self, saved: SavedVars) {
        for (name, old_var) in saved.into_iter().rev() {
            match old_var {
                Some(Variable {
                    value: Some(val), ..
                }) => {
                    let _ = self.env.assign_var(&name, val);
                }
                Some(Variable {
                    value: None,
                    exported,
                    ..
                }) => {
                    let _ = self.env.unset_var(&name);
                    if exported {
                        self.env.vars.export(&name);
                    }
                }
                None => {
                    let _ = self.env.unset_var(&name);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// Temporary prefix assignments are rolled back in reverse order, so a
    /// duplicated name ends with its original value.
    #[test]
    fn duplicate_temp_assignment_restores_original_value() {
        use crate::exec::Executor;
        use crate::parser::Parser;

        let mut exec = Executor::new("yosh", vec![]);
        let prog = Parser::new("a=orig; f() { :; }; a=1 a=2 f")
            .parse_program()
            .unwrap();
        exec.exec_program(&prog);
        assert_eq!(exec.env.vars.get("a"), Some("orig"));
    }
}
