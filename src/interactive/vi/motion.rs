//! Motion types and the motion-target math: pure functions over
//! `(&[char], pos)` so they can be unit-tested without a terminal.

use crate::interactive::vim;

/// Direction/kind of a pending `f`/`F`/`t`/`T` character search.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FindKind {
    /// `f` — to the count-th occurrence after the cursor.
    Find,
    /// `F` — to the count-th occurrence before the cursor.
    FindBack,
    /// `t` — to just before the count-th occurrence after the cursor.
    To,
    /// `T` — to just after the count-th occurrence before the cursor.
    ToBack,
}

impl FindKind {
    pub(super) fn reversed(self) -> Self {
        match self {
            Self::Find => Self::FindBack,
            Self::FindBack => Self::Find,
            Self::To => Self::ToBack,
            Self::ToBack => Self::To,
        }
    }
}

/// A cursor motion (also the object of `d`/`c`/`y` operators).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViMotion {
    /// `l` / `<space>`
    CharForward,
    /// `h`
    CharBack,
    /// `w` / `W`
    WordForward { big: bool },
    /// `e` / `E`
    WordEnd { big: bool },
    /// `b` / `B`
    WordBack { big: bool },
    /// `0`
    LineStart,
    /// `^`
    FirstNonBlank,
    /// `$`
    LineEnd,
    /// `|` — column `count` (1-based)
    Column,
    /// `f c` / `F c` / `t c` / `T c` (and `;` / `,` after the engine
    /// substitutes the remembered kind/char)
    FindChar(FindKind, char),
    /// vim `%` — jump to the matching `( ) [ ] { }`. Inclusive under an
    /// operator; count ignored.
    MatchPair,
    /// vim `ge` / `gE` — end of the previous word / WORD. Inclusive
    /// under an operator.
    WordEndBack { big: bool },
}

// ---------------------------------------------------------------------------
// Motion-target math (pure functions over the buffer)
// ---------------------------------------------------------------------------

/// Start of the logical line containing `pos`.
pub fn line_start(buf: &[char], pos: usize) -> usize {
    buf[..pos]
        .iter()
        .rposition(|&c| c == '\n')
        .map(|i| i + 1)
        .unwrap_or(0)
}

/// End (exclusive; index of `'\n'` or `buf.len()`) of the logical line
/// containing `pos`.
pub fn line_end(buf: &[char], pos: usize) -> usize {
    buf[pos..]
        .iter()
        .position(|&c| c == '\n')
        .map(|i| pos + i)
        .unwrap_or(buf.len())
}

