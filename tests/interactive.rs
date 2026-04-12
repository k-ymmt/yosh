use crossterm::event::KeyCode;
use kish::env::ShellEnv;
use kish::env::aliases::AliasStore;
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
