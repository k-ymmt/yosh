use crossterm::event::KeyCode;
use std::fs;
use yosh::env::ShellEnv;
use yosh::env::aliases::AliasStore;
use yosh::interactive::command_completion::{CommandCompleter, CommandCompletionContext};
use yosh::interactive::completion::CompletionContext;
use yosh::interactive::edit_action::EditAction;
use yosh::interactive::fuzzy_search::FuzzySearchUI;
use yosh::interactive::highlight::{CheckerEnv, HighlightScanner};
use yosh::interactive::history::History;
use yosh::interactive::keymap::{BufferState, Keymap};
use yosh::interactive::line_editor::LineEditor;
use yosh::interactive::parse_status::{ParseStatus, classify_parse};
use yosh::interactive::prompt::expand_prompt;

mod helpers;
use helpers::grid_terminal::GridTerminal;
use helpers::mock_terminal::{MockTerminal, alt, chars, ctrl, key};

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
    let mut env = ShellEnv::new("yosh", vec![]);
    let _ = env.vars.unset("PS1");
    let prompt = expand_prompt(&mut env, "PS1");
    assert_eq!(prompt, "$ ");
}

#[test]
fn test_prompt_default_ps2() {
    let mut env = ShellEnv::new("yosh", vec![]);
    let _ = env.vars.unset("PS2");
    let prompt = expand_prompt(&mut env, "PS2");
    assert_eq!(prompt, "> ");
}

#[test]
fn test_prompt_custom_ps1() {
    let mut env = ShellEnv::new("yosh", vec![]);
    env.vars.set("PS1", "myshell> ").unwrap();
    let prompt = expand_prompt(&mut env, "PS1");
    assert_eq!(prompt, "myshell> ");
}

#[test]
fn test_prompt_with_variable_expansion() {
    let mut env = ShellEnv::new("yosh", vec![]);
    env.vars.set("MYVAR", "hello").unwrap();
    env.vars.set("PS1", "${MYVAR}$ ").unwrap();
    let prompt = expand_prompt(&mut env, "PS1");
    assert_eq!(prompt, "hello$ ");
}

