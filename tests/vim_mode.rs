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
    vim_read_full_with(events, aliases, is_incomplete, None).0
}

/// `vim_read_full` with an optional editor-command override (for
/// `Ctrl-X Ctrl-E`); also returns the terminal for output assertions.
fn vim_read_full_with(
    events: Vec<crossterm::event::Event>,
    aliases: &yosh::env::aliases::AliasStore,
    is_incomplete: &dyn Fn(&str) -> bool,
    editor_cmd: Option<&str>,
) -> (Option<String>, MockTerminal) {
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
    if let Some(cmd) = editor_cmd {
        editor.set_editor_command(cmd.to_string());
    }
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
    let line = editor
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
        .expect("read failed");
    (line, term)
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

// ── Phase 3: VISUAL mode ───────────────────────────────────────────────

#[test]
fn vim_visual_motion_extends_and_d_deletes() {
    // v at 'e', e to end of "echo": selection "echo"; d deletes it.
    let line = vim_read(seq![
        chars("echo hello"),
        [esc()],
        chars("0ved"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some(" hello"));
}

#[test]
fn vim_visual_x_deletes_selection() {
    let line = vim_read(seq![chars("abc"), [esc()], chars("0vlx"), [enter()]]);
    assert_eq!(line.as_deref(), Some("c"));
}

#[test]
fn vim_visual_c_deletes_and_enters_insert() {
    let line = vim_read(seq![
        chars("echo hello"),
        [esc()],
        chars("0vec"),
        chars("say"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("say hello"));
}

#[test]
fn vim_visual_y_then_p_puts_charwise() {
    // v l selects "ab"; y yanks, cursor to selection start; p after
    // cursor 0 → "aabbc".
    let line = vim_read(seq![chars("abc"), [esc()], chars("0vlyp"), [enter()]]);
    assert_eq!(line.as_deref(), Some("aabbc"));
}

#[test]
fn vim_visual_y_cursor_moves_to_selection_start() {
    let line = vim_read(seq![chars("abcde"), [esc()], chars("0v2lyx"), [enter()]]);
    assert_eq!(line.as_deref(), Some("bcde"));
}

#[test]
fn vim_visual_line_d_deletes_whole_line() {
    let line = vim_read(seq![
        chars("aa"),
        [alt_enter()],
        chars("bb"),
        [esc()],
        chars("Vd"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("aa"));
}

#[test]
fn vim_visual_line_c_collapses_to_empty_line() {
    let line = vim_read(seq![
        chars("a"),
        [alt_enter()],
        chars("b"),
        [alt_enter()],
        chars("c"),
        [esc()],
        chars("kVc"),
        chars("X"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("a\nX\nc"));
}

#[test]
fn vim_visual_kind_switch_preserves_anchor() {
    // v..l charwise, V switches to linewise (anchor kept), d deletes
    // the whole touched line.
    let line = vim_read(seq![
        chars("aa"),
        [alt_enter()],
        chars("bb"),
        [esc()],
        chars("vVd"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("aa"));
}

#[test]
fn vim_visual_same_kind_toggle_exits() {
    // v then v exits VISUAL; x then acts as a Command-mode delete.
    let line = vim_read(seq![chars("abc"), [esc()], chars("0vvx"), [enter()]]);
    assert_eq!(line.as_deref(), Some("bc"));
}

#[test]
fn vim_visual_esc_exits_to_command() {
    let line = vim_read(seq![chars("abc"), [esc()], chars("0v"), [esc()], chars("x"), [enter()]]);
    assert_eq!(line.as_deref(), Some("bc"));
}

#[test]
fn vim_visual_ctrl_c_exits_without_cancelling_line() {
    // Vim behavior: Ctrl-C leaves VISUAL; it does not cancel the read.
    let line = vim_read(seq![
        chars("abc"),
        [esc()],
        chars("v"),
        [ctrl('c')],
        chars("x"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("ab"));
}

#[test]
fn vim_visual_o_swaps_ends() {
    // v at 0, 2l → sel a..c; o → cursor back to 0 with anchor at 2;
    // l shrinks the selection to b..c; d deletes "bc".
    let line = vim_read(seq![chars("abcde"), [esc()], chars("0v2lold"), [enter()]]);
    assert_eq!(line.as_deref(), Some("ade"));
}

#[test]
fn vim_visual_r_replaces_selection_preserving_newlines() {
    // Oracle-verified: charwise rX over ab\ncd → selection chars become
    // X but the '\n' separator survives.
    let line = vim_read(seq![
        chars("ab"),
        [alt_enter()],
        chars("cd"),
        [esc()],
        chars("vkrX"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("aX\nXX"));
}

#[test]
fn vim_visual_tilde_toggles_case_of_selection() {
    let line = vim_read(seq![chars("abc"), [esc()], chars("0vl~"), [enter()]]);
    assert_eq!(line.as_deref(), Some("ABc"));
}

#[test]
fn vim_visual_enter_submits_whole_line() {
    let line = vim_read(seq![chars("abc"), [esc()], chars("0vl"), [enter()]]);
    assert_eq!(line.as_deref(), Some("abc"));
}

#[test]
fn vim_visual_k_at_edge_is_pure_buffer_motion() {
    // k in VISUAL never recalls history: the selection stays on the
    // current buffer and the cursor simply stops at the first line.
    let line = vim_read_with_history(
        &["prev entry"],
        seq![chars("abc"), [esc()], chars("0vkd"), [enter()]],
    );
    assert_eq!(line.as_deref(), Some("bc"));
}

#[test]
fn vim_visual_capital_d_deletes_touched_lines() {
    // Charwise selection; D deletes the whole logical lines it touches.
    let line = vim_read(seq![
        chars("aa"),
        [alt_enter()],
        chars("bb"),
        [esc()],
        chars("vD"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("aa"));
}

#[test]
fn vim_visual_capital_y_yanks_lines() {
    let line = vim_read(seq![chars("ab"), [esc()], chars("vYp"), [enter()]]);
    assert_eq!(line.as_deref(), Some("ab\nab"));
}

#[test]
fn vim_visual_p_swaps_deleted_text_into_register() {
    // Yank "ab", select "cd", p replaces it; the register now holds the
    // deleted "cd", so a Command-mode p pastes "cd".
    let line = vim_read(seq![
        chars("ab cd"),
        [esc()],
        chars("0vly"),
        chars("wvlp"),
        chars("p"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("ab abcd"));
}

#[test]
fn vim_visual_capital_p_leaves_register_unchanged() {
    let line = vim_read(seq![
        chars("ab cd"),
        [esc()],
        chars("0vly"),
        chars("wvlP"),
        chars("p"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("ab abab"));
}

#[test]
fn vim_visual_empty_buffer_operators_are_noops() {
    // v on an empty buffer: d is a bell-free no-op; typing continues.
    let line = vim_read(seq![[esc()], chars("vd"), chars("iok"), [enter()]]);
    assert_eq!(line.as_deref(), Some("ok"));
}

#[test]
fn vim_visual_empty_buffer_c_enters_insert() {
    let line = vim_read(seq![[esc()], chars("vc"), chars("ok"), [enter()]]);
    assert_eq!(line.as_deref(), Some("ok"));
}

// ── Phase 3: selection rendering ([REV] markers) ───────────────────────

#[test]
fn vim_visual_selection_renders_reverse_video() {
    let mut ed = LineEditor::new();
    ed.set_edit_mode(EditMode::Vim);
    let mut history = History::new();
    let mut term = MockTerminal::new(seq![chars("abc"), [esc()], chars("0vl"), [enter()]]);
    let line = ed
        .read_line("$ ", &[], &mut history, &mut term)
        .expect("read failed");
    assert_eq!(line.as_deref(), Some("abc"));
    let all = term.output().join("");
    // After `v l` the selection covers "ab": reverse on, both chars,
    // reverse off before the unselected 'c'.
    assert!(all.contains("[REV]ab[/REV]"), "no reverse span in: {all}");
}

#[test]
fn vim_visual_selection_to_buffer_end_cleans_up_reverse() {
    let mut ed = LineEditor::new();
    ed.set_edit_mode(EditMode::Vim);
    let mut history = History::new();
    // ESC leaves the cursor on 'b'; v selects it — the selection ends
    // at the buffer's last character (boundary cleanup case).
    let mut term = MockTerminal::new(seq![chars("ab"), [esc()], chars("v"), [enter()]]);
    let line = ed
        .read_line("$ ", &[], &mut history, &mut term)
        .expect("read failed");
    assert_eq!(line.as_deref(), Some("ab"));
    let all = term.output().join("");
    assert!(all.contains("[REV]b[/REV]"), "no boundary cleanup in: {all}");
}

#[test]
fn vim_visual_reverse_reasserted_across_style_transitions() {
    // With syntax highlighting active, a selection spanning several
    // style spans must re-assert reverse after every style transition
    // (reset_style clears the reverse attribute).
    let aliases = yosh::env::aliases::AliasStore::default();
    let (line, term) = vim_read_full_with(
        seq![chars("echo 'hi'"), [esc()], chars("0v$"), [enter()]],
        &aliases,
        &|_| false,
        None,
    );
    assert_eq!(line.as_deref(), Some("echo 'hi'"));
    let all = term.output().join("");
    let frames: Vec<&str> = all.split("[REV]").collect();
    // At least two reverse assertions in the fully-selected frame
    // (initial entry + at least one re-assertion at a span boundary).
    assert!(
        frames.len() >= 3,
        "expected multiple [REV] assertions in: {all}"
    );
}

#[test]
fn vim_visual_multiline_selection_keeps_prompts_unreversed() {
    // A linewise selection over a two-line buffer: reverse must be off
    // before the row break / continuation prompt is written.
    let aliases = yosh::env::aliases::AliasStore::default();
    let (line, term) = vim_read_full_with(
        seq![
            chars("echo 'a"),
            [esc()],
            [enter()],
            chars("b'"),
            [esc()],
            chars("Vk"),
            [enter()]
        ],
        &aliases,
        &|s: &str| s.chars().filter(|&c| c == '\'').count() % 2 == 1,
        None,
    );
    assert_eq!(line.as_deref(), Some("echo 'a\nb'"));
    let all = term.output().join("");
    // The continuation prompt "> " must never be inside a reverse span:
    // no "[REV]" directly abutting the prompt without an intervening
    // "[/REV]".
    for (i, _) in all.match_indices("> ") {
        let before = &all[..i];
        let on = before.matches("[REV]").count();
        let off = before.matches("[/REV]").count();
        assert!(on == off, "prompt rendered inside reverse span: {all}");
    }
}

// ── Phase 3: Ctrl-X Ctrl-E ─────────────────────────────────────────────

/// Write a stub editor script that replaces the temp file's content.
fn stub_editor(dir: &std::path::Path, new_content: &str) -> String {
    let script = dir.join("stub_editor.sh");
    std::fs::write(
        &script,
        format!("#!/bin/sh\nprintf '%s' '{new_content}' > \"$1\"\n"),
    )
    .unwrap();
    format!("/bin/sh {}", script.display())
}

#[test]
fn vim_ctrl_x_ctrl_e_loads_result_without_submitting() {
    let dir = tempfile::tempdir().expect("tempdir");
    let editor = stub_editor(dir.path(), "echo edited");
    let aliases = yosh::env::aliases::AliasStore::default();
    // The edited buffer is NOT submitted: appending " ok" then Enter
    // proves the read continued.
    let (line, _term) = vim_read_full_with(
        seq![
            chars("orig"),
            [esc()],
            [ctrl('x')],
            [ctrl('e')],
            chars("A ok"),
            [enter()]
        ],
        &aliases,
        &|_| false,
        Some(&editor),
    );
    assert_eq!(line.as_deref(), Some("echo edited ok"));
}

#[test]
fn vim_ctrl_x_ctrl_e_is_undoable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let editor = stub_editor(dir.path(), "echo edited");
    let aliases = yosh::env::aliases::AliasStore::default();
    let (line, _term) = vim_read_full_with(
        seq![
            chars("orig"),
            [esc()],
            [ctrl('x')],
            [ctrl('e')],
            chars("u"),
            [enter()]
        ],
        &aliases,
        &|_| false,
        Some(&editor),
    );
    assert_eq!(line.as_deref(), Some("orig"));
}

#[test]
fn vim_ctrl_x_other_key_bells() {
    // Ctrl-X followed by anything but Ctrl-E is not a chord.
    let line = vim_read(seq![
        chars("abc"),
        [esc()],
        [ctrl('x')],
        [ctrl('l')],
        chars("x"),
        [enter()]
    ]);
    // Ctrl-X Ctrl-L bells (chord broken); x then deletes normally.
    assert_eq!(line.as_deref(), Some("ab"));
}

// ── Phase 4: text objects + motions ────────────────────────────────────

#[test]
fn vim_diw_deletes_inner_word() {
    let line = vim_read(seq![
        chars("one two three"),
        [esc()],
        chars("0wdiw"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("one  three"));
}

#[test]
fn vim_daw_includes_trailing_whitespace() {
    let line = vim_read(seq![
        chars("one two three"),
        [esc()],
        chars("0wdaw"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("one three"));
}

#[test]
fn vim_daw_on_last_word_takes_leading_whitespace() {
    let line = vim_read(seq![chars("one two"), [esc()], chars("daw"), [enter()]]);
    assert_eq!(line.as_deref(), Some("one"));
}

#[test]
fn vim_count_applies_to_aw() {
    let line = vim_read(seq![
        chars("one two three"),
        [esc()],
        chars("0d2aw"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("three"));
}

#[test]
fn vim_ciw_changes_word() {
    let line = vim_read(seq![
        chars("one two"),
        [esc()],
        chars("0ciw"),
        chars("1"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("1 two"));
}

#[test]
fn vim_ci_quote_changes_quoted_string() {
    let line = vim_read(seq![
        chars("say \"hi there\" now"),
        [esc()],
        chars("07lci\""),
        chars("bye"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("say \"bye\" now"));
}

#[test]
fn vim_di_quote_before_first_quote_targets_next_span() {
    let line = vim_read(seq![
        chars("say 'hi'"),
        [esc()],
        chars("0di'"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("say ''"));
}

#[test]
fn vim_ci_paren_on_empty_pair_enters_insert_inside() {
    // Oracle-verified: ci(X on () yields (X).
    let line = vim_read(seq![
        chars("()"),
        [esc()],
        chars("0ci("),
        chars("X"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("(X)"));
}

#[test]
fn vim_di_paren_nested_resolves_enclosing() {
    let line = vim_read(seq![
        chars("f(a(b)c)"),
        [esc()],
        chars("02ldi("),
        [enter()]
    ]);
    // Cursor on 'a' (index 2): the enclosing pair is the outer one.
    assert_eq!(line.as_deref(), Some("f()"));
}

#[test]
fn vim_da_bracket_multiline() {
    let line = vim_read(seq![
        chars("f[a"),
        [alt_enter()],
        chars("b]c"),
        [esc()],
        chars("kllda["),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("fc"));
}

#[test]
fn vim_unknown_object_char_bells() {
    let line = vim_read(seq![chars("abc"), [esc()], chars("0diq"), [enter()]]);
    assert_eq!(line.as_deref(), Some("abc"));
}

#[test]
fn vim_percent_jumps_to_match() {
    // Oracle-verified: 0%x on `{ "}" }` deletes the final }, not the
    // quoted one.
    let line = vim_read(seq![
        chars("{ \"}\" }"),
        [esc()],
        chars("0%x"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("{ \"}\" "));
}

#[test]
fn vim_d_percent_is_inclusive() {
    let line = vim_read(seq![chars("(ab) c"), [esc()], chars("0d%"), [enter()]]);
    assert_eq!(line.as_deref(), Some(" c"));
}

#[test]
fn vim_percent_without_pair_bells() {
    let line = vim_read(seq![chars("abc"), [esc()], chars("0%x"), [enter()]]);
    // % bells; x still deletes at the unmoved cursor.
    assert_eq!(line.as_deref(), Some("bc"));
}

#[test]
fn vim_ge_moves_to_previous_word_end() {
    let line = vim_read(seq![
        chars("one two three"),
        [esc()],
        chars("0wwgex"),
        [enter()]
    ]);
    // From 't' of three: ge lands on the 'o' of two; x deletes it.
    assert_eq!(line.as_deref(), Some("one tw three"));
}

#[test]
fn vim_dge_is_inclusive_backward() {
    let line = vim_read(seq![chars("one two"), [esc()], chars("0wdge"), [enter()]]);
    // From 't' of two (4): dge deletes 'e'(2)..='t'(4) → "on" + "wo".
    assert_eq!(line.as_deref(), Some("onwo"));
}

#[test]
fn vim_g_plus_other_key_bells() {
    let line = vim_read(seq![chars("abc"), [esc()], chars("0gzx"), [enter()]]);
    assert_eq!(line.as_deref(), Some("bc"));
}

#[test]
fn vim_dot_repeats_diw() {
    let line = vim_read(seq![
        chars("aa bb"),
        [esc()],
        chars("0diww."),
        [enter()]
    ]);
    // diw deletes "aa"; w moves to "bb"; . repeats diw.
    assert_eq!(line.as_deref(), Some(" "));
}

#[test]
fn vim_visual_iw_selects_word_from_single_char() {
    let line = vim_read(seq![
        chars("one two"),
        [esc()],
        chars("0wviwd"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some("one "));
}

#[test]
fn vim_visual_iw_extends_larger_selection() {
    // Oracle-verified: 0vwiwd on `one two three` leaves ` three`.
    let line = vim_read(seq![
        chars("one two three"),
        [esc()],
        chars("0vwiwd"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some(" three"));
}

#[test]
fn vim_visual_percent_extends_selection() {
    let line = vim_read(seq![chars("(ab) c"), [esc()], chars("0v%d"), [enter()]]);
    assert_eq!(line.as_deref(), Some(" c"));
}

#[test]
fn vim_visual_ge_extends_selection() {
    let line = vim_read(seq![chars("one two"), [esc()], chars("0wvged"), [enter()]]);
    assert_eq!(line.as_deref(), Some("onwo"));
}

#[test]
fn vim_visual_empty_inner_object_bells_keeps_selection() {
    // i" on "" leaves the selection unchanged with a bell; d then
    // deletes the original one-char selection.
    let line = vim_read(seq![
        chars("x \"\" y"),
        [esc()],
        chars("0vi\"d"),
        [enter()]
    ]);
    assert_eq!(line.as_deref(), Some(" \"\" y"));
}
