use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;

use super::Executor;
use crate::env::jobs::{self, JobSpecError, JobStatus};
use crate::error::{RuntimeErrorKind, ShellError};
use crate::signal;

/// Result of waiting for a foreground job.
pub(super) struct ForegroundWaitResult {
    /// Exit status of the last process to report.
    pub(super) last_status: i32,
    /// Per-process exit statuses (pid, exit_code) in reporting order — used by pipefail.
    pub(super) process_statuses: Vec<(nix::unistd::Pid, i32)>,
    /// Whether the job was stopped (e.g., Ctrl+Z) rather than exiting.
    pub(super) stopped: bool,
}

/// Strip the leading `%` (and optional `?`) from a job spec string for
/// inclusion in error messages. Matches bash: `wait %sleep` with ambiguous
/// match reports `wait: sleep: ambiguous job spec`, not `%sleep:`.
/// Inputs that don't start with `%` are returned unchanged.
fn strip_job_spec_prefix(spec: &str) -> &str {
    match spec.strip_prefix('%') {
        Some(rest) => rest.strip_prefix('?').unwrap_or(rest),
        None => spec,
    }
}

impl Executor {
    /// POSIX wait builtin: wait for background jobs.
    pub(super) fn builtin_wait(&mut self, args: &[String]) -> Result<i32, ShellError> {
        let target_pids: Vec<Pid> = if args.is_empty() {
            self.env
                .process
                .jobs
                .all_jobs()
                .filter(|j| j.status == JobStatus::Running)
                .map(|j| j.pgid)
                .collect()
        } else {
            let mut pids = Vec::new();
            for arg in args {
                if arg.starts_with('%') {
                    match self.env.process.jobs.resolve_job_spec(arg) {
                        Ok(job_id) => {
                            if let Some(job) = self.env.process.jobs.get(job_id) {
                                pids.push(job.pgid);
                            } else {
                                return Err(ShellError::runtime(
                                    RuntimeErrorKind::CommandNotFound,
                                    format!("wait: {}: no such job", arg),
                                ));
                            }
                        }
                        Err(JobSpecError::Ambiguous) => {
                            let display = strip_job_spec_prefix(arg);
                            return Err(ShellError::runtime(
                                RuntimeErrorKind::CommandNotFound,
                                format!("wait: {}: ambiguous job spec", display),
                            ));
                        }
                        Err(_) => {
                            return Err(ShellError::runtime(
                                RuntimeErrorKind::CommandNotFound,
                                format!("wait: {}: no such job", arg),
                            ));
                        }
                    }
                } else {
                    match arg.parse::<i32>() {
                        Ok(n) => pids.push(Pid::from_raw(n)),
                        Err(_) => {
                            return Err(ShellError::runtime(
                                RuntimeErrorKind::InvalidArgument,
                                format!("wait: {}: not a pid or valid job spec", arg),
                            ));
                        }
                    }
                }
            }
            pids
        };

        if target_pids.is_empty() {
            return Ok(self.env.exec.last_exit_status);
        }

        let mut last_status = 0;

        for pid in &target_pids {
            // Check if already completed in jobs table
            let already_done = self
                .env
                .process
                .jobs
                .all_jobs()
                .find(|j| j.pgid == *pid)
                .and_then(|j| match j.status {
                    JobStatus::Done(code) => Some(code),
                    JobStatus::Terminated(sig) => Some(128 + sig),
                    _ => None,
                });
            if let Some(s) = already_done {
                last_status = s;
                continue;
            }

            loop {
                match waitpid(*pid, Some(WaitPidFlag::WNOHANG)) {
                    Ok(WaitStatus::Exited(p, code)) => {
                        self.env
                            .process
                            .jobs
                            .update_status(p, JobStatus::Done(code));
                        last_status = code;
                        break;
                    }
                    Ok(WaitStatus::Signaled(p, sig, _)) => {
                        let code = 128 + sig as i32;
                        self.env
                            .process
                            .jobs
                            .update_status(p, JobStatus::Terminated(sig as i32));
                        last_status = code;
                        break;
                    }
                    Ok(WaitStatus::StillAlive) => {
                        // Poll self-pipe with a short timeout so we also notice
                        // SIGCHLD (which is not written to the self-pipe).
                        let pipe_fd = signal::self_pipe_read_fd();
                        let mut fds = [nix::poll::PollFd::new(
                            unsafe { std::os::fd::BorrowedFd::borrow_raw(pipe_fd) },
                            nix::poll::PollFlags::POLLIN,
                        )];
                        match nix::poll::poll(&mut fds, nix::poll::PollTimeout::from(50u16)) {
                            Ok(_)
                                if fds[0]
                                    .revents()
                                    .is_some_and(|r| r.contains(nix::poll::PollFlags::POLLIN)) =>
                            {
                                let signals = signal::drain_pending_signals();
                                if !signals.is_empty() {
                                    self.process_pending_signals();
                                    last_status = 128 + *signals.last().unwrap();
                                    return Ok(last_status);
                                }
                            }
                            Err(nix::errno::Errno::EINTR) => {
                                // Interrupted — retry waitpid
                            }
                            _ => {
                                // Timeout or no self-pipe data — retry waitpid
                            }
                        }
                    }
                    Err(nix::errno::Errno::ECHILD) => {
                        let err = ShellError::runtime(
                            RuntimeErrorKind::CommandNotFound,
                            format!("wait: pid {} is not a child of this shell", pid),
                        );
                        eprintln!("{}", err);
                        last_status = 127;
                        break;
                    }
                    Err(_) | Ok(_) => break,
                }
            }
        }

        Ok(last_status)
    }