#[test]
fn test_prompt_empty_string() {
    let mut env = ShellEnv::new("yosh", vec![]);
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
fn test_classify_incomplete_for() {
    // Verifies the `\n:\ndone\n` probe path via for-loop header-only input.
    let aliases = AliasStore::default();
    match classify_parse("for x in 1\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_incomplete_brace_group() {
    // Verifies the `\n:\n}\n` probe path via open-brace-only input.
    let aliases = AliasStore::default();
    match classify_parse("{ true\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_does_not_hang_on_dsemi_garbage() {
    // Regression guard: the `\n:\n;;\nesac\n` probe candidate
    // "if true; then\n\n;;\nesac\n" used to cause parse_compound_list
    // to loop forever. With the parse_simple_command empty-result
    // guard, classify_parse must return in finite time.
    let aliases = AliasStore::default();
    let _ = classify_parse("if ;;\n", &aliases);
    // Test passes as long as it returns (no assertion on specific
    // classification; correctness of the specific variant is covered
    // by parser-level tests).
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
fn test_classify_trailing_pipe_inside_comment_is_complete() {
    // `echo hi #|` — the `|` is comment text; classifying it Incomplete
    // used to trap Enter forever in the multiline editor.
    let aliases = AliasStore::default();
    match classify_parse("echo hi #|\n", &aliases) {
        ParseStatus::Complete(_) => {}
        other => panic!("expected Complete, got {:?}", other),
    }
}

#[test]
fn test_classify_trailing_and_and_inside_comment_is_complete() {
    let aliases = AliasStore::default();
    match classify_parse("echo hi # foo &&\n", &aliases) {
        ParseStatus::Complete(_) => {}
        other => panic!("expected Complete, got {:?}", other),
    }
}

#[test]
fn test_classify_real_trailing_pipe_before_comment_incomplete() {
    // A real trailing pipe followed by a comment is still Incomplete.
    let aliases = AliasStore::default();
    match classify_parse("echo hi | # comment\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_hash_in_quotes_not_a_comment() {
    let aliases = AliasStore::default();
    match classify_parse("echo '#|'\n", &aliases) {
        ParseStatus::Complete(_) => {}
        other => panic!("expected Complete, got {:?}", other),
    }
}

#[test]
fn test_classify_nested_header_only_constructs_incomplete() {
    // Closing-keyword probes must compose: `while true\nif true\n` needs
    // both `then :\nfi` and `do :\ndone` appended before it parses.
    let aliases = AliasStore::default();
    match classify_parse("while true\nif true\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_triple_nested_header_only_incomplete() {
    let aliases = AliasStore::default();
    match classify_parse("while true\nwhile true\nif true\n", &aliases) {
        ParseStatus::Incomplete => {}
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

#[test]
fn test_classify_invalid_input_still_error() {
    // Probe composition must not reclassify genuinely invalid input.
    let aliases = AliasStore::default();
    match classify_parse("if then\n", &aliases) {
        ParseStatus::Error(_) => {}
        other => panic!("expected Error, got {:?}", other),
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
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
    assert_eq!(result, Some("hello".to_string()));
}

#[test]
fn test_mock_ctrl_c_returns_empty() {
    let events = vec![key(KeyCode::Char('a')), key(KeyCode::Char('b')), ctrl('c')];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
    assert_eq!(result, Some(String::new()));
}

#[test]
fn test_mock_ctrl_d_empty_returns_none() {
    let events = vec![ctrl('d')];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
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
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
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
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
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
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
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
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
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
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
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
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
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
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
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
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
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
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
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
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
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
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
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
        key(KeyCode::Up),    // select "echo second" (index 1)
        key(KeyCode::Enter), // select from search
        key(KeyCode::Enter), // submit
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
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
        key(KeyCode::Enter), // select from search
        key(KeyCode::Enter), // submit
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
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
        ctrl('g'),           // cancel search
        key(KeyCode::Enter), // submit
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
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

// ── Autosuggestion tests ────────────────────────────────────────────────

#[test]
fn test_suggest_accept_full_with_right_arrow() {
    let mut history = History::new();
    history.add("git commit -m 'fix'", 500, "");

    // Type "git c", then Right (accept suggestion), then Enter
    let mut events = chars("git c");
    events.push(key(KeyCode::Right));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
    assert_eq!(result, Some("git commit -m 'fix'".to_string()));
}

#[test]
fn test_suggest_accept_full_with_ctrl_f() {
    let mut history = History::new();
    history.add("cargo test --release", 500, "");

    // Type "cargo t", then Ctrl+F (accept suggestion), then Enter
    let mut events = chars("cargo t");
    events.push(ctrl('f'));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
    assert_eq!(result, Some("cargo test --release".to_string()));
}

#[test]
fn test_right_arrow_normal_when_no_suggestion() {
    let mut history = History::new();
    history.add("git commit", 500, "");

    // Type "abc", Left, Right (normal cursor move), Enter
    let mut events = chars("abc");
    events.push(key(KeyCode::Left));
    events.push(key(KeyCode::Right));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
    assert_eq!(result, Some("abc".to_string()));
}

#[test]
fn test_suggest_appears_on_typing() {
    let mut history = History::new();
    history.add("git commit -m 'fix'", 500, "");

    // Type "git c" then Enter
    let mut events = chars("git c");
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
    assert_eq!(result, Some("git c".to_string()));

    // Check that dim suggestion text was rendered
    let output = term.output().join("");
    assert!(
        output.contains("[DIM]"),
        "suggestion should trigger dim rendering"
    );
    assert!(
        output.contains("ommit -m 'fix'"),
        "suggestion text should appear in output"
    );
}

#[test]
fn test_suggest_hidden_when_cursor_not_at_end() {
    let mut history = History::new();
    history.add("echo hello world", 500, "");

    // Type "echo h", Left (cursor no longer at end), then Enter
    let mut events = chars("echo h");
    events.push(key(KeyCode::Left));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let _ = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();

    let output_parts = term.output();
    let last_outputs = output_parts.iter().rev().take(10).collect::<Vec<_>>();
    let last_chunk: String = last_outputs.iter().rev().map(|s| s.as_str()).collect();
    let last_dim_pos = last_chunk.rfind("[DIM]");
    let last_nodim_pos = last_chunk.rfind("[/DIM]");
    match (last_dim_pos, last_nodim_pos) {
        (Some(d), Some(nd)) => assert!(
            d < nd,
            "suggestion should not be active after cursor moved left"
        ),
        (None, _) => {}
        (Some(_), None) => panic!("unclosed [DIM] in output"),
    }
}

#[test]
fn test_suggest_cleared_on_history_navigation() {
    let mut history = History::new();
    history.add("echo hello", 500, "");
    history.add("echo world", 500, "");

    // Type "echo " (suggestion active), then Up (history nav clears suggestion), Enter
    let mut events = chars("echo ");
    events.push(key(KeyCode::Up));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
    // Up replaces buffer with "echo world" (most recent)
    assert_eq!(result, Some("echo world".to_string()));
}

#[test]
fn test_suggest_accept_word_with_alt_f() {
    let mut history = History::new();
    history.add("git commit -m 'fix'", 500, "");

    // Type "git", Alt+F (accept " commit"), then Enter
    let mut events = chars("git");
    events.push(alt('f'));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
    assert_eq!(result, Some("git commit".to_string()));
}

#[test]
fn test_suggest_accept_word_stepwise() {
    let mut history = History::new();
    history.add("git commit -m 'fix'", 500, "");

    // Type "git", Alt+F three times (accept " commit", " -m", " 'fix'"), then Enter
    let mut events = chars("git");
    events.push(alt('f')); // accept " commit"
    events.push(alt('f')); // accept " -m"
    events.push(alt('f')); // accept " 'fix'"
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
    assert_eq!(result, Some("git commit -m 'fix'".to_string()));
}

#[test]
fn test_alt_f_noop_without_suggestion() {
    let mut history = History::new(); // empty history, no suggestions

    // Type "hello", Alt+F (no-op), Enter
    let mut events = chars("hello");
    events.push(alt('f'));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
    assert_eq!(result, Some("hello".to_string()));
}

#[test]
fn test_ctrl_r_redraws_prompt_after_selection() {
    // After selecting from Ctrl+R, the prompt (PS1) must be redrawn because
    // the fuzzy search UI overwrites it.
    let mut history = History::new();
    history.add("echo hello", 500, "");

    let events = vec![
        ctrl('r'),
        key(KeyCode::Enter), // select "echo hello"
        key(KeyCode::Enter), // submit
    ];

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor
        .read_line("mysh$ ", &[], &mut history, &mut term)
        .unwrap();
    assert_eq!(result, Some("echo hello".to_string()));

    // The prompt "mysh$ " must appear in the terminal output after
    // the fuzzy search UI was cleared.
    let output = term.output().join("");
    // Find the LAST occurrence of the prompt — that's the redraw after Ctrl+R.
    assert!(
        output.contains("mysh$ "),
        "prompt was not redrawn after Ctrl+R selection"
    );
}

#[test]
fn test_suggest_updates_on_backspace() {
    let mut history = History::new();
    history.add("echo hello", 500, "");
    history.add("echo world", 500, "");

    // Type "echo w" (suggests "orld"), Backspace (now "echo " suggests "world"),
    // Right (accept "world"), Enter
    let mut events = chars("echo w");
    events.push(key(KeyCode::Backspace));
    events.push(key(KeyCode::Right)); // accept "world" (most recent match for "echo ")
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let result = editor
        .read_line("$ ", &[], &mut history, &mut term)
        .unwrap();
    assert_eq!(result, Some("echo world".to_string()));
}

// ── Tab completion tests ───────────────────────────────────────────────

#[test]
fn test_tab_completes_single_candidate() {
    let tmp = tempfile::TempDir::new().unwrap();
    fs::File::create(tmp.path().join("unique_file.txt")).unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/home/user".to_string(),
        show_dotfiles: false,
    };

    // Type "ls uni" + Tab + Enter (argument position ensures path completion)
    let mut events = chars("ls uni");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let aliases = AliasStore::default();
    let mut command_completer = CommandCompleter::new();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let mut spec_store = yosh::interactive::spec_completion::SpecStore::new(
        std::path::PathBuf::from("/nonexistent"),
    );
    let result = editor
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
            &|_| false,
        )
        .unwrap();
    assert_eq!(result, Some("ls unique_file.txt ".to_string()));
}

#[test]
fn test_tab_complete_quoted_word_preserves_quote() {
    // Regression: the reconstructed replacement dropped the opening
    // quote of the completion word, so `cd "/…/My D<Tab>` replaced the
    // quoted word with an unquoted space-containing path (a broken
    // argument). The quote must survive, and a completed filename gets
    // a bash-like closing quote before the trailing space.
    let tmp = tempfile::TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("My Dir")).unwrap();
    fs::File::create(tmp.path().join("My Dir").join("notes.txt")).unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/home/user".to_string(),
        show_dotfiles: false,
    };

    // Type `cat "My Dir/no` + Tab + Enter (argument position → path
    // completion; the quoted space stays inside one word).
    let mut events = chars("cat \"My Dir/no");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let aliases = AliasStore::default();
    let mut command_completer = CommandCompleter::new();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let mut spec_store = yosh::interactive::spec_completion::SpecStore::new(
        std::path::PathBuf::from("/nonexistent"),
    );
    let result = editor
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
            &|_| false,
        )
        .unwrap();
    assert_eq!(result, Some("cat \"My Dir/notes.txt\" ".to_string()));
}

#[test]
fn test_tab_complete_with_multibyte_word_does_not_panic() {
    // Regression: handle_tab_complete passed the CHAR-index cursor
    // (`self.pos`) to the byte-indexed completion APIs. With a multibyte
    // word before the cursor the byte slice landed mid-character and
    // panicked ("byte index N is not a char boundary"). The cursor must
    // be converted to a byte offset first.
    let tmp = tempfile::TempDir::new().unwrap();
    fs::File::create(tmp.path().join("日本語ファイル.txt")).unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/home/user".to_string(),
        show_dotfiles: false,
    };

    // Type "ls 日本語" + Tab + Enter (argument position → path completion).
    let mut events = chars("ls 日本語");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let aliases = AliasStore::default();
    let mut command_completer = CommandCompleter::new();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let mut spec_store = yosh::interactive::spec_completion::SpecStore::new(
        std::path::PathBuf::from("/nonexistent"),
    );
    let result = editor
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
            &|_| false,
        )
        .unwrap();
    assert_eq!(result, Some("ls 日本語ファイル.txt ".to_string()));
}

#[test]
fn test_tab_completes_common_prefix() {
    let tmp = tempfile::TempDir::new().unwrap();
    fs::File::create(tmp.path().join("file_alpha.rs")).unwrap();
    fs::File::create(tmp.path().join("file_beta.rs")).unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/home/user".to_string(),
        show_dotfiles: false,
    };

    // Type "ls file" + Tab + Enter (argument position ensures path completion)
    let mut events = chars("ls file");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let aliases = AliasStore::default();
    let mut command_completer = CommandCompleter::new();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let mut spec_store = yosh::interactive::spec_completion::SpecStore::new(
        std::path::PathBuf::from("/nonexistent"),
    );
    let result = editor
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
            &|_| false,
        )
        .unwrap();
    assert_eq!(result, Some("ls file_".to_string()));
}

#[test]
fn test_tab_directory_appends_slash() {
    let tmp = tempfile::TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("mydir")).unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/home/user".to_string(),
        show_dotfiles: false,
    };

    // Type "ls my" + Tab + Enter (argument position ensures path completion)
    let mut events = chars("ls my");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let aliases = AliasStore::default();
    let mut command_completer = CommandCompleter::new();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let mut spec_store = yosh::interactive::spec_completion::SpecStore::new(
        std::path::PathBuf::from("/nonexistent"),
    );
    let result = editor
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
            &|_| false,
        )
        .unwrap();
    assert_eq!(result, Some("ls mydir/".to_string()));
}

#[test]
fn test_tab_no_match_does_nothing() {
    let tmp = tempfile::TempDir::new().unwrap();
    fs::File::create(tmp.path().join("abc.txt")).unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/home/user".to_string(),
        show_dotfiles: false,
    };

    // Type "ls xyz" + Tab + Enter (argument position ensures path completion)
    let mut events = chars("ls xyz");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let aliases = AliasStore::default();
    let mut command_completer = CommandCompleter::new();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let mut spec_store = yosh::interactive::spec_completion::SpecStore::new(
        std::path::PathBuf::from("/nonexistent"),
    );
    let result = editor
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
            &|_| false,
        )
        .unwrap();
    assert_eq!(result, Some("ls xyz".to_string()));
}

