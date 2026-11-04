//! The `wait` builtin.

use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;

use super::super::Executor;
use super::{job_wait_status, strip_job_spec_prefix, terminated_wait_status};
use crate::env::jobs::{JobSpecError, JobStatus};
use crate::error::{RuntimeErrorKind, ShellError};
use crate::signal;

/// One `wait` operand's resolved target: a bare pid, or a whole job
/// (job-spec operand, or a pid that belongs to a live multi-pid job —
/// bash waits for the entire job in both cases and reports the JOB's
/// status, pipefail included; empirical 2026-09-02: `sleep 2 | true &
/// wait $!` blocks the full 2s, and with pipefail returns 1).
enum WaitTarget {
    Pid(Pid),
    /// Member pids in pipeline order.
    Job(Vec<Pid>),
    /// An operand that failed to resolve (unknown/ambiguous job spec).
    /// The diagnostic has been printed; the operand contributes 127
    /// but must NOT abort the remaining operands — POSIX XCU wait
    /// treats unknown ids as terminated with status 127 and still
    /// waits the rest (bash: `wait %999 "$p"` reports the error, then
    /// blocks on $p and returns its status; empirical 2026-09-02,
    /// adversarial review round 3).
    Unknown,
}

/// Outcome of waiting for a single pid.
enum PidWait {
    /// The pid's own wait-style status (`code` or `128+sig`).
    Status(i32),
    /// Not a child and not remembered — the "not a child of this
    /// shell" diagnostic has been printed.
    Errored,
    /// A trapped non-CHLD signal interrupted the wait: 128+sig, and
    /// the whole `wait` builtin must return it immediately.
    Interrupted(i32),
}

impl Executor {
    /// POSIX wait builtin: wait for background jobs.
    pub(in crate::exec) fn builtin_wait(&mut self, args: &[String]) -> Result<i32, ShellError> {
        // POSIX XCU wait: with no operands, wait for all known process
        // IDs and exit ZERO regardless of the children's statuses
        // (bash/dash agree). Child statuses only propagate when a pid
        // or job-spec operand is given. A >128 trapped-signal
        // interruption still overrides this via its early return.
        let no_operands = args.is_empty();
        let targets = self.resolve_wait_targets(args)?;

        // No early return when `targets` is empty (no operands and every
        // job already terminal): the finished-job cleanup and the
        // reaped-status discard below must still run, or `sleep 0 &
        // sleep 1; wait; sleep 1 & jobs` keeps listing the Done job as
        // %1 (dash/bash list only the new job, as %1). A no-operand
        // wait with nothing to wait for still exits 0 (`false; wait`).
        let pipefail = self.env.mode.options.pipefail;
        let mut last_status = 0;

        for target in &targets {
            match target {
                WaitTarget::Pid(pid) => match self.wait_one_pid(*pid) {
                    PidWait::Status(s) => last_status = s,
                    PidWait::Errored => last_status = 127,
                    PidWait::Interrupted(s) => return Ok(s),
                },
                WaitTarget::Job(pids) => {
                    let mut statuses: Vec<Option<i32>> = Vec::with_capacity(pids.len());
                    for pid in pids {
                        match self.wait_one_pid(*pid) {
                            PidWait::Status(s) => statuses.push(Some(s)),
                            PidWait::Errored => statuses.push(None),
                            PidWait::Interrupted(s) => return Ok(s),
                        }
                    }
                    last_status = job_wait_status(&statuses, pipefail);
                }
                WaitTarget::Unknown => last_status = 127,
            }
        }

        if !self.env.mode.is_interactive {
            self.forget_waited_jobs(&targets, no_operands);
        }

        if no_operands {
            // Completed no-operand wait: discard remembered statuses of
            // already-forgotten jobs (bash behavior; the >128 trapped-
            // signal early return above intentionally skips this).
            self.env.process.jobs.clear_reaped();
        }
        Ok(if no_operands { 0 } else { last_status })
    }

