pub mod line_editor;
pub mod parse_status;
pub mod prompt;

use std::io::{self, Write};

use crate::exec::Executor;
use crate::signal;

use line_editor::LineEditor;
use parse_status::{ParseStatus, classify_parse};
use prompt::expand_prompt;

pub struct Repl {
    executor: Executor,
    line_editor: LineEditor,
}

impl Repl {
    pub fn new(shell_name: String) -> Self {
        signal::init_signal_handling();
        let mut executor = Executor::new(shell_name, vec![]);
        executor.env.is_interactive = true;
        Self {
            executor,
            line_editor: LineEditor::new(),
        }
    }

    /// Run the interactive REPL loop. Returns the exit status.
    pub fn run(&mut self) -> i32 {
        let mut input_buffer = String::new();

        loop {
            // Choose PS1 or PS2
            let prompt_var = if input_buffer.is_empty() { "PS1" } else { "PS2" };
            let prompt = expand_prompt(&mut self.executor.env, prompt_var);
            let prompt_width = prompt.chars().count();

            // Display prompt on stderr
            eprint!("{}", prompt);
            io::stderr().flush().ok();

            // Read a line
            let line = match self.line_editor.read_line(prompt_width) {
                Ok(Some(line)) => line,
                Ok(None) => {
                    // EOF (Ctrl+D)
                    if self.executor.env.options.ignoreeof {
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
                    for cmd in &commands {
                        let status = self.executor.exec_complete_command(cmd);
                        self.executor.env.last_exit_status = status;
                        // Note: errexit (set -e) is intentionally not checked here.
                        // Most POSIX shells do not exit on errexit in interactive mode.
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
        self.executor.env.last_exit_status
    }
}
