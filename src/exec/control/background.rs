//! Asynchronous lists (`cmd &`): forking the background subshell, its
//! signal setup, the parent's job-table entry, and reaping finished
//! background children.

use nix::sys::signal::SigSet;
use nix::unistd::{ForkResult, Pid};

use crate::error::{RuntimeErrorKind, ShellError};
use crate::exec::{
    Executor, block_all_signals, exit_child, fork_shell, preview_command, restore_signal_mask,
    stdin_from_dev_null,
};
use crate::parser::ast::{AndOrList, Command};
use crate::signal;

impl Executor {
    /// Reap any zombie background children without blocking.
    pub(crate) fn reap_zombies(&mut self) {
        use crate::env::jobs::JobStatus;
        use nix::sys::wait::{WaitPidFlag, WaitStatus};
        // Fast path: no live (Running/Stopped) jobs means no child of
        // this shell can be waiting to be reaped here — foreground,
        // pipeline, and command-substitution children are all waited
        // synchronously at their spawn sites. Skips a waitpid(2) on
        // every complete command.
        if !self.env.process.jobs.has_live_jobs() {
            return;
        }
        loop {
            let (pid, status) = match nix::sys::wait::waitpid(
                Pid::from_raw(-1),
                Some(
                    WaitPidFlag::WNOHANG
                        | WaitPidFlag::WUNTRACED
                        // A background job stopped and then resumed by an
                        // external `kill -CONT` must transition back to
                        // Running in `jobs` output (previously it stayed
                        // displayed Stopped forever).
                        | WaitPidFlag::WCONTINUED,
                ),
            ) {
                Ok(WaitStatus::Exited(pid, code)) => (pid, JobStatus::Done(code)),
                Ok(WaitStatus::Signaled(pid, sig, _)) => (pid, JobStatus::Terminated(sig as i32)),
                Ok(WaitStatus::Stopped(pid, sig)) => (pid, JobStatus::Stopped(sig as i32)),
                Ok(WaitStatus::Continued(pid)) => (pid, JobStatus::Running),
                Ok(WaitStatus::StillAlive) => break,
                // Exited/Signaled/Stopped/Continued/StillAlive is
                // exhaustive on macOS (Ptrace* variants are Linux-only);
                // keep a catch-all for cross-platform builds.
                #[allow(unreachable_patterns)]
                Ok(_) => continue,
                Err(_) => break,
            };
            self.env.process.jobs.update_status(pid, status);
        }
    }

    /// Execute a command asynchronously (background with &).
    pub(super) fn exec_async(&mut self, and_or: &AndOrList) -> Result<i32, ShellError> {
        // A bare multi-command pipeline payload forks its members
        // directly from this shell (no wrapper subshell), so the job
        // table tracks every member pid — stops become visible to
        // `jobs`/`bg` and `$!` is the last member (bash parity). Negated
        // pipelines and AND-OR lists keep the wrapper below: their final
        // status is computed by shell code that must outlive the members.
        if and_or.rest.is_empty() && !and_or.first.negated && and_or.first.commands.len() >= 2 {
            return self.exec_async_pipeline(&and_or.first);
        }
        // Block signals across the fork: the child inherits the parent's
        // self-pipe handler AND the shared pipe, so a signal delivered to
        // the child before reset_child_signals runs would be written into
        // the pipe and later misread by the parent as its own (observed as
        // `kill -TERM $!` racing the child's signal reset and terminating
        // the parent shell). Both sides restore the mask below.
        let prev_mask = block_all_signals();
        match unsafe { fork_shell() } {
            Err(e) => {
                restore_signal_mask(&prev_mask);
                Err(ShellError::runtime(
                    RuntimeErrorKind::IoError,
                    format!("fork: {}", e),
                ))
            }
            Ok(ForkResult::Child) => self.run_async_child(and_or, &prev_mask),
            Ok(ForkResult::Parent { child }) => {
                restore_signal_mask(&prev_mask);
                self.register_async_job(child, and_or);
                Ok(0)
            }
        }
    }

