use std::os::unix::io::RawFd;

use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{fork, setpgid, ForkResult, Pid};

use crate::parser::ast::Pipeline;
use crate::signal;

use super::Executor;

impl Executor {
    /// Execute a pipeline.
    pub fn exec_pipeline(&mut self, pipeline: &Pipeline) -> i32 {
        let status = if pipeline.commands.len() == 1 {
            self.exec_command(&pipeline.commands[0])
        } else {
            self.exec_multi_pipeline(pipeline)
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

    fn exec_multi_pipeline(&mut self, pipeline: &Pipeline) -> i32 {
        let n = pipeline.commands.len();
        assert!(n >= 2);

        // Create n-1 pipes: pipes[i] connects command i to command i+1
        // pipes[i].0 = read end, pipes[i].1 = write end
        let mut pipes: Vec<(RawFd, RawFd)> = Vec::with_capacity(n - 1);
        for _ in 0..n - 1 {
            match create_pipe() {
                Ok(fds) => pipes.push(fds),
                Err(e) => {
                    eprintln!("yosh: pipe: {}", e);
                    close_all_pipes(&pipes);
                    return 1;
                }
            }
        }

        let mut children: Vec<Pid> = Vec::with_capacity(n);
        let mut pgid = Pid::from_raw(0);

        for (i, cmd) in pipeline.commands.iter().enumerate() {
            match unsafe { fork() } {
                Err(e) => {
                    eprintln!("yosh: fork: {}", e);
                    close_all_pipes(&pipes);
                    return 1;
                }
                Ok(ForkResult::Child) => {
                    // Set process group
                    let my_pid = nix::unistd::getpid();
                    if i == 0 {
                        setpgid(my_pid, my_pid).ok();
                    } else {
                        setpgid(my_pid, pgid).ok();
                    }
                    let ignored = self.env.traps.ignored_signals();
                    self.env.traps.reset_non_ignored();
                    if self.env.mode.options.monitor {
                        signal::setup_foreground_child_signals(&ignored);
                    } else {
                        signal::reset_child_signals(&ignored);
                    }

                    // Set up stdin from previous pipe's read end (if not first)
                    if i > 0 {
                        let read_fd = pipes[i - 1].0;
                        if unsafe { libc::dup2(read_fd, 0) } == -1 {
                            eprintln!("yosh: dup2: {}", std::io::Error::last_os_error());
                            unsafe { libc::_exit(1) };
                        }
                    }
                    // Set up stdout to next pipe's write end (if not last)
                    if i < n - 1 {
                        let write_fd = pipes[i].1;
                        if unsafe { libc::dup2(write_fd, 1) } == -1 {
                            eprintln!("yosh: dup2: {}", std::io::Error::last_os_error());
                            unsafe { libc::_exit(1) };
                        }
                    }

                    close_all_pipes(&pipes);

                    let status = self.exec_command(cmd);
                    std::process::exit(status);
                }
                Ok(ForkResult::Parent { child }) => {
                    if i == 0 {
                        pgid = child;
                    }
                    setpgid(child, pgid).ok();
                    children.push(child);
                }
            }
        }

        // Parent: close all pipe fds
        close_all_pipes(&pipes);

        if self.env.mode.options.monitor {
            let cmd_str = "(pipeline)".to_string();
            let job_id = self.env.process.jobs.add_job(pgid, children.clone(), cmd_str, true);
            crate::env::jobs::give_terminal(pgid).ok();

            let result = self.wait_for_foreground_job(job_id);

            crate::env::jobs::take_terminal(self.env.process.shell_pgid).ok();

            if result.stopped {
                result.last_status
            } else if self.env.mode.options.pipefail {
                let mut ordered = vec![0i32; n];
                for (pid, code) in &result.process_statuses {
                    if let Some(idx) = children.iter().position(|c| c == pid) {
                        ordered[idx] = *code;
                    }
                }
                ordered.iter().rev().find(|&&s| s != 0).copied().unwrap_or(0)
            } else {
                result.last_status
            }
        } else {
            // Non-monitor mode: simple wait loop (existing behavior)
            let mut last_status = 0;
            let mut max_nonzero = 0;
            for (idx, child) in children.into_iter().enumerate() {
                let status = wait_for_child(child);
                if status != 0 {
                    max_nonzero = status;
                }
                if idx == n - 1 {
                    last_status = status;
                }
            }

            if self.env.mode.options.pipefail {
                max_nonzero
            } else {
                last_status
            }
        }
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
fn wait_for_child(child: Pid) -> i32 {
    match waitpid(child, None) {
        Ok(WaitStatus::Exited(_, code)) => code,
        Ok(WaitStatus::Signaled(_, sig, _)) => 128 + sig as i32,
        Ok(_) => 0,
        Err(e) => {
            eprintln!("yosh: waitpid: {}", e);
            1
        }
    }
}
