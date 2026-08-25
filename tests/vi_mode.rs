// Integration tests for the vi editing mode (POSIX sh vi Line Editing),
// driven through LineEditor::read_line with a mock terminal.

use crossterm::event::KeyCode;
use yosh::interactive::history::History;
use yosh::interactive::line_editor::LineEditor;
use yosh::interactive::vi::EditMode;

mod helpers;
use helpers::mock_terminal::{MockTerminal, chars, ctrl, key};

/// Run a vi-mode read over the given events and return the submitted line
/// (None = EOF).
fn vi_read(events: Vec<crossterm::event::Event>) -> Option<String> {
    let mut ed = LineEditor::new();
    ed.set_edit_mode(EditMode::Vi);
    let mut history = History::new();
    let mut term = MockTerminal::new(events);
    ed.read_line("$ ", &[], &mut history, &mut term)
        .expect("read_line failed")
}

fn esc() -> crossterm::event::Event {
    key(KeyCode::Esc)
}

fn enter() -> crossterm::event::Event {
    key(KeyCode::Enter)
}

/// Build an event sequence from segments: plain strings become chars.
macro_rules! seq {
    ($($part:expr),* $(,)?) => {{
        let mut v: Vec<crossterm::event::Event> = Vec::new();
        $( v.extend($part); )*
        v
    }};
}

#[test]
fn vi_insert_mode_types_and_submits() {
    let line = vi_read(seq![chars("echo hi"), [enter()]]);
    assert_eq!(line.as_deref(), Some("echo hi"));
}

#[test]
fn vi_esc_moves_cursor_back_and_x_deletes() {
    // "abc", ESC leaves cursor on 'c'... h moves to 'b', x deletes it.
    let line = vi_read(seq![chars("abc"), [esc()], chars("hx"), [enter()]]);
    assert_eq!(line.as_deref(), Some("ac"));
}

#[test]
fn vi_zero_motion_and_x() {
    let line = vi_read(seq![chars("abc"), [esc()], chars("0x"), [enter()]]);
    assert_eq!(line.as_deref(), Some("bc"));
}

#[test]
fn vi_count_applies_to_x() {
    let line = vi_read(seq![chars("abcdef"), [esc()], chars("02x"), [enter()]]);
    assert_eq!(line.as_deref(), Some("cdef"));
}

#[test]
fn vi_count_applies_to_motion() {
    // 0 then 3l lands on 'd'; x deletes it.
    let line = vi_read(seq![chars("abcde"), [esc()], chars("03lx"), [enter()]]);
    assert_eq!(line.as_deref(), Some("abce"));
}

#[test]
fn vi_dollar_moves_to_last_char() {
    let line = vi_read(seq![chars("abcde"), [esc()], chars("0$x"), [enter()]]);
    assert_eq!(line.as_deref(), Some("abcd"));
}

#[test]
fn vi_word_motion_w() {
    let line = vi_read(seq![chars("echo hello"), [esc()], chars("0wx"), [enter()]]);
    assert_eq!(line.as_deref(), Some("echo ello"));
}

#[test]
fn vi_find_char_motion() {
    let line = vi_read(seq![chars("echo hello"), [esc()], chars("0flx"), [enter()]]);
    assert_eq!(line.as_deref(), Some("echo helo"));
}

