use std::os::unix::io::RawFd;

use nix::sys::wait::waitpid;
use nix::unistd::{ForkResult, Pid, setpgid};

use crate::error::{RuntimeErrorKind, ShellError};
use crate::parser::ast::{Command, Pipeline};
use crate::signal;

use super::command::wait_child;
use super::{Executor, block_all_signals, fork_shell, restore_signal_mask, stdin_from_dev_null};

impl Executor {
    /// Execute a pipeline.
    pub(crate) fn exec_pipeline(&mut self, pipeline: &Pipeline) -> i32 {
        let status = if pipeline.commands.len() == 1 {
            self.exec_command(&pipeline.commands[0])
        } else {
            match self.exec_multi_pipeline(pipeline) {
                Ok(s) => s,
                Err(e) => {
                    crate::byteenc::write_stderr_decoded_line(&e.to_string());
                    e.exit_code()
                }
            }
        };

        // Apply negation
        let final_status = if pipeline.negated {
            if status == 0 { 1 } else { 0 }
        } else {
            status
        };

        self.env.exec.last_exit_status = final_status;
        final_status
    }

    fn exec_multi_pipeline(&mut self, pipeline: &Pipeline) -> Result<i32, ShellError> {
        let n = pipeline.commands.len();
        assert!(n >= 2);

        let pipes = create_pipes(n - 1)?;

        let mut children: Vec<Pid> = Vec::with_capacity(n);
        let mut pgid = Pid::from_raw(0);

        for (i, cmd) in pipeline.commands.iter().enumerate() {
            match unsafe { fork_shell() } {
                Err(e) => {
                    close_all_pipes(&pipes);
                    // Mid-pipeline fork failure: terminate and reap the
                    // members forked so far before propagating.
                    kill_and_reap(&children);
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::IoError,
                        format!("fork: {}", e),
                    ));
                }
                Ok(ForkResult::Child) => {
                    super::mark_forked_child();
                    self.env.mode.enter_subshell();
                    // Job control only: put the pipeline in its own process
                    // group. Without monitor mode, members must STAY in the
                    // shell's process group so a terminal-generated SIGINT
                    // (sent to the foreground group) reaches them — the
                    // same guard exec_external_with_redirects applies to
                    // simple commands.
                    if self.env.mode.options.monitor {
                        let my_pid = nix::unistd::getpid();
                        if i == 0 {
                            setpgid(my_pid, my_pid).ok();
                        } else {
                            setpgid(my_pid, pgid).ok();
                        }
                    }
                    let ignored = self.env.traps.ignored_signals();
                    self.env.traps.reset_for_subshell();
                    // Pipeline elements are subshells: the parent's
                    // remembered reaped statuses and terminal table jobs
                    // are not their children.
                    self.env.process.jobs.reset_for_subshell();
                    // Shell-child variants: a pipeline member keeps
                    // running shell code, so the self-pipe is re-created
                    // (traps set inside the member must work) while
                    // parent handlers reset to SIG_DFL per POSIX §2.12.
                    if self.env.mode.options.monitor {
                        signal::setup_foreground_shell_child_signals(&ignored);
                        // A pipeline element is a subshell, not a
                        // job-controlling shell: with monitor left on, a
                        // nested external command would be forked into its
                        // own new process group and the element would call
                        // tcsetpgrp around it. Those calls race with the
                        // parent shell and the other elements for terminal
                        // ownership, and tcsetpgrp from a non-foreground
                        // process group stops the whole pipeline with
                        // SIGTTOU. Nested commands must instead stay in
                        // this pipeline's process group.
                        self.env.mode.options.monitor = false;
                    } else {
                        signal::reset_shell_child_signals(&ignored);
                    }

                    connect_member_to_pipes(&pipes, i);

                    let status = self.exec_command(cmd);
                    // POSIX §2.12: the EXIT trap runs on shell exit,
                    // including subshell exit — and each pipeline member is
                    // a subshell environment. Traps inherited from the
                    // parent were reset above (reset_for_subshell), so this
                    // only fires an EXIT trap set INSIDE this member, e.g.
                    // `{ trap 'echo t' EXIT; :; } | cat`. Bash (5.x) fires
                    // on every member; dash only on the rightmost. We adopt
                    // the bash stance, mirroring exec_subshell's sequence.
                    let status = self.child_exit_status(status);
                    self.execute_exit_trap();
                    super::exit_child(status);
                }
                Ok(ForkResult::Parent { child }) => {
                    if i == 0 {
                        pgid = child;
                    }
                    if self.env.mode.options.monitor {
                        // Race-free group placement: both parent and child
                        // call setpgid. Only under job control (see the
                        // child-side comment above).
                        setpgid(child, pgid).ok();
                    }
                    children.push(child);
                }
            }
        }

        // Parent: close all pipe fds
        close_all_pipes(&pipes);

        if self.env.mode.options.monitor {
            let cmd_str =
                super::preview_pipeline(pipeline).unwrap_or_else(|| "(pipeline)".to_string());
            let job_id = self
                .env
                .process
                .jobs
                .add_job(pgid, children.clone(), cmd_str, true);
            crate::env::jobs::give_terminal(pgid).ok();

            let result = self.wait_for_foreground_job(job_id);

            crate::env::jobs::take_terminal(self.env.process.shell_pgid).ok();
            self.restore_shell_termios_if_interactive();

            if result.stopped {
                Ok(result.last_status)
            } else if self.env.mode.options.pipefail {
                let mut ordered = vec![0i32; n];
                for (pid, code) in &result.process_statuses {
                    if let Some(idx) = children.iter().position(|c| c == pid) {
                        ordered[idx] = *code;
                    }
                }
                Ok(ordered
                    .iter()
                    .rev()
                    .find(|&&s| s != 0)
                    .copied()
                    .unwrap_or(0))
            } else {
                // POSIX: a pipeline's exit status is that of its LAST
                // element. result.last_status is the last-REAPED process,
                // which can be an earlier element — e.g. `cat big | false`,
                // where cat outlives false and exits 0 after it.
                let last_pid = children[n - 1];
                Ok(result
                    .process_statuses
                    .iter()
                    .find(|(pid, _)| *pid == last_pid)
                    .map(|(_, code)| *code)
                    .unwrap_or(result.last_status))
            }
        } else {
            // Non-monitor mode: simple wait loop (existing behavior)
            let mut last_status = 0;
            let mut max_nonzero = 0;
            for (idx, child) in children.into_iter().enumerate() {
                let status = wait_child(child).unwrap_or(1);
                if status != 0 {
                    max_nonzero = status;
                }
                if idx == n - 1 {
                    last_status = status;
                }
            }

            if self.env.mode.options.pipefail {
                Ok(max_nonzero)
            } else {
                Ok(last_status)
            }
        }
    }

    /// Execute a background pipeline (`a | b &`) by forking its members
    /// directly from this shell — no wrapper subshell — so the job table
    /// tracks every member pid: member stops are visible to `jobs`/`bg`,
    /// and a member shaped like a simple command execs an external
    /// utility in place (async exec-in-place extended to pipelines; see
    /// docs/superpowers/specs/2026-09-02-job-member-status-async-pipeline-design.md).
    /// `$!` and the "[n] pid" notice are the LAST member's pid, matching
    /// bash (empirical, bash 3.2 2026-09-02); the job's pgid stays the
    /// first member, which `jobs -p` and `fg`/`bg`/`kill %n` use.
    pub(crate) fn exec_async_pipeline(&mut self, pipeline: &Pipeline) -> Result<i32, ShellError> {
        let n = pipeline.commands.len();
        debug_assert!(n >= 2, "single-command payloads use exec_async's wrapper");

        let pipes = create_pipes(n - 1)?;

        // Block signals across the fork loop: each child inherits the
        // parent's self-pipe handler AND the shared pipe until its own
        // signal setup runs, so a signal delivered in that window would
        // be written into the shared pipe and later misread by the
        // parent as its own (same protection as exec_async).
        let prev_mask = block_all_signals();

        let mut children: Vec<Pid> = Vec::with_capacity(n);
        let mut pgid = Pid::from_raw(0);

        for (i, cmd) in pipeline.commands.iter().enumerate() {
            match unsafe { fork_shell() } {
                Err(e) => {
                    // Mid-pipeline fork failure: terminate and reap the
                    // members forked so far before propagating.
                    kill_and_reap(&children);
                    close_all_pipes(&pipes);
                    restore_signal_mask(&prev_mask);
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::IoError,
                        format!("fork: {}", e),
                    ));
                }
                Ok(ForkResult::Child) => {
                    super::mark_forked_child();
                    self.env.mode.enter_subshell();
                    // Async jobs always run in their own process group,
                    // monitor or not (exec_async parity).
                    let my_pid = nix::unistd::getpid();
                    if i == 0 {
                        setpgid(my_pid, my_pid).ok();
                    } else {
                        setpgid(my_pid, pgid).ok();
                    }
                    let ignored = self.env.traps.ignored_signals();
                    self.env.traps.reset_for_subshell();
                    self.env.process.jobs.reset_for_subshell();
                    if self.env.mode.options.monitor {
                        signal::setup_background_shell_child_signals(&ignored);
                        // A background job is a subshell, not a
                        // job-controlling shell (see exec_async).
                        self.env.mode.options.monitor = false;
                    } else {
                        // POSIX §2.9.3.1 / §2.12 async-list semantics
                        // (see exec_async): SIGINT/SIGQUIT ignored, and
                        // stdin from /dev/null for the pipeline's head —
                        // later members read the pipe instead.
                        self.ignore_int_quit_in_async_child(ignored);
                        if i == 0 {
                            stdin_from_dev_null();
                        }
                    }
                    // Signal dispositions are now the child's own —
                    // deliver anything that arrived while blocked.
                    restore_signal_mask(&prev_mask);

                    connect_member_to_pipes(&pipes, i);

                    // A simple-command member execs external utilities
                    // in place: without this the member subshell would
                    // block in waitpid over a grandchild whose stops the
                    // job table cannot see — the wrapper problem one
                    // level down. Non-simple members (compounds) keep
                    // the subshell semantics.
                    if matches!(cmd, Command::Simple(_)) {
                        self.env.exec.async_exec_in_place = true;
                    }
                    let status = self.exec_command(cmd);
                    // Member EXIT traps fire like foreground pipeline
                    // members' do (bash fires on every member).
                    let status = self.child_exit_status(status);
                    self.execute_exit_trap();
                    super::exit_child(status);
                }
                Ok(ForkResult::Parent { child }) => {
                    if i == 0 {
                        pgid = child;
                    }
                    // Race-free group placement: both sides call setpgid.
                    setpgid(child, pgid).ok();
                    children.push(child);
                }
            }
        }

        restore_signal_mask(&prev_mask);
        close_all_pipes(&pipes);

        let cmd_str =
            super::preview_pipeline(pipeline).unwrap_or_else(|| "(background)".to_string());
        let job_id = self
            .env
            .process
            .jobs
            .add_job(pgid, children.clone(), cmd_str, false);
        // bash parity: `$!` and the notice show the LAST member, not the
        // process-group leader.
        let last = *children.last().expect("pipeline has >= 2 members");
        self.env.process.jobs.set_last_bg_pid(last);
        // POSIX §2.9.3.1: the "[n] pid" notice belongs to job control;
        // plain non-interactive scripts stay silent (bash/dash agree).
        if self.env.mode.is_interactive || self.env.mode.options.monitor {
            crate::byteenc::write_stderr_decoded_line(&format!("[{}] {}", job_id, last.as_raw()));
        }
        Ok(0)
    }
}

