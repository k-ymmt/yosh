use std::ffi::CString;

use nix::unistd::{execvp, fork, ForkResult};

use crate::builtin::{classify_builtin, exec_regular_builtin, BuiltinKind};
use crate::builtin::special::exec_special_builtin;
use crate::env::jobs;
use crate::error::{ShellError, RuntimeErrorKind};
use crate::expand::expand_words;
use crate::parser::ast::{Assignment, SimpleCommand};
use crate::signal;

use super::command::wait_child;
use super::redirect::RedirectState;
use super::Executor;

impl Executor {
    /// Execute a simple command (assignments, builtins, or external programs).
    pub(crate) fn exec_simple_command(&mut self, cmd: &SimpleCommand) -> Result<i32, ShellError> {
        // Expand all words
        let expanded = match expand_words(&mut self.env, &cmd.words) {
            Ok(words) => words,
            Err(e) => {
                self.env.exec.last_exit_status = 1;
                return Err(e);
            }
        };

        // Check if expansion triggered a flow control signal (e.g., nounset error)
        if self.env.exec.flow_control.is_some() {
            self.env.exec.last_exit_status = 1;
            return Ok(1);
        }

        // Assignment-only command (no words)
        if expanded.is_empty() {
            // Track the exit status from any command substitutions in the assignments.
            // POSIX: the exit status of an assignment-only command is the exit status
            // of the last command substitution performed during expansion.
            let mut last_cmd_sub_status = 0i32;
            for assignment in &cmd.assignments {
                // Reset before each expansion so we can capture per-assignment status
                self.env.exec.last_exit_status = 0;
                let value = match assignment.value.as_ref() {
                    Some(w) => match crate::expand::expand_word_to_string(&mut self.env, w) {
                        Ok(v) => v,
                        Err(e) => {
                            self.env.exec.last_exit_status = 1;
                            return Err(e);
                        }
                    },
                    None => String::new(),
                };
                // Capture the status set by any command substitution during expansion
                last_cmd_sub_status = self.env.exec.last_exit_status;
                if let Err(e) = self.env.vars.set_with_options(&assignment.name, value, self.env.mode.options.allexport) {
                    self.env.exec.last_exit_status = 1;
                    return Err(ShellError::runtime(RuntimeErrorKind::ReadonlyVariable, format!("{}", e)));
                }
            }
            self.env.exec.last_exit_status = last_cmd_sub_status;
            return Ok(last_cmd_sub_status);
        }

        if self.env.mode.options.xtrace && !expanded.is_empty() {
            eprintln!("+ {}", expanded.join(" "));
        }

        let mut expanded_iter = expanded.into_iter();
        let command_name = expanded_iter.next().unwrap();
        let args: Vec<String> = expanded_iter.collect();

        // Build command string for hooks
        let cmd_str_for_hooks = std::iter::once(command_name.as_str())
            .chain(args.iter().map(|s| s.as_str()))
            .collect::<Vec<_>>()
            .join(" ");
        self.plugins.call_pre_exec(&mut self.env, &cmd_str_for_hooks);

        // Check for function call (before builtins, matching POSIX lookup order)
        if let Some(func_def) = self.env.functions.get(&command_name).cloned() {
            let saved = self.apply_temp_assignments(&cmd.assignments).map_err(|e| {
                self.env.exec.last_exit_status = 1;
                e
            })?;
            let mut redirect_state = RedirectState::new();
            if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                self.restore_assignments(saved);
                self.env.exec.last_exit_status = 1;
                return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
            }
            let status = self.exec_function_call(&func_def, &args);
            redirect_state.restore();
            self.restore_assignments(saved);
            self.plugins.call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
            self.env.exec.last_exit_status = status;
            return Ok(status);
        }

