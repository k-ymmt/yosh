use crossterm::event::KeyCode;
use kish::env::ShellEnv;
use kish::env::aliases::AliasStore;
use kish::interactive::fuzzy_search::FuzzySearchUI;
use kish::interactive::history::History;
use kish::interactive::line_editor::LineEditor;
use kish::interactive::parse_status::{classify_parse, ParseStatus};
use kish::interactive::prompt::expand_prompt;

mod helpers;
use helpers::mock_terminal::{MockTerminal, chars, ctrl, key};

#[test]
fn test_insert_char_at_start() {
    let mut ed = LineEditor::new();
    ed.insert_char('a');
    assert_eq!(ed.buffer(), "a");
    assert_eq!(ed.cursor(), 1);
}

#[test]
fn test_insert_char_multiple() {
    let mut ed = LineEditor::new();
    ed.insert_char('a');
    ed.insert_char('b');
    ed.insert_char('c');
    assert_eq!(ed.buffer(), "abc");
    assert_eq!(ed.cursor(), 3);
}

#[test]
fn test_insert_char_at_middle() {
    let mut ed = LineEditor::new();
    ed.insert_char('a');
    ed.insert_char('c');
    ed.move_cursor_left();
    ed.insert_char('b');
    assert_eq!(ed.buffer(), "abc");
    assert_eq!(ed.cursor(), 2);
}

#[test]
fn test_delete_char_backspace() {
    let mut ed = LineEditor::new();
    ed.insert_char('a');
    ed.insert_char('b');
    ed.backspace();
    assert_eq!(ed.buffer(), "a");
    assert_eq!(ed.cursor(), 1);
}

#[test]
fn test_backspace_at_start_does_nothing() {
    let mut ed = LineEditor::new();
    ed.backspace();
    assert_eq!(ed.buffer(), "");
    assert_eq!(ed.cursor(), 0);
}

#[test]
fn test_delete_at_cursor() {
    let mut ed = LineEditor::new();
    ed.insert_char('a');
    ed.insert_char('b');
    ed.insert_char('c');
    ed.move_cursor_left();
    ed.delete();
    assert_eq!(ed.buffer(), "ab");
    assert_eq!(ed.cursor(), 2);
}

#[test]
fn test_delete_at_end_does_nothing() {
    let mut ed = LineEditor::new();
    ed.insert_char('a');
    ed.delete();
    assert_eq!(ed.buffer(), "a");
    assert_eq!(ed.cursor(), 1);
}

#[test]
fn test_move_cursor_left() {
    let mut ed = LineEditor::new();
    ed.insert_char('a');
    ed.insert_char('b');
    ed.move_cursor_left();
    assert_eq!(ed.cursor(), 1);
}

#[test]
fn test_move_cursor_left_at_start_does_nothing() {
    let mut ed = LineEditor::new();
    ed.move_cursor_left();
    assert_eq!(ed.cursor(), 0);
}

#[test]
fn test_move_cursor_right() {
    let mut ed = LineEditor::new();
    ed.insert_char('a');
    ed.insert_char('b');
    ed.move_cursor_left();
    ed.move_cursor_left();
    ed.move_cursor_right();
    assert_eq!(ed.cursor(), 1);
}

#[test]
fn test_move_cursor_right_at_end_does_nothing() {
    let mut ed = LineEditor::new();
    ed.insert_char('a');
    ed.move_cursor_right();
    assert_eq!(ed.cursor(), 1);
}

#[test]
fn test_move_to_start() {
    let mut ed = LineEditor::new();
    ed.insert_char('a');
    ed.insert_char('b');
    ed.insert_char('c');
    ed.move_to_start();
    assert_eq!(ed.cursor(), 0);
}

#[test]
fn test_move_to_end() {
    let mut ed = LineEditor::new();
    ed.insert_char('a');
    ed.insert_char('b');
    ed.insert_char('c');
    ed.move_to_start();
    ed.move_to_end();
    assert_eq!(ed.cursor(), 3);
}

#[test]
fn test_clear_buffer() {
    let mut ed = LineEditor::new();
    ed.insert_char('a');
    ed.insert_char('b');
    ed.clear();
    assert_eq!(ed.buffer(), "");
    assert_eq!(ed.cursor(), 0);
}

#[test]
fn test_is_empty() {
    let mut ed = LineEditor::new();
    assert!(ed.is_empty());
    ed.insert_char('a');
    assert!(!ed.is_empty());
}

#[test]
fn test_to_string() {
    let mut ed = LineEditor::new();
    ed.insert_char('h');
    ed.insert_char('i');
    assert_eq!(ed.to_string(), "hi");
}

