use std::ffi::CString;

use nix::unistd::{ForkResult, execvp, fork};

use crate::builtin::special::exec_special_builtin;
use crate::builtin::{BuiltinKind, classify_builtin, exec_regular_builtin};
use crate::env::jobs;
use crate::error::{RuntimeErrorKind, ShellError};
use crate::expand::expand_words;
use crate::parser::ast::{Assignment, ParamExpr, SimpleCommand, Word, WordPart};
use crate::signal;

use super::Executor;
use super::command::wait_child;
use super::redirect::RedirectState;

/// For export/readonly, re-process each Word argument by trying to parse
/// it as an Assignment first. Words that successfully parse as `NAME=value`
/// get their value Word expanded in a tilde-aware way (EscapedLiteral
/// segments bypass tilde recognition in split_tildes_in_literal), avoiding
/// the lossy string-based tilde expansion the builtin used to perform.
///
/// Returns a Vec of `NAME=value` or `NAME` strings suitable for the
/// existing builtin_export / builtin_readonly signatures.
fn expand_assignment_builtin_args(
    env: &mut crate::env::ShellEnv,
    words: &[crate::parser::ast::Word],
) -> crate::error::Result<Vec<String>> {
    use crate::parser::Parser;
    use crate::parser::ast::Assignment;

    let mut out = Vec::with_capacity(words.len());
    for word in words {
        match Parser::try_parse_assignment(word) {
            Some(Assignment { name, value: Some(value_word) }) => {
                let value = crate::expand::expand_word_to_string(env, &value_word)?;
                out.push(format!("{}={}", name, value));
            }
            Some(Assignment { name, value: None }) => {
                out.push(format!("{}=", name));
            }
            None => {
                // Not an assignment (e.g. `export NAME` bare form or `export -p`).
                // Fall back to normal word expansion (may produce multiple fields
                // after IFS split; we preserve all of them).
                let expanded = crate::expand::expand_words(env, std::slice::from_ref(word))?;
                out.extend(expanded);
            }
        }
    }
    Ok(out)
}

