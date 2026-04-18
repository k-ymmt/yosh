use crate::env::ShellEnv;
use crate::error::{RuntimeErrorKind, ShellError};
use nix::unistd::Pid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CdMode {
    Logical,
    Physical,
}

pub fn builtin_cd(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    let is_dash = !args.is_empty() && args[0] == "-";

    let target = if args.is_empty() {
        match env.vars.get("HOME") {
            Some(h) => h.to_string(),
            None => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidArgument,
                    "cd: HOME not set",
                ));
            }
        }
    } else if args[0] == "-" {
        match env.vars.get("OLDPWD") {
            Some(old) => old.to_string(),
            None => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidArgument,
                    "cd: OLDPWD not set",
                ));
            }
        }
    } else {
        args[0].clone()
    };

    // Save current directory BEFORE attempting chdir
    let old_pwd = std::env::current_dir().ok();

    match std::env::set_current_dir(&target) {
        Ok(_) => {
            // Only update OLDPWD after successful chdir
            if let Some(old) = old_pwd {
                let _ = env.vars.set("OLDPWD", old.to_string_lossy().to_string());
            }
            if is_dash {
                println!("{}", target);
            }
            // Update $PWD
            match std::env::current_dir() {
                Ok(cwd) => {
                    let cwd_str = cwd.to_string_lossy().into_owned();
                    let _ = env.vars.set("PWD", cwd_str);
                }
                Err(e) => {
                    eprintln!("yosh: cd: could not determine new directory: {}", e);
                }
            }
            Ok(0)
        }
        Err(e) => Err(ShellError::runtime(
            RuntimeErrorKind::IoError,
            format!("cd: {}: {}", target, e),
        )),
    }
}

pub fn builtin_echo(args: &[String]) -> Result<i32, ShellError> {
    if args.first().map(|a| a.as_str()) == Some("-n") {
        print!("{}", args[1..].join(" "));
    } else {
        println!("{}", args.join(" "));
    }
    Ok(0)
}

pub fn builtin_alias(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    if args.is_empty() {
        for (name, value) in env.aliases.sorted_iter() {
            println!("alias {}='{}'", name, value);
        }
        return Ok(0);
    }
    let mut status = 0;
    for arg in args {
        if let Some(pos) = arg.find('=') {
            let name = &arg[..pos];
            let value = &arg[pos + 1..];
            env.aliases.set(name, value);
        } else {
            match env.aliases.get(arg) {
                Some(value) => println!("alias {}='{}'", arg, value),
                None => {
                    eprintln!("yosh: alias: {}: not found", arg);
                    status = 1;
                }
            }
        }
    }
    Ok(status)
}

pub fn builtin_unalias(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    if args.is_empty() {
        return Err(ShellError::runtime(
            RuntimeErrorKind::InvalidArgument,
            "unalias: usage: unalias name [name ...]",
        ));
    }
    let mut status = 0;
    for arg in args {
        if arg == "-a" {
            env.aliases.clear();
        } else if !env.aliases.remove(arg) {
            eprintln!("yosh: unalias: {}: not found", arg);
            status = 1;
        }
    }
    Ok(status)
}

pub fn builtin_kill(args: &[String], shell_pgid: Pid) -> Result<i32, ShellError> {
    if args.is_empty() {
        return Err(ShellError::runtime(
            RuntimeErrorKind::InvalidArgument,
            "kill: usage: kill [-s sigspec | -signum] pid...",
        ));
    }

    if args[0] == "-l" {
        return kill_list(&args[1..]);
    }

    let (sig_num, pid_args) = match parse_kill_signal(args) {
        Ok(v) => v,
        Err(msg) => {
            return Err(ShellError::runtime(
                RuntimeErrorKind::InvalidArgument,
                format!("kill: {}", msg),
            ));
        }
    };

    let mut status = 0;
    for pid_str in pid_args {
        let pid: i32 = match pid_str.parse() {
            Ok(n) => n,
            Err(_) => {
                eprintln!("yosh: kill: {}: invalid pid", pid_str);
                status = 1;
                continue;
            }
        };
        // PID 0 means "caller's process group" to the kernel, but in a pipeline
        // subshell the caller's group is the pipeline's group. Substitute the
        // shell's original process group so kill 0 behaves as POSIX expects.
        let target = if pid == 0 {
            let gpid = shell_pgid.as_raw();
            if gpid <= 1 {
                eprintln!("yosh: kill: invalid shell process group");
                status = 1;
                continue;
            }
            Pid::from_raw(-gpid)
        } else {
            Pid::from_raw(pid)
        };
        if let Err(e) = nix::sys::signal::kill(
            target,
            nix::sys::signal::Signal::try_from(sig_num).ok(),
        ) {
            eprintln!("yosh: kill: ({}) - {}", pid_str, e);
            status = 1;
        }
    }
    Ok(status)
}

