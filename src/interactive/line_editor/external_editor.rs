//! External-editor integration: vi `v` and vim `Ctrl-X Ctrl-E`.

use std::io;

use super::LineEditor;
use crate::interactive::history::History;
use crate::interactive::terminal::Terminal;

impl LineEditor {
    /// Run `editor` on a temp file seeded with `content`. Returns the
    /// edited text (trailing newline stripped) on success; on failure
    /// requests a bell, invalidates the render state, and returns
    /// `None`. Shared by POSIX-vi `v` and vim `Ctrl-X Ctrl-E`.
    fn run_external_editor<T: Terminal>(
        &mut self,
        term: &mut T,
        content: &str,
        editor: &str,
    ) -> io::Result<Option<String>> {
        // O_EXCL via create_new: refuses to follow a pre-planted symlink
        // at a predictable path and never overwrites an existing file.
        let mut path = std::env::temp_dir();
        let mut created = None;
        for attempt in 0..64u32 {
            let mut candidate = path.clone();
            let uniq = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            candidate.push(format!(
                "yosh-vi-{}-{}-{}.sh",
                std::process::id(),
                uniq,
                attempt
            ));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(f) => {
                    created = Some((candidate, f));
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        let Some((tmp_path, mut file)) = created else {
            self.pending_bell = true;
            return Ok(None);
        };
        path = tmp_path;
        {
            use std::io::Write as _;
            writeln!(file, "{}", content)?;
        }
        drop(file);

        term.reset_style()?;
        term.disable_raw_mode()?;
        let mut parts = editor.split_whitespace();
        let status = parts.next().map(|prog| {
            std::process::Command::new(prog)
                .args(parts)
                .arg(&path)
                .status()
        });
        term.enable_raw_mode()?;

        let ok = matches!(status, Some(Ok(st)) if st.success());
        let result = if ok {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            Some(text.strip_suffix('\n').unwrap_or(&text).to_string())
        } else {
            self.pending_bell = true;
            self.invalidate_render_state();
            None
        };
        let _ = std::fs::remove_file(&path);
        Ok(result)
    }

    /// vi `v`: run the external editor on the current line (or history
    /// entry `entry`; 0 = current line). On success the edited text
    /// replaces the buffer and `Ok(true)` asks the caller to submit it.
    pub(super) fn vi_edit_in_editor<T: Terminal>(
        &mut self,
        term: &mut T,
        history: &History,
        entry: u32,
    ) -> io::Result<bool> {
        let content = if entry == 0 {
            self.buffer()
        } else {
            match history.entries().get(entry as usize - 1) {
                Some(e) => e.clone(),
                None => {
                    self.pending_bell = true;
                    return Ok(false);
                }
            }
        };
        // POSIX prescribes vi; honor the conventional VISUAL/EDITOR
        // overrides first. Shell variables set inside yosh are not
        // process-environment exports, so this reads the inherited
        // environment only (ShellEnv resolution remains TODO residual
        // (b); the vim Ctrl-X Ctrl-E path uses the ShellEnv snapshot).
        let editor = std::env::var("VISUAL")
            .unwrap_or_else(|_| std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string()));
        match self.run_external_editor(term, &content, &editor)? {
            Some(text) => {
                self.buf = text.chars().collect();
                self.pos = self.buf.len();
                self.invalidate_width_cache();
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// vim `Ctrl-X Ctrl-E`: run the snapshotted editor command on the
    /// buffer; the result replaces the buffer (undoable) and the read
    /// continues — the user reviews and presses Enter (zsh
    /// `edit-command-line` behavior).
    pub(super) fn vim_edit_buffer<T: Terminal>(&mut self, term: &mut T) -> io::Result<bool> {
        let content = self.buffer();
        let editor = self.editor_command.clone();
        match self.run_external_editor(term, &content, &editor)? {
            Some(text) => {
                let pre = (self.buf.clone(), self.pos);
                self.buf = text.chars().collect();
                self.pos = self.buf.len();
                // The buffer replacement is one undo unit (§7.2).
                if self.buf != pre.0 {
                    self.vim_commit_undo(&pre.0, pre.1);
                }
                self.invalidate_width_cache();
                self.clamp_vi_command_pos();
                self.suggestion = None;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}
