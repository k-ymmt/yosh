//! Read sessions: the raw-mode terminal event loop that turns key
//! events into edits, redraws and the final line.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::io;

use super::{KeyAction, LineEditor};
use crate::interactive::command_completion::CommandCompletionContext;
use crate::interactive::completion::CompletionContext;
use crate::interactive::display_width::display_width;
use crate::interactive::fuzzy_search::FuzzySearchUI;
use crate::interactive::highlight::{CheckerEnv, ColorSpan, HighlightScanner};
use crate::interactive::history::History;
use crate::interactive::spec_completion::SpecStore;
use crate::interactive::terminal::{CursorStyle, Terminal};
use crate::interactive::vi::{ViFlavor, ViMode};

/// Convert alias-macro text into key events: ESC, Enter, Tab, and
/// control characters map to their key equivalents so an alias value
/// can contain editing commands (POSIX `@letter`).
fn key_events_for_text(text: &str) -> Vec<KeyEvent> {
    text.chars()
        .map(|c| match c {
            '\x1b' => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            '\n' | '\r' => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            '\t' => KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            '\x7f' => KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
            // 0x1C..=0x1F as crossterm decodes them from a terminal
            // (Ctrl+4..=Ctrl+7), so e.g. a Ctrl+_ in a macro undoes.
            c @ '\x1c'..='\x1f' => KeyEvent::new(
                KeyCode::Char(((c as u8) - 0x1c + b'4') as char),
                KeyModifiers::CONTROL,
            ),
            c if (c as u32) < 0x20 => KeyEvent::new(
                KeyCode::Char((((c as u8) | 0x60) as char).to_ascii_lowercase()),
                KeyModifiers::CONTROL,
            ),
            c => KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        })
        .collect()
}

/// What a completion-enabled read needs to paint the buffer: the
/// syntax highlighter and the lazily resolved continuation prompt.
struct PaintContext<'a, 'env> {
    scanner: &'a mut HighlightScanner,
    checker_env: &'a CheckerEnv<'env>,
    /// Earlier lines of the command being entered, for highlighting.
    accumulated: &'a str,
    cont_prompt: &'a mut dyn FnMut() -> String,
}

impl LineEditor {
    /// Read a line of input from the terminal, handling cursor movement and
    /// editing keys.  Returns `Ok(Some(line))` on Enter, `Ok(None)` on
    /// Ctrl-D with an empty buffer (EOF), or `Ok(Some(""))` on Ctrl-C.
    #[allow(dead_code)] // Used by tests; production code uses read_line_with_completion
    pub fn read_line<T: Terminal>(
        &mut self,
        prompt: &str,
        upper_lines: &[String],
        history: &mut History,
        term: &mut T,
    ) -> io::Result<Option<String>> {
        self.clear();
        self.interrupted = false;
        term.enable_raw_mode()?;
        let result = self.read_line_loop(prompt, upper_lines, history, term);
        self.reset_cursor_style(term);
        let _ = term.disable_raw_mode();
        result
    }

    fn read_line_loop<T: Terminal>(
        &mut self,
        prompt: &str,
        upper_lines: &[String],
        history: &mut History,
        term: &mut T,
    ) -> io::Result<Option<String>> {
        let prompt_width = display_width(prompt);
        loop {
            self.sync_cursor_style(term)?;
            term.flush()?;
            match self.next_event(term)? {
                Event::Key(key_event) => {
                    match self.handle_key(key_event, history) {
                        KeyAction::Submit => {
                            history.reset_cursor();
                            self.clear_lingering_suggestion(term, prompt, prompt_width, "> ", &[])?;
                            self.finish_line(term)?;
                            return Ok(Some(self.buffer()));
                        }
                        KeyAction::Eof => {
                            return Ok(None);
                        }
                        KeyAction::Interrupt => {
                            history.reset_cursor();
                            self.clear_lingering_suggestion(term, prompt, prompt_width, "> ", &[])?;
                            self.finish_line(term)?;
                            self.clear();
                            self.interrupted = true;
                            return Ok(Some(String::new()));
                        }
                        KeyAction::FuzzySearch => {
                            self.run_fuzzy_search(term, history)?;
                            // The screen was repainted outside of redraw's own
                            // bookkeeping; force a full repaint next time so
                            // the diff-based partial repaint doesn't assume a
                            // stale on-screen state.
                            self.repaint_prompt(term, prompt, upper_lines, false)?;
                        }
                        KeyAction::ClearScreen => {
                            self.repaint_prompt(term, prompt, upper_lines, true)?;
                        }
                        KeyAction::ViListExpansions(items) => {
                            self.print_vi_expansions(term, prompt, upper_lines, &items)?;
                        }
                        // The plain read path has no alias store or
                        // editor integration (test-only entry point).
                        KeyAction::ViAliasMacro(_) => {}
                        KeyAction::ViEditInEditor(_) | KeyAction::ViEditBuffer => {
                            self.pending_bell = true;
                        }
                        KeyAction::TabComplete | KeyAction::Continue => {}
                    }
                    self.flush_pending_bell(term)?;
                    self.update_suggestion(history);
                    let (tw, th) = Self::term_size(term);
                    self.redraw(term, prompt, prompt_width, "> ", &[], tw, th)?;
                }
                Event::Resize(_cols, _rows) => {
                    // Terminal dimensions changed, invalidating all cached
                    // row/column math from the previous render; force a full
                    // repaint.
                    self.invalidate_render_state();
                    let (tw, th) = Self::term_size(term);
                    self.update_suggestion(history);
                    self.redraw(term, prompt, prompt_width, "> ", &[], tw, th)?;
                }
                _ => {}
            }
        }
    }