fn parse_kill_signal(args: &[String]) -> Result<(i32, &[String]), String> {
    if args[0] == "-s" {
        if args.len() < 3 {
            return Err("option requires an argument -- s".to_string());
        }
        let sig = crate::signal::signal_name_to_number(&args[1])?;
        Ok((sig, &args[2..]))
    } else if args[0] == "--" {
        Ok((libc::SIGTERM, &args[1..]))
    } else if args[0].starts_with('-') && args[0].len() > 1 {
        let spec = &args[0][1..];
        if let Ok(num) = spec.parse::<i32>() {
            Ok((num, &args[1..]))
        } else {
            let sig = crate::signal::signal_name_to_number(spec)?;
            Ok((sig, &args[1..]))
        }
    } else {
        Ok((libc::SIGTERM, args))
    }
}

fn kill_list(args: &[String]) -> Result<i32, ShellError> {
    if args.is_empty() {
        let names: Vec<&str> = crate::signal::SIGNAL_TABLE
            .iter()
            .map(|&(_, name)| name)
            .collect();
        println!("{}", names.join(" "));
        return Ok(0);
    }
    for arg in args {
        if let Ok(num) = arg.parse::<i32>() {
            let sig = if num > 128 { num - 128 } else { num };
            match crate::signal::signal_number_to_name(sig) {
                Some(name) => println!("{}", name),
                None => {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::InvalidArgument,
                        format!("kill: {}: invalid signal number", arg),
                    ));
                }
            }
        } else {
            match crate::signal::signal_name_to_number(arg) {
                Ok(num) => println!("{}", num),
                Err(e) => {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::InvalidArgument,
                        format!("kill: {}", e),
                    ));
                }
            }
        }
    }
    Ok(0)
}

pub fn builtin_umask(args: &[String]) -> Result<i32, ShellError> {
    if args.is_empty() {
        let current = unsafe { libc::umask(0) };
        unsafe { libc::umask(current) };
        println!("{:04o}", current);
        return Ok(0);
    }

    if args[0] == "-S" {
        let current = unsafe { libc::umask(0) };
        unsafe { libc::umask(current) };
        println!("{}", umask_to_symbolic(current));
        return Ok(0);
    }

    // Try octal parse first
    if args[0].chars().all(|c| c.is_ascii_digit()) {
        return umask_set_octal(&args[0]);
    }

    // Try symbolic parse
    umask_set_symbolic(&args[0])
}

fn umask_to_symbolic(mask: libc::mode_t) -> String {
    let perms = 0o777 & !mask;
    let fmt = |bits: libc::mode_t| -> String {
        let mut s = String::new();
        if bits & 4 != 0 { s.push('r'); }
        if bits & 2 != 0 { s.push('w'); }
        if bits & 1 != 0 { s.push('x'); }
        s
    };
    format!(
        "u={},g={},o={}",
        fmt((perms >> 6) & 7),
        fmt((perms >> 3) & 7),
        fmt(perms & 7),
    )
}

fn umask_set_octal(s: &str) -> Result<i32, ShellError> {
    for c in s.chars() {
        if !('0'..='7').contains(&c) {
            return Err(ShellError::runtime(
                RuntimeErrorKind::InvalidArgument,
                format!("umask: {}: invalid octal number", s),
            ));
        }
    }
    match libc::mode_t::from_str_radix(s, 8) {
        Ok(mode) => {
            unsafe { libc::umask(mode) };
            Ok(0)
        }
        Err(_) => Err(ShellError::runtime(
            RuntimeErrorKind::InvalidArgument,
            format!("umask: {}: invalid octal number", s),
        )),
    }
}