    /// Resolve `wait` operands into targets. Unknown/ambiguous job specs
    /// print their diagnostic and become [`WaitTarget::Unknown`]; an
    /// operand that is neither a job spec nor a positive pid is a hard
    /// error.
    fn resolve_wait_targets(&self, args: &[String]) -> Result<Vec<WaitTarget>, ShellError> {
        if args.is_empty() {
            // POSIX: wait until all known process IDs TERMINATE — a
            // Stopped job has not terminated, so it must be waited for
            // too (dash agrees, blocking until the job is continued and
            // exits; previously Stopped jobs were skipped and the wait
            // returned 0 immediately). All member pids, not just the
            // leader: a pipeline job terminates when every member does.
            return Ok(self
                .env
                .process
                .jobs
                .all_jobs()
                .filter(|j| matches!(j.status, JobStatus::Running | JobStatus::Stopped(_)))
                .map(|j| WaitTarget::Job(j.pids.clone()))
                .collect());
        }

        let mut targets = Vec::new();
        for arg in args {
            if arg.starts_with('%') {
                // A spec that fails to resolve contributes 127 but
                // does not abort the remaining operands (see
                // WaitTarget::Unknown).
                match self.env.process.jobs.resolve_job_spec(arg) {
                    Ok(job_id) => {
                        if let Some(job) = self.env.process.jobs.get(job_id) {
                            targets.push(WaitTarget::Job(job.pids.clone()));
                        } else {
                            crate::byteenc::write_stderr_decoded_line(&format!(
                                "yosh: wait: {}: no such job",
                                arg
                            ));
                            targets.push(WaitTarget::Unknown);
                        }
                    }
                    Err(JobSpecError::Ambiguous) => {
                        let display = strip_job_spec_prefix(arg);
                        crate::byteenc::write_stderr_decoded_line(&format!(
                            "yosh: wait: {}: ambiguous job spec",
                            display
                        ));
                        targets.push(WaitTarget::Unknown);
                    }
                    Err(_) => {
                        crate::byteenc::write_stderr_decoded_line(&format!(
                            "yosh: wait: {}: no such job",
                            arg
                        ));
                        targets.push(WaitTarget::Unknown);
                    }
                }
            } else {
                match arg.parse::<i32>() {
                    // POSIX: a pid operand is an unsigned decimal
                    // integer. 0 and negatives are rejected — passing
                    // them through would make waitpid(2) wait a whole
                    // process group (bash rejects them too;
                    // adversarial review round 3).
                    Ok(n) if n > 0 => {
                        let pid = Pid::from_raw(n);
                        // A pid that names a member of a live job
                        // targets the whole job (bash: `wait $!`
                        // on `a | b &` waits for a too and reports
                        // the job's status). A pid outside the
                        // table stays a bare-pid wait — including
                        // reaped-map lookups, which remember each
                        // member's OWN status once the job is gone.
                        let job_pids = self
                            .env
                            .process
                            .jobs
                            .all_jobs()
                            .find(|j| j.pids.contains(&pid))
                            .map(|j| j.pids.clone());
                        match job_pids {
                            Some(pids) => targets.push(WaitTarget::Job(pids)),
                            None => targets.push(WaitTarget::Pid(pid)),
                        }
                    }
                    Ok(_) | Err(_) => {
                        return Err(ShellError::runtime(
                            RuntimeErrorKind::InvalidArgument,
                            format!("wait: {}: not a pid or valid job spec", arg),
                        ));
                    }
                }
            }
        }
        Ok(targets)
    }