/// Create a pipe, returning (read_fd, write_fd) as raw file descriptors.
fn create_pipe() -> Result<(RawFd, RawFd), std::io::Error> {
    let mut fds: [libc::c_int; 2] = [0; 2];
    let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // With stdin/stdout/stderr closed (`exec 0<&-`), pipe() hands out
    // those low numbers. A member's `dup2(pipe_end, 0/1)` is then a no-op
    // and the following `close_all_pipes` closes the very fd it just
    // "installed" (`exec 0<&-; echo hi | cat` → cat: Bad file
    // descriptor). Move such ends above the standard fds.
    for i in 0..2 {
        if fds[i] <= 2 {
            let moved = unsafe { libc::fcntl(fds[i], libc::F_DUPFD, 3) };
            let err = std::io::Error::last_os_error();
            unsafe { libc::close(fds[i]) };
            if moved == -1 {
                // Close the other end before failing.
                unsafe { libc::close(fds[1 - i]) };
                return Err(err);
            }
            fds[i] = moved;
        }
    }
    Ok((fds[0], fds[1]))
}

/// Close all pipe file descriptors.
fn close_all_pipes(pipes: &[(RawFd, RawFd)]) {
    for &(read_fd, write_fd) in pipes {
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
    }
}

/// Create `count` pipes; on failure, close the ones already created.
/// `pipes[i]` connects command i to command i+1 as (read end, write end).
fn create_pipes(count: usize) -> Result<Vec<(RawFd, RawFd)>, ShellError> {
    let mut pipes: Vec<(RawFd, RawFd)> = Vec::with_capacity(count);
    for _ in 0..count {
        match create_pipe() {
            Ok(fds) => pipes.push(fds),
            Err(e) => {
                close_all_pipes(&pipes);
                return Err(ShellError::runtime(
                    RuntimeErrorKind::IoError,
                    format!("pipe: {}", e),
                ));
            }
        }
    }
    Ok(pipes)
}

