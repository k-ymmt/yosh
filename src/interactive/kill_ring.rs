// src/interactive/kill_ring.rs

use std::collections::VecDeque;

/// Circular buffer of killed (cut) text, supporting yank and yank-pop.
pub struct KillRing {
    ring: VecDeque<String>,
    max_size: usize,
    yank_index: usize,
}

impl KillRing {
    pub fn new(max_size: usize) -> Self {
        Self {
            ring: VecDeque::new(),
            max_size,
            yank_index: 0,
        }
    }

    /// Add text to the kill ring.
    /// If `append` is true, concatenate to the most recent entry (for consecutive forward kills).
    pub fn kill(&mut self, text: &str, append: bool) {
        if text.is_empty() {
            return;
        }
        if append && !self.ring.is_empty() {
            let front = self.ring.front_mut().unwrap();
            front.push_str(text);
        } else {
            self.ring.push_front(text.to_string());
            if self.ring.len() > self.max_size {
                self.ring.pop_back();
            }
        }
        self.yank_index = 0;
    }

    /// Prepend text to the most recent entry (for consecutive backward kills).
    /// If `append` is false or ring is empty, behaves like `kill()`.
    pub fn prepend(&mut self, text: &str, append: bool) {
        if text.is_empty() {
            return;
        }
        if append && !self.ring.is_empty() {
            let front = self.ring.front_mut().unwrap();
            front.insert_str(0, text);
        } else {
            self.ring.push_front(text.to_string());
            if self.ring.len() > self.max_size {
                self.ring.pop_back();
            }
        }
        self.yank_index = 0;
    }

    /// Return the most recent kill (for Ctrl+Y). Resets yank_index to 0.
    pub fn yank(&mut self) -> Option<&str> {
        if self.ring.is_empty() {
            return None;
        }
        self.yank_index = 0;
        Some(self.ring[0].as_str())
    }

    /// Cycle to the next older entry in the kill ring (for Alt+Y).
    /// Returns None if the ring is empty.
    pub fn yank_pop(&mut self) -> Option<&str> {
        if self.ring.is_empty() {
            return None;
        }
        self.yank_index = (self.yank_index + 1) % self.ring.len();
        Some(self.ring[self.yank_index].as_str())
    }
}