pub(crate) fn parse_cd_options(
    args: &[String],
) -> Result<(CdMode, Option<String>), ShellError> {
    let mut mode = CdMode::Logical;
    let mut iter = args.iter();
    let operand: Option<String>;

    loop {
        match iter.next() {
            None => {
                operand = None;
                break;
            }
            Some(a) if a == "--" => {
                operand = iter.next().cloned();
                if iter.next().is_some() {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::IoError,
                        "cd: too many arguments",
                    ));
                }
                break;
            }
            Some(a) if a == "-" => {
                operand = Some(a.clone());
                if iter.next().is_some() {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::IoError,
                        "cd: too many arguments",
                    ));
                }
                break;
            }
            Some(a) if a.starts_with('-') && a.len() >= 2 => {
                for ch in a[1..].chars() {
                    match ch {
                        'L' => mode = CdMode::Logical,
                        'P' => mode = CdMode::Physical,
                        other => {
                            return Err(ShellError::runtime(
                                RuntimeErrorKind::InvalidArgument,
                                format!("cd: -{}: invalid option", other),
                            ));
                        }
                    }
                }
                // continue parsing
            }
            Some(a) => {
                operand = Some(a.clone());
                if iter.next().is_some() {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::IoError,
                        "cd: too many arguments",
                    ));
                }
                break;
            }
        }
    }

    Ok((mode, operand))
}

/// Lexical path canonicalization per POSIX §4 cd step 8 (logical mode).
/// Pure string operation: does not touch the filesystem.
/// Handles: leading-`/` absolute vs relative (prepend `pwd`), `.` skip,
/// `..` lexical pop, `//` collapse. When popping past the root, stays
/// at `/`.
pub(crate) fn lexical_canonicalize(path: &str, pwd: &str) -> String {
    let combined: String = if path.starts_with('/') {
        path.to_string()
    } else if path.is_empty() {
        pwd.to_string()
    } else {
        format!("{}/{}", pwd.trim_end_matches('/'), path)
    };

    let mut stack: Vec<&str> = Vec::new();
    for comp in combined.split('/') {
        match comp {
            "" | "." => continue,
            ".." => {
                if stack.last().map(|s| *s != "..").unwrap_or(false) {
                    stack.pop();
                } else if !combined.starts_with('/') {
                    stack.push("..");
                }
                // absolute path: dotdot above root is a no-op
            }
            other => stack.push(other),
        }
    }

    if stack.is_empty() {
        "/".to_string()
    } else {
        let mut out = String::new();
        for c in &stack {
            out.push('/');
            out.push_str(c);
        }
        out
    }
}

