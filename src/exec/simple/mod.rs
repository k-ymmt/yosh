//! Simple-command execution (POSIX §2.9.1): word expansion and dispatch
//! to assignments, functions, built-ins and external utilities.

mod assign;
mod command_builtin;
mod cstrings;
mod external;
mod resolve;
mod xtrace;

use crate::builtin::special::run_special_builtin;
use crate::builtin::{BuiltinKind, classify_builtin, exec_regular_builtin};
use crate::env::ShellEnv;
use crate::error::{RuntimeErrorKind, ShellError, ShellErrorKind};
use crate::expand::expand_words;
use crate::parser::ast::{Assignment, ParamExpr, SimpleCommand, Word, WordPart};
use crate::parser::try_parse_assignment;

use super::Executor;
use super::redirect::{RedirectState, expand_redirects};
use xtrace::xtrace_prefix;

/// For export/readonly, re-process each Word argument by trying to parse
/// it as an Assignment first. Words that successfully parse as `NAME=value`
/// get their value Word expanded in a tilde-aware way (EscapedLiteral
/// segments bypass tilde recognition in split_tildes_in_literal), avoiding
/// the lossy string-based tilde expansion the builtin used to perform.
///
/// Returns a Vec of `NAME=value` or `NAME` strings suitable for the
/// existing builtin_export / builtin_readonly signatures.
fn expand_assignment_builtin_args(
    env: &mut ShellEnv,
    words: &[Word],
) -> crate::error::Result<Vec<String>> {
    let mut out = Vec::with_capacity(words.len());
    for word in words {
        match try_parse_assignment(word) {
            Some(Assignment {
                name,
                value: Some(value_word),
            }) => {
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
                let expanded = expand_words(env, std::slice::from_ref(word))?;
                out.extend(expanded);
            }
        }
    }
    Ok(out)
}

impl Executor {
    /// Execute a simple command (assignments, builtins, or external programs).
    pub(crate) fn exec_simple_command(&mut self, cmd: &SimpleCommand) -> Result<i32, ShellError> {
        self.env.exec.lineno = cmd.line;

        // Take (read + clear) the async exec-in-place marker BEFORE any
        // expansion: command substitutions below fork children whose
        // cloned ShellEnv must not inherit a set flag, and nested
        // exec_simple_command calls (function bodies, `command`) must
        // not consume it. Only the external-utility dispatch at the
        // bottom honors it; every other dispatch ignores it and keeps
        // wrapper-subshell semantics.
        let async_exec_in_place = std::mem::take(&mut self.env.exec.async_exec_in_place);

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
            // of the last command substitution performed, or 0 if none. Both
            // command-word (`$(false)` alone) and assignment-value substitutions
            // are eligible. Word-side substitutions already updated
            // env.exec.last_exit_status during expand_words above.
            //
            // $? must remain visible to value expansions (e.g. `x=$?`) until a
            // command substitution overwrites it, so we do NOT reset it up front.
            let words_had_cmd_sub = cmd.words.iter().any(word_has_command_sub);
            let mut last_cmd_sub_status: Option<i32> = if words_had_cmd_sub {
                Some(self.env.exec.last_exit_status)
            } else {
                None
            };

            // POSIX §2.9.1: a command consisting only of redirections must still
            // apply them. Apply with save=true so the shell-process fds are
            // restored after this "command" completes (matches bash/dash).
            let mut redirect_state = RedirectState::new();
            if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                self.env.exec.last_exit_status = 1;
                return Err(e);
            }

