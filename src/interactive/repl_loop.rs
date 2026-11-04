//! The read-eval loop: prompting, reading a line with the editor,
//! accumulating continuation lines, running complete input and
//! recording it in the history.

use std::io::{self, Write};

use super::Repl;
use super::command_completion::CommandCompletionContext;
use super::completion::CompletionContext;
use super::highlight::CheckerEnv;
use super::history;
use super::parse_status::{ParseStatus, classify_parse};
use super::prompt::{PromptInfo, expand_prompt};
use super::vi;

impl Repl {
    /// Run the interactive REPL loop. Returns the exit status.
    pub fn run(&mut self) -> i32 {
        let mut input_buffer = String::new();

        loop {
            // `exit` in ~/.yoshrc or $ENV terminates the shell before the
            // first prompt (dash parity), not after the first typed command.
            if let Some(code) = self.executor.exit_requested {
                self.executor.env.exec.last_exit_status = code;
                break;
            }

            // Reap zombies and display job notifications before prompt
            self.executor.reap_zombies();
            self.executor.display_job_notifications();

            // Fire pre_prompt hook for PS1 (not PS2 continuation)
            if input_buffer.is_empty() {
                self.executor
                    .plugins
                    .call_pre_prompt(&mut self.executor.env);
            }

            // Choose PS1 or PS2
            let prompt_var = if input_buffer.is_empty() {
                "PS1"
            } else {
                "PS2"
            };
            let prompt = expand_prompt(&mut self.executor.env, prompt_var);
            let prompt_info = PromptInfo::from_prompt(&prompt);

            // Display prompt on stderr
            for line in &prompt_info.upper_lines {
                eprint!("{}\r\n", line);
            }
            eprint!("{}", prompt_info.last_line);
            io::stderr().flush().ok();

            let line = match self.read_input(&prompt_info, &input_buffer) {
                Ok(Some(line)) => line,
                Ok(None) => {
                    // EOF (Ctrl+D)
                    if self.executor.env.mode.options.ignoreeof {
                        eprintln!("\r\nyosh: Use \"exit\" to leave the shell.");
                        input_buffer.clear();
                        continue;
                    }
                    // Exit the shell
                    eprintln!();
                    break;
                }
                Err(_) => {
                    break;
                }
            };

            if self.line_editor.take_interrupted() && self.handle_prompt_interrupt() {
                input_buffer.clear();
                continue;
            }

            // Ctrl+C returns empty string — reset buffer and re-prompt
            if line.is_empty() && !input_buffer.is_empty() {
                input_buffer.clear();
                continue;
            }

            // Skip empty lines at PS1
            if line.is_empty() && input_buffer.is_empty() {
                continue;
            }

            // Accumulate input. The editor buffer is already in the byteenc
            // encoding: typed text is encoded at the terminal ingress
            // (`CrosstermTerminal::read_event`), and history entries, alias
            // macros and completions are stored encoded. Encoding it again
            // here would turn a recalled raw byte (an escape codepoint) into
            // the four bytes of that codepoint's UTF-8 form.
            input_buffer.push_str(&line);
            input_buffer.push('\n');

            // Verbose mode: print the input
            self.executor.verbose_print(&line);

            if !self.run_input(&mut input_buffer) {
                // Continue reading (PS2 will be shown next iteration)
                continue;
            }

            // Process any pending signals
            self.executor.process_pending_signals();
            self.end_top_level_command();
            if let Some(code) = self.executor.exit_requested {
                self.executor.env.exec.last_exit_status = code;
                break;
            }
        }

        self.shut_down()
    }