#[test]
fn test_backspace_in_middle() {
    let mut ed = LineEditor::new();
    ed.insert_char('a');
    ed.insert_char('b');
    ed.insert_char('c');
    ed.move_cursor_left();
    ed.backspace();
    assert_eq!(ed.buffer(), "ac");
    assert_eq!(ed.cursor(), 1);
}

// ── Prompt expansion tests ──────────────────────────────────────────────────

#[test]
fn test_prompt_default_ps1() {
    let mut env = ShellEnv::new("kish", vec![]);
    let _ = env.vars.unset("PS1");
    let prompt = expand_prompt(&mut env, "PS1");
    assert_eq!(prompt, "$ ");
}

#[test]
fn test_prompt_default_ps2() {
    let mut env = ShellEnv::new("kish", vec![]);
    let _ = env.vars.unset("PS2");
    let prompt = expand_prompt(&mut env, "PS2");
    assert_eq!(prompt, "> ");
}

#[test]
fn test_prompt_custom_ps1() {
    let mut env = ShellEnv::new("kish", vec![]);
    env.vars.set("PS1", "myshell> ").unwrap();
    let prompt = expand_prompt(&mut env, "PS1");
    assert_eq!(prompt, "myshell> ");
}

#[test]
fn test_prompt_with_variable_expansion() {
    let mut env = ShellEnv::new("kish", vec![]);
    env.vars.set("MYVAR", "hello").unwrap();
    env.vars.set("PS1", "${MYVAR}$ ").unwrap();
    let prompt = expand_prompt(&mut env, "PS1");
    assert_eq!(prompt, "hello$ ");
}

#[test]
fn test_prompt_empty_string() {
    let mut env = ShellEnv::new("kish", vec![]);
    env.vars.set("PS1", "").unwrap();
    let prompt = expand_prompt(&mut env, "PS1");
    assert_eq!(prompt, "");
}

// ── Parse status classification tests ──────────────────────────────────────

#[test]
fn test_classify_complete_command() {
    let aliases = AliasStore::default();
    match classify_parse("echo hello\n", &aliases) {
        ParseStatus::Complete(_) => {}
        other => panic!("expected Complete, got {:?}", other),
    }
}

#[test]
fn test_classify_empty_input() {
    let aliases = AliasStore::default();
    match classify_parse("\n", &aliases) {
        ParseStatus::Empty => {}
        other => panic!("expected Empty, got {:?}", other),
    }
}

