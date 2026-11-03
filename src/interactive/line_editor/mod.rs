//! Interactive line editor: the editing buffer, its emacs/vi/vim key
//! handling, and the terminal renderer.
//!
//! The [`LineEditor`] state lives here; behavior is split by
//! responsibility across the submodules, each adding an
//! `impl LineEditor` block:
//!
//! - `editing`: buffer/cursor editing primitives and the width cache
//! - `suggestion`: history autosuggestion accept/update
//! - `kill_undo`: kill-ring/unnamed-register writes and undo units
//! - `emacs`: key dispatch and the keymap (emacs-flavored) actions
//! - `vi_insert`: vi insert-mode keys, leaving insert mode, `.` replay
//! - `vi_command`: vi command-mode dispatch and the `.`/undo wrapper
//! - `vi_edit`: the POSIX vi command-mode buffer edits
//! - `vi_history`, `vi_expand`: vi history search/recall, `=` `\` `*`
//! - `vim_ops`, `vim_visual`: vim-flavor linewise ops/puts, VISUAL mode
//! - `read_loop`: the read sessions and terminal event loop
//! - `screen`: screen-level helpers around a read
//! - `render`: the single-line and multiline renderers
//! - `tab_complete`: Tab completion invocation and insertion
//! - `external_editor`: vi `v` / vim `Ctrl-X Ctrl-E`

mod editing;
mod emacs;
mod external_editor;
mod kill_undo;
mod read_loop;
mod render;
mod screen;
mod suggestion;
mod tab_complete;
mod vi_command;
mod vi_edit;
mod vi_expand;
mod vi_history;
mod vi_insert;
mod vim_ops;
mod vim_visual;

use crossterm::event::KeyEvent;
use std::collections::VecDeque;

use super::edit_action::EditAction;
use super::highlight::ColorSpan;
use super::keymap::Keymap;
use super::kill_ring::KillRing;
use super::terminal::CursorStyle;
use super::undo::UndoManager;
use super::vi::{self, EditMode, SearchDir, ViEngine};
use super::vim::UnnamedRegister;