    /// Emit the cursor shape matching the current vi submode when it
    /// changed since the last emit. No-op in emacs mode (the user's
    /// terminal default is left untouched).
    fn sync_cursor_style<T: Terminal>(&mut self, term: &mut T) -> io::Result<()> {
        if !self.edit_mode.is_vi_family() {
            return Ok(());
        }
        let want = match self.vi.mode {
            ViMode::Insert => CursorStyle::Bar,
            ViMode::Command | ViMode::Visual { .. } => CursorStyle::Block,
        };
        if self.last_cursor_style != Some(want) {
            term.set_cursor_style(want)?;
            self.last_cursor_style = Some(want);
        }
        Ok(())
    }

    /// Restore the terminal-default cursor shape if a vi read changed it.
    pub(super) fn reset_cursor_style<T: Terminal>(&mut self, term: &mut T) {
        if self.last_cursor_style.take().is_some() {
            let _ = term.set_cursor_style(CursorStyle::Default);
            let _ = term.flush();
        }
    }

    /// Pop a queued synthetic event (`@letter` macro input) or read one
    /// from the terminal.
    fn next_event<T: Terminal>(&mut self, term: &mut T) -> io::Result<Event> {
        if let Some(k) = self.pending_events.pop_front() {
            return Ok(Event::Key(k));
        }
        // Reading from the terminal replenishes the alias-macro budget:
        // the recursion cap is per keystroke, not per read session.
        self.vi_macro_budget = 64;
        term.read_event()
    }

    /// Run the fuzzy history search UI (outside raw mode); a selected
    /// entry replaces the buffer. The caller repaints the prompt.
    fn run_fuzzy_search<T: Terminal>(
        &mut self,
        term: &mut T,
        history: &mut History,
    ) -> io::Result<()> {
        self.suggestion = None;
        term.disable_raw_mode()?;
        if let Ok(Some(line)) = FuzzySearchUI::run(history, term) {
            self.replace_buffer(&line);
        }
        term.enable_raw_mode()
    }

    /// POSIX `@letter`: queue alias `_letter`'s `value` as editor input.
    /// The per-keystroke expansion budget (see `next_event`) breaks
    /// recursive macros like `alias _a='@a'`; the size cap bounds a
    /// single expansion burst.
    fn queue_alias_macro(&mut self, value: &str) {
        if self.vi_macro_budget > 0 && self.pending_events.len() + value.chars().count() <= 4096 {
            self.vi_macro_budget -= 1;
            self.pending_events.extend(key_events_for_text(value));
        }
    }

    /// Enter on structurally incomplete input (unclosed if/quote/heredoc,
    /// trailing pipe, …): start a continuation line in-buffer instead of
    /// submitting, keeping the whole construct editable with cursor
    /// movement across lines.
    fn start_continuation_line(&mut self) {
        // vi Enter executes the whole buffer regardless of cursor
        // position, so the continuation line starts after the end of what
        // was submitted — not at a mid-buffer command-mode cursor. The
        // continuation conventionally starts in insert mode, whichever
        // submode Enter was pressed in.
        if self.edit_mode.is_vi_family() {
            // The continuation is a fresh insert session: save the
            // pre-continuation state for `u` and start the `.` text
            // capture at the new line. In the vim flavor an ongoing
            // Insert session simply absorbs the '\n'; from Command mode
            // the pre-newline snapshot moves to the staging slot (one
            // unit spanning the newline plus the typed continuation).
            if self.vi.flavor == ViFlavor::Vim {
                if self.vi.mode != ViMode::Insert {
                    self.vim_undo_staged = Some((self.buf.clone(), self.pos));
                }
            } else {
                self.undo.save(&self.buf, self.pos);
            }
            self.pos = self.buf.len();
            self.vi.mode = ViMode::Insert;
            self.vi.replace_overwrite = false;
            self.insert_char('\n');
            self.vi_insert_start = Some(self.pos);
        } else {
            self.insert_char('\n');
        }
    }