            for assignment in &cmd.assignments {
                let has_cmd_sub = assignment.value.as_ref().is_some_and(word_has_command_sub);
                let value = match assignment.value.as_ref() {
                    // A failed expansion (`x=${zz:?}`, `set -u; x=$zz`)
                    // must neither assign nor report status 0: it is an
                    // expansion error (POSIX §2.8.1).
                    Some(w) => match self.expand_assignment_value(w) {
                        Ok(v) => v,
                        Err(e) => {
                            redirect_state.restore();
                            self.env.exec.last_exit_status = 1;
                            return Err(e);
                        }
                    },
                    None => String::new(),
                };
                // Trace before the variable is set so a readonly-failure still
                // produces the trace line (bash behaviour). Nested command-sub
                // traces inside `value` have already fired during expansion.
                if self.env.mode.options.xtrace {
                    let prefix = xtrace_prefix(&mut self.env);
                    crate::byteenc::write_stderr_decoded_line(&format!(
                        "{}{}={}",
                        prefix, assignment.name, value
                    ));
                }
                // If the value expansion contained a command substitution, $?
                // now reflects its exit status. Record it regardless of whether
                // the delta was non-zero — the substitution may coincidentally
                // return the same status as the prior command (e.g.
                // `false; x=$(false)`).
                if has_cmd_sub {
                    last_cmd_sub_status = Some(self.env.exec.last_exit_status);
                }
                // assign_var_with_options (not vars.set_with_options):
                // `PATH=/dir` must invalidate the utility hash (POSIX §2.5.3).
                let allexport = self.env.mode.options.allexport;
                if let Err(e) = self
                    .env
                    .assign_var_with_options(&assignment.name, value, allexport)
                {
                    redirect_state.restore();
                    self.env.exec.last_exit_status = 1;
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::ReadonlyVariable,
                        e.to_string(),
                    ));
                }
            }
            let final_status = last_cmd_sub_status.unwrap_or(0);
            redirect_state.restore();
            self.env.exec.last_exit_status = final_status;
            return Ok(final_status);
        }

        let mut expanded_iter = expanded.into_iter();
        let command_name = expanded_iter.next().unwrap();
        let args: Vec<String> = expanded_iter.collect();
        let kind = classify_builtin(&command_name);

        // POSIX §2.9.1 step order: redirection words are expanded before
        // the prefix assignments, so `x=bar cmd >$x` opens the file named
        // by the *old* `$x` (dash/bash agree). Expanding here, in the
        // shell process, also makes an expansion error in a redirection
        // word a shell-level expansion error for every command kind —
        // previously external commands expanded them in the forked child,
        // so `/bin/echo ran >${zz:?}` still ran the command.
        let redirs = match expand_redirects(&cmd.redirects, &mut self.env) {
            Ok(r) => r,
            Err(e) => {
                self.env.exec.last_exit_status = 1;
                // A here-document body failure is a redirection error,
                // which POSIX §2.8.1 makes fatal for a special built-in
                // in a non-interactive shell (expansion errors are fatal
                // for every command and handled by the caller).
                if !matches!(e.kind, ShellErrorKind::Expansion(_))
                    && matches!(kind, BuiltinKind::Special)
                    && !self.env.mode.is_interactive
                {
                    if !e.message.is_empty() {
                        eprintln!("{}", e);
                    }
                    self.exit_shell(1);
                }
                return Err(e);
            }
        };

        // Build command string for hooks — only when some loaded plugin
        // actually registered pre-exec or post-exec, since this joined
        // string is otherwise built and immediately discarded on every
        // single simple command.
        let cmd_str_for_hooks = if self.plugins.has_exec_hooks() {
            std::iter::once(command_name.as_str())
                .chain(args.iter().map(|s| s.as_str()))
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            String::new()
        };
        self.plugins
            .call_pre_exec(&mut self.env, &cmd_str_for_hooks);

        // Check for function call. POSIX §2.9.1.4: special built-ins are
        // found before functions, so a function may never shadow one;
        // functions do take precedence over regular built-ins and PATH.
        if !matches!(kind, BuiltinKind::Special)
            // `.cloned()` here is an Rc refcount bump, not a deep copy of
            // the definition — the table stores `Rc<FunctionDef>`.
            && let Some(func_def) = self.env.functions.get(&command_name).cloned()
        {
            let status = self
                .run_with_temp_assignments(cmd, &redirs, &command_name, &args, |exec| {
                    exec.exec_function_call(&func_def, &args)
                })
                .inspect_err(|_| self.env.exec.last_exit_status = 1)?;
            self.plugins
                .call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
            self.env.exec.last_exit_status = status;
            return Ok(status);
        }

        // wait / fg / bg / jobs / command need Executor access (job
        // table, signal processing, -p / no-flag execution paths); fc (a
        // regular builtin) needs it to re-execute history entries.
        if matches!(
            command_name.as_str(),
            "wait" | "fg" | "bg" | "jobs" | "command" | "fc"
        ) {
            let status = self
                .run_with_temp_assignments(cmd, &redirs, &command_name, &args, |exec| {
                    let res = match command_name.as_str() {
                        "wait" => exec.builtin_wait(&args),
                        "fg" => exec.builtin_fg(&args),
                        "bg" => exec.builtin_bg(&args),
                        "jobs" => exec.builtin_jobs(&args),
                        "fc" => crate::builtin::fc::builtin_fc(&args, exec),
                        "command" => Ok(exec.builtin_command(&args)),
                        _ => unreachable!(),
                    };
                    res.unwrap_or_else(|e| {
                        eprintln!("{}", e);
                        e.exit_code()
                    })
                })
                .inspect_err(|_| self.env.exec.last_exit_status = 1)?;
            self.plugins
                .call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
            self.env.exec.last_exit_status = status;
            return Ok(status);
        }

        match kind {
            BuiltinKind::Special => {
                // Special builtins: prefix assignments persist in current env
                let mut assigned = Vec::with_capacity(cmd.assignments.len());
                for assignment in &cmd.assignments {
                    let value = match assignment.value.as_ref() {
                        Some(w) => self
                            .expand_assignment_value(w)
                            .inspect_err(|_| self.env.exec.last_exit_status = 1)?,
                        None => String::new(),
                    };
                    // assign_var_with_options: a `PATH=… special-builtin`
                    // prefix assignment persists (POSIX §2.9.1) and must
                    // invalidate the utility hash like any PATH write.
                    let allexport = self.env.mode.options.allexport;
                    if let Err(e) =
                        self.env
                            .assign_var_with_options(&assignment.name, value.clone(), allexport)
                    {
                        self.env.exec.last_exit_status = 1;
                        return Err(ShellError::runtime(
                            RuntimeErrorKind::ReadonlyVariable,
                            e.to_string(),
                        ));
                    }
                    // `VAR=x exec cmd`: the assignment belongs in the
                    // exec'd program's environment (dash/bash/ksh agree);
                    // it also persists in the shell per the special
                    // built-in rule, which is moot once exec succeeds.
                    if command_name == "exec" && !args.is_empty() {
                        self.env.vars.export(&assignment.name);
                    }
                    assigned.push((assignment.name.clone(), value));
                }
                self.xtrace_simple(&assigned, &command_name, &args);

                // exec with no args: redirects persist (don't save/restore)
                if command_name == "exec" && args.is_empty() {
                    let mut redirect_state = RedirectState::new();
                    if let Err(e) = redirect_state.apply_expanded(&redirs, false) {
                        self.env.exec.last_exit_status = 1;
                        if !self.env.mode.is_interactive {
                            if !e.message.is_empty() {
                                eprintln!("{}", e);
                            }
                            self.exit_shell(1);
                        }
                        return Err(e);
                    }
                    self.env.exec.last_exit_status = 0;
                    return Ok(0);
                }

                let mut redirect_state = RedirectState::new();
                if let Err(e) = redirect_state.apply_expanded(&redirs, true) {
                    self.env.exec.last_exit_status = 1;
                    if !self.env.mode.is_interactive {
                        if !e.message.is_empty() {
                            eprintln!("{}", e);
                        }
                        self.exit_shell(1);
                    }
                    return Err(e);
                }
                // `args` for export/readonly was already produced by
                // expand_assignment_builtin_args in the early dispatch above
                // (single-pass expansion avoids double-running command
                // substitutions in assignment RHS).
                let outcome = run_special_builtin(&command_name, &args, self);
                redirect_state.restore();
                let status = outcome.status;
                // POSIX §2.8.1: a special built-in utility error makes a
                // non-interactive shell exit (dash and bash --posix agree).
                // Only this direct dispatch requests the exit — `command`
                // strips the special properties (`command set -Z` must not
                // exit), and it reaches the builtin via another path.
                if outcome.utility_error && !self.env.mode.is_interactive {
                    self.exit_requested = Some(status);
                }
                self.plugins
                    .call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
                self.env.exec.last_exit_status = status;
                Ok(status)
            }
            BuiltinKind::Regular => {
                // Regular builtins: prefix assignments are temporary
                let old_pwd = if command_name == "cd" {
                    self.env.vars.get("PWD").map(|s| s.to_string())
                } else {
                    None
                };
                let status = self
                    .run_with_temp_assignments(cmd, &redirs, &command_name, &args, |exec| {
                        exec_regular_builtin(&command_name, &args, &mut exec.env)
                    })
                    .inspect_err(|_| self.env.exec.last_exit_status = 1)?;
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
                // Check plugin commands before external. The 3-valued
                // return distinguishes "no plugin claimed this name"
                // (fall through to PATH) from "plugin claimed it but
                // failed" (do NOT fall through — the plugin owned it).
                use crate::plugin::PluginExec;
                match self
                    .plugins
                    .exec_command(&mut self.env, &command_name, &args)
                {
                    PluginExec::Handled(status) => {
                        self.plugins
                            .call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
                        self.env.exec.last_exit_status = status;
                        return Ok(status);
                    }
                    PluginExec::Failed => {
                        self.plugins
                            .call_post_exec(&mut self.env, &cmd_str_for_hooks, 1);
                        self.env.exec.last_exit_status = 1;
                        return Ok(1);
                    }
                    PluginExec::NotHandled => {}
                }

                let env_vars = self.build_env_vars(&cmd.assignments).inspect_err(|_| {
                    self.env.exec.last_exit_status = 1;
                })?;
                self.xtrace_simple(&env_vars, &command_name, &args);
                // DEVIATION (async exec-in-place, 2026-09-01): plugins'
                // post_exec does not fire for a background external
                // simple command — the async child is replaced by the
                // exec. pre_exec fired above, as it always did in the
                // wrapper. Pre-optimization, post_exec ran inside the
                // forked wrapper whose plugin state was discarded on
                // exit, so only external side effects are lost.
                let status = self.exec_external_with_redirects(
                    &command_name,
                    &args,
                    &env_vars,
                    &redirs,
                    async_exec_in_place,
                );
                self.plugins
                    .call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
                self.env.exec.last_exit_status = status;
                Ok(status)
            }
        }
    }
}

