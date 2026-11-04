//! Leaving the REPL: per-command flow-control cleanup, the final signal
//! drain and EXIT trap, saving the history, and dying of a fatal signal.

use super::Repl;
use super::history;

impl Repl {
    /// Finish a command (or trap action) run at the top level of the
    /// REPL: any flow-control signal still pending here (ExpansionError
    /// from a nounset / `${x:?}` failure, or a stray return/break/
    /// continue) has aborted that command and must not leak into the
    /// next one — otherwise the next typed command is silently skipped.
    /// Every top-level run goes through here: typed commands, trap
    /// actions for signals drained between commands, and the INT trap
    /// fired by Ctrl+C at the prompt.
    pub(super) fn end_top_level_command(&mut self) {
        self.executor.env.exec.flow_control = None;
    }

    /// Wind the shell down once the REPL loop has ended. Returns the exit
    /// status, or does not return when an untrapped signal ends the shell.
    pub(super) fn shut_down(&mut self) -> i32 {
        // A signal that ended the read (SIGHUP at the prompt) is handled
        // here: its trap action, or the default action, which runs the
        // EXIT trap and marks the shell to die of the signal below.
        self.executor.process_pending_signals();
        self.end_top_level_command();
        match self.executor.exit_requested {
            Some(code) => self.executor.env.exec.last_exit_status = code,
            None => self.executor.execute_exit_trap(),
        }

        self.save_history();

        // An untrapped fatal signal (SIGHUP at the prompt, `kill -USR1
        // $$`) ends the shell by that signal, so the parent sees a
        // signal death, as with dash and bash — not an exit with 128+N.
        if let Some(sig) = self.executor.exit_signal {
            crate::exec::shell_exit_by_signal(sig);
        }
        self.executor.env.exec.last_exit_status
    }

    /// Save the history to `$HISTFILE` (if set), keeping at most
    /// `$HISTFILESIZE` entries; a failure is only a warning.
    fn save_history(&mut self) {
        let histfile = self
            .executor
            .env
            .vars
            .get("HISTFILE")
            .unwrap_or("")
            .to_string();
        let histfilesize = history::histfilesize_setting(&self.executor.env.vars);
        if !histfile.is_empty()
            && let Err(e) = self
                .executor
                .env
                .history
                .save(std::path::Path::new(&histfile), histfilesize)
        {
            eprintln!("yosh: warning: cannot save history to {}: {}", histfile, e);
        }
    }
}