#[test]
fn test_classify_incomplete_if() {
    let aliases = AliasStore::default();
    match classify_parse("if true; then\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_incomplete_while() {
    let aliases = AliasStore::default();
    match classify_parse("while true; do\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_incomplete_single_quote() {
    let aliases = AliasStore::default();
    match classify_parse("echo 'hello\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_incomplete_double_quote() {
    let aliases = AliasStore::default();
    match classify_parse("echo \"hello\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_incomplete_backslash_newline() {
    let aliases = AliasStore::default();
    match classify_parse("echo hello \\\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_incomplete_pipe() {
    let aliases = AliasStore::default();
    match classify_parse("echo hello |\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_incomplete_and_or() {
    let aliases = AliasStore::default();
    match classify_parse("true &&\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_error() {
    let aliases = AliasStore::default();
    match classify_parse("echo hello >>\n", &aliases) {
        ParseStatus::Error(_) => {}
        other => panic!("expected Error, got {:?}", other),
    }
}

#[test]
fn test_classify_multiple_commands() {
    let aliases = AliasStore::default();
    match classify_parse("echo a; echo b\n", &aliases) {
        ParseStatus::Complete(_) => {}
        other => panic!("expected Complete, got {:?}", other),
    }
}

// ── MockTerminal-based LineEditor tests ─────────────────────────────────

#[test]
fn test_mock_basic_input() {
    let mut events = chars("hello");
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("hello".to_string()));
}

#[test]
fn test_mock_ctrl_c_returns_empty() {
    let events = vec![
        key(KeyCode::Char('a')),
        key(KeyCode::Char('b')),
        ctrl('c'),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some(String::new()));
}

#[test]
fn test_mock_ctrl_d_empty_returns_none() {
    let events = vec![ctrl('d')];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, None);
}

#[test]
fn test_mock_ctrl_d_nonempty_deletes_char() {
    // Type "ab", move left, Ctrl+D deletes 'b', Enter submits "a"
    let events = vec![
        key(KeyCode::Char('a')),
        key(KeyCode::Char('b')),
        key(KeyCode::Left),
        ctrl('d'),
        key(KeyCode::Enter),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("a".to_string()));
}

#[test]
fn test_mock_ctrl_a_and_ctrl_e() {
    // Type "abc", Ctrl+A (start), type "x", Ctrl+E (end), type "y"
    let events = vec![
        key(KeyCode::Char('a')),
        key(KeyCode::Char('b')),
        key(KeyCode::Char('c')),
        ctrl('a'),
        key(KeyCode::Char('x')),
        ctrl('e'),
        key(KeyCode::Char('y')),
        key(KeyCode::Enter),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("xabcy".to_string()));
}

#[test]
fn test_mock_ctrl_b_and_ctrl_f() {
    // Type "abc", Ctrl+B twice (back to pos 1), type "x", Ctrl+F (forward), type "y"
    let events = vec![
        key(KeyCode::Char('a')),
        key(KeyCode::Char('b')),
        key(KeyCode::Char('c')),
        ctrl('b'),
        ctrl('b'),
        key(KeyCode::Char('x')),
        ctrl('f'),
        key(KeyCode::Char('y')),
        key(KeyCode::Enter),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("axbyc".to_string()));
}

#[test]
fn test_mock_home_end_keys() {
    // Type "abc", Home, type "x", End, type "y"
    let events = vec![
        key(KeyCode::Char('a')),
        key(KeyCode::Char('b')),
        key(KeyCode::Char('c')),
        key(KeyCode::Home),
        key(KeyCode::Char('x')),
        key(KeyCode::End),
        key(KeyCode::Char('y')),
        key(KeyCode::Enter),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("xabcy".to_string()));
}

#[test]
fn test_mock_backspace() {
    // Type "abc", Backspace twice, Enter
    let events = vec![
        key(KeyCode::Char('a')),
        key(KeyCode::Char('b')),
        key(KeyCode::Char('c')),
        key(KeyCode::Backspace),
        key(KeyCode::Backspace),
        key(KeyCode::Enter),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("a".to_string()));
}

#[test]
fn test_mock_delete_key() {
    // Type "abc", Home, Delete, Enter -> "bc"
    let events = vec![
        key(KeyCode::Char('a')),
        key(KeyCode::Char('b')),
        key(KeyCode::Char('c')),
        key(KeyCode::Home),
        key(KeyCode::Delete),
        key(KeyCode::Enter),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("bc".to_string()));
}

#[test]
fn test_mock_history_up_down() {
    let mut history = History::new();
    history.add("first", 500, "");
    history.add("second", 500, "");

    // Up (second), Up (first), Down (second), Enter
    let events = vec![
        key(KeyCode::Up),
        key(KeyCode::Up),
        key(KeyCode::Down),
        key(KeyCode::Enter),
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("second".to_string()));
}

#[test]
fn test_mock_history_up_and_edit() {
    let mut history = History::new();
    history.add("echo old", 500, "");

    // Up (recall "echo old"), Backspace x3 (remove "old"), type "new", Enter
    let mut events = vec![key(KeyCode::Up)];
    events.extend(vec![key(KeyCode::Backspace); 3]);
    events.extend(chars("new"));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("echo new".to_string()));
}

#[test]
fn test_mock_history_preserves_typed_text() {
    let mut history = History::new();
    history.add("old", 500, "");

    // Type "partial", Up (recall "old"), Down (back to "partial"), Enter
    let mut events = chars("partial");
    events.push(key(KeyCode::Up));
    events.push(key(KeyCode::Down));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("partial".to_string()));
}

// ── Ctrl+R fuzzy search tests ───────────────────────────────────────────

#[test]
fn test_mock_ctrl_r_selects_matching_entry() {
    let mut history = History::new();
    history.add("ls -la", 500, "");
    history.add("git commit -m 'fix'", 500, "");
    history.add("cargo test", 500, "");

    // Ctrl+R -> type "git" -> Enter (select) -> Enter (submit)
    let mut events = vec![ctrl('r')];
    events.extend(chars("git"));
    events.push(key(KeyCode::Enter)); // select from search
    events.push(key(KeyCode::Enter)); // submit in line editor

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("git commit -m 'fix'".to_string()));
}

#[test]
fn test_mock_ctrl_r_cancel_with_esc() {
    let mut history = History::new();
    history.add("ls -la", 500, "");
    history.add("git commit", 500, "");

    // Type "hello", Ctrl+R -> type "git" -> Esc (cancel) -> Enter (submit "hello")
    let mut events = chars("hello");
    events.push(ctrl('r'));
    events.extend(chars("git"));
    events.push(key(KeyCode::Esc)); // cancel search
    events.push(key(KeyCode::Enter)); // submit whatever is in buffer

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    // After Esc, buffer should retain pre-search content "hello"
    assert_eq!(result, Some("hello".to_string()));
}

#[test]
fn test_mock_ctrl_r_navigate_up() {
    let mut history = History::new();
    history.add("echo first", 500, "");
    history.add("echo second", 500, "");
    history.add("echo third", 500, "");

    // Ctrl+R (no query, all entries shown, newest first: third=0, second=1, first=2)
    // Up moves selection from index 0 to 1 (second)
    // Enter selects "echo second"
    let events = vec![
        ctrl('r'),
        key(KeyCode::Up),     // select "echo second" (index 1)
        key(KeyCode::Enter),  // select from search
        key(KeyCode::Enter),  // submit
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("echo second".to_string()));
}

#[test]
fn test_mock_ctrl_r_backspace_updates_candidates() {
    let mut history = History::new();
    history.add("git log", 500, "");
    history.add("cargo test", 500, "");

    // Ctrl+R -> type "gi" -> Backspace x2 (clear) -> type "ca" -> Enter (selects "cargo test")
    let events = vec![
        ctrl('r'),
        key(KeyCode::Char('g')),
        key(KeyCode::Char('i')),
        key(KeyCode::Backspace),
        key(KeyCode::Backspace),
        key(KeyCode::Char('c')),
        key(KeyCode::Char('a')),
        key(KeyCode::Enter),  // select from search
        key(KeyCode::Enter),  // submit
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    assert_eq!(result, Some("cargo test".to_string()));
}

#[test]
fn test_mock_fuzzy_search_direct_select() {
    // Test FuzzySearchUI::run directly (not through LineEditor)
    let mut history = History::new();
    history.add("ls -la", 500, "");
    history.add("git status", 500, "");
    history.add("cargo build", 500, "");

    // Type "sta" -> Enter (selects "git status" as best match)
    let mut events = chars("sta");
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let result = FuzzySearchUI::run(&history, &mut term).unwrap();
    assert_eq!(result, Some("git status".to_string()));
}

#[test]
fn test_mock_fuzzy_search_direct_cancel() {
    let mut history = History::new();
    history.add("ls -la", 500, "");

    let events = vec![key(KeyCode::Esc)];

    let mut term = MockTerminal::new(events);
    let result = FuzzySearchUI::run(&history, &mut term).unwrap();
    assert_eq!(result, None);
}

#[test]
fn test_mock_fuzzy_search_empty_history() {
    let history = History::new();
    let mut term = MockTerminal::new(vec![]);
    let result = FuzzySearchUI::run(&history, &mut term).unwrap();
    assert_eq!(result, None);
}

#[test]
fn test_mock_ctrl_r_with_ctrl_g_cancel() {
    let mut history = History::new();
    history.add("some command", 500, "");

    // Ctrl+R -> Ctrl+G (cancel) -> Enter (submit empty)
    let events = vec![
        ctrl('r'),
        ctrl('g'),            // cancel search
        key(KeyCode::Enter),  // submit
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor.read_line(2, &mut history, &mut term).unwrap();
    // Buffer is empty since Ctrl+R was triggered from empty state and cancelled
    assert_eq!(result, Some(String::new()));
}

#[test]
fn test_fuzzy_search_arrow_keys_no_cursor_drift() {
    // Regression: pressing ↑/↓ in Ctrl+R caused the UI to drift up by one
    // line per redraw because draw() used move_up(max_visible + 2) instead of
    // move_up(max_visible + 1).
    let mut history = History::new();
    history.add("echo first", 500, "");
    history.add("echo second", 500, "");
    history.add("echo third", 500, "");
    history.add("echo fourth", 500, "");
    history.add("echo fifth", 500, "");

    // Navigate up 3 times, down 2 times, then cancel.
    // Each arrow key triggers a draw() call; with the old bug the cursor would
    // drift by -(N+1) rows where N = number of Continue events.
    let events = vec![
        key(KeyCode::Up),
        key(KeyCode::Up),
        key(KeyCode::Up),
        key(KeyCode::Down),
        key(KeyCode::Down),
        key(KeyCode::Esc), // cancel
    ];

    let mut term = MockTerminal::new(events);
    let _ = FuzzySearchUI::run(&history, &mut term).unwrap();

    // After run() completes the cursor must be back at its starting row.
    assert_eq!(
        term.cursor_row(),
        0,
        "cursor drifted {} rows from origin after ↑↓ navigation in fuzzy search",
        term.cursor_row()
    );
}

#[test]
fn test_fuzzy_search_select_no_cursor_drift() {
    // Same check but exiting via Enter (Select) instead of Esc (Cancel).
    let mut history = History::new();
    history.add("echo first", 500, "");
    history.add("echo second", 500, "");
    history.add("echo third", 500, "");

    let events = vec![
        key(KeyCode::Up),    // select "echo second"
        key(KeyCode::Up),    // select "echo first"
        key(KeyCode::Down),  // back to "echo second"
        key(KeyCode::Enter), // select
    ];

    let mut term = MockTerminal::new(events);
    let result = FuzzySearchUI::run(&history, &mut term).unwrap();
    assert_eq!(result, Some("echo second".to_string()));
    assert_eq!(
        term.cursor_row(),
        0,
        "cursor drifted {} rows from origin after selection in fuzzy search",
        term.cursor_row()
    );
}
