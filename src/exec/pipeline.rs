use std::os::unix::io::RawFd;

use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{ForkResult, Pid, setpgid};

use crate::error::{RuntimeErrorKind, ShellError};
use crate::parser::ast::{Command, Pipeline};
use crate::signal;

use super::{Executor, fork_shell};

impl Executor {
    /// Execute a pipeline.
    pub(crate) fn exec_pipeline(&mut self, pipeline: &Pipeline) -> i32 {
        let status = if pipeline.commands.len() == 1 {
            self.exec_command(&pipeline.commands[0])
        } else {
            match self.exec_multi_pipeline(pipeline) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{}", e);
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

        // Create n-1 pipes: pipes[i] connects command i to command i+1
        // pipes[i].0 = read end, pipes[i].1 = write end
        let mut pipes: Vec<(RawFd, RawFd)> = Vec::with_capacity(n - 1);
        for _ in 0..n - 1 {
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

        let mut children: Vec<Pid> = Vec::with_capacity(n);
        let mut pgid = Pid::from_raw(0);

        for (i, cmd) in pipeline.commands.iter().enumerate() {
            match unsafe { fork_shell() } {
                Err(e) => {
                    close_all_pipes(&pipes);
                    // A mid-pipeline fork failure leaves the elements
                    // forked so far running but never registered with
                    // add_job — reap_zombies' no-live-jobs fast path
                    // would then skip waitpid forever, leaving permanent
                    // zombies. Terminate and reap them before propagating
                    // the error. SIGTERM first for politeness, then
                    // SIGKILL so the blocking waitpid below can never
                    // hang on a member that inherited `trap '' TERM`
                    // (Ignore traps survive reset_for_subshell).
                    for &child in &children {
                        let _ = nix::sys::signal::kill(child, nix::sys::signal::Signal::SIGTERM);
                        let _ = nix::sys::signal::kill(child, nix::sys::signal::Signal::SIGKILL);
                    }
                    for &child in &children {
                        let _ = waitpid(child, None);
                    }
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::IoError,
                        format!("fork: {}", e),
                    ));
                }
                Ok(ForkResult::Child) => {
                    super::mark_forked_child();
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

                    // Set up stdin from previous pipe's read end (if not first)
                    if i > 0 {
                        let read_fd = pipes[i - 1].0;
                        if unsafe { libc::dup2(read_fd, 0) } == -1 {
                            eprintln!("yosh: dup2: {}", std::io::Error::last_os_error());
                            super::exit_child(1);
                        }
                    }
                    // Set up stdout to next pipe's write end (if not last)
                    if i < n - 1 {
                        let write_fd = pipes[i].1;
                        if unsafe { libc::dup2(write_fd, 1) } == -1 {
                            eprintln!("yosh: dup2: {}", std::io::Error::last_os_error());
                            super::exit_child(1);
                        }
                    }

                    close_all_pipes(&pipes);

                    let status = self.exec_command(cmd);
                    // POSIX §2.12: the EXIT trap runs on shell exit,
                    // including subshell exit — and each pipeline member is
                    // a subshell environment. Traps inherited from the
                    // parent were reset above (reset_for_subshell), so this
                    // only fires an EXIT trap set INSIDE this member, e.g.
                    // `{ trap 'echo t' EXIT; :; } | cat`. Bash (5.x) fires
                    // on every member; dash only on the rightmost. We adopt
                    // the bash stance, mirroring exec_subshell's sequence.
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
                let status = wait_for_child(child).unwrap_or(1);
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

        let mut pipes: Vec<(RawFd, RawFd)> = Vec::with_capacity(n - 1);
        for _ in 0..n - 1 {
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

        // Block signals across the fork loop: each child inherits the
        // parent's self-pipe handler AND the shared pipe until its own
        // signal setup runs, so a signal delivered in that window would
        // be written into the shared pipe and later misread by the
        // parent as its own (same protection as exec_async).
        let all_signals = nix::sys::signal::SigSet::all();
        let prev_mask = nix::sys::signal::SigSet::empty();
        let mut prev_mask_opt = Some(prev_mask);
        let _ = nix::sys::signal::sigprocmask(
            nix::sys::signal::SigmaskHow::SIG_BLOCK,
            Some(&all_signals),
            prev_mask_opt.as_mut(),
        );
        let prev_mask = prev_mask_opt.unwrap();
        let restore_mask = |mask: &nix::sys::signal::SigSet| {
            let _ = nix::sys::signal::sigprocmask(
                nix::sys::signal::SigmaskHow::SIG_SETMASK,
                Some(mask),
                None,
            );
        };

        let mut children: Vec<Pid> = Vec::with_capacity(n);
        let mut pgid = Pid::from_raw(0);

        for (i, cmd) in pipeline.commands.iter().enumerate() {
            match unsafe { fork_shell() } {
                Err(e) => {
                    // Mid-pipeline fork failure: terminate and reap the
                    // members forked so far before propagating (see
                    // exec_multi_pipeline for the SIGTERM+SIGKILL
                    // rationale — Ignore traps survive reset_for_subshell).
                    for &child in &children {
                        let _ = nix::sys::signal::kill(child, nix::sys::signal::Signal::SIGTERM);
                        let _ = nix::sys::signal::kill(child, nix::sys::signal::Signal::SIGKILL);
                    }
                    for &child in &children {
                        let _ = waitpid(child, None);
                    }
                    close_all_pipes(&pipes);
                    restore_mask(&prev_mask);
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::IoError,
                        format!("fork: {}", e),
                    ));
                }
                Ok(ForkResult::Child) => {
                    super::mark_forked_child();
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
                        let mut ignored = ignored;
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
                        if i == 0
                            && let Ok(devnull) = std::fs::File::open("/dev/null")
                        {
                            use std::os::fd::AsRawFd;
                            unsafe {
                                libc::dup2(devnull.as_raw_fd(), libc::STDIN_FILENO);
                            }
                        }
                    }
                    // Signal dispositions are now the child's own —
                    // deliver anything that arrived while blocked.
                    restore_mask(&prev_mask);

                    if i > 0 {
                        let read_fd = pipes[i - 1].0;
                        if unsafe { libc::dup2(read_fd, 0) } == -1 {
                            eprintln!("yosh: dup2: {}", std::io::Error::last_os_error());
                            super::exit_child(1);
                        }
                    }
                    if i < n - 1 {
                        let write_fd = pipes[i].1;
                        if unsafe { libc::dup2(write_fd, 1) } == -1 {
                            eprintln!("yosh: dup2: {}", std::io::Error::last_os_error());
                            super::exit_child(1);
                        }
                    }
                    close_all_pipes(&pipes);

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

        restore_mask(&prev_mask);
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
            eprintln!("[{}] {}", job_id, last.as_raw());
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

/// Wait for a child process and return its exit code.
fn wait_for_child(child: Pid) -> Result<i32, ShellError> {
    match waitpid(child, None) {
        Ok(WaitStatus::Exited(_, code)) => Ok(code),
        Ok(WaitStatus::Signaled(_, sig, _)) => Ok(128 + sig as i32),
        Ok(_) => Ok(0),
        Err(e) => Err(ShellError::runtime(
            RuntimeErrorKind::IoError,
            format!("waitpid: {}", e),
        )),
    }
}