    /// Drop the jobs a completed non-interactive `wait` reported.
    ///
    /// A non-interactive shell has no notification pass, so a job whose
    /// termination `wait` just reported would otherwise stay in the
    /// table forever, holding its job number and showing up as Done
    /// in later `jobs` output (bash drops it; `sleep 0 & wait; sleep
    /// 1 & jobs` lists `[1] Running`). The interactive shell keeps it
    /// for the prompt's Done notification. cleanup_notified records
    /// the members' statuses, so `wait $!` keeps working.
    fn forget_waited_jobs(&mut self, targets: &[WaitTarget], no_operands: bool) {
        let jobs = &self.env.process.jobs;
        let finished: Vec<crate::env::jobs::JobId> = if no_operands {
            // Every known job has now terminated.
            jobs.all_jobs()
                .filter(|j| j.status.is_terminal())
                .map(|j| j.id)
                .collect()
        } else {
            targets
                .iter()
                .filter_map(|t| match t {
                    WaitTarget::Job(pids) => jobs
                        .all_jobs()
                        .find(|j| j.pids == *pids && j.status.is_terminal())
                        .map(|j| j.id),
                    _ => None,
                })
                .collect()
        };
        for id in finished {
            self.env.process.jobs.mark_notified(id);
        }
        self.env.process.jobs.cleanup_notified();
    }

    /// The tracked status of `pid` in the live job table, if it is a
    /// member of a job there.
    fn job_table_member_status(&self, pid: Pid) -> Option<JobStatus> {
        self.env
            .process
            .jobs
            .all_jobs()
            .find(|j| j.pids.contains(&pid))
            .and_then(|j| j.member_status(pid))
    }

