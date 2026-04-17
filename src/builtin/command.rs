//! POSIX `command` builtin.
//!
//! `command [-p] [-v|-V] command_name [argument...]`
//!
//! - `-p`  use the POSIX default PATH for lookup (from `confstr(_CS_PATH)`)
//! - `-v`  concise description of `command_name`
//! - `-V`  verbose description of `command_name`
//! - no flags: execute `command_name`, bypassing shell functions
//!
//! This file holds only the flag parser + description output paths. The
//! actual execution (for `-p` and no-flag forms) is dispatched from
//! `exec/simple.rs` so the `command` invocation has access to the
//! `Executor` for redirects/assignments.

/// Parsed form of a `command [...]` invocation.
#[derive(Debug, PartialEq, Eq)]
pub struct CommandFlags {
    pub use_default_path: bool,
    pub verbose: Verbosity,
    pub name: String,
    pub rest: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verbosity {
    /// No `-v` / `-V` flag: this is an execute invocation.
    Execute,
    /// `-v`: concise description.
    Brief,
    /// `-V`: verbose description.
    Verbose,
}

/// Parse the argument list for `command`. Returns `Err(message)` on invalid
/// flags or missing command name. Messages are already formatted for stderr
/// (e.g., `"command: -x: invalid option"`).
pub fn parse_flags(args: &[String]) -> Result<CommandFlags, String> {
    let mut use_default_path = false;
    let mut verbose = Verbosity::Execute;
    let mut idx = 0;

    while idx < args.len() {
        let a = &args[idx];
        if a == "--" {
            idx += 1;
            break;
        }
        if !a.starts_with('-') || a == "-" {
            break;
        }
        // Parse clustered flags: "-pv" = -p -v, "-Vp" = -V -p.
        for ch in a[1..].chars() {
            match ch {
                'p' => use_default_path = true,
                'v' => verbose = Verbosity::Brief,
                'V' => verbose = Verbosity::Verbose,
                other => return Err(format!("command: -{}: invalid option", other)),
            }
        }
        idx += 1;
    }

    if idx >= args.len() {
        return Err("command: missing command name".to_string());
    }

    let name = args[idx].clone();
    let rest = args[idx + 1..].to_vec();
    Ok(CommandFlags { use_default_path, verbose, name, rest })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn no_flags_execute() {
        let p = parse_flags(&v(&["ls", "-l"])).unwrap();
        assert!(!p.use_default_path);
        assert_eq!(p.verbose, Verbosity::Execute);
        assert_eq!(p.name, "ls");
        assert_eq!(p.rest, v(&["-l"]));
    }

    #[test]
    fn p_flag() {
        let p = parse_flags(&v(&["-p", "ls"])).unwrap();
        assert!(p.use_default_path);
        assert_eq!(p.name, "ls");
    }

    #[test]
    fn v_flag() {
        let p = parse_flags(&v(&["-v", "ls"])).unwrap();
        assert_eq!(p.verbose, Verbosity::Brief);
    }

    #[test]
    fn big_v_flag() {
        let p = parse_flags(&v(&["-V", "ls"])).unwrap();
        assert_eq!(p.verbose, Verbosity::Verbose);
    }

    #[test]
    fn combined_flags() {
        let p = parse_flags(&v(&["-pv", "ls"])).unwrap();
        assert!(p.use_default_path);
        assert_eq!(p.verbose, Verbosity::Brief);
    }

    #[test]
    fn double_dash_stops_parsing() {
        let p = parse_flags(&v(&["--", "-v", "arg"])).unwrap();
        assert_eq!(p.verbose, Verbosity::Execute);
        assert_eq!(p.name, "-v");
        assert_eq!(p.rest, v(&["arg"]));
    }

    #[test]
    fn single_dash_is_a_name() {
        let p = parse_flags(&v(&["-"])).unwrap();
        assert_eq!(p.name, "-");
    }

    #[test]
    fn invalid_option_errors() {
        let err = parse_flags(&v(&["-x", "ls"])).unwrap_err();
        assert!(err.contains("-x"));
    }

    #[test]
    fn missing_name_errors() {
        let err = parse_flags(&v(&[])).unwrap_err();
        assert!(err.to_lowercase().contains("missing"));

        let err = parse_flags(&v(&["-v"])).unwrap_err();
        assert!(err.to_lowercase().contains("missing"));
    }
}
