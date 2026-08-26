pub mod command_checker;
pub mod command_completion;
pub mod completion;
pub mod display_width;
pub mod edit_action;
pub mod fuzzy_search;
pub mod highlight;
pub mod highlight_scanner;
pub mod history;
pub mod keymap;
pub mod kill_ring;
pub mod line_editor;
pub mod parse_status;
pub mod prompt;
pub mod selector;
pub mod spec_completion;
pub mod terminal;
pub mod undo;
pub mod vi;

use std::io::{self, Write};

use crate::exec::Executor;
use crate::signal;

use command_completion::{CommandCompleter, CommandCompletionContext};
use completion::CompletionContext;
use highlight::{CheckerEnv, HighlightScanner};
use line_editor::LineEditor;
use parse_status::{ParseStatus, classify_parse};
use prompt::{PromptInfo, expand_prompt};
use spec_completion::SpecStore;
use terminal::CrosstermTerminal;

pub struct Repl {
    executor: Executor,
    line_editor: LineEditor,
    terminal: CrosstermTerminal,
    scanner: HighlightScanner,
    command_completer: CommandCompleter,
    spec_store: SpecStore,
}

impl Repl {
    pub fn new(
        shell_name: String,
        positional: Vec<String>,
        explicit_s: bool,
        invocation_ops: &[crate::env::InvocationOp],
    ) -> Self {
        signal::init_signal_handling();
        signal::set_interactive_shell(true);
        let mut executor = Executor::new(shell_name, positional);
        crate::env::default_path::ensure_default_path(&mut executor.env);
        executor.env.mode.is_interactive = true;
        executor.env.mode.options.monitor = true;
        // Interactive default editing mode is emacs (bash parity); an
        // invocation `-o vi` below or a later `set -o vi` switches it.
        executor.env.mode.options.emacs = true;
        // `$-` reports `s` only for an explicit `yosh -s` (bash agrees:
        // an interactive bash without -s has no `s` in $-; dash differs
        // and always reports it when reading stdin).
        executor.env.mode.options.stdin_reads = explicit_s;
        // Invocation-time set options, applied after the interactive
        // monitor default so an explicit `+m` can turn it off (POSIX:
        // -m is on by default for interactive shells). Validated at
        // parse time in main.
        executor
            .env
            .mode
            .options
            .apply_invocation_ops(invocation_ops);
        if executor.env.mode.options.monitor {
            // A REPL launched in the background of a job-controlling
            // parent (`yosh &`) stops inside wait_until_foreground
            // (SIGTTIN) until the user foregrounds it, instead of
            // letting the take_terminal below steal the terminal from
            // the parent. With no controlling terminal at all, job
            // control is disabled, matching run_string's invocation
            // `-m` ownership gate.
            if signal::wait_until_foreground() {
                signal::init_job_control_signals();
                // Ensure shell has terminal
                crate::env::jobs::take_terminal(executor.env.process.shell_pgid).ok();
            } else {
                executor.env.mode.options.monitor = false;
            }
        }
        // With invocation `+m` the monitor block above is skipped: leaving
        // SIGTSTP/SIGTTIN/SIGTTOU at SIG_DFL matches the state the
        // runtime `set +m` path produces via reset_job_control_signals,
        // so Ctrl-Z suspends the whole shell normally and children do
        // not inherit SIG_IGN for the job-control signals.

        // Snapshot the terminal's termios so we can restore it after every
        // foreground job completes. capture_tty_termios returns Ok(None)
        // silently if stdin is not a TTY. Captured even under invocation
        // `+m` so a later `set -m` still has a snapshot to restore from;
        // the symmetric `is_interactive && monitor` guard lives in
        // `restore_shell_termios_if_interactive`, which runs after each
        // foreground wait and re-checks the live monitor flag.
        if executor.env.mode.is_interactive
            && let Ok(Some(t)) = crate::exec::terminal_state::capture_tty_termios()
        {
            executor.env.process.jobs.set_shell_tmodes(t);
        }

        // Set history variable defaults
        let home = executor.env.vars.get("HOME").unwrap_or("").to_string();
        let histfile = format!("{}/.yosh_history", home);
        let _ = executor.env.vars.set("HISTFILE", &histfile);
        let _ = executor.env.vars.set("HISTSIZE", "500");
        let _ = executor.env.vars.set("HISTFILESIZE", "500");
        let _ = executor.env.vars.set("HISTCONTROL", "ignoreboth");

        // POSIX XCU §2.5.3: PS1 has a default value for interactive shells.
        // Set it as a real variable so observers like `[ -n "${PS1+x}" ]`
        // see it. Defer to inherited / rc-set value if already present.
        if executor.env.vars.get("PS1").is_none() {
            // SAFETY: getuid() is always safe to call.
            let default = if unsafe { libc::getuid() } == 0 {
                "# "
            } else {
                "$ "
            };
            let _ = executor.env.vars.set("PS1", default);
        }

        // Load history from file
        executor.env.history.load(std::path::Path::new(&histfile));

        // Load plugins
        executor.load_plugins();

        // Source ~/.yoshrc (yosh-specific startup file)
        if !home.is_empty() {
            let rc_path = std::path::PathBuf::from(&home).join(".yoshrc");
            executor.source_file(&rc_path); // Silent skip if absent
        }

        // Source $ENV (POSIX: parameter-expanded path for interactive shells)
        if let Some(env_val) = executor.env.vars.get("ENV").map(|s| s.to_string())
            && !env_val.is_empty()
        {
            // POSIX 2.6.1: tilde expansion occurs before parameter expansion
            let home = executor.env.vars.get("HOME").map(|s| s.to_string());
            let after_tilde = crate::expand::expand_tilde_prefix(home.as_deref(), &env_val);

            // Parse as double-quoted word for parameter expansion
            let input = format!("\"{}\"", after_tilde);
            let expanded = match crate::lexer::Lexer::new(&input).next_token() {
                Ok(tok) => {
                    if let crate::lexer::token::Token::Word(word) = tok.token {
                        crate::expand::expand_word_to_string(&mut executor.env, &word)
                            .ok()
                            .or_else(|| Some(after_tilde.clone()))
                    } else {
                        Some(after_tilde.clone())
                    }
                }
                Err(_) => Some(after_tilde.clone()),
            };
            if let Some(path) = expanded
                && executor.source_file(std::path::Path::new(&path)).is_none()
            {
                eprintln!("yosh: {}: No such file or directory", path);
            }
        }

        let spec_store = SpecStore::from_home(&home);

        Self {
            executor,
            line_editor: LineEditor::new(),
            terminal: CrosstermTerminal::new(),
            scanner: HighlightScanner::new(),
            command_completer: CommandCompleter::new(),
            spec_store,
        }
    }