impl Executor {
    /// Execute a simple command (assignments, builtins, or external programs).
    pub(crate) fn exec_simple_command(&mut self, cmd: &SimpleCommand) -> Result<i32, ShellError> {
        let _ = self.env.vars.set("LINENO", cmd.line.to_string());

        // Expand ONLY the command name first, so we can dispatch to an
        // assignment-aware expansion for export/readonly. A single early
        // expansion of `cmd.words[..]` would fire side effects (command
        // substitutions, arithmetic, `${x=...}`) on every argument — the
        // assignment-aware re-expansion used by export/readonly would then
        // run them a second time. Gate the full expansion on the detected
        // command name instead.
        let name_fields = if cmd.words.is_empty() {
            Vec::new()
        } else {
            match expand_words(&mut self.env, std::slice::from_ref(&cmd.words[0])) {
                Ok(words) => words,
                Err(e) => {
                    self.env.exec.last_exit_status = 1;
                    return Err(e);
                }
            }
        };

        // Check if expansion triggered a flow control signal (e.g., nounset error)
        if self.env.exec.flow_control.is_some() {
            self.env.exec.last_exit_status = 1;
            return Ok(1);
        }

        // Determine the effective command name (first field after expansion,
        // which may be empty when `cmd.words[0]` expanded to nothing e.g. an
        // unset variable with nullglob-like semantics).
        let probable_name: Option<&str> = name_fields.first().map(|s| s.as_str());
        let is_assignment_builtin = matches!(probable_name, Some("export") | Some("readonly"));

        // Expand the remaining words. For export/readonly, route through the
        // assignment-aware expander so each Word is expanded exactly once
        // (preserving EscapedLiteral / Tilde metadata for `NAME=\~/path`
        // style arguments). For all other commands, use the normal
        // field-splitting expander.
        let rest_fields: Vec<String> = if cmd.words.len() <= 1 {
            Vec::new()
        } else if is_assignment_builtin {
            match expand_assignment_builtin_args(&mut self.env, &cmd.words[1..]) {
                Ok(v) => v,
                Err(e) => {
                    self.env.exec.last_exit_status = 1;
                    return Err(e);
                }
            }
        } else {
            match expand_words(&mut self.env, &cmd.words[1..]) {
                Ok(v) => v,
                Err(e) => {
                    self.env.exec.last_exit_status = 1;
                    return Err(e);
                }
            }
        };

        if self.env.exec.flow_control.is_some() {
            self.env.exec.last_exit_status = 1;
            return Ok(1);
        }

        // Stitch: fields from the command-name word come first, remaining
        // words' fields follow. This matches the ordering the original
        // single `expand_words(&cmd.words)` would have produced.
        let mut expanded: Vec<String> = Vec::with_capacity(name_fields.len() + rest_fields.len());
        expanded.extend(name_fields);
        expanded.extend(rest_fields);

        // Assignment-only command (no words, or all words expanded to empty)
        if expanded.is_empty() {
            // POSIX §2.9.1: exit status of an assignment-only command is the status
            // of the last command substitution performed, or 0 if none.
            //
            // $? must remain visible to value expansions (e.g. `x=$?`) until a
            // command substitution overwrites it, so we do NOT reset it up front.
            let mut last_cmd_sub_status: Option<i32> = None;
            for assignment in &cmd.assignments {
                let has_cmd_sub = assignment.value.as_ref().is_some_and(word_has_command_sub);
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
                // If the value expansion contained a command substitution, $?
                // now reflects its exit status. Record it regardless of whether
                // the delta was non-zero — the substitution may coincidentally
                // return the same status as the prior command (e.g.
                // `false; x=$(false)`).
                if has_cmd_sub {
                    last_cmd_sub_status = Some(self.env.exec.last_exit_status);
                }
                if let Err(e) = self.env.vars.set_with_options(
                    &assignment.name,
                    value,
                    self.env.mode.options.allexport,
                ) {
                    self.env.exec.last_exit_status = 1;
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::ReadonlyVariable,
                        format!("{}", e),
                    ));
                }
            }
            let final_status = last_cmd_sub_status.unwrap_or(0);
            self.env.exec.last_exit_status = final_status;
            return Ok(final_status);
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
        self.plugins
            .call_pre_exec(&mut self.env, &cmd_str_for_hooks);

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
            self.plugins
                .call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
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
            let status = self.builtin_wait(&args).unwrap_or_else(|e| {
                eprintln!("{}", e);
                e.exit_code()
            });
            redirect_state.restore();
            self.restore_assignments(saved);
            self.plugins
                .call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
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
            }
            .unwrap_or_else(|e| {
                eprintln!("{}", e);
                e.exit_code()
            });
            redirect_state.restore();
            self.restore_assignments(saved);
            self.plugins
                .call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
            self.env.exec.last_exit_status = status;
            return Ok(status);
        }

        // `command` needs Executor access for -p / no-flag execution paths.
        if command_name == "command" {
            let saved = self
                .apply_temp_assignments(&cmd.assignments)
                .inspect_err(|_| {
                    self.env.exec.last_exit_status = 1;
                })?;
            let mut redirect_state = RedirectState::new();
            if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                self.restore_assignments(saved);
                self.env.exec.last_exit_status = 1;
                return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
            }
            let status = self.builtin_command(&args);
            redirect_state.restore();
            self.restore_assignments(saved);
            self.plugins
                .call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
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
                    if let Err(e) = self.env.vars.set_with_options(
                        &assignment.name,
                        value,
                        self.env.mode.options.allexport,
                    ) {
                        self.env.exec.last_exit_status = 1;
                        return Err(ShellError::runtime(
                            RuntimeErrorKind::ReadonlyVariable,
                            format!("{}", e),
                        ));
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
                // `args` for export/readonly was already produced by
                // expand_assignment_builtin_args in the early dispatch above
                // (single-pass expansion avoids double-running command
                // substitutions in assignment RHS).
                let status = exec_special_builtin(&command_name, &args, self);
                redirect_state.restore();
                self.plugins
                    .call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
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
                self.plugins
                    .call_post_exec(&mut self.env, &cmd_str_for_hooks, status);

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
                if let Some(status) = self
                    .plugins
                    .exec_command(&mut self.env, &command_name, &args)
                {
                    self.plugins
                        .call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
                    self.env.exec.last_exit_status = status;
                    return Ok(status);
                }

                let env_vars = self.build_env_vars(&cmd.assignments).map_err(|e| {
                    self.env.exec.last_exit_status = 1;
                    e
                })?;
                let status = self.exec_external_with_redirects(
                    &command_name,
                    &args,
                    &env_vars,
                    &cmd.redirects,
                );
                self.plugins
                    .call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
                self.env.exec.last_exit_status = status;
                Ok(status)
            }
        }
    }

    /// Merge exported shell variables with command-specific assignments.
    pub(crate) fn build_env_vars(
        &mut self,
        assignments: &[Assignment],
    ) -> crate::error::Result<Vec<(String, String)>> {
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
                    super::exit_child(1);
                }

                // Set environment variables using libc::setenv directly.
                // SAFETY: single-threaded child after fork. We must NOT use
                // std::env::set_var here because it acquires Rust's internal
                // ENV_LOCK (RwLock). If another thread in the parent held
                // that lock at fork() time, the child inherits the locked
                // state and deadlocks — the lock holder thread does not exist
                // in the child.
                for (k, v) in env_vars {
                    if let (Ok(c_key), Ok(c_val)) =
                        (CString::new(k.as_str()), CString::new(v.as_str()))
                    {
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
                super::exit_child(exit_code);
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

impl Executor {
    /// POSIX `command` builtin. Dispatches by verbosity:
    /// - Brief (`-v`) / Verbose (`-V`) → print and return exit status
    /// - Execute (`-p` or no flag) → handled in later tasks (returns 1 for now)
    pub(crate) fn builtin_command(&mut self, args: &[String]) -> i32 {
        use crate::builtin::command::{Verbosity, parse_flags, render_brief, render_verbose};

        let parsed = match parse_flags(args) {
            Ok(p) => p,
            Err(msg) => {
                eprintln!("yosh: {}", msg);
                return 2;
            }
        };

        match parsed.verbose {
            Verbosity::Brief => {
                let (out, code) = render_brief(&self.env, &parsed.name);
                if !out.is_empty() {
                    println!("{}", out);
                }
                code
            }
            Verbosity::Verbose => {
                let (out, err, code) = render_verbose(&self.env, &parsed.name);
                if !out.is_empty() {
                    println!("{}", out);
                }
                if !err.is_empty() {
                    eprintln!("{}", err);
                }
                code
            }
            Verbosity::Execute => {
                if parsed.use_default_path {
                    self.exec_command_with_default_path(&parsed.name, &parsed.rest)
                } else {
                    // No-flag path (function-skip): implemented in the next task.
                    self.exec_command_skip_functions(&parsed.name, &parsed.rest)
                }
            }
        }
    }

    /// `command -p name args...`: look up `name` via the POSIX default PATH
    /// (ignoring $PATH entirely) and exec it. Builtins are still honored
    /// for the name: POSIX says `command -p` runs the named utility in
    /// preference over functions, but builtins are part of the utility set.
    pub(crate) fn exec_command_with_default_path(&mut self, name: &str, args: &[String]) -> i32 {
        use crate::builtin::special::exec_special_builtin;
        use crate::builtin::{BuiltinKind, classify_builtin, exec_regular_builtin};
        use crate::env::default_path::default_path;

        // If `name` is a builtin, run the builtin (POSIX: command -p still
        // runs builtins; -p only affects external lookup).
        match classify_builtin(name) {
            BuiltinKind::Special => {
                let status = exec_special_builtin(name, args, self);
                return status;
            }
            BuiltinKind::Regular => {
                // Don't re-enter special-cased handlers (wait/fg/bg/jobs/command).
                // If we get here with one of those, fall through to external.
                if !matches!(name, "wait" | "fg" | "bg" | "jobs" | "command") {
                    return exec_regular_builtin(name, args, &mut self.env);
                }
            }
            BuiltinKind::NotBuiltin => {}
        }

        use crate::exec::command::{PathLookup, lookup_in_path};

        let dp = default_path(&self.env).to_string();
        match lookup_in_path(name, &dp) {
            PathLookup::Executable(p) => exec_external_absolute(&p, name, args, &mut self.env),
            PathLookup::NotExecutable(p) => {
                eprintln!("yosh: command: {}: permission denied", p.display());
                126
            }
            PathLookup::NotFound => {
                eprintln!("yosh: command: {}: not found", name);
                127
            }
        }
    }

    /// `command name args...`: execute `name` using the current $PATH but
    /// bypassing shell functions. Aliases are already handled (they're
    /// expanded at parse time, so `command` arrived here only if the
    /// parser saw `command` itself, not the expanded alias).
    pub(crate) fn exec_command_skip_functions(&mut self, name: &str, args: &[String]) -> i32 {
        use crate::builtin::special::exec_special_builtin;
        use crate::builtin::{BuiltinKind, classify_builtin, exec_regular_builtin};

        // Builtins take precedence over external; functions are deliberately
        // skipped.
        match classify_builtin(name) {
            BuiltinKind::Special => return exec_special_builtin(name, args, self),
            BuiltinKind::Regular => {
                if !matches!(name, "wait" | "fg" | "bg" | "jobs" | "command") {
                    return exec_regular_builtin(name, args, &mut self.env);
                }
                // For the special-cased regular builtins, fall through to
                // external lookup (running `command wait` via PATH would be
                // surprising, but this matches how yosh currently dispatches
                // those names only when invoked as direct simple commands).
            }
            BuiltinKind::NotBuiltin => {}
        }

        // External: resolve via $PATH (not the POSIX default path).
        use crate::exec::command::{PathLookup, lookup_in_path};

        let path_var = self
            .env
            .vars
            .get("PATH")
            .map(|s| s.to_string())
            .unwrap_or_default();
        match lookup_in_path(name, &path_var) {
            PathLookup::Executable(p) => exec_external_absolute(&p, name, args, &mut self.env),
            PathLookup::NotExecutable(p) => {
                eprintln!("yosh: command: {}: permission denied", p.display());
                126
            }
            PathLookup::NotFound => {
                eprintln!("yosh: command: {}: not found", name);
                127
            }
        }
    }
}

/// Spawn an absolute path with `args`, inheriting the shell's exported
/// environment. Used by `command -p` (after default-PATH lookup) and by
/// `command name` (after current-PATH lookup).
///
/// Uses `std::process::Command` rather than manual fork+execvp because
/// yosh's existing external-command pipeline is tightly coupled to job
/// control, redirects, and env-sync concerns that we don't need here
/// (command -p / no-flag forms always run in the foreground with the
/// simple-command redirects already applied by the special-case handler).
fn exec_external_absolute(
    resolved: &std::path::Path,
    display_name: &str,
    args: &[String],
    env: &mut crate::env::ShellEnv,
) -> i32 {
    use std::os::unix::process::CommandExt;
    use std::os::unix::process::ExitStatusExt;

    let env_pairs: Vec<(String, String)> = env.vars.environ().to_vec();

    let result = std::process::Command::new(resolved)
        .arg0(display_name)
        .args(args)
        .env_clear()
        .envs(env_pairs)
        .status();

    match result {
        Ok(s) => {
            if let Some(code) = s.code() {
                code
            } else if let Some(sig) = s.signal() {
                128 + sig
            } else {
                1
            }
        }
        Err(e) => match e.kind() {
            std::io::ErrorKind::NotFound => {
                eprintln!("yosh: command: {}: not found", display_name);
                127
            }
            std::io::ErrorKind::PermissionDenied => {
                eprintln!("yosh: command: {}: permission denied", display_name);
                126
            }
            _ => {
                eprintln!("yosh: command: {}: {}", display_name, e);
                1
            }
        },
    }
}

/// True if the word (or any nested word inside quoting/parameter expansion)
/// contains a command substitution. Used by the assignment-only command path
/// to decide whether `$?` must be updated to reflect the substitution.
fn word_has_command_sub(word: &Word) -> bool {
    word.parts.iter().any(part_has_command_sub)
}

fn part_has_command_sub(part: &WordPart) -> bool {
    match part {
        WordPart::Literal(_)
        | WordPart::EscapedLiteral(_)
        | WordPart::SingleQuoted(_)
        | WordPart::DollarSingleQuoted(_)
        | WordPart::Tilde(_) => false,
        WordPart::CommandSub(_) | WordPart::ArithSub(_) => true,
        WordPart::DoubleQuoted(parts) => parts.iter().any(part_has_command_sub),
        WordPart::Parameter(p) => param_has_command_sub(p),
    }
}

fn param_has_command_sub(p: &ParamExpr) -> bool {
    match p {
        ParamExpr::Simple(_)
        | ParamExpr::Positional(_)
        | ParamExpr::Special(_)
        | ParamExpr::Length(_) => false,
        ParamExpr::Default { word, .. }
        | ParamExpr::Assign { word, .. }
        | ParamExpr::Error { word, .. }
        | ParamExpr::Alt { word, .. } => word.as_ref().is_some_and(word_has_command_sub),
        ParamExpr::StripShortSuffix(_, w)
        | ParamExpr::StripLongSuffix(_, w)
        | ParamExpr::StripShortPrefix(_, w)
        | ParamExpr::StripLongPrefix(_, w) => word_has_command_sub(w),
    }
}
