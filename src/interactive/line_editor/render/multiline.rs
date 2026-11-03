//! Multiline renderer: explicit row packing and a cursor-following viewport.

use std::io;
use unicode_width::UnicodeWidthChar;

use super::super::LineEditor;
use super::{CellPainter, term_col};
use crate::interactive::display_width::display_width;
use crate::interactive::highlight::ColorSpan;
use crate::interactive::terminal::Terminal;
use crate::interactive::vi::{self, ViMode};

/// What a rendered cell's char is styled as.
#[derive(Clone, Copy, PartialEq)]
enum Cell {
    /// Buffer char at this index — styled by the highlight spans.
    Buf(usize),
    /// Autosuggestion char — rendered dim, never highlighted.
    Sugg,
}

/// A logical display line: its characters and how each is styled.
type DisplayLine = Vec<(char, Cell)>;

/// One physical row of the packed layout.
///
/// `first` marks the row where its logical line's prefix write begins.
/// A prefix wider than the terminal auto-wraps when painted (it is
/// written atomically with `write_str`, since it may contain ANSI escapes
/// that must not be split); `in_prefix` marks the extra physical rows
/// that auto-wrap produces — painting must not emit a row break to enter
/// them. `start_col` is where a row's cells begin (non-zero only on the
/// last prefix-flow row).
struct Row {
    line: usize,
    first: bool,
    in_prefix: bool,
    start_col: usize,
    cells: DisplayLine,
}

impl Row {
    fn new(line: usize, first: bool, in_prefix: bool, start_col: usize) -> Self {
        Self {
            line,
            first,
            in_prefix,
            start_col,
            cells: Vec::new(),
        }
    }
}

/// Pack logical display lines into physical rows of at most `tw`
/// columns (`tw == 0` = unknown width: one row per logical line, prefix
/// rows aside). Line 0 is prefixed by a `prompt_width`-wide prompt, the
/// rest by a `cont_width`-wide continuation prompt. Also returns the
/// `(row index, column)` of the buffer cursor, given as logical line
/// `cursor_line` / char offset `cursor_ofs`, if it was placed.
fn pack_rows(
    dlines: &[DisplayLine],
    prompt_width: usize,
    cont_width: usize,
    tw: usize,
    cursor_line: usize,
    cursor_ofs: usize,
) -> (Vec<Row>, Option<(usize, usize)>) {
    // Column budget per row; tw == 0 means "unknown width" — treat as
    // unbounded so each logical line stays on one row.
    let tw_eff = if tw == 0 { usize::MAX } else { tw };
    let mut rows: Vec<Row> = Vec::new();
    // (row index, column) the cursor lands on, in absolute row terms.
    let mut cursor_at: Option<(usize, usize)> = None;
    for (li, cells) in dlines.iter().enumerate() {
        let prefix_w = if li == 0 { prompt_width } else { cont_width };
        // Physical rows the prefix's characters occupy beyond the
        // first. `(prefix_w - 1) / tw` (not `prefix_w / tw`) because a
        // prefix that exactly fills its last row leaves the cursor in
        // the terminal's deferred-wrap state on that row rather than
        // opening a new one.
        let extra = if tw > 0 && prefix_w > 0 {
            (prefix_w - 1) / tw
        } else {
            0
        };
        let mut cur;
        let mut cur_col;
        if extra == 0 {
            cur_col = prefix_w;
            cur = Row::new(li, true, false, cur_col);
        } else {
            rows.push(Row::new(li, true, false, 0));
            for _ in 1..extra {
                rows.push(Row::new(li, false, true, 0));
            }
            cur_col = prefix_w - extra * tw;
            cur = Row::new(li, false, true, cur_col);
        }
        for (ci, &(ch, kind)) in cells.iter().enumerate() {
            let w = UnicodeWidthChar::width(ch).unwrap_or(0);
            // Wrap before a cell that no longer fits — but always place
            // at least one cell on a continuation row so packing makes
            // progress even for degenerate widths.
            if cur_col + w > tw_eff && (cur.first || cur.in_prefix || !cur.cells.is_empty()) {
                let done = std::mem::replace(&mut cur, Row::new(li, false, false, 0));
                rows.push(done);
                cur_col = 0;
            }
            if li == cursor_line && ci == cursor_ofs {
                cursor_at = Some((rows.len(), cur_col));
            }
            cur.cells.push((ch, kind));
            cur_col += w;
        }
        if li == cursor_line && cursor_ofs == cells.len() && cursor_at.is_none() {
            cursor_at = Some((rows.len(), cur_col));
        }
        rows.push(cur);
    }
    (rows, cursor_at)
}

