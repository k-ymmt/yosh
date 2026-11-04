//! The `fg` and `bg` builtins.

use super::super::Executor;
use super::{job_wait_status, strip_job_spec_prefix, terminated_wait_status};
use crate::env::jobs::{self, JobId, JobSpecError, JobStatus};
use crate::error::{RuntimeErrorKind, ShellError};

/// A `JobControlError` runtime error with the given message.
fn job_control_error(message: String) -> ShellError {
    ShellError::runtime(RuntimeErrorKind::JobControlError, message)
}

impl Executor {
    /// True when this process is a forked child of the job-controlling
    /// shell (subshell, async list, pipeline element, command sub) —
    /// `shell_pid` backs `$$` and survives forks unchanged, so a
    /// mismatch with the real pid identifies the fork. Job-control
    /// builtins must refuse there: the inherited job table describes
    /// the PARENT's jobs, and `(bg %1)` would SIGCONT a process the
    /// parent still tracks as stopped (bash: "no job control", while
    /// `$-` keeps `m` — the flag stays set, control is suppressed).
    fn in_forked_subshell(&self) -> bool {
        nix::unistd::getpid() != self.env.process.shell_pid
    }

    /// Resolve the job a `fg` / `bg` invocation (`builtin`) targets:
    /// the current job with no operand, else the job spec `args[0]`.
    /// Refuses with "no job control" outside monitor mode or in a
    /// forked subshell (see [`Self::in_forked_subshell`]).
    fn resolve_resume_target(&self, builtin: &str, args: &[String]) -> Result<JobId, ShellError> {
        if !self.env.mode.options.monitor || self.in_forked_subshell() {
            return Err(job_control_error(format!("{}: no job control", builtin)));
        }

        if args.is_empty() {
            return match self.env.process.jobs.current_id() {
                Some(id) => Ok(id),
                None => Err(job_control_error(format!("{}: no current job", builtin))),
            };
        }
        match self.env.process.jobs.resolve_job_spec(&args[0]) {
            Ok(id) => Ok(id),
            Err(JobSpecError::Ambiguous) => {
                let display = strip_job_spec_prefix(&args[0]);
                Err(job_control_error(format!(
                    "{}: {}: ambiguous job spec",
                    builtin, display
                )))
            }
            Err(_) => Err(job_control_error(format!(
                "{}: {}: no such job",
                builtin, args[0]
            ))),
        }
    }

    pub(in crate::exec) fn builtin_fg(&mut self, args: &[String]) -> Result<i32, ShellError> {
        let job_id = self.resolve_resume_target("fg", args)?;

        let (pgid, command, pids, mut member_wait_statuses) = {
            let job = match self.env.process.jobs.get(job_id) {
                Some(j) => j,
                None => {
                    return Err(job_control_error("fg: job not found".to_string()));
                }
            };
            // Snapshot members already terminal BEFORE resuming: for a
            // pipeline job, members reaped earlier (by the notification
            // reaper or a prior foreground wait) never report through
            // the wait below, but their statuses still shape the job's
            // final status (adversarial review 2026-09-02 round 2:
            // `sleep 2 | false & fg` must exit 1, the last MEMBER's
            // status, not the last-reaped process's 0; bash agrees).
            let pre: Vec<Option<i32>> = job
                .pids
                .iter()
                .map(|&p| job.member_status(p).and_then(terminated_wait_status))
                .collect();
            (job.pgid, job.command.clone(), job.pids.clone(), pre)
        };

        // Print the command being foregrounded
        crate::byteenc::write_stderr_decoded_line(&command);

        // Update job state — the SIGCONT below resumes the whole
        // process group, so every stopped member becomes Running.
        if let Some(job) = self.env.process.jobs.get_mut(job_id) {
            job.foreground = true;
            job.resume_stopped_members();
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

        // Give the terminal to the job BEFORE waking it (glibc-manual
        // order). The reverse order races with a job that re-checks
        // terminal ownership on SIGCONT — a backgrounded yosh in its
        // wait_until_foreground startup loop would observe itself still
        // background and immediately self-stop again, leaving fg to
        // report a freshly re-stopped job (2026-08-25 wrap-up review
        // round 1 finding).
        jobs::give_terminal(pgid).ok();

        // Send SIGCONT to resume if stopped
        nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGCONT).ok();

        // Wait for the job
        let result = self.wait_for_foreground_job(job_id);
        let status = if result.stopped {
            // Stopped again: report 128+sig like any foreground stop.
            result.last_status
        } else {
            // Merge this wait's reaps over the pre-captured terminal
            // members and report the JOB's status in pipeline order —
            // pipefail-aware, like the foreground pipeline path and
            // `wait %job` (bash parity, empirical 2026-09-02).
            for (pid, code) in &result.process_statuses {
                if let Some(i) = pids.iter().position(|p| p == pid) {
                    member_wait_statuses[i] = Some(*code);
                }
            }
            job_wait_status(&member_wait_statuses, self.env.mode.options.pipefail)
        };

        // Take terminal back
        jobs::take_terminal(self.env.process.shell_pgid).ok();

        // Restore shell termios after any foreground completion
        // (stopped or exited).
        self.restore_shell_termios_if_interactive();

        Ok(status)
    }

    pub(in crate::exec) fn builtin_bg(&mut self, args: &[String]) -> Result<i32, ShellError> {
        let job_id = self.resolve_resume_target("bg", args)?;

        let pgid = {
            let job = match self.env.process.jobs.get(job_id) {
                Some(j) => j,
                None => {
                    return Err(job_control_error("bg: job not found".to_string()));
                }
            };
            if !matches!(job.status, JobStatus::Stopped(_)) {
                return Err(job_control_error(format!("bg: job {} not stopped", job_id)));
            }
            job.pgid
        };

        // Update job state — the SIGCONT below resumes the whole
        // process group, so every stopped member becomes Running.
        if let Some(job) = self.env.process.jobs.get_mut(job_id) {
            job.resume_stopped_members();
            job.foreground = false;
            crate::byteenc::write_stderr_decoded_line(&format!("[{}]+ {} &", job.id, job.command));
        }
        // bash: `$!` is the job most recently placed in the background,
        // whether started with `&` or resumed with `bg`.
        self.env.process.jobs.set_last_bg_pid(pgid);

        // Send SIGCONT
        nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGCONT).ok();

        Ok(0)
    }
}
