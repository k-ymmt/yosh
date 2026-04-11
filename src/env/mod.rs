pub mod aliases;
pub mod jobs;
pub mod traps;
pub mod vars;

use std::collections::HashMap;

use nix::unistd::{Pid, getpid};
use jobs::JobTable;
use aliases::AliasStore;
use vars::VarStore;
pub use traps::{TrapAction, TrapStore};

use crate::parser::ast::FunctionDef;

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
    pub cmd_string: bool, // -c
}

impl ShellOptions {
    /// Returns active flags as a string (e.g., "aex") for `$-`.
    /// Order: a, b, C, e, f, m, n, u, v, x
    pub fn to_flag_string(&self) -> String {
        let mut s = String::new();
        if self.allexport  { s.push('a'); }
        if self.notify     { s.push('b'); }
        if self.cmd_string { s.push('c'); }
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

/// Execution-related state.
#[derive(Debug, Clone)]
pub struct ExecState {
    pub last_exit_status: i32,
    pub flow_control: Option<FlowControl>,
}

/// Process and job management state.
#[derive(Debug, Clone)]
pub struct ProcessState {
    pub shell_pid: Pid,
    pub shell_pgid: Pid,
    pub jobs: JobTable,
}

/// Shell mode and option flags.
#[derive(Debug, Clone)]
pub struct ShellMode {
    pub options: ShellOptions,
    pub is_interactive: bool,
    pub in_dot_script: bool,
}

/// The complete shell environment.
#[derive(Debug, Clone)]
pub struct ShellEnv {
    pub vars: VarStore,
    pub exec: ExecState,
    pub process: ProcessState,
    pub mode: ShellMode,
    pub functions: HashMap<String, FunctionDef>,
    pub traps: TrapStore,
    pub aliases: AliasStore,
    pub shell_name: String,
}

impl ShellEnv {
    /// Create a new ShellEnv, initializing variables from the process environment.
    ///
    /// `shell_name` is $0 (argv[0]), `args` are the positional parameters ($1, $2, ...).
    pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
        let mut vars = VarStore::from_environ();
        vars.set_positional_params(args);
        ShellEnv {
            vars,
            exec: ExecState {
                last_exit_status: 0,
                flow_control: None,
            },
            process: ProcessState {
                shell_pid: getpid(),
                shell_pgid: nix::unistd::getpgrp(),
                jobs: JobTable::default(),
            },
            mode: ShellMode {
                options: ShellOptions::default(),
                is_interactive: false,
                in_dot_script: false,
            },
            shell_name: shell_name.into(),
            functions: HashMap::new(),
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
        assert_eq!(env.vars.positional_params(), &["arg1", "arg2"]);
        assert_eq!(env.exec.last_exit_status, 0);
        // PID should be a positive number
        assert!(env.process.shell_pid.as_raw() > 0);
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
    fn test_jobs_table() {
        let env = ShellEnv::new("kish", vec![]);
        assert!(env.process.jobs.is_empty());
    }

    #[test]
    fn test_shell_pgid() {
        let env = ShellEnv::new("kish", vec![]);
        assert!(env.process.shell_pgid.as_raw() > 0);
    }
}