#[test]
fn test_double_tab_opens_completion_ui() {
    let dir = std::env::temp_dir().join(format!("yosh-tab-test5-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("file_alpha.rs"), "").unwrap();
    fs::write(dir.join("file_beta.rs"), "").unwrap();

    let ctx = yosh::interactive::completion::CompletionContext {
        cwd: dir.to_str().unwrap().to_string(),
        home: "/tmp".to_string(),
        show_dotfiles: false,
    };

    // Type "ls file_", Tab (common prefix already complete, no change),
    // Tab again (opens CompletionUI), Up (select file_beta.rs), Enter (confirm), Enter (submit)
    // Using "ls " prefix puts "file_" in argument position to ensure path completion
    let mut events = chars("ls file_");
    events.push(key(KeyCode::Tab)); // first tab: completes common prefix (already "file_")
    events.push(key(KeyCode::Tab)); // second tab: opens CompletionUI
    events.push(key(KeyCode::Up)); // select file_beta.rs
    events.push(key(KeyCode::Enter)); // confirm selection in UI
    events.push(key(KeyCode::Enter)); // submit line

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let aliases = AliasStore::default();
    let mut command_completer = CommandCompleter::new();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let mut spec_store = yosh::interactive::spec_completion::SpecStore::new(
        std::path::PathBuf::from("/nonexistent"),
    );
    let result = editor
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
            &|_| false,
        )
        .unwrap();
    assert_eq!(result, Some("ls file_beta.rs ".to_string()));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_tab_command_completion_at_line_start() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Create an executable in a temp PATH directory
    let bin_dir = tempfile::TempDir::new().unwrap();
    let cmd_path = bin_dir.path().join("yosh_test_mycmd");
    fs::File::create(&cmd_path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&cmd_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/tmp".to_string(),
        show_dotfiles: false,
    };

    let mut command_completer = CommandCompleter::new();
    let aliases = AliasStore::default();
    let path_str = bin_dir.path().to_str().unwrap().to_string();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: &path_str,
        builtins: &[],
        aliases: &aliases,
    };

    // Type "yosh_test_my" + Tab + Enter — should complete to "yosh_test_mycmd "
    let mut events = chars("yosh_test_my");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let mut spec_store = yosh::interactive::spec_completion::SpecStore::new(
        std::path::PathBuf::from("/nonexistent"),
    );
    let result = editor
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
            &|_| false,
        )
        .unwrap();
    assert_eq!(result, Some("yosh_test_mycmd ".to_string()));
}

#[test]
fn test_tab_command_position_path_fallback() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Create a file in cwd starting with "./"
    fs::File::create(tmp.path().join("myscript.sh")).unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/tmp".to_string(),
        show_dotfiles: false,
    };

    let mut command_completer = CommandCompleter::new();
    let aliases = AliasStore::default();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };

    // Type "./my" + Tab + Enter — should fall back to path completion
    let mut events = chars("./my");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let mut spec_store = yosh::interactive::spec_completion::SpecStore::new(
        std::path::PathBuf::from("/nonexistent"),
    );
    let result = editor
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
            &|_| false,
        )
        .unwrap();
    assert_eq!(result, Some("./myscript.sh ".to_string()));
}

#[test]
fn test_tab_argument_position_uses_path_completion() {
    let tmp = tempfile::TempDir::new().unwrap();
    fs::File::create(tmp.path().join("testfile.txt")).unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/tmp".to_string(),
        show_dotfiles: false,
    };

    let mut command_completer = CommandCompleter::new();
    let aliases = AliasStore::default();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };

    // Type "cat test" + Tab + Enter — argument position should use path completion
    let mut events = chars("cat test");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let mut spec_store = yosh::interactive::spec_completion::SpecStore::new(
        std::path::PathBuf::from("/nonexistent"),
    );
    let result = editor
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
            &|_| false,
        )
        .unwrap();
    assert_eq!(result, Some("cat testfile.txt ".to_string()));
}

#[test]
fn test_tab_completes_builtin() {
    let tmp = tempfile::TempDir::new().unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/tmp".to_string(),
        show_dotfiles: false,
    };

    let mut command_completer = CommandCompleter::new();
    let aliases = AliasStore::default();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &["export", "exec", "exit"],
        aliases: &aliases,
    };

    // Type "expo" + Tab + Enter — should complete to "export "
    let mut events = chars("expo");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let mut spec_store = yosh::interactive::spec_completion::SpecStore::new(
        std::path::PathBuf::from("/nonexistent"),
    );
    let result = editor
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
            &|_| false,
        )
        .unwrap();
    assert_eq!(result, Some("export ".to_string()));
}

#[test]
fn test_tab_spec_completion_subcommand_values() {
    use yosh::interactive::spec_completion::SpecStore;

    let tmp = tempfile::TempDir::new().unwrap();
    let spec_dir = tmp.path().join("completions");
    fs::create_dir_all(&spec_dir).unwrap();
    fs::write(
        spec_dir.join("mytool.toml"),
        "[[subcommands]]\nname = \"deploy\"\n\n[[subcommands.args]]\nvalues = [\"prod\", \"stage\"]\n",
    )
    .unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/home/user".to_string(),
        show_dotfiles: false,
    };

    // "mytool deploy pr" + Tab → unique candidate "prod" is inserted.
    let mut events = chars("mytool deploy pr");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let aliases = AliasStore::default();
    let mut command_completer = CommandCompleter::new();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };
    let mut spec_store = SpecStore::new(spec_dir);
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let result = editor
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
            &|_| false,
        )
        .unwrap();
    assert_eq!(result, Some("mytool deploy prod ".to_string()));
}

#[test]
fn test_tab_spec_none_source_suppresses_path_completion() {
    use yosh::interactive::spec_completion::SpecStore;

    let tmp = tempfile::TempDir::new().unwrap();
    // A file that plain path completion WOULD match.
    fs::File::create(tmp.path().join("unique_file.txt")).unwrap();
    let spec_dir = tmp.path().join("completions");
    fs::create_dir_all(&spec_dir).unwrap();
    fs::write(spec_dir.join("mytool.toml"), "[[args]]\ntype = \"none\"\n").unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/home/user".to_string(),
        show_dotfiles: false,
    };

    let mut events = chars("mytool uni");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let aliases = AliasStore::default();
    let mut command_completer = CommandCompleter::new();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };
    let mut spec_store = SpecStore::new(spec_dir);
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let result = editor
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
            &|_| false,
        )
        .unwrap();
    // Tab must NOT expand to unique_file.txt.
    assert_eq!(result, Some("mytool uni".to_string()));
}

#[test]
fn test_tab_no_spec_falls_back_to_path_completion() {
    use yosh::interactive::spec_completion::SpecStore;

    let tmp = tempfile::TempDir::new().unwrap();
    fs::File::create(tmp.path().join("unique_file.txt")).unwrap();

    let ctx = CompletionContext {
        cwd: tmp.path().to_str().unwrap().to_string(),
        home: "/home/user".to_string(),
        show_dotfiles: false,
    };

    let mut events = chars("ls uni");
    events.push(key(KeyCode::Tab));
    events.push(key(KeyCode::Enter));

    let mut term = MockTerminal::new(events);
    let mut editor = LineEditor::new();
    let mut history = History::new();
    let aliases = AliasStore::default();
    let mut command_completer = CommandCompleter::new();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };
    // Store pointing at a dir with no specs — behavior must be identical
    // to the pre-feature path completion.
    let mut spec_store = SpecStore::new(tmp.path().join("no_specs"));
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let result = editor
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
            &|_| false,
        )
        .unwrap();
    assert_eq!(result, Some("ls unique_file.txt ".to_string()));
}

// ── Kill ring tests ───────────────────────────────────────────────────

#[test]
fn test_kill_ring_kill_and_yank() {
    use yosh::interactive::kill_ring::KillRing;
    let mut kr = KillRing::new(60);
    kr.kill("hello", false);
    assert_eq!(kr.yank(), Some("hello"));
}

#[test]
fn test_kill_ring_multiple_kills() {
    use yosh::interactive::kill_ring::KillRing;
    let mut kr = KillRing::new(60);
    kr.kill("first", false);
    kr.kill("second", false);
    assert_eq!(kr.yank(), Some("second"));
}

#[test]
fn test_kill_ring_append_forward() {
    use yosh::interactive::kill_ring::KillRing;
    let mut kr = KillRing::new(60);
    kr.kill("hello", false);
    kr.kill(" world", true);
    assert_eq!(kr.yank(), Some("hello world"));
}

