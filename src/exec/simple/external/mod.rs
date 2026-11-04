//! Running external utilities (and plugin commands): resolving the
//! command, then the spawn / fork paths ([`spawn`]) or the
//! exec-in-place tail ([`in_place`]).

mod in_place;
mod spawn;

use crate::error::ShellError;

use super::super::Executor;
use super::super::redirect::{ExpandedRedirect, RedirectState};
use super::Invocation;
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
    /// the real command (see [`Self::exec_resolved_in_place`]). Never
    /// returns in that case unless resolution fails (127/126 fall through
    /// to the normal wrapper exit).
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
                    return self.exec_resolved_in_place(&path, cmd, args, env_overrides, redirects);
                }
                self.spawn_external_at_path(&path, cmd, args, env_overrides, redirects)
            }
        }
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

/// Flush the shell's buffered stdout and stderr, so output it produced
/// before an external command starts is written before the command's.
fn flush_std_streams() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
}
