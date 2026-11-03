//! Single-logical-line renderer with diff-based partial repaint.

use std::io;
use unicode_width::UnicodeWidthChar;

use super::super::LineEditor;
use super::super::editing::chars_width;
use super::{CellPainter, style_diff_pos, term_col};
use crate::interactive::highlight::ColorSpan;
use crate::interactive::terminal::Terminal;
use crate::interactive::vi::ViMode;

impl LineEditor {
    /// Redraw the current buffer on screen, positioning the cursor correctly.
    /// Handles input that wraps past the terminal width. Buffers containing
    /// literal newlines are rendered as multiple logical lines, each
    /// continuation line prefixed with `cont_prompt`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn redraw_content<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        prompt_width: usize,
        cont_prompt: &str,
        spans: &[ColorSpan],
        term_width: u16,
        term_height: u16,
    ) -> io::Result<()> {
        let tw = term_width as usize;
        let th = (term_height as usize).max(1);

        // Precompute the width/row info both the dispatch decision and the
        // single-line renderer need.
        let (buf_pos_width, buf_total_width) = self.buf_prefix_and_total_width();
        let suggestion_active = self.suggestion.is_some() && self.pos == self.buf.len();
        let suggestion_width: usize = if suggestion_active {
            self.suggestion
                .as_ref()
                .unwrap()
                .chars()
                .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
                .sum()
        } else {
            0
        };
        let content_width = prompt_width + buf_total_width + suggestion_width;
        let total_rows = if tw > 0 && content_width > 0 {
            (content_width.saturating_sub(1)) / tw
        } else {
            0
        };

        // Multiline buffers get their own renderer; the soft-wrap row math
        // below assumes a single logical line. The same renderer also
        // handles a suggestion whose remainder spans lines, and — because
        // it is the only path with viewport clamping — any buffer whose
        // soft-wrapped render would be taller than the terminal.
        let suggestion_multiline =
            suggestion_active && self.suggestion.as_deref().is_some_and(|s| s.contains('\n'));
        // Wide (width-2) chars cannot straddle a row boundary, so a real
        // terminal can wrap up to one column early per row and pure width
        // division underestimates physical rows for CJK-heavy content. The
        // height dispatch therefore uses the pessimistic bound (one wasted
        // column per row) so a physically taller-than-screen buffer cannot
        // stay on the auto-wrap single-line path; `total_rows` itself keeps
        // the exact-division model the single-line painter is built on.
        let worst_case_rows = if tw > 1 {
            content_width.div_ceil(tw - 1)
        } else {
            content_width
        };
        if self.buf.contains(&'\n') || suggestion_multiline || (tw > 0 && worst_case_rows > th) {
            return self.redraw_multiline(term, prompt, prompt_width, cont_prompt, spans, tw, th);
        }

        // A VISUAL selection changes on every cursor move, so the diff
        // path would buy nothing while one is active — and the first
        // frame after it is dismissed must clear the reverse cells.
        // Bypass the partial repaint in both cases (keeps
        // `style_diff_pos` untouched by selection state).
        let sel_active = matches!(self.vi.mode, ViMode::Visual { .. });
        let force_full = sel_active || self.prev_render_had_selection;

        match self.partial_repaint_start(spans, prompt_width, tw, force_full) {
            Some(start) => {
                self.paint_partial(term, start, prompt_width, tw, spans, suggestion_active)?
            }
            None => self.paint_full(term, prompt, spans, suggestion_active)?,
        }

        self.prev_total_rows = total_rows;
        self.prev_render = Some((self.buf.clone(), spans.to_vec()));
        self.prev_render_multiline = false;
        self.prev_render_had_selection = sel_active;
        self.viewport_top = 0;

        // Position cursor at self.pos
        let cursor_total = prompt_width + buf_pos_width;
        let cursor_row = cursor_total.checked_div(tw).unwrap_or(0);
        let cursor_col = cursor_total.checked_rem(tw).unwrap_or(cursor_total);

        // Move from end-of-content row to cursor row
        let end_row = total_rows;
        if end_row > cursor_row {
            term.move_up((end_row - cursor_row) as u16)?;
        }
        term.move_to_column(term_col(cursor_col))?;
        // The physical row we just left the cursor on: `cursor_row`, except
        // when the cursor logically sits one past a content-final wrap
        // boundary — we never move down past the last rendered row, so the
        // physical row is clamped to it.
        self.prev_cursor_row = cursor_row.min(total_rows);
        term.flush()?;
        Ok(())
    }

    /// Decide whether a partial repaint (from the first changed cell) is
    /// safe, returning the char index to repaint from, or `None` to fall
    /// back to the full clear+repaint.
    ///
    /// The repaint start column is capped by the first position where
    /// the *style* differs from the previous render, not just where the
    /// *character* differs: highlighting can retroactively recolor
    /// already-typed, unchanged characters (e.g. typing the closing
    /// quote of `echo 'hello` flips the whole `'hello` run from an
    /// unclosed-quote Error span to a normal String span even though
    /// none of those characters changed). Using only a character-level
    /// diff would miss that recoloring.
    ///
    /// The multi-row (wrapped) case positions by physical rows: the
    /// cursor sits on `prev_cursor_row`, the first changed cell is at
    /// `prefix_width / tw` (always within the previously rendered rows
    /// because the unchanged prefix has identical layout in both
    /// renders), and writing relies on terminal auto-wrap exactly like
    /// the full-repaint path.
    fn partial_repaint_start(
        &self,
        spans: &[ColorSpan],
        prompt_width: usize,
        tw: usize,
        force_full: bool,
    ) -> Option<usize> {
        // If the *previous* render went through the multiline renderer, the
        // diff-based partial repaint would misinterpret its layout; the
        // full clear + repaint path handles it fine (clearing only uses
        // row counts).
        if tw == 0 || self.prev_render_multiline || force_full {
            return None;
        }
        let (prev_buf, prev_spans) = self.prev_render.as_ref()?;
        let char_diff = prev_buf
            .iter()
            .zip(self.buf.iter())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| prev_buf.len().min(self.buf.len()));
        let start = style_diff_pos(prev_spans, spans, char_diff);

        // A diff that starts exactly on a wrap boundary would have to write
        // into the terminal's deferred-wrap cell (or onto a physical row
        // that has not been created yet); fall back to the full repaint for
        // that column rather than modeling deferred-wrap state.
        let w = prompt_width + chars_width(&self.buf[..start]);
        if w > 0 && w % tw == 0 {
            return None;
        }
        Some(start)
    }

    /// Partial repaint: rewrite only from buffer char `start` onward.
    fn paint_partial<T: Terminal>(
        &mut self,
        term: &mut T,
        start: usize,
        prompt_width: usize,
        tw: usize,
        spans: &[ColorSpan],
        suggestion_active: bool,
    ) -> io::Result<()> {
        let prefix_width = prompt_width + chars_width(&self.buf[..start]);
        let diff_row = prefix_width / tw;
        let diff_col = prefix_width % tw;

        // Move from the cursor's row to the first changed row.
        if self.prev_cursor_row > diff_row {
            term.move_up((self.prev_cursor_row - diff_row) as u16)?;
        } else if diff_row > self.prev_cursor_row {
            term.move_down((diff_row - self.prev_cursor_row) as u16)?;
        }
        term.move_to_column(term_col(diff_col))?;
        term.clear_until_newline()?;

        // Clear any previously rendered rows below the changed row, so
        // shrinking content doesn't leave stale wrapped tails behind.
        if self.prev_total_rows > diff_row {
            for _ in diff_row..self.prev_total_rows {
                term.move_down(1)?;
                term.clear_current_line()?;
            }
            term.move_up((self.prev_total_rows - diff_row) as u16)?;
            term.move_to_column(term_col(diff_col))?;
        }

        // Seed the painter as if it had walked from 0, so the first char
        // written at `start` gets the right style without re-walking (and
        // re-emitting escapes for) the unchanged prefix.
        let mut painter = CellPainter::new(spans);
        painter.seed(term, start)?;
        for (i, ch) in self.buf.iter().enumerate().skip(start) {
            painter.enter_buf_cell(term, i, false)?;
            term.write_char(*ch)?;
        }
        painter.reset(term)?;

        if suggestion_active {
            self.paint_suggestion(term)?;
        }
        Ok(())
    }

    /// Full clear + repaint of the prompt, buffer and suggestion.
    fn paint_full<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        spans: &[ColorSpan],
        suggestion_active: bool,
    ) -> io::Result<()> {
        self.clear_prev_render(term)?;

        // Repaint the prompt
        term.write_str(prompt)?;

        // Write the buffer with or without highlighting
        let selection = self.visual_selection_range();
        if selection.is_none() && spans.is_empty() {
            term.write_str(&self.buffer())?;
        } else {
            // Spans are produced in left-to-right scan order (sorted,
            // non-overlapping — see HighlightScanner::scan_from), so a
            // single advancing cursor into `spans` (see
            // `style_at_advancing`) replaces a per-char linear search,
            // turning classification from O(n*spans) into O(n + spans).
            // A VISUAL selection is overlaid as reverse video.
            let mut painter = CellPainter::new(spans);
            for (i, ch) in self.buf.iter().enumerate() {
                let sel_here = selection.is_some_and(|(s, e)| i >= s && i < e);
                painter.enter_buf_cell(term, i, sel_here)?;
                term.write_char(*ch)?;
            }
            // Boundary cleanup: a selection reaching the final buffer
            // character never hits a "leaving" cell.
            painter.reset(term)?;
        }

        if suggestion_active {
            self.paint_suggestion(term)?;
        }
        Ok(())
    }

    /// Draw the autosuggestion (dim) at the current terminal position.
    fn paint_suggestion<T: Terminal>(&self, term: &mut T) -> io::Result<()> {
        term.set_dim(true)?;
        term.write_str(self.suggestion.as_deref().unwrap_or(""))?;
        term.set_dim(false)?;
        Ok(())
    }
}
