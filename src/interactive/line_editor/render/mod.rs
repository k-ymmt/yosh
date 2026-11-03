//! Rendering the edit buffer: the redraw dispatcher, render-state
//! bookkeeping, and the single-line / multiline painters.

mod multiline;
mod single_line;

use std::io;

use super::LineEditor;
use crate::interactive::highlight::{ColorSpan, HighlightStyle};
use crate::interactive::terminal::Terminal;
use crate::interactive::vi::ViMode;
use crate::interactive::vim;

/// Return the highlight style covering char index `i`, advancing `span_idx`
/// forward as needed.
///
/// `spans` must be sorted by `start` and non-overlapping (true of every
/// `HighlightScanner::scan` result — spans are pushed in left-to-right scan
/// order). Callers must invoke this with strictly increasing `i` across a
/// single pass and share the same `span_idx` across calls, mirroring how
/// `redraw` walks the buffer once per char. `span_idx` may end up pointing
/// past a span whose range doesn't include `i` (e.g. gaps between spans);
/// that's fine — the `Some(sp) if ...` guard falls through to `Default`
/// without advancing past spans that still might cover a later `i`.
fn style_at_advancing(spans: &[ColorSpan], span_idx: &mut usize, i: usize) -> HighlightStyle {
    while *span_idx < spans.len() && spans[*span_idx].end <= i {
        *span_idx += 1;
    }
    match spans.get(*span_idx) {
        Some(sp) if sp.start <= i && i < sp.end => sp.style,
        _ => HighlightStyle::Default,
    }
}

/// Find the first char index in `[0, limit)` where the highlight style
/// produced by `old_spans` differs from `new_spans`, or `limit` if they
/// agree on the whole range.
///
/// Used to cap the diff-based partial-repaint start column: even when the
/// characters in `[0, limit)` are identical between renders, a scanner
/// re-classification (e.g. a quote becoming closed) can change which style
/// applies to that unchanged text, and that region must still be repainted.
fn style_diff_pos(old_spans: &[ColorSpan], new_spans: &[ColorSpan], limit: usize) -> usize {
    if old_spans.is_empty() && new_spans.is_empty() {
        return limit;
    }
    let mut old_idx = 0;
    let mut new_idx = 0;
    for i in 0..limit {
        let old_style = style_at_advancing(old_spans, &mut old_idx, i);
        let new_style = style_at_advancing(new_spans, &mut new_idx, i);
        if old_style != new_style {
            return i;
        }
    }
    limit
}

impl LineEditor {
    /// Forget everything `redraw` knows about the on-screen state. Must be
    /// called whenever the screen is repainted outside `redraw`'s own
    /// bookkeeping (clear-screen, selector UIs, prompt reprints): those
    /// paths leave the cursor on the freshly printed prompt row, so the
    /// row counters reset to zero along with the render cache.
    pub(super) fn invalidate_render_state(&mut self) {
        self.prev_render = None;
        self.prev_total_rows = 0;
        self.prev_cursor_row = 0;
        self.prev_render_multiline = false;
        self.prev_render_had_selection = false;
        self.viewport_top = 0;
    }

    /// The active VISUAL selection as a non-empty end-exclusive char
    /// range for rendering, if any.
    pub(super) fn visual_selection_range(&self) -> Option<(usize, usize)> {
        match self.vi.mode {
            ViMode::Visual { kind, anchor } => {
                let (s, e) = vim::visual_selection(&self.buf, anchor, self.pos, kind);
                (s < e).then_some((s, e))
            }
            _ => None,
        }
    }

    /// Move the cursor up to the first content row and clear every row of
    /// the previous render, leaving the cursor at column 0 of the first
    /// row. The cursor sits on `prev_cursor_row` (not necessarily the
    /// last rendered row — e.g. after moving left across a wrap
    /// boundary). Shared by both full-repaint paths.
    pub(super) fn clear_prev_render<T: Terminal>(&mut self, term: &mut T) -> io::Result<()> {
        if self.prev_cursor_row > 0 {
            term.move_up(self.prev_cursor_row as u16)?;
        }
        term.move_to_column(0)?;
        for i in 0..=self.prev_total_rows {
            if i > 0 {
                term.move_down(1)?;
            }
            term.clear_current_line()?;
        }
        if self.prev_total_rows > 0 {
            term.move_up(self.prev_total_rows as u16)?;
        }
        term.move_to_column(0)?;
        Ok(())
    }

