// src/interactive/undo.rs

/// A snapshot of the line buffer state at a point in time.
struct UndoEntry {
    buf: Vec<char>,
    pos: usize,
}

/// Manages undo history as a stack of buffer snapshots.
pub struct UndoManager {
    stack: Vec<UndoEntry>,
    max_size: usize,
}

impl UndoManager {
    pub fn new(max_size: usize) -> Self {
        Self {
            stack: Vec::new(),
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
    pub fn undo(&mut self) -> Option<(Vec<char>, usize)> {
        self.stack.pop().map(|entry| (entry.buf, entry.pos))
    }

    /// Clear all undo history (called on Submit).
    pub fn clear(&mut self) {
        self.stack.clear();
    }
}
