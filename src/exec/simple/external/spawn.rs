//! Starting an already-resolved external utility as a child process:
//! the posix_spawn fast path, the fork path, and the child-side exec
//! tail shared with exec-in-place.

use std::ffi::CString;

use nix::errno::Errno;
use nix::unistd::{ForkResult, execve};

use crate::env::jobs;
use crate::exec::command::wait_child;
use crate::exec::redirect::{ExpandedRedirect, RedirectState, errno_text};
use crate::exec::{Executor, fork_shell};
use crate::signal;

use super::super::command_line;
use super::super::cstrings::{
    enoexec_sh_fallback, envp_with_overrides, exec_cstrings_or_report, nul_terminated_ptrs,
};
use super::flush_std_streams;

impl Executor {
    /// Fork and exec an already-resolved external path with the full
    /// standard child setup: monitor-mode process-group placement and
    /// terminal handoff, trap-aware signal reset, redirect application,
    /// environment merge, and parent-side job-table bookkeeping /
    /// foreground wait. `display_name` becomes the child's argv[0].
    ///
    /// Shared by [`Self::exec_external_with_redirects`] (which resolves
    /// the name via `$PATH` first) and the `command` builtin execution
    /// paths (which resolve via `$PATH` or the POSIX default PATH
    /// themselves) — `command sleep 30` must be suspendable with Ctrl+Z
    /// exactly like `sleep 30`, so both must fork through this one path.
    pub(crate) fn spawn_external_at_path(
        &mut self,
        path: &std::path::Path,
        display_name: &str,
        args: &[String],
        env_overrides: &[(String, String)],
        redirects: &[ExpandedRedirect],
    ) -> i32 {
        let cmd = display_name;
        let (c_cmd, c_args) = match exec_cstrings_or_report(path, cmd, args) {
            Ok(v) => v,
            Err(status) => return status,
        };

        let monitor = self.env.mode.options.monitor;
        let ignored = self.env.traps.ignored_signals();

        // Plain case (no job control, no redirections): posix_spawn is
        // ~0.4 ms cheaper per command than fork+execve on macOS and the
        // child starts in the same state (see `spawn_plain`).
        if !monitor
            && redirects.is_empty()
            && let Some(sigdef) = signal::spawn_default_sigset(&ignored)
            && let Some(status) = self.spawn_plain(&c_cmd, &c_args, env_overrides, &sigdef)
        {
            return status;
        }

        match unsafe { fork_shell() } {
            Err(e) => {
                crate::byteenc::write_stderr_decoded_line(&format!("yosh: fork: {}", e));
                1
            }
            Ok(ForkResult::Child) => {
                crate::exec::mark_forked_child();
                // In monitor mode: put child in its own process group so Ctrl+Z
                // (SIGTSTP) stops only the foreground job, not the shell.
                if monitor {
                    let my_pid = nix::unistd::getpid();
                    nix::unistd::setpgid(my_pid, my_pid).ok();
                    signal::setup_foreground_child_signals(&ignored);
                } else {
                    signal::reset_child_signals(&ignored);
                }

                self.exec_child_prepared(&c_cmd, &c_args, cmd, env_overrides, redirects);
            }
            Ok(ForkResult::Parent { child }) => {
                if monitor {
                    self.wait_foreground_child(child, command_line(cmd, args))
                } else {
                    wait_child(child).unwrap_or(1)
                }
            }
        }
    }

    /// Parent side of the monitor-mode fork path: register `child` as a
    /// foreground job, hand it the terminal and wait for it to finish or
    /// stop, then take the terminal back. Returns the job's status.
    fn wait_foreground_child(&mut self, child: nix::unistd::Pid, command_text: String) -> i32 {
        let shell_pgid = self.env.process.shell_pgid;
        // Ensure child is in its own process group (race-free: both
        // parent and child call setpgid).
        nix::unistd::setpgid(child, child).ok();

        let job_id = self
            .env
            .process
            .jobs
            .add_job(child, vec![child], command_text, true);

        // Hand terminal to the child's process group.
        jobs::give_terminal(child).ok();

        let result = self.wait_for_foreground_job(job_id);

        // Take terminal back for the shell.
        jobs::take_terminal(shell_pgid).ok();

        // Restore the shell's termios after any foreground
        // completion (stopped or exited) — a crashed or
        // suspended TUI may have left the terminal in raw mode.
        self.restore_shell_termios_if_interactive();

        result.last_status
    }

    /// `posix_spawn` an external command and wait for it: the no-fork
    /// equivalent of the non-monitor, redirect-free fork path. The child
    /// gets the same argv/envp, the inherited signal mask, `sigdef`
    /// signals at SIG_DFL (see [`signal::spawn_default_sigset`]) and the
    /// shell's fds (the self-pipe is CLOEXEC).
    ///
    /// Returns `None` when the spawn itself fails (ENOENT, EACCES,
    /// E2BIG, EAGAIN, ...): the caller then takes the fork path, so the
    /// diagnostic, the exit status and the SIGCHLD all come from a
    /// failed-exec child exactly as before. Reporting in the shell
    /// process instead would let a closed stderr pipe SIGPIPE-kill the
    /// shell and would skip a `trap ... CHLD`.
    fn spawn_plain(
        &mut self,
        c_cmd: &CString,
        c_args: &[CString],
        env_overrides: &[(String, String)],
        sigdef: &libc::sigset_t,
    ) -> Option<i32> {
        // Like `fork_shell`: nothing buffered may be written after the
        // child's output.
        flush_std_streams();
        let envp = envp_with_overrides(self.env.vars.envp(), env_overrides);
        let envp_ptrs = nul_terminated_ptrs(&envp);

        let child = match posix_spawn_sigdef(c_cmd, c_args, &envp_ptrs, sigdef) {
            Ok(pid) => pid,
            Err(Errno::ENOEXEC) => {
                // POSIX execvp fallback (see `enoexec_sh_fallback`):
                // run the file as a script with /bin/sh.
                let (sh, sh_args) = enoexec_sh_fallback(c_cmd, c_args);
                posix_spawn_sigdef(&sh, &sh_args, &envp_ptrs, sigdef).ok()?
            }
            Err(_) => return None,
        };
        Some(wait_child(child).unwrap_or(1))
    }

