use std::ffi::CString;

use nix::unistd::{ForkResult, execve};

use crate::env::jobs;
use crate::error::ShellError;
use crate::signal;

use super::super::command::wait_child;
use super::super::redirect::{ExpandedRedirect, RedirectState};
use super::super::{Executor, fork_shell};
use super::Invocation;
use super::cstrings::{
    ExecCStringError, build_exec_cstrings_for_path, merge_envp, nul_terminated_ptrs,
};
use super::resolve::{ResolvedExec, effective_path_override, resolve_exec_path};

impl Executor {
    /// Run a command that is neither a function nor a built-in: a plugin
    /// command if a plugin claims the name, otherwise an external utility
    /// with the prefix assignments in its environment only.
    pub(super) fn exec_non_builtin_command(
        &mut self,
        inv: &Invocation,
        async_exec_in_place: bool,
    ) -> Result<i32, ShellError> {
        // Check plugin commands before external. The 3-valued
        // return distinguishes "no plugin claimed this name"
        // (fall through to PATH) from "plugin claimed it but
        // failed" (do NOT fall through — the plugin owned it).
        use crate::plugin::PluginExec;
        let status = match self.plugins.exec_command(&mut self.env, inv.name, inv.args) {
            PluginExec::Handled(status) => status,
            PluginExec::Failed => 1,
            PluginExec::NotHandled => {
                let env_vars = self.build_env_vars(inv.assignments)?;
                self.xtrace_simple(&env_vars, inv.name, inv.args);
                // DEVIATION (async exec-in-place, 2026-09-01): plugins'
                // post_exec does not fire for a background external
                // simple command — the async child is replaced by the
                // exec. pre_exec fired above, as it always did in the
                // wrapper. Pre-optimization, post_exec ran inside the
                // forked wrapper whose plugin state was discarded on
                // exit, so only external side effects are lost.
                self.exec_external_with_redirects(
                    inv.name,
                    inv.args,
                    &env_vars,
                    inv.redirs,
                    async_exec_in_place,
                )
            }
        };
        Ok(self.finish_simple_command(inv.hook_cmd, status))
    }

