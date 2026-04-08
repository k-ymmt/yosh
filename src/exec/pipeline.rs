use std::os::unix::io::RawFd;

use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{fork, ForkResult, Pid};

use crate::parser::ast::Pipeline;

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

        self.env.last_exit_status = final_status;
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
                    eprintln!("kish: pipe: {}", e);
                    close_all_pipes(&pipes);
                    return 1;
                }
            }
        }

        let mut children: Vec<Pid> = Vec::with_capacity(n);

        for (i, cmd) in pipeline.commands.iter().enumerate() {
            match unsafe { fork() } {
                Err(e) => {
                    eprintln!("kish: fork: {}", e);
                    // Close all remaining pipes and return error
                    close_all_pipes(&pipes);
                    return 1;
                }
                Ok(ForkResult::Child) => {
                    // Set up stdin from previous pipe's read end (if not first)
                    if i > 0 {
                        let read_fd = pipes[i - 1].0;
                        if unsafe { libc::dup2(read_fd, 0) } == -1 {
                            eprintln!("kish: dup2: {}", std::io::Error::last_os_error());
                            unsafe { libc::_exit(1) };
                        }
                    }
                    // Set up stdout to next pipe's write end (if not last)
                    if i < n - 1 {
                        let write_fd = pipes[i].1;
                        if unsafe { libc::dup2(write_fd, 1) } == -1 {
                            eprintln!("kish: dup2: {}", std::io::Error::last_os_error());
                            unsafe { libc::_exit(1) };
                        }
                    }

                    // Close all pipe fds in child
                    close_all_pipes(&pipes);

                    let status = self.exec_command(cmd);
                    std::process::exit(status);
                }
                Ok(ForkResult::Parent { child }) => {
                    children.push(child);
                }
            }
        }

        // Parent: close all pipe fds
        close_all_pipes(&pipes);

        // Parent: wait for all children, return last child's exit status
        // POSIX: pipeline exit status = exit status of last command
        let mut last_status = 0;
        for (idx, child) in children.into_iter().enumerate() {
            let status = wait_for_child(child);
            if idx == n - 1 {
                last_status = status;
            }
        }

        last_status
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
            eprintln!("kish: waitpid: {}", e);
            1
        }
    }
}