#[test]
fn test_kill_ring_yank_pop_cycles() {
    use yosh::interactive::kill_ring::KillRing;
    let mut kr = KillRing::new(60);
    kr.kill("first", false);
    kr.kill("second", false);
    kr.kill("third", false);
    assert_eq!(kr.yank(), Some("third"));
    assert_eq!(kr.yank_pop(), Some("second"));
    assert_eq!(kr.yank_pop(), Some("first"));
    // Wraps around
    assert_eq!(kr.yank_pop(), Some("third"));
}

#[test]
fn test_kill_ring_yank_empty() {
    use yosh::interactive::kill_ring::KillRing;
    let mut kr = KillRing::new(60);
    assert_eq!(kr.yank(), None);
}

#[test]
fn test_kill_ring_yank_pop_empty() {
    use yosh::interactive::kill_ring::KillRing;
    let mut kr = KillRing::new(60);
    assert_eq!(kr.yank_pop(), None);
}

#[test]
fn test_kill_ring_max_size() {
    use yosh::interactive::kill_ring::KillRing;
    let mut kr = KillRing::new(3);
    kr.kill("a", false);
    kr.kill("b", false);
    kr.kill("c", false);
    kr.kill("d", false);
    // "a" should have been evicted
    assert_eq!(kr.yank(), Some("d"));
    assert_eq!(kr.yank_pop(), Some("c"));
    assert_eq!(kr.yank_pop(), Some("b"));
    // Wraps: back to "d" (only 3 entries)
    assert_eq!(kr.yank_pop(), Some("d"));
}

#[test]
fn test_kill_ring_prepend() {
    use yosh::interactive::kill_ring::KillRing;
    let mut kr = KillRing::new(60);
    kr.kill("world", false);
    kr.prepend("hello ", true);
    assert_eq!(kr.yank(), Some("hello world"));
}

// ── Undo manager tests ────────────────────────────────────────────────

#[test]
fn test_undo_save_and_restore() {
    use yosh::interactive::undo::UndoManager;
    let mut um = UndoManager::new(256);
    um.save(&['h', 'e', 'l', 'l', 'o'], 5);
    let (buf, pos) = um.undo().unwrap();
    assert_eq!(buf, vec!['h', 'e', 'l', 'l', 'o']);
    assert_eq!(pos, 5);
}

#[test]
fn test_undo_multiple_states() {
    use yosh::interactive::undo::UndoManager;
    let mut um = UndoManager::new(256);
    um.save(&[], 0);
    um.save(&['a'], 1);
    let (buf, pos) = um.undo().unwrap();
    assert_eq!(buf, vec!['a']);
    assert_eq!(pos, 1);
    let (buf, pos) = um.undo().unwrap();
    assert_eq!(buf, Vec::<char>::new());
    assert_eq!(pos, 0);
    assert!(um.undo().is_none());
}

#[test]
fn test_undo_empty_returns_none() {
    use yosh::interactive::undo::UndoManager;
    let mut um = UndoManager::new(256);
    assert!(um.undo().is_none());
}

#[test]
fn test_undo_clear_resets_stack() {
    use yosh::interactive::undo::UndoManager;
    let mut um = UndoManager::new(256);
    um.save(&['a'], 1);
    um.save(&['a', 'b'], 2);
    um.clear();
    assert!(um.undo().is_none());
}

#[test]
fn test_undo_respects_max_size() {
    use yosh::interactive::undo::UndoManager;
    let mut um = UndoManager::new(2);
    um.save(&[], 0);
    um.save(&['a'], 1);
    um.save(&['a', 'b'], 2); // evicts ([], 0)
    let (buf, _) = um.undo().unwrap();
    assert_eq!(buf, vec!['a', 'b']);
    let (buf, _) = um.undo().unwrap();
    assert_eq!(buf, vec!['a']);
    assert!(um.undo().is_none());
}

// ── Keymap tests ──────────────────────────────────────────────────────

fn default_state() -> BufferState {
    BufferState {
        is_empty: false,
        at_end: false,
        has_suggestion: false,
        last_action: EditAction::Noop,
    }
}

fn key_event(
    code: KeyCode,
    modifiers: crossterm::event::KeyModifiers,
) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, modifiers)
}

#[test]
fn test_keymap_ctrl_k() {
    let mut km = Keymap::new();
    let (action, count) = km.resolve(
        key_event(KeyCode::Char('k'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::KillToEnd);
    assert_eq!(count, 1);
}

#[test]
fn test_keymap_ctrl_u() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('u'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::KillToStart);
}

#[test]
fn test_keymap_ctrl_w() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('w'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::KillBackwardWord);
}

#[test]
fn test_keymap_ctrl_y() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('y'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::Yank);
}

#[test]
fn test_keymap_alt_y_after_yank() {
    let mut km = Keymap::new();
    let state = BufferState {
        last_action: EditAction::Yank,
        ..default_state()
    };
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('y'), crossterm::event::KeyModifiers::ALT),
        &state,
    );
    assert_eq!(action, EditAction::YankPop);
}

#[test]
fn test_keymap_alt_y_without_yank() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('y'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::Noop);
}

#[test]
fn test_keymap_alt_b() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('b'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::MoveBackwardWord);
}

#[test]
fn test_keymap_alt_d() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('d'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::KillForwardWord);
}

#[test]
fn test_keymap_ctrl_t() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('t'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::TransposeChars);
}

#[test]
fn test_keymap_alt_t() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('t'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::TransposeWords);
}

#[test]
fn test_keymap_alt_u() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('u'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::UpcaseWord);
}

#[test]
fn test_keymap_alt_l() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('l'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::DowncaseWord);
}

#[test]
fn test_keymap_alt_c() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('c'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::CapitalizeWord);
}

#[test]
fn test_keymap_ctrl_underscore() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('_'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::Undo);
}

#[test]
fn test_keymap_ctrl_l() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('l'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::ClearScreen);
}

#[test]
fn test_keymap_ctrl_g_cancel() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('g'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::Cancel);
}

#[test]
fn test_keymap_ctrl_d_empty_is_eof() {
    let mut km = Keymap::new();
    let state = BufferState {
        is_empty: true,
        ..default_state()
    };
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('d'), crossterm::event::KeyModifiers::CONTROL),
        &state,
    );
    assert_eq!(action, EditAction::Eof);
}

#[test]
fn test_keymap_ctrl_d_nonempty_is_delete() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('d'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::DeleteForward);
}

#[test]
fn test_keymap_right_with_suggestion_accepts() {
    let mut km = Keymap::new();
    let state = BufferState {
        at_end: true,
        has_suggestion: true,
        ..default_state()
    };
    let (action, _) = km.resolve(
        key_event(KeyCode::Right, crossterm::event::KeyModifiers::empty()),
        &state,
    );
    assert_eq!(action, EditAction::AcceptSuggestion);
}

#[test]
fn test_keymap_right_without_suggestion_moves() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Right, crossterm::event::KeyModifiers::empty()),
        &default_state(),
    );
    assert_eq!(action, EditAction::MoveForward);
}

#[test]
fn test_keymap_alt_f_with_suggestion_accepts_word() {
    let mut km = Keymap::new();
    let state = BufferState {
        has_suggestion: true,
        ..default_state()
    };
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('f'), crossterm::event::KeyModifiers::ALT),
        &state,
    );
    assert_eq!(action, EditAction::AcceptWordSuggestion);
}

#[test]
fn test_keymap_alt_f_without_suggestion_moves_word() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('f'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::MoveForwardWord);
}