#[test]
fn vi_insert_entry_i_at_cursor() {
    // ESC on "abc" → cursor 'c'; h → 'b'; i inserts before 'b'.
    let line = vi_read(seq![
        chars("abc"),
        [esc()],
        chars("hi"),
        chars("X"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("aXbc"));
}

#[test]
fn vi_insert_entry_a_after_cursor() {
    let line = vi_read(seq![
        chars("abc"),
        [esc()],
        chars("0a"),
        chars("X"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("aXbc"));
}

#[test]
fn vi_insert_entry_capital_a_appends_at_line_end() {
    let line = vi_read(seq![
        chars("ab"),
        [esc()],
        chars("0A"),
        chars("c"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("abc"));
}

#[test]
fn vi_insert_entry_capital_i_at_first_non_blank() {
    let line = vi_read(seq![
        chars("  ab"),
        [esc()],
        chars("I"),
        chars("X"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("  Xab"));
}

#[test]
fn vi_replace_char() {
    let line = vi_read(seq![chars("abc"), [esc()], chars("0rz"), [enter()]]);
    assert_eq!(line.as_deref(), Some("zbc"));
}

#[test]
fn vi_replace_char_with_count() {
    let line = vi_read(seq![chars("abcd"), [esc()], chars("03rz"), [enter()]]);
    assert_eq!(line.as_deref(), Some("zzzd"));
}

#[test]
fn vi_replace_mode_overwrites() {
    let line = vi_read(seq![
        chars("abc"),
        [esc()],
        chars("0R"),
        chars("xy"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("xyc"));
}

#[test]
fn vi_replace_mode_appends_past_line_end() {
    let line = vi_read(seq![
        chars("ab"),
        [esc()],
        chars("0R"),
        chars("xyz"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("xyz"));
}

#[test]
fn vi_tilde_toggles_case_and_advances() {
    let line = vi_read(seq![chars("abc"), [esc()], chars("0~~"), [enter()]]);
    assert_eq!(line.as_deref(), Some("ABc"));
}

#[test]
fn vi_capital_x_deletes_before_cursor() {
    // ESC on "abc" → cursor on 'c' (pos 2); X deletes 'b'.
    let line = vi_read(seq![chars("abc"), [esc()], chars("X"), [enter()]]);
    assert_eq!(line.as_deref(), Some("ac"));
}

#[test]
fn vi_ctrl_c_interrupts_in_command_mode() {
    let line = vi_read(seq![chars("abc"), [esc()], [ctrl('c')]]);
    assert_eq!(line.as_deref(), Some(""));
}

#[test]
fn vi_ctrl_d_is_eof_on_empty_line_in_command_mode() {
    let line = vi_read(seq![[esc()], [ctrl('d')]]);
    assert_eq!(line, None);
}

#[test]
fn vi_semicolon_repeats_find() {
    // 0 f l → first 'l' (index 6 of "echo hello"); ; → second 'l'; x.
    let line = vi_read(seq![
        chars("echo hello"),
        [esc()],
        chars("0fl;x"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("echo helo"));
}

#[test]
fn vi_invalid_command_rings_bell_and_keeps_buffer() {
    // 'q' is not a vi command: buffer unchanged, still submits fine.
    let line = vi_read(seq![chars("abc"), [esc()], chars("q"), [enter()]]);
    assert_eq!(line.as_deref(), Some("abc"));
}

// ── Operators / put / undo / repeat ────────────────────────────────────

#[test]
fn vi_dw_deletes_word() {
    let line = vi_read(seq![chars("echo hello"), [esc()], chars("0dw"), [enter()]]);
    assert_eq!(line.as_deref(), Some("hello"));
}

#[test]
fn vi_dw_on_last_word_deletes_to_end() {
    let line = vi_read(seq![chars("echo hello"), [esc()], chars("0wdw"), [enter()]]);
    assert_eq!(line.as_deref(), Some("echo "));
}

#[test]
fn vi_de_keeps_following_space() {
    let line = vi_read(seq![chars("echo hello"), [esc()], chars("0de"), [enter()]]);
    assert_eq!(line.as_deref(), Some(" hello"));
}

#[test]
fn vi_d_dollar_deletes_to_line_end() {
    let line = vi_read(seq![chars("echo hello"), [esc()], chars("0wd$"), [enter()]]);
    assert_eq!(line.as_deref(), Some("echo "));
}

#[test]
fn vi_db_excludes_cursor_char() {
    // Cursor on 'h' of hello; db deletes "echo " leaving "hello".
    let line = vi_read(seq![chars("echo hello"), [esc()], chars("0wdb"), [enter()]]);
    assert_eq!(line.as_deref(), Some("hello"));
}

#[test]
fn vi_dd_clears_line() {
    let line = vi_read(seq![
        chars("echo hello"),
        [esc()],
        chars("dd"),
        chars("iok"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("ok"));
}

#[test]
fn vi_count_multiplies_across_operator() {
    // 2d2w from start of "a b c d e f" deletes 4 words.
    let line = vi_read(seq![
        chars("a b c d e f"),
        [esc()],
        chars("02d2w"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("e f"));
}

#[test]
fn vi_cw_changes_word_like_ce() {
    // cw on "hello world" replaces just "hello" (not the space).
    let line = vi_read(seq![
        chars("hello world"),
        [esc()],
        chars("0cw"),
        chars("bye"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("bye world"));
}

#[test]
fn vi_cc_changes_whole_line() {
    let line = vi_read(seq![
        chars("old stuff"),
        [esc()],
        chars("cc"),
        chars("new"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("new"));
}

#[test]
fn vi_capital_s_equals_cc() {
    let line = vi_read(seq![
        chars("old stuff"),
        [esc()],
        chars("S"),
        chars("new"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("new"));
}

#[test]
fn vi_capital_c_changes_to_line_end() {
    let line = vi_read(seq![
        chars("echo hello"),
        [esc()],
        chars("0wC"),
        chars("bye"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("echo bye"));
}

#[test]
fn vi_capital_d_deletes_to_line_end() {
    let line = vi_read(seq![chars("echo hello"), [esc()], chars("0wD"), [enter()]]);
    assert_eq!(line.as_deref(), Some("echo "));
}

#[test]
fn vi_s_substitutes_chars() {
    let line = vi_read(seq![
        chars("abc"),
        [esc()],
        chars("02s"),
        chars("xy"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("xyc"));
}

#[test]
fn vi_x_then_p_pastes_after_cursor() {
    // x deletes 'a' (cursor now on 'b'); p puts 'a' after 'b'.
    let line = vi_read(seq![chars("abc"), [esc()], chars("0xp"), [enter()]]);
    assert_eq!(line.as_deref(), Some("bac"));
}

#[test]
fn vi_yy_then_p_duplicates_line_text() {
    let line = vi_read(seq![chars("ab"), [esc()], chars("yy$p"), [enter()]]);
    assert_eq!(line.as_deref(), Some("abab"));
}

#[test]
fn vi_yw_then_capital_p_pastes_before_cursor() {
    let line = vi_read(seq![chars("ab cd"), [esc()], chars("0ywP"), [enter()]]);
    assert_eq!(line.as_deref(), Some("ab ab cd"));
}

#[test]
fn vi_p_with_count_repeats_text() {
    let line = vi_read(seq![chars("ab"), [esc()], chars("x2p"), [enter()]]);
    // x deletes 'b' (cursor on 'a'), 2p puts "bb" after 'a'.
    assert_eq!(line.as_deref(), Some("abb"));
}

#[test]
fn vi_undo_restores_deleted_char() {
    let line = vi_read(seq![chars("abc"), [esc()], chars("0xu"), [enter()]]);
    assert_eq!(line.as_deref(), Some("abc"));
}

#[test]
fn vi_undo_reverts_whole_insert_session() {
    // cw+typed text is one change; a single u restores the original.
    let line = vi_read(seq![
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
fn vi_undo_all_restores_line_base() {
    // Multiple edits, then U restores the empty starting line... the
    // typed text itself belongs to the read's initial insert session,
    // whose baseline is the empty line.
    let line = vi_read(seq![
        chars("abc"),
        [esc()],
        chars("x0xU"),
        chars("iok"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("ok"));
}

#[test]
fn vi_dot_repeats_x() {
    let line = vi_read(seq![chars("abcd"), [esc()], chars("0x."), [enter()]]);
    assert_eq!(line.as_deref(), Some("cd"));
}

#[test]
fn vi_dot_with_count_overrides() {
    let line = vi_read(seq![chars("abcdef"), [esc()], chars("0x3."), [enter()]]);
    // x deletes 'a'; 3. deletes 'b','c','d'.
    assert_eq!(line.as_deref(), Some("ef"));
}

#[test]
fn vi_dot_repeats_insert_text() {
    // "a" appends "X" twice via `.`.
    let line = vi_read(seq![
        chars("s"),
        [esc()],
        chars("a"),
        chars("X"),
        [esc()],
        chars("."),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("sXX"));
}

#[test]
fn vi_dot_repeats_cw_with_text() {
    let line = vi_read(seq![
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

// ── History navigation and search ──────────────────────────────────────

/// Like `vi_read`, with pre-populated history entries (oldest first).
fn vi_read_with_history(entries: &[&str], events: Vec<crossterm::event::Event>) -> Option<String> {
    let mut ed = LineEditor::new();
    ed.set_edit_mode(EditMode::Vi);
    let mut history = History::new();
    for e in entries {
        history.add(e, 500, "");
    }
    let mut term = MockTerminal::new(events);
    ed.read_line("$ ", &[], &mut history, &mut term)
        .expect("read_line failed")
}

#[test]
fn vi_k_recalls_previous_with_cursor_at_start() {
    // k recalls "echo last"; x deletes the first char (cursor at start).
    let line = vi_read_with_history(&["echo last"], seq![[esc()], chars("kx"), [enter()]]);
    assert_eq!(line.as_deref(), Some("cho last"));
}

#[test]
fn vi_k_j_walk_history() {
    let line = vi_read_with_history(&["one", "two"], seq![[esc()], chars("kkj"), [enter()]]);
    assert_eq!(line.as_deref(), Some("two"));
}

#[test]
fn vi_capital_g_goes_to_oldest() {
    let line = vi_read_with_history(&["first", "second"], seq![[esc()], chars("G"), [enter()]]);
    assert_eq!(line.as_deref(), Some("first"));
}

#[test]
fn vi_count_g_goes_to_numbered_entry() {
    let line = vi_read_with_history(&["first", "second"], seq![[esc()], chars("2G"), [enter()]]);
    assert_eq!(line.as_deref(), Some("second"));
}

#[test]
fn vi_invalid_g_number_bells_and_keeps_buffer() {
    let line = vi_read_with_history(
        &["only"],
        seq![chars("keep"), [esc()], chars("9G"), [enter()]],
    );
    assert_eq!(line.as_deref(), Some("keep"));
}

#[test]
fn vi_slash_searches_backward() {
    let line = vi_read_with_history(
        &["echo alpha", "ls beta", "echo gamma"],
        seq![[esc()], chars("/alpha"), [enter()], [enter()]],
    );
    assert_eq!(line.as_deref(), Some("echo alpha"));
}

#[test]
fn vi_slash_caret_anchors_at_line_start() {
    // ^ls must match only at the beginning: skips "echo ls-like".
    let line = vi_read_with_history(
        &["ls one", "echo ls-like"],
        seq![[esc()], chars("/^ls"), [enter()], [enter()]],
    );
    assert_eq!(line.as_deref(), Some("ls one"));
}

#[test]
fn vi_n_repeats_search_older() {
    let line = vi_read_with_history(
        &["echo alpha", "ls beta", "echo gamma"],
        seq![[esc()], chars("/echo"), [enter()], chars("n"), [enter()]],
    );
    // /echo finds "echo gamma" (nearest older); n continues to "echo alpha".
    assert_eq!(line.as_deref(), Some("echo alpha"));
}

#[test]
fn vi_capital_n_reverses_search_direction() {
    let line = vi_read_with_history(
        &["echo alpha", "ls beta", "echo gamma"],
        seq![
            [esc()],
            chars("/echo"),
            [enter()],
            chars("n"),
            chars("N"),
            [enter()]
        ],
    );
    // gamma → alpha (n) → back to gamma (N).
    assert_eq!(line.as_deref(), Some("echo gamma"));
}

#[test]
fn vi_search_no_match_keeps_buffer() {
    let line = vi_read_with_history(
        &["one"],
        seq![chars("keep"), [esc()], chars("/zzz"), [enter()], [enter()]],
    );
    assert_eq!(line.as_deref(), Some("keep"));
}

#[test]
fn vi_search_esc_cancels_input() {
    let line = vi_read_with_history(
        &["one"],
        seq![chars("keep"), [esc()], chars("/on"), [esc()], [enter()]],
    );
    assert_eq!(line.as_deref(), Some("keep"));
}

#[test]
fn vi_underscore_appends_last_bigword_of_previous_line() {
    let line = vi_read_with_history(
        &["cp file1 file2"],
        seq![chars("echo"), [esc()], chars("_"), [enter()]],
    );
    assert_eq!(line.as_deref(), Some("echo file2"));
}

#[test]
fn vi_count_underscore_selects_bigword() {
    let line = vi_read_with_history(
        &["cp file1 file2"],
        seq![chars("echo"), [esc()], chars("2_"), [enter()]],
    );
    assert_eq!(line.as_deref(), Some("echo file1"));
}

#[test]
fn vi_undo_all_after_history_recall_restores_entry() {
    // Recall, mangle, U restores the recalled entry.
    let line = vi_read_with_history(&["echo hello"], seq![[esc()], chars("kxxxU"), [enter()]]);
    assert_eq!(line.as_deref(), Some("echo hello"));
}

#[test]
fn emacs_mode_ignores_esc_and_vi_keys() {
    // Default mode is emacs: ESC is a no-op and 'x' is a plain char.
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let mut term = MockTerminal::new(seq![chars("ab"), [esc()], chars("x"), [enter()]]);
    let line = ed
        .read_line("$ ", &[], &mut history, &mut term)
        .expect("read_line failed");
    assert_eq!(line.as_deref(), Some("abx"));
}
