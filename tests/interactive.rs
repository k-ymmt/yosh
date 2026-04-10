use kish::interactive::line_editor::LineEditor;

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
