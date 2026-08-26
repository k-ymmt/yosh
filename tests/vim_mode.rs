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

// ── Phase 2: typed unnamed register + linewise Normal-mode ops ─────────

#[test]
fn vim_dd_deletes_line_and_trailing_separator() {
    let line = vim_read(seq![
        chars("aa"),
        [alt_enter()],
        chars("bb"),
        [esc()],
        chars("kdd"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("bb"));
}

#[test]
fn vim_dd_on_last_line_consumes_preceding_separator() {
    let line = vim_read(seq![
        chars("aa"),
        [alt_enter()],
        chars("bb"),
        [esc()],
        chars("dd"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("aa"));
}

#[test]
fn vim_dd_whole_buffer_leaves_empty() {
    let line = vim_read(seq![
        chars("aa"),
        [esc()],
        chars("dd"),
        chars("iok"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("ok"));
}

#[test]
fn vim_count_dd_deletes_lines() {
    let line = vim_read(seq![
        chars("aa"),
        [alt_enter()],
        chars("bb"),
        [alt_enter()],
        chars("cc"),
        [esc()],
        chars("kk2dd"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("cc"));
}

#[test]
fn vim_dd_cursor_lands_on_first_non_blank() {
    let line = vim_read(seq![
        chars("aa"),
        [alt_enter()],
        chars("  bb"),
        [esc()],
        chars("kddx"),
        [enter()]
    ]);
    // dd removes "aa\n"; cursor on the 'b' of "  bb"; x deletes it.
    assert_eq!(line.as_deref(), Some("  b"));
}

#[test]
fn vim_cc_collapses_line_to_empty_and_inserts() {
    // Oracle-verified: cc on line b of a\nb\nc yields a\n<insert>\nc.
    let line = vim_read(seq![
        chars("a"),
        [alt_enter()],
        chars("b"),
        [alt_enter()],
        chars("c"),
        [esc()],
        chars("kcc"),
        chars("X"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("a\nX\nc"));
}

#[test]
fn vim_count_cc_collapses_lines_to_one() {
    let line = vim_read(seq![
        chars("a"),
        [alt_enter()],
        chars("b"),
        [alt_enter()],
        chars("c"),
        [esc()],
        chars("kk2cc"),
        chars("X"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("X\nc"));
}

#[test]
fn vim_yy_p_pastes_line_below() {
    let line = vim_read(seq![chars("ab"), [esc()], chars("yyp"), [enter()]]);
    assert_eq!(line.as_deref(), Some("ab\nab"));
}

#[test]
fn vim_capital_y_is_linewise_yy() {
    // Vim default: Y = yy, not POSIX y$.
    let line = vim_read(seq![chars("ab"), [esc()], chars("0Yp"), [enter()]]);
    assert_eq!(line.as_deref(), Some("ab\nab"));
}

#[test]
fn vim_yy_capital_p_pastes_line_above() {
    let line = vim_read(seq![
        chars("aa"),
        [alt_enter()],
        chars("bb"),
        [esc()],
        chars("yyP"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("aa\nbb\nbb"));
}

#[test]
fn vim_ddp_swaps_lines() {
    let line = vim_read(seq![
        chars("aa"),
        [alt_enter()],
        chars("bb"),
        [esc()],
        chars("kddp"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("bb\naa"));
}

#[test]
fn vim_linewise_put_with_count_repeats_block() {
    let line = vim_read(seq![chars("ab"), [esc()], chars("yy2p"), [enter()]]);
    assert_eq!(line.as_deref(), Some("ab\nab\nab"));
}

#[test]
fn vim_linewise_put_cursor_on_first_non_blank_of_pasted_line() {
    let line = vim_read(seq![
        chars("  ab"),
        [esc()],
        chars("yypx"),
        [enter()]
    ]);
    // Pasted "  ab" below; cursor on its 'a'; x deletes it.
    assert_eq!(line.as_deref(), Some("  ab\n  b"));
}

#[test]
fn vim_charwise_put_splices_at_cursor() {
    let line = vim_read(seq![chars("abc"), [esc()], chars("0xp"), [enter()]]);
    assert_eq!(line.as_deref(), Some("bac"));
}

#[test]
fn vim_dot_repeats_dd() {
    let line = vim_read(seq![
        chars("a"),
        [alt_enter()],
        chars("b"),
        [alt_enter()],
        chars("c"),
        [esc()],
        chars("kkdd."),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("c"));
}

#[test]
fn vim_register_persists_across_reads() {
    let mut ed = LineEditor::new();
    ed.set_edit_mode(EditMode::Vim);
    let mut history = History::new();
    let mut term = MockTerminal::new(seq![chars("ab"), [esc()], chars("yy"), [enter()]]);
    let first = ed
        .read_line("$ ", &[], &mut history, &mut term)
        .expect("read failed");
    assert_eq!(first.as_deref(), Some("ab"));
    let mut term = MockTerminal::new(seq![[esc()], chars("p"), [enter()]]);
    let second = ed
        .read_line("$ ", &[], &mut history, &mut term)
        .expect("read failed");
    assert_eq!(second.as_deref(), Some("\nab"));
}

#[test]
fn vim_p_after_emacs_kill_reads_kill_ring_front() {
    // Kill in emacs mode, then switch the same editor to vim and put:
    // the unnamed register mirrors the kill ring's front entry.
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let mut term = MockTerminal::new(seq![chars("abc"), [ctrl('a')], [ctrl('k')], [enter()]]);
    let first = ed
        .read_line("$ ", &[], &mut history, &mut term)
        .expect("read failed");
    assert_eq!(first.as_deref(), Some(""));
    ed.set_edit_mode(EditMode::Vim);
    let mut term = MockTerminal::new(seq![[esc()], chars("p"), [enter()]]);
    let second = ed
        .read_line("$ ", &[], &mut history, &mut term)
        .expect("read failed");
    assert_eq!(second.as_deref(), Some("abc"));
}

#[test]
fn vim_p_after_merged_emacs_kills_puts_merged_whole() {
    // Two consecutive emacs backward-word kills merge in the ring; the
    // register mirrors the merged front entry.
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let mut term = MockTerminal::new(seq![chars("one two"), [ctrl('w')], [ctrl('w')], [enter()]]);
    let first = ed
        .read_line("$ ", &[], &mut history, &mut term)
        .expect("read failed");
    assert_eq!(first.as_deref(), Some(""));
    ed.set_edit_mode(EditMode::Vim);
    let mut term = MockTerminal::new(seq![[esc()], chars("p"), [enter()]]);
    let second = ed
        .read_line("$ ", &[], &mut history, &mut term)
        .expect("read failed");
    assert_eq!(second.as_deref(), Some("one two"));
}