    /// Run the interactive REPL loop. Returns the exit status.
    pub fn run(&mut self) -> i32 {
        let mut input_buffer = String::new();

        loop {
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

            // Build completion context
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
            let comp_ctx = CompletionContext {
                cwd,
                home,
                show_dotfiles,
            };

            // Build checker env for syntax highlighting
            let path_val = self.executor.env.vars.get("PATH").unwrap_or("").to_string();

            self.spec_store
                .set_exec_env(self.executor.env.vars.environ().to_vec());

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
            // post-submit classification below would see (byteenc-encoded,
            // newline-terminated, appended to any accumulated PS2 input).
            let is_incomplete = |buf_text: &str| {
                let candidate = format!(
                    "{}{}\n",
                    input_buffer,
                    crate::byteenc::encode_bytes(buf_text.as_bytes())
                );
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
                &input_buffer,
                &mut cont_prompt,
                &is_incomplete,
            );

            // Restore the taken fields before anything else touches env.
            self.executor.env.history = history;
            self.executor.env.aliases = aliases;

            let line = match read_result {
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

            // Ctrl+C returns empty string — reset buffer and re-prompt
            if line.is_empty() && !input_buffer.is_empty() {
                input_buffer.clear();
                continue;
            }

            // Skip empty lines at PS1
            if line.is_empty() && input_buffer.is_empty() {
                continue;
            }

            // Accumulate input, normalizing through the byteenc encoding so
            // a literal escape-range codepoint typed/pasted at the prompt is
            // re-escaped (keeps encode/decode injective; no-op otherwise).
            input_buffer.push_str(&crate::byteenc::encode_bytes(line.as_bytes()));
            input_buffer.push('\n');

            // Verbose mode: print the input
            self.executor.verbose_print(&line);

            // Try to parse
            match classify_parse(&input_buffer, &self.executor.env.aliases) {
                ParseStatus::Complete(commands) => {
                    let (histsize, histcontrol) = history_settings(&self.executor.env);
                    let cmd_text = input_buffer.trim_end().to_string();

                    for cmd in &commands {
                        let status = self.executor.exec_complete_command(cmd);
                        self.executor.env.exec.last_exit_status = status;
                        // The REPL is the top level: any flow-control signal
                        // still pending here (ExpansionError from a nounset /
                        // ${x:?} failure, or a stray return/break/continue)
                        // has aborted the current command and must not leak
                        // into the next one.
                        self.executor.env.exec.flow_control = None;
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
                        self.executor
                            .env
                            .history
                            .add(&cmd_text, histsize, &histcontrol);
                    }

                    input_buffer.clear();
                }
                ParseStatus::Incomplete => {
                    // Continue reading (PS2 will be shown next iteration)
                    continue;
                }
                ParseStatus::Empty => {
                    // Comment-only lines still enter history (bash does
                    // the same; POSIX requires it for the vi `#` command,
                    // whose whole point is stashing the line in history).
                    let cmd_text = input_buffer.trim_end().to_string();
                    if cmd_text.trim_start().starts_with('#') {
                        let (histsize, histcontrol) = history_settings(&self.executor.env);
                        self.executor
                            .env
                            .history
                            .add(&cmd_text, histsize, &histcontrol);
                    }
                    input_buffer.clear();
                }
                ParseStatus::Error(msg) => {
                    eprintln!("yosh: {}", msg);
                    input_buffer.clear();
                }
            }

            // Process any pending signals
            self.executor.process_pending_signals();
            if let Some(code) = self.executor.exit_requested {
                self.executor.env.exec.last_exit_status = code;
                break;
            }
        }

        self.executor.process_pending_signals();
        if self.executor.exit_requested.is_none() {
            self.executor.execute_exit_trap();
        }

        // Save history to file
        let histfile = self
            .executor
            .env
            .vars
            .get("HISTFILE")
            .unwrap_or("")
            .to_string();
        let histfilesize: usize = self
            .executor
            .env
            .vars
            .get("HISTFILESIZE")
            .and_then(|s| s.parse().ok())
            .unwrap_or(500);
        if !histfile.is_empty()
            && let Err(e) = self
                .executor
                .env
                .history
                .save(std::path::Path::new(&histfile), histfilesize)
        {
            eprintln!("yosh: warning: cannot save history to {}: {}", histfile, e);
        }

        self.executor.env.exec.last_exit_status
    }
}

/// The HISTSIZE / HISTCONTROL pair `History::add` wants, with the REPL
/// defaults applied when the variables are unset or unparsable.
fn history_settings(env: &crate::env::ShellEnv) -> (usize, String) {
    let histsize: usize = env
        .vars
        .get("HISTSIZE")
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);
    let histcontrol = env
        .vars
        .get("HISTCONTROL")
        .unwrap_or("ignoreboth")
        .to_string();
    (histsize, histcontrol)
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
