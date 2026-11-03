//! Commands without a command word: only assignments and/or redirections.

use crate::error::ShellError;
use crate::parser::ast::{Assignment, ParamExpr, SimpleCommand, Word, WordPart};

use super::super::Executor;
use super::super::redirect::RedirectState;

impl Executor {
    /// Execute an assignment-only command (no words, or all words expanded
    /// to empty): its redirections are performed and undone, and its
    /// assignments persist.
    ///
    /// POSIX §2.9.1: exit status of an assignment-only command is the status
    /// of the last command substitution performed, or 0 if none. Both
    /// command-word (`$(false)` alone) and assignment-value substitutions
    /// are eligible. Word-side substitutions already updated
    /// env.exec.last_exit_status during the word expansion.
    pub(super) fn exec_assignment_only(&mut self, cmd: &SimpleCommand) -> Result<i32, ShellError> {
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
        redirect_state.apply(&cmd.redirects, &mut self.env, true)?;
        let assigned = self.assign_all_persistent(&cmd.assignments, &mut last_cmd_sub_status);
        redirect_state.restore();
        assigned?;

        let final_status = last_cmd_sub_status.unwrap_or(0);
        self.env.exec.last_exit_status = final_status;
        Ok(final_status)
    }

    /// Perform the assignments of an assignment-only command in order,
    /// recording in `last_cmd_sub_status` the status of each value's
    /// command substitution. Stops at the first error.
    fn assign_all_persistent(
        &mut self,
        assignments: &[Assignment],
        last_cmd_sub_status: &mut Option<i32>,
    ) -> Result<(), ShellError> {
        for assignment in assignments {
            let has_cmd_sub = assignment.value.as_ref().is_some_and(word_has_command_sub);
            // A failed expansion (`x=${zz:?}`, `set -u; x=$zz`) must
            // neither assign nor report status 0: it is an expansion
            // error (POSIX §2.8.1).
            let value = self.expand_assignment(assignment)?;
            // Trace before the variable is set so a readonly-failure still
            // produces the trace line (bash behaviour). Nested command-sub
            // traces inside `value` have already fired during expansion.
            self.xtrace_assignment(&assignment.name, &value);
            // If the value expansion contained a command substitution, $?
            // now reflects its exit status. Record it regardless of whether
            // the delta was non-zero — the substitution may coincidentally
            // return the same status as the prior command (e.g.
            // `false; x=$(false)`).
            if has_cmd_sub {
                *last_cmd_sub_status = Some(self.env.exec.last_exit_status);
            }
            self.assign_persistent(&assignment.name, value)?;
        }
        Ok(())
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
}
