pub mod aliases;
pub mod vars;

use std::collections::HashMap;

use nix::unistd::{Pid, getpid};
use aliases::AliasStore;
use vars::VarStore;

use crate::parser::ast::FunctionDef;

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
}

impl TrapStore {
    /// Convert a signal name or number string to a signal number.
    /// Returns None for unknown signals.
    pub fn signal_name_to_number(name: &str) -> Option<i32> {
        // Try numeric parse first
        if let Ok(n) = name.parse::<i32>() {
            return Some(n);
        }
        match name.to_uppercase().as_str() {
            "EXIT"    => Some(0),
            "HUP"  | "SIGHUP"  => Some(1),
            "INT"  | "SIGINT"  => Some(2),
            "QUIT" | "SIGQUIT" => Some(3),
            "ABRT" | "SIGABRT" => Some(6),
            "KILL" | "SIGKILL" => Some(9),
            "ALRM" | "SIGALRM" => Some(14),
            "TERM" | "SIGTERM" => Some(15),
            _ => None,
        }
    }

    /// Convert a signal number to its canonical name.
    fn signal_number_to_name(num: i32) -> &'static str {
        match num {
            0  => "EXIT",
            1  => "HUP",
            2  => "INT",
            3  => "QUIT",
            6  => "ABRT",
            9  => "KILL",
            14 => "ALRM",
            15 => "TERM",
            _  => "UNKNOWN",
        }
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

