//! Screen-level helpers around a read: leaving the input area,
//! reprinting the prompt, listing expansions and the bell.

use std::io;

use super::LineEditor;
use crate::interactive::highlight::ColorSpan;
use crate::interactive::terminal::Terminal;

impl LineEditor {
    /// If an un-accepted autosuggestion is on screen, repaint without it so
    /// it doesn't linger in scrollback after the read ends — a multiline
    /// suggestion would otherwise leave dim phantom continuation lines that
    /// were never input above the command's output.
    pub(super) fn clear_lingering_suggestion<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        prompt_width: usize,
        cont_prompt: &str,
        spans: &[ColorSpan],
    ) -> io::Result<()> {
        if self.suggestion.take().is_some() {
            let (tw, th) = Self::term_size(term);
            self.redraw(term, prompt, prompt_width, cont_prompt, spans, tw, th)?;
        }
        Ok(())
    }

    /// Move the cursor down past the last rendered row so subsequent
    /// terminal output starts below the full (possibly wrapped) input.
    /// No-op when nothing has been rendered yet.
    fn move_below_render<T: Terminal>(&self, term: &mut T) -> io::Result<()> {
        if self.prev_total_rows > self.prev_cursor_row {
            term.move_down((self.prev_total_rows - self.prev_cursor_row) as u16)?;
        }
        Ok(())
    }

    /// Terminal size with the conventional 80x24 fallback.
    pub(super) fn term_size<T: Terminal>(term: &T) -> (u16, u16) {
        term.size().unwrap_or((80, 24))
    }

    /// End-of-read screen sequence shared by Submit/Interrupt (and the
    /// like): move below the rendered input, return to column 0, restore
    /// the cursor shape, and start a fresh output line.
    pub(super) fn finish_line<T: Terminal>(&mut self, term: &mut T) -> io::Result<()> {
        self.move_below_render(term)?;
        term.move_to_column(0)?;
        self.reset_cursor_style(term);
        term.write_str("\r\n")?;
        term.flush()?;
        Ok(())
    }

    /// Repaint the prompt block (upper display lines plus the last prompt
    /// line) after a full-screen UI or external program disturbed the
    /// display, then reset the render bookkeeping so the next redraw does
    /// a full repaint. `clear_screen` clears the whole screen (Ctrl+L);
    /// otherwise only the current line is cleared.
    pub(super) fn repaint_prompt<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        upper_lines: &[String],
        clear_screen: bool,
    ) -> io::Result<()> {
        if clear_screen {
            term.clear_all()?;
        } else {
            term.move_to_column(0)?;
            term.clear_current_line()?;
        }
        self.reprint_prompt_lines(term, prompt, upper_lines)
    }

    /// Write the prompt block (upper display lines, then the last prompt
    /// line) at the cursor and forget the previous render, since the
    /// input area now starts on the freshly printed prompt row.
    fn reprint_prompt_lines<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        upper_lines: &[String],
    ) -> io::Result<()> {
        for line in upper_lines {
            term.write_str(line)?;
            term.write_str("\r\n")?;
        }
        term.write_str(prompt)?;
        self.invalidate_render_state();
        Ok(())
    }

    /// Print the vi `=` expansion list below the current render, then
    /// reprint the prompt for the follow-up redraw.
    pub(super) fn print_vi_expansions<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        upper_lines: &[String],
        items: &[String],
    ) -> io::Result<()> {
        self.move_below_render(term)?;
        term.move_to_column(0)?;
        term.write_str("\r\n")?;
        term.write_str(&items.join("  "))?;
        term.write_str("\r\n")?;
        self.reprint_prompt_lines(term, prompt, upper_lines)
    }

    /// Emit (and clear) a pending vi alert as a terminal BEL.
    pub(super) fn flush_pending_bell<T: Terminal>(&mut self, term: &mut T) -> io::Result<()> {
        if self.pending_bell {
            self.pending_bell = false;
            term.write_str("\x07")?;
        }
        Ok(())
    }
}