/// Paint `rows` (a viewport slice of the packed layout) starting at the
/// cursor's current position, which must be column 0 of the first row.
fn paint_rows<T: Terminal>(
    term: &mut T,
    rows: &[Row],
    prompt: &str,
    cont_prompt: &str,
    spans: &[ColorSpan],
    selection: Option<(usize, usize)>,
) -> io::Result<()> {
    let mut painter = CellPainter::new(spans);
    let mut dim_on = false;
    for (vi, row) in rows.iter().enumerate() {
        if vi > 0 || row.first {
            // Row breaks and prefixes are always unstyled; reverse
            // is switched off before any non-buffer output and
            // re-asserted at the first selected cell of the next
            // row, so prompts and wraps are never reverse-rendered.
            painter.reset(term)?;
            if dim_on {
                term.set_dim(false)?;
                dim_on = false;
            }
        }
        // `in_prefix` rows are reached by the prefix write's auto-wrap,
        // not by an explicit row break. (If the viewport ever starts
        // mid-prefix — a prompt wider than a whole screen — the prefix
        // tail is not repainted; cells are positioned explicitly below.)
        if vi > 0 && !row.in_prefix {
            term.write_str("\r\n")?;
        }
        if row.first {
            term.write_str(if row.line == 0 { prompt } else { cont_prompt })?;
        } else if vi == 0 && row.in_prefix && !row.cells.is_empty() {
            term.move_to_column(term_col(row.start_col))?;
        }
        for &(ch, kind) in &row.cells {
            match kind {
                Cell::Buf(i) => {
                    if dim_on {
                        term.set_dim(false)?;
                        dim_on = false;
                    }
                    let sel_here = selection.is_some_and(|(s, e)| i >= s && i < e);
                    painter.enter_buf_cell(term, i, sel_here)?;
                }
                Cell::Sugg => {
                    painter.reset(term)?;
                    if !dim_on {
                        term.set_dim(true)?;
                        dim_on = true;
                    }
                }
            }
            term.write_char(ch)?;
        }
    }
    painter.reset(term)?;
    if dim_on {
        term.set_dim(false)?;
    }
    Ok(())
}

impl LineEditor {
    /// Render a buffer as logical lines: each soft-wraps within the terminal
    /// width, and every continuation line is prefixed with `cont_prompt`. An
    /// autosuggestion remainder is rendered dim after the buffer, its own
    /// newlines starting further continuation lines. Always a full clear +
    /// repaint — the partial-repaint diff only models single-line layouts.
    ///
    /// Unlike the single-line renderer this path does not rely on terminal
    /// auto-wrap: the layout is packed into explicit physical rows first,
    /// and only a viewport of at most `th` rows — always containing the
    /// cursor — is painted. That keeps every relative cursor movement within
    /// the screen, so renders taller than the terminal cannot corrupt the
    /// display (`move_up` past the top row would otherwise clamp and
    /// misalign all subsequent bookkeeping).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn redraw_multiline<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        prompt_width: usize,
        cont_prompt: &str,
        spans: &[ColorSpan],
        tw: usize,
        th: usize,
    ) -> io::Result<()> {
        let cont_width = display_width(cont_prompt);
        // VISUAL selection overlay (always a full repaint here).
        let sel_active = matches!(self.vi.mode, ViMode::Visual { .. });
        let selection = self.visual_selection_range();

        // The cursor's display line/offset is computed against buffer
        // lines only (suggestion cells come after the cursor).
        let cursor_line = self.cursor_line_index();
        let cursor_ofs = self.pos - vi::line_start(&self.buf, self.pos);
        let dlines = self.display_lines();
        let (rows, cursor_at) = pack_rows(
            &dlines,
            prompt_width,
            cont_width,
            tw,
            cursor_line,
            cursor_ofs,
        );
        let total_rows = rows.len();
        let (cursor_row, cursor_col) = cursor_at.unwrap_or((0, prompt_width));

        // ---- Choose the viewport: at most `th` rows, containing the
        // cursor, scrolled minimally from the previous window ----
        let visible = total_rows.min(th);
        let mut vt = self.viewport_top.min(total_rows - visible);
        if cursor_row < vt {
            vt = cursor_row;
        } else if cursor_row >= vt + visible {
            vt = cursor_row + 1 - visible;
        }
        self.viewport_top = vt;

        // ---- Clear every row of the previous render (its bookkeeping is
        // viewport-relative, so all rows are on screen) ----
        self.clear_prev_render(term)?;

        paint_rows(
            term,
            &rows[vt..vt + visible],
            prompt,
            cont_prompt,
            spans,
            selection,
        )?;

        // ---- Position the cursor (window-relative) ----
        // The paint left the physical cursor on the last visible row.
        let end_row = vt + visible - 1;
        if end_row > cursor_row {
            term.move_up((end_row - cursor_row) as u16)?;
        }
        // A cursor one past an exactly-full row sits in the terminal's
        // deferred-wrap position; emit the last real column explicitly
        // instead of relying on the terminal clamping an out-of-range CHA.
        let cursor_col = if tw > 0 {
            cursor_col.min(tw - 1)
        } else {
            cursor_col
        };
        term.move_to_column(term_col(cursor_col))?;

        self.prev_total_rows = visible - 1;
        self.prev_cursor_row = cursor_row - vt;
        self.prev_render_multiline = true;
        self.prev_render_had_selection = sel_active;
        self.prev_render = Some((self.buf.clone(), spans.to_vec()));
        term.flush()?;
        Ok(())
    }

    /// Build the logical display lines: the buffer split at its newlines,
    /// followed by an active autosuggestion's cells, whose own newlines
    /// start further continuation lines.
    fn display_lines(&self) -> Vec<DisplayLine> {
        let mut dlines: Vec<DisplayLine> = vec![Vec::new()];
        for (i, &c) in self.buf.iter().enumerate() {
            if c == '\n' {
                dlines.push(Vec::new());
            } else {
                dlines.last_mut().unwrap().push((c, Cell::Buf(i)));
            }
        }
        let suggestion_active = self.suggestion.is_some() && self.pos == self.buf.len();
        if suggestion_active {
            let sugg = self.suggestion.clone().unwrap_or_default();
            for (si, seg) in sugg.split('\n').enumerate() {
                if si > 0 {
                    dlines.push(Vec::new());
                }
                dlines
                    .last_mut()
                    .unwrap()
                    .extend(seg.chars().map(|c| (c, Cell::Sugg)));
            }
        }
        dlines
    }
}