    /// Print all active traps in a format suitable for re-input.
    /// Format: `trap -- 'cmd' SIGNAME` or `trap -- '' SIGNAME`.
    /// Default actions are skipped. Exit trap first, then signals sorted by number.
    pub fn display_all(&self) {
        // Exit trap first
        if let Some(action) = &self.exit_trap {
            match action {
                TrapAction::Command(cmd) => println!("trap -- '{}' EXIT", cmd),
                TrapAction::Ignore       => println!("trap -- '' EXIT"),
                TrapAction::Default      => {}
            }
        }
        // Signal traps sorted by number
        let mut keys: Vec<i32> = self.signal_traps.keys().copied().collect();
        keys.sort();
        for num in keys {
            if let Some(action) = self.signal_traps.get(&num) {
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

/// Flow control signals for break, continue, and return.
#[derive(Debug, Clone, PartialEq)]
pub enum FlowControl {
    Break(usize),
    Continue(usize),
    Return(i32),
}

/// POSIX shell option flags (set -o / set +o).
#[derive(Debug, Clone, Default)]
pub struct ShellOptions {
    pub allexport: bool,  // -a
    pub notify: bool,     // -b
    pub noclobber: bool,  // -C
    pub errexit: bool,    // -e
    pub noglob: bool,     // -f
    pub noexec: bool,     // -n
    pub monitor: bool,    // -m
    pub nounset: bool,    // -u
    pub verbose: bool,    // -v
    pub xtrace: bool,     // -x
    pub ignoreeof: bool,
    pub pipefail: bool,
}

impl ShellOptions {
    /// Returns active flags as a string (e.g., "aex") for `$-`.
    /// Order: a, b, C, e, f, m, n, u, v, x
    pub fn to_flag_string(&self) -> String {
        let mut s = String::new();
        if self.allexport  { s.push('a'); }
        if self.notify     { s.push('b'); }
        if self.noclobber  { s.push('C'); }
        if self.errexit    { s.push('e'); }
        if self.noglob     { s.push('f'); }
        if self.monitor    { s.push('m'); }
        if self.noexec     { s.push('n'); }
        if self.nounset    { s.push('u'); }
        if self.verbose    { s.push('v'); }
        if self.xtrace     { s.push('x'); }
        s
    }

    /// Set or unset a flag by its short character.
    pub fn set_by_char(&mut self, c: char, on: bool) -> Result<(), String> {
        match c {
            'a' => self.allexport = on,
            'b' => self.notify    = on,
            'C' => self.noclobber = on,
            'e' => self.errexit   = on,
            'f' => self.noglob    = on,
            'm' => self.monitor   = on,
            'n' => self.noexec    = on,
            'u' => self.nounset   = on,
            'v' => self.verbose   = on,
            'x' => self.xtrace    = on,
            _   => return Err(format!("unknown option: -{}", c)),
        }
        Ok(())
    }

    /// Set or unset a flag by its long name.
    pub fn set_by_name(&mut self, name: &str, on: bool) -> Result<(), String> {
        match name {
            "allexport"  => self.allexport  = on,
            "notify"     => self.notify     = on,
            "noclobber"  => self.noclobber  = on,
            "errexit"    => self.errexit    = on,
            "noglob"     => self.noglob     = on,
            "monitor"    => self.monitor    = on,
            "noexec"     => self.noexec     = on,
            "nounset"    => self.nounset    = on,
            "verbose"    => self.verbose    = on,
            "xtrace"     => self.xtrace     = on,
            "ignoreeof"  => self.ignoreeof  = on,
            "pipefail"   => self.pipefail   = on,
            _            => return Err(format!("unknown option: {}", name)),
        }
        Ok(())
    }

    /// Print all options in "name    on/off" format (sorted alphabetically).
    pub fn display_all(&self) {
        let entries = self.all_entries();
        for (name, value) in &entries {
            println!("{:<12} {}", name, if *value { "on" } else { "off" });
        }
    }

    /// Print in "set -o name" / "set +o name" format.
    pub fn display_restorable(&self) {
        let entries = self.all_entries();
        for (name, value) in &entries {
            if *value {
                println!("set -o {}", name);
            } else {
                println!("set +o {}", name);
            }
        }
    }

    /// Returns all options as sorted (name, value) pairs.
    fn all_entries(&self) -> Vec<(&'static str, bool)> {
        let mut entries: Vec<(&'static str, bool)> = vec![
            ("allexport",  self.allexport),
            ("errexit",    self.errexit),
            ("ignoreeof",  self.ignoreeof),
            ("monitor",    self.monitor),
            ("noclobber",  self.noclobber),
            ("noexec",     self.noexec),
            ("noglob",     self.noglob),
            ("notify",     self.notify),
            ("nounset",    self.nounset),
            ("pipefail",   self.pipefail),
            ("verbose",    self.verbose),
            ("xtrace",     self.xtrace),
        ];
        entries.sort_by_key(|(name, _)| *name);
        entries
    }
}

/// The complete shell environment.
#[derive(Debug, Clone)]
pub struct ShellEnv {
    pub vars: VarStore,
    pub last_exit_status: i32,
    pub shell_pid: Pid,
    pub shell_name: String,
    pub positional_params: Vec<String>,
    /// PID of the most recently started background job ($!)
    pub last_bg_pid: Option<i32>,
    pub functions: HashMap<String, FunctionDef>,
    pub flow_control: Option<FlowControl>,
    pub options: ShellOptions,
    pub traps: TrapStore,
    pub aliases: AliasStore,
}

impl ShellEnv {
    /// Create a new ShellEnv, initializing variables from the process environment.
    ///
    /// `shell_name` is $0 (argv[0]), `args` are the positional parameters ($1, $2, ...).
    pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
        ShellEnv {
            vars: VarStore::from_environ(),
            last_exit_status: 0,
            shell_pid: getpid(),
            shell_name: shell_name.into(),
            positional_params: args,
            last_bg_pid: None,
            functions: HashMap::new(),
            flow_control: None,
            options: ShellOptions::default(),
            traps: TrapStore::default(),
            aliases: AliasStore::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_env_construction() {
        let env = ShellEnv::new("kish", vec!["arg1".to_string(), "arg2".to_string()]);
        assert_eq!(env.shell_name, "kish");
        assert_eq!(env.positional_params, vec!["arg1", "arg2"]);
        assert_eq!(env.last_exit_status, 0);
        // PID should be a positive number
        assert!(env.shell_pid.as_raw() > 0);
    }

    #[test]
    fn test_shell_options_default() {
        let opts = ShellOptions::default();
        assert!(!opts.allexport);
        assert!(!opts.errexit);
        assert!(!opts.noglob);
        assert!(!opts.noexec);
        assert!(!opts.nounset);
        assert!(!opts.verbose);
        assert!(!opts.xtrace);
        assert!(!opts.noclobber);
        assert!(!opts.pipefail);
        assert_eq!(opts.to_flag_string(), "");
    }

    #[test]
    fn test_shell_options_set_by_char() {
        let mut opts = ShellOptions::default();
        opts.set_by_char('a', true).unwrap();
        opts.set_by_char('x', true).unwrap();
        assert!(opts.allexport);
        assert!(opts.xtrace);
        let s = opts.to_flag_string();
        assert!(s.contains('a'));
        assert!(s.contains('x'));

        opts.set_by_char('a', false).unwrap();
        assert!(!opts.allexport);

        assert!(opts.set_by_char('Z', true).is_err());
    }

    #[test]
    fn test_shell_options_set_by_name() {
        let mut opts = ShellOptions::default();
        opts.set_by_name("allexport", true).unwrap();
        assert!(opts.allexport);
        opts.set_by_name("allexport", false).unwrap();
        assert!(!opts.allexport);
        assert!(opts.set_by_name("invalid", true).is_err());
    }

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
}