    /// Read one line with the editor after the prompt `prompt_info` has
    /// been displayed. `input_buffer` holds the continuation lines
    /// accumulated so far (empty at PS1).
    fn read_input(
        &mut self,
        prompt_info: &PromptInfo,
        input_buffer: &str,
    ) -> io::Result<Option<String>> {
        let comp_ctx = self.completion_context();

        // Build checker env for syntax highlighting
        let path_val = self.executor.env.vars.get("PATH").unwrap_or("").to_string();

        self.spec_store
            .set_exec_env(self.executor.env.vars.environ().to_vec());

        self.sync_editor_settings();

        // Take history and aliases out of the environment for the
        // duration of the read: the lazy PS2 closure below borrows the
        // whole ShellEnv mutably while the editor holds these two.
        // Restored right after the read returns.
        let mut history = std::mem::take(&mut self.executor.env.history);
        let aliases = std::mem::take(&mut self.executor.env.aliases);

        let checker_env = CheckerEnv {
            path: &path_val,
            aliases: &aliases,
        };

        let mut cmd_ctx = CommandCompletionContext {
            completer: &mut self.command_completer,
            path: &path_val,
            builtins: crate::builtin::BUILTIN_NAMES,
            aliases: &aliases,
        };

        // Completeness probe for in-editor multiline editing: Enter on
        // input this closure deems incomplete inserts a newline into the
        // editor buffer instead of submitting. Mirrors exactly what the
        // post-submit classification in `run_input` would see (the editor
        // buffer, already byteenc-encoded, newline-terminated, appended to
        // any accumulated PS2 input).
        let is_incomplete = |buf_text: &str| {
            let candidate = format!("{}{}\n", input_buffer, buf_text);
            matches!(
                classify_parse(&candidate, &aliases),
                ParseStatus::Incomplete
            )
        };

        // Lazy continuation prompt for in-editor multiline editing: the
        // editor invokes this at most once per read, on the first
        // multiline render, so a side-effectful PS2 (e.g. command
        // substitution `$(date +%T)> `) no longer executes on every
        // prompt display. Only the last line of a multi-line PS2 is
        // rendered on continuation lines — that is the supported shape
        // (upper lines are a PS1-display-only feature). PS2 command
        // substitution runs with history/aliases temporarily taken out
        // of the environment; neither affects prompt expansion.
        let env = &mut self.executor.env;
        let mut cont_prompt = || {
            let expanded = expand_prompt(env, "PS2");
            PromptInfo::from_prompt(&expanded).last_line
        };

        // Read a line
        let read_result = self.line_editor.read_line_with_completion(
            &prompt_info.last_line,
            &prompt_info.upper_lines,
            &mut history,
            &mut self.terminal,
            &comp_ctx,
            &mut cmd_ctx,
            &mut self.spec_store,
            &mut self.scanner,
            &checker_env,
            input_buffer,
            &mut cont_prompt,
            &is_incomplete,
        );

        // Restore the taken fields before anything else touches env.
        self.executor.env.history = history;
        self.executor.env.aliases = aliases;

        read_result
    }