    /// Fork, apply redirects in child, exec the command, wait in parent.
    ///
    /// `env_overrides` are prefix-assignment (name, value) pairs from the
    /// command itself (e.g. `FOO=bar cmd`); they are NOT pre-merged with
    /// the shell's exported environ (see `build_env_vars`). The merge
    /// happens here, in the child, right before `setenv` — the common
    /// case of no prefix assignments then never touches `environ()`'s
    /// cached Vec at all beyond the borrow needed to iterate it.
    ///
    /// The command is resolved to an absolute path in the parent (via
    /// `find_in_path`, consulting/populating `utility_hash`) before
    /// forking, and the child calls `execv` on the resolved path instead
    /// of `execvp` — avoiding a redundant PATH re-walk in the child.
    /// `cmd` containing `/` bypasses PATH search entirely (POSIX exec
    /// semantics), matching `execvp`'s own behavior.
    ///
    /// A `PATH` prefix assignment in `env_overrides` (e.g. `PATH=/custom
    /// cmd`) is honored for this resolution too — it takes precedence
    /// over the shell's own `$PATH`, matching what a setenv'd child's
    /// `execvp` would have searched pre-branch (see `effective_path_override`).
    /// With `in_place` (async exec-in-place, see `exec_async`), the
    /// resolved command is exec'd directly in the current process — this
    /// process is already the forked async child in its own process
    /// group, so replacing it makes the parent's job-table entry track
    /// the real command. Never returns in that case unless resolution
    /// fails (127/126 fall through to the normal wrapper exit).
    pub(crate) fn exec_external_with_redirects(
        &mut self,
        cmd: &str,
        args: &[String],
        env_overrides: &[(String, String)],
        redirects: &[ExpandedRedirect],
        in_place: bool,
    ) -> i32 {
        let effective_path = effective_path_override(env_overrides);
        let resolved = resolve_exec_path(
            cmd,
            &self.env.vars,
            effective_path,
            &mut self.env.utility_hash,
        );

        match resolved {
            // No executable file candidate exists anywhere in the
            // search — nothing to fork/exec. Matches the exit code
            // `execvp`'s ENOENT branch previously produced.
            ResolvedExec::NotFound => self.diagnose_with_redirects(
                redirects,
                &format!("yosh: {}: command not found", cmd),
                127,
            ),
            ResolvedExec::NotExecutable => self.diagnose_with_redirects(
                redirects,
                &format!("yosh: {}: permission denied", cmd),
                126,
            ),
            ResolvedExec::Executable(path) => {
                if in_place {
                    let (c_cmd, c_args) = match build_exec_cstrings_for_path(&path, cmd, args) {
                        Ok(v) => v,
                        Err(ExecCStringError::CommandName) => {
                            eprintln!("yosh: {}: invalid command name", cmd);
                            return 127;
                        }
                        Err(ExecCStringError::Argument(arg)) => {
                            eprintln!("yosh: {}: invalid argument", arg);
                            return 1;
                        }
                    };
                    // Same pre-exec dispositions the forked grandchild
                    // used to get: everything to SIG_DFL except trap-
                    // ignored signals (in the non-monitor async child
                    // `ignored` already carries the SIGINT/SIGQUIT
                    // entries exec_async inserted), and the inherited
                    // self-pipe fds closed. The explicit reset matters
                    // for SIG_IGN dispositions, which survive exec —
                    // notably the background shell child's SIGTTIN
                    // ignore: the exec'd command must be stoppable so
                    // the now correctly tracked stop is visible.
                    let ignored = self.env.traps.ignored_signals();
                    signal::reset_child_signals(&ignored);
                    self.exec_child_prepared(&c_cmd, &c_args, cmd, env_overrides, redirects);
                }
                self.spawn_external_at_path(&path, cmd, args, env_overrides, redirects)
            }
        }
    }

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
        let (c_cmd, c_args) = match build_exec_cstrings_for_path(path, cmd, args) {
            Ok(v) => v,
            Err(ExecCStringError::CommandName) => {
                eprintln!("yosh: {}: invalid command name", cmd);
                return 127;
            }
            Err(ExecCStringError::Argument(arg)) => {
                eprintln!("yosh: {}: invalid argument", arg);
                return 1;
            }
        };

        let monitor = self.env.mode.options.monitor;
        let shell_pgid = self.env.process.shell_pgid;
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
                eprintln!("yosh: fork: {}", e);
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
                    // Ensure child is in its own process group (race-free: both
                    // parent and child call setpgid).
                    nix::unistd::setpgid(child, child).ok();

