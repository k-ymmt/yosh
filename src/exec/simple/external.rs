//! Running external utilities (and plugin commands): the posix_spawn
//! fast path, the fork path, and the exec-in-place tail.

use std::ffi::CString;

use nix::errno::Errno;
use nix::unistd::{ForkResult, execve};

use crate::env::jobs;
use crate::error::ShellError;
use crate::parser::ast::{AndOrList, Command, CompleteCommand, SeparatorOp};
use crate::signal;

use super::super::command::wait_child;
use super::super::redirect::{ExpandedRedirect, RedirectState};
use super::super::{Executor, fork_shell};
use super::cstrings::{
    enoexec_sh_fallback, envp_with_overrides, exec_cstrings_or_report, nul_terminated_ptrs,
};
use super::resolve::{ResolvedExec, effective_path_override, resolve_exec_path};
use super::{Invocation, command_line};

impl Executor {
    /// In a forked subshell child whose only remaining work is one
    /// simple command (a foreground pipeline member, `( cmd )`,
    /// `$( cmd )`), arm the one-shot exec-in-place marker so an external
    /// utility replaces this child instead of being spawned as a
    /// grandchild and waited for: one process creation instead of two
    /// per command, as in dash and bash. Builtins, functions and plugin
    /// commands ignore the marker (see `dispatch_simple_command`).
    ///
    /// Not armed while a plugin registers exec hooks: in place, the
    /// command's `post_exec` hook would no longer fire in the child.
    /// The async paths (`exec_async`) arm the marker unconditionally
    /// (documented deviation there).
    pub(crate) fn arm_subshell_exec_in_place(&mut self) {
        if !self.plugins.has_exec_hooks() {
            self.env.exec.async_exec_in_place = true;
        }
    }

    /// [`Self::arm_subshell_exec_in_place`] when `body` — the whole
    /// remaining program of a forked subshell — is a single simple
    /// command: one complete command holding one `;`-terminated (not
    /// `&`) AND-OR list of one non-negated, one-member pipeline. Anything
    /// that runs after the command (a second command, `&&`/`||`, `!`)
    /// keeps the wrapper semantics.
    pub(crate) fn arm_exec_in_place_if_sole_simple(&mut self, body: &[CompleteCommand]) {
        let [cc] = body else { return };
        let [(and_or, sep)] = cc.items.as_slice() else {
            return;
        };
        if !matches!(sep, Some(SeparatorOp::Amp))
            && and_or.rest.is_empty()
            && !and_or.first.negated
            && matches!(and_or.first.commands.as_slice(), [Command::Simple(_)])
        {
            self.arm_subshell_exec_in_place();
        }
    }

    /// `sh -c` tail call: `and_or` is the last list of the command string
    /// (see `Executor::mark_shell_tail`). When it is one plain simple
    /// command and nothing could observe the shell after it, arm the
    /// exec-in-place marker so an external utility replaces the shell
    /// instead of being spawned and waited for — one process creation
    /// and wait fewer per `sh -c 'cmd'`, as dash (EV_EXIT) and bash do.
    ///
    /// Kept as a fork (the marker stays clear) when anything would still
    /// run or be reported after the command: any trap — the EXIT action,
    /// a signal action run after the command, even `trap ''` (dash's
    /// `have_traps` is narrower; this is the conservative choice) — any
    /// job in the table, an interactive shell or monitor mode (job
    /// control and the terminal handoff need the shell), `&&`/`||`/`!`
    /// (the shell still computes the status) and every non-simple
    /// command. Builtins, functions and plugin commands ignore the
    /// marker (see `dispatch_simple_command`), and plugins with exec
    /// hooks never get it armed (`arm_subshell_exec_in_place`). As with
    /// dash, the command then *is* the shell process: a parent sees it
    /// die of a signal itself instead of the shell exiting 128+N.
    pub(crate) fn arm_exec_in_place_if_shell_tail(&mut self, and_or: &AndOrList) {
        if and_or.rest.is_empty()
            && !and_or.first.negated
            && matches!(and_or.first.commands.as_slice(), [Command::Simple(_)])
            && !self.env.mode.is_interactive
            && !self.env.mode.flag_i
            && !self.env.mode.options.monitor
            && self.env.traps.exit_trap.is_none()
            && self.env.traps.signal_traps.is_empty()
            && self.env.process.jobs.is_empty()
            && self.exit_requested.is_none()
            && self.env.exec.flow_control.is_none()
        {
            self.arm_subshell_exec_in_place();
        }
    }

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
                let (trace, env_vars) = self.build_env_vars(inv.assignments)?;
                self.xtrace_simple(&trace, inv.name, inv.args);
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