/// A minimal line-editing buffer used by the interactive REPL.
///
/// The buffer stores characters as a `Vec<char>` so that cursor
/// movement and insertion work correctly with multi-byte UTF-8
/// characters.
pub struct LineEditor {
    buf: Vec<char>,
    pos: usize,
    suggestion: Option<String>,
    tab_count: u8,
    keymap: Keymap,
    kill_ring: KillRing,
    undo: UndoManager,
    yank_state: Option<YankState>,
    last_action: EditAction,
    last_was_insert: bool,
    prev_total_rows: usize,
    /// Physical row (0-based, relative to the first content row) the cursor
    /// was left on by the previous `redraw`. Both repaint strategies move
    /// relative to this row, so it must be reset together with
    /// `prev_total_rows` whenever the screen is repainted outside `redraw`
    /// (see `invalidate_render_state`).
    prev_cursor_row: usize,
    /// Cached total display width of `buf`, invalidated (`None`) on every
    /// buffer mutation and lazily recomputed the next time it's needed.
    /// Avoids re-summing `UnicodeWidthChar::width` over the whole buffer on
    /// every keystroke, including pure cursor-movement keys that don't
    /// touch `buf` at all.
    cached_total_width: Option<usize>,
    /// Bumped every time `buf`'s *content* changes (not on pure cursor
    /// movement). Used to detect whether a previously computed suggestion
    /// is still valid for the current buffer content, so cursor-only
    /// actions (which can still hide/reveal the suggestion depending on
    /// whether the cursor is at the end of the buffer) don't need to
    /// re-run `History::suggest` — only re-derive it from the cache.
    buf_generation: u64,
    /// The `(buf_generation, result)` pair last passed to `History::suggest`,
    /// so `update_suggestion` can reuse the result when `buf_generation`
    /// hasn't changed instead of rebuilding `self.buffer()` and re-querying
    /// history on every keystroke.
    suggestion_cache: Option<(u64, Option<String>)>,
    /// `(buf, spans)` as last painted to the screen by `redraw`, used to
    /// find the first column that actually needs repainting. `None` before
    /// the first `redraw` call (or after anything that invalidates the
    /// on-screen state wholesale, e.g. `clear`).
    prev_render: Option<(Vec<char>, Vec<ColorSpan>)>,
    /// Continuation prompt (expanded PS2 last line) resolved lazily on the
    /// first multiline render of the current `read_line` session, so a PS2
    /// containing command substitution is only executed when a continuation
    /// line is actually displayed. Reset by `clear`.
    cont_prompt_cache: Option<String>,
    /// Readline-style "preferred column" for consecutive vertical moves:
    /// captured on the first Up/Down, retained while the cursor passes
    /// through shorter lines, and cleared by any other edit action or
    /// buffer mutation.
    preferred_col: Option<usize>,
    /// Whether the previous `redraw` painted a VISUAL selection. The
    /// single-line diff-based partial repaint is bypassed while a
    /// selection is active and on the first frame after it is
    /// dismissed (the reverse-video cells must be cleared).
    prev_render_had_selection: bool,
    /// Whether the previous `redraw` went through the multiline renderer.
    /// The single-line partial-repaint diff must not run against a
    /// multiline on-screen layout, and buffer content alone can't tell
    /// (an oversized single-logical-line buffer also renders multiline).
    prev_render_multiline: bool,
    /// First visible physical row (into the full row layout) of the
    /// multiline renderer's viewport. 0 while everything fits on screen;
    /// scrolls minimally to keep the cursor row visible when the render
    /// is taller than the terminal.
    viewport_top: usize,
    /// Active editing flavor (`set -o emacs` / `set -o vi`), synced by
    /// the REPL before each read.
    edit_mode: EditMode,
    /// vi command-mode key state machine (unused while `edit_mode` is
    /// `Emacs`).
    vi: ViEngine,
    /// Terminal alert requested by an invalid vi command; emitted (as
    /// BEL) and cleared by the read loop before the next redraw.
    pending_bell: bool,
    /// Cursor shape last emitted to the terminal, so the read loop only
    /// writes DECSCUSR on actual mode transitions. `None` = never set
    /// (or already reset to the terminal default).
    last_cursor_style: Option<CursorStyle>,
    /// vi `U` baseline: the buffer content the current line started
    /// from (empty at read start; updated on history recall).
    vi_line_base: Vec<char>,
    /// Cursor position when the current vi insert session began via a
    /// command-mode command; used to capture the inserted text for `.`.
    vi_insert_start: Option<usize>,
    /// Text entered during the most recent vi insert session, replayed
    /// by `.` for insert-entering commands.
    vi_last_insert: String,
    /// Active `/` / `?` pattern input: keystrokes go to the pattern and
    /// the render shows `/pattern` in place of the buffer.
    vi_search_input: Option<(SearchDir, String)>,
    /// Most recent executed search, for `n` / `N` and the empty-pattern
    /// reuse rule.
    vi_last_search: Option<(SearchDir, String)>,
    /// Synthetic key events queued by `@letter` alias macros, consumed
    /// before reading from the terminal.
    pending_events: VecDeque<KeyEvent>,
    /// Remaining `@letter` expansions allowed before the next real
    /// terminal read. A recursive macro (`alias _a='@a'`) would
    /// otherwise refill the queue forever without ever reaching the
    /// terminal (and its Ctrl+C).
    vi_macro_budget: u32,
    /// vi insert-mode Ctrl+V: the next key is inserted literally.
    vi_literal_next: bool,
    /// Editor command for vim-mode `Ctrl-X Ctrl-E`, snapshotted from
    /// the ShellEnv variable store (`VISUAL`, else `EDITOR`, else
    /// `vi`) by the REPL before each read.
    editor_command: String,
    /// Vim typed unnamed register (`""`). Mirrors the kill ring's front
    /// entry after every kill-ring write; read by vim-flavor `p`/`P`.
    /// Persists across reads like the kill ring.
    unnamed_register: UnnamedRegister,
    /// vim §7.2 Insert staging slot: the pre-state of the pending
    /// Insert session (set when a command enters Insert, or at read
    /// start). Committed as one undo unit when Insert ends iff the
    /// buffer changed — a no-op insert session commits nothing and
    /// preserves redo.
    vim_undo_staged: Option<(Vec<char>, usize)>,
}

#[derive(Debug, Clone)]
struct YankState {
    start: usize,
    len: usize,
}