    pub(super) fn builtin_jobs(&mut self, args: &[String]) -> Result<i32, ShellError> {
        let long_format = args.contains(&"-l".to_string());
        let pgid_only = args.contains(&"-p".to_string());

        // Collect job IDs first to avoid borrow issues
        let job_ids: Vec<crate::env::jobs::JobId> =
            self.env.process.jobs.all_jobs().map(|j| j.id).collect();

        for id in &job_ids {
            if pgid_only {
                if let Some(job) = self.env.process.jobs.get(*id) {
                    println!("{}", job.pgid.as_raw());
                }
            } else if long_format {
                if let Some(line) = self.env.process.jobs.format_job_long(*id) {
                    println!("{}", line);
                }
            } else if let Some(line) = self.env.process.jobs.format_job(*id) {
                println!("{}", line);
            }
        }

        // Mark done/terminated jobs as notified
        let pending = self.env.process.jobs.pending_notifications();
        for id in pending {
            self.env.process.jobs.mark_notified(id);
        }

        Ok(0)
    }

    pub(super) fn builtin_fg(&mut self, args: &[String]) -> Result<i32, ShellError> {
        if !self.env.mode.options.monitor {
            return Err(ShellError::runtime(
                RuntimeErrorKind::JobControlError,
                "fg: no job control".to_string(),
            ));
        }

        let job_id = if args.is_empty() {
            match self.env.process.jobs.current_id() {
                Some(id) => id,
                None => {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::JobControlError,
                        "fg: no current job".to_string(),
                    ));
                }
            }
        } else {
            match self.env.process.jobs.resolve_job_spec(&args[0]) {
                Ok(id) => id,
                Err(JobSpecError::Ambiguous) => {
                    let display = strip_job_spec_prefix(&args[0]);
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::JobControlError,
                        format!("fg: {}: ambiguous job spec", display),
                    ));
                }
                Err(_) => {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::JobControlError,
                        format!("fg: {}: no such job", args[0]),
                    ));
                }
            }
        };

        let (pgid, command) = {
            let job = match self.env.process.jobs.get(job_id) {
                Some(j) => j,
                None => {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::JobControlError,
                        "fg: job not found".to_string(),
                    ));
                }
            };
            (job.pgid, job.command.clone())
        };

        // Print the command being foregrounded
        eprintln!("{}", command);

        // Update job state
        if let Some(job) = self.env.process.jobs.get_mut(job_id) {
            job.foreground = true;
            if matches!(job.status, JobStatus::Stopped(_)) {
                job.status = JobStatus::Running;
            }
        }

        // Restore the job's saved termios (if any) before handing the
        // terminal back. Falls back to the shell's snapshot so a job that
        // reaches fg without a stored termios (e.g. one that was never
        // stopped) at least lands in the shell's canonical mode.
        if self.env.mode.is_interactive && self.env.mode.options.monitor {
            let target = {
                let job_t = self
                    .env
                    .process
                    .jobs
                    .get(job_id)
                    .and_then(|j| j.saved_tmodes().cloned());
                job_t.or_else(|| self.env.process.jobs.shell_tmodes().cloned())
            };
            if let Some(t) = target {
                let _ = crate::exec::terminal_state::apply_tty_termios(&t);
            }
        }

        // Send SIGCONT to resume if stopped
        nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGCONT).ok();

        // Give terminal to the job
        jobs::give_terminal(pgid).ok();

        // Wait for the job
        let result = self.wait_for_foreground_job(job_id);
        let status = result.last_status;

        // Take terminal back
        jobs::take_terminal(self.env.process.shell_pgid).ok();

        // Restore shell termios after any foreground completion
        // (stopped or exited).
        self.restore_shell_termios_if_interactive();

        Ok(status)
    }

    pub(super) fn builtin_bg(&mut self, args: &[String]) -> Result<i32, ShellError> {
        if !self.env.mode.options.monitor {
            return Err(ShellError::runtime(
                RuntimeErrorKind::JobControlError,
                "bg: no job control".to_string(),
            ));
        }

        let job_id = if args.is_empty() {
            match self.env.process.jobs.current_id() {
                Some(id) => id,
                None => {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::JobControlError,
                        "bg: no current job".to_string(),
                    ));
                }
            }
        } else {
            match self.env.process.jobs.resolve_job_spec(&args[0]) {
                Ok(id) => id,
                Err(JobSpecError::Ambiguous) => {
                    let display = strip_job_spec_prefix(&args[0]);
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::JobControlError,
                        format!("bg: {}: ambiguous job spec", display),
                    ));
                }
                Err(_) => {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::JobControlError,
                        format!("bg: {}: no such job", args[0]),
                    ));
                }
            }
        };

        let pgid = {
            let job = match self.env.process.jobs.get(job_id) {
                Some(j) => j,
                None => {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::JobControlError,
                        "bg: job not found".to_string(),
                    ));
                }
            };
            if !matches!(job.status, JobStatus::Stopped(_)) {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::JobControlError,
                    format!("bg: job {} not stopped", job_id),
                ));
            }
            job.pgid
        };

        // Update job state
        if let Some(job) = self.env.process.jobs.get_mut(job_id) {
            job.status = JobStatus::Running;
            job.foreground = false;
            eprintln!("[{}]+ {} &", job.id, job.command);
        }

        // Send SIGCONT
        nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGCONT).ok();

        Ok(0)
    }

    /// Apply the shell's captured termios snapshot when in interactive
    /// + monitor mode. Best-effort; silent on failure or when the
    ///   snapshot is not set (non-interactive, non-monitor, or capture
    ///   failed at REPL startup).
    pub(super) fn restore_shell_termios_if_interactive(&self) {
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
            job.status = JobStatus::Stopped(sig);
            job.notified = false;
            job.foreground = false;
            job.set_saved_tmodes(captured);
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
    pub(super) fn wait_for_foreground_job(
        &mut self,
        job_id: crate::env::jobs::JobId,
    ) -> ForegroundWaitResult {
        let (pgid, total_processes) = match self.env.process.jobs.get(job_id) {
            Some(j) => (j.pgid, j.pids.len()),
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
                        eprintln!("{}", line);
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

        ForegroundWaitResult {
            last_status,
            process_statuses,
            stopped: false,
        }
    }

    /// Display pending job notifications and clean up completed jobs.
    pub fn display_job_notifications(&mut self) {
        let pending = self.env.process.jobs.pending_notifications();
        for id in &pending {
            if let Some(line) = self.env.process.jobs.format_job(*id) {
                eprintln!("{}", line);
            }
            self.env.process.jobs.mark_notified(*id);
        }
        self.env.process.jobs.cleanup_notified();
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
