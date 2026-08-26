// Integration tests for the vim editing mode (`set -o vim`), driven
// through LineEditor::read_line with a mock terminal.
//
// Phase 1: the vim flavor exists but changes nothing — this file's
// baseline sweep asserts vim mode behaves exactly as vi mode for the
// shared machinery (insert/command dispatch, motions, operators, undo
// sessions, `.` repeat, history, multiline continuation).

use crossterm::event::KeyCode;
use yosh::interactive::history::History;
use yosh::interactive::line_editor::LineEditor;
use yosh::interactive::vi::EditMode;

mod helpers;
use helpers::mock_terminal::{MockTerminal, alt, chars, ctrl, key};

/// Run a vim-mode read over the given events and return the submitted
/// line (None = EOF).
fn vim_read(events: Vec<crossterm::event::Event>) -> Option<String> {
    let mut ed = LineEditor::new();
    ed.set_edit_mode(EditMode::Vim);
    let mut history = History::new();
    let mut term = MockTerminal::new(events);
    ed.read_line("$ ", &[], &mut history, &mut term)
        .expect("read_line failed")
}

/// Like `vim_read`, with pre-populated history entries (oldest first).
fn vim_read_with_history(entries: &[&str], events: Vec<crossterm::event::Event>) -> Option<String> {
    let mut ed = LineEditor::new();
    ed.set_edit_mode(EditMode::Vim);
    let mut history = History::new();
    for e in entries {
        history.add(e, 500, "");
    }
    let mut term = MockTerminal::new(events);
    ed.read_line("$ ", &[], &mut history, &mut term)
        .expect("read_line failed")
}

/// Full-featured read (completion loop) with aliases and an
/// is_incomplete probe, for features unavailable in the plain path.
fn vim_read_full(
    events: Vec<crossterm::event::Event>,
    aliases: &yosh::env::aliases::AliasStore,
    is_incomplete: &dyn Fn(&str) -> bool,
) -> Option<String> {
    use yosh::interactive::command_completion::{CommandCompleter, CommandCompletionContext};
    use yosh::interactive::completion::CompletionContext;
    use yosh::interactive::highlight::{CheckerEnv, HighlightScanner};

    let ctx = CompletionContext {
        cwd: ".".to_string(),
        home: String::new(),
        show_dotfiles: false,
    };
    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    editor.set_edit_mode(EditMode::Vim);
    let mut history = History::new();
    let mut command_completer = CommandCompleter::new();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases,
    };
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv { path: "", aliases };
    let mut spec_store = yosh::interactive::spec_completion::SpecStore::new(
        std::path::PathBuf::from("/nonexistent"),
    );
    editor
        .read_line_with_completion(
            "$ ",
            &[],
            &mut history,
            &mut term,
            &ctx,
            &mut cmd_ctx,
            &mut spec_store,
            &mut scanner,
            &checker_env,
            "",
            &mut || "> ".to_string(),
            is_incomplete,
        )
        .expect("read failed")
}

fn esc() -> crossterm::event::Event {
    key(KeyCode::Esc)
}

fn enter() -> crossterm::event::Event {
    key(KeyCode::Enter)
}

fn alt_enter() -> crossterm::event::Event {
    crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
        KeyCode::Enter,
        crossterm::event::KeyModifiers::ALT,
    ))
}

/// Build an event sequence from segments: plain strings become chars.
macro_rules! seq {
    ($($part:expr),* $(,)?) => {{
        let mut v: Vec<crossterm::event::Event> = Vec::new();
        $( v.extend($part); )*
        v
    }};
}

// ── Phase 1 baseline sweep: vim mode behaves as vi mode ────────────────

#[test]
fn vim_insert_mode_types_and_submits() {
    let line = vim_read(seq![chars("echo hi"), [enter()]]);
    assert_eq!(line.as_deref(), Some("echo hi"));
}

#[test]
fn vim_esc_moves_cursor_back_and_x_deletes() {
    let line = vim_read(seq![chars("abc"), [esc()], chars("hx"), [enter()]]);
    assert_eq!(line.as_deref(), Some("ac"));
}

#[test]
fn vim_counts_and_motions() {
    let line = vim_read(seq![chars("abcde"), [esc()], chars("03lx"), [enter()]]);
    assert_eq!(line.as_deref(), Some("abce"));
}

#[test]
fn vim_dw_deletes_word() {
    let line = vim_read(seq![chars("echo hello"), [esc()], chars("0dw"), [enter()]]);
    assert_eq!(line.as_deref(), Some("hello"));
}

