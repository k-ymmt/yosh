pub mod command;
mod compound;
mod control;
mod function;
mod job_control;
pub mod pipeline;
pub mod redirect;
mod simple;
pub(crate) mod terminal_state;

use crate::env::ShellEnv;
use crate::parser::ast::{AndOrList, Command, WordPart};
use crate::plugin::PluginManager;
use crate::signal;

/// Exit a post-fork child process safely.
///
/// Uses `libc::_exit` to skip Rust runtime cleanup, which can deadlock
/// on std-internal mutexes inherited locked from a multithreaded parent
/// (e.g. `std::sys::pal::unix::stack_overflow::thread_info::LOCK`).
/// Flushes stdout/stderr first so buffered output is not lost.
///
/// Use ONLY after `fork()` in the child branch, never in the shell parent.
pub(crate) fn exit_child(status: i32) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    unsafe { libc::_exit(status) }
}

/// True once this process is a post-fork child of the shell (subshell,
/// pipeline member, command substitution, or async list). Set in every
/// `ForkResult::Child` branch so that exit paths reached *during* child
/// execution (e.g. the `exit` special built-in) can pick the fork-safe
/// exit.
static IN_FORKED_CHILD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Mark this process as a post-fork child. Call first thing in every
/// `ForkResult::Child` branch.
pub(crate) fn mark_forked_child() {
    IN_FORKED_CHILD.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Exit the shell process, using [`exit_child`] when this process is a
/// post-fork child: `std::process::exit` runs Rust runtime cleanup that
/// can deadlock on std-internal mutexes inherited locked from a
/// multithreaded parent.
pub(crate) fn shell_exit(status: i32) -> ! {
    if IN_FORKED_CHILD.load(std::sync::atomic::Ordering::Relaxed) {
        exit_child(status);
    }
    std::process::exit(status);
}

/// Reconstruct a short, human-readable preview of an AndOrList for display in
/// `jobs` output and for `%string` / `%?string` job-spec matching against
/// `Job.command`. Uses the literal words of the first simple command when the
/// pipeline starts with one and every word is purely literal; falls back to
/// "(background)" otherwise (compound commands, unexpanded parameters, command
/// substitutions in the command word, etc.).
fn preview_command(and_or: &AndOrList) -> String {
    let Some(Command::Simple(sc)) = and_or.first.commands.first() else {
        return "(background)".to_string();
    };
    if sc.words.is_empty() {
        return "(background)".to_string();
    }
    let mut words = Vec::with_capacity(sc.words.len());
    for w in &sc.words {
        let mut s = String::new();
        for part in &w.parts {
            match part {
                WordPart::Literal(lit) => s.push_str(lit),
                WordPart::EscapedLiteral(lit) => s.push_str(lit),
                WordPart::SingleQuoted(lit) => {
                    s.push('\'');
                    s.push_str(lit);
                    s.push('\'');
                }
                _ => return "(background)".to_string(),
            }
        }
        words.push(s);
    }
    words.join(" ")
}

pub struct Executor {
    pub env: ShellEnv,
    pub plugins: PluginManager,
    errexit_suppressed_depth: usize,
    /// Set by `exec_and_or` when the status it returned is exempt from
    /// `set -e` per POSIX §2.15 set: the pipeline that produced it began
    /// with `!`, or it came from a non-final component of an AND-OR list
    /// (short-circuit). Consumed by `check_errexit`.
    errexit_exempt_status: bool,
    pub exit_requested: Option<i32>,
}

impl Executor {
    pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
        Executor {
            env: ShellEnv::new(shell_name, args),
            plugins: PluginManager::new(),
            errexit_suppressed_depth: 0,
            errexit_exempt_status: false,
            exit_requested: None,
        }
    }

    /// Create an Executor from an existing ShellEnv (e.g. for subshell/command substitution).
    pub fn from_env(env: ShellEnv) -> Self {
        Executor {
            env,
            plugins: PluginManager::new(),
            errexit_suppressed_depth: 0,
            errexit_exempt_status: false,
            exit_requested: None,
        }
    }

    /// Load plugins from the lock file (~/.config/yosh/plugins.lock).
    pub fn load_plugins(&mut self) {
        let config_path = plugin_config_path();
        self.plugins.load_from_config(&config_path, &mut self.env);
    }

    /// Source a file in the current shell context.
    /// Returns `None` if the file doesn't exist, `Some(status)` otherwise.
    pub fn source_file(&mut self, path: &std::path::Path) -> Option<i32> {
        // Read as bytes so non-UTF-8 script content is preserved via the
        // byteenc escape encoding instead of failing with InvalidData.
        let raw = std::fs::read(path).ok()?;
        let content = crate::byteenc::encode_bytes(&raw).into_owned();
        let prev_dot_script = self.env.mode.in_dot_script;
        self.env.mode.in_dot_script = true;
        self.env.exec.indirection_level += 1;
        let status = match crate::parser::Parser::new_with_aliases(&content, &self.env.aliases)
            .parse_program()
        {
            Ok(program) => {
                let s = self.exec_program(&program);
                if let Some(crate::env::FlowControl::Return(code)) = self.env.exec.flow_control {
                    self.env.exec.flow_control = None;
                    self.env.mode.in_dot_script = prev_dot_script;
                    self.env.exec.indirection_level -= 1;
                    return Some(code);
                }
                s
            }
            Err(e) => {
                eprintln!("yosh: {}", e);
                2
            }
        };
        self.env.mode.in_dot_script = prev_dot_script;
        self.env.exec.indirection_level -= 1;
        Some(status)
    }

    /// Execute closure within errexit-suppressed context.
    pub fn with_errexit_suppressed<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.errexit_suppressed_depth += 1;
        let result = f(self);
        self.errexit_suppressed_depth -= 1;
        result
    }

    /// Check if errexit is active and not suppressed.
    pub fn should_errexit(&self) -> bool {
        self.env.mode.options.errexit && self.errexit_suppressed_depth == 0
    }

    /// Drop any propagated errexit exemption (used at boundaries the
    /// exemption must not cross, e.g. function-call return).
    pub(crate) fn clear_errexit_exempt(&mut self) {
        self.errexit_exempt_status = false;
    }

    /// Errexit check after command execution.
    pub fn check_errexit(&mut self, status: i32) {
        if status != 0 && !self.errexit_exempt_status && self.should_errexit() {
            self.execute_exit_trap();
            if self.env.mode.is_interactive {
                self.exit_requested = Some(status);
            } else {
                shell_exit(status);
            }
        }
    }

    /// Execute a trap action string, exposing the pre-trap `$?` to the
    /// `exit` special built-in via `trap_context_status` (POSIX §2.12:
    /// `exit` without an operand inside a trap action uses the value `$?`
    /// had when the trap action started).
    ///
    /// Used by the EXIT-trap path, which keeps errexit suppressed inside
    /// the action (see `test_errexit_trap_action_suppressed`); signal
    /// traps go through [`Self::run_signal_trap_action`] instead.
    pub(crate) fn run_trap_action(&mut self, cmd: &str) {
        let prev = self
            .env
            .exec
            .trap_context_status
            .replace(self.env.exec.last_exit_status);
        self.with_errexit_suppressed(|exec| {
            exec.eval_string(cmd);
        });
        self.env.exec.trap_context_status = prev;
    }

    /// Execute a SIGNAL trap action (POSIX §2.12). Differences from the
    /// EXIT-trap path ([`Self::run_trap_action`]):
    ///
    /// - On completion, `$?` is restored to its pre-trap value so the
    ///   interrupted command sequence observes its own status, not the
    ///   trap action's (`trap 'false' USR1; kill -USR1 $$; echo $?`
    ///   prints 0 — bash/dash/zsh agree). The restore is skipped when
    ///   the action itself requested an exit (`exit` builtin, errexit,
    ///   fatal error) or flow control (`break`/`continue`), whose
    ///   resulting status must survive.
    /// - errexit is NOT suppressed: `set -e; trap 'false' USR1;
    ///   kill -USR1 $$; echo ok` exits 1 without printing ok
    ///   (bash/dash/zsh agree).
    ///
    /// The pre-trap `$?` is still exposed to `exit` without an operand
    /// via `trap_context_status`.
    fn run_signal_trap_action(&mut self, cmd: &str) {
        let saved_status = self.env.exec.last_exit_status;
        let prev = self.env.exec.trap_context_status.replace(saved_status);
        self.eval_string(cmd);
        self.env.exec.trap_context_status = prev;
        if self.exit_requested.is_none() && self.env.exec.flow_control.is_none() {
            self.env.exec.last_exit_status = saved_status;
        }
    }

    /// Exit the whole shell process from the shell parent (NOT a forked
    /// child — post-fork children must use [`exit_child`]).
    ///
    /// Fires the EXIT trap first (POSIX §2.12: the EXIT trap runs on any
    /// shell exit, including a fatal error such as a special-builtin
    /// redirection failure in a non-interactive shell — dash agrees),
    /// then exits via `std::process::exit`, matching the exit path used
    /// by `check_errexit` and `handle_default_signal`.
    pub(crate) fn exit_shell(&mut self, status: i32) -> ! {
        self.execute_exit_trap();
        shell_exit(status);
    }

    /// Execute the EXIT trap if set.
    pub fn execute_exit_trap(&mut self) {
        if let Some(crate::env::TrapAction::Command(cmd)) = self.env.traps.exit_trap.take() {
            self.run_trap_action(&cmd);
        }
    }

    /// Process any pending signals from the self-pipe.
    pub fn process_pending_signals(&mut self) {
        let signals = signal::drain_pending_signals();
        self.run_signal_traps(&signals);
    }

    /// Run the trap/default action for each already-drained signal.
    /// Split from `process_pending_signals` so callers that drained the
    /// self-pipe themselves (e.g. `wait`) can still fire the trap actions.
    pub(crate) fn run_signal_traps(&mut self, signals: &[i32]) {
        for &sig in signals {
            // SIGCHLD default action is to ignore (just reap children).
            // We must not route it through handle_default_signal which
            // exits the shell.  Reaping is already handled by
            // reap_zombies() in the interactive loop.
            if sig == libc::SIGCHLD {
                // Default and Ignore: just ignore SIGCHLD (reaping is done
                // elsewhere). Only the user-installed `Command` trap runs.
                if let Some(crate::env::TrapAction::Command(cmd)) =
                    self.env.traps.get_signal_trap(sig).cloned()
                {
                    self.run_signal_trap_action(&cmd);
                }
                continue;
            }

            match self.env.traps.get_signal_trap(sig).cloned() {
                Some(crate::env::TrapAction::Command(cmd)) => {
                    self.run_signal_trap_action(&cmd);
                }
                Some(crate::env::TrapAction::Ignore) => {}
                Some(crate::env::TrapAction::Default) | None => {
                    self.handle_default_signal(sig);
                }
            }
        }
    }

    /// Handle a signal with default behavior (terminate).
    pub(crate) fn handle_default_signal(&mut self, sig: i32) {
        // POSIX sh: an interactive shell ignores untrapped SIGQUIT and
        // SIGTERM, and untrapped SIGINT only discards the current line —
        // none of the three may terminate the shell (bash/dash survive
        // `kill -INT/-QUIT/-TERM $$` silently). The EXIT trap must not
        // run either, since the shell is not exiting.
        if self.env.mode.is_interactive
            && matches!(sig, libc::SIGINT | libc::SIGQUIT | libc::SIGTERM)
        {
            return;
        }
        self.execute_exit_trap();
        if self.env.mode.is_interactive {
            self.exit_requested = Some(128 + sig);
        } else {
            shell_exit(128 + sig);
        }
    }

    /// Evaluate a string as shell commands (used by trap actions and eval).
    pub fn eval_string(&mut self, input: &str) {
        match crate::parser::Parser::new_with_aliases(input, &self.env.aliases).parse_program() {
            Ok(program) => {
                self.exec_program(&program);
            }
            Err(e) => {
                // A trap action with a syntax error must not fail silently
                // at fire time (bash/dash print the diagnostic).
                // ShellError's Display already carries the "yosh: " prefix.
                eprintln!("{}", e);
            }
        }
    }

    /// Print the line if verbose mode is enabled.
    pub fn verbose_print(&self, line: &str) {
        if self.env.mode.options.verbose {
            eprintln!("{}", line);
        }
    }
}

