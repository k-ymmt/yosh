pub mod fuzzy_search;
pub mod history;
pub mod line_editor;
pub mod parse_status;
pub mod prompt;
pub mod terminal;

use std::io::{self, Write};

use crate::exec::Executor;
use crate::signal;

use line_editor::LineEditor;
use parse_status::{ParseStatus, classify_parse};
use prompt::expand_prompt;
use terminal::CrosstermTerminal;

pub struct Repl {
    executor: Executor,
    line_editor: LineEditor,
    terminal: CrosstermTerminal,
}

impl Repl {
    pub fn new(shell_name: String) -> Self {
        signal::init_signal_handling();
        let mut executor = Executor::new(shell_name, vec![]);
        executor.env.mode.is_interactive = true;
        executor.env.mode.options.monitor = true;
        signal::init_job_control_signals();
        // Ensure shell has terminal
        crate::env::jobs::take_terminal(executor.env.process.shell_pgid).ok();

        // Set history variable defaults
        let home = executor.env.vars.get("HOME").unwrap_or("").to_string();
        let histfile = format!("{}/.kish_history", home);
        let _ = executor.env.vars.set("HISTFILE", &histfile);
        let _ = executor.env.vars.set("HISTSIZE", "500");
        let _ = executor.env.vars.set("HISTFILESIZE", "500");
        let _ = executor.env.vars.set("HISTCONTROL", "ignoreboth");

        // Load history from file
        executor.env.history.load(std::path::Path::new(&histfile));

        Self {
            executor,
            line_editor: LineEditor::new(),
            terminal: CrosstermTerminal::new(),
        }
    }

    /// Run the interactive REPL loop. Returns the exit status.
    pub fn run(&mut self) -> i32 {
        let mut input_buffer = String::new();

        loop {
            // Reap zombies and display job notifications before prompt
            self.executor.reap_zombies();
            self.executor.display_job_notifications();

            // Choose PS1 or PS2
            let prompt_var = if input_buffer.is_empty() { "PS1" } else { "PS2" };
            let prompt = expand_prompt(&mut self.executor.env, prompt_var);
            let prompt_width = prompt.chars().count();

            // Display prompt on stderr
            eprint!("{}", prompt);
            io::stderr().flush().ok();

            // Read a line
            let line = match self.line_editor.read_line(prompt_width, &mut self.executor.env.history, &mut self.terminal) {
                Ok(Some(line)) => line,
                Ok(None) => {
                    // EOF (Ctrl+D)
                    if self.executor.env.mode.options.ignoreeof {
                        eprintln!("\r\nkish: Use \"exit\" to leave the shell.");
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
                    let histsize: usize = self.executor.env.vars.get("HISTSIZE")
                        .and_then(|s| s.parse().ok()).unwrap_or(500);
                    let histcontrol = self.executor.env.vars.get("HISTCONTROL")
                        .unwrap_or("ignoreboth").to_string();
                    let cmd_text = input_buffer.trim_end().to_string();
                    self.executor.env.history.add(&cmd_text, histsize, &histcontrol);

                    for cmd in &commands {
                        let status = self.executor.exec_complete_command(cmd);
                        self.executor.env.exec.last_exit_status = status;
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
                    eprintln!("kish: {}", msg);
                    input_buffer.clear();
                }
            }

            // Process any pending signals
            self.executor.process_pending_signals();
        }

        self.executor.process_pending_signals();
        self.executor.execute_exit_trap();

        // Save history to file
        let histfile = self.executor.env.vars.get("HISTFILE").unwrap_or("").to_string();
        let histfilesize: usize = self.executor.env.vars.get("HISTFILESIZE")
            .and_then(|s| s.parse().ok()).unwrap_or(500);
        if !histfile.is_empty() {
            self.executor.env.history.save(std::path::Path::new(&histfile), histfilesize);
        }

        self.executor.env.exec.last_exit_status
    }
}
