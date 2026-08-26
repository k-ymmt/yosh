// src/interactive/vim.rs

//! Vim-flavor editing logic for `set -o vim` (non-POSIX extension).
//!
//! Everything specific to Vim-editor semantics — the typed unnamed
//! register and the linewise range math — lives here, separate from the
//! POSIX vi machinery in `vi.rs`. `line_editor.rs` calls into this
//! module only when `ViEngine::flavor == ViFlavor::Vim`.

use super::vi::{self, VisualKind};

/// Whether register text is a character span or whole line(s).
/// Linewise text always carries one trailing `'\n'` per line.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RegisterKind {
    #[default]
    Charwise,
    Linewise,
}

/// The single typed unnamed register (Vim's `""`). Mirrors the kill
/// ring's front entry after every kill-ring write (front-entry
/// invariant), so text killed in emacs or POSIX-vi mode is `p`-puttable
/// after `set -o vim`.
#[derive(Clone, Debug, Default)]
pub struct UnnamedRegister {
    pub text: String,
    pub kind: RegisterKind,
}

/// Normalized end-exclusive char range of a VISUAL selection: the
/// inclusive `min(anchor, pos) ..= max(anchor, pos)` span, clamped onto
/// the buffer (empty range on an empty buffer), expanded to
/// logical-line boundaries for a linewise selection.
pub fn visual_selection(
    buf: &[char],
    anchor: usize,
    pos: usize,
    kind: VisualKind,
) -> (usize, usize) {
    if buf.is_empty() {
        return (0, 0);
    }
    let lo = anchor.min(pos).min(buf.len() - 1);
    let hi = anchor.max(pos).min(buf.len() - 1);
    match kind {
        VisualKind::Char => (lo, hi + 1),
        VisualKind::Line => (vi::line_start(buf, lo), vi::line_end(buf, hi)),
    }
}

/// Char range `[line_start(i), line_end(j))` of the `count` logical
/// lines starting at the cursor's line, clamped to the last line.
/// Separators between the lines are inside the range; the trailing
/// separator (if any) is not.
pub fn linewise_target(buf: &[char], pos: usize, count: u32) -> (usize, usize) {
    let ls = vi::line_start(buf, pos);
    let mut le = vi::line_end(buf, pos);
    for _ in 1..count.max(1) {
        if le >= buf.len() {
            break;
        }
        le = vi::line_end(buf, le + 1);
    }
    (ls, le)
}

/// Register text for a linewise operation over `[ls, le)`: the selected
/// lines joined with `'\n'` plus one trailing `'\n'` (synthesized for
/// the buffer's final line), regardless of which separator a delete
/// consumes.
pub fn linewise_register_text(buf: &[char], ls: usize, le: usize) -> String {
    let mut s: String = buf[ls..le].iter().collect();
    s.push('\n');
    s
}

/// Delete range for `dd` / linewise-VISUAL `d`: the line span plus one
/// consumed separator — the trailing one when a line follows, else the
/// preceding one, else none (whole buffer).
pub fn linewise_delete_range(buf: &[char], ls: usize, le: usize) -> (usize, usize) {
    if le < buf.len() {
        (ls, le + 1)
    } else if ls > 0 {
        (ls - 1, le)
    } else {
        (ls, le)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn visual_selection_charwise_inclusive_and_clamped() {
        let b = chars("abcde");
        assert_eq!(visual_selection(&b, 1, 3, VisualKind::Char), (1, 4));
        assert_eq!(visual_selection(&b, 3, 1, VisualKind::Char), (1, 4));
        assert_eq!(visual_selection(&b, 0, 0, VisualKind::Char), (0, 1));
        // Out-of-range positions clamp onto the last character.
        assert_eq!(visual_selection(&b, 0, 99, VisualKind::Char), (0, 5));
        // Empty buffer: the empty range.
        let e: Vec<char> = Vec::new();
        assert_eq!(visual_selection(&e, 0, 0, VisualKind::Char), (0, 0));
    }

    #[test]
    fn visual_selection_linewise_expands_to_line_boundaries() {
        let b = chars("aa\nbb\ncc");
        assert_eq!(visual_selection(&b, 4, 4, VisualKind::Line), (3, 5));
        assert_eq!(visual_selection(&b, 1, 6, VisualKind::Line), (0, 8));
        assert_eq!(visual_selection(&b, 6, 1, VisualKind::Line), (0, 8));
        // Linewise selection of an empty logical line selects it (an
        // empty range at the line position).
        let b = chars("a\n\nb");
        assert_eq!(visual_selection(&b, 2, 2, VisualKind::Line), (2, 2));
    }

    #[test]
    fn linewise_target_single_line() {
        let b = chars("abc");
        assert_eq!(linewise_target(&b, 1, 1), (0, 3));
        // Count clamps at the last line.
        assert_eq!(linewise_target(&b, 1, 5), (0, 3));
    }

    #[test]
    fn linewise_target_multiline_counts() {
        let b = chars("aa\nbb\ncc");
        assert_eq!(linewise_target(&b, 0, 1), (0, 2));
        assert_eq!(linewise_target(&b, 0, 2), (0, 5));
        assert_eq!(linewise_target(&b, 0, 3), (0, 8));
        assert_eq!(linewise_target(&b, 0, 9), (0, 8));
        // From the middle line.
        assert_eq!(linewise_target(&b, 3, 1), (3, 5));
        assert_eq!(linewise_target(&b, 3, 2), (3, 8));
    }

    #[test]
    fn linewise_target_empty_line() {
        let b = chars("a\n\nb");
        // The empty middle line selects itself.
        assert_eq!(linewise_target(&b, 2, 1), (2, 2));
    }

    #[test]
    fn register_text_appends_trailing_newline() {
        let b = chars("aa\nbb\ncc");
        assert_eq!(linewise_register_text(&b, 0, 2), "aa\n");
        assert_eq!(linewise_register_text(&b, 0, 5), "aa\nbb\n");
        assert_eq!(linewise_register_text(&b, 6, 8), "cc\n");
        // Empty line yields just the separator.
        let b = chars("a\n\nb");
        assert_eq!(linewise_register_text(&b, 2, 2), "\n");
    }

    #[test]
    fn delete_range_consumes_trailing_separator() {
        let b = chars("aa\nbb");
        assert_eq!(linewise_delete_range(&b, 0, 2), (0, 3));
    }

    #[test]
    fn delete_range_consumes_preceding_separator_at_tail() {
        let b = chars("aa\nbb");
        assert_eq!(linewise_delete_range(&b, 3, 5), (2, 5));
    }

    #[test]
    fn delete_range_whole_buffer_has_no_separator() {
        let b = chars("aa");
        assert_eq!(linewise_delete_range(&b, 0, 2), (0, 2));
        let b: Vec<char> = Vec::new();
        assert_eq!(linewise_delete_range(&b, 0, 0), (0, 0));
    }
}