fn plugin_config_path() -> std::path::PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        std::path::PathBuf::from(home).join(".config/yosh/plugins.lock")
    } else {
        std::path::PathBuf::from("/nonexistent")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_errexit_default_off() {
        let exec = Executor::new("yosh", vec![]);
        assert!(!exec.should_errexit());
    }

    #[test]
    fn test_should_errexit_enabled() {
        let mut exec = Executor::new("yosh", vec![]);
        exec.env.mode.options.errexit = true;
        assert!(exec.should_errexit());
    }

    #[test]
    fn test_with_errexit_suppressed() {
        let mut exec = Executor::new("yosh", vec![]);
        exec.env.mode.options.errexit = true;
        assert!(exec.should_errexit());
        let result = exec.with_errexit_suppressed(|e| {
            assert!(!e.should_errexit());
            42
        });
        assert_eq!(result, 42);
        assert!(exec.should_errexit());
    }

    #[test]
    fn test_with_errexit_suppressed_nested() {
        let mut exec = Executor::new("yosh", vec![]);
        exec.env.mode.options.errexit = true;
        exec.with_errexit_suppressed(|e| {
            assert!(!e.should_errexit());
            e.with_errexit_suppressed(|e2| {
                assert!(!e2.should_errexit());
            });
            assert!(!e.should_errexit());
        });
        assert!(exec.should_errexit());
    }

    #[test]
    fn plugin_config_path_points_to_lock_file() {
        let path = super::plugin_config_path();
        assert!(path.to_string_lossy().ends_with("plugins.lock"));
    }

    #[test]
    fn exit_requested_defaults_to_none() {
        let exec = Executor::new("yosh", vec![]);
        assert_eq!(exec.exit_requested, None);
    }

    #[test]
    fn handle_default_signal_sets_exit_requested_in_interactive_mode() {
        let mut exec = Executor::new("yosh", vec![]);
        exec.env.mode.is_interactive = true;
        exec.handle_default_signal(libc::SIGHUP);
        assert_eq!(exec.exit_requested, Some(128 + libc::SIGHUP));
    }

    #[test]
    fn handle_default_signal_ignores_term_quit_int_in_interactive_mode() {
        // POSIX sh: interactive shells ignore untrapped SIGTERM/SIGQUIT,
        // and untrapped SIGINT must not exit the shell either.
        for sig in [libc::SIGTERM, libc::SIGQUIT, libc::SIGINT] {
            let mut exec = Executor::new("yosh", vec![]);
            exec.env.mode.is_interactive = true;
            exec.handle_default_signal(sig);
            assert_eq!(
                exec.exit_requested, None,
                "interactive shell must not exit on signal {sig}"
            );
        }
    }

    #[test]
    fn handle_default_signal_ignore_does_not_consume_exit_trap() {
        // The ignored-signal early return must not fire (or take) the
        // EXIT trap — the shell is not exiting.
        let mut exec = Executor::new("yosh", vec![]);
        exec.env.mode.is_interactive = true;
        exec.env
            .traps
            .set_trap("EXIT", crate::env::TrapAction::Command("x=exited".into()))
            .unwrap();
        exec.handle_default_signal(libc::SIGTERM);
        assert_eq!(exec.env.vars.get("x"), None);
        assert!(exec.env.traps.exit_trap.is_some());
    }

    #[test]
    fn run_signal_traps_runs_term_trap_in_interactive_mode() {
        // A user-installed trap on TERM still fires in an interactive
        // shell — only the *untrapped* default is ignored.
        let mut exec = Executor::new("yosh", vec![]);
        exec.env.mode.is_interactive = true;
        exec.env
            .traps
            .set_trap("TERM", crate::env::TrapAction::Command("x=trapped".into()))
            .unwrap();
        exec.run_signal_traps(&[libc::SIGTERM]);
        assert_eq!(exec.env.vars.get("x"), Some("trapped"));
        assert_eq!(exec.exit_requested, None);
    }

    #[test]
    fn check_errexit_sets_exit_requested_in_interactive_mode() {
        let mut exec = Executor::new("yosh", vec![]);
        exec.env.mode.is_interactive = true;
        exec.env.mode.options.errexit = true;
        exec.check_errexit(1);
        assert_eq!(exec.exit_requested, Some(1));
    }

    #[test]
    fn source_file_nonexistent_returns_none() {
        let mut exec = Executor::new("yosh", vec![]);
        let result = exec.source_file(std::path::Path::new("/nonexistent/file.sh"));
        assert_eq!(result, None);
    }

    #[test]
    fn source_file_sets_variable() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "MY_TEST_VAR=hello_from_rc").unwrap();
        let mut exec = Executor::new("yosh", vec![]);
        let result = exec.source_file(tmp.path());
        assert_eq!(result, Some(0));
        assert_eq!(exec.env.vars.get("MY_TEST_VAR"), Some("hello_from_rc"));
    }

    #[test]
    fn source_file_parse_error_returns_some_2() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "if").unwrap();
        let mut exec = Executor::new("yosh", vec![]);
        let result = exec.source_file(tmp.path());
        assert_eq!(result, Some(2));
    }

    // ── preview_command ──

    fn first_and_or(src: &str) -> AndOrList {
        let prog = crate::parser::Parser::new(src).parse_program().unwrap();
        prog.commands[0].items[0].0.clone()
    }

    #[test]
    fn preview_command_literal_words() {
        assert_eq!(preview_command(&first_and_or("sleep 5")), "sleep 5");
    }

    #[test]
    fn preview_command_single_quoted_word_kept_quoted() {
        assert_eq!(
            preview_command(&first_and_or("echo 'a b' c")),
            "echo 'a b' c"
        );
    }

    #[test]
    fn preview_command_pipeline_uses_first_simple_command() {
        assert_eq!(preview_command(&first_and_or("sleep 5 | cat")), "sleep 5");
    }

    #[test]
    fn preview_command_compound_falls_back() {
        assert_eq!(
            preview_command(&first_and_or("( echo hi )")),
            "(background)"
        );
        assert_eq!(
            preview_command(&first_and_or("while true; do :; done")),
            "(background)"
        );
    }

    #[test]
    fn preview_command_unexpandable_word_falls_back() {
        // Parameter expansion and command substitution in any word are not
        // previewable without expansion.
        assert_eq!(preview_command(&first_and_or("echo $x")), "(background)");
        assert_eq!(
            preview_command(&first_and_or("echo $(date)")),
            "(background)"
        );
    }

    #[test]
    fn indirection_level_balanced_after_function_call() {
        use crate::parser::Parser;
        let mut exec = Executor::new("yosh", vec![]);
        let prog = Parser::new("f() { :; }; f").parse_program().unwrap();
        exec.exec_program(&prog);
        assert_eq!(exec.env.exec.indirection_level, 0);
    }

    #[test]
    fn indirection_level_balanced_after_dot_script() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, ":").unwrap();
        let mut exec = Executor::new("yosh", vec![]);
        exec.source_file(tmp.path());
        assert_eq!(exec.env.exec.indirection_level, 0);
    }

    #[test]
    fn indirection_level_balanced_after_dot_script_early_return() {
        // A `return` inside a sourced script takes the early-return path in
        // source_file; the decrement on that path must still fire.
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "return 0").unwrap();
        let mut exec = Executor::new("yosh", vec![]);
        exec.source_file(tmp.path());
        assert_eq!(exec.env.exec.indirection_level, 0);
    }
}
