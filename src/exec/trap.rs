//! Trap actions (POSIX §2.12): the EXIT trap, signal traps, and the
//! default action of untrapped signals.

use crate::env::{FlowControl, TrapAction};
use crate::signal;

use super::{Executor, shell_exit};

impl Executor {
    /// Execute a trap action string, exposing the pre-trap `$?` to the
    /// `exit` special built-in via `trap_context_status` (POSIX §2.12:
    /// `exit` without an operand inside a trap action uses the value `$?`
    /// had when the trap action started).
    ///
    /// Used by the EXIT-trap path; signal traps go through
    /// [`Self::run_signal_trap_action`] instead. `set -e` applies inside
    /// the action like anywhere else (`set -e; trap 'false; echo t' EXIT`
    /// prints nothing and exits 1 — dash and bash agree), see
    /// [`Self::with_errexit_context_reset`].
    pub(crate) fn run_trap_action(&mut self, cmd: &str) {
        let prev = self
            .env
            .exec
            .trap_context_status
            .replace(self.env.exec.last_exit_status);
        self.with_errexit_context_reset(|exec| exec.eval_trap_text(cmd));
        self.env.exec.trap_context_status = prev;
    }

    /// Run a trap action's text as a trap action: the recursion guard
    /// lets it use half of its stack reserve. A trap fires at a command
    /// boundary — `exit` deep in a recursion that the guard accepted
    /// level by level, a signal arriving there — where the remaining
    /// stack can be just under the reserve the guard demands of one more
    /// level; refusing the action there would skip an EXIT trap's
    /// cleanup and change the exit status (`trap 'echo t' EXIT; f() { if
    /// [ "$1" -gt 0 ]; then f $(($1-1)); else exit 7; fi; }; f 413`
    /// prints t and exits 7 in dash and bash). The half reserve still
    /// covers a command's execution, and descent-checked parsing,
    /// arithmetic and expansion fail gracefully below it, so the action
    /// itself — including functions it calls and the command
    /// substitutions and subshells it forks, held to the same half
    /// reserve — cannot overflow. Trap actions nested by signals that
    /// keep firing inside the action stop at that same bound.
    fn eval_trap_text(&mut self, cmd: &str) {
        super::stack_guard::as_trap_action(|| {
            if !self.recursion_too_deep("trap") {
                self.exec_text_reporting_errors(cmd);
            }
        });
    }