    // ── Tab completion support ─────────────────────────────────────────

    /// Read a line of input with Tab completion support.
    ///
    /// Behaves identically to [`read_line`] but also handles Tab key events
    /// by invoking the completion engine.
    ///
    /// Multiline editing: when Enter is pressed and `is_incomplete` returns
    /// `true` for the current buffer contents, a literal newline is inserted
    /// at the cursor instead of submitting, and editing continues across
    /// lines. Continuation lines are rendered with the prompt returned by
    /// `cont_prompt` (the caller's expanded `PS2` last line); the callback
    /// is invoked lazily — once per read, on the first multiline render —
    /// so a side-effectful PS2 (command substitution) only runs when a
    /// continuation line is actually displayed.
    #[allow(clippy::too_many_arguments)]
    pub fn read_line_with_completion<T: Terminal>(
        &mut self,
        prompt: &str,
        upper_lines: &[String],
        history: &mut History,
        term: &mut T,
        ctx: &CompletionContext,
        cmd_ctx: &mut CommandCompletionContext<'_>,
        specs: &mut SpecStore,
        scanner: &mut HighlightScanner,
        checker_env: &CheckerEnv<'_>,
        accumulated: &str,
        cont_prompt: &mut dyn FnMut() -> String,
        is_incomplete: &dyn Fn(&str) -> bool,
    ) -> io::Result<Option<String>> {
        self.clear();
        self.interrupted = false;
        term.enable_raw_mode()?;
        let mut paint = PaintContext {
            scanner,
            checker_env,
            accumulated,
            cont_prompt,
        };
        let result = self.read_line_loop_with_completion(
            prompt,
            upper_lines,
            history,
            term,
            ctx,
            cmd_ctx,
            specs,
            &mut paint,
            is_incomplete,
        );
        self.reset_cursor_style(term);
        let _ = term.disable_raw_mode();
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn read_line_loop_with_completion<T: Terminal>(
        &mut self,
        prompt: &str,
        upper_lines: &[String],
        history: &mut History,
        term: &mut T,
        ctx: &CompletionContext,
        cmd_ctx: &mut CommandCompletionContext<'_>,
        specs: &mut SpecStore,
        paint: &mut PaintContext<'_, '_>,
        is_incomplete: &dyn Fn(&str) -> bool,
    ) -> io::Result<Option<String>> {
        let prompt_width = display_width(prompt);
        loop {
            self.sync_cursor_style(term)?;
            term.flush()?;
            match self.next_event(term)? {
                Event::Key(key_event) => {
                    match self.handle_key(key_event, history) {
                        KeyAction::Submit => {
                            // Reset history navigation either way: a
                            // continuation makes the buffer a new
                            // in-progress construct, and a stale cursor
                            // would let a later Up replace it with an
                            // unrelated older entry.
                            history.reset_cursor();
                            if is_incomplete(&self.buffer()) {
                                self.start_continuation_line();
                            } else {
                                self.end_read_screen(term, prompt, prompt_width, paint)?;
                                return Ok(Some(self.buffer()));
                            }
                        }
                        KeyAction::Eof => {
                            return Ok(None);
                        }
                        KeyAction::Interrupt => {
                            history.reset_cursor();
                            self.end_read_screen(term, prompt, prompt_width, paint)?;
                            self.clear();
                            self.interrupted = true;
                            return Ok(Some(String::new()));
                        }
                        KeyAction::FuzzySearch => {
                            term.reset_style()?;
                            self.run_fuzzy_search(term, history)?;
                            self.repaint_prompt(term, prompt, upper_lines, false)?;
                        }
                        KeyAction::TabComplete => {
                            term.reset_style()?;
                            self.handle_tab_complete(
                                term,
                                prompt,
                                upper_lines,
                                ctx,
                                cmd_ctx,
                                specs,
                            )?;
                        }
                        KeyAction::ClearScreen => {
                            self.repaint_prompt(term, prompt, upper_lines, true)?;
                        }
                        KeyAction::ViListExpansions(items) => {
                            term.reset_style()?;
                            self.print_vi_expansions(term, prompt, upper_lines, &items)?;
                        }
                        KeyAction::ViAliasMacro(c) => {
                            // No effect if the alias is unset.
                            if let Some(value) = cmd_ctx.aliases.get(&format!("_{}", c)) {
                                self.queue_alias_macro(value);
                            }
                        }
                        KeyAction::ViEditInEditor(entry) => {
                            term.reset_style()?;
                            match self.vi_edit_in_editor(term, history, entry)? {
                                Some(true) => {
                                    // Execute the edited line (POSIX v).
                                    history.reset_cursor();
                                    term.move_to_column(0)?;
                                    self.reset_cursor_style(term);
                                    term.write_str("\r\n")?;
                                    term.flush()?;
                                    return Ok(Some(self.buffer()));
                                }
                                // Editor failed or was aborted: repaint
                                // the prompt line and continue editing.
                                Some(false) => {
                                    self.repaint_prompt(term, prompt, upper_lines, false)?
                                }
                                // No such history line: the bell is all;
                                // the regular redraw below keeps the input.
                                None => {}
                            }
                        }
                        KeyAction::ViEditBuffer => {
                            // vim Ctrl-X Ctrl-E: the edit result loads
                            // into the buffer; the read continues.
                            term.reset_style()?;
                            if self.vim_edit_buffer(term)? {
                                self.repaint_prompt(term, prompt, upper_lines, false)?;
                            }
                        }
                        KeyAction::Continue => {}
                    }
                    self.flush_pending_bell(term)?;
                    self.repaint_input(term, prompt, prompt_width, history, paint)?;
                }
                Event::Resize(_cols, _rows) => {
                    self.invalidate_render_state();
                    self.repaint_input(term, prompt, prompt_width, history, paint)?;
                }
                _ => {}
            }
        }
    }

    /// Highlight spans and continuation prompt for painting the current
    /// buffer.
    fn paint_inputs(&mut self, paint: &mut PaintContext<'_, '_>) -> (Vec<ColorSpan>, String) {
        let spans = paint
            .scanner
            .scan(paint.accumulated, &self.buf, paint.checker_env);
        let cont = self.resolve_cont_prompt(paint.cont_prompt);
        (spans, cont)
    }

    /// Refresh the autosuggestion and redraw the input area.
    fn repaint_input<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        prompt_width: usize,
        history: &History,
        paint: &mut PaintContext<'_, '_>,
    ) -> io::Result<()> {
        self.update_suggestion(history);
        let (spans, cont) = self.paint_inputs(paint);
        let (tw, th) = Self::term_size(term);
        self.redraw(term, prompt, prompt_width, &cont, &spans, tw, th)
    }

