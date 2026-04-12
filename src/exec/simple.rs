use std::ffi::CString;

use nix::unistd::{execvp, fork, ForkResult};

use crate::builtin::{classify_builtin, exec_regular_builtin, BuiltinKind};
use crate::builtin::special::exec_special_builtin;
use crate::expand::expand_words;
use crate::parser::ast::{Assignment, SimpleCommand};
use crate::signal;

use super::command::wait_child;
use super::redirect::RedirectState;
use super::Executor;

impl Executor {
    /// Execute a simple command (assignments, builtins, or external programs).
    pub(crate) fn exec_simple_command(&mut self, cmd: &SimpleCommand) -> i32 {
        // Expand all words
        let expanded = match expand_words(&mut self.env, &cmd.words) {
            Ok(words) => words,
            Err(e) => {
                eprintln!("{}", e);
                self.env.exec.last_exit_status = 1;
                return 1;
            }
        };

        // Check if expansion triggered a flow control signal (e.g., nounset error)
        if self.env.exec.flow_control.is_some() {
            self.env.exec.last_exit_status = 1;
            return 1;
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
                            eprintln!("{}", e);
                            self.env.exec.last_exit_status = 1;
                            return 1;
                        }
                    },
                    None => String::new(),
                };
                // Capture the status set by any command substitution during expansion
                last_cmd_sub_status = self.env.exec.last_exit_status;
                if let Err(e) = self.env.vars.set_with_options(&assignment.name, value, self.env.mode.options.allexport) {
                    eprintln!("kish: {}", e);
                    self.env.exec.last_exit_status = 1;
                    return 1;
                }
            }
            self.env.exec.last_exit_status = last_cmd_sub_status;
            return last_cmd_sub_status;
        }

        if self.env.mode.options.xtrace && !expanded.is_empty() {
            eprintln!("+ {}", expanded.join(" "));
        }

        let mut expanded_iter = expanded.into_iter();
        let command_name = expanded_iter.next().unwrap();
        let args: Vec<String> = expanded_iter.collect();

        // Check for function call (before builtins, matching POSIX lookup order)
        if let Some(func_def) = self.env.functions.get(&command_name).cloned() {
            let saved = match self.apply_temp_assignments(&cmd.assignments) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{}", e);
                    self.env.exec.last_exit_status = 1;
                    return 1;
                }
            };
            let mut redirect_state = RedirectState::new();
            if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                eprintln!("kish: {}", e);
                self.restore_assignments(saved);
                self.env.exec.last_exit_status = 1;
                return 1;
            }
            let status = self.exec_function_call(&func_def, &args);
            redirect_state.restore();
            self.restore_assignments(saved);
            self.env.exec.last_exit_status = status;
            return status;
        }

        // wait needs Executor access (bg_jobs + signal processing)
        if command_name == "wait" {
            let saved = match self.apply_temp_assignments(&cmd.assignments) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{}", e);
                    self.env.exec.last_exit_status = 1;
                    return 1;
                }
            };
            let mut redirect_state = RedirectState::new();
            if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                eprintln!("kish: {}", e);
                self.restore_assignments(saved);
                self.env.exec.last_exit_status = 1;
                return 1;
            }
            let status = self.builtin_wait(&args);
            redirect_state.restore();
            self.restore_assignments(saved);
            self.env.exec.last_exit_status = status;
            return status;
        }

        // fg/bg/jobs need Executor access for job table + terminal control
        if command_name == "fg" || command_name == "bg" || command_name == "jobs" {
            let saved = match self.apply_temp_assignments(&cmd.assignments) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{}", e);
                    self.env.exec.last_exit_status = 1;
                    return 1;
                }
            };
            let mut redirect_state = RedirectState::new();
            if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                eprintln!("kish: {}", e);
                self.restore_assignments(saved);
                self.env.exec.last_exit_status = 1;
                return 1;
            }
            let status = match command_name.as_str() {
                "fg" => self.builtin_fg(&args),
                "bg" => self.builtin_bg(&args),
                "jobs" => self.builtin_jobs(&args),
                _ => unreachable!(),
            };
            redirect_state.restore();
            self.restore_assignments(saved);
            self.env.exec.last_exit_status = status;
            return status;
        }

        match classify_builtin(&command_name) {
            BuiltinKind::Special => {
                // Special builtins: prefix assignments persist in current env
                for assignment in &cmd.assignments {
                    let value = match assignment.value.as_ref() {
                        Some(w) => match crate::expand::expand_word_to_string(&mut self.env, w) {
                            Ok(v) => v,
                            Err(e) => {
                                eprintln!("{}", e);
                                self.env.exec.last_exit_status = 1;
                                return 1;
                            }
                        },
                        None => String::new(),
                    };
                    if let Err(e) = self.env.vars.set_with_options(&assignment.name, value, self.env.mode.options.allexport) {
                        eprintln!("kish: {}", e);
                        self.env.exec.last_exit_status = 1;
                        return 1;
                    }
                }
                // exec with no args: redirects persist (don't save/restore)
                if command_name == "exec" && args.is_empty() {
                    let mut redirect_state = RedirectState::new();
                    if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, false) {
                        eprintln!("kish: {}", e);
                        self.env.exec.last_exit_status = 1;
                        return 1;
                    }
                    self.env.exec.last_exit_status = 0;
                    return 0;
                }

                let mut redirect_state = RedirectState::new();
                if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                    eprintln!("kish: {}", e);
                    self.env.exec.last_exit_status = 1;
                    return 1;
                }
                let status = exec_special_builtin(&command_name, &args, self);
                redirect_state.restore();
                self.env.exec.last_exit_status = status;
                status
            }
            BuiltinKind::Regular => {
                // Regular builtins: prefix assignments are temporary
                let saved = match self.apply_temp_assignments(&cmd.assignments) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("{}", e);
                        self.env.exec.last_exit_status = 1;
                        return 1;
                    }
                };
                let mut redirect_state = RedirectState::new();
                if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                    eprintln!("kish: {}", e);
                    self.restore_assignments(saved);
                    self.env.exec.last_exit_status = 1;
                    return 1;
                }
                let status = exec_regular_builtin(&command_name, &args, &mut self.env);
                redirect_state.restore();
                self.restore_assignments(saved);
                self.env.exec.last_exit_status = status;
                status
            }
            BuiltinKind::NotBuiltin => {
                let env_vars = match self.build_env_vars(&cmd.assignments) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        self.env.exec.last_exit_status = 1;
                        return 1;
                    }
                };
                let status = self.exec_external_with_redirects(
                    &command_name, &args, &env_vars, &cmd.redirects,
                );
                self.env.exec.last_exit_status = status;
                status
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
                eprintln!("kish: {}: invalid command name", cmd);
                return 127;
            }
        };
        let mut c_args: Vec<CString> = Vec::with_capacity(args.len() + 1);
        c_args.push(c_cmd.clone());
        for a in args {
            match CString::new(a.as_str()) {
                Ok(s) => c_args.push(s),
                Err(_) => {
                    eprintln!("kish: {}: invalid argument", a);
                    return 1;
                }
            }
        }

        match unsafe { fork() } {
            Err(e) => {
                eprintln!("kish: fork: {}", e);
                1
            }
            Ok(ForkResult::Child) => {
                signal::reset_child_signals(&self.env.traps.ignored_signals());

                // Apply redirects (no need to save, we're in the child)
                let mut redir_state = RedirectState::new();
                if let Err(e) = redir_state.apply(redirects, &mut self.env, false) {
                    eprintln!("kish: {}", e);
                    std::process::exit(1);
                }

                // Set environment variables
                for (k, v) in env_vars {
                    // SAFETY: single-threaded child after fork
                    unsafe { std::env::set_var(k, v) };
                }

                let err = execvp(&c_cmd, &c_args).unwrap_err();
                use nix::errno::Errno;
                let exit_code = match err {
                    Errno::ENOENT => {
                        eprintln!("kish: {}: command not found", cmd);
                        127
                    }
                    Errno::EACCES => {
                        eprintln!("kish: {}: permission denied", cmd);
                        126
                    }
                    _ => {
                        eprintln!("kish: {}: {}", cmd, err);
                        127
                    }
                };
                std::process::exit(exit_code);
            }
            Ok(ForkResult::Parent { child }) => wait_child(child),
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
