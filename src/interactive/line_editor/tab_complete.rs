//! Tab completion: invoking the completion engines and inserting the
//! (quoted) result into the buffer.

use std::io;

use super::LineEditor;
use crate::interactive::command_completion::CommandCompletionContext;
use crate::interactive::completion::{
    self, CompletionContext, CompletionUI, extract_completion_word, is_command_position,
};
use crate::interactive::spec_completion::{self, SpecStore};
use crate::interactive::terminal::Terminal;

impl LineEditor {
    pub(super) fn handle_tab_complete<T: Terminal>(
        &mut self,
        term: &mut T,
        prompt: &str,
        upper_lines: &[String],
        ctx: &CompletionContext,
        cmd_ctx: &mut CommandCompletionContext<'_>,
        specs: &mut SpecStore,
    ) -> io::Result<()> {
        // `self.pos` is a char index into `self.buf`; the completion APIs
        // (`extract_completion_word`, `completion::complete`) take BYTE
        // indices into the rendered buffer string. Convert once here —
        // passing the char index would slice mid-character (panic) as soon
        // as the buffer contains a multibyte char before the cursor.
        let byte_pos: usize = self.buf[..self.pos].iter().map(|c| c.len_utf8()).sum();
        let (word_start, word) = {
            let buf = self.buffer();
            let (ws, w) = extract_completion_word(&buf, byte_pos);
            (ws, w.to_owned())
        };
        let is_cmd_pos = {
            let buf = self.buffer();
            is_command_position(&buf, word_start)
        };

        // Command-name completion of a word the user quoted (`\printf`,
        // `'ls`): the quoting suppresses alias substitution, so it must
        // survive completion. Holds the unquoted word; candidates that
        // extend it keep the typed text and only append the rest.
        let mut quoted_cmd_word: Option<String> = None;
        let (candidates, common_prefix, dir_prefix) = if is_cmd_pos && !word.contains('/') {
            // Command name completion (matched against the unquoted word;
            // the whole typed word is replaced, so dir_prefix is empty)
            let unquoted = completion::shell_unquote(&word);
            if word.contains(['\\', '\'', '"']) {
                quoted_cmd_word = Some(unquoted.clone());
            }
            let (cands, common) = cmd_ctx.completer.complete_common_prefix(
                &unquoted,
                cmd_ctx.path,
                cmd_ctx.builtins,
                cmd_ctx.aliases,
            );
            (cands, common, String::new())
        } else if let Some(result) =
            spec_completion::try_complete(&self.buffer(), word_start, &word, specs, ctx, cmd_ctx)
        {
            // Spec-based completion (user-defined per-command TOML)
            (result.candidates, result.common_prefix, result.keep_prefix)
        } else {
            // Path completion (existing)
            let result = completion::complete(&self.buffer(), byte_pos, ctx);
            (result.candidates, result.common_prefix, result.dir_prefix)
        };

        if candidates.is_empty() {
            return Ok(());
        }

        // `replace_word` only replaces up to the cursor; with text after
        // the cursor (mid-buffer completion), appending the closing quote
        // + trailing space would corrupt the tail — `cat "foo<cursor>BAR"`
        // became `cat "foobar" BAR"`. Only close the argument when the
        // cursor is at the end of the buffer.
        let at_end = self.pos == self.buf.len();
        let replacement_for = |name: &str, finish: bool| match quoted_cmd_word
            .as_deref()
            .and_then(|unq| name.strip_prefix(unq))
        {
            Some(rest) => completion::completion_replacement(&word, rest, finish),
            None => completion::completion_replacement(&dir_prefix, name, finish),
        };

        // Candidates are raw names; `completion_replacement` quotes them
        // for the context open at the end of `dir_prefix` (backslash
        // escapes when unquoted) so a name like `sub dir` or
        // `x$(cmd)y` is read back literally, and closes an open quote +
        // appends a space after a completed non-directory (bash-like).
        if self.tab_count == 1 {
            if candidates.len() == 1 {
                // Single candidate: replace word
                let replacement = replacement_for(&candidates[0], at_end);
                self.replace_word(word_start, &replacement);
            } else {
                // Multiple candidates: replace with common prefix if longer
                // (byte lengths on both sides: `word_start`/`byte_pos` are
                // byte offsets into the rendered buffer).
                let current_word_len = self.buffer()[word_start..byte_pos].len();
                let new_word = replacement_for(&common_prefix, false);
                if new_word.len() > current_word_len {
                    self.replace_word(word_start, &new_word);
                }
            }
        } else if self.tab_count >= 2 && candidates.len() >= 2 {
            // Show interactive completion UI
            self.suggestion = None;
            term.disable_raw_mode()?;
            let selected = CompletionUI::run(&candidates, term)?;
            if let Some(sel) = selected {
                let replacement = replacement_for(&sel, at_end);
                self.replace_word(word_start, &replacement);
            }
            term.enable_raw_mode()?;
            self.repaint_prompt(term, prompt, upper_lines, false)?;
        }

        Ok(())
    }

    /// Replace the word starting at byte offset `word_start` with `replacement`.
    fn replace_word(&mut self, word_start: usize, replacement: &str) {
        // Convert byte offset to char index
        let char_start = self.buffer()[..word_start].chars().count();
        // Drain chars from char_start to current pos
        let drain_end = self.pos;
        self.buf.drain(char_start..drain_end);
        // Insert replacement chars at char_start
        let rep_chars: Vec<char> = replacement.chars().collect();
        let rep_len = rep_chars.len();
        for (i, ch) in rep_chars.into_iter().enumerate() {
            self.buf.insert(char_start + i, ch);
        }
        self.pos = char_start + rep_len;
        self.invalidate_width_cache();
    }
}