    /// Redraw dispatcher: while vi `/` / `?` pattern input is active the
    /// input line displays the search prompt and pattern in place of the
    /// buffer (the buffer itself is untouched and reappears when the
    /// search resolves or is cancelled).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn redraw<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        prompt_width: usize,
        cont_prompt: &str,
        spans: &[ColorSpan],
        term_width: u16,
        term_height: u16,
    ) -> io::Result<()> {
        if let Some((dir, pat)) = &self.vi_search_input {
            let display: Vec<char> = std::iter::once(dir.prompt_char())
                .chain(pat.chars())
                .collect();
            let saved_buf = std::mem::replace(&mut self.buf, display);
            let saved_pos = self.pos;
            let saved_suggestion = self.suggestion.take();
            self.pos = self.buf.len();
            self.invalidate_width_cache();
            let result = self.redraw_content(
                term,
                prompt,
                prompt_width,
                cont_prompt,
                &[],
                term_width,
                term_height,
            );
            self.buf = saved_buf;
            self.pos = saved_pos;
            self.suggestion = saved_suggestion;
            self.invalidate_width_cache();
            return result;
        }
        self.redraw_content(
            term,
            prompt,
            prompt_width,
            cont_prompt,
            spans,
            term_width,
            term_height,
        )
    }
}

#[cfg(test)]
mod redraw_helper_tests {
    use super::*;

    /// Reference implementation matching the pre-optimization behavior
    /// (`spans.iter().find(...)` per char): O(n*spans) but trivially
    /// correct, used as an oracle for `style_at_advancing`.
    fn naive_style_at(spans: &[ColorSpan], i: usize) -> HighlightStyle {
        spans
            .iter()
            .find(|sp| sp.start <= i && i < sp.end)
            .map(|sp| sp.style)
            .unwrap_or(HighlightStyle::Default)
    }

    fn span(start: usize, end: usize, style: HighlightStyle) -> ColorSpan {
        ColorSpan { start, end, style }
    }

    /// Assert that walking `len` chars with the advancing cursor produces
    /// the same style sequence as the naive per-char `find`.
    fn assert_equivalent(spans: &[ColorSpan], len: usize) {
        let mut span_idx = 0;
        for i in 0..len {
            let got = style_at_advancing(spans, &mut span_idx, i);
            let want = naive_style_at(spans, i);
            assert_eq!(got, want, "mismatch at i={i} for spans={spans:?}");
        }
    }

    #[test]
    fn empty_spans_all_default() {
        assert_equivalent(&[], 10);
    }

    #[test]
    fn single_span_covering_prefix() {
        let spans = vec![span(0, 4, HighlightStyle::CommandValid)];
        assert_equivalent(&spans, 10);
    }

    #[test]
    fn single_span_in_middle_with_gaps() {
        let spans = vec![span(3, 6, HighlightStyle::Variable)];
        assert_equivalent(&spans, 10);
    }

    #[test]
    fn adjacent_spans_no_gap() {
        let spans = vec![
            span(0, 4, HighlightStyle::CommandValid),
            span(4, 9, HighlightStyle::Default),
            span(9, 13, HighlightStyle::Operator),
        ];
        assert_equivalent(&spans, 15);
    }

    #[test]
    fn spans_with_gaps_between() {
        let spans = vec![
            span(0, 2, HighlightStyle::Keyword),
            span(5, 7, HighlightStyle::String),
            span(10, 12, HighlightStyle::Error),
        ];
        assert_equivalent(&spans, 15);
    }

    #[test]
    fn span_at_very_end() {
        let spans = vec![span(8, 10, HighlightStyle::Comment)];
        assert_equivalent(&spans, 10);
    }

    #[test]
    fn many_small_spans_pseudo_random_layout() {
        // Deterministic pseudo-random-ish layout: alternating short spans
        // and gaps of varying length, covering a range of styles.
        let styles = [
            HighlightStyle::CommandValid,
            HighlightStyle::CommandInvalid,
            HighlightStyle::Keyword,
            HighlightStyle::Operator,
            HighlightStyle::Redirect,
            HighlightStyle::String,
            HighlightStyle::DoubleString,
            HighlightStyle::Variable,
            HighlightStyle::CommandSub,
            HighlightStyle::ArithSub,
            HighlightStyle::Comment,
            HighlightStyle::Error,
            HighlightStyle::Assignment,
            HighlightStyle::Tilde,
        ];
        let mut spans = Vec::new();
        let mut pos = 0usize;
        for (idx, style) in styles.iter().enumerate() {
            let span_len = (idx % 3) + 1; // 1..=3
            let gap = idx % 4; // 0..=3
            pos += gap;
            spans.push(span(pos, pos + span_len, *style));
            pos += span_len;
        }
        assert_equivalent(&spans, pos + 10);
    }

    #[test]
    fn single_char_spans_tightly_packed() {
        let spans: Vec<ColorSpan> = (0..20)
            .map(|i| span(i, i + 1, HighlightStyle::Default))
            .collect();
        assert_equivalent(&spans, 25);
    }
}
