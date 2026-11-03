//! Kill-ring / unnamed-register writes and undo-unit bookkeeping
//! shared by every editing mode.

use super::{LineEditor, YankState};
use crate::interactive::vi::ViFlavor;
use crate::interactive::vim::{RegisterKind, UnnamedRegister};

impl LineEditor {
    /// Per-arm undo snapshot for the shared vi command arms: a no-op in
    /// the vim flavor, where the §7.2 compare-and-commit wrapper owns
    /// all undo bookkeeping.
    pub(super) fn save_undo_posix(&mut self) {
        if self.vi.flavor != ViFlavor::Vim {
            self.undo.save(&self.buf, self.pos);
        }
    }

    /// Commit one vim undo unit: push the pre-state and invalidate the
    /// redo stack. Callers check that the buffer actually changed.
    pub(super) fn vim_commit_undo(&mut self, pre_buf: &[char], pre_pos: usize) {
        self.undo.save(pre_buf, pre_pos);
        self.undo.clear_redo();
    }

    /// Kill-ring write plus the §8.1 front-entry mirror: every kill in
    /// any editing mode routes through here (or
    /// [`record_kill_prepend`](Self::record_kill_prepend)) so the vim
    /// unnamed register always equals the ring's front entry. `kind` is
    /// `Charwise` except for vim-flavor linewise operations.
    pub(super) fn record_kill(&mut self, text: &str, append: bool, kind: RegisterKind) {
        self.kill_ring.kill(text, append);
        self.sync_unnamed_register(kind);
    }

    /// [`record_kill`](Self::record_kill) for backward kills that merge
    /// by prepending to the ring's front entry.
    pub(super) fn record_kill_prepend(&mut self, text: &str, append: bool, kind: RegisterKind) {
        self.kill_ring.prepend(text, append);
        self.sync_unnamed_register(kind);
    }

    /// Copy the kill ring's front entry into the unnamed register.
    /// A no-op when the ring is empty (empty kills are dropped by the
    /// ring and are not writes).
    fn sync_unnamed_register(&mut self, kind: RegisterKind) {
        if let Some(front) = self.kill_ring.front() {
            self.unnamed_register = UnnamedRegister {
                text: front.to_string(),
                kind,
            };
        }
    }

    /// Emacs `C-y`: insert the kill ring's front entry, remembering where
    /// it went so an immediately following `M-y` can replace it.
    pub(super) fn yank(&mut self) {
        if let Some(text) = self.kill_ring.yank().map(|s| s.to_string()) {
            let (start, len) = self.insert_str(&text);
            self.yank_state = Some(YankState { start, len });
        }
    }

    /// Emacs `M-y` right after a yank: replace the yanked text with the
    /// next-older kill ring entry.
    pub(super) fn yank_pop(&mut self) {
        if let Some(ys) = self.yank_state.clone() {
            self.remove_range(ys.start, ys.len);
            if let Some(text) = self.kill_ring.yank_pop().map(|s| s.to_string()) {
                let (start, len) = self.insert_str(&text);
                self.yank_state = Some(YankState { start, len });
            }
        }
    }
}