    /// Leave the input area at the end of a read (Submit / Interrupt):
    /// drop the highlight style, repaint without a lingering
    /// autosuggestion, and start a fresh output line below the input.
    fn end_read_screen<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        prompt_width: usize,
        paint: &mut PaintContext<'_, '_>,
    ) -> io::Result<()> {
        term.reset_style()?;
        let (spans, cont) = self.paint_inputs(paint);
        self.clear_lingering_suggestion(term, prompt, prompt_width, &cont, &spans)?;
        self.finish_line(term)
    }

    /// Return the continuation prompt for the upcoming redraw, resolving
    /// the lazy callback once per read session and only when the buffer is
    /// actually multiline (single-line redraws never print it).
    fn resolve_cont_prompt(&mut self, cont_prompt: &mut dyn FnMut() -> String) -> String {
        // A multiline autosuggestion renders continuation prompts too, so it
        // needs the resolved PS2 even while the buffer itself is single-line.
        let sugg_multiline = self.pos == self.buf.len()
            && self.suggestion.as_deref().is_some_and(|s| s.contains('\n'));
        if !self.buf.contains(&'\n') && !sugg_multiline {
            return String::new();
        }
        if self.cont_prompt_cache.is_none() {
            self.cont_prompt_cache = Some(cont_prompt());
        }
        self.cont_prompt_cache.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Macro text maps control bytes to the key events a terminal
    /// produces through crossterm (0x1F is Ctrl+7, DEL is Backspace).
    #[test]
    fn macro_control_bytes_match_terminal_key_events() {
        let ev = key_events_for_text("\x1f\x7f\x08\x1b");
        assert_eq!(
            ev[0],
            KeyEvent::new(KeyCode::Char('7'), KeyModifiers::CONTROL)
        );
        assert_eq!(ev[1], KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(
            ev[2],
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL)
        );
        assert_eq!(ev[3], KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    }
}
