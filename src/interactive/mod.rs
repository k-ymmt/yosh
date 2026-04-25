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
pub mod terminal;
pub mod undo;

use std::io::{self, Write};

use crate::exec::Executor;
use crate::signal;

use command_completion::{CommandCompleter, CommandCompletionContext};
use completion::CompletionContext;
use highlight::{CheckerEnv, HighlightScanner};
use line_editor::LineEditor;
use parse_status::{ParseStatus, classify_parse};
use prompt::{PromptInfo, expand_prompt};
use terminal::CrosstermTerminal;

pub struct Repl {
    executor: Executor,
    line_editor: LineEditor,
    terminal: CrosstermTerminal,
    scanner: HighlightScanner,
    command_completer: CommandCompleter,
}

impl Repl {
    pub fn new(shell_name: String) -> Self {
        signal::init_signal_handling();
        let mut executor = Executor::new(shell_name, vec![]);
        crate::env::default_path::ensure_default_path(&mut executor.env);
        executor.env.mode.is_interactive = true;
        executor.env.mode.options.monitor = true;
        signal::init_job_control_signals();
        // Ensure shell has terminal
        crate::env::jobs::take_terminal(executor.env.process.shell_pgid).ok();

        // Snapshot the terminal's termios so we can restore it after every
        // foreground job completes. Only meaningful in interactive + monitor
        // mode (both flags were set above). capture_tty_termios returns
        // Ok(None) silently if stdin is not a TTY.
        //
        // The `is_interactive && monitor` check is documentation-only at
        // this site (the flags are unconditionally true two lines above),
        // but mirrors the symmetric guard inside `wait_for_foreground_job`'s
        // `restore_shell_termios_if_interactive`, where the check IS
        // load-bearing. Keep both in sync so a future "simplification"
        // does not drop one and leave the other dangling.
        if executor.env.mode.is_interactive && executor.env.mode.options.monitor {
            if let Ok(Some(t)) = crate::exec::terminal_state::capture_tty_termios() {
                executor.env.process.jobs.set_shell_tmodes(t);
            }
        }

        // Set history variable defaults
        let home = executor.env.vars.get("HOME").unwrap_or("").to_string();
        let histfile = format!("{}/.yosh_history", home);
        let _ = executor.env.vars.set("HISTFILE", &histfile);
        let _ = executor.env.vars.set("HISTSIZE", "500");
        let _ = executor.env.vars.set("HISTFILESIZE", "500");
        let _ = executor.env.vars.set("HISTCONTROL", "ignoreboth");

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
        if let Some(env_val) = executor.env.vars.get("ENV").map(|s| s.to_string()) {
            if !env_val.is_empty() {
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
                if let Some(path) = expanded {
                    if executor.source_file(std::path::Path::new(&path)).is_none() {
                        eprintln!("yosh: {}: No such file or directory", path);
                    }
                }
            }
        }

        Self {
            executor,
            line_editor: LineEditor::new(),
            terminal: CrosstermTerminal::new(),
            scanner: HighlightScanner::new(),
            command_completer: CommandCompleter::new(),
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
            let checker_env = CheckerEnv {
                path: &path_val,
                aliases: &self.executor.env.aliases,
            };

            let mut cmd_ctx = CommandCompletionContext {
                completer: &mut self.command_completer,
                path: &path_val,
                builtins: crate::builtin::BUILTIN_NAMES,
                aliases: &self.executor.env.aliases,
            };

            // Read a line
            let line = match self.line_editor.read_line_with_completion(
                &prompt_info.last_line,
                &prompt_info.upper_lines,
                &mut self.executor.env.history,
                &mut self.terminal,
                &comp_ctx,
                &mut cmd_ctx,
                &mut self.scanner,
                &checker_env,
                &input_buffer,
            ) {
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

            // Accumulate input
            input_buffer.push_str(&line);
            input_buffer.push('\n');

            // Verbose mode: print the input
            self.executor.verbose_print(&line);

            // Try to parse
            match classify_parse(&input_buffer, &self.executor.env.aliases) {
                ParseStatus::Complete(commands) => {
                    // Add to history before executing
                    let histsize: usize = self
                        .executor
                        .env
                        .vars
                        .get("HISTSIZE")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(500);
                    let histcontrol = self
                        .executor
                        .env
                        .vars
                        .get("HISTCONTROL")
                        .unwrap_or("ignoreboth")
                        .to_string();
                    let cmd_text = input_buffer.trim_end().to_string();
                    self.executor
                        .env
                        .history
                        .add(&cmd_text, histsize, &histcontrol);

                    for cmd in &commands {
                        let status = self.executor.exec_complete_command(cmd);
                        self.executor.env.exec.last_exit_status = status;
                        if self.executor.exit_requested.is_some() {
                            break;
                        }
                    }
                    input_buffer.clear();
                }
                ParseStatus::Incomplete => {
                    // Continue reading (PS2 will be shown next iteration)
                    continue;
                }
                ParseStatus::Empty => {
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
        if !histfile.is_empty() {
            self.executor
                .env
                .history
                .save(std::path::Path::new(&histfile), histfilesize);
        }

        self.executor.env.exec.last_exit_status
    }
}
