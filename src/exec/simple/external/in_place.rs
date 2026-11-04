//! Exec-in-place: replacing the current process with an external
//! utility instead of forking it, and the conditions that arm it (a
//! forked subshell whose remaining work is one simple command, an
//! `sh -c` tail call).

use std::path::Path;

use crate::parser::ast::{AndOrList, Command, CompleteCommand, SeparatorOp};
use crate::signal;

use crate::exec::Executor;
use crate::exec::redirect::ExpandedRedirect;

use super::super::cstrings::exec_cstrings_or_report;
use super::flush_std_streams;

impl Executor {
    /// In a forked subshell child whose only remaining work is one
    /// simple command (a foreground pipeline member, `( cmd )`,
    /// `$( cmd )`), arm the one-shot exec-in-place marker so an external
    /// utility replaces this child instead of being spawned as a
    /// grandchild and waited for: one process creation instead of two
    /// per command, as in dash and bash. Builtins, functions and plugin
    /// commands ignore the marker (see `dispatch_simple_command`).
    ///
    /// Not armed while a plugin registers exec hooks: in place, the
    /// command's `post_exec` hook would no longer fire in the child.
    /// The async paths (`exec_async`) arm the marker unconditionally
    /// (documented deviation there).
    pub(crate) fn arm_subshell_exec_in_place(&mut self) {
        if !self.plugins.has_exec_hooks() {
            self.env.exec.async_exec_in_place = true;
        }
    }

    /// [`Self::arm_subshell_exec_in_place`] when `body` — the whole
    /// remaining program of a forked subshell — is a single simple
    /// command: one complete command holding one `;`-terminated (not
    /// `&`) AND-OR list of one non-negated, one-member pipeline. Anything
    /// that runs after the command (a second command, `&&`/`||`, `!`)
    /// keeps the wrapper semantics.
    pub(crate) fn arm_exec_in_place_if_sole_simple(&mut self, body: &[CompleteCommand]) {
        let [cc] = body else { return };
        let [(and_or, sep)] = cc.items.as_slice() else {
            return;
        };
        if !matches!(sep, Some(SeparatorOp::Amp)) && is_sole_simple_command(and_or) {
            self.arm_subshell_exec_in_place();
        }
    }

    /// `sh -c` tail call: `and_or` is the last list of the command string
    /// (see `Executor::mark_shell_tail`). When it is one plain simple
    /// command and nothing could observe the shell after it, arm the
    /// exec-in-place marker so an external utility replaces the shell
    /// instead of being spawned and waited for — one process creation
    /// and wait fewer per `sh -c 'cmd'`, as dash (EV_EXIT) and bash do.
    ///
    /// Kept as a fork (the marker stays clear) when anything would still
    /// run or be reported after the command: any trap — the EXIT action,
    /// a signal action run after the command, even `trap ''` (dash's
    /// `have_traps` is narrower; this is the conservative choice) — any
    /// job in the table, an interactive shell or monitor mode (job
    /// control and the terminal handoff need the shell), `&&`/`||`/`!`
    /// (the shell still computes the status) and every non-simple
    /// command. Builtins, functions and plugin commands ignore the
    /// marker (see `dispatch_simple_command`), and plugins with exec
    /// hooks never get it armed (`arm_subshell_exec_in_place`). As with
    /// dash, the command then *is* the shell process: a parent sees it
    /// die of a signal itself instead of the shell exiting 128+N.
    pub(crate) fn arm_exec_in_place_if_shell_tail(&mut self, and_or: &AndOrList) {
        if is_sole_simple_command(and_or)
            && !self.env.mode.is_interactive
            && !self.env.mode.flag_i
            && !self.env.mode.options.monitor
            // `set -v` still echoes the input after the command (also
            // when set earlier on the same line).
            && !self.env.mode.options.verbose
            && self.env.traps.exit_trap.is_none()
            && self.env.traps.signal_traps.is_empty()
            && self.env.process.jobs.is_empty()
            && self.exit_requested.is_none()
            && self.env.exec.flow_control.is_none()
        {
            self.arm_subshell_exec_in_place();
        }
    }

    /// Exec the resolved external command `path` in the current process
    /// (async exec-in-place, see `exec_async`, or an armed subshell / `sh
    /// -c` tail): this process is already the forked child, so replacing
    /// it makes the parent's job-table entry (or wait) track the real
    /// command. Returns only when the command cannot be exec'd before
    /// the replacement starts (argument conversion failure, a trap
    /// action that asked the shell to exit), with the status to exit
    /// with.
    pub(super) fn exec_resolved_in_place(
        &mut self,
        path: &Path,
        cmd: &str,
        args: &[String],
        env_overrides: &[(String, String)],
        redirects: &[ExpandedRedirect],
    ) -> i32 {
        let (c_cmd, c_args) = match exec_cstrings_or_report(path, cmd, args) {
            Ok(v) => v,
            Err(status) => return status,
        };
        // Same pre-exec dispositions the forked grandchild used to get:
        // everything to SIG_DFL except trap-ignored signals (in the
        // non-monitor async child `ignored` already carries the
        // SIGINT/SIGQUIT entries exec_async inserted). The explicit reset
        // matters for SIG_IGN dispositions, which survive exec — notably
        // the background shell child's SIGTTIN ignore: the exec'd command
        // must be stoppable so the now correctly tracked stop is visible.
        let ignored = self.env.traps.ignored_signals();
        signal::reset_child_dispositions(&ignored);
        // A signal that arrived before the reset (during the command's
        // expansion: `sh -c 'cmd $(kill $$)'`) waits on the self-pipe,
        // which the exec would drop: act on it now, as the shell would
        // after the command. Later ones meet SIG_DFL. Then close the pipe
        // (see `reset_child_signals`).
        self.process_pending_signals();
        if let Some(code) = self.exit_requested {
            return code;
        }
        signal::close_self_pipe();
        // Output the shell buffered (`printf x; cmd` as an `sh -c` tail)
        // must precede the command's.
        flush_std_streams();
        self.exec_child_prepared(&c_cmd, &c_args, cmd, env_overrides, redirects);
    }
}

/// `and_or` is one non-negated, one-member pipeline whose member is a
/// simple command — nothing runs after that command inside the list.
fn is_sole_simple_command(and_or: &AndOrList) -> bool {
    and_or.rest.is_empty()
        && !and_or.first.negated
        && matches!(and_or.first.commands.as_slice(), [Command::Simple(_)])
}
