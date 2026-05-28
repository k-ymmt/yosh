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
    pub exit_requested: Option<i32>,
}

impl Executor {
    pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
        Executor {
            env: ShellEnv::new(shell_name, args),
            plugins: PluginManager::new(),
            errexit_suppressed_depth: 0,
            exit_requested: None,
        }
    }

    /// Create an Executor from an existing ShellEnv (e.g. for subshell/command substitution).
    pub fn from_env(env: ShellEnv) -> Self {
        Executor {
            env,
            plugins: PluginManager::new(),
            errexit_suppressed_depth: 0,
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
        let content = std::fs::read_to_string(path).ok()?;
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

    /// Errexit check after command execution.
    pub fn check_errexit(&mut self, status: i32) {
        if status != 0 && self.should_errexit() {
            self.execute_exit_trap();
            if self.env.mode.is_interactive {
                self.exit_requested = Some(status);
            } else {
                std::process::exit(status);
            }
        }
    }

    /// Execute the EXIT trap if set.
    pub fn execute_exit_trap(&mut self) {
        if let Some(crate::env::TrapAction::Command(cmd)) = self.env.traps.exit_trap.take() {
            self.with_errexit_suppressed(|exec| {
                exec.eval_string(&cmd);
            });
        }
    }

    /// Process any pending signals from the self-pipe.
    pub fn process_pending_signals(&mut self) {
        let signals = signal::drain_pending_signals();
        for sig in signals {
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
                    self.with_errexit_suppressed(|exec| {
                        exec.eval_string(&cmd);
                    });
                }
                continue;
            }

            match self.env.traps.get_signal_trap(sig).cloned() {
                Some(crate::env::TrapAction::Command(cmd)) => {
                    self.with_errexit_suppressed(|exec| {
                        exec.eval_string(&cmd);
                    });
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
        self.execute_exit_trap();
        if self.env.mode.is_interactive {
            self.exit_requested = Some(128 + sig);
        } else {
            std::process::exit(128 + sig);
        }
    }

    /// Evaluate a string as shell commands (used by trap actions and eval).
    pub fn eval_string(&mut self, input: &str) {
        if let Ok(program) =
            crate::parser::Parser::new_with_aliases(input, &self.env.aliases).parse_program()
        {
            self.exec_program(&program);
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
        let mut exec = Executor::new("yosh", vec![]);
        let dir = std::env::temp_dir();
        let path = dir.join("yosh_test_source_file.sh");
        std::fs::write(&path, "MY_TEST_VAR=hello_from_rc\n").unwrap();
        let result = exec.source_file(&path);
        std::fs::remove_file(&path).ok();
        assert_eq!(result, Some(0));
        assert_eq!(exec.env.vars.get("MY_TEST_VAR"), Some("hello_from_rc"));
    }

    #[test]
    fn source_file_parse_error_returns_some_2() {
        let mut exec = Executor::new("yosh", vec![]);
        let dir = std::env::temp_dir();
        let path = dir.join("yosh_test_source_parse_error.sh");
        std::fs::write(&path, "if\n").unwrap();
        let result = exec.source_file(&path);
        std::fs::remove_file(&path).ok();
        assert_eq!(result, Some(2));
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
