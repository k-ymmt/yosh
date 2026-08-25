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

/// Parsed form of a `jobs [-l|-p] [--] [job_spec...]` invocation.
#[derive(Debug)]
struct JobsOpts {
    long_format: bool,
    pgid_only: bool,
    operands: Vec<String>,
}

/// Parse `jobs` flags + operands. Returns `Err(message)` on unknown
/// option; `message` is already prefixed (e.g., `"jobs: -x: invalid option"`)
/// for the caller to write to stderr verbatim.
fn parse_options(args: &[String]) -> Result<JobsOpts, String> {
    let mut long_format = false;
    let mut pgid_only = false;
    let mut idx = 0;

    while idx < args.len() {
        let a = &args[idx];
        if a == "--" {
            idx += 1;
            break;
        }
        if !a.starts_with('-') || a == "-" {
            break;
        }
        for ch in a[1..].chars() {
            match ch {
                'l' => long_format = true,
                'p' => pgid_only = true,
                other => return Err(format!("jobs: -{}: invalid option", other)),
            }
        }
        idx += 1;
    }

    let operands = args[idx..].to_vec();
    Ok(JobsOpts {
        long_format,
        pgid_only,
        operands,
    })
}

impl Executor {
    /// POSIX wait builtin: wait for background jobs.
    pub(super) fn builtin_wait(&mut self, args: &[String]) -> Result<i32, ShellError> {
        // POSIX XCU wait: with no operands, wait for all known process
        // IDs and exit ZERO regardless of the children's statuses
        // (bash/dash agree). Child statuses only propagate when a pid
        // or job-spec operand is given. A >128 trapped-signal
        // interruption still overrides this via its early return.
        let no_operands = args.is_empty();
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
            // No operands and nothing running: POSIX no-operand wait
            // exits 0 (previously leaked $?, so `false; wait` was 1).
            // Completing a no-operand wait discards remembered statuses
            // of already-forgotten jobs (bash: `wait; wait $p` then
            // reports "not a child", empirical 2026-08-25).
            self.env.process.jobs.clear_reaped();
            return Ok(0);
        }

        let mut last_status = 0;

