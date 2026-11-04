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
mod repl_loop;
pub mod selector;
mod shutdown;
pub mod spec_completion;
mod startup;
pub mod terminal;
pub mod undo;
pub mod vi;
pub mod vim;

use crate::exec::Executor;

use command_completion::CommandCompleter;
use highlight::HighlightScanner;
use line_editor::LineEditor;
use spec_completion::SpecStore;
use terminal::CrosstermTerminal;

pub use startup::{set_default_prompts, source_login_files, source_startup_files};

/// The interactive shell: the executor plus the line editor and its
/// completion / highlighting state. Created by [`Repl::new`] (startup),
/// driven by [`Repl::run`] (the read-eval loop and shutdown).
pub struct Repl {
    executor: Executor,
    line_editor: LineEditor,
    terminal: CrosstermTerminal,
    scanner: HighlightScanner,
    command_completer: CommandCompleter,
    spec_store: SpecStore,
}