fn umask_set_symbolic(s: &str) -> Result<i32, ShellError> {
    let current = unsafe { libc::umask(0) };
    unsafe { libc::umask(current) };

    let mut mask = current;

    for clause in s.split(',') {
        let bytes = clause.as_bytes();
        if bytes.is_empty() {
            return Err(ShellError::runtime(
                RuntimeErrorKind::InvalidArgument,
                format!("umask: {}: invalid symbolic mode", s),
            ));
        }

        let mut i = 0;
        let mut who_mask: libc::mode_t = 0;

        // Parse who (u/g/o/a)
        while i < bytes.len() && matches!(bytes[i], b'u' | b'g' | b'o' | b'a') {
            match bytes[i] {
                b'u' => who_mask |= 0o700,
                b'g' => who_mask |= 0o070,
                b'o' => who_mask |= 0o007,
                b'a' => who_mask |= 0o777,
                _ => unreachable!(),
            }
            i += 1;
        }

        // Default to 'a' if no who specified
        if who_mask == 0 {
            who_mask = 0o777;
        }

        // Parse operator (=, +, -)
        if i >= bytes.len() || !matches!(bytes[i], b'=' | b'+' | b'-') {
            return Err(ShellError::runtime(
                RuntimeErrorKind::InvalidArgument,
                format!("umask: {}: invalid symbolic mode", s),
            ));
        }
        let op = bytes[i] as char;
        i += 1;

        // Parse permissions (r/w/x)
        let mut perm_bits: libc::mode_t = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'r' => perm_bits |= 0o444,
                b'w' => perm_bits |= 0o222,
                b'x' => perm_bits |= 0o111,
                _ => {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::InvalidArgument,
                        format!("umask: {}: invalid symbolic mode", s),
                    ));
                }
            }
            i += 1;
        }

        // Apply within the who mask
        let effective_perms = perm_bits & who_mask;

        match op {
            '=' => {
                // Clear who bits, then set umask to deny everything NOT in perm
                mask = (mask & !who_mask) | (who_mask & !effective_perms);
            }
            '+' => {
                // Adding permissions = clearing umask bits
                mask &= !effective_perms;
            }
            '-' => {
                // Removing permissions = setting umask bits
                mask |= effective_perms;
            }
            _ => unreachable!(),
        }
    }

    unsafe { libc::umask(mask) };
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    // ── parse_cd_options ─────────────────────────────────────────

    #[test]
    fn parse_no_args_defaults_to_logical_none() {
        let (mode, op) = parse_cd_options(&[]).unwrap();
        assert_eq!(mode, CdMode::Logical);
        assert_eq!(op, None);
    }

    #[test]
    fn parse_dash_is_operand_not_option() {
        let (mode, op) = parse_cd_options(&s(&["-"])).unwrap();
        assert_eq!(mode, CdMode::Logical);
        assert_eq!(op.as_deref(), Some("-"));
    }

    #[test]
    fn parse_l_flag() {
        let (mode, op) = parse_cd_options(&s(&["-L"])).unwrap();
        assert_eq!(mode, CdMode::Logical);
        assert_eq!(op, None);
    }

    #[test]
    fn parse_p_flag() {
        let (mode, op) = parse_cd_options(&s(&["-P"])).unwrap();
        assert_eq!(mode, CdMode::Physical);
        assert_eq!(op, None);
    }

    #[test]
    fn parse_flag_with_operand() {
        let (mode, op) = parse_cd_options(&s(&["-P", "/tmp"])).unwrap();
        assert_eq!(mode, CdMode::Physical);
        assert_eq!(op.as_deref(), Some("/tmp"));
    }

    #[test]
    fn parse_combined_flags_last_wins() {
        let (mode, _) = parse_cd_options(&s(&["-LP"])).unwrap();
        assert_eq!(mode, CdMode::Physical);
        let (mode, _) = parse_cd_options(&s(&["-PL"])).unwrap();
        assert_eq!(mode, CdMode::Logical);
    }

    #[test]
    fn parse_separate_flags_last_wins() {
        let (mode, op) = parse_cd_options(&s(&["-L", "-P", "foo"])).unwrap();
        assert_eq!(mode, CdMode::Physical);
        assert_eq!(op.as_deref(), Some("foo"));
    }

    #[test]
    fn parse_double_dash_terminates_options() {
        let (mode, op) = parse_cd_options(&s(&["--", "-foo"])).unwrap();
        assert_eq!(mode, CdMode::Logical);
        assert_eq!(op.as_deref(), Some("-foo"));
    }

    #[test]
    fn parse_invalid_option_errors() {
        let err = parse_cd_options(&s(&["-x"])).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("invalid option"));
    }

    #[test]
    fn parse_too_many_operands_errors() {
        let err = parse_cd_options(&s(&["a", "b"])).unwrap_err();
        assert!(err.to_string().contains("too many arguments"));
    }

    // ── lexical_canonicalize ─────────────────────────────────────

    #[test]
    fn lex_absolute_returned_as_is() {
        assert_eq!(lexical_canonicalize("/tmp", "/Users/foo"), "/tmp");
    }

    #[test]
    fn lex_absolute_with_dotdot() {
        assert_eq!(lexical_canonicalize("/tmp/../etc", "/"), "/etc");
    }

    #[test]
    fn lex_relative_resolves_against_pwd() {
        assert_eq!(lexical_canonicalize("../bar", "/tmp/foo"), "/tmp/bar");
    }

    #[test]
    fn lex_single_dots_skipped() {
        assert_eq!(lexical_canonicalize("./foo/./bar", "/tmp"), "/tmp/foo/bar");
    }

    #[test]
    fn lex_repeated_slashes_collapsed() {
        assert_eq!(lexical_canonicalize("/tmp//foo", "/"), "/tmp/foo");
    }

    #[test]
    fn lex_dotdot_above_root_stays_at_root() {
        assert_eq!(lexical_canonicalize("/..", "/"), "/");
    }

    #[test]
    fn lex_multiple_dotdots_pop_correctly() {
        assert_eq!(lexical_canonicalize("a/b/../..", "/tmp/x"), "/tmp/x");
    }

    #[test]
    fn lex_empty_operand_returns_pwd() {
        assert_eq!(lexical_canonicalize("", "/tmp"), "/tmp");
    }

    #[test]
    fn lex_root_stays_root() {
        assert_eq!(lexical_canonicalize("/", "/tmp"), "/");
    }

    #[test]
    fn lex_trailing_slash_dropped() {
        assert_eq!(lexical_canonicalize("/tmp/", "/"), "/tmp");
    }
}
