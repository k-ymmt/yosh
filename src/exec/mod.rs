pub mod command;
mod compound;
mod control;
mod function;
mod job_control;
pub mod pipeline;
mod process;
pub mod redirect;
mod simple;
pub(crate) mod terminal_state;
mod trap;

pub(crate) use process::{
    block_all_signals, exit_child, fork_shell, mark_forked_child, restore_signal_mask, shell_exit,
    stdin_from_dev_null,
};

use crate::env::ShellEnv;
use crate::parser::ast::{AndOrList, Command, WordPart};
use crate::plugin::PluginManager;

/// Render one pipeline member when it is a simple command with purely
/// literal words. `None` otherwise (compound commands, redirect-only
/// commands without words, unexpanded parameters / command substitutions
/// — rendering those would require expansion).
fn preview_member(cmd: &Command) -> Option<String> {
    let Command::Simple(sc) = cmd else {
        return None;
    };
    if sc.words.is_empty() {
        return None;
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
                _ => return None,
            }
        }
        words.push(s);
    }
    Some(words.join(" "))
}

/// Human-readable preview of a pipeline for `jobs` output: the whole
/// pipeline joined with " | " when every member renders; otherwise the
/// FIRST member alone (the pre-2026-09-02 behavior, kept as a fallback
/// so `%string` job specs still resolve when a later member is
/// redirect-only or has dynamic words — `sleep 10 | >/dev/null &` must
/// stay addressable as `%sleep`); `None` when even that fails.
fn preview_pipeline(pipeline: &crate::parser::ast::Pipeline) -> Option<String> {
    pipeline
        .commands
        .iter()
        .map(preview_member)
        .collect::<Option<Vec<String>>>()
        .map(|members| members.join(" | "))
        .or_else(|| pipeline.commands.first().and_then(preview_member))
}

/// Reconstruct a short, human-readable preview of an AndOrList for display in
/// `jobs` output and for `%string` / `%?string` job-spec matching against
/// `Job.command`. Uses the literal rendering of the first pipeline (see
/// `preview_pipeline`); falls back to "(background)" otherwise.
fn preview_command(and_or: &AndOrList) -> String {
    preview_pipeline(&and_or.first).unwrap_or_else(|| "(background)".to_string())
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
    /// Load the plugins recorded in `plugins.lock`.
    ///
    /// `interactive` selects the load policy: interactive shells load
    /// every enabled plugin (hooks and prompt integration need them),
    /// while non-interactive runs (`yosh script.sh`, `yosh -c`) only
    /// load plugins that can be reached from a script — those that
    /// provide custom commands. Hooks are an interactive-shell feature,
    /// like zsh's `preexec`; skipping them in scripts avoids paying a
    /// plugin's startup work (measured 2026-09-16: ~32 ms per script
    /// invocation for a prompt plugin that runs external commands in
    /// its init/pre-exec path) on every script run.
    pub fn load_plugins(&mut self, interactive: bool) {
        let config_path = plugin_config_path();
        self.plugins
            .load_from_config(&config_path, &mut self.env, interactive);
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

    /// True when the command that just ran left the executor unwinding:
    /// a pending flow-control signal (`break` / `continue` / `return`, a
    /// fatal expansion error) or a pending shell exit (`exit`, a fatal
    /// special-builtin or assignment error in a non-interactive shell).
    /// Every compound construct checks this after each sub-list instead
    /// of acting on the status it reported — e.g. a fatal error in an
    /// `if` condition must not run the `else` branch (`readonly r=1; if
    /// r=2 true; then :; else echo WRONG; fi` prints nothing; dash).
    pub(crate) fn should_unwind(&self) -> bool {
        self.env.exec.flow_control.is_some() || self.exit_requested.is_some()
    }

    /// True when a fatal expansion error (`${x:?}`, a nounset failure)
    /// has been diagnosed and is still pending as
    /// `FlowControl::ExpansionError`.
    pub(crate) fn expansion_error_pending(&self) -> bool {
        self.env.exec.flow_control == Some(crate::env::FlowControl::ExpansionError)
    }

    /// The status a forked child shell (subshell, pipeline member, async
    /// list, command substitution) exits with after running `status`'s
    /// command: a pending exit request — a fatal special-builtin error, or
    /// `exit` / errexit in an interactive shell's child — wins over the
    /// status the interrupted command list reported, which `!` may have
    /// inverted (`( ! unset -Z x )` exits 2, not 0). Likewise a fatal
    /// expansion error still pending as `FlowControl::ExpansionError`
    /// (`${x:?}`, a readonly `${x:=v}`) ends the child with status 1 even
    /// if the aborted list reported 0 (`$( ! echo ${x:=v} )` with `x`
    /// readonly is not a success).
    pub(crate) fn child_exit_status(&self, status: i32) -> i32 {
        if let Some(code) = self.exit_requested {
            return code;
        }
        if self.expansion_error_pending() {
            return 1;
        }
        status
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
    fn preview_command_pipeline_renders_all_members() {
        assert_eq!(
            preview_command(&first_and_or("sleep 5 | cat")),
            "sleep 5 | cat"
        );
    }

    #[test]
    fn preview_command_pipeline_with_unpreviewable_member_uses_first() {
        // A later member with dynamic words cannot be rendered, but the
        // preview falls back to the first member (pre-2026-09-02
        // behavior) so `%string` job specs keep resolving.
        assert_eq!(
            preview_command(&first_and_or("echo hi | grep $x")),
            "echo hi"
        );
        // An unpreviewable FIRST member still falls back to the marker.
        assert_eq!(
            preview_command(&first_and_or("grep $x | cat")),
            "(background)"
        );
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