/// In the forked child for member `i`: read stdin from the previous
/// pipe (unless first), write stdout to the next pipe (unless last),
/// then close every pipe fd. A dup2 failure exits the child with 1.
fn connect_member_to_pipes(pipes: &[(RawFd, RawFd)], i: usize) {
    if i > 0 {
        let read_fd = pipes[i - 1].0;
        if unsafe { libc::dup2(read_fd, 0) } == -1 {
            crate::byteenc::write_stderr_decoded_line(&format!(
                "yosh: dup2: {}",
                std::io::Error::last_os_error()
            ));
            super::exit_child(1);
        }
    }
    if i < pipes.len() {
        let write_fd = pipes[i].1;
        if unsafe { libc::dup2(write_fd, 1) } == -1 {
            crate::byteenc::write_stderr_decoded_line(&format!(
                "yosh: dup2: {}",
                std::io::Error::last_os_error()
            ));
            super::exit_child(1);
        }
    }
    close_all_pipes(pipes);
}

/// Terminate and reap the members forked before a mid-pipeline fork
/// failure. They were never registered with add_job, so reap_zombies'
/// no-live-jobs fast path would otherwise skip waitpid forever, leaving
/// permanent zombies. SIGTERM first for politeness, then SIGKILL so the
/// blocking waitpid can never hang on a member that inherited
/// `trap '' TERM` (Ignore traps survive reset_for_subshell).
fn kill_and_reap(children: &[Pid]) {
    for &child in children {
        let _ = nix::sys::signal::kill(child, nix::sys::signal::Signal::SIGTERM);
        let _ = nix::sys::signal::kill(child, nix::sys::signal::Signal::SIGKILL);
    }
    for &child in children {
        let _ = waitpid(child, None);
    }
}
