//! Single-logical-line renderer with diff-based partial repaint.

use std::io;
use unicode_width::UnicodeWidthChar;

use super::super::LineEditor;
use super::{style_at_advancing, style_diff_pos};
use crate::interactive::highlight::{ColorSpan, HighlightStyle, apply_style};
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
        let col = |n: usize| -> u16 { n.min(u16::MAX as usize) as u16 };

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
        // If the *previous* render went through the multiline renderer, the
        // diff-based partial repaint below would misinterpret its layout;
        // the full clear + repaint path handles it fine (clearing only uses
        // row counts).
        let prev_was_multiline = self.prev_render_multiline;

        // A VISUAL selection changes on every cursor move, so the diff
        // path would buy nothing while one is active — and the first
        // frame after it is dismissed must clear the reverse cells.
        // Bypass the partial repaint in both cases (keeps
        // `style_diff_pos` untouched by selection state).
        let sel_active = matches!(self.vi.mode, ViMode::Visual { .. });
        let selection = self.visual_selection_range();
        let force_full = sel_active || self.prev_render_had_selection;

        // Decide whether a partial repaint (from the first changed cell)
        // is safe, or whether to fall back to the full clear+repaint.
        //
        // The repaint start column is capped by the first position where
        // the *style* differs from the previous render, not just where the
        // *character* differs: highlighting can retroactively recolor
        // already-typed, unchanged characters (e.g. typing the closing
        // quote of `echo 'hello` flips the whole `'hello` run from an
        // unclosed-quote Error span to a normal String span even though
        // none of those characters changed). Using only a character-level
        // diff would miss that recoloring.
        //
        // The multi-row (wrapped) case positions by physical rows: the
        // cursor sits on `prev_cursor_row`, the first changed cell is at
        // `prefix_width / tw` (always within the previously rendered rows
        // because the unchanged prefix has identical layout in both
        // renders), and writing relies on terminal auto-wrap exactly like
        // the full-repaint path.
        let mut partial_repaint_start =
            self.prev_render
                .as_ref()
                .and_then(|(prev_buf, prev_spans)| {
                    if tw == 0 || prev_was_multiline || force_full {
                        return None;
                    }
                    let char_diff = prev_buf
                        .iter()
                        .zip(self.buf.iter())
                        .position(|(a, b)| a != b)
                        .unwrap_or_else(|| prev_buf.len().min(self.buf.len()));
                    let style_diff = style_diff_pos(prev_spans, spans, char_diff);
                    Some(style_diff)
                });

        // A diff that starts exactly on a wrap boundary would have to write
        // into the terminal's deferred-wrap cell (or onto a physical row
        // that has not been created yet); fall back to the full repaint for
        // that column rather than modeling deferred-wrap state.
        if let Some(start) = partial_repaint_start {
            let w = prompt_width
                + self.buf[..start]
                    .iter()
                    .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
                    .sum::<usize>();
            if tw > 0 && w > 0 && w % tw == 0 {
                partial_repaint_start = None;
            }
        }

        if let Some(start) = partial_repaint_start {
            // ---- Partial repaint: rewrite only from `start` onward ----
            let prefix_width: usize = prompt_width
                + self.buf[..start]
                    .iter()
                    .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
                    .sum::<usize>();
            let diff_row = prefix_width / tw;
            let diff_col = prefix_width % tw;

            // Move from the cursor's row to the first changed row.
            if self.prev_cursor_row > diff_row {
                term.move_up((self.prev_cursor_row - diff_row) as u16)?;
            } else if diff_row > self.prev_cursor_row {
                term.move_down((diff_row - self.prev_cursor_row) as u16)?;
            }
            term.move_to_column(col(diff_col))?;
            term.clear_until_newline()?;

            // Clear any previously rendered rows below the changed row, so
            // shrinking content doesn't leave stale wrapped tails behind.
            if self.prev_total_rows > diff_row {
                for _ in diff_row..self.prev_total_rows {
                    term.move_down(1)?;
                    term.clear_current_line()?;
                }
                term.move_up((self.prev_total_rows - diff_row) as u16)?;
                term.move_to_column(col(diff_col))?;
            }

            let mut span_idx = 0;
            let mut current_style = HighlightStyle::Default;
            if !spans.is_empty() {
                // Seed span_idx/current_style as if we'd walked from 0, so
                // the first char written at `start` gets the right style
                // without re-walking (and re-emitting escapes for) the
                // unchanged prefix.
                current_style = style_at_advancing(spans, &mut span_idx, start);
                if current_style != HighlightStyle::Default {
                    apply_style(term, current_style)?;
                }
            }
            for (i, ch) in self.buf.iter().enumerate().skip(start) {
                if !spans.is_empty() {
                    let new_style = style_at_advancing(spans, &mut span_idx, i);
                    if new_style != current_style {
                        if current_style != HighlightStyle::Default {
                            term.reset_style()?;
                        }
                        apply_style(term, new_style)?;
                        current_style = new_style;
                    }
                }
                term.write_char(*ch)?;
            }
            if current_style != HighlightStyle::Default {
                term.reset_style()?;
            }

            if suggestion_active {
                term.set_dim(true)?;
                term.write_str(self.suggestion.as_deref().unwrap_or(""))?;
                term.set_dim(false)?;
            }
        } else {
            // ---- Full clear + repaint (original behavior) ----
            self.clear_prev_render(term)?;

            // Repaint the prompt
            term.write_str(prompt)?;

            // Write the buffer with or without highlighting
            if let Some((sel_s, sel_e)) = selection {
                // VISUAL selection: per-char walk tracking both the
                // highlight style and the reverse-video span. Because
                // `reset_style` clears the reverse attribute too, every
                // style transition inside the selection re-asserts
                // reverse after reapplying the color; leaving the
                // selection resets and reapplies the color style.
                let mut current_style = HighlightStyle::Default;
                let mut span_idx = 0;
                let mut in_sel = false;
                for (i, ch) in self.buf.iter().enumerate() {
                    let new_style = if spans.is_empty() {
                        HighlightStyle::Default
                    } else {
                        style_at_advancing(spans, &mut span_idx, i)
                    };
                    let sel_here = i >= sel_s && i < sel_e;
                    if new_style != current_style || (in_sel && !sel_here) {
                        if current_style != HighlightStyle::Default || in_sel {
                            term.reset_style()?;
                            in_sel = false;
                        }
                        if new_style != HighlightStyle::Default {
                            apply_style(term, new_style)?;
                        }
                        current_style = new_style;
                    }
                    if sel_here && !in_sel {
                        term.set_reverse(true)?;
                        in_sel = true;
                    }
                    term.write_char(*ch)?;
                }
                // Boundary cleanup: a selection reaching the final
                // buffer character never hits a "leaving" cell.
                if current_style != HighlightStyle::Default || in_sel {
                    term.reset_style()?;
                }
            } else if spans.is_empty() {
                term.write_str(&self.buffer())?;
            } else {
                // Spans are produced in left-to-right scan order (sorted,
                // non-overlapping — see HighlightScanner::scan_from), so a
                // single advancing cursor into `spans` (see
                // `style_at_advancing`) replaces the previous per-char
                // `spans.iter().find(...)` linear search, turning
                // classification from O(n*spans) into O(n + spans).
                let mut current_style = HighlightStyle::Default;
                let mut span_idx = 0;
                for (i, ch) in self.buf.iter().enumerate() {
                    let new_style = style_at_advancing(spans, &mut span_idx, i);
                    if new_style != current_style {
                        if current_style != HighlightStyle::Default {
                            term.reset_style()?;
                        }
                        apply_style(term, new_style)?;
                        current_style = new_style;
                    }
                    term.write_char(*ch)?;
                }
                if current_style != HighlightStyle::Default {
                    term.reset_style()?;
                }
            }

            // Draw suggestion
            if suggestion_active {
                term.set_dim(true)?;
                term.write_str(self.suggestion.as_deref().unwrap_or(""))?;
                term.set_dim(false)?;
            }
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
        term.move_to_column(col(cursor_col))?;
        // The physical row we just left the cursor on: `cursor_row`, except
        // when the cursor logically sits one past a content-final wrap
        // boundary — we never move down past the last rendered row, so the
        // physical row is clamped to it.
        self.prev_cursor_row = cursor_row.min(total_rows);
        term.flush()?;
        Ok(())
    }
}
