//! Anchored prefix / suffix search (`${v#pat}`, `${v%%pat}`, …): the
//! cut point a pattern selects, found in O(n·m) by segment placement.

use super::{CompiledPattern, PatternToken};

impl CompiledPattern {
    /// The shortest (or, with `longest`, the longest) prefix of `s` the
    /// pattern matches, as the byte offset where that prefix ends; `None`
    /// when no prefix matches. Equivalent to testing
    /// `self.matches(&s[..end])` for every char boundary `end` in order,
    /// but O(n·m) instead of O(n²·m) (`${v#*[/]}` on a 100 KB value
    /// took minutes with the per-boundary scan).
    ///
    /// The pattern is `seg0 * seg1 * … * segk`, each segment a run of
    /// single-character tokens. `s[..end]` matches iff `seg0` matches at
    /// the start, the middle segments fit in order after it, and `segk`
    /// ends exactly at `end` after them. Placing each middle segment at
    /// its leftmost fit is optimal and does not depend on `end`, so one
    /// greedy pass fixes the earliest point `segk` may start, and the
    /// candidate ends are then exactly the places `segk` matches from
    /// there on.
    pub fn match_prefix(&self, s: &str, longest: bool) -> Option<usize> {
        let segs: Vec<&[PatternToken]> = self.tokens.split(is_star).collect();
        let (first, rest) = segs.split_first().expect("split yields one segment");
        let mut pos = match_segment(first, s, 0)?;
        let Some((last, middle)) = rest.split_last() else {
            // No `*`: the only matching prefix is `seg0` itself.
            return Some(pos);
        };
        for seg in middle {
            pos = leftmost_fit(seg, s, pos)?;
        }
        if longest {
            // The latest start of `segk` ends latest (fixed char count).
            let latest = back_chars(s, s.len(), last.len())?;
            let mut start = latest;
            loop {
                if start < pos {
                    return None;
                }
                if let Some(end) = match_segment(last, s, start) {
                    return Some(end);
                }
                if start == pos {
                    return None;
                }
                start = back_chars(s, start, 1)?;
            }
        } else {
            leftmost_fit(last, s, pos)
        }
    }

    /// The shortest (or, with `longest`, the longest) suffix of `s` the
    /// pattern matches, as the byte offset where that suffix starts;
    /// `None` when no suffix matches. The mirror image of
    /// [`Self::match_prefix`]: `segk` must end at the end of `s`, the
    /// middle segments are placed at their rightmost fits before it, and
    /// the candidate starts are where `seg0` matches ending no later than
    /// the leftmost of them.
    pub fn match_suffix(&self, s: &str, longest: bool) -> Option<usize> {
        let segs: Vec<&[PatternToken]> = self.tokens.split(is_star).collect();
        let (last, rest) = segs.split_last().expect("split yields one segment");
        let mut limit = back_chars(s, s.len(), last.len())?;
        match_segment(last, s, limit)?;
        let Some((first, middle)) = rest.split_first() else {
            // No `*`: the only matching suffix is `segk` itself.
            return Some(limit);
        };
        for seg in middle.iter().rev() {
            limit = rightmost_fit(seg, s, limit)?;
        }
        if longest {
            // The earliest start of `seg0` that still ends by `limit`.
            let mut start = 0;
            loop {
                match match_segment(first, s, start) {
                    Some(end) if end <= limit => return Some(start),
                    Some(_) => return None,
                    None => {}
                }
                if start >= limit {
                    return None;
                }
                start = next_char(s, start);
            }
        } else {
            rightmost_fit(first, s, limit)
        }
    }
}

fn is_star(t: &PatternToken) -> bool {
    matches!(t, PatternToken::Star)
}

/// Match the star-free `seg` against the chars of `s` starting at byte
/// offset `at`; the byte offset just past the match.
fn match_segment(seg: &[PatternToken], s: &str, at: usize) -> Option<usize> {
    let mut chars = s[at..].chars();
    let mut pos = at;
    for tok in seg {
        let c = chars.next()?;
        if !tok.matches_char(c) {
            return None;
        }
        pos += c.len_utf8();
    }
    Some(pos)
}

/// End of the leftmost match of `seg` starting at or after `from`.
fn leftmost_fit(seg: &[PatternToken], s: &str, from: usize) -> Option<usize> {
    let mut start = from;
    loop {
        if let Some(end) = match_segment(seg, s, start) {
            return Some(end);
        }
        if start >= s.len() {
            return None;
        }
        start = next_char(s, start);
    }
}

/// Start of the rightmost match of `seg` ending at or before `limit`.
fn rightmost_fit(seg: &[PatternToken], s: &str, limit: usize) -> Option<usize> {
    let mut start = back_chars(s, limit, seg.len())?;
    loop {
        if match_segment(seg, s, start).is_some() {
            return Some(start);
        }
        if start == 0 {
            return None;
        }
        start = back_chars(s, start, 1)?;
    }
}

/// Byte offset of the char boundary after the one at `at` (< `s.len()`).
fn next_char(s: &str, at: usize) -> usize {
    at + s[at..].chars().next().map_or(1, char::len_utf8)
}

/// Byte offset `n` chars before the char boundary `at`, or `None` when
/// fewer than `n` chars precede it.
fn back_chars(s: &str, at: usize, n: usize) -> Option<usize> {
    if n == 0 {
        return Some(at);
    }
    s[..at].char_indices().rev().nth(n - 1).map(|(i, _)| i)
}
