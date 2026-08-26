// src/interactive/undo.rs

/// A snapshot of the line buffer state at a point in time.
struct UndoEntry {
    buf: Vec<char>,
    pos: usize,
}

/// Manages undo history as a stack of buffer snapshots. The redo stack
/// is used only by the vim flavor (`set -o vim`); emacs and POSIX-vi
/// undo keep their original walk over `undo()`.
pub struct UndoManager {
    stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    max_size: usize,
}

impl UndoManager {
    pub fn new(max_size: usize) -> Self {
        Self {
            stack: Vec::new(),
            redo_stack: Vec::new(),
            max_size,
        }
    }

    /// Save the current buffer state before a modification.
    pub fn save(&mut self, buf: &[char], pos: usize) {
        if self.stack.len() >= self.max_size {
            self.stack.remove(0);
        }
        self.stack.push(UndoEntry {
            buf: buf.to_vec(),
            pos,
        });
    }

    /// Restore the most recently saved state. Returns `None` if the stack is empty.
    /// (emacs / POSIX-vi undo walk; does not touch the redo stack.)
    pub fn undo(&mut self) -> Option<(Vec<char>, usize)> {
        self.stack.pop().map(|entry| (entry.buf, entry.pos))
    }

    /// vim undo: pop the undo stack, pushing the caller's current state
    /// onto the redo stack. `None` (bell) when there is nothing to undo.
    pub fn undo_swap(&mut self, cur_buf: &[char], cur_pos: usize) -> Option<(Vec<char>, usize)> {
        let entry = self.stack.pop()?;
        self.redo_stack.push(UndoEntry {
            buf: cur_buf.to_vec(),
            pos: cur_pos,
        });
        Some((entry.buf, entry.pos))
    }

    /// vim redo: the reverse of [`undo_swap`](Self::undo_swap).
    pub fn redo_swap(&mut self, cur_buf: &[char], cur_pos: usize) -> Option<(Vec<char>, usize)> {
        let entry = self.redo_stack.pop()?;
        self.save(cur_buf, cur_pos);
        Some((entry.buf, entry.pos))
    }

    /// Drop all redo entries — a new committed change invalidates them.
    pub fn clear_redo(&mut self) {
        self.redo_stack.clear();
    }

    /// Clear all undo history (called on Submit and on vim history
    /// recall).
    pub fn clear(&mut self) {
        self.stack.clear();
        self.redo_stack.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn undo_redo_round_trip() {
        let mut u = UndoManager::new(16);
        u.save(&chars("a"), 0); // unit: "a" -> "ab"
        let (b, p) = u.undo_swap(&chars("ab"), 1).unwrap();
        assert_eq!((b.as_slice(), p), (chars("a").as_slice(), 0));
        let (b, p) = u.redo_swap(&chars("a"), 0).unwrap();
        assert_eq!((b.as_slice(), p), (chars("ab").as_slice(), 1));
        // Redo consumed; another redo bells.
        assert!(u.redo_swap(&chars("ab"), 1).is_none());
        // Undo works again after the redo.
        assert!(u.undo_swap(&chars("ab"), 1).is_some());
    }

    #[test]
    fn commit_clears_redo() {
        let mut u = UndoManager::new(16);
        u.save(&chars("a"), 0);
        u.undo_swap(&chars("ab"), 1).unwrap();
        assert!(!u.redo_stack.is_empty());
        u.save(&chars("a"), 0);
        u.clear_redo();
        assert!(u.redo_swap(&chars("ax"), 1).is_none());
    }

    #[test]
    fn plain_undo_does_not_touch_redo() {
        let mut u = UndoManager::new(16);
        u.save(&chars("a"), 0);
        assert!(u.undo().is_some());
        assert!(u.redo_swap(&chars("a"), 0).is_none());
    }

    #[test]
    fn clear_drops_both_stacks() {
        let mut u = UndoManager::new(16);
        u.save(&chars("a"), 0);
        u.undo_swap(&chars("ab"), 1).unwrap();
        u.clear();
        assert!(u.undo().is_none());
        assert!(u.redo_swap(&chars("x"), 0).is_none());
    }
}