#[test]
fn test_keymap_numeric_arg() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('3'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::SetNumericArg(3));
    assert_eq!(km.pending_numeric_arg(), Some(3));

    let (action, count) = km.resolve(
        key_event(KeyCode::Char('f'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::MoveForward);
    assert_eq!(count, 3);
    assert_eq!(km.pending_numeric_arg(), None);
}

#[test]
fn test_keymap_numeric_arg_multi_digit() {
    let mut km = Keymap::new();
    km.resolve(
        key_event(KeyCode::Char('1'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    km.resolve(
        key_event(KeyCode::Char('5'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(km.pending_numeric_arg(), Some(15));
}

#[test]
fn test_keymap_ctrl_g_resets_numeric_arg() {
    let mut km = Keymap::new();
    km.resolve(
        key_event(KeyCode::Char('5'), crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(km.pending_numeric_arg(), Some(5));
    let (action, _) = km.resolve(
        key_event(KeyCode::Char('g'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(action, EditAction::Cancel);
    assert_eq!(km.pending_numeric_arg(), None);
}

#[test]
fn test_keymap_existing_bindings_preserved() {
    let mut km = Keymap::new();
    let (a, _) = km.resolve(
        key_event(KeyCode::Char('a'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(a, EditAction::MoveToStart);

    let (a, _) = km.resolve(
        key_event(KeyCode::Char('e'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(a, EditAction::MoveToEnd);

    let (a, _) = km.resolve(
        key_event(KeyCode::Char('b'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(a, EditAction::MoveBackward);

    let (a, _) = km.resolve(
        key_event(KeyCode::Enter, crossterm::event::KeyModifiers::empty()),
        &default_state(),
    );
    assert_eq!(a, EditAction::Submit);

    let (a, _) = km.resolve(
        key_event(KeyCode::Char('c'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(a, EditAction::Interrupt);

    let (a, _) = km.resolve(
        key_event(KeyCode::Char('r'), crossterm::event::KeyModifiers::CONTROL),
        &default_state(),
    );
    assert_eq!(a, EditAction::FuzzySearch);
}

#[test]
fn test_keymap_alt_backspace() {
    let mut km = Keymap::new();
    let (action, _) = km.resolve(
        key_event(KeyCode::Backspace, crossterm::event::KeyModifiers::ALT),
        &default_state(),
    );
    assert_eq!(action, EditAction::KillBackwardWord);
}

// ── Word boundary tests ───────────────────────────────────────────────

#[test]
fn test_move_backward_word() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() {
        ed.insert_char(ch);
    }
    ed.move_backward_word();
    assert_eq!(ed.cursor(), 6);
    ed.move_backward_word();
    assert_eq!(ed.cursor(), 0);
    ed.move_backward_word();
    assert_eq!(ed.cursor(), 0);
}

#[test]
fn test_move_forward_word() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() {
        ed.insert_char(ch);
    }
    ed.move_to_start();
    ed.move_forward_word();
    assert_eq!(ed.cursor(), 5);
    ed.move_forward_word();
    assert_eq!(ed.cursor(), 11);
    ed.move_forward_word();
    assert_eq!(ed.cursor(), 11);
}

#[test]
fn test_move_backward_word_with_multiple_spaces() {
    let mut ed = LineEditor::new();
    for ch in "foo   bar".chars() {
        ed.insert_char(ch);
    }
    ed.move_backward_word();
    assert_eq!(ed.cursor(), 6);
}

#[test]
fn test_move_forward_word_with_symbols() {
    let mut ed = LineEditor::new();
    for ch in "foo--bar".chars() {
        ed.insert_char(ch);
    }
    ed.move_to_start();
    ed.move_forward_word();
    assert_eq!(ed.cursor(), 3);
    ed.move_forward_word();
    assert_eq!(ed.cursor(), 8);
}

#[test]
fn test_kill_to_end() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() {
        ed.insert_char(ch);
    }
    ed.move_to_start();
    for _ in 0..5 {
        ed.move_cursor_right();
    }
    let killed = ed.kill_to_end();
    assert_eq!(ed.buffer(), "hello");
    assert_eq!(killed, " world");
}

#[test]
fn test_kill_to_start() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() {
        ed.insert_char(ch);
    }
    ed.move_to_start();
    for _ in 0..5 {
        ed.move_cursor_right();
    }
    let killed = ed.kill_to_start();
    assert_eq!(ed.buffer(), " world");
    assert_eq!(ed.cursor(), 0);
    assert_eq!(killed, "hello");
}

#[test]
fn test_kill_backward_word() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() {
        ed.insert_char(ch);
    }
    let killed = ed.kill_backward_word();
    assert_eq!(ed.buffer(), "hello ");
    assert_eq!(killed, "world");
}

#[test]
fn test_kill_forward_word() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() {
        ed.insert_char(ch);
    }
    ed.move_to_start();
    let killed = ed.kill_forward_word();
    assert_eq!(ed.buffer(), " world");
    assert_eq!(killed, "hello");
}

#[test]
fn test_transpose_chars_middle() {
    let mut ed = LineEditor::new();
    for ch in "abc".chars() {
        ed.insert_char(ch);
    }
    ed.move_to_start();
    ed.move_cursor_right();
    ed.transpose_chars();
    assert_eq!(ed.buffer(), "bac");
    assert_eq!(ed.cursor(), 2);
}

#[test]
fn test_transpose_chars_at_end() {
    let mut ed = LineEditor::new();
    for ch in "abc".chars() {
        ed.insert_char(ch);
    }
    ed.transpose_chars();
    assert_eq!(ed.buffer(), "acb");
    assert_eq!(ed.cursor(), 3);
}

#[test]
fn test_transpose_chars_at_start_noop() {
    let mut ed = LineEditor::new();
    for ch in "abc".chars() {
        ed.insert_char(ch);
    }
    ed.move_to_start();
    ed.transpose_chars();
    assert_eq!(ed.buffer(), "abc");
    assert_eq!(ed.cursor(), 0);
}

#[test]
fn test_upcase_word() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() {
        ed.insert_char(ch);
    }
    ed.move_to_start();
    ed.upcase_word();
    assert_eq!(ed.buffer(), "HELLO world");
    assert_eq!(ed.cursor(), 5);
}

#[test]
fn test_downcase_word() {
    let mut ed = LineEditor::new();
    for ch in "HELLO WORLD".chars() {
        ed.insert_char(ch);
    }
    ed.move_to_start();
    ed.downcase_word();
    assert_eq!(ed.buffer(), "hello WORLD");
    assert_eq!(ed.cursor(), 5);
}

#[test]
fn test_capitalize_word() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() {
        ed.insert_char(ch);
    }
    ed.move_to_start();
    ed.capitalize_word();
    assert_eq!(ed.buffer(), "Hello world");
    assert_eq!(ed.cursor(), 5);
}

#[test]
fn test_transpose_words() {
    let mut ed = LineEditor::new();
    for ch in "hello world".chars() {
        ed.insert_char(ch);
    }
    ed.transpose_words();
    assert_eq!(ed.buffer(), "world hello");
    assert_eq!(ed.cursor(), 11);
}

#[test]
fn test_transpose_words_cursor_in_middle() {
    let mut ed = LineEditor::new();
    for ch in "aaa bbb ccc".chars() {
        ed.insert_char(ch);
    }
    ed.move_to_start();
    for _ in 0..5 {
        ed.move_cursor_right();
    }
    ed.transpose_words();
    assert_eq!(ed.buffer(), "bbb aaa ccc");
    assert_eq!(ed.cursor(), 7);
}

// ── Integration tests: kill ring via MockTerminal ─────────────────────

#[test]
fn test_mock_ctrl_k_kills_to_end() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("hello world"),
        vec![ctrl('a')],
        vec![key(KeyCode::Right); 5],
        vec![ctrl('k')],
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &[], &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "hello");
}

#[test]
fn test_mock_ctrl_u_kills_to_start() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("hello world"),
        vec![ctrl('a')],
        vec![key(KeyCode::Right); 5],
        vec![ctrl('u')],
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &[], &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), " world");
}

#[test]
fn test_mock_ctrl_w_kills_backward_word() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("hello world"),
        vec![ctrl('w')],
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &[], &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "hello ");
}

#[test]
fn test_mock_ctrl_y_yanks() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("hello world"),
        vec![ctrl('w')],
        vec![ctrl('a')],
        vec![ctrl('y')],
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &[], &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "worldhello ");
}

#[test]
fn test_mock_ctrl_underscore_undo() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("hello"),
        vec![ctrl('a')],
        vec![ctrl('k')],
        vec![ctrl('_')],
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &[], &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "hello");
}

#[test]
fn test_mock_alt_b_word_backward() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("hello world"),
        vec![alt('b')],
        vec![ctrl('k')],
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &[], &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "hello ");
}

#[test]
fn test_mock_ctrl_l_clears_screen() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [chars("test"), vec![ctrl('l')], vec![key(KeyCode::Enter)]].concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &[], &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "test");
}

#[test]
fn test_mock_ctrl_t_transpose() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [chars("ab"), vec![ctrl('t')], vec![key(KeyCode::Enter)]].concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &[], &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "ba");
}

#[test]
fn test_mock_alt_u_upcase() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("hello"),
        vec![ctrl('a')],
        vec![alt('u')],
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &[], &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "HELLO");
}