    /// The filename-completion context for the next read: the current
    /// directory, `$HOME` and the dotfile preference.
    fn completion_context(&self) -> CompletionContext {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| self.executor.env.vars.get("PWD").unwrap_or(".").to_string());
        let home = self.executor.env.vars.get("HOME").unwrap_or("").to_string();
        let show_dotfiles = self
            .executor
            .env
            .vars
            .get("YOSH_SHOW_DOTFILES")
            .map(|v| v == "1")
            .unwrap_or(false);
        CompletionContext {
            cwd,
            home,
            show_dotfiles,
        }
    }

    /// Bring the editor's mode and external-editor command up to date
    /// with the shell's options and variables before a read.
    fn sync_editor_settings(&mut self) {
        // Sync the editing flavor each prompt so `set -o vi` /
        // `set -o vim` / `set -o emacs` take effect at the next
        // read. None set (e.g. `set +o vi`) falls back to emacs
        // behavior.
        let options = &self.executor.env.mode.options;
        self.line_editor.set_edit_mode(if options.vim {
            vi::EditMode::Vim
        } else if options.vi {
            vi::EditMode::Vi
        } else {
            vi::EditMode::Emacs
        });

        // Snapshot the vim-mode Ctrl-X Ctrl-E editor command from
        // the ShellEnv variable store only (it imports the process
        // environment at startup, so inherited values are visible
        // while `unset VISUAL` is respected).
        let editor_cmd = self
            .executor
            .env
            .vars
            .get("VISUAL")
            .filter(|v| !v.is_empty())
            .or_else(|| self.executor.env.vars.get("EDITOR"))
            .filter(|v| !v.is_empty())
            .unwrap_or("vi")
            .to_string();
        self.line_editor.set_editor_command(editor_cmd);
    }

    /// Ctrl+C at the prompt is the terminal's interrupt character,
    /// read as a key because the editor runs with ISIG off: run an
    /// INT trap action as the SIGINT it stands for would (bash and
    /// dash do). Default / ignored INT just discards the line.
    /// Only the default disposition leaves `$?` at 128+SIGINT, as
    /// in dash, zsh and bash 5: the shell was "killed" out of the
    /// line. An ignored INT (`trap '' INT`) did not interrupt
    /// anything, and a trapped one runs its action with `$?`
    /// unchanged — dash and zsh keep the previous status in both
    /// cases (`false`, Ctrl+C, `echo $?` prints 1).
    ///
    /// Returns true when the INT trap action ran: the line is abandoned
    /// and the caller re-prompts.
    fn handle_prompt_interrupt(&mut self) -> bool {
        use crate::env::traps::TrapAction;
        match self.executor.env.traps.get_signal_trap(libc::SIGINT) {
            Some(TrapAction::Command(_)) => {
                self.executor.run_signal_traps(&[libc::SIGINT]);
                self.end_top_level_command();
                return true;
            }
            Some(TrapAction::Ignore) => {}
            Some(TrapAction::Default) | None => {
                self.executor.env.exec.last_exit_status = 128 + libc::SIGINT;
            }
        }
        false
    }

    /// Classify the accumulated input and act on it: run complete
    /// commands, report a syntax error, and record the input in the
    /// history; then clear `input_buffer`. Returns false (leaving the
    /// buffer as is) when the input is incomplete and needs another line.
    fn run_input(&mut self, input_buffer: &mut String) -> bool {
        match classify_parse(input_buffer, &self.executor.env.aliases) {
            ParseStatus::Complete(commands) => {
                let settings = history::history_settings(&self.executor.env.vars);
                let cmd_text = input_buffer.trim_end().to_string();

                for cmd in &commands {
                    let status = self.executor.exec_complete_command(cmd);
                    self.executor.env.exec.last_exit_status = status;
                    self.end_top_level_command();
                    if self.executor.exit_requested.is_some() {
                        break;
                    }
                }

                // Record AFTER execution so `fc` resolving "previous
                // command" sees the user's prior input, not the fc
                // command itself. `exit` is still captured: the
                // break above falls through to this add call.
                //
                // POSIX rationale: "the fc command shall not be
                // entered into the history list" — skip the add when
                // the input is an fc invocation (see
                // should_skip_history for the light-parse contract).
                if !should_skip_history(&cmd_text) {
                    self.add_history(&cmd_text, &settings);
                }
            }
            ParseStatus::Incomplete => return false,
            ParseStatus::Empty => {
                // Comment-only lines still enter history (bash does
                // the same; POSIX requires it for the vi `#` command,
                // whose whole point is stashing the line in history).
                let cmd_text = input_buffer.trim_end().to_string();
                if cmd_text.trim_start().starts_with('#') {
                    let settings = history::history_settings(&self.executor.env.vars);
                    self.add_history(&cmd_text, &settings);
                }
            }
            ParseStatus::Error(msg) => {
                eprintln!("yosh: {}", msg);
                // The rejected line still enters history (bash, ksh
                // and zsh), so Up recalls it for fixing instead of
                // making the user retype it.
                let cmd_text = input_buffer.trim_end().to_string();
                if !should_skip_history(&cmd_text) {
                    let settings = history::history_settings(&self.executor.env.vars);
                    self.add_history(&cmd_text, &settings);
                }
            }
        }
        input_buffer.clear();
        true
    }

    /// Add `cmd_text` to the history under `(HISTSIZE, HISTCONTROL)`
    /// `settings` (see [`history::history_settings`]).
    fn add_history(&mut self, cmd_text: &str, settings: &(usize, String)) {
        let (histsize, histcontrol) = settings;
        self.executor
            .env
            .history
            .add(cmd_text, *histsize, histcontrol);
    }
}

/// POSIX-strict `fc` history exclusion: the fc rationale says the fc
/// command "shall not be entered into the history list", so `Repl::run`
/// skips `history.add` for fc invocations.
///
/// Light-parse contract: only the first whitespace-delimited word of the
/// typed text is inspected. A leading `fc` (bare `fc`, `fc -l`, ...,
/// including whitespace-led input) is skipped; `fc` behind pipes,
/// semicolons, `&&`, subshells, or as an argument is deliberately NOT
/// detected and is still recorded. Trade-off: up-arrow can no longer
/// recall the fc invocation itself.
fn should_skip_history(cmd_text: &str) -> bool {
    cmd_text.split_whitespace().next() == Some("fc")
}

#[cfg(test)]
mod tests {
    use super::should_skip_history;

    #[test]
    fn skips_bare_fc() {
        assert!(should_skip_history("fc"));
    }

    #[test]
    fn skips_fc_with_args() {
        assert!(should_skip_history("fc -l"));
        assert!(should_skip_history("fc -s one=two echo"));
    }

    #[test]
    fn skips_whitespace_led_fc() {
        assert!(should_skip_history("  fc -l"));
        assert!(should_skip_history("\tfc"));
    }

    #[test]
    fn keeps_non_fc_commands() {
        assert!(!should_skip_history("echo fc"));
        assert!(!should_skip_history("fcc -l"));
        assert!(!should_skip_history(""));
        assert!(!should_skip_history("   "));
    }

    #[test]
    fn light_parse_does_not_look_past_first_word() {
        // Deliberate limitation: fc behind separators is still recorded.
        assert!(!should_skip_history("true; fc -l"));
        assert!(!should_skip_history("echo x | fc -l"));
    }
}