    /// Child side of [`Self::exec_async`]: become a background subshell
    /// in its own process group, set up its signals, run `and_or` and
    /// exit with its status (after the EXIT trap). `prev_mask` is the
    /// signal mask from before the fork window was blocked.
    fn run_async_child(&mut self, and_or: &AndOrList, prev_mask: &SigSet) -> ! {
        crate::exec::mark_forked_child();
        self.env.mode.enter_subshell();
        // Set process group BEFORE signal setup to ensure proper isolation.
        let pid = nix::unistd::getpid();
        nix::unistd::setpgid(pid, pid).ok();

        let ignored = self.env.traps.ignored_signals();
        self.env.traps.reset_for_subshell();
        // The async child is a fresh subshell: the parent's
        // remembered reaped statuses and terminal table jobs
        // are not its children.
        self.env.process.jobs.reset_for_subshell();
        // Shell-child variants: an async child keeps running
        // shell code, so the self-pipe is re-created (traps set
        // inside the list must work) while parent handlers reset
        // to SIG_DFL per POSIX §2.12.
        if self.env.mode.options.monitor {
            signal::setup_background_shell_child_signals(&ignored);
            // A background job is a subshell, not a job-controlling
            // shell: with monitor left on, a nested external command
            // would be forked into its own new process group and this
            // (background) subshell would call tcsetpgrp around it,
            // stopping the job with SIGTTOU. Nested commands must
            // stay in this job's process group instead.
            self.env.mode.options.monitor = false;
        } else {
            // POSIX §2.9.3.1 / §2.12: with job control disabled,
            // commands in an asynchronous list ignore SIGINT and
            // SIGQUIT, and read stdin from /dev/null (before any
            // explicit redirection, which happens later during
            // command execution). Record the ignores in the trap
            // store so nested forks (subshells, exec'd commands)
            // keep them ignored; a `trap` in the async list may
            // still override them.
            self.ignore_int_quit_in_async_child(ignored);
            stdin_from_dev_null();
        }
        // Signal dispositions are now the child's own — deliver
        // anything that arrived while the fork window was blocked.
        restore_signal_mask(prev_mask);

        self.mark_async_payload(and_or);

        let status = self.exec_and_or(and_or);
        // POSIX §2.12: an EXIT trap set inside the async list
        // runs when its subshell exits (`{ trap 'echo t' EXIT;
        // :; } &` prints t; dash agrees), like every other
        // forked subshell.
        let status = self.child_exit_status(status);
        self.env.exec.last_exit_status = status;
        self.execute_exit_trap();
        exit_child(status);
    }

    /// In the async child, flag a `&` payload that is one command so it
    /// runs as the subshell's whole body rather than inside a wrapper.
    fn mark_async_payload(&mut self, and_or: &AndOrList) {
        // bash's nofork optimization: a `&` payload shaped like a
        // single simple command may exec in place (no grandchild
        // fork) if it dispatches to an external utility, so the
        // parent's job-table entry — this child's pid — IS the
        // command: stops become visible to job control and the
        // exec'd command's $PPID is the forking shell.
        // exec_simple_command takes the flag at entry and ignores
        // it for builtins/functions, which keep the wrapper-
        // subshell semantics.
        if and_or.rest.is_empty() && !and_or.first.negated && and_or.first.commands.len() == 1 {
            match and_or.first.commands[0] {
                Command::Simple(_) => self.env.exec.async_exec_in_place = true,
                // A `&` payload that is one compound command is
                // this subshell's whole body: its redirections
                // set up the subshell's environment rather than
                // a temporary scope inside it, so they stay
                // applied when the body ends or exits and the
                // EXIT trap writes through them (`{ trap 'echo
                // t' EXIT; exit; } >&2 &` prints t on stderr;
                // dash and bash agree). exec_compound_command
                // takes the flag at entry.
                Command::Compound(..) => self.async_compound_redirects_permanent = true,
                _ => {}
            }
        }
    }

    /// Parent side of [`Self::exec_async`]: record the background child
    /// in the job table and, in an interactive shell, print its notice.
    fn register_async_job(&mut self, child: Pid, and_or: &AndOrList) {
        nix::unistd::setpgid(child, child).ok();
        let command_name = preview_command(and_or);
        let job_id = self
            .env
            .process
            .jobs
            .add_job(child, vec![child], command_name, false);
        // POSIX §2.9.3.1: the "[n] pid" notice is written only by an
        // interactive shell; scripts stay silent, also under `set -m`
        // (bash/dash agree).
        if self.env.mode.is_interactive {
            crate::byteenc::write_stderr_decoded_line(&format!("[{}] {}", job_id, child.as_raw()));
        }
    }

    /// Signal setup of a non-job-control async child (POSIX §2.9.3.1 /
    /// §2.12): SIGINT and SIGQUIT are ignored, recorded in the trap store
    /// so nested forks (subshells, exec'd commands) keep them ignored —
    /// a `trap` in the async list may still override them — and every
    /// other disposition is reset like any shell child's (`ignored`: the
    /// signals already ignored on entry).
    pub(in crate::exec) fn ignore_int_quit_in_async_child(&mut self, mut ignored: Vec<i32>) {
        self.env
            .traps
            .signal_traps
            .insert(libc::SIGINT, crate::env::TrapAction::Ignore);
        self.env
            .traps
            .signal_traps
            .insert(libc::SIGQUIT, crate::env::TrapAction::Ignore);
        ignored.push(libc::SIGINT);
        ignored.push(libc::SIGQUIT);
        signal::reset_shell_child_signals(&ignored);
    }
}