    /// Wait for a single pid to terminate, reporting its own wait-style
    /// status. Consults, in order: the live job table (that MEMBER's
    /// tracked status), the reaped-status map (POSIX XCU wait: known
    /// `$!` pids stay waitable after the notification pass drops the
    /// job — non-consuming until a no-operand wait clears it), then a
    /// blocking waitpid poll loop that services trapped signals
    /// (returning `Interrupted` for non-CHLD ones).
    fn wait_one_pid(&mut self, pid: Pid) -> PidWait {
        // The map is consulted only when the pid is absent from the
        // table — matched against every member pid, not just the
        // leader, so a recycled pid backing any live process cannot
        // resolve to a stale status.
        match self.job_table_member_status(pid) {
            Some(status) => {
                if let Some(s) = terminated_wait_status(status) {
                    return PidWait::Status(s);
                }
            }
            None => {
                if let Some(s) = self.env.process.jobs.reaped_status(pid) {
                    return PidWait::Status(s);
                }
            }
        }

        loop {
            // Drain pending signals BEFORE reaping: when a trapped
            // signal and the child's exit are both already pending
            // (e.g. the child signaled this shell and exited while
            // the shell was stopped), the signal interruption must
            // win — POSIX wait returns 128+sig and the pid stays
            // waitable; bash agrees (wrap-up review 2026-09-02
            // round 3 finding). The pipe is drained directly, so a
            // later process_pending_signals finds it empty and does
            // nothing.
            let signals = signal::drain_pending_signals();
            if !signals.is_empty() {
                // SIGCHLD is the child-exit notification itself,
                // not an interruption of wait — after the traps run
                // below, loop and re-poll waitpid, which then reaps
                // the exited child. This holds even when the user
                // trapped CHLD: bash/dash both return the child's
                // status from `trap 'echo T' CHLD; cmd & wait $!`
                // rather than 128+SIGCHLD (empirical, 2026-08-25).
                // Only other signals interrupt wait with 128+sig.
                // `find`, not `rfind`: the self-pipe preserves
                // delivery order, and the FIRST non-CHLD signal is
                // the one that interrupted wait — with several
                // drained together (`kill -USR1; kill -TERM`),
                // bash/dash report 128+first (158), and the first
                // trap action must see that status, not the last
                // signal's (wrap-up review 2026-09-02 round 2
                // finding).
                let interrupt_sig = signals.iter().find(|&&s| s != libc::SIGCHLD).copied();
                // POSIX XCU wait: interrupted by a signal for which
                // a trap is set, wait returns 128+sig — and bash
                // runs the trap only AFTER wait has returned, so
                // the action starts with `$?` already 128+sig and
                // an operandless `exit` inside it propagates that
                // status via trap_context_status (`trap 'exit'
                // TERM; cmd & wait` exits 143, not the pre-wait
                // `$?`; wrap-up review 2026-09-02 round 1 finding).
                // Expose it before running the actions.
                if let Some(sig) = interrupt_sig {
                    self.env.exec.last_exit_status = 128 + sig;
                }
                self.run_signal_traps(&signals);
                if let Some(sig) = interrupt_sig {
                    return PidWait::Interrupted(128 + sig);
                }
            }
            match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
                Ok(WaitStatus::Exited(p, code)) => {
                    self.env
                        .process
                        .jobs
                        .update_status(p, JobStatus::Done(code));
                    return PidWait::Status(code);
                }
                Ok(WaitStatus::Signaled(p, sig, _)) => {
                    self.env
                        .process
                        .jobs
                        .update_status(p, JobStatus::Terminated(sig as i32));
                    return PidWait::Status(128 + sig as i32);
                }
                Ok(WaitStatus::StillAlive) => {
                    // Poll the self-pipe with a short timeout so a signal
                    // arriving mid-wait is noticed promptly. In monitor
                    // mode SIGCHLD is registered on the self-pipe too, so
                    // a child exit also wakes this poll. Readable data is
                    // left in the pipe — the drain at the top of the loop
                    // consumes and handles it on the next iteration.
                    let pipe_fd = signal::self_pipe_read_fd();
                    let mut fds = [nix::poll::PollFd::new(
                        unsafe { std::os::fd::BorrowedFd::borrow_raw(pipe_fd) },
                        nix::poll::PollFlags::POLLIN,
                    )];
                    let _ = nix::poll::poll(&mut fds, nix::poll::PollTimeout::from(50u16));
                }
                Err(nix::errno::Errno::ECHILD) => {
                    // A trap action run mid-wait (e.g. a CHLD trap
                    // calling `wait` itself) may have already reaped
                    // this pid and recorded its status in the jobs
                    // table — report that status instead of an error.
                    let reaped = self
                        .job_table_member_status(pid)
                        .and_then(terminated_wait_status);
                    if let Some(s) = reaped {
                        return PidWait::Status(s);
                    }
                    // Or a notification pass mid-wait may have
                    // reaped AND removed the job — consult the
                    // retained reaped-status map before erroring.
                    if let Some(s) = self.env.process.jobs.reaped_status(pid) {
                        return PidWait::Status(s);
                    }
                    let err = ShellError::runtime(
                        RuntimeErrorKind::CommandNotFound,
                        format!("wait: pid {} is not a child of this shell", pid),
                    );
                    crate::byteenc::write_stderr_decoded_line(&err.to_string());
                    return PidWait::Errored;
                }
                Err(_) | Ok(_) => return PidWait::Status(0),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn wait_job_with_unwaitable_member_reports_127() {
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

        // Waiting a member pid targets the whole job. The leader here is
        // a fake pid this process never spawned: its waitpid errors with
        // nothing remembered, so the JOB is not fully waitable and must
        // report 127 (bash parity; a Done member's 0/4 must not leak
        // through — adversarial review round 3). The member's own Done(4)
        // is still consulted via the fast path (no error for it).
        let status = exec
            .builtin_wait(&["88891".to_string()])
            .expect("wait on a member pid must not hard-error");
        assert_eq!(status, 127);
    }

    #[test]
    fn wait_unknown_jobspec_does_not_abort_later_operands() {
        use crate::env::jobs::JobStatus;
        use nix::unistd::Pid;
        let mut exec = Executor::new("yosh", vec![]);
        let pid = Pid::from_raw(88895);
        let id = exec.env.process.jobs.add_job(pid, vec![pid], "cmd", false);
        exec.env.process.jobs.update_status(pid, JobStatus::Done(5));
        let _ = id;

        // POSIX XCU wait: unknown ids are treated as terminated with 127
        // and the REMAINING operands are still waited; the last operand
        // determines the status (bash: `wait %999 "$p"` prints the error,
        // waits $p, returns its status — empirical 2026-09-02;
        // previously the bad spec aborted the whole builtin with 127).
        let status = exec
            .builtin_wait(&["%999".to_string(), "88895".to_string()])
            .expect("an unknown job spec must not abort wait");
        assert_eq!(status, 5);
    }

    #[test]
    fn wait_rejects_nonpositive_pid_operands() {
        let mut exec = Executor::new("yosh", vec![]);
        // A negative operand must not reach waitpid(2), where it would
        // wait an arbitrary process GROUP (adversarial review round 3;
        // bash rejects it too).
        assert!(exec.builtin_wait(&["-123".to_string()]).is_err());
        assert!(exec.builtin_wait(&["0".to_string()]).is_err());
    }

    #[test]
    fn wait_member_pid_of_live_job_reports_job_status() {
        use crate::env::jobs::JobStatus;
        use nix::unistd::Pid;
        let mut exec = Executor::new("yosh", vec![]);
        let leader = Pid::from_raw(88900);
        let member = Pid::from_raw(88901);
        exec.env
            .process
            .jobs
            .add_job(leader, vec![leader, member], "a | b", false);
        exec.env
            .process
            .jobs
            .update_status(leader, JobStatus::Done(2));
        exec.env
            .process
            .jobs
            .update_status(member, JobStatus::Done(6));

        // A pid naming a member of a LIVE job waits for the whole job
        // and reports the JOB's status — the last member's (bash:
        // `wait $!` on `a | b &` blocks until a finishes too and
        // returns the pipeline status; empirical 2026-09-02).
        let status = exec
            .builtin_wait(&["88900".to_string()])
            .expect("wait on the leader must not error");
        assert_eq!(status, 6);
        let status = exec
            .builtin_wait(&["88901".to_string()])
            .expect("wait on the member must not error");
        assert_eq!(status, 6);
    }

    #[test]
    fn wait_job_status_respects_pipefail() {
        use crate::env::jobs::JobStatus;
        use nix::unistd::Pid;
        let mut exec = Executor::new("yosh", vec![]);
        let leader = Pid::from_raw(88920);
        let member = Pid::from_raw(88921);
        let add_finished_job = |exec: &mut Executor| {
            let jobs = &mut exec.env.process.jobs;
            jobs.add_job(leader, vec![leader, member], "false | true", false);
            jobs.update_status(leader, JobStatus::Done(2));
            jobs.update_status(member, JobStatus::Done(0));
        };
        add_finished_job(&mut exec);

        // Without pipefail: the last member's status.
        let status = exec
            .builtin_wait(&["%1".to_string()])
            .expect("wait %1 must not error");
        assert_eq!(status, 0);

        // The waited-for job left the table (non-interactive shell);
        // set it up again for the pipefail case.
        assert!(exec.env.process.jobs.is_empty());
        add_finished_job(&mut exec);

        // With pipefail: the last NONZERO member status (bash:
        // `set -o pipefail; false | true & wait %1` is 1, empirical
        // 2026-09-02 — pre-fix the direct-fork path returned 0).
        exec.env.mode.options.pipefail = true;
        let status = exec
            .builtin_wait(&["%1".to_string()])
            .expect("wait %1 must not error");
        assert_eq!(status, 2);
    }

    #[test]
    fn wait_reports_member_own_status_after_cleanup() {
        use crate::env::jobs::JobStatus;
        use nix::unistd::Pid;
        let mut exec = Executor::new("yosh", vec![]);
        let leader = Pid::from_raw(88910);
        let member = Pid::from_raw(88911);
        let id = exec
            .env
            .process
            .jobs
            .add_job(leader, vec![leader, member], "a | b", false);
        exec.env
            .process
            .jobs
            .update_status(leader, JobStatus::Done(2));
        exec.env
            .process
            .jobs
            .update_status(member, JobStatus::Done(6));
        exec.env.process.jobs.mark_notified(id);
        exec.env.process.jobs.cleanup_notified();
        assert!(exec.env.process.jobs.get(id).is_none());

        // The reaped-status map now remembers each member's OWN status
        // (previously the aggregate was recorded for every member pid).
        let status = exec
            .builtin_wait(&["88910".to_string()])
            .expect("wait on the remembered leader must not error");
        assert_eq!(status, 2);
        let status = exec
            .builtin_wait(&["88911".to_string()])
            .expect("wait on the remembered member must not error");
        assert_eq!(status, 6);
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
}
