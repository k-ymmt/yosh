//! Key-resolution tests for [`ViEngine`] (Command and VISUAL mode).

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

#[test]
fn engine_counts_and_zero() {
    let mut e = ViEngine::new();
    e.mode = ViMode::Command;
    assert_eq!(e.resolve_command_key(key('2')), ViOutcome::Pending);
    assert_eq!(e.resolve_command_key(key('3')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('l')),
        ViOutcome::Cmd(ViCmd::Move(ViMotion::CharForward), 23)
    );
    // Bare 0 is the line-start motion...
    assert_eq!(
        e.resolve_command_key(key('0')),
        ViOutcome::Cmd(ViCmd::Move(ViMotion::LineStart), 1)
    );
    // ...but 0 after a digit is part of the count.
    assert_eq!(e.resolve_command_key(key('1')), ViOutcome::Pending);
    assert_eq!(e.resolve_command_key(key('0')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('h')),
        ViOutcome::Cmd(ViCmd::Move(ViMotion::CharBack), 10)
    );
}

#[test]
fn engine_find_char_and_semicolon_memory() {
    let mut e = ViEngine::new();
    e.mode = ViMode::Command;
    assert_eq!(e.resolve_command_key(key('f')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('x')),
        ViOutcome::Cmd(ViCmd::Move(ViMotion::FindChar(FindKind::Find, 'x')), 1)
    );
    assert_eq!(
        e.resolve_command_key(key(';')),
        ViOutcome::Cmd(ViCmd::Move(ViMotion::FindChar(FindKind::Find, 'x')), 1)
    );
    assert_eq!(
        e.resolve_command_key(key(',')),
        ViOutcome::Cmd(ViCmd::Move(ViMotion::FindChar(FindKind::FindBack, 'x')), 1)
    );
    // Count before f applies to the find.
    assert_eq!(e.resolve_command_key(key('2')), ViOutcome::Pending);
    assert_eq!(e.resolve_command_key(key('t')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('y')),
        ViOutcome::Cmd(ViCmd::Move(ViMotion::FindChar(FindKind::To, 'y')), 2)
    );
}

#[test]
fn engine_replace_char_pending() {
    let mut e = ViEngine::new();
    e.mode = ViMode::Command;
    assert_eq!(e.resolve_command_key(key('r')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('Z')),
        ViOutcome::Cmd(ViCmd::ReplaceChar('Z'), 1)
    );
}