#[test]
fn test_mock_numeric_arg_movement() {
    let mut ed = LineEditor::new();
    let mut history = History::new();
    let events = [
        chars("abcdef"),
        vec![ctrl('a')],
        vec![alt('3')],
        vec![ctrl('f')],
        vec![ctrl('k')],
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut term = MockTerminal::new(events);
    let result = ed.read_line("$ ", &[], &mut history, &mut term);
    assert_eq!(result.unwrap().unwrap(), "abc");
}

// ── Multiline editing tests ─────────────────────────────────────────────

/// Alt+Enter key event (force-insert newline).
fn alt_enter() -> crossterm::event::Event {
    crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
        KeyCode::Enter,
        crossterm::event::KeyModifiers::ALT,
    ))
}

/// Completeness probe backed by the real REPL classifier.
fn shell_incomplete(text: &str) -> bool {
    let aliases = AliasStore::default();
    let candidate = format!("{}\n", text);
    matches!(
        classify_parse(&candidate, &aliases),
        ParseStatus::Incomplete
    )
}

/// Drive `read_line_with_completion` with minimal contexts, a "$ " prompt,
/// a "> " continuation prompt, and the given completeness probe.
fn read_multiline(
    events: Vec<crossterm::event::Event>,
    history: &mut History,
    is_incomplete: &dyn Fn(&str) -> bool,
) -> (Option<String>, Vec<String>) {
    let (result, term) = read_multiline_sized(events, history, is_incomplete, 80, 24);
    (result, term.output().to_vec())
}

