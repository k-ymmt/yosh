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

// ---------------------------------------------------------------------------
// `%` match pair and `ge`/`gE` motion math
// ---------------------------------------------------------------------------

/// True when the unescaped-quote count before `idx` on its logical line
/// is odd for either quote character — Vim's `findmatchlimit()`-style
/// in-string heuristic used by `%` (oracle-verified for `"` and `'`).
fn in_string(buf: &[char], idx: usize) -> bool {
    let ls = vi::line_start(buf, idx);
    let mut dq = 0usize;
    let mut sq = 0usize;
    for i in ls..idx {
        if quote_escaped(buf, ls, i) {
            continue;
        }
        match buf[i] {
            '"' => dq += 1,
            '\'' => sq += 1,
            _ => {}
        }
    }
    dq % 2 == 1 || sq % 2 == 1
}

/// True when `buf[i]` is preceded by an odd number of backslashes
/// (bounded by `ls`) — the `quoteescape` default.
fn quote_escaped(buf: &[char], ls: usize, i: usize) -> bool {
    let mut n = 0usize;
    let mut j = i;
    while j > ls && buf[j - 1] == '\\' {
        n += 1;
        j -= 1;
    }
    n % 2 == 1
}

/// `%`: scan forward from the cursor to the end of the logical line for
/// the first of `( ) [ ] { }` and return the index of its match
/// (whole-buffer scan with nesting; bracket characters inside quoted
/// strings are skipped). `None` = no pair char on the rest of the line,
/// or no match (bell).
pub fn match_pair_target(buf: &[char], pos: usize) -> Option<usize> {
    if buf.is_empty() || pos >= buf.len() {
        return None;
    }
    let le = vi::line_end(buf, pos);
    let origin = (pos..le).find(|&i| {
        matches!(buf[i], '(' | ')' | '[' | ']' | '{' | '}') && !in_string(buf, i)
    })?;
    let (open, close, forward) = match buf[origin] {
        '(' => ('(', ')', true),
        ')' => ('(', ')', false),
        '[' => ('[', ']', true),
        ']' => ('[', ']', false),
        '{' => ('{', '}', true),
        _ => ('{', '}', false),
    };
    let mut depth = 0i32;
    if forward {
        for i in origin..buf.len() {
            if i != origin && in_string(buf, i) {
                continue;
            }
            if buf[i] == open {
                depth += 1;
            } else if buf[i] == close {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }
        None
    } else {
        for i in (0..=origin).rev() {
            if i != origin && in_string(buf, i) {
                continue;
            }
            if buf[i] == close {
                depth += 1;
            } else if buf[i] == open {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }
        None
    }
}

/// One `ge`/`gE` step: the end of the previous word / WORD strictly
/// before the cursor. `None` when there is none.
pub fn prev_word_end(buf: &[char], pos: usize, big: bool) -> Option<usize> {
    let mut i = pos.checked_sub(1)?;
    loop {
        let c = vi::char_class(buf[i], big);
        if c != 0 {
            let next = buf
                .get(i + 1)
                .map(|&ch| vi::char_class(ch, big))
                .unwrap_or(0);
            if next != c {
                return Some(i);
            }
        }
        if i == 0 {
            return None;
        }
        i -= 1;
    }
}

// ---------------------------------------------------------------------------
// Text objects (§6)
// ---------------------------------------------------------------------------

/// Range of the text object `obj` at `pos` (end-exclusive). `around`
/// distinguishes `a`-objects from `i`-objects. `None` = no such object
/// (bell); an *empty* range is a valid empty inner object (`i(` on
/// `()`): deletes/yanks are bell-free no-ops, `c` enters Insert inside.
pub fn text_object_range(
    buf: &[char],
    pos: usize,
    obj: char,
    around: bool,
    count: u32,
) -> Option<(usize, usize)> {
    if buf.is_empty() {
        return None;
    }
    let pos = pos.min(buf.len() - 1);
    match obj {
        'w' => word_object_range(buf, pos, false, around, count),
        'W' => word_object_range(buf, pos, true, around, count),
        // Count on quote objects is ignored (Vim ignores it too).
        '"' | '\'' | '`' => quote_object_range(buf, pos, obj, around),
        '(' | ')' | 'b' => bracket_object_range(buf, pos, '(', ')', around),
        '[' | ']' => bracket_object_range(buf, pos, '[', ']', around),
        '{' | '}' | 'B' => bracket_object_range(buf, pos, '{', '}', around),
        '<' | '>' => bracket_object_range(buf, pos, '<', '>', around),
        _ => None,
    }
}

/// Bounds of the maximal same-class run containing `i`.
fn run_bounds(buf: &[char], i: usize, big: bool) -> (usize, usize) {
    let c = vi::char_class(buf[i], big);
    let mut s = i;
    while s > 0 && vi::char_class(buf[s - 1], big) == c {
        s -= 1;
    }
    let mut e = i + 1;
    while e < buf.len() && vi::char_class(buf[e], big) == c {
        e += 1;
    }
    (s, e)
}

/// `iw`/`aw` (and `iW`/`aW`): word objects. `iw` counts word and blank
/// runs as separate objects; `aw` is word + trailing whitespace (or
/// leading when there is none trailing — the Vim rule), `count` words.
fn word_object_range(
    buf: &[char],
    pos: usize,
    big: bool,
    around: bool,
    count: u32,
) -> Option<(usize, usize)> {
    let len = buf.len();
    let (mut s, mut e) = run_bounds(buf, pos, big);
    if !around {
        // [count]iw: extend over count-1 further runs (blank runs are
        // objects of their own, matching Vim).
        for _ in 1..count.max(1) {
            if e >= len {
                break;
            }
            e = run_bounds(buf, e, big).1;
        }
        return Some((s, e));
    }
    let mut trailing = false;
    if vi::char_class(buf[pos], big) == 0 {
        // aw on whitespace: the blank run plus the following word.
        if e < len {
            e = run_bounds(buf, e, big).1;
        }
    } else if e < len && vi::char_class(buf[e], big) == 0 {
        e = run_bounds(buf, e, big).1;
        trailing = true;
    }
    for _ in 1..count.max(1) {
        if e >= len {
            break;
        }
        e = run_bounds(buf, e, big).1;
        if e < len && vi::char_class(buf[e], big) == 0 {
            e = run_bounds(buf, e, big).1;
            trailing = true;
        }
    }
    if !trailing {
        while s > 0 && vi::char_class(buf[s - 1], big) == 0 {
            s -= 1;
        }
    }
    Some((s, e))
}

/// `i"`/`a"` (and `'`, `` ` ``): quoted string on the current logical
/// line. Backslash-escaped quotes are skipped. When the cursor is
/// before the first quote, the object is the next quoted span (Vim
/// behavior). `a` includes trailing whitespace after the closing quote,
/// or leading whitespace before the opening quote when none trails.
fn quote_object_range(buf: &[char], pos: usize, q: char, around: bool) -> Option<(usize, usize)> {
    let ls = vi::line_start(buf, pos);
    let le = vi::line_end(buf, pos);
    let quotes: Vec<usize> = (ls..le)
        .filter(|&i| buf[i] == q && !quote_escaped(buf, ls, i))
        .collect();
    for pair in quotes.chunks(2) {
        let [o, c] = *pair else { break };
        if pos <= c {
            return Some(if around {
                let mut e = c + 1;
                let mut trailing = false;
                while e < le && vi::is_blank(buf[e]) {
                    e += 1;
                    trailing = true;
                }
                let mut s = o;
                if !trailing {
                    while s > ls && vi::is_blank(buf[s - 1]) {
                        s -= 1;
                    }
                }
                (s, e)
            } else {
                (o + 1, c)
            });
        }
    }
    None
}

/// The match of the opening bracket at `from`, scanning forward with
/// nesting (whole buffer — bracket blocks may span `'\n'`).
fn bracket_match_forward(buf: &[char], from: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0i32;
    for (i, &c) in buf.iter().enumerate().skip(from) {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// The match of the closing bracket at `from`, scanning backward.
fn bracket_match_backward(buf: &[char], from: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0i32;
    for i in (0..=from).rev() {
        if buf[i] == close {
            depth += 1;
        } else if buf[i] == open {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// `i(`/`a(` and friends: bracket block with the cursor on a bracket or
/// inside the pair; nested pairs resolved by matching; multiline.
fn bracket_object_range(
    buf: &[char],
    pos: usize,
    open: char,
    close: char,
    around: bool,
) -> Option<(usize, usize)> {
    let (o, c) = if buf[pos] == open {
        (pos, bracket_match_forward(buf, pos, open, close)?)
    } else if buf[pos] == close {
        (bracket_match_backward(buf, pos, open, close)?, pos)
    } else {
        // Enclosing pair: nearest unmatched opening bracket before the
        // cursor, and its forward match (which must lie at/after it).
        let mut depth = 0i32;
        let mut found = None;
        for i in (0..pos).rev() {
            if buf[i] == close {
                depth += 1;
            } else if buf[i] == open {
                if depth == 0 {
                    found = Some(i);
                    break;
                }
                depth -= 1;
            }
        }
        let o = found?;
        let c = bracket_match_forward(buf, o, open, close)?;
        if c < pos {
            return None;
        }
        (o, c)
    };
    Some(if around { (o, c + 1) } else { (o + 1, c) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn match_pair_basic_and_nested() {
        let b = chars("a (b [c] d) e");
        assert_eq!(match_pair_target(&b, 0), Some(10)); // first pair on line: ( at 2 → ) at 10
        assert_eq!(match_pair_target(&b, 2), Some(10));
        assert_eq!(match_pair_target(&b, 10), Some(2));
        assert_eq!(match_pair_target(&b, 5), Some(7)); // [ at 5 → ] at 7
    }

    #[test]
    fn match_pair_skips_quoted_brackets() {
        // Oracle-verified: 0%x on `{ "}" }` deletes the final }, not the
        // quoted one; same with single quotes.
        let b = chars("{ \"}\" }");
        assert_eq!(match_pair_target(&b, 0), Some(6));
        let b = chars("{ '}' }");
        assert_eq!(match_pair_target(&b, 0), Some(6));
    }

    #[test]
    fn match_pair_no_pair_on_line_is_none() {
        let b = chars("abc");
        assert_eq!(match_pair_target(&b, 0), None);
        let b = chars("(a\nb)");
        // From line 2 there is a pair char on the line...
        assert_eq!(match_pair_target(&b, 3), Some(0)); // ) at 4 → ( at 0
    }

    #[test]
    fn prev_word_end_basic() {
        let b = chars("one two three");
        assert_eq!(prev_word_end(&b, 8, false), Some(6)); // from 't' of three...
        assert_eq!(prev_word_end(&b, 4, false), Some(2));
        assert_eq!(prev_word_end(&b, 2, false), None);
        assert_eq!(prev_word_end(&b, 0, false), None);
    }

    #[test]
    fn prev_word_end_punctuation_runs() {
        let b = chars("a=b c");
        // From 'c' (4): previous word end is 'b' (2).
        assert_eq!(prev_word_end(&b, 4, false), Some(2));
        // From 'b' (2): '=' run ends at 1 (word vs punct are separate).
        assert_eq!(prev_word_end(&b, 2, false), Some(1));
        // Bigword: "a=b" is one WORD; from 'c' its end is 2.
        assert_eq!(prev_word_end(&b, 4, true), Some(2));
        assert_eq!(prev_word_end(&b, 2, true), None);
    }

    #[test]
    fn word_object_iw_aw() {
        let b = chars("one two three");
        // iw mid-word.
        assert_eq!(text_object_range(&b, 5, 'w', false, 1), Some((4, 7)));
        // aw includes trailing whitespace.
        assert_eq!(text_object_range(&b, 5, 'w', true, 1), Some((4, 8)));
        // aw on the last word includes leading whitespace instead.
        assert_eq!(text_object_range(&b, 9, 'w', true, 1), Some((7, 13)));
        // iw on whitespace selects the blank run.
        assert_eq!(text_object_range(&b, 3, 'w', false, 1), Some((3, 4)));
        // aw on whitespace: blanks + following word.
        assert_eq!(text_object_range(&b, 3, 'w', true, 1), Some((3, 7)));
        // 2aw: two words with their trailing whitespace.
        assert_eq!(text_object_range(&b, 0, 'w', true, 2), Some((0, 8)));
        // 2iw: word + following blank run.
        assert_eq!(text_object_range(&b, 0, 'w', false, 2), Some((0, 4)));
    }

    #[test]
    fn word_object_bigword() {
        let b = chars("a=b c");
        assert_eq!(text_object_range(&b, 1, 'W', false, 1), Some((0, 3)));
        assert_eq!(text_object_range(&b, 1, 'w', false, 1), Some((1, 2)));
    }

    #[test]
    fn quote_object_inner_and_around() {
        let b = chars("say \"hi there\" now");
        assert_eq!(text_object_range(&b, 7, '"', false, 1), Some((5, 13)));
        // a" includes the trailing space after the closing quote.
        assert_eq!(text_object_range(&b, 7, '"', true, 1), Some((4, 15)));
    }

    #[test]
    fn quote_object_cursor_before_first_quote() {
        let b = chars("say \"hi\"");
        // Cursor before the first quote: the next quoted span.
        assert_eq!(text_object_range(&b, 0, '"', false, 1), Some((5, 7)));
    }

    #[test]
    fn quote_object_skips_escaped_quotes() {
        let b = chars("\"a\\\"b\" c");
        // The escaped quote at 3 is not a delimiter: i" spans a\"b.
        assert_eq!(text_object_range(&b, 2, '"', false, 1), Some((1, 5)));
    }

    #[test]
    fn quote_object_empty_inner() {
        let b = chars("x \"\" y");
        assert_eq!(text_object_range(&b, 2, '"', false, 1), Some((3, 3)));
    }

    #[test]
    fn quote_object_no_pair_is_none() {
        let b = chars("say \"hi");
        assert_eq!(text_object_range(&b, 0, '"', false, 1), None);
    }

    #[test]
    fn bracket_object_inner_around_nested() {
        let b = chars("f(a(b)c)");
        assert_eq!(text_object_range(&b, 4, '(', false, 1), Some((4, 5)));
        assert_eq!(text_object_range(&b, 2, '(', false, 1), Some((2, 7)));
        assert_eq!(text_object_range(&b, 2, '(', true, 1), Some((1, 8)));
        // On the bracket itself.
        assert_eq!(text_object_range(&b, 1, 'b', false, 1), Some((2, 7)));
        assert_eq!(text_object_range(&b, 7, 'b', false, 1), Some((2, 7)));
    }

    #[test]
    fn bracket_object_multiline() {
        let b = chars("f(a\nb)");
        assert_eq!(text_object_range(&b, 2, '(', false, 1), Some((2, 5)));
    }

    #[test]
    fn bracket_object_empty_inner() {
        let b = chars("f()");
        assert_eq!(text_object_range(&b, 1, '(', false, 1), Some((2, 2)));
        assert_eq!(text_object_range(&b, 2, '(', false, 1), Some((2, 2)));
    }

    #[test]
    fn bracket_object_unmatched_is_none() {
        let b = chars("f(a");
        assert_eq!(text_object_range(&b, 2, '(', false, 1), None);
        let b = chars("abc");
        assert_eq!(text_object_range(&b, 1, '(', false, 1), None);
    }

    #[test]
    fn unknown_object_char_is_none() {
        let b = chars("abc");
        assert_eq!(text_object_range(&b, 0, 'q', false, 1), None);
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
