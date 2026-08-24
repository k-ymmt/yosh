/// POSIX shell option flags (set -o / set +o).
#[derive(Debug, Clone, Default)]
pub struct ShellOptions {
    pub allexport: bool, // -a
    pub notify: bool,    // -b
    pub noclobber: bool, // -C
    pub errexit: bool,   // -e
    pub noglob: bool,    // -f
    pub noexec: bool,    // -n
    pub monitor: bool,   // -m
    pub nounset: bool,   // -u
    pub verbose: bool,   // -v
    pub xtrace: bool,    // -x
    pub ignoreeof: bool,
    pub pipefail: bool,
    pub cmd_string: bool, // -c
}

impl ShellOptions {
    /// Returns active flags as a string (e.g., "aex") for `$-`.
    /// Order: a, b, C, e, f, m, n, u, v, x
    pub fn to_flag_string(&self) -> String {
        let mut s = String::new();
        if self.allexport {
            s.push('a');
        }
        if self.notify {
            s.push('b');
        }
        if self.cmd_string {
            s.push('c');
        }
        if self.noclobber {
            s.push('C');
        }
        if self.errexit {
            s.push('e');
        }
        if self.noglob {
            s.push('f');
        }
        if self.monitor {
            s.push('m');
        }
        if self.noexec {
            s.push('n');
        }
        if self.nounset {
            s.push('u');
        }
        if self.verbose {
            s.push('v');
        }
        if self.xtrace {
            s.push('x');
        }
        s
    }

    /// Set or unset a flag by its short character.
    pub fn set_by_char(&mut self, c: char, on: bool) -> Result<(), String> {
        match c {
            'a' => self.allexport = on,
            'b' => self.notify = on,
            'C' => self.noclobber = on,
            'e' => self.errexit = on,
            'f' => self.noglob = on,
            'm' => self.monitor = on,
            'n' => self.noexec = on,
            // POSIX requires accepting -h (locate utilities); the behavior
            // itself is obsolescent, so it is a no-op.
            'h' => {}
            'u' => self.nounset = on,
            'v' => self.verbose = on,
            'x' => self.xtrace = on,
            _ => return Err(format!("unknown option: -{}", c)),
        }
        Ok(())
    }

    /// Set or unset a flag by its long name.
    pub fn set_by_name(&mut self, name: &str, on: bool) -> Result<(), String> {
        match name {
            "allexport" => self.allexport = on,
            "notify" => self.notify = on,
            "noclobber" => self.noclobber = on,
            "errexit" => self.errexit = on,
            "noglob" => self.noglob = on,
            "monitor" => self.monitor = on,
            "noexec" => self.noexec = on,
            "nounset" => self.nounset = on,
            "verbose" => self.verbose = on,
            "xtrace" => self.xtrace = on,
            "ignoreeof" => self.ignoreeof = on,
            "pipefail" => self.pipefail = on,
            _ => return Err(format!("unknown option: {}", name)),
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
            ("allexport", self.allexport),
            ("errexit", self.errexit),
            ("ignoreeof", self.ignoreeof),
            ("monitor", self.monitor),
            ("noclobber", self.noclobber),
            ("noexec", self.noexec),
            ("noglob", self.noglob),
            ("notify", self.notify),
            ("nounset", self.nounset),
            ("pipefail", self.pipefail),
            ("verbose", self.verbose),
            ("xtrace", self.xtrace),
        ];
        entries.sort_by_key(|(name, _)| *name);
        entries
    }
}

/// Shell mode and option flags.
#[derive(Debug, Clone)]
pub struct ShellMode {
    pub options: ShellOptions,
    pub is_interactive: bool,
    /// Snapshot of the `i` letter for `$-`, decoupled from the live
    /// `is_interactive` behavior flag. Command-substitution children of
    /// an interactive shell must run with `is_interactive: false` (so
    /// they do not inherit the interactive untrapped-TERM/QUIT/INT
    /// ignore in `handle_default_signal` and become unkillable), yet
    /// POSIX XCU 2.5.2 requires their `$-` to still report `i` —
    /// bash/dash agree. `flag_string` ORs this with `is_interactive`.
    pub flag_i: bool,
    pub in_dot_script: bool,
}

impl ShellMode {
    /// Full `$-` flag string: the option letters plus `i` when the shell
    /// is interactive (POSIX XCU 2.5.2 special parameters). `i` is
    /// invocation state, not a settable option, so it lives here rather
    /// than in [`ShellOptions`].
    pub fn flag_string(&self) -> String {
        let mut s = self.options.to_flag_string();
        if self.is_interactive || self.flag_i {
            // Preserve to_flag_string's alphabetical-ish emit order
            // (a, b, c, C, e, f, [i], m, n, u, v, x).
            let pos = s.find(['m', 'n', 'u', 'v', 'x']).unwrap_or(s.len());
            s.insert(pos, 'i');
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_flag_string_includes_i_when_interactive() {
        let mut mode = ShellMode {
            options: ShellOptions::default(),
            is_interactive: true,
            flag_i: false,
            in_dot_script: false,
        };
        mode.options.monitor = true;
        // `i` slots into the emit order before `m`.
        assert_eq!(mode.flag_string(), "im");

        mode.options.allexport = true;
        assert_eq!(mode.flag_string(), "aim");

        mode.is_interactive = false;
        assert_eq!(mode.flag_string(), "am");
    }

    #[test]
    fn test_flag_string_appends_i_when_no_later_flags() {
        let mode = ShellMode {
            options: ShellOptions::default(),
            is_interactive: true,
            flag_i: false,
            in_dot_script: false,
        };
        assert_eq!(mode.flag_string(), "i");
    }

    #[test]
    fn test_flag_i_snapshot_reports_i_without_is_interactive() {
        // Command-sub children: is_interactive=false (behavior) but the
        // `i` letter snapshot keeps $- faithful to the parent shell.
        let mode = ShellMode {
            options: ShellOptions::default(),
            is_interactive: false,
            flag_i: true,
            in_dot_script: false,
        };
        assert_eq!(mode.flag_string(), "i");
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
}