        for pid in &target_pids {
            // Check if already completed: first the live jobs table,
            // then — when the interactive notification pass has already
            // reaped, reported, and dropped the job — the retained
            // reaped-status map (POSIX XCU wait: known `$!` pids stay
            // waitable until consumed by a no-operand wait). The map is
            // consulted only when the pid is absent from the table —
            // matched against every member pid, not just the leader, so
            // a recycled pid backing any live process cannot resolve to
            // a stale status.
            let table_status = self
                .env
                .process
                .jobs
                .all_jobs()
                .find(|j| j.pids.contains(pid))
                .map(|j| j.status);
            match table_status {
                Some(JobStatus::Done(code)) => {
                    last_status = code;
                    continue;
                }
                Some(JobStatus::Terminated(sig)) => {
                    last_status = 128 + sig;
                    continue;
                }
                Some(_) => {}
                None => {
                    if let Some(s) = self.env.process.jobs.reaped_status(*pid) {
                        last_status = s;
                        continue;
                    }
                }
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
                        // Poll the self-pipe with a short timeout so a signal
                        // arriving mid-wait is noticed promptly. In monitor
                        // mode SIGCHLD is registered on the self-pipe too, so
                        // a child exit also wakes this poll.
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
                                    // The self-pipe is already drained, so run
                                    // the trap actions for the drained signals
                                    // directly (process_pending_signals would
                                    // find an empty pipe and do nothing).
                                    self.run_signal_traps(&signals);
                                    // SIGCHLD is the child-exit notification
                                    // itself, not an interruption of wait —
                                    // fall through and re-poll waitpid, which
                                    // now reaps the exited child. This holds
                                    // even when the user trapped CHLD: the
                                    // trap action ran above, and bash/dash
                                    // both return the child's status from
                                    // `trap 'echo T' CHLD; cmd & wait $!`
                                    // rather than 128+SIGCHLD (empirical,
                                    // 2026-08-25). Only other signals
                                    // interrupt wait with 128+sig.
                                    if let Some(&sig) =
                                        signals.iter().rfind(|&&s| s != libc::SIGCHLD)
                                    {
                                        last_status = 128 + sig;
                                        return Ok(last_status);
                                    }
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
                        // A trap action run mid-wait (e.g. a CHLD trap
                        // calling `wait` itself) may have already reaped
                        // this pid and recorded its status in the jobs
                        // table — report that status instead of an error.
                        let reaped = self
                            .env
                            .process
                            .jobs
                            .all_jobs()
                            .find(|j| j.pids.contains(pid))
                            .and_then(|j| match j.status {
                                JobStatus::Done(code) => Some(code),
                                JobStatus::Terminated(sig) => Some(128 + sig),
                                _ => None,
                            });
                        if let Some(s) = reaped {
                            last_status = s;
                            break;
                        }
                        // Or a notification pass mid-wait may have
                        // reaped AND removed the job — consult the
                        // retained reaped-status map before erroring.
                        if let Some(s) = self.env.process.jobs.reaped_status(*pid) {
                            last_status = s;
                            break;
                        }
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

        if no_operands {
            // Completed no-operand wait: discard remembered statuses of
            // already-forgotten jobs (bash behavior; the >128 trapped-
            // signal early return above intentionally skips this).
            self.env.process.jobs.clear_reaped();
        }
        Ok(if no_operands { 0 } else { last_status })
    }

    pub(super) fn builtin_jobs(&mut self, args: &[String]) -> Result<i32, ShellError> {
        let opts = match parse_options(args) {
            Ok(o) => o,
            Err(msg) => {
                eprintln!("yosh: {}", msg);
                return Ok(1);
            }
        };

        // Decide which job IDs to print.
        let mut exit_status = 0;
        let job_ids: Vec<crate::env::jobs::JobId> = if opts.operands.is_empty() {
            self.env.process.jobs.all_jobs().map(|j| j.id).collect()
        } else {
            let mut resolved = Vec::with_capacity(opts.operands.len());
            for spec in &opts.operands {
                match self.env.process.jobs.resolve_job_spec(spec) {
                    Ok(id) => resolved.push(id),
                    Err(JobSpecError::Ambiguous) => {
                        let display = strip_job_spec_prefix(spec);
                        eprintln!("yosh: jobs: {}: ambiguous job spec", display);
                        exit_status = 1;
                    }
                    Err(_) => {
                        eprintln!("yosh: jobs: {}: no such job", spec);
                        exit_status = 1;
                    }
                }
            }
            resolved
        };

        for id in &job_ids {
            if opts.pgid_only {
                if let Some(job) = self.env.process.jobs.get(*id) {
                    println!("{}", job.pgid.as_raw());
                }
            } else if opts.long_format {
                if let Some(line) = self.env.process.jobs.format_job_long(*id) {
                    println!("{}", line);
                }
            } else if let Some(line) = self.env.process.jobs.format_job(*id) {
                println!("{}", line);
            }
        }

        // Mark done/terminated jobs as notified.
        let pending = self.env.process.jobs.pending_notifications();
        for id in pending {
            self.env.process.jobs.mark_notified(id);
        }

        Ok(exit_status)
    }

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

    pub(super) fn builtin_fg(&mut self, args: &[String]) -> Result<i32, ShellError> {
        if !self.env.mode.options.monitor || self.in_forked_subshell() {
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
        let status = result.last_status;

        // Take terminal back
        jobs::take_terminal(self.env.process.shell_pgid).ok();

        // Restore shell termios after any foreground completion
        // (stopped or exited).
        self.restore_shell_termios_if_interactive();

        Ok(status)
    }

    pub(super) fn builtin_bg(&mut self, args: &[String]) -> Result<i32, ShellError> {
        if !self.env.mode.options.monitor || self.in_forked_subshell() {
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
        // bash: `$!` is the job most recently placed in the background,
        // whether started with `&` or resumed with `bg`.
        self.env.process.jobs.set_last_bg_pid(pgid);

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
    pub(crate) fn display_job_notifications(&mut self) {
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
    fn parse_options_recognizes_long_flag() {
        let args = vec!["-l".to_string()];
        let opts = parse_options(&args).unwrap();
        assert!(opts.long_format);
        assert!(!opts.pgid_only);
        assert_eq!(opts.operands, Vec::<String>::new());
    }

    #[test]
    fn parse_options_recognizes_pgid_flag() {
        let args = vec!["-p".to_string()];
        let opts = parse_options(&args).unwrap();
        assert!(!opts.long_format);
        assert!(opts.pgid_only);
    }

    #[test]
    fn parse_options_clustered_flags() {
        let args = vec!["-lp".to_string()];
        let opts = parse_options(&args).unwrap();
        assert!(opts.long_format);
        assert!(opts.pgid_only);
    }

    #[test]
    fn parse_options_double_dash_ends_flags() {
        let args = vec!["--".to_string(), "%1".to_string()];
        let opts = parse_options(&args).unwrap();
        assert_eq!(opts.operands, vec!["%1".to_string()]);
    }

    #[test]
    fn parse_options_rejects_unknown_flag() {
        let args = vec!["-x".to_string()];
        let err = parse_options(&args).unwrap_err();
        assert!(err.contains("jobs:") && err.contains("-x"));
    }

    #[test]
    fn parse_options_collects_operands_after_flags() {
        let args = vec!["-l".to_string(), "%1".to_string(), "%2".to_string()];
        let opts = parse_options(&args).unwrap();
        assert!(opts.long_format);
        assert_eq!(opts.operands, vec!["%1".to_string(), "%2".to_string()]);
    }

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
    fn wait_reports_status_of_notified_and_cleaned_job() {
        use crate::env::jobs::JobStatus;
        use nix::unistd::Pid;
        let mut exec = Executor::new("yosh", vec![]);
        let pid = Pid::from_raw(88888);
        let id = exec
            .env
            .process
            .jobs
            .add_job(pid, vec![pid], "sh -c 'exit 7'", false);

        // Simulate the interactive notification pass: reap, report, drop.
        exec.env.process.jobs.update_status(pid, JobStatus::Done(7));
        exec.env.process.jobs.mark_notified(id);
        exec.env.process.jobs.cleanup_notified();
        assert!(exec.env.process.jobs.get(id).is_none());

        // POSIX XCU wait: the known pid must stay waitable — previously
        // this errored "pid 88888 is not a child of this shell" (127).
        let status = exec
            .builtin_wait(&["88888".to_string()])
            .expect("wait on a remembered pid must not error");
        assert_eq!(status, 7);

        // Non-consuming (bash behavior): a second wait reports it again.
        let status = exec
            .builtin_wait(&["88888".to_string()])
            .expect("repeated wait on a remembered pid must not error");
        assert_eq!(status, 7);
    }

    #[test]
    fn wait_matches_non_leader_member_pid_in_table() {
        use crate::env::jobs::JobStatus;
        use nix::unistd::Pid;
        let mut exec = Executor::new("yosh", vec![]);
        let leader = Pid::from_raw(88890);
        let member = Pid::from_raw(88891);
        exec.env
            .process
            .jobs
            .add_job(leader, vec![leader, member], "a | b", false);
        exec.env
            .process
            .jobs
            .update_status(member, JobStatus::Done(4));

        // The already-done fast path must match member pids, not just the
        // pgid leader — previously this fell through to waitpid/ECHILD and
        // errored 127 ("not a child of this shell").
        let status = exec
            .builtin_wait(&["88891".to_string()])
            .expect("wait on a member pid of a Done job must not error");
        assert_eq!(status, 4);
    }

    #[test]
    fn no_operand_wait_discards_remembered_statuses() {
        use nix::unistd::Pid;
        let mut exec = Executor::new("yosh", vec![]);
        let pid = Pid::from_raw(88889);
        exec.env.process.jobs.record_reaped(pid, 7);

        // Bare `wait` with nothing running completes immediately and
        // discards the remembered statuses (bash behavior).
        let status = exec.builtin_wait(&[]).expect("bare wait must succeed");
        assert_eq!(status, 0);
        assert_eq!(exec.env.process.jobs.reaped_status(pid), None);
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
