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
    pub in_dot_script: bool,
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
    fn test_shell_options_set_by_name() {
        let mut opts = ShellOptions::default();
        opts.set_by_name("allexport", true).unwrap();
        assert!(opts.allexport);
        opts.set_by_name("allexport", false).unwrap();
        assert!(!opts.allexport);
        assert!(opts.set_by_name("invalid", true).is_err());
    }
}
