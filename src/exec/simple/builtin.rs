//! Built-in dispatch for a simple command: special built-ins (persistent
//! prefix assignments, fatal errors), regular built-ins, and the regular
//! built-ins implemented on the Executor itself.

use crate::builtin::exec_regular_builtin;
use crate::builtin::special::run_special_builtin;
use crate::error::ShellError;

use super::super::Executor;
use super::super::redirect::RedirectState;
use super::Invocation;

/// A regular built-in implemented on the Executor (job table / signal
/// access) rather than in `exec_regular_builtin`.
pub(super) type HostedBuiltin = fn(&mut Executor, &[String]) -> i32;

/// The Executor-hosted regular built-ins: wait / fg / bg / jobs / command
/// need Executor access (job table, signal processing, -p / no-flag
/// execution paths); fc needs it to re-execute history entries. `None`
/// when `name` is not one of them. A built-in error is printed and turned
/// into its exit status.
pub(super) fn executor_hosted_builtin(name: &str) -> Option<HostedBuiltin> {
    fn report(result: Result<i32, ShellError>) -> i32 {
        result.unwrap_or_else(|e| {
            eprintln!("{}", e);
            e.exit_code()
        })
    }
    let builtin: HostedBuiltin = match name {
        "wait" => |exec, args| report(exec.builtin_wait(args)),
        "fg" => |exec, args| report(exec.builtin_fg(args)),
        "bg" => |exec, args| report(exec.builtin_bg(args)),
        "jobs" => |exec, args| report(exec.builtin_jobs(args)),
        "fc" => |exec, args| report(crate::builtin::fc::builtin_fc(args, exec)),
        "command" => Executor::builtin_command,
        _ => return None,
    };
    Some(builtin)
}

impl Executor {
    /// Run a special built-in. Its prefix assignments persist in the
    /// shell (POSIX §2.9.1), and a redirection error or utility error is
    /// fatal in a non-interactive shell (POSIX §2.8.1).
    pub(super) fn exec_special_builtin_command(
        &mut self,
        inv: &Invocation,
    ) -> Result<i32, ShellError> {
        // `exec` with operands replaces the shell; without them, its
        // redirections persist instead of being undone afterwards.
        let replaces_shell = inv.name == "exec" && !inv.args.is_empty();
        let persist_redirects = inv.name == "exec" && inv.args.is_empty();

        let mut assigned = Vec::with_capacity(inv.assignments.len());
        for assignment in inv.assignments {
            let value = self.expand_assignment(assignment)?;
            self.assign_persistent(&assignment.name, value.clone())?;
            // `VAR=x exec cmd`: the assignment belongs in the
            // exec'd program's environment (dash/bash/ksh agree);
            // it also persists in the shell per the special
            // built-in rule, which is moot once exec succeeds.
            if replaces_shell {
                self.env.vars.export(&assignment.name);
            }
            assigned.push((assignment.name.clone(), value));
        }
        self.xtrace_simple(&assigned, inv.name, inv.args);

        let mut redirect_state = RedirectState::new();
        if let Err(e) = redirect_state.apply_expanded(inv.redirs, !persist_redirects) {
            return Err(self.special_builtin_redirect_error(e));
        }
        if persist_redirects {
            self.env.exec.last_exit_status = 0;
            return Ok(0);
        }

        // `args` for export/readonly was already produced by
        // expand_assignment_builtin_args in the early dispatch
        // (single-pass expansion avoids double-running command
        // substitutions in assignment RHS).
        let outcome = run_special_builtin(inv.name, inv.args, self);
        redirect_state.restore();
        let status = outcome.status;
        // POSIX §2.8.1: a special built-in utility error makes a
        // non-interactive shell exit (dash and bash --posix agree).
        // Only this direct dispatch requests the exit — `command`
        // strips the special properties (`command set -Z` must not
        // exit), and it reaches the builtin via another path.
        if outcome.utility_error && !self.env.mode.is_interactive {
            self.exit_requested = Some(status);
        }
        Ok(self.finish_simple_command(inv.hook_cmd, status))
    }

    /// A redirection error on a special built-in: POSIX §2.8.1 makes it
    /// fatal in a non-interactive shell, which exits with status 1 (after
    /// the EXIT trap). Otherwise `$?` becomes 1 and `e` is returned for
    /// the caller to report.
    pub(super) fn special_builtin_redirect_error(&mut self, e: ShellError) -> ShellError {
        self.env.exec.last_exit_status = 1;
        if !self.env.mode.is_interactive {
            if !e.message.is_empty() {
                eprintln!("{}", e);
            }
            self.exit_shell(1);
        }
        e
    }

    /// Run a regular built-in with its prefix assignments applied
    /// temporarily, firing the plugins' on-cd hook after a successful `cd`.
    pub(super) fn exec_regular_builtin_command(
        &mut self,
        inv: &Invocation,
    ) -> Result<i32, ShellError> {
        let old_pwd = if inv.name == "cd" {
            self.env.vars.get("PWD").map(|s| s.to_string())
        } else {
            None
        };
        let status = self.run_with_temp_assignments(inv, |exec| {
            exec_regular_builtin(inv.name, inv.args, &mut exec.env)
        })?;
        self.plugins
            .call_post_exec(&mut self.env, inv.hook_cmd, status);

        // on_cd hook: fire if cd succeeded (before `$?` is set, like
        // the post-exec hook).
        if inv.name == "cd" && status == 0 {
            let old = old_pwd.unwrap_or_default();
            let new_dir = self.env.vars.get("PWD").unwrap_or("").to_string();
            self.plugins.call_on_cd(&mut self.env, &old, &new_dir);
        }

        self.env.exec.last_exit_status = status;
        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    /// POSIX §2.8.1: a special built-in utility error exits only a
    /// NON-interactive shell, and never when reached through `command`.
    #[test]
    fn special_builtin_error_requests_exit_only_when_noninteractive() {
        use crate::exec::Executor;
        use crate::parser::Parser;

        let run = |src: &str, interactive: bool| {
            let mut exec = Executor::new("yosh", vec![]);
            exec.env.mode.is_interactive = interactive;
            let prog = Parser::new(src).parse_program().unwrap();
            exec.exec_program(&prog);
            (exec.exit_requested, exec.env.vars.get("after").is_some())
        };
        assert_eq!(run("unset -Z x; after=1", false), (Some(2), false));
        assert_eq!(run("unset -Z x; after=1", true), (None, true));
        assert_eq!(run("command unset -Z x; after=1", false), (None, true));
        assert_eq!(
            run("readonly r=1; export r=2; after=1", false),
            (Some(1), false)
        );
        // A nonzero status that is not an error (`eval false`) does not exit.
        assert_eq!(run("eval false; after=1", false), (None, true));
    }
}