#[test]
fn vim_cw_changes_word_like_ce() {
    let line = vim_read(seq![
        chars("hello world"),
        [esc()],
        chars("0cw"),
        chars("bye"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("bye world"));
}

#[test]
fn vim_count_multiplies_across_operator() {
    let line = vim_read(seq![
        chars("a b c d e f"),
        [esc()],
        chars("02d2w"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("e f"));
}

#[test]
fn vim_x_then_p_pastes_after_cursor() {
    let line = vim_read(seq![chars("abc"), [esc()], chars("0xp"), [enter()]]);
    assert_eq!(line.as_deref(), Some("bac"));
}

#[test]
fn vim_undo_restores_deleted_char() {
    let line = vim_read(seq![chars("abc"), [esc()], chars("0xu"), [enter()]]);
    assert_eq!(line.as_deref(), Some("abc"));
}

#[test]
fn vim_undo_reverts_whole_insert_session() {
    let line = vim_read(seq![
        chars("hello world"),
        [esc()],
        chars("0cw"),
        chars("bye"),
        [esc()],
        chars("u"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("hello world"));
}

#[test]
fn vim_undo_all_restores_line_base() {
    let line = vim_read(seq![
        chars("abc"),
        [esc()],
        chars("x0xU"),
        chars("iok"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("ok"));
}

#[test]
fn vim_dot_repeats_change_with_text() {
    let line = vim_read(seq![
        chars("aa bb"),
        [esc()],
        chars("0cw"),
        chars("zz"),
        [esc()],
        chars("0w."),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("zz zz"));
}

#[test]
fn vim_replace_mode_overwrites() {
    let line = vim_read(seq![
        chars("abc"),
        [esc()],
        chars("0R"),
        chars("xy"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("xyc"));
}

#[test]
fn vim_tilde_toggles_case_and_advances() {
    let line = vim_read(seq![chars("abc"), [esc()], chars("0~~"), [enter()]]);
    assert_eq!(line.as_deref(), Some("ABc"));
}

#[test]
fn vim_ctrl_c_interrupts_in_command_mode() {
    let line = vim_read(seq![chars("abc"), [esc()], [ctrl('c')]]);
    assert_eq!(line.as_deref(), Some(""));
}

#[test]
fn vim_ctrl_d_is_eof_on_empty_line_in_command_mode() {
    let line = vim_read(seq![[esc()], [ctrl('d')]]);
    assert_eq!(line, None);
}

#[test]
fn vim_alt_char_acts_as_esc_prefix() {
    let line = vim_read(seq![chars("abc"), [alt('h')], chars("x"), [enter()]]);
    assert_eq!(line.as_deref(), Some("ac"));
}

#[test]
fn vim_k_recalls_history_with_cursor_at_start() {
    let line = vim_read_with_history(&["echo last"], seq![[esc()], chars("kx"), [enter()]]);
    assert_eq!(line.as_deref(), Some("cho last"));
}

#[test]
fn vim_capital_g_goes_to_oldest() {
    let line = vim_read_with_history(&["first", "second"], seq![[esc()], chars("G"), [enter()]]);
    assert_eq!(line.as_deref(), Some("first"));
}

#[test]
fn vim_slash_searches_history() {
    let line = vim_read_with_history(
        &["echo alpha", "ls beta", "echo gamma"],
        seq![[esc()], chars("/alpha"), [enter()], [enter()]],
    );
    assert_eq!(line.as_deref(), Some("echo alpha"));
}

#[test]
fn vim_hash_comments_and_submits() {
    let line = vim_read(seq![chars("echo hi"), [esc()], chars("#")]);
    assert_eq!(line.as_deref(), Some("#echo hi"));
}

#[test]
fn vim_incomplete_enter_appends_continuation_at_buffer_end() {
    let aliases = yosh::env::aliases::AliasStore::default();
    let odd_quotes = |s: &str| s.chars().filter(|&c| c == '\'').count() % 2 == 1;
    let line = vim_read_full(
        seq![
            chars("echo 'a"),
            [esc()],
            chars("0"),
            [enter()],
            chars("b'"),
            [enter()]
        ],
        &aliases,
        &odd_quotes,
    );
    assert_eq!(line.as_deref(), Some("echo 'a\nb'"));
}

#[test]
fn vim_alias_macro_runs_editing_commands() {
    let mut aliases = yosh::env::aliases::AliasStore::default();
    aliases.set("_m", "0x");
    let line = vim_read_full(
        seq![chars("abc"), [esc()], chars("@m"), [enter()]],
        &aliases,
        &|_| false,
    );
    assert_eq!(line.as_deref(), Some("bc"));
}

#[test]
fn vim_k_within_multiline_clamps_off_the_newline() {
    let line = vim_read(seq![
        chars("ab"),
        [alt_enter()],
        chars("wxyz"),
        [esc()],
        chars("kx"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("a\nwxyz"));
}

#[test]
fn vim_insert_ctrl_w_stops_at_logical_line_start() {
    let line = vim_read(seq![
        chars("abc"),
        [alt_enter()],
        [ctrl('w')],
        chars("z"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("abc\nz"));
}