                    let full_cmd = std::iter::once(cmd)
                        .chain(args.iter().map(|s| s.as_str()))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let job_id = self
                        .env
                        .process
                        .jobs
                        .add_job(child, vec![child], full_cmd, true);

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
                } else {
                    wait_child(child).unwrap_or(1)
                }
            }
        }
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
        use nix::errno::Errno;
        {
            // Like `fork_shell`: nothing buffered may be written after
            // the child's output.
            use std::io::Write;
            let _ = std::io::stdout().flush();
            let _ = std::io::stderr().flush();
        }
        let base = self.env.vars.envp();
        let merged;
        let envp: &[CString] = if env_overrides.is_empty() {
            base
        } else {
            merged = merge_envp(base, env_overrides);
            &merged
        };
        let envp_ptrs = nul_terminated_ptrs(envp);

        let spawn = |path: &CString, argv: &[CString]| -> Result<nix::unistd::Pid, Errno> {
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
                    rc = libc::posix_spawnattr_setflags(
                        attr,
                        libc::POSIX_SPAWN_SETSIGDEF as libc::c_short,
                    );
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
        };

        let child = match spawn(c_cmd, c_args) {
            Ok(pid) => pid,
            Err(Errno::ENOEXEC) => {
                // POSIX execvp fallback (see `exec_child_prepared`):
                // run the file as a script with /bin/sh.
                let sh = CString::new("/bin/sh").expect("/bin/sh has no NUL");
                let mut sh_args = Vec::with_capacity(c_args.len() + 1);
                sh_args.push(sh.clone());
                sh_args.push(c_cmd.clone());
                sh_args.extend_from_slice(&c_args[1..]);
                spawn(&sh, &sh_args).ok()?
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
    /// and the async exec-in-place path in
    /// [`Self::exec_external_with_redirects`], where the current process
    /// is the forked async child itself.
    fn exec_child_prepared(
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
            eprintln!("{}", e);
            crate::exec::exit_child(1);
        }

        // Set environment variables using libc::setenv directly.
        // SAFETY: single-threaded child after fork (or the async child
        // about to exec). We must NOT use std::env::set_var here because
        // it acquires Rust's internal ENV_LOCK (RwLock). If another
        // thread in the parent held that lock at fork() time, the child
        // inherits the locked state and deadlocks — the lock holder
        // thread does not exist in the child.
        //
        // The exported environ is applied first, then `env_overrides`
        // (command prefix assignments) on top, so overrides win —
        // matching the previous merged-Vec's replace-or-push order.
        // Names/values are byteenc-encoded; decode so children see
        // the original raw bytes for non-UTF-8 environment data.
        // Exec with a prebuilt envp (cached in the parent across spawns)
        // instead of `setenv` per exported variable after the fork.
        let base = self.env.vars.envp();
        let merged;
        let envp: &[CString] = if env_overrides.is_empty() {
            base
        } else {
            merged = merge_envp(base, env_overrides);
            &merged
        };

        let err = execve(c_cmd, c_args, envp).unwrap_err();
        use nix::errno::Errno;
        let exit_code = match err {
            Errno::ENOENT => {
                eprintln!("yosh: {}: command not found", cmd);
                127
            }
            Errno::EACCES => {
                eprintln!("yosh: {}: permission denied", cmd);
                126
            }
            Errno::ENOEXEC => {
                // POSIX execvp fallback that execv does not do
                // for us: an executable file the kernel refuses
                // to exec (no `#!` line / unrecognized format)
                // is re-run as a shell script:
                //   /bin/sh <resolved-path> <args...>
                // c_cmd is the resolved path; c_args[0] is the
                // display-name argv[0], replaced by the script
                // path in the sh invocation (matching libc
                // execvp, which passes the *file* to sh).
                match CString::new("/bin/sh") {
                    Ok(sh) => {
                        let mut sh_args = Vec::with_capacity(c_args.len() + 1);
                        sh_args.push(sh.clone());
                        sh_args.push(c_cmd.clone());
                        sh_args.extend_from_slice(&c_args[1..]);
                        let err2 = execve(&sh, &sh_args, envp).unwrap_err();
                        eprintln!("yosh: {}: {}", cmd, err2);
                    }
                    Err(_) => unreachable!("/bin/sh has no NUL"),
                }
                126
            }
            // Found but not executable for another reason (E2BIG,
            // ETXTBSY, ENOMEM, ...): POSIX 2.8.2 status 126, as in
            // dash and bash. Only ENOENT means "not found" (127).
            _ => {
                eprintln!("yosh: {}: {}", cmd, err);
                126
            }
        };
        crate::exec::exit_child(exit_code);
    }

    /// Print a pre-fork diagnostic (`command not found` / `permission
    /// denied`) with the command's redirections in effect, as if the
    /// command had run: `nonexist 2>/dev/null` is silent and
    /// `x=$(nonexist 2>&1)` captures the message (dash/bash agree).
    /// The redirections are also *performed* (`nonexist 2>err` creates
    /// `err`). A redirection failure takes precedence (status 1).
    fn diagnose_with_redirects(
        &mut self,
        redirects: &[ExpandedRedirect],
        msg: &str,
        status: i32,
    ) -> i32 {
        let mut redirect_state = RedirectState::new();
        if let Err(e) = redirect_state.apply_expanded(redirects, true) {
            eprintln!("{}", e);
            return 1;
        }
        crate::byteenc::write_stderr_decoded_line(msg);
        redirect_state.restore();
        status
    }
}
