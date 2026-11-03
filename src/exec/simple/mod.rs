//! Simple-command execution (POSIX §2.9.1): word expansion and dispatch
//! to assignments, functions, built-ins and external utilities.

mod assign;
mod assignment_only;
mod builtin;
mod command_builtin;
mod cstrings;
mod external;
mod resolve;
mod xtrace;

use crate::builtin::{BuiltinKind, classify_builtin};
use crate::env::ShellEnv;
use crate::error::{ShellError, ShellErrorKind};
use crate::expand::expand_words;
use crate::parser::ast::{Assignment, SimpleCommand, Word};
use crate::parser::try_parse_assignment;

use super::Executor;
use super::redirect::{ExpandedRedirect, expand_redirects};
use builtin::executor_hosted_builtin;

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

/// `name args...` joined with spaces: the command line handed to plugin
/// exec hooks and recorded as a foreground job's command text.
fn command_line(name: &str, args: &[String]) -> String {
    std::iter::once(name)
        .chain(args.iter().map(|s| s.as_str()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A simple command whose words and redirections have been expanded,
/// ready to be dispatched to a function, built-in or external utility.
struct Invocation<'a> {
    /// The command's prefix assignments, not yet expanded.
    assignments: &'a [Assignment],
    name: &'a str,
    args: &'a [String],
    redirs: &'a [ExpandedRedirect],
    /// The command line for plugin pre-/post-exec hooks; empty when no
    /// loaded plugin registered either hook.
    hook_cmd: &'a str,
}

impl Executor {
    /// Execute a simple command (assignments, builtins, or external programs).
    ///
    /// Every error leaves `$?` at 1 (the caller reports the error and may
    /// replace it with the error's own exit status).
    pub(crate) fn exec_simple_command(&mut self, cmd: &SimpleCommand) -> Result<i32, ShellError> {
        self.dispatch_simple_command(cmd)
            .inspect_err(|_| self.env.exec.last_exit_status = 1)
    }

    fn dispatch_simple_command(&mut self, cmd: &SimpleCommand) -> Result<i32, ShellError> {
        self.env.exec.lineno = cmd.line;

        // Take (read + clear) the async exec-in-place marker BEFORE any
        // expansion: command substitutions below fork children whose
        // cloned ShellEnv must not inherit a set flag, and nested
        // exec_simple_command calls (function bodies, `command`) must
        // not consume it. Only the external-utility dispatch at the
        // bottom honors it; every other dispatch ignores it and keeps
        // wrapper-subshell semantics.
        let async_exec_in_place = std::mem::take(&mut self.env.exec.async_exec_in_place);

        let Some(expanded) = self.expand_command_words(&cmd.words)? else {
            // Expansion triggered a flow control signal (e.g. a nounset
            // error): the command is not run.
            self.env.exec.last_exit_status = 1;
            return Ok(1);
        };

        // Assignment-only command (no words, or all words expanded to empty)
        if expanded.is_empty() {
            return self.exec_assignment_only(cmd);
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
            // A here-document body failure is a redirection error, which
            // POSIX §2.8.1 makes fatal for a special built-in in a
            // non-interactive shell (expansion errors are fatal for every
            // command and handled by the caller).
            Err(e)
                if matches!(kind, BuiltinKind::Special)
                    && !matches!(e.kind, ShellErrorKind::Expansion(_)) =>
            {
                return Err(self.special_builtin_redirect_error(e));
            }
            Err(e) => return Err(e),
        };

        // Build command string for hooks — only when some loaded plugin
        // actually registered pre-exec or post-exec, since this joined
        // string is otherwise built and immediately discarded on every
        // single simple command.
        let hook_cmd = if self.plugins.has_exec_hooks() {
            command_line(&command_name, &args)
        } else {
            String::new()
        };
        self.plugins.call_pre_exec(&mut self.env, &hook_cmd);

        let inv = Invocation {
            assignments: &cmd.assignments,
            name: &command_name,
            args: &args,
            redirs: &redirs,
            hook_cmd: &hook_cmd,
        };

        // Check for function call. POSIX §2.9.1.4: special built-ins are
        // found before functions, so a function may never shadow one;
        // functions do take precedence over regular built-ins and PATH.
        if !matches!(kind, BuiltinKind::Special)
            // `.cloned()` here is an Rc refcount bump, not a deep copy of
            // the definition — the table stores `Rc<FunctionDef>`.
            && let Some(func_def) = self.env.functions.get(&command_name).cloned()
        {
            let status = self.run_with_temp_assignments(&inv, |exec| {
                exec.exec_function_call(&func_def, &args)
            })?;
            return Ok(self.finish_simple_command(&hook_cmd, status));
        }

        // wait / fg / bg / jobs / command / fc are regular built-ins that
        // live on the Executor (see `executor_hosted_builtin`).
        if let Some(builtin) = executor_hosted_builtin(&command_name) {
            let status = self.run_with_temp_assignments(&inv, |exec| builtin(exec, &args))?;
            return Ok(self.finish_simple_command(&hook_cmd, status));
        }

        match kind {
            BuiltinKind::Special => self.exec_special_builtin_command(&inv),
            BuiltinKind::Regular => self.exec_regular_builtin_command(&inv),
            BuiltinKind::NotBuiltin => self.exec_non_builtin_command(&inv, async_exec_in_place),
        }
    }

    /// Expand the command words into fields. `None` when an expansion
    /// raised a flow-control signal (e.g. a nounset error), which aborts
    /// the command.
    ///
    /// ONLY the command name is expanded first, so we can dispatch to an
    /// assignment-aware expansion for export/readonly. A single early
    /// expansion of all words would fire side effects (command
    /// substitutions, arithmetic, `${x=...}`) on every argument — the
    /// assignment-aware re-expansion used by export/readonly would then
    /// run them a second time. Gate the full expansion on the detected
    /// command name instead.
    fn expand_command_words(&mut self, words: &[Word]) -> Result<Option<Vec<String>>, ShellError> {
        let name_fields = match words.first() {
            Some(name) => expand_words(&mut self.env, std::slice::from_ref(name))?,
            None => Vec::new(),
        };
        if self.env.exec.flow_control.is_some() {
            return Ok(None);
        }

        // Determine the effective command name (first field after expansion,
        // which may be empty when `words[0]` expanded to nothing e.g. an
        // unset variable with nullglob-like semantics).
        let probable_name: Option<&str> = name_fields.first().map(|s| s.as_str());
        let is_assignment_builtin = matches!(probable_name, Some("export") | Some("readonly"));

        // Expand the remaining words. For export/readonly, route through the
        // assignment-aware expander so each Word is expanded exactly once
        // (preserving EscapedLiteral / Tilde metadata for `NAME=\~/path`
        // style arguments). For all other commands, use the normal
        // field-splitting expander.
        let rest_fields: Vec<String> = if words.len() <= 1 {
            Vec::new()
        } else if is_assignment_builtin {
            expand_assignment_builtin_args(&mut self.env, &words[1..])?
        } else {
            expand_words(&mut self.env, &words[1..])?
        };
        if self.env.exec.flow_control.is_some() {
            return Ok(None);
        }

        // Stitch: fields from the command-name word come first, remaining
        // words' fields follow. This matches the ordering the original
        // single `expand_words(words)` would have produced.
        let mut expanded: Vec<String> = Vec::with_capacity(name_fields.len() + rest_fields.len());
        expanded.extend(name_fields);
        expanded.extend(rest_fields);
        Ok(Some(expanded))
    }

    /// Final bookkeeping for a command that ran: the plugins' post-exec
    /// hook, then `$?`. Returns `status`.
    fn finish_simple_command(&mut self, hook_cmd: &str, status: i32) -> i32 {
        self.plugins.call_post_exec(&mut self.env, hook_cmd, status);
        self.env.exec.last_exit_status = status;
        status
    }
}
