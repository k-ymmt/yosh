use std::io::Read;
use std::os::fd::FromRawFd;

use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{ForkResult, fork};

use crate::env::ShellEnv;
use crate::exec::Executor;
use crate::parser::ast::Program;

/// Execute a command substitution and return its stdout output.
/// Forks a child process, captures stdout via a pipe, and returns the
/// output with trailing newlines stripped (POSIX requirement).
pub fn execute(env: &mut ShellEnv, program: &Program) -> String {
    // Create a pipe: fds[0] = read end, fds[1] = write end
    let mut pipe_fds: [libc::c_int; 2] = [0; 2];
    let ret = unsafe { libc::pipe(pipe_fds.as_mut_ptr()) };
    if ret != 0 {
        eprintln!("yosh: pipe: failed to create pipe");
        return String::new();
    }
    let pipe_read = pipe_fds[0];
    let pipe_write = pipe_fds[1];

    match unsafe { fork() } {
        Err(e) => {
            eprintln!("yosh: fork: {}", e);
            unsafe {
                libc::close(pipe_read);
                libc::close(pipe_write);
            }
            String::new()
        }
        Ok(ForkResult::Child) => {
            // Close the read end in the child
            unsafe { libc::close(pipe_read) };

            // Redirect stdout to the write end of the pipe
            unsafe {
                libc::dup2(pipe_write, 1);
                libc::close(pipe_write);
            }

            // Create a new executor with a clone of the parent's environment
            let mut child_env = ShellEnv {
                vars: env.vars.clone(),
                exec: crate::env::ExecState {
                    last_exit_status: env.exec.last_exit_status,
                    flow_control: None,
                },
                process: crate::env::ProcessState {
                    shell_pid: env.process.shell_pid,
                    shell_pgid: env.process.shell_pgid,
                    jobs: env.process.jobs.clone(),
                },
                mode: crate::env::ShellMode {
                    options: env.mode.options.clone(),
                    is_interactive: false,
                    in_dot_script: false,
                },
                shell_name: env.shell_name.clone(),
                functions: env.functions.clone(),
                traps: env.traps.clone(),
                aliases: env.aliases.clone(),
                history: env.history.clone(),
                default_path_cache: env.default_path_cache.clone(),
            };
            child_env.traps.reset_for_command_sub();
            let mut executor = Executor::from_env(child_env);

            let status = executor.exec_program(program);
            crate::exec::exit_child(status);
        }
        Ok(ForkResult::Parent { child }) => {
            // Close the write end in the parent
            unsafe { libc::close(pipe_write) };

            // Read all output from the pipe
            let mut output = String::new();
            // SAFETY: pipe_read is a valid file descriptor opened by pipe()
            let mut file = unsafe { std::fs::File::from_raw_fd(pipe_read) };
            if let Err(e) = file.read_to_string(&mut output) {
                eprintln!("yosh: command substitution: read error: {}", e);
            }

            // Wait for the child to finish
            match waitpid(child, None) {
                Ok(WaitStatus::Exited(_, code)) => {
                    env.exec.last_exit_status = code;
                }
                Ok(WaitStatus::Signaled(_, signal, _)) => {
                    env.exec.last_exit_status = 128 + signal as i32;
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("yosh: waitpid: {}", e);
                }
            }

            // Strip trailing newlines (POSIX requirement)
            while output.ends_with('\n') {
                output.pop();
            }

            output
        }
    }
}