    /// Run `f` (a trap action) as a fresh errexit context: an action that
    /// fires while the interrupted code was errexit-suppressed (`f ||
    /// true`, an `if` condition) is not itself suppressed — the
    /// suppression belongs to the interrupted command list, not to the
    /// trap action (dash and bash agree).
    fn with_errexit_context_reset<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let saved_depth = std::mem::take(&mut self.errexit_suppressed_depth);
        let saved_exempt = std::mem::take(&mut self.errexit_exempt_status);
        let result = f(self);
        self.errexit_suppressed_depth = saved_depth;
        self.errexit_exempt_status = saved_exempt;
        result
    }

    /// Execute a SIGNAL trap action (POSIX §2.12). Differences from the
    /// EXIT-trap path ([`Self::run_trap_action`]):
    ///
    /// - On completion, `$?` is restored to its pre-trap value so the
    ///   interrupted command sequence observes its own status, not the
    ///   trap action's (`trap 'false' USR1; kill -USR1 $$; echo $?`
    ///   prints 0 — bash/dash/zsh agree). The restore is skipped when
    ///   the action itself requested an exit (`exit` builtin, errexit,
    ///   fatal error) or flow control (`break`/`continue`), whose
    ///   resulting status must survive.
    /// - errexit is NOT suppressed, even when the signal arrives while
    ///   the interrupted code is (`f || true`): `set -e; trap 'false'
    ///   USR1; kill -USR1 $$; echo ok` exits 1 without printing ok
    ///   (bash/dash/zsh agree).
    ///
    /// The pre-trap `$?` is still exposed to `exit` without an operand
    /// via `trap_context_status`.
    fn run_signal_trap_action(&mut self, cmd: &str) {
        let saved_status = self.env.exec.last_exit_status;
        let prev = self.env.exec.trap_context_status.replace(saved_status);
        self.with_errexit_context_reset(|exec| exec.eval_trap_text(cmd));
        self.env.exec.trap_context_status = prev;
        if self.exit_requested.is_none() && self.env.exec.flow_control.is_none() {
            self.env.exec.last_exit_status = saved_status;
        }
    }

    /// Exit the whole shell process from the shell parent (NOT a forked
    /// child — post-fork children must use [`exit_child`](super::exit_child)).
    ///
    /// Fires the EXIT trap first (POSIX §2.12: the EXIT trap runs on any
    /// shell exit, including a fatal error such as a special-builtin
    /// redirection failure in a non-interactive shell — dash agrees),
    /// then exits via `std::process::exit`, matching the exit path used
    /// by `check_errexit` and `handle_default_signal`.
    pub(crate) fn exit_shell(&mut self, status: i32) -> ! {
        self.execute_exit_trap();
        shell_exit(status);
    }

    /// Execute the EXIT trap if set.
    ///
    /// A pending exit request (the fatal error that is ending the shell,
    /// e.g. `unset -Z x`) is set aside while the action runs, so it does
    /// not cut the action short: `trap 'echo a; echo b' EXIT; unset -Z x`
    /// prints both lines (dash and bash agree). A fatal error raised by the
    /// action itself ends a non-interactive shell with that error's status
    /// (`trap 'unset -Z x' EXIT; true` exits 2; a fatal expansion error
    /// such as `trap ': ${x:?}' EXIT` exits 1). Callers that exit right
    /// after this (forked children via `child_exit_status`, the top-level
    /// shell) therefore never report the pre-trap status over it.
    pub fn execute_exit_trap(&mut self) {
        if let Some(TrapAction::Command(cmd)) = self.env.traps.exit_trap.take() {
            // The shell is terminating, possibly from inside redirected
            // compound commands or functions (`{ exit; } >f`, `set -e`
            // failing in `f >f`): unwind those redirections first so the
            // action writes to the shell's own fds, as dash and bash do.
            super::redirect::restore_all_saved_redirections();
            let pending = self.exit_requested.take();
            // The action sees the status the shell is exiting with as
            // `$?`, not whatever a surrounding conditional left behind
            // after the fatal error was deferred (`if r=2 true; then :;
            // fi` with `r` readonly: `trap 'echo $?' EXIT` prints 1).
            if let Some(code) = pending {
                self.env.exec.last_exit_status = code;
            }
            // A fatal expansion error that is ending the shell is set
            // aside the same way, or it would cancel the action's first
            // command (`trap 'echo t' EXIT; : ${x:?}` prints t).
            let pending_flow = self
                .env
                .exec
                .flow_control
                .take_if(|f| *f == FlowControl::ExpansionError);
            self.run_trap_action(&cmd);
            match self.exit_requested {
                Some(code) if !self.env.mode.is_interactive => shell_exit(code),
                Some(_) => {}
                None => self.exit_requested = pending,
            }
            // A fatal expansion error raised by the action itself
            // (`trap ': ${x:?}' EXIT`) ends a non-interactive shell — or
            // the subshell / command substitution whose exit fired the
            // trap — with the expansion-error status, overriding the
            // status it was about to exit with (POSIX §2.8.1; dash exits
            // 2 here, and so does `out=$(trap ': ${x:?}' EXIT; :)`).
            if !self.env.mode.is_interactive && self.expansion_error_pending() {
                shell_exit(1);
            }
            if self.env.exec.flow_control.is_none() {
                self.env.exec.flow_control = pending_flow;
            }
        }
    }

    /// Process any pending signals from the self-pipe.
    pub fn process_pending_signals(&mut self) {
        let signals = signal::drain_pending_signals();
        self.run_signal_traps(&signals);
    }

    /// Run the trap/default action for each already-drained signal.
    /// Split from `process_pending_signals` so callers that drained the
    /// self-pipe themselves (e.g. `wait`) can still fire the trap actions.
    pub(crate) fn run_signal_traps(&mut self, signals: &[i32]) {
        for &sig in signals {
            // SIGCHLD default action is to ignore (just reap children).
            // We must not route it through handle_default_signal which
            // exits the shell.  Reaping is already handled by
            // reap_zombies() in the interactive loop.
            if sig == libc::SIGCHLD {
                // Default and Ignore: just ignore SIGCHLD (reaping is done
                // elsewhere). Only the user-installed `Command` trap runs.
                if let Some(TrapAction::Command(cmd)) = self.env.traps.get_signal_trap(sig).cloned()
                {
                    self.run_signal_trap_action(&cmd);
                }
                continue;
            }

            match self.env.traps.get_signal_trap(sig).cloned() {
                Some(TrapAction::Command(cmd)) => {
                    self.run_signal_trap_action(&cmd);
                }
                Some(TrapAction::Ignore) => {}
                Some(TrapAction::Default) | None => {
                    self.handle_default_signal(sig);
                }
            }
        }
    }

    /// Handle a signal with default behavior: the signal's `SIG_DFL`
    /// action (see [`signal::default_action`]), which for most signals
    /// terminates the shell.
    pub(crate) fn handle_default_signal(&mut self, sig: i32) {
        match signal::default_action(sig) {
            // A signal whose default is to be discarded (WINCH, URG, ...)
            // never ends the shell, e.g. when a second delivery was
            // queued before the trap reset itself with `trap - WINCH`.
            signal::DefaultAction::Ignore => return,
            // Re-raise so the current disposition decides: SIG_DFL stops
            // the shell like an untrapped delivery would; monitor mode
            // keeps these ignored so the shell itself is never stopped.
            signal::DefaultAction::Stop => {
                // SAFETY: raise is async-signal-safe and only delivers
                // `sig` to this process.
                unsafe { libc::raise(sig) };
                return;
            }
            signal::DefaultAction::Terminate => {}
        }
        // POSIX sh: an interactive shell ignores untrapped SIGQUIT and
        // SIGTERM, and untrapped SIGINT only discards the current line —
        // none of the three may terminate the shell (bash/dash survive
        // `kill -INT/-QUIT/-TERM $$` silently). The EXIT trap must not
        // run either, since the shell is not exiting.
        if self.env.mode.is_interactive
            && matches!(sig, libc::SIGINT | libc::SIGQUIT | libc::SIGTERM)
        {
            return;
        }
        self.execute_exit_trap();
        if self.env.mode.is_interactive {
            self.exit_requested = Some(128 + sig);
        } else {
            shell_exit(128 + sig);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_default_signal_sets_exit_requested_in_interactive_mode() {
        let mut exec = Executor::new("yosh", vec![]);
        exec.env.mode.is_interactive = true;
        exec.handle_default_signal(libc::SIGHUP);
        assert_eq!(exec.exit_requested, Some(128 + libc::SIGHUP));
    }

    #[test]
    fn handle_default_signal_ignores_term_quit_int_in_interactive_mode() {
        // POSIX sh: interactive shells ignore untrapped SIGTERM/SIGQUIT,
        // and untrapped SIGINT must not exit the shell either.
        for sig in [libc::SIGTERM, libc::SIGQUIT, libc::SIGINT] {
            let mut exec = Executor::new("yosh", vec![]);
            exec.env.mode.is_interactive = true;
            exec.handle_default_signal(sig);
            assert_eq!(
                exec.exit_requested, None,
                "interactive shell must not exit on signal {sig}"
            );
        }
    }

    #[test]
    fn handle_default_signal_ignore_does_not_consume_exit_trap() {
        // The ignored-signal early return must not fire (or take) the
        // EXIT trap — the shell is not exiting.
        let mut exec = Executor::new("yosh", vec![]);
        exec.env.mode.is_interactive = true;
        exec.env
            .traps
            .set_trap("EXIT", TrapAction::Command("x=exited".into()))
            .unwrap();
        exec.handle_default_signal(libc::SIGTERM);
        assert_eq!(exec.env.vars.get("x"), None);
        assert!(exec.env.traps.exit_trap.is_some());
    }

    #[test]
    fn handle_default_signal_ignores_signals_whose_default_is_ignore() {
        // WINCH/URG/CONT are discarded at SIG_DFL: emulating the default
        // action must not end a non-interactive shell or fire EXIT.
        let mut exec = Executor::new("yosh", vec![]);
        exec.env
            .traps
            .set_trap("EXIT", TrapAction::Command("x=exited".into()))
            .unwrap();
        for sig in [libc::SIGWINCH, libc::SIGURG, libc::SIGCONT, libc::SIGCHLD] {
            assert_eq!(
                signal::default_action(sig),
                signal::DefaultAction::Ignore,
                "{sig}"
            );
            exec.handle_default_signal(sig);
        }
        assert_eq!(exec.env.vars.get("x"), None);
        assert_eq!(exec.exit_requested, None);
        assert_eq!(
            signal::default_action(libc::SIGTERM),
            signal::DefaultAction::Terminate
        );
        assert_eq!(
            signal::default_action(libc::SIGTSTP),
            signal::DefaultAction::Stop
        );
    }

    #[test]
    fn run_signal_traps_runs_term_trap_in_interactive_mode() {
        // A user-installed trap on TERM still fires in an interactive
        // shell — only the *untrapped* default is ignored.
        let mut exec = Executor::new("yosh", vec![]);
        exec.env.mode.is_interactive = true;
        exec.env
            .traps
            .set_trap("TERM", TrapAction::Command("x=trapped".into()))
            .unwrap();
        exec.run_signal_traps(&[libc::SIGTERM]);
        assert_eq!(exec.env.vars.get("x"), Some("trapped"));
        assert_eq!(exec.exit_requested, None);
    }
}
