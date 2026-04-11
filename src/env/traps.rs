use std::collections::HashMap;

/// Action to take when a trap fires.
#[derive(Debug, Clone, PartialEq)]
pub enum TrapAction {
    Default,
    Ignore,
    Command(String),
}

/// Storage for shell trap settings.
#[derive(Debug, Clone, Default)]
pub struct TrapStore {
    pub exit_trap: Option<TrapAction>,
    pub signal_traps: HashMap<i32, TrapAction>,
    /// Saved parent traps for display in subshells (POSIX: $(trap) shows parent traps).
    saved_traps: Option<Box<(Option<TrapAction>, HashMap<i32, TrapAction>)>>,
}

impl TrapStore {
    /// Convert a signal name or number string to a signal number.
    /// Returns None for unknown signals.
    pub fn signal_name_to_number(name: &str) -> Option<i32> {
        // Try numeric parse first
        if let Ok(n) = name.parse::<i32>() {
            return Some(n);
        }
        // "EXIT" is trap-specific (signal 0)
        if name.eq_ignore_ascii_case("EXIT") {
            return Some(0);
        }
        // Delegate to the canonical signal table
        crate::signal::signal_name_to_number(name).ok()
    }

    /// Convert a signal number to its canonical name.
    fn signal_number_to_name(num: i32) -> &'static str {
        if num == 0 {
            return "EXIT";
        }
        crate::signal::signal_number_to_name(num).unwrap_or("UNKNOWN")
    }

    /// Set a trap for the given condition (signal name or number).
    pub fn set_trap(&mut self, condition: &str, action: TrapAction) -> Result<(), String> {
        let num = Self::signal_name_to_number(condition)
            .ok_or_else(|| format!("invalid signal name: {}", condition))?;
        if num == 0 {
            self.exit_trap = Some(action);
        } else {
            self.signal_traps.insert(num, action);
        }
        Ok(())
    }

    /// Get the trap action for the given condition (signal name or number).
    #[allow(dead_code)]
    pub fn get_trap(&self, condition: &str) -> Option<&TrapAction> {
        let num = Self::signal_name_to_number(condition)?;
        if num == 0 {
            self.exit_trap.as_ref()
        } else {
            self.signal_traps.get(&num)
        }
    }

    /// Remove/reset the trap for the given condition.
    pub fn remove_trap(&mut self, condition: &str) {
        if let Some(num) = Self::signal_name_to_number(condition) {
            if num == 0 {
                self.exit_trap = None;
            } else {
                self.signal_traps.remove(&num);
            }
        }
    }

    /// Reset all non-ignored traps to default (POSIX subshell behavior).
    /// Command traps are removed. Ignore traps are preserved.
    pub fn reset_non_ignored(&mut self) {
        if matches!(self.exit_trap, Some(TrapAction::Command(_))) {
            self.exit_trap = None;
        }
        self.signal_traps.retain(|_, action| matches!(action, TrapAction::Ignore));
    }

    /// Reset traps for command substitution context.
    /// Saves parent traps so `trap` (no args) in `$(trap)` shows parent traps (POSIX).
    pub fn reset_for_command_sub(&mut self) {
        self.saved_traps = Some(Box::new((
            self.exit_trap.clone(),
            self.signal_traps.clone(),
        )));
        self.reset_non_ignored();
    }

    /// Return signal numbers that have TrapAction::Ignore disposition.
    pub fn ignored_signals(&self) -> Vec<i32> {
        self.signal_traps
            .iter()
            .filter(|(_, action)| matches!(action, TrapAction::Ignore))
            .map(|(&num, _)| num)
            .collect()
    }

    /// Get the trap action for a signal by number (not EXIT).
    pub fn get_signal_trap(&self, sig: i32) -> Option<&TrapAction> {
        self.signal_traps.get(&sig)
    }

    /// Print all active traps in a format suitable for re-input.
    /// Format: `trap -- 'cmd' SIGNAME` or `trap -- '' SIGNAME`.
    /// Default actions are skipped. Exit trap first, then signals sorted by number.
    /// In subshells, displays the saved parent traps (POSIX requirement).
    pub fn display_all(&self) {
        let (exit_trap, signal_traps) = if let Some(saved) = &self.saved_traps {
            (&saved.0, &saved.1)
        } else {
            (&self.exit_trap, &self.signal_traps)
        };

        // Exit trap first
        if let Some(action) = exit_trap {
            match action {
                TrapAction::Command(cmd) => println!("trap -- '{}' EXIT", cmd),
                TrapAction::Ignore       => println!("trap -- '' EXIT"),
                TrapAction::Default      => {}
            }
        }
        // Signal traps sorted by number
        let mut keys: Vec<i32> = signal_traps.keys().copied().collect();
        keys.sort();
        for num in keys {
            if let Some(action) = signal_traps.get(&num) {
                let name = Self::signal_number_to_name(num);
                match action {
                    TrapAction::Command(cmd) => println!("trap -- '{}' SIG{}", cmd, name),
                    TrapAction::Ignore       => println!("trap -- '' SIG{}", name),
                    TrapAction::Default      => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trap_store_default() {
        let store = TrapStore::default();
        assert!(store.exit_trap.is_none());
        assert!(store.signal_traps.is_empty());
    }

    #[test]
    fn test_trap_store_set_exit() {
        let mut store = TrapStore::default();
        store.set_trap("EXIT", TrapAction::Command("echo bye".to_string())).unwrap();
        assert!(matches!(store.get_trap("EXIT"), Some(TrapAction::Command(_))));
    }

    #[test]
    fn test_trap_store_set_signal() {
        let mut store = TrapStore::default();
        store.set_trap("INT", TrapAction::Ignore).unwrap();
        assert!(matches!(store.get_trap("INT"), Some(TrapAction::Ignore)));
        store.set_trap("INT", TrapAction::Default).unwrap();
        assert!(matches!(store.get_trap("INT"), Some(TrapAction::Default)));
    }

    #[test]
    fn test_trap_store_signal_name_to_number() {
        assert_eq!(TrapStore::signal_name_to_number("EXIT"), Some(0));
        assert_eq!(TrapStore::signal_name_to_number("HUP"), Some(1));
        assert_eq!(TrapStore::signal_name_to_number("INT"), Some(2));
        assert_eq!(TrapStore::signal_name_to_number("QUIT"), Some(3));
        assert_eq!(TrapStore::signal_name_to_number("TERM"), Some(15));
        assert_eq!(TrapStore::signal_name_to_number("0"), Some(0));
        assert_eq!(TrapStore::signal_name_to_number("2"), Some(2));
        assert_eq!(TrapStore::signal_name_to_number("INVALID"), None);
    }

    #[test]
    fn test_trap_store_remove() {
        let mut store = TrapStore::default();
        store.set_trap("EXIT", TrapAction::Command("echo bye".to_string())).unwrap();
        store.remove_trap("EXIT");
        assert!(store.exit_trap.is_none());
    }

    #[test]
    fn test_trap_store_reset_non_ignored() {
        let mut store = TrapStore::default();
        store.set_trap("INT", TrapAction::Command("echo caught".to_string())).unwrap();
        store.set_trap("HUP", TrapAction::Ignore).unwrap();
        store.set_trap("TERM", TrapAction::Command("echo term".to_string())).unwrap();
        store.reset_non_ignored();
        assert!(store.signal_traps.get(&2).is_none());
        assert_eq!(store.signal_traps.get(&1), Some(&TrapAction::Ignore));
        assert!(store.signal_traps.get(&15).is_none());
    }

    #[test]
    fn test_trap_store_get_signal_trap() {
        let mut store = TrapStore::default();
        store.set_trap("INT", TrapAction::Command("echo caught".to_string())).unwrap();
        assert!(matches!(store.get_signal_trap(2), Some(TrapAction::Command(_))));
        assert!(store.get_signal_trap(15).is_none());
    }
}