    /// Resolve and run an external command with its (already expanded)
    /// redirections, waiting for it (see [`Self::spawn_external_at_path`]).
    ///
    /// `env_overrides` are prefix-assignment (name, value) pairs from the
    /// command itself (e.g. `FOO=bar cmd`); they are NOT pre-merged with
    /// the shell's exported environ (see `build_env_vars`). The merge
    /// happens right before the spawn / exec — the common case of no
    /// prefix assignments then uses the cached envp as is.
    ///
    /// The command is resolved to an absolute path in the shell (via
    /// `lookup_in_path`, consulting/populating `utility_hash`) before
    /// spawning, and the child is exec'd on the resolved path instead
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
                    let (c_cmd, c_args) = match exec_cstrings_or_report(&path, cmd, args) {
                        Ok(v) => v,
                        Err(status) => return status,
                    };
                    // Same pre-exec dispositions the forked grandchild
                    // used to get: everything to SIG_DFL except trap-
                    // ignored signals (in the non-monitor async child
                    // `ignored` already carries the SIGINT/SIGQUIT
                    // entries exec_async inserted). The explicit reset
                    // matters for SIG_IGN dispositions, which survive
                    // exec — notably the background shell child's
                    // SIGTTIN ignore: the exec'd command must be
                    // stoppable so the now correctly tracked stop is
                    // visible. The self-pipe fds stay open (CLOEXEC).
                    let ignored = self.env.traps.ignored_signals();
                    signal::reset_child_signals(&ignored);
                    // A signal that arrived before the reset (during
                    // the command's expansion: `sh -c 'cmd $(kill $$)'`)
                    // waits on the self-pipe, which the exec would drop:
                    // act on it now, as the shell would after the
                    // command. Later ones meet SIG_DFL.
                    self.process_pending_signals();
                    if let Some(code) = self.exit_requested {
                        return code;
                    }
                    {
                        // Output the shell buffered (`printf x; cmd` as
                        // an `sh -c` tail) must precede the command's.
                        use std::io::Write;
                        let _ = std::io::stdout().flush();
                        let _ = std::io::stderr().flush();
                    }
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
        let (c_cmd, c_args) = match exec_cstrings_or_report(path, cmd, args) {
            Ok(v) => v,
            Err(status) => return status,
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
                    // Ensure child is in its own process group (race-free: both
                    // parent and child call setpgid).
                    nix::unistd::setpgid(child, child).ok();

                    let job_id = self.env.process.jobs.add_job(
                        child,
                        vec![child],
                        command_line(cmd, args),
                        true,
                    );

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
        {
            // Like `fork_shell`: nothing buffered may be written after
            // the child's output.
            use std::io::Write;
            let _ = std::io::stdout().flush();
            let _ = std::io::stderr().flush();
        }
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
                    super::super::redirect::errno_text(err2)
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
                    super::super::redirect::errno_text(err)
                ));
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
            crate::byteenc::write_stderr_decoded_line(&e.to_string());
            return 1;
        }
        crate::byteenc::write_stderr_decoded_line(msg);
        redirect_state.restore();
        status
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
