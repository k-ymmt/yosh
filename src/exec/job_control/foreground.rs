//! Waiting for a foreground job to exit or stop, and the termios
//! bookkeeping around foreground stops.

use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};

use super::super::Executor;
use crate::env::jobs::JobStatus;
use crate::env::{FlowControl, TrapAction};

/// Result of waiting for a foreground job.
pub(in crate::exec) struct ForegroundWaitResult {
    /// Exit status of the last process to report.
    pub(in crate::exec) last_status: i32,
    /// Per-process exit statuses (pid, exit_code) in reporting order — used by pipefail.
    pub(in crate::exec) process_statuses: Vec<(nix::unistd::Pid, i32)>,
    /// Whether the job was stopped (e.g., Ctrl+Z) rather than exiting.
    pub(in crate::exec) stopped: bool,
}

impl Executor {
    /// Apply the shell's captured termios snapshot when in interactive
    /// + monitor mode. Best-effort; silent on failure or when the
    ///   snapshot is not set (non-interactive, non-monitor, or capture
    ///   failed at REPL startup).
    pub(in crate::exec) fn restore_shell_termios_if_interactive(&self) {
        if self.env.mode.is_interactive
            && self.env.mode.options.monitor
            && let Some(shell_t) = self.env.process.jobs.shell_tmodes()
        {
            let _ = crate::exec::terminal_state::apply_tty_termios(shell_t);
        }
    }

    /// Apply the per-job state transition for `WaitStatus::Stopped`.
    ///
    /// Decides only on `(job_id, sig, captured)`: writes the Stopped
    /// status, resets the `notified` flag so the change is reported,
    /// clears the foreground flag, and stores the captured termios —
    /// including `None`, which intentionally clears any previously saved
    /// snapshot. Preserves glibc-manual semantics across mid-session
    /// `exec 0</dev/null`: a stale snapshot from a TTY the shell no
    /// longer drives must not survive into a later `fg`.
    ///
    /// Silently no-ops if `job_id` is no longer in the table; the caller
    /// (`wait_for_foreground_job`) already tolerates that race.
    fn record_stopped_state(
        &mut self,
        job_id: crate::env::jobs::JobId,
        sig: i32,
        captured: Option<nix::sys::termios::Termios>,
    ) {
        if let Some(job) = self.env.process.jobs.get_mut(job_id) {
            // The terminal delivered the stop signal to the whole
            // foreground process group, so every still-running member
            // received it; already-exited members keep their statuses.
            job.stop_live_members(sig);
            job.notified = false;
            job.foreground = false;
            job.set_saved_tmodes(captured);
        }
    }

    /// Ctrl+C on a foreground job reaches only the job's process group, so
    /// the shell learns of it from the job's last process dying by SIGINT. An interactive
    /// shell whose SIGINT disposition is the default then abandons the rest
    /// of the command line, as if it had been interrupted itself (dash
    /// re-raises SIGINT; bash agrees): `sleep 5; echo x` + Ctrl+C prints
    /// nothing. Unwinds like a fatal expansion error; the REPL / `-c` loop
    /// clears the flag before the next command line.
    fn abandon_line_after_foreground_sigint(&mut self) {
        if self.env.mode.is_interactive
            && self.env.exec.flow_control.is_none()
            && !matches!(
                self.env.traps.get_signal_trap(libc::SIGINT),
                Some(TrapAction::Command(_) | TrapAction::Ignore)
            )
        {
            self.env.exec.flow_control = Some(FlowControl::ExpansionError);
        }
    }