/// Like [`read_multiline`] but with an explicit terminal size, returning the
/// terminal so tests can assert on its movement bookkeeping.
fn read_multiline_sized(
    events: Vec<crossterm::event::Event>,
    history: &mut History,
    is_incomplete: &dyn Fn(&str) -> bool,
    width: u16,
    height: u16,
) -> (Option<String>, MockTerminal) {
    let ctx = CompletionContext {
        cwd: "/".to_string(),
        home: "/home/user".to_string(),
        show_dotfiles: false,
    };
    let mut term = MockTerminal::new(events);
    term.set_size(width, height);
    let mut editor = LineEditor::new();
    let aliases = AliasStore::default();
    let mut command_completer = CommandCompleter::new();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let mut spec_store = yosh::interactive::spec_completion::SpecStore::new(
        std::path::PathBuf::from("/nonexistent"),
    );
    let result = editor
        .read_line_with_completion(
            "$ ",
            &[],
            history,
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
        .unwrap();
    (result, term)
}

#[test]
fn test_multiline_enter_on_incomplete_inserts_newline() {
    // `if true` is incomplete; Enter continues in-buffer until `fi` closes it.
    let events = [
        chars("if true"),
        vec![key(KeyCode::Enter)],
        chars("then echo hi"),
        vec![key(KeyCode::Enter)],
        chars("fi"),
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut history = History::new();
    let (result, _) = read_multiline(events, &mut history, &shell_incomplete);
    assert_eq!(result, Some("if true\nthen echo hi\nfi".to_string()));
}

#[test]
fn test_multiline_unclosed_quote_continues() {
    let events = [
        chars("echo 'a"),
        vec![key(KeyCode::Enter)],
        chars("b'"),
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut history = History::new();
    let (result, _) = read_multiline(events, &mut history, &shell_incomplete);
    assert_eq!(result, Some("echo 'a\nb'".to_string()));
}

#[test]
fn test_multiline_continuation_prompt_rendered() {
    // After Enter on incomplete input, the redraw must paint the "> "
    // continuation prompt. Ctrl+C ends the read (returns Some("")).
    let events = [chars("if true"), vec![key(KeyCode::Enter)], vec![ctrl('c')]].concat();
    let mut history = History::new();
    let (result, output) = read_multiline(events, &mut history, &shell_incomplete);
    assert_eq!(result, Some(String::new()));
    assert!(
        output.iter().any(|s| s == "> "),
        "expected continuation prompt in output: {:?}",
        output
    );
}

#[test]
fn test_multiline_alt_enter_forces_newline() {
    // Alt+Enter inserts a newline even when the input is complete.
    let events = [
        chars("echo a"),
        vec![alt_enter()],
        chars("echo b"),
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut history = History::new();
    let (result, _) = read_multiline(events, &mut history, &|_| false);
    assert_eq!(result, Some("echo a\necho b".to_string()));
}

#[test]
fn test_multiline_up_down_cursor_movement() {
    // Up moves to the previous logical line (column preserved), Down back.
    let events = [
        chars("abc"),
        vec![alt_enter()],
        chars("def"),
        vec![key(KeyCode::Up)],
        chars("X"),
        vec![key(KeyCode::Down)],
        chars("Y"),
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut history = History::new();
    let (result, _) = read_multiline(events, &mut history, &|_| false);
    assert_eq!(result, Some("abcX\ndefY".to_string()));
}

#[test]
fn test_multiline_up_at_first_line_navigates_history() {
    let mut history = History::new();
    history.add("echo old", 500, "");
    // Buffer "ab\ncd": first Up moves to line 1, second Up recalls history.
    let events = [
        chars("ab"),
        vec![alt_enter()],
        chars("cd"),
        vec![key(KeyCode::Up)],
        vec![key(KeyCode::Up)],
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let (result, _) = read_multiline(events, &mut history, &|_| false);
    assert_eq!(result, Some("echo old".to_string()));
}

#[test]
fn test_multiline_ctrl_a_e_are_line_local() {
    let events = [
        chars("ab"),
        vec![alt_enter()],
        chars("cd"),
        vec![ctrl('a')],
        chars("Z"),
        vec![ctrl('e')],
        chars("W"),
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut history = History::new();
    let (result, _) = read_multiline(events, &mut history, &|_| false);
    assert_eq!(result, Some("ab\nZcdW".to_string()));
}

#[test]
fn test_multiline_ctrl_k_at_line_end_kills_newline() {
    // Cursor at end of first line; Ctrl+K removes the newline, joining lines.
    let events = [
        chars("ab"),
        vec![alt_enter()],
        chars("cd"),
        vec![key(KeyCode::Up)],
        vec![ctrl('k')],
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut history = History::new();
    let (result, _) = read_multiline(events, &mut history, &|_| false);
    assert_eq!(result, Some("abcd".to_string()));
}

#[test]
fn test_multiline_ctrl_u_kills_to_line_start_only() {
    let events = [
        chars("ab"),
        vec![alt_enter()],
        chars("cd"),
        vec![ctrl('u')],
        chars("x"),
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut history = History::new();
    let (result, _) = read_multiline(events, &mut history, &|_| false);
    assert_eq!(result, Some("ab\nx".to_string()));
}

#[test]
fn test_multiline_up_preserves_display_column_wide_chars() {
    // Line 1 "あい" (width 4). Cursor on line 2 at col 3 ("def" end… use
    // "abc" end = col 3): Up lands after "あ" (width 2 ≤ 3 < 4 would split
    // "い", so the cursor stops before it).
    let events = [
        chars("あい"),
        vec![alt_enter()],
        chars("abc"),
        vec![key(KeyCode::Up)],
        chars("X"),
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut history = History::new();
    let (result, _) = read_multiline(events, &mut history, &|_| false);
    assert_eq!(result, Some("あXい\nabc".to_string()));
}

#[test]
fn test_multiline_backspace_joins_lines() {
    // Backspace at a line start deletes the newline separator.
    let events = [
        chars("ab"),
        vec![alt_enter()],
        vec![key(KeyCode::Backspace)],
        chars("c"),
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut history = History::new();
    let (result, _) = read_multiline(events, &mut history, &|_| false);
    assert_eq!(result, Some("abc".to_string()));
}

#[test]
fn test_multiline_buffer_line_helpers() {
    let mut ed = LineEditor::new();
    for ch in "ab\ncd\nef".chars() {
        ed.insert_char(ch);
    }
    assert_eq!(ed.line_count(), 3);
    assert_eq!(ed.cursor_line_index(), 2);
    ed.move_cursor_up();
    assert_eq!(ed.cursor_line_index(), 1);
    ed.move_to_start();
    assert_eq!(ed.cursor(), 3); // start of "cd"
    ed.move_to_end();
    assert_eq!(ed.cursor(), 5); // end of "cd"
    ed.move_cursor_down();
    assert_eq!(ed.cursor_line_index(), 2);
}

// ── Viewport-clamped rendering (taller-than-terminal constructs) ────────

#[test]
fn test_multiline_taller_than_terminal_stays_in_viewport() {
    // 8 logical lines on a 5-row terminal: every relative cursor movement
    // must stay within the viewport (move_up ≤ height - 1). Before viewport
    // clamping, redraw moved up by the full off-screen row count, which a
    // real terminal clamps at the top row — corrupting the display.
    let mut events = chars("l0");
    for i in 1..8 {
        events.push(alt_enter());
        events.extend(chars(&format!("l{}", i)));
    }
    events.push(key(KeyCode::Enter));
    let mut history = History::new();
    let (result, term) = read_multiline_sized(events, &mut history, &|_| false, 80, 5);
    assert_eq!(result, Some("l0\nl1\nl2\nl3\nl4\nl5\nl6\nl7".to_string()));
    assert!(
        term.max_move_up() <= 4,
        "cursor moved up {} rows on a 5-row terminal",
        term.max_move_up()
    );
}

#[test]
fn test_multiline_viewport_scrolls_to_cursor() {
    // Move the cursor from the last of 8 lines back to the first on a
    // 4-row terminal: the viewport must follow the cursor, keeping edits
    // at the top of the construct working.
    let mut events = chars("l0");
    for i in 1..8 {
        events.push(alt_enter());
        events.extend(chars(&format!("l{}", i)));
    }
    for _ in 0..7 {
        events.push(key(KeyCode::Up));
    }
    events.extend(chars("X"));
    events.push(key(KeyCode::Enter));
    let mut history = History::new();
    let (result, term) = read_multiline_sized(events, &mut history, &|_| false, 80, 4);
    assert_eq!(result, Some("l0X\nl1\nl2\nl3\nl4\nl5\nl6\nl7".to_string()));
    assert!(
        term.max_move_up() <= 3,
        "cursor moved up {} rows on a 4-row terminal",
        term.max_move_up()
    );
}

#[test]
fn test_soft_wrapped_single_line_taller_than_terminal() {
    // A single logical line that soft-wraps past the terminal height must
    // take the viewport-clamped path too.
    let long: String = "x".repeat(50);
    let mut events = chars(&long);
    events.push(key(KeyCode::Enter));
    let mut history = History::new();
    let (result, term) = read_multiline_sized(events, &mut history, &|_| false, 10, 3);
    assert_eq!(result, Some(long));
    assert!(
        term.max_move_up() <= 2,
        "cursor moved up {} rows on a 3-row terminal",
        term.max_move_up()
    );
}

// ── Preferred-column stickiness across vertical moves ───────────────────

#[test]
fn test_up_down_preferred_column_stickiness() {
    // From column 8 on the last line, Up clamps to the short middle line
    // but a second Up must return to column 8 (readline behavior), not
    // stay at the clamped column.
    let events = [
        chars("abcdefgh"),
        vec![alt_enter()],
        chars("xy"),
        vec![alt_enter()],
        chars("abcdefgh"),
        vec![key(KeyCode::Up), key(KeyCode::Up)],
        chars("Z"),
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut history = History::new();
    let (result, _) = read_multiline(events, &mut history, &|_| false);
    assert_eq!(result, Some("abcdefghZ\nxy\nabcdefgh".to_string()));
}

#[test]
fn test_preferred_column_resets_on_horizontal_move() {
    // A horizontal move between vertical moves ends the sticky run: the
    // next Up targets the new column, not the original one.
    let events = [
        chars("abcdefgh"),
        vec![alt_enter()],
        chars("xy"),
        vec![alt_enter()],
        chars("abcdefgh"),
        vec![key(KeyCode::Up), key(KeyCode::Left), key(KeyCode::Up)],
        chars("Z"),
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut history = History::new();
    let (result, _) = read_multiline(events, &mut history, &|_| false);
    assert_eq!(result, Some("aZbcdefgh\nxy\nabcdefgh".to_string()));
}

// ── Multiline autosuggestions ───────────────────────────────────────────

#[test]
fn test_multiline_suggestion_rendered_dim_with_cont_prompt() {
    // A history entry spanning lines must be suggested (not suppressed)
    // and its continuation lines rendered with the continuation prompt.
    let mut history = History::new();
    history.add("for i in 1 2; do\necho $i\ndone", 500, "");
    let events = [chars("for"), vec![ctrl('c')]].concat();
    let (result, term) = read_multiline_sized(events, &mut history, &shell_incomplete, 80, 24);
    assert_eq!(result, Some(String::new()));
    let output = term.output().to_vec();
    assert!(
        output.iter().any(|s| s == "[DIM]"),
        "multiline suggestion should render dim: {:?}",
        output
    );
    assert!(
        output.iter().any(|s| s == "> "),
        "suggestion continuation lines should carry the continuation prompt: {:?}",
        output
    );
}

#[test]
fn test_multiline_suggestion_accept_full() {
    let mut history = History::new();
    history.add("for i in 1 2; do\necho $i\ndone", 500, "");
    let events = [
        chars("for"),
        vec![key(KeyCode::Right)],
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let (result, _) = read_multiline_sized(events, &mut history, &shell_incomplete, 80, 24);
    assert_eq!(result, Some("for i in 1 2; do\necho $i\ndone".to_string()));
}

#[test]
fn test_word_accept_stops_at_suggestion_newline() {
    // Alt+F must not swallow a line boundary as part of a "word": the
    // first accept stops before the suggestion's '\n'; a further accept
    // crosses it deliberately.
    let mut history = History::new();
    history.add("a b\nc d", 500, "");
    let events = [chars("a"), vec![alt('f')], vec![key(KeyCode::Enter)]].concat();
    let (result, _) = read_multiline_sized(events, &mut history, &|_| false, 80, 24);
    assert_eq!(result, Some("a b".to_string()));

    let mut history = History::new();
    history.add("a b\nc d", 500, "");
    let events = [
        chars("a"),
        vec![alt('f'), alt('f')],
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let (result, _) = read_multiline_sized(events, &mut history, &|_| false, 80, 24);
    assert_eq!(result, Some("a b\nc".to_string()));
}

#[test]
fn test_numeric_arg_preserves_preferred_column() {
    // Alt+1 between two Ups is a count prefix, not an edit: the second Up
    // must still return to the original column 8, not the clamped column 2.
    let events = [
        chars("abcdefgh"),
        vec![alt_enter()],
        chars("xy"),
        vec![alt_enter()],
        chars("abcdefgh"),
        vec![key(KeyCode::Up), alt('1'), key(KeyCode::Up)],
        chars("Z"),
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let mut history = History::new();
    let (result, _) = read_multiline(events, &mut history, &|_| false);
    assert_eq!(result, Some("abcdefghZ\nxy\nabcdefgh".to_string()));
}

#[test]
fn test_transpose_invalidates_suggestion() {
    // Ctrl+T changes buffer content; a suggestion computed for the
    // pre-transpose prefix must not be acceptable afterwards.
    let mut history = History::new();
    history.add("echo hi", 500, "");
    let events = [
        chars("ec"),
        vec![ctrl('t')],           // buffer now "ce"
        vec![key(KeyCode::Right)], // no suggestion for "ce" — normal cursor move
        vec![key(KeyCode::Enter)],
    ]
    .concat();
    let (result, _) = read_multiline_sized(events, &mut history, &|_| false, 80, 24);
    assert_eq!(result, Some("ce".to_string()));
}

// ── Grid-emulation tests (physical screen assertions) ───────────────────
//
// GridTerminal models xterm geometry (auto-wrap, deferred wrap, edge
// clamping, scrolling), so these tests pin the *final screen contents* —
// the class of guarantee neither the stream-recording MockTerminal nor the
// PTY escape scan provides.

/// Drive `read_line_with_completion` against a [`GridTerminal`]. The event
/// stream may end without submitting — the read then returns an error and
/// the grid holds the last painted state for inspection.
fn read_multiline_grid(
    events: Vec<crossterm::event::Event>,
    history: &mut History,
    is_incomplete: &dyn Fn(&str) -> bool,
    prompt: &str,
    width: u16,
    height: u16,
) -> (std::io::Result<Option<String>>, GridTerminal) {
    read_multiline_grid_from(events, history, is_incomplete, prompt, width, height, 0)
}

/// Like [`read_multiline_grid`] but with the cursor pre-advanced
/// `start_row` rows — simulating a prompt that begins near the bottom of a
/// screen that already holds output, so growth exercises real scrolling.
#[allow(clippy::too_many_arguments)]
fn read_multiline_grid_from(
    events: Vec<crossterm::event::Event>,
    history: &mut History,
    is_incomplete: &dyn Fn(&str) -> bool,
    prompt: &str,
    width: u16,
    height: u16,
    start_row: usize,
) -> (std::io::Result<Option<String>>, GridTerminal) {
    let ctx = CompletionContext {
        cwd: "/".to_string(),
        home: "/home/user".to_string(),
        show_dotfiles: false,
    };
    let mut term = GridTerminal::new(events, width, height);
    for _ in 0..start_row {
        use yosh::interactive::terminal::Terminal;
        term.write_str("\r\n").unwrap();
    }
    let mut editor = LineEditor::new();
    let aliases = AliasStore::default();
    let mut command_completer = CommandCompleter::new();
    let mut cmd_ctx = CommandCompletionContext {
        completer: &mut command_completer,
        path: "",
        builtins: &[],
        aliases: &aliases,
    };
    let mut scanner = HighlightScanner::new();
    let checker_env = CheckerEnv {
        path: "",
        aliases: &aliases,
    };
    let mut spec_store = yosh::interactive::spec_completion::SpecStore::new(
        std::path::PathBuf::from("/nonexistent"),
    );
    let result = editor.read_line_with_completion(
        prompt,
        &[],
        history,
        &mut term,
        &ctx,
        &mut cmd_ctx,
        &mut spec_store,
        &mut scanner,
        &checker_env,
        "",
        &mut || "> ".to_string(),
        is_incomplete,
    );
    (result, term)
}

/// Build "l0" .. Alt+Enter .. "l{n-1}" events (no submit).
fn grid_lines_events(n: usize) -> Vec<crossterm::event::Event> {
    let mut events = chars("l0");
    for i in 1..n {
        events.push(alt_enter());
        events.extend(chars(&format!("l{}", i)));
    }
    events
}

#[test]
fn test_grid_tall_construct_shows_tail_viewport() {
    // 8 logical lines on a 5-row screen: the final screen must show
    // exactly the last 5 rows, cursor at the end of the last line, with
    // no cursor movement ever clamped at a screen edge.
    let mut history = History::new();
    let (_, term) =
        read_multiline_grid(grid_lines_events(8), &mut history, &|_| false, "$ ", 20, 5);
    assert_eq!(
        term.screen(),
        vec!["> l3", "> l4", "> l5", "> l6", "> l7"],
        "screen should show the tail viewport"
    );
    assert_eq!(term.cursor(), (4, 4));
    assert_eq!(
        term.edge_violations(),
        0,
        "renderer addressed off-screen rows"
    );
    assert_eq!(term.col_overflows(), 0);
}

#[test]
fn test_grid_growth_from_bottom_row_scrolls_cleanly() {
    // Start with the prompt on the bottom row of a screen already full of
    // output: growing the construct line by line forces the terminal to
    // scroll during paints, and the relative bookkeeping must stay aligned
    // (the screen still ends up showing exactly the tail viewport).
    let mut history = History::new();
    let (_, term) = read_multiline_grid_from(
        grid_lines_events(8),
        &mut history,
        &|_| false,
        "$ ",
        20,
        5,
        4,
    );
    assert_eq!(term.screen(), vec!["> l3", "> l4", "> l5", "> l6", "> l7"]);
    assert_eq!(term.cursor(), (4, 4));
    assert!(term.scrolls() > 0, "growth from the bottom row must scroll");
    assert_eq!(term.edge_violations(), 0);
    assert_eq!(term.col_overflows(), 0);
}

#[test]
fn test_grid_viewport_follows_cursor_to_top() {
    // Walking the cursor to the first of 8 lines on a 4-row screen must
    // scroll the viewport up to show the head of the construct.
    let mut events = grid_lines_events(8);
    for _ in 0..7 {
        events.push(key(KeyCode::Up));
    }
    let mut history = History::new();
    let (_, term) = read_multiline_grid(events, &mut history, &|_| false, "$ ", 20, 4);
    assert_eq!(term.screen(), vec!["$ l0", "> l1", "> l2", "> l3"]);
    // Sticky column: col 2 within the line, after the 2-wide prompt.
    assert_eq!(term.cursor(), (0, 4));
    assert_eq!(term.edge_violations(), 0);
    assert_eq!(term.col_overflows(), 0);
}

#[test]
fn test_grid_prompt_wider_than_terminal() {
    // A 26-wide prompt on a 10-col screen auto-wraps over three physical
    // rows; the packer must model those rows so the clear/repaint cycle
    // stays aligned. (Adversarial-review regression: the packer used to
    // count every prefix as one row, smearing the display.)
    let prompt = "PROMPTPROMPTPROMPTPROMPT$ ";
    let events = [chars("abc"), vec![alt_enter()], chars("def")].concat();
    let mut history = History::new();
    let (_, term) = read_multiline_grid(events, &mut history, &|_| false, prompt, 10, 6);
    assert_eq!(
        term.screen(),
        vec!["PROMPTPROM", "PTPROMPTPR", "OMPT$ abc", "> def", "", ""]
    );
    assert_eq!(term.cursor(), (3, 5));
    assert_eq!(term.edge_violations(), 0);
    assert_eq!(term.col_overflows(), 0);
}

#[test]
fn test_grid_cjk_tall_buffer_takes_viewport_path() {
    // 12 double-width chars on a 9x3 screen: pure width division predicts
    // 3 rows (fits), but wide chars wrap early — physically 4 rows. The
    // pessimistic dispatch must route to the viewport renderer; the screen
    // shows the tail window with no clamped movement.
    let events = chars(&"あ".repeat(12));
    let mut history = History::new();
    let (_, term) = read_multiline_grid(events, &mut history, &|_| false, "$ ", 9, 3);
    // Packing: 3 chars fit after the 2-wide prompt, then 4 per row: the
    // 4 physical rows are [3, 4, 4, 1] chars; the viewport shows the tail.
    assert_eq!(term.screen(), vec!["ああああ", "ああああ", "あ"]);
    assert_eq!(term.edge_violations(), 0);
    assert_eq!(term.col_overflows(), 0);
}

#[test]
fn test_grid_exact_width_row_cursor_clamped() {
    // Prompt(2) + "abcdefgh"(8) exactly fills a 10-col row; Ctrl+E on that
    // line puts the cursor in the deferred-wrap position, which must be
    // emitted as the last real column — never past the screen edge.
    let events = [
        chars("abcdefgh"),
        vec![alt_enter()],
        chars("x"),
        vec![key(KeyCode::Up), ctrl('e')],
    ]
    .concat();
    let mut history = History::new();
    let (_, term) = read_multiline_grid(events, &mut history, &|_| false, "$ ", 10, 5);
    assert_eq!(term.screen(), vec!["$ abcdefgh", "> x", "", "", ""]);
    assert_eq!(term.cursor(), (0, 9));
    assert_eq!(
        term.col_overflows(),
        0,
        "cursor column emitted past the screen edge"
    );
    assert_eq!(term.edge_violations(), 0);
}

#[test]
fn test_grid_multiline_suggestion_layout() {
    // A multiline history suggestion renders as continuation-prompt lines
    // below the input, cursor staying at the end of the typed prefix.
    let mut history = History::new();
    history.add("for i in 1 2; do\necho $i\ndone", 500, "");
    let events = chars("for");
    let (_, term) = read_multiline_grid(events, &mut history, &shell_incomplete, "$ ", 30, 10);
    let screen = term.screen();
    assert_eq!(screen[0], "$ for i in 1 2; do");
    assert_eq!(screen[1], "> echo $i");
    assert_eq!(screen[2], "> done");
    assert_eq!(term.cursor(), (0, 5));
    assert_eq!(term.edge_violations(), 0);
}

#[test]
fn test_grid_submit_clears_unaccepted_multiline_suggestion() {
    // Submitting while a multiline suggestion is displayed must not leave
    // its dim phantom continuation lines on screen above the output.
    let mut history = History::new();
    history.add("echo apple\nls", 500, "");
    let events = [chars("echo a"), vec![key(KeyCode::Enter)]].concat();
    let (result, term) = read_multiline_grid(events, &mut history, &|_| false, "$ ", 30, 10);
    assert_eq!(result.unwrap(), Some("echo a".to_string()));
    let screen = term.screen();
    assert_eq!(screen[0], "$ echo a");
    assert!(
        screen
            .iter()
            .all(|l| !l.contains("pple") && !l.contains("ls")),
        "un-accepted suggestion left on screen: {:?}",
        screen
    );
    assert_eq!(term.edge_violations(), 0);
}
