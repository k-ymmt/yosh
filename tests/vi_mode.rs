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