impl Default for LineEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl LineEditor {
    /// Create an empty line editor.
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            pos: 0,
            suggestion: None,
            tab_count: 0,
            keymap: Keymap::new(),
            kill_ring: KillRing::new(60),
            undo: UndoManager::new(256),
            yank_state: None,
            last_action: EditAction::Noop,
            last_was_insert: false,
            prev_total_rows: 0,
            prev_cursor_row: 0,
            cached_total_width: None,
            buf_generation: 0,
            suggestion_cache: None,
            prev_render: None,
            cont_prompt_cache: None,
            preferred_col: None,
            prev_render_had_selection: false,
            prev_render_multiline: false,
            viewport_top: 0,
            edit_mode: EditMode::Emacs,
            vi: ViEngine::new(),
            pending_bell: false,
            last_cursor_style: None,
            vi_line_base: Vec::new(),
            vi_insert_start: None,
            vi_last_insert: String::new(),
            vi_search_input: None,
            vi_last_search: None,
            pending_events: VecDeque::new(),
            vi_macro_budget: 0,
            vi_literal_next: false,
            editor_command: "vi".to_string(),
            unnamed_register: UnnamedRegister::default(),
            vim_undo_staged: None,
        }
    }

    /// Select the editing flavor for subsequent reads (`set -o emacs` /
    /// `set -o vi` / `set -o vim`). Takes effect at the start of the
    /// next read.
    pub fn set_edit_mode(&mut self, mode: EditMode) {
        self.edit_mode = mode;
        self.vi.flavor = if mode == EditMode::Vim {
            vi::ViFlavor::Vim
        } else {
            vi::ViFlavor::Posix
        };
    }

    /// Snapshot the editor command (`$VISUAL` / `$EDITOR` resolution
    /// from the shell variable store) for vim-mode `Ctrl-X Ctrl-E`,
    /// taken by the REPL before each read.
    pub fn set_editor_command(&mut self, cmd: String) {
        self.editor_command = cmd;
    }

    /// Return the current buffer contents as a `String`.
    pub fn buffer(&self) -> String {
        self.buf.iter().collect()
    }

    /// Return the current cursor position (0-based character index).
    #[allow(dead_code)] // public API for interactive mode enhancements
    pub fn cursor(&self) -> usize {
        self.pos
    }

    /// Return `true` if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Clear the buffer and reset the cursor to 0.
    pub fn clear(&mut self) {
        self.buf.clear();
        self.pos = 0;
        self.suggestion = None;
        self.tab_count = 0;
        self.yank_state = None;
        self.last_action = EditAction::Noop;
        self.last_was_insert = false;
        self.undo.clear();
        self.cont_prompt_cache = None;
        self.vi.reset_for_read();
        self.pending_bell = false;
        self.vi_line_base.clear();
        self.vi_insert_start = None;
        self.vi_search_input = None;
        self.pending_events.clear();
        self.vi_literal_next = false;
        self.vim_undo_staged = None;
        if self.edit_mode == EditMode::Vim {
            // The initial Insert session (every read starts in Insert
            // with no entry transition) is staged with the empty-buffer
            // snapshot; committed on the first Esc iff text was typed.
            self.vim_undo_staged = Some((Vec::new(), 0));
        } else if self.edit_mode.is_vi_family() {
            // Base undo entry so `u` after the first insert session can
            // restore the empty line (vi undo is session-granular; the
            // per-run emacs saves are suppressed in vi insert mode).
            self.undo.save(&[], 0);
        }
        self.invalidate_width_cache();
        self.invalidate_render_state();
    }
}

impl std::fmt::Display for LineEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.buffer())
    }
}

/// Result of processing a single key event.
enum KeyAction {
    Continue,
    Submit,
    Eof,
    Interrupt,
    FuzzySearch,
    TabComplete,
    ClearScreen,
    /// vi `=`: list these pathname expansions below the line.
    ViListExpansions(Vec<String>),
    /// vi `@letter`: feed the value of alias `_letter` as editor input.
    ViAliasMacro(char),
    /// vi `v`: edit the line (or history entry `n`; 0 = current line)
    /// in an external editor, then execute the result.
    ViEditInEditor(u32),
    /// vim `Ctrl-X Ctrl-E`: edit the buffer in an external editor; the
    /// result replaces the buffer and editing continues (no submit).
    ViEditBuffer,
}