#[test]
fn engine_esc_cancels_pending() {
    let mut e = ViEngine::new();
    e.mode = ViMode::Command;
    assert_eq!(e.resolve_command_key(key('3')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        ViOutcome::Pending
    );
    // Count was discarded.
    assert_eq!(
        e.resolve_command_key(key('l')),
        ViOutcome::Cmd(ViCmd::Move(ViMotion::CharForward), 1)
    );
}

#[test]
fn engine_operator_sequences() {
    let mut e = ViEngine::new();
    e.mode = ViMode::Command;
    // dw
    assert_eq!(e.resolve_command_key(key('d')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('w')),
        ViOutcome::Cmd(
            ViCmd::Op(OpKind::Delete, ViMotion::WordForward { big: false }),
            1
        )
    );
    // 2d3w = 6 words
    for k in ['2', 'd', '3'] {
        assert_eq!(e.resolve_command_key(key(k)), ViOutcome::Pending);
    }
    assert_eq!(
        e.resolve_command_key(key('w')),
        ViOutcome::Cmd(
            ViCmd::Op(OpKind::Delete, ViMotion::WordForward { big: false }),
            6
        )
    );
    // dd
    assert_eq!(e.resolve_command_key(key('d')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('d')),
        ViOutcome::Cmd(ViCmd::OpLine(OpKind::Delete), 1)
    );
    // count ignored for $ under an operator
    assert_eq!(e.resolve_command_key(key('5')), ViOutcome::Pending);
    assert_eq!(e.resolve_command_key(key('c')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('$')),
        ViOutcome::Cmd(ViCmd::Op(OpKind::Change, ViMotion::LineEnd), 1)
    );
    // df x
    assert_eq!(e.resolve_command_key(key('d')), ViOutcome::Pending);
    assert_eq!(e.resolve_command_key(key('f')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('x')),
        ViOutcome::Cmd(
            ViCmd::Op(OpKind::Delete, ViMotion::FindChar(FindKind::Find, 'x')),
            1
        )
    );
}

#[test]
fn engine_repeat_count_sentinel() {
    let mut e = ViEngine::new();
    e.mode = ViMode::Command;
    // Bare `.` carries the "no explicit count" sentinel 0.
    assert_eq!(
        e.resolve_command_key(key('.')),
        ViOutcome::Cmd(ViCmd::Repeat, 0)
    );
    assert_eq!(e.resolve_command_key(key('3')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('.')),
        ViOutcome::Cmd(ViCmd::Repeat, 3)
    );
}

#[test]
fn engine_alias_macro_pending() {
    let mut e = ViEngine::new();
    e.mode = ViMode::Command;
    assert_eq!(e.resolve_command_key(key('@')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('z')),
        ViOutcome::Cmd(ViCmd::AliasMacro('z'), 1)
    );
    // Expansion commands resolve directly.
    assert_eq!(
        e.resolve_command_key(key('=')),
        ViOutcome::Cmd(ViCmd::ExpandList, 1)
    );
    assert_eq!(
        e.resolve_command_key(key('\\')),
        ViOutcome::Cmd(ViCmd::CompleteUnique, 1)
    );
    assert_eq!(
        e.resolve_command_key(key('#')),
        ViOutcome::Cmd(ViCmd::CommentSubmit, 1)
    );
    // v with and without a number (0 = current line sentinel).
    assert_eq!(
        e.resolve_command_key(key('v')),
        ViOutcome::Cmd(ViCmd::EditInEditor, 0)
    );
    assert_eq!(e.resolve_command_key(key('2')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('v')),
        ViOutcome::Cmd(ViCmd::EditInEditor, 2)
    );
}

#[test]
fn engine_count_is_capped() {
    let mut e = ViEngine::new();
    e.mode = ViMode::Command;
    for _ in 0..13 {
        assert_eq!(e.resolve_command_key(key('9')), ViOutcome::Pending);
    }
    assert_eq!(
        e.resolve_command_key(key('l')),
        ViOutcome::Cmd(ViCmd::Move(ViMotion::CharForward), 1_000_000)
    );
    // Operator-side accumulation and the multiplied total cap too.
    assert_eq!(e.resolve_command_key(key('9')), ViOutcome::Pending);
    assert_eq!(e.resolve_command_key(key('9')), ViOutcome::Pending);
    assert_eq!(e.resolve_command_key(key('d')), ViOutcome::Pending);
    for _ in 0..13 {
        assert_eq!(e.resolve_command_key(key('9')), ViOutcome::Pending);
    }
    assert_eq!(
        e.resolve_command_key(key('w')),
        ViOutcome::Cmd(
            ViCmd::Op(OpKind::Delete, ViMotion::WordForward { big: false }),
            1_000_000
        )
    );
}

#[test]
fn engine_semicolon_without_find_bells() {
    let mut e = ViEngine::new();
    e.mode = ViMode::Command;
    assert_eq!(
        e.resolve_command_key(key(';')),
        ViOutcome::Cmd(ViCmd::Bell, 1)
    );
}

// ── vim flavor resolution grammar ──────────────────────────────────

fn vim_engine() -> ViEngine {
    let mut e = ViEngine::new();
    e.mode = ViMode::Command;
    e.flavor = ViFlavor::Vim;
    e
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

#[test]
fn vim_v_enters_visual_and_y_is_linewise() {
    let mut e = vim_engine();
    assert_eq!(
        e.resolve_command_key(key('v')),
        ViOutcome::Cmd(ViCmd::EnterVisual(VisualKind::Char), 1)
    );
    assert_eq!(
        e.resolve_command_key(key('V')),
        ViOutcome::Cmd(ViCmd::EnterVisual(VisualKind::Line), 1)
    );
    assert_eq!(
        e.resolve_command_key(key('Y')),
        ViOutcome::Cmd(ViCmd::OpLine(OpKind::Yank), 1)
    );
    // Count on v is ignored (§12).
    assert_eq!(e.resolve_command_key(key('3')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('v')),
        ViOutcome::Cmd(ViCmd::EnterVisual(VisualKind::Char), 1)
    );
}

#[test]
fn vim_text_object_pending_grammar() {
    let mut e = vim_engine();
    // d i w with a count typed before the operator.
    assert_eq!(e.resolve_command_key(key('2')), ViOutcome::Pending);
    assert_eq!(e.resolve_command_key(key('d')), ViOutcome::Pending);
    assert_eq!(e.resolve_command_key(key('a')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('w')),
        ViOutcome::Cmd(
            ViCmd::OpObject {
                op: OpKind::Delete,
                around: true,
                obj: 'w'
            },
            2
        )
    );
    // ci" resolves with around=false.
    assert_eq!(e.resolve_command_key(key('c')), ViOutcome::Pending);
    assert_eq!(e.resolve_command_key(key('i')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('"')),
        ViOutcome::Cmd(
            ViCmd::OpObject {
                op: OpKind::Change,
                around: false,
                obj: '"'
            },
            1
        )
    );
}

#[test]
fn vim_g_prefix_grammar() {
    let mut e = vim_engine();
    assert_eq!(e.resolve_command_key(key('g')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('e')),
        ViOutcome::Cmd(ViCmd::Move(ViMotion::WordEndBack { big: false }), 1)
    );
    // Under an operator, with counts multiplying.
    assert_eq!(e.resolve_command_key(key('2')), ViOutcome::Pending);
    assert_eq!(e.resolve_command_key(key('d')), ViOutcome::Pending);
    assert_eq!(e.resolve_command_key(key('g')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('E')),
        ViOutcome::Cmd(
            ViCmd::Op(OpKind::Delete, ViMotion::WordEndBack { big: true }),
            2
        )
    );
    // g + unknown → bell.
    assert_eq!(e.resolve_command_key(key('g')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('z')),
        ViOutcome::Cmd(ViCmd::Bell, 1)
    );
}

#[test]
fn vim_percent_resolution() {
    let mut e = vim_engine();
    assert_eq!(
        e.resolve_command_key(key('%')),
        ViOutcome::Cmd(ViCmd::Move(ViMotion::MatchPair), 1)
    );
    assert_eq!(e.resolve_command_key(key('d')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('%')),
        ViOutcome::Cmd(ViCmd::Op(OpKind::Delete, ViMotion::MatchPair), 1)
    );
    // POSIX flavor: % stays a bell.
    let mut p = ViEngine::new();
    p.mode = ViMode::Command;
    assert_eq!(
        p.resolve_command_key(key('%')),
        ViOutcome::Cmd(ViCmd::Bell, 1)
    );
}

#[test]
fn vim_ctrl_x_chord() {
    let mut e = vim_engine();
    assert_eq!(e.resolve_command_key(ctrl('x')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(ctrl('e')),
        ViOutcome::Cmd(ViCmd::EditBuffer, 1)
    );
    // Broken chord bells.
    assert_eq!(e.resolve_command_key(ctrl('x')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(key('q')),
        ViOutcome::Cmd(ViCmd::Bell, 1)
    );
    // POSIX flavor: Ctrl-X is not a prefix.
    let mut p = ViEngine::new();
    p.mode = ViMode::Command;
    assert_eq!(
        p.resolve_command_key(ctrl('x')),
        ViOutcome::Cmd(ViCmd::Bell, 1)
    );
}

#[test]
fn vim_ctrl_r_is_redo_with_count() {
    let mut e = vim_engine();
    assert_eq!(e.resolve_command_key(key('3')), ViOutcome::Pending);
    assert_eq!(
        e.resolve_command_key(ctrl('r')),
        ViOutcome::Cmd(ViCmd::Redo, 3)
    );
    // POSIX flavor keeps fuzzy history search.
    let mut p = ViEngine::new();
    p.mode = ViMode::Command;
    assert_eq!(
        p.resolve_command_key(ctrl('r')),
        ViOutcome::Cmd(ViCmd::FuzzySearch, 1)
    );
}

#[test]
fn vim_visual_resolution_basics() {
    use VisualCmd as VC;
    let mut e = vim_engine();
    e.mode = ViMode::Visual {
        kind: VisualKind::Char,
        anchor: 0,
    };
    assert_eq!(
        e.resolve_visual_key(key('d')),
        VisualOutcome::Cmd(VC::Delete, 1)
    );
    assert_eq!(
        e.resolve_visual_key(key('o')),
        VisualOutcome::Cmd(VC::SwapEnds, 1)
    );
    // i + object.
    assert_eq!(e.resolve_visual_key(key('i')), VisualOutcome::Pending);
    assert_eq!(
        e.resolve_visual_key(key('w')),
        VisualOutcome::Cmd(
            VC::TextObject {
                around: false,
                obj: 'w'
            },
            1
        )
    );
    // Esc with a pending prefix cancels only the prefix.
    assert_eq!(e.resolve_visual_key(key('f')), VisualOutcome::Pending);
    assert_eq!(
        e.resolve_visual_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        VisualOutcome::Pending
    );
    // Bare Esc exits.
    assert_eq!(
        e.resolve_visual_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        VisualOutcome::Cmd(VC::Exit, 1)
    );
    // j/k resolve to pure buffer line motion.
    assert_eq!(
        e.resolve_visual_key(key('k')),
        VisualOutcome::Cmd(VC::LineUp, 1)
    );
}