    /// Child-side / in-place tail of external execution: apply redirects,
    /// merge the exported environ with prefix-assignment overrides, and
    /// `execv` the already-resolved command (with the POSIX ENOEXEC
    /// `/bin/sh` fallback). Never returns — on failure the process exits
    /// with the classic codes (redirect failure 1, ENOENT 127,
    /// EACCES/ENOEXEC 126).
    ///
    /// Callers do the fork-specific setup first (process group, signal
    /// dispositions): the fork path in [`Self::spawn_external_at_path`],
    /// and the exec-in-place path in [`Self::exec_resolved_in_place`],
    /// where the current process is the forked child itself.
    pub(super) fn exec_child_prepared(
        &mut self,
        c_cmd: &CString,
        c_args: &[CString],
        cmd: &str,
        env_overrides: &[(String, String)],
        redirects: &[ExpandedRedirect],
    ) -> ! {
        // Apply redirects (no need to save — this process is replaced)
        let mut redir_state = RedirectState::new();
        if let Err(e) = redir_state.apply_expanded(redirects, false) {
            crate::byteenc::write_stderr_decoded_line(&e.to_string());
            crate::exec::exit_child(1);
        }

        // Exec with the prebuilt exported envp (cached in the parent
        // across spawns, names/values decoded from byteenc), with the
        // command's prefix assignments overlaid so they win. Never
        // `std::env::set_var` here: it takes Rust's internal ENV_LOCK,
        // which another parent thread may have held at fork() time —
        // the child would deadlock on it.
        let envp = envp_with_overrides(self.env.vars.envp(), env_overrides);

        let err = execve(c_cmd, c_args, &envp).unwrap_err();
        let exit_code = match err {
            Errno::ENOENT => {
                crate::byteenc::write_stderr_decoded_line(&format!(
                    "yosh: {}: command not found",
                    cmd
                ));
                127
            }
            Errno::EACCES => {
                crate::byteenc::write_stderr_decoded_line(&format!(
                    "yosh: {}: permission denied",
                    cmd
                ));
                126
            }
            Errno::ENOEXEC => {
                // POSIX execvp fallback (see `enoexec_sh_fallback`).
                let (sh, sh_args) = enoexec_sh_fallback(c_cmd, c_args);
                let err2 = execve(&sh, &sh_args, &envp).unwrap_err();
                crate::byteenc::write_stderr_decoded_line(&format!(
                    "yosh: {}: {}",
                    cmd,
                    errno_text(err2)
                ));
                126
            }
            // Found but not executable for another reason (E2BIG,
            // ETXTBSY, ENOMEM, ...): POSIX 2.8.2 status 126, as in
            // dash and bash. Only ENOENT means "not found" (127).
            _ => {
                crate::byteenc::write_stderr_decoded_line(&format!(
                    "yosh: {}: {}",
                    cmd,
                    errno_text(err)
                ));
                126
            }
        };
        crate::exec::exit_child(exit_code);
    }
}

/// `posix_spawn` `path` with `argv` and the NUL-terminated `envp_ptrs`,
/// resetting the `sigdef` signals to SIG_DFL in the child.
fn posix_spawn_sigdef(
    path: &CString,
    argv: &[CString],
    envp_ptrs: &[*mut libc::c_char],
    sigdef: &libc::sigset_t,
) -> Result<nix::unistd::Pid, Errno> {
    let argv_ptrs = nul_terminated_ptrs(argv);
    let mut attr = std::mem::MaybeUninit::<libc::posix_spawnattr_t>::uninit();
    // SAFETY: attr is initialized by posix_spawnattr_init before
    // use and destroyed after; every pointer array is
    // NUL-terminated and outlives the call.
    unsafe {
        if libc::posix_spawnattr_init(attr.as_mut_ptr()) != 0 {
            return Err(Errno::last());
        }
        let attr = attr.assume_init_mut();
        let mut rc = libc::posix_spawnattr_setsigdefault(attr, sigdef);
        if rc == 0 {
            rc = libc::posix_spawnattr_setflags(attr, libc::POSIX_SPAWN_SETSIGDEF as libc::c_short);
        }
        let mut pid: libc::pid_t = 0;
        if rc == 0 {
            rc = libc::posix_spawn(
                &mut pid,
                path.as_ptr(),
                std::ptr::null(),
                attr,
                argv_ptrs.as_ptr(),
                envp_ptrs.as_ptr(),
            );
        }
        libc::posix_spawnattr_destroy(attr);
        if rc == 0 {
            Ok(nix::unistd::Pid::from_raw(pid))
        } else {
            Err(Errno::from_raw(rc))
        }
    }
}