        // wait needs Executor access (bg_jobs + signal processing)
        if command_name == "wait" {
            let saved = self.apply_temp_assignments(&cmd.assignments).map_err(|e| {
                self.env.exec.last_exit_status = 1;
                e
            })?;
            let mut redirect_state = RedirectState::new();
            if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                self.restore_assignments(saved);
                self.env.exec.last_exit_status = 1;
                return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
            }
            let status = self.builtin_wait(&args).unwrap_or_else(|e| { eprintln!("{}", e); e.exit_code() });
            redirect_state.restore();
            self.restore_assignments(saved);
            self.plugins.call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
            self.env.exec.last_exit_status = status;
            return Ok(status);
        }

        // fg/bg/jobs need Executor access for job table + terminal control
        if command_name == "fg" || command_name == "bg" || command_name == "jobs" {
            let saved = self.apply_temp_assignments(&cmd.assignments).map_err(|e| {
                self.env.exec.last_exit_status = 1;
                e
            })?;
            let mut redirect_state = RedirectState::new();
            if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                self.restore_assignments(saved);
                self.env.exec.last_exit_status = 1;
                return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
            }
            let status = match command_name.as_str() {
                "fg" => self.builtin_fg(&args),
                "bg" => self.builtin_bg(&args),
                "jobs" => self.builtin_jobs(&args),
                _ => unreachable!(),
            }.unwrap_or_else(|e| { eprintln!("{}", e); e.exit_code() });
            redirect_state.restore();
            self.restore_assignments(saved);
            self.plugins.call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
            self.env.exec.last_exit_status = status;
            return Ok(status);
        }

        match classify_builtin(&command_name) {
            BuiltinKind::Special => {
                // Special builtins: prefix assignments persist in current env
                for assignment in &cmd.assignments {
                    let value = match assignment.value.as_ref() {
                        Some(w) => match crate::expand::expand_word_to_string(&mut self.env, w) {
                            Ok(v) => v,
                            Err(e) => {
                                self.env.exec.last_exit_status = 1;
                                return Err(e);
                            }
                        },
                        None => String::new(),
                    };
                    if let Err(e) = self.env.vars.set_with_options(&assignment.name, value, self.env.mode.options.allexport) {
                        self.env.exec.last_exit_status = 1;
                        return Err(ShellError::runtime(RuntimeErrorKind::ReadonlyVariable, format!("{}", e)));
                    }
                }
                // exec with no args: redirects persist (don't save/restore)
                if command_name == "exec" && args.is_empty() {
                    let mut redirect_state = RedirectState::new();
                    if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, false) {
                        self.env.exec.last_exit_status = 1;
                        return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
                    }
                    self.env.exec.last_exit_status = 0;
                    return Ok(0);
                }

                let mut redirect_state = RedirectState::new();
                if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                    self.env.exec.last_exit_status = 1;
                    return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
                }
                let status = exec_special_builtin(&command_name, &args, self);
                redirect_state.restore();
                self.plugins.call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
                self.env.exec.last_exit_status = status;
                Ok(status)
            }
            BuiltinKind::Regular => {
                // Regular builtins: prefix assignments are temporary
                let saved = self.apply_temp_assignments(&cmd.assignments).map_err(|e| {
                    self.env.exec.last_exit_status = 1;
                    e
                })?;
                let mut redirect_state = RedirectState::new();
                if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                    self.restore_assignments(saved);
                    self.env.exec.last_exit_status = 1;
                    return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
                }
                let old_pwd = if command_name == "cd" {
                    self.env.vars.get("PWD").map(|s| s.to_string())
                } else {
                    None
                };
                let status = exec_regular_builtin(&command_name, &args, &mut self.env);
                redirect_state.restore();
                self.restore_assignments(saved);
                self.plugins.call_post_exec(&mut self.env, &cmd_str_for_hooks, status);

                // on_cd hook: fire if cd succeeded
                if command_name == "cd" && status == 0 {
                    let old = old_pwd.unwrap_or_default();
                    let new_dir = self.env.vars.get("PWD").unwrap_or("").to_string();
                    self.plugins.call_on_cd(&mut self.env, &old, &new_dir);
                }

                self.env.exec.last_exit_status = status;
                Ok(status)
            }
            BuiltinKind::NotBuiltin => {
                // Check plugin commands before external
                if let Some(status) = self.plugins.exec_command(&mut self.env, &command_name, &args) {
                    self.plugins.call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
                    self.env.exec.last_exit_status = status;
                    return Ok(status);
                }

                let env_vars = self.build_env_vars(&cmd.assignments).map_err(|e| {
                    self.env.exec.last_exit_status = 1;
                    e
                })?;
                let status = self.exec_external_with_redirects(
                    &command_name, &args, &env_vars, &cmd.redirects,
                );
                self.plugins.call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
                self.env.exec.last_exit_status = status;
                Ok(status)
            }
        }
    }

    /// Merge exported shell variables with command-specific assignments.
    pub(crate) fn build_env_vars(&mut self, assignments: &[Assignment]) -> crate::error::Result<Vec<(String, String)>> {
        let mut vars = self.env.vars.environ().to_vec();
        for assign in assignments {
            let value = match assign.value.as_ref() {
                Some(w) => crate::expand::expand_word_to_string(&mut self.env, w)?,
                None => String::new(),
            };
            // Replace existing or push new
            if let Some(entry) = vars.iter_mut().find(|(k, _)| k == &assign.name) {
                entry.1 = value;
            } else {
                vars.push((assign.name.clone(), value));
            }
        }
        Ok(vars)
    }

    /// Fork, apply redirects in child, exec the command, wait in parent.
    pub(crate) fn exec_external_with_redirects(
        &mut self,
        cmd: &str,
        args: &[String],
        env_vars: &[(String, String)],
        redirects: &[crate::parser::ast::Redirect],
    ) -> i32 {
        // Build argv CStrings
        let c_cmd = match CString::new(cmd) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("yosh: {}: invalid command name", cmd);
                return 127;
            }
        };
        let mut c_args: Vec<CString> = Vec::with_capacity(args.len() + 1);
        c_args.push(c_cmd.clone());
        for a in args {
            match CString::new(a.as_str()) {
                Ok(s) => c_args.push(s),
                Err(_) => {
                    eprintln!("yosh: {}: invalid argument", a);
                    return 1;
                }
            }
        }

        let monitor = self.env.mode.options.monitor;
        let shell_pgid = self.env.process.shell_pgid;
        let ignored = self.env.traps.ignored_signals();

        match unsafe { fork() } {
            Err(e) => {
                eprintln!("yosh: fork: {}", e);
                1
            }
            Ok(ForkResult::Child) => {
                // In monitor mode: put child in its own process group so Ctrl+Z
                // (SIGTSTP) stops only the foreground job, not the shell.
                if monitor {
                    let my_pid = nix::unistd::getpid();
                    nix::unistd::setpgid(my_pid, my_pid).ok();
                    signal::setup_foreground_child_signals(&ignored);
                } else {
                    signal::reset_child_signals(&ignored);
                }

                // Apply redirects (no need to save, we're in the child)
                let mut redir_state = RedirectState::new();
                if let Err(e) = redir_state.apply(redirects, &mut self.env, false) {
                    eprintln!("yosh: {}", e);
                    std::process::exit(1);
                }

                // Set environment variables using libc::setenv directly.
                // SAFETY: single-threaded child after fork. We must NOT use
                // std::env::set_var here because it acquires Rust's internal
                // ENV_LOCK (RwLock). If another thread in the parent held
                // that lock at fork() time, the child inherits the locked
                // state and deadlocks — the lock holder thread does not exist
                // in the child.
                for (k, v) in env_vars {
                    if let (Ok(c_key), Ok(c_val)) = (
                        CString::new(k.as_str()),
                        CString::new(v.as_str()),
                    ) {
                        unsafe { libc::setenv(c_key.as_ptr(), c_val.as_ptr(), 1) };
                    }
                }

                let err = execvp(&c_cmd, &c_args).unwrap_err();
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
                    _ => {
                        eprintln!("yosh: {}: {}", cmd, err);
                        127
                    }
                };
                std::process::exit(exit_code);
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
                    let job_id = self.env.process.jobs.add_job(
                        child,
                        vec![child],
                        full_cmd,
                        true,
                    );

                    // Hand terminal to the child's process group.
                    jobs::give_terminal(child).ok();

                    let result = self.wait_for_foreground_job(job_id);

                    // Take terminal back for the shell.
                    jobs::take_terminal(shell_pgid).ok();

                    result.last_status
                } else {
                    wait_child(child).unwrap_or(1)
                }
            }
        }
    }

    /// Apply prefix assignments temporarily, returning saved values for later restoration.
    pub(crate) fn apply_temp_assignments(
        &mut self,
        assignments: &[Assignment],
    ) -> crate::error::Result<Vec<(String, Option<String>)>> {
        let mut saved = Vec::new();
        for assignment in assignments {
            let old_val = self.env.vars.get(&assignment.name).map(|s| s.to_string());
            saved.push((assignment.name.clone(), old_val));
            let value = match assignment.value.as_ref() {
                Some(w) => crate::expand::expand_word_to_string(&mut self.env, w)?,
                None => String::new(),
            };
            let _ = self.env.vars.set(&assignment.name, value);
        }
        Ok(saved)
    }

    /// Restore variables to their pre-assignment values.
    pub(crate) fn restore_assignments(&mut self, saved: Vec<(String, Option<String>)>) {
        for (name, old_val) in saved {
            match old_val {
                Some(val) => {
                    let _ = self.env.vars.set(&name, val);
                }
                None => {
                    let _ = self.env.vars.unset(&name);
                }
            }
        }
    }
}