/// True if the word (or any nested word inside quoting/parameter expansion)
/// contains a command substitution. Used by the assignment-only command path
/// to decide whether `$?` must be updated to reflect the substitution.
///
/// Command substitutions ONLY: a pure arithmetic expansion does not run a
/// command and must not count, so `false; x=$((1+1))` yields `$?` = 0
/// (bash agrees; POSIX leaves it implementation-defined). An arithmetic
/// expansion that itself embeds `$(cmd)` DOES count — the arithmetic
/// evaluator (via `expand::dollar::expand_string`) runs the substitution
/// and updates `last_exit_status`, so mirror its trigger (`$` followed by
/// `(`).
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
        WordPart::CommandSub(_) => true,
        // ArithSub carries the raw expression text; `$(` and backticks are
        // what the arithmetic evaluator dispatches to command substitution.
        WordPart::ArithSub(expr) => expr.contains("$(") || expr.contains('`'),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes tests that redirect the PROCESS-shared stdio (fd 1)
    /// via `exec_program` on redirect-only commands: the redirect is
    /// applied to the shell process itself with save=true, and two such
    /// tests interleaving mid-dup2-swap under the parallel libtest
    /// harness fail with `dup: Bad file descriptor` (observed
    /// 2026-07-13, 2026-09-01, 2026-09-02; always passes in isolation —
    /// production is unaffected, the real shell is single-threaded).
    /// Lock at the top of every test that redirects the process's own
    /// stdio. Poisoning only means another such test's assert failed;
    /// the fd state itself was restored by `exec_program`, so continue.
    static STDIO_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    // ── word_has_command_sub: CmdSub-only predicate (SP5) ──

    #[test]
    fn pure_arith_sub_does_not_count_as_command_sub() {
        // `x=$((1+1))` after `false` must yield $?=0 (bash behavior).
        assert!(!part_has_command_sub(&WordPart::ArithSub(
            "1+1".to_string()
        )));
    }

    #[test]
    fn arith_sub_with_embedded_command_sub_counts() {
        // `$(( $(cmd) + 1 ))` runs the substitution and updates $?.
        assert!(part_has_command_sub(&WordPart::ArithSub(
            " $(exit 5) + 1".to_string()
        )));
    }

    #[test]
    fn arith_sub_with_backtick_command_sub_counts() {
        // `` $(( `cmd` + 1 )) `` also runs a substitution and updates $?.
        assert!(part_has_command_sub(&WordPart::ArithSub(
            " `echo 2` + 1".to_string()
        )));
    }

    #[test]
    fn arith_sub_predicate_recurses_through_double_quotes() {
        assert!(!part_has_command_sub(&WordPart::DoubleQuoted(vec![
            WordPart::ArithSub("2*3".to_string())
        ])));
    }

    #[test]
    fn standalone_false_cmd_sub_propagates_exit_status() {
        use crate::exec::Executor;
        use crate::parser::Parser;
        let mut exec = Executor::new("yosh", vec![]);
        let prog = Parser::new("$(false)\n").parse_program().unwrap();
        exec.exec_program(&prog);
        assert_eq!(
            exec.env.exec.last_exit_status, 1,
            "$? must reflect the substituted command's exit status"
        );
    }

    #[test]
    fn standalone_true_cmd_sub_propagates_zero() {
        use crate::exec::Executor;
        use crate::parser::Parser;
        let mut exec = Executor::new("yosh", vec![]);
        let prog = Parser::new("$(true)\n").parse_program().unwrap();
        exec.exec_program(&prog);
        assert_eq!(exec.env.exec.last_exit_status, 0);
    }

    #[test]
    fn cmd_sub_followed_by_echo_dollarquestion_outputs_status() {
        // Regression-friendly form: the test ensures the fix does not break
        // the existing assignment-side propagation.
        use crate::exec::Executor;
        use crate::parser::Parser;
        let mut exec = Executor::new("yosh", vec![]);
        let prog = Parser::new("x=$(false)\n").parse_program().unwrap();
        exec.exec_program(&prog);
        assert_eq!(exec.env.exec.last_exit_status, 1);
    }

    #[test]
    fn redirect_only_creates_file() {
        let _stdio = STDIO_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use crate::exec::Executor;
        use crate::parser::Parser;

        let tmp =
            std::env::temp_dir().join(format!("yosh_redirect_only_{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        let source = format!(">{}\n", tmp.display());
        let prog = Parser::new(&source).parse_program().unwrap();
        let mut exec = Executor::new("yosh", vec![]);
        exec.exec_program(&prog);

        assert!(
            tmp.exists(),
            "redirect-only command must create {}",
            tmp.display()
        );
        let metadata = std::fs::metadata(&tmp).unwrap();
        assert_eq!(
            metadata.len(),
            0,
            "the file must be truncated to zero bytes"
        );

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn redirect_only_truncates_existing_file() {
        let _stdio = STDIO_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use crate::exec::Executor;
        use crate::parser::Parser;

        let tmp = std::env::temp_dir().join(format!(
            "yosh_redirect_only_trunc_{}.txt",
            std::process::id()
        ));
        std::fs::write(&tmp, b"existing-content").unwrap();

        let source = format!(">{}\n", tmp.display());
        let prog = Parser::new(&source).parse_program().unwrap();
        let mut exec = Executor::new("yosh", vec![]);
        exec.exec_program(&prog);

        let metadata = std::fs::metadata(&tmp).unwrap();
        assert_eq!(metadata.len(), 0, "existing file must be truncated");

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn redirect_only_returns_zero() {
        let _stdio = STDIO_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use crate::exec::Executor;
        use crate::parser::Parser;

        let tmp =
            std::env::temp_dir().join(format!("yosh_redirect_only_rc_{}.txt", std::process::id()));
        let source = format!(">{}\n", tmp.display());
        let prog = Parser::new(&source).parse_program().unwrap();
        let mut exec = Executor::new("yosh", vec![]);
        let status = exec.exec_program(&prog);

        assert_eq!(status, 0, "redirect-only command returns 0 on success");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn redirect_only_with_readonly_assignment_errors_cleanly() {
        let _stdio = STDIO_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use crate::exec::Executor;
        use crate::parser::Parser;

        let tmp =
            std::env::temp_dir().join(format!("yosh_redirect_readonly_{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        let mut exec = Executor::new("yosh", vec![]);
        // Mark x readonly so the assignment x=replaced will fail.
        exec.env.vars.set("x", "initial").expect("set must succeed");
        exec.env.vars.set_readonly("x");

        // `>tmp x=replaced` — assignment-only with redirect; readonly x triggers error.
        let source = format!(">{} x=replaced\n", tmp.display());
        let prog = Parser::new(&source).parse_program().unwrap();
        let status = exec.exec_program(&prog);

        // The readonly error must yield non-zero exit status.
        // Crucially, this exercises the error path with redirect_state.restore()
        // so that stdout/stderr are not leaked for subsequent tests.
        assert_ne!(
            status, 0,
            "readonly assignment must error; status={}",
            status
        );

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn assignment_only_under_xtrace_returns_ok_and_sets_var() {
        use crate::exec::Executor;
        use crate::parser::Parser;

        // Verifies the assignment-only branch's control flow is unchanged
        // by the trace insertion: variable assignment succeeds and the
        // command returns exit status 0 with xtrace enabled.
        let source = "set -x\nfoo=bar";
        let prog = Parser::new(source).parse_program().unwrap();
        let mut exec = Executor::new("yosh", vec![]);
        exec.exec_program(&prog);
        assert_eq!(exec.env.vars.get("foo"), Some("bar"));
        assert_eq!(exec.env.exec.last_exit_status, 0);
    }

    /// POSIX §2.8.1: a special built-in utility error exits only a
    /// NON-interactive shell, and never when reached through `command`.
    #[test]
    fn special_builtin_error_requests_exit_only_when_noninteractive() {
        use crate::exec::Executor;
        use crate::parser::Parser;

        let run = |src: &str, interactive: bool| {
            let mut exec = Executor::new("yosh", vec![]);
            exec.env.mode.is_interactive = interactive;
            let prog = Parser::new(src).parse_program().unwrap();
            exec.exec_program(&prog);
            (exec.exit_requested, exec.env.vars.get("after").is_some())
        };
        assert_eq!(run("unset -Z x; after=1", false), (Some(2), false));
        assert_eq!(run("unset -Z x; after=1", true), (None, true));
        assert_eq!(run("command unset -Z x; after=1", false), (None, true));
        assert_eq!(
            run("readonly r=1; export r=2; after=1", false),
            (Some(1), false)
        );
        // A nonzero status that is not an error (`eval false`) does not exit.
        assert_eq!(run("eval false; after=1", false), (None, true));
    }
}