/// vi blank: space or tab (newline is a line separator, not a blank).
pub fn is_blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// vi character class: 0 = blank (incl. newline), 1 = word char
/// (alphanumeric or underscore), 2 = other punctuation. Bigwords
/// collapse classes 1 and 2.
pub(crate) fn char_class(c: char, big: bool) -> u8 {
    if is_blank(c) || c == '\n' {
        0
    } else if big || c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

/// One `w` step: start of the next word. `None` when the cursor cannot
/// advance (already in the last word's trailing position / at end).
fn next_word_start(buf: &[char], pos: usize, big: bool) -> Option<usize> {
    let len = buf.len();
    if pos >= len {
        return None;
    }
    let mut i = pos;
    let c0 = char_class(buf[i], big);
    if c0 != 0 {
        while i < len && char_class(buf[i], big) == c0 {
            i += 1;
        }
    }
    while i < len && char_class(buf[i], big) == 0 {
        i += 1;
    }
    if i == pos { None } else { Some(i) }
}

/// One `e` step: end of the current word, or of the next word when the
/// cursor already sits on a word end.
fn word_end_after(buf: &[char], pos: usize, big: bool) -> Option<usize> {
    let len = buf.len();
    let mut i = pos.checked_add(1)?;
    if i >= len {
        return None;
    }
    while i < len && char_class(buf[i], big) == 0 {
        i += 1;
    }
    if i >= len {
        return None;
    }
    let c = char_class(buf[i], big);
    while i + 1 < len && char_class(buf[i + 1], big) == c {
        i += 1;
    }
    Some(i)
}

/// One `b` step: start of the current word, or of the previous word
/// when the cursor already sits on a word start.
fn prev_word_start(buf: &[char], pos: usize, big: bool) -> Option<usize> {
    if pos == 0 {
        return None;
    }
    let mut i = pos - 1;
    while char_class(buf[i], big) == 0 {
        if i == 0 {
            return None;
        }
        i -= 1;
    }
    let c = char_class(buf[i], big);
    while i > 0 && char_class(buf[i - 1], big) == c {
        i -= 1;
    }
    Some(i)
}

/// Index of the first non-blank character of the cursor's logical line
/// (line start when the line is all blank).
fn first_non_blank(buf: &[char], pos: usize) -> usize {
    let ls = line_start(buf, pos);
    let le = line_end(buf, pos);
    buf[ls..le]
        .iter()
        .position(|&c| !is_blank(c))
        .map(|i| ls + i)
        .unwrap_or(ls)
}

/// Index of the count-th occurrence of `target` for `f`/`F`/`t`/`T`,
/// scoped to the cursor's logical line.
fn find_char_index(
    buf: &[char],
    pos: usize,
    kind: FindKind,
    target: char,
    count: u32,
) -> Option<usize> {
    let ls = line_start(buf, pos);
    let le = line_end(buf, pos);
    match kind {
        FindKind::Find | FindKind::To => {
            let mut i = pos;
            for _ in 0..count.max(1) {
                // An empty line (or cursor at/after line end) has no
                // "after the cursor" region to search.
                if i + 1 > le {
                    return None;
                }
                i = buf[i + 1..le].iter().position(|&c| c == target)? + i + 1;
            }
            Some(i)
        }
        FindKind::FindBack | FindKind::ToBack => {
            let mut i = pos;
            for _ in 0..count.max(1) {
                if i <= ls {
                    return None;
                }
                i = buf[ls..i].iter().rposition(|&c| c == target)? + ls;
            }
            Some(i)
        }
    }
}

/// Cursor destination for a plain (non-operator) motion. `None` = the
/// motion cannot move the cursor (alert). Forward motions cap at the
/// last character; command-mode cursors rest *on* a character, so the
/// destination never equals a non-empty buffer's length.
pub fn motion_move(buf: &[char], pos: usize, motion: ViMotion, count: u32) -> Option<usize> {
    let count = count.max(1) as usize;
    let ls = line_start(buf, pos);
    let le = line_end(buf, pos);
    let line_last = if le > ls { le - 1 } else { ls };
    match motion {
        ViMotion::CharForward => {
            if pos >= line_last {
                return None;
            }
            Some((pos + count).min(line_last))
        }
        ViMotion::CharBack => {
            if pos <= ls {
                return None;
            }
            Some(pos.saturating_sub(count).max(ls))
        }
        ViMotion::WordForward { big } => {
            let mut p = pos;
            for step in 0..count {
                match next_word_start(buf, p, big) {
                    Some(np) if np < buf.len() => p = np,
                    // Landing exactly at (or past) buffer end caps the
                    // cursor at the last character.
                    _ => {
                        if step == 0 && pos + 1 >= buf.len() {
                            return None;
                        }
                        p = buf.len().saturating_sub(1);
                        break;
                    }
                }
            }
            if p == pos { None } else { Some(p) }
        }
        ViMotion::WordEnd { big } => {
            let mut p = pos;
            for step in 0..count {
                match word_end_after(buf, p, big) {
                    Some(np) => p = np,
                    None => {
                        if step == 0 {
                            return None;
                        }
                        break;
                    }
                }
            }
            Some(p)
        }
        ViMotion::WordBack { big } => {
            let mut p = pos;
            for step in 0..count {
                match prev_word_start(buf, p, big) {
                    Some(np) => p = np,
                    None => {
                        if step == 0 {
                            return None;
                        }
                        break;
                    }
                }
            }
            Some(p)
        }
        ViMotion::LineStart => Some(ls),
        ViMotion::FirstNonBlank => Some(first_non_blank(buf, pos)),
        ViMotion::LineEnd => Some(line_last),
        ViMotion::Column => Some((ls + count - 1).min(line_last)),
        ViMotion::FindChar(kind, target) => {
            let idx = find_char_index(buf, pos, kind, target, count as u32)?;
            match kind {
                FindKind::Find | FindKind::FindBack => Some(idx),
                FindKind::To => Some(idx.saturating_sub(1)),
                FindKind::ToBack => Some(idx + 1),
            }
        }
        ViMotion::MatchPair => vim::match_pair_target(buf, pos),
        ViMotion::WordEndBack { big } => {
            let mut p = pos;
            for step in 0..count {
                match vim::prev_word_end(buf, p, big) {
                    Some(np) => p = np,
                    None => {
                        if step == 0 {
                            return None;
                        }
                        break;
                    }
                }
            }
            Some(p)
        }
    }
}

/// Character range `[start, end)` a `d`/`c`/`y` operator applies to for
/// a motion. `None` = the motion cannot produce a range (alert); an
/// *empty* range is returned as-is (callers treat it as an alert too).
///
/// Encodes the vi/POSIX rules directly: forward-inclusive motions
/// (`e E f t $`) include their target character; forward-exclusive
/// motions (`l w |`) stop before it; backward motions never include the
/// character under the cursor.
pub fn motion_range(
    buf: &[char],
    pos: usize,
    motion: ViMotion,
    count: u32,
) -> Option<(usize, usize)> {
    let count = count.max(1) as usize;
    let ls = line_start(buf, pos);
    let le = line_end(buf, pos);
    match motion {
        ViMotion::CharForward => {
            if pos >= le {
                return None;
            }
            Some((pos, (pos + count).min(le)))
        }
        ViMotion::CharBack => {
            if pos <= ls {
                return None;
            }
            Some((pos.saturating_sub(count).max(ls), pos))
        }
        ViMotion::WordForward { big } => {
            if pos >= buf.len() {
                return None;
            }
            let mut p = pos;
            for _ in 0..count {
                match next_word_start(buf, p, big) {
                    Some(np) if np < buf.len() => p = np,
                    // No further word: the operator consumes the rest of
                    // the buffer (`dw` on the last word deletes to the
                    // end).
                    _ => {
                        p = buf.len();
                        break;
                    }
                }
            }
            if p == pos { None } else { Some((pos, p)) }
        }
        ViMotion::WordEnd { big } => {
            let mut p = pos;
            let mut moved = false;
            for _ in 0..count {
                match word_end_after(buf, p, big) {
                    Some(np) => {
                        p = np;
                        moved = true;
                    }
                    None => break,
                }
            }
            if moved { Some((pos, p + 1)) } else { None }
        }
        ViMotion::WordBack { big } => {
            let mut p = pos;
            let mut moved = false;
            for _ in 0..count {
                match prev_word_start(buf, p, big) {
                    Some(np) => {
                        p = np;
                        moved = true;
                    }
                    None => break,
                }
            }
            if moved { Some((p, pos)) } else { None }
        }
        ViMotion::LineStart => {
            if pos > ls {
                Some((ls, pos))
            } else {
                None
            }
        }
        ViMotion::FirstNonBlank => {
            let f = first_non_blank(buf, pos);
            match f.cmp(&pos) {
                std::cmp::Ordering::Less => Some((f, pos)),
                std::cmp::Ordering::Greater => Some((pos, f)),
                std::cmp::Ordering::Equal => None,
            }
        }
        ViMotion::LineEnd => {
            if pos < le {
                Some((pos, le))
            } else {
                None
            }
        }
        ViMotion::Column => {
            let line_last = if le > ls { le - 1 } else { ls };
            let t = (ls + count - 1).min(line_last);
            match t.cmp(&pos) {
                std::cmp::Ordering::Less => Some((t, pos)),
                std::cmp::Ordering::Greater => Some((pos, t)),
                std::cmp::Ordering::Equal => None,
            }
        }
        ViMotion::FindChar(kind, target) => {
            let idx = find_char_index(buf, pos, kind, target, count as u32)?;
            match kind {
                FindKind::Find => Some((pos, idx + 1)),
                FindKind::To => Some((pos, idx)),
                FindKind::FindBack => Some((idx, pos)),
                FindKind::ToBack => Some((idx + 1, pos)),
            }
        }
        // Inclusive in both directions (Vim semantics).
        ViMotion::MatchPair => {
            let t = vim::match_pair_target(buf, pos)?;
            Some((pos.min(t), pos.max(t) + 1))
        }
        // Inclusive backward motion: `dge` deletes through the cursor
        // character.
        ViMotion::WordEndBack { big } => {
            let mut p = pos;
            let mut moved = false;
            for _ in 0..count {
                match vim::prev_word_end(buf, p, big) {
                    Some(np) => {
                        p = np;
                        moved = true;
                    }
                    None => break,
                }
            }
            if moved { Some((p, pos + 1)) } else { None }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn word_motions_basic() {
        let b = chars("echo hello world");
        assert_eq!(
            motion_move(&b, 0, ViMotion::WordForward { big: false }, 1),
            Some(5)
        );
        assert_eq!(
            motion_move(&b, 0, ViMotion::WordForward { big: false }, 2),
            Some(11)
        );
        assert_eq!(
            motion_move(&b, 5, ViMotion::WordBack { big: false }, 1),
            Some(0)
        );
        assert_eq!(
            motion_move(&b, 0, ViMotion::WordEnd { big: false }, 1),
            Some(3)
        );
        // e at word end jumps to the next word's end.
        assert_eq!(
            motion_move(&b, 3, ViMotion::WordEnd { big: false }, 1),
            Some(9)
        );
    }

    #[test]
    fn word_vs_bigword_punctuation() {
        let b = chars("a=b c");
        // word: 'a' / '=' / 'b' are three words.
        assert_eq!(
            motion_move(&b, 0, ViMotion::WordForward { big: false }, 1),
            Some(1)
        );
        // bigword: 'a=b' is one bigword.
        assert_eq!(
            motion_move(&b, 0, ViMotion::WordForward { big: true }, 1),
            Some(4)
        );
    }

    #[test]
    fn word_forward_caps_at_last_char() {
        let b = chars("echo hi");
        // w from inside the last word: caps at last character.
        assert_eq!(
            motion_move(&b, 5, ViMotion::WordForward { big: false }, 1),
            Some(6)
        );
        // Already at the last character: alert.
        assert_eq!(
            motion_move(&b, 6, ViMotion::WordForward { big: false }, 1),
            None
        );
    }

    #[test]
    fn line_scoped_motions() {
        let b = chars("  echo hi");
        assert_eq!(motion_move(&b, 5, ViMotion::LineStart, 1), Some(0));
        assert_eq!(motion_move(&b, 5, ViMotion::FirstNonBlank, 1), Some(2));
        assert_eq!(motion_move(&b, 0, ViMotion::LineEnd, 1), Some(8));
        assert_eq!(motion_move(&b, 5, ViMotion::Column, 3), Some(2));
        assert_eq!(motion_move(&b, 0, ViMotion::Column, 100), Some(8));
    }

    #[test]
    fn char_motions_stay_in_logical_line() {
        let b = chars("ab\ncd");
        // l on 'b' (index 1): line end, alert.
        assert_eq!(motion_move(&b, 1, ViMotion::CharForward, 1), None);
        // h on 'c' (index 3): line start, alert.
        assert_eq!(motion_move(&b, 3, ViMotion::CharBack, 1), None);
        assert_eq!(motion_move(&b, 3, ViMotion::LineEnd, 1), Some(4));
        assert_eq!(motion_move(&b, 3, ViMotion::LineStart, 1), Some(3));
    }

    #[test]
    fn find_char_motions() {
        let b = chars("echo hello");
        let f = |pos, kind, c, n| motion_move(&b, pos, ViMotion::FindChar(kind, c), n);
        assert_eq!(f(0, FindKind::Find, 'l', 1), Some(7));
        assert_eq!(f(0, FindKind::Find, 'l', 2), Some(8));
        assert_eq!(f(0, FindKind::To, 'l', 1), Some(6));
        assert_eq!(f(9, FindKind::FindBack, 'e', 1), Some(6));
        assert_eq!(f(9, FindKind::ToBack, 'e', 1), Some(7));
        assert_eq!(f(0, FindKind::Find, 'z', 1), None);
    }

    #[test]
    fn motion_range_word_forward() {
        let b = chars("echo hello world");
        // dw from 0: up to start of "hello" (exclusive).
        assert_eq!(
            motion_range(&b, 0, ViMotion::WordForward { big: false }, 1),
            Some((0, 5))
        );
        // dw on the last word: consumes to the end.
        assert_eq!(
            motion_range(&b, 11, ViMotion::WordForward { big: false }, 1),
            Some((11, 16))
        );
    }

    #[test]
    fn motion_range_inclusive_motions() {
        let b = chars("echo hello");
        // de: includes the word's last char.
        assert_eq!(
            motion_range(&b, 0, ViMotion::WordEnd { big: false }, 1),
            Some((0, 4))
        );
        // d$: includes the line's last char.
        assert_eq!(motion_range(&b, 5, ViMotion::LineEnd, 1), Some((5, 10)));
        // dfl: includes the found char.
        assert_eq!(
            motion_range(&b, 0, ViMotion::FindChar(FindKind::Find, 'l'), 1),
            Some((0, 8))
        );
        // dtl: excludes the found char.
        assert_eq!(
            motion_range(&b, 0, ViMotion::FindChar(FindKind::To, 'l'), 1),
            Some((0, 7))
        );
    }

    #[test]
    fn motion_range_backward_excludes_cursor() {
        let b = chars("echo hello");
        // db from 'h' of hello: deletes "echo " but not 'h'.
        assert_eq!(
            motion_range(&b, 5, ViMotion::WordBack { big: false }, 1),
            Some((0, 5))
        );
        assert_eq!(motion_range(&b, 5, ViMotion::LineStart, 1), Some((0, 5)));
        // d0 at line start: nothing to do.
        assert_eq!(motion_range(&b, 0, ViMotion::LineStart, 1), None);
    }
}