    /// Wait for a foreground job to complete or stop.
    ///
    /// Returns a `ForegroundWaitResult` containing the last exit status,
    /// per-process statuses (for pipefail), and whether the job was stopped.
    ///
    /// Side effect: on `WaitStatus::Stopped`, captures the current TTY
    /// termios when in interactive + monitor mode and stdin is a TTY
    /// (otherwise `None`: the call-site guard short-circuits to `None`
    /// outside that mode, and `capture_tty_termios` itself returns
    /// `Ok(None)` when stdin is no longer a TTY). The result is handed to
    /// `record_stopped_state`, which writes it to `job.saved_tmodes` so a
    /// later `fg` can replay it. The capture is always written — including
    /// `None` overwrites — to avoid keeping a stale snapshot across
    /// `exec 0</dev/null` style redirections.
    pub(in crate::exec) fn wait_for_foreground_job(
        &mut self,
        job_id: crate::env::jobs::JobId,
    ) -> ForegroundWaitResult {
        let (pgid, total_processes, last_pid) = match self.env.process.jobs.get(job_id) {
            Some(j) => (j.pgid, j.pids.len(), j.pids.last().copied()),
            None => {
                return ForegroundWaitResult {
                    last_status: 1,
                    process_statuses: Vec::new(),
                    stopped: false,
                };
            }
        };

        let mut last_status = 0;
        let mut process_statuses: Vec<(nix::unistd::Pid, i32)> = Vec::new();
        let mut interrupted = false;

        loop {
            if process_statuses.len() >= total_processes {
                self.env.process.jobs.mark_notified(job_id);
                self.env.process.jobs.remove_job(job_id);
                break;
            }

            match waitpid(
                nix::unistd::Pid::from_raw(-pgid.as_raw()),
                Some(WaitPidFlag::WUNTRACED),
            ) {
                Ok(WaitStatus::Exited(pid, code)) => {
                    self.env
                        .process
                        .jobs
                        .update_status(pid, JobStatus::Done(code));
                    last_status = code;
                    process_statuses.push((pid, code));
                }
                Ok(WaitStatus::Signaled(pid, sig, _)) => {
                    // Only the job's last process counts, as for its
                    // status: `kill -INT`ed early pipeline members do not
                    // interrupt the shell (dash's getstatus agrees).
                    interrupted |= Some(pid) == last_pid && sig == nix::sys::signal::Signal::SIGINT;
                    let code = 128 + sig as i32;
                    self.env
                        .process
                        .jobs
                        .update_status(pid, JobStatus::Terminated(sig as i32));
                    last_status = code;
                    process_statuses.push((pid, code));
                }
                Ok(WaitStatus::Stopped(_pid, sig)) => {
                    // Snapshot the terminal state the stopped child was
                    // using, so `fg` can replay it on resume. Must run
                    // before we print anything, since the print itself
                    // happens in whatever termios the child left behind.
                    let captured = if self.env.mode.is_interactive && self.env.mode.options.monitor
                    {
                        crate::exec::terminal_state::capture_tty_termios()
                            .ok()
                            .flatten()
                    } else {
                        None
                    };
                    self.record_stopped_state(job_id, sig as i32, captured);
                    if let Some(line) = self.env.process.jobs.format_job(job_id) {
                        crate::byteenc::write_stderr_decoded_line(&line);
                    }
                    last_status = 128 + sig as i32;
                    return ForegroundWaitResult {
                        last_status,
                        process_statuses,
                        stopped: true,
                    };
                }
                Err(nix::errno::Errno::ECHILD) => {
                    self.env.process.jobs.remove_job(job_id);
                    break;
                }
                Err(nix::errno::Errno::EINTR) => {
                    self.process_pending_signals();
                    continue;
                }
                _ => break,
            }
        }

        if interrupted {
            self.abandon_line_after_foreground_sigint();
        }

        ForegroundWaitResult {
            last_status,
            process_statuses,
            stopped: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_stopped_state_clears_stale_saved_tmodes_on_none_capture() {
        use crate::env::jobs::JobStatus;
        use nix::unistd::Pid;
        let mut exec = Executor::new("yosh", vec![]);
        let pid = Pid::from_raw(12345);
        let id = exec
            .env
            .process
            .jobs
            .add_job(pid, vec![pid], "test-cmd", true);

        // Pre-populate saved_tmodes as if a previous stop captured a TTY snapshot.
        let zeroed: libc::termios = unsafe { std::mem::zeroed() };
        let t: nix::sys::termios::Termios = zeroed.into();
        exec.env
            .process
            .jobs
            .get_mut(id)
            .unwrap()
            .set_saved_tmodes(Some(t));
        assert!(
            exec.env
                .process
                .jobs
                .get(id)
                .unwrap()
                .saved_tmodes()
                .is_some(),
            "precondition: saved_tmodes should be populated before the simulated stop",
        );

        // Simulate the next stop where capture_tty_termios() returned Ok(None)
        // (e.g., after `exec 0</dev/null` redirected stdin away from the TTY).
        exec.record_stopped_state(id, libc::SIGTSTP, None);

        let job = exec
            .env
            .process
            .jobs
            .get(id)
            .expect("job should still be in table");
        assert!(
            job.saved_tmodes().is_none(),
            "stale termios must be cleared when capture returns None",
        );
        assert!(matches!(job.status, JobStatus::Stopped(_)));
        assert!(!job.foreground);
    }

    #[test]
    fn record_stopped_state_stores_some_capture() {
        use crate::env::jobs::JobStatus;
        use nix::unistd::Pid;
        let mut exec = Executor::new("yosh", vec![]);
        let pid = Pid::from_raw(12346);
        let id = exec
            .env
            .process
            .jobs
            .add_job(pid, vec![pid], "test-cmd", true);

        assert!(
            exec.env
                .process
                .jobs
                .get(id)
                .unwrap()
                .saved_tmodes()
                .is_none(),
            "precondition: saved_tmodes should start as None for a fresh job",
        );

        let zeroed: libc::termios = unsafe { std::mem::zeroed() };
        let t: nix::sys::termios::Termios = zeroed.into();

        exec.record_stopped_state(id, libc::SIGTSTP, Some(t));

        let job = exec
            .env
            .process
            .jobs
            .get(id)
            .expect("job should still be in table");
        assert!(job.saved_tmodes().is_some(), "Some capture must be stored");
        assert!(matches!(job.status, JobStatus::Stopped(_)));
        assert!(!job.foreground);
    }

    #[test]
    fn record_stopped_state_no_op_on_unknown_job() {
        let mut exec = Executor::new("yosh", vec![]);
        // job_id 9999 was never added; the helper must silently no-op
        // (the same race-tolerance the caller, `wait_for_foreground_job`,
        // already exhibits when a job is removed between waitpid and the
        // state-write).
        exec.record_stopped_state(9999, libc::SIGTSTP, None);
        assert!(exec.env.process.jobs.get(9999).is_none());
    }
}
