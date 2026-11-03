use crate::builtin::output::{
    report_write_error, try_write_stdout_decoded, write_stdout_decoded, write_stdout_or_report,
};
use crate::builtin::path::{encode_path, os_path};
use crate::env::ShellEnv;
use crate::error::{RuntimeErrorKind, ShellError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CdMode {
    Logical,
    Physical,
}

pub fn builtin_cd(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // 1. Parse options
    let (mode, operand) = parse_cd_options(args)?;

    // 2. Resolve operand -> target path
    let (target, from_cdpath) = resolve_target(operand.as_deref(), env)?;

    // 3. Capture old PWD (logical if available, else from the kernel)
    let old_pwd = env
        .vars
        .get("PWD")
        .map(|s| s.to_string())
        .or_else(|| std::env::current_dir().ok().map(|p| encode_path(&p)))
        .unwrap_or_else(|| "/".to_string());

    // 4. Compute and apply the chdir per mode
    let new_pwd = match mode {
        CdMode::Logical => {
            let candidate = lexical_canonicalize(&target, &old_pwd);
            if let Err(e) = std::env::set_current_dir(os_path(&candidate)) {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::IoError,
                    format!("cd: {}: {}", target, e),
                ));
            }
            candidate
        }
        CdMode::Physical => {
            if let Err(e) = std::env::set_current_dir(os_path(&target)) {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::IoError,
                    format!("cd: {}: {}", target, e),
                ));
            }
            match std::env::current_dir() {
                Ok(p) => encode_path(&p),
                Err(e) => {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::IoError,
                        format!("cd: {}: {}", target, e),
                    ));
                }
            }
        }
    };

    // 5. Update PWD / OLDPWD
    let _ = env.vars.set("OLDPWD", old_pwd);
    let _ = env.vars.set("PWD", new_pwd.clone());

    // 6. Print new PWD if the operand came from CDPATH or was "-"
    if from_cdpath {
        write_stdout_decoded(&new_pwd, true);
    }

    Ok(0)
}

pub fn builtin_echo(args: &[String]) -> Result<i32, ShellError> {
    Ok(if args.first().map(|a| a.as_str()) == Some("-n") {
        write_stdout_or_report("echo", &args[1..].join(" "), false)
    } else {
        write_stdout_or_report("echo", &args.join(" "), true)
    })
}

/// One alias in the re-input format shared with `command -v`
/// (`alias name='value'`, embedded `'` as `'\''`).
pub(crate) fn alias_line(name: &str, value: &str) -> String {
    format!(
        "alias {}={}",
        name,
        crate::builtin::quote::single_quote(value)
    )
}

pub fn builtin_alias(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // XBD §12.2 Guideline 10: accept `--` (bash does; alias has no options).
    let args = match args.first().map(String::as_str) {
        Some("--") => &args[1..],
        _ => args,
    };
    if args.is_empty() {
        for (name, value) in env.aliases.sorted_iter() {
            if let Err(e) = try_write_stdout_decoded(&alias_line(name, value), true) {
                return Ok(report_write_error("alias", &e));
            }
        }
        return Ok(0);
    }
    let mut status = 0;
    for arg in args {
        match arg.find('=') {
            // An operand with a non-empty name before `=` defines an alias.
            Some(pos) if pos > 0 => {
                let name = &arg[..pos];
                let value = &arg[pos + 1..];
                env.aliases.set(name, value);
            }
            // `=foo` has an empty name: like dash/bash it is looked up
            // (and not found) rather than defining a nameless alias.
            _ => match env.aliases.get(arg) {
                Some(value) => {
                    if let Err(e) = try_write_stdout_decoded(&alias_line(arg, value), true) {
                        return Ok(report_write_error("alias", &e));
                    }
                }
                None => {
                    eprintln!("yosh: alias: {}: not found", arg);
                    status = 1;
                }
            },
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
    let mut operands_only = false;
    for arg in args {
        if !operands_only && arg == "--" {
            // End of options (XBD §12.2 Guideline 10).
            operands_only = true;
        } else if !operands_only && arg == "-a" {
            env.aliases.clear();
        } else if !env.aliases.remove(arg) {
            eprintln!("yosh: unalias: {}: not found", arg);
            status = 1;
        }
    }
    Ok(status)
}

const KILL_USAGE: &str = "kill: usage: kill [-s sigspec | -signum | -sigspec] pid | %jobspec ... or kill -l [exitstatus]";

pub fn builtin_kill(args: &[String], env: &ShellEnv) -> Result<i32, ShellError> {
    let shell_pgid = env.process.shell_pgid;
    if args.is_empty() {
        return Err(ShellError::runtime(
            RuntimeErrorKind::InvalidArgument,
            KILL_USAGE,
        ));
    }

    if args[0] == "-l" {
        // `kill -l -- 9`: `--` ends options (XBD §12.2 Guideline 10).
        let rest = match args.get(1).map(String::as_str) {
            Some("--") => &args[2..],
            _ => &args[1..],
        };
        return kill_list(rest);
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
    if pid_args.is_empty() {
        // `kill -9` with no pid is a usage error, not a silent success.
        return Err(ShellError::runtime(
            RuntimeErrorKind::InvalidArgument,
            KILL_USAGE,
        ));
    }

    let mut status = 0;
    for pid_str in pid_args {
        // %jobspec operand: signal the job's process group (POSIX kill).
        if pid_str.starts_with('%') {
            let pgid = env
                .process
                .jobs
                .resolve_job_spec(pid_str)
                .ok()
                .and_then(|id| env.process.jobs.get(id))
                .map(|job| job.pgid);
            match pgid {
                Some(pgid) => {
                    if let Err(e) = send_signal(-pgid.as_raw(), sig_num) {
                        eprintln!("yosh: kill: ({}) - {}", pid_str, e);
                        status = 1;
                    }
                }
                None => {
                    eprintln!("yosh: kill: {}: no such job", pid_str);
                    status = 1;
                }
            }
            continue;
        }
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
            -gpid
        } else {
            pid
        };
        if let Err(e) = send_signal(target, sig_num) {
            eprintln!("yosh: kill: ({}) - {}", pid_str, e);
            status = 1;
        }
    }
    Ok(status)
}

/// `kill(2)` with a raw signal number (0 = existence check). Validation
/// happened in [`parse_sig_spec`]; going through `libc` directly avoids
/// nix's `Signal` enum, whose failed conversion used to degrade an
/// unknown number into a silent signal-0 probe.
fn send_signal(pid: i32, sig: i32) -> Result<(), nix::errno::Errno> {
    // SAFETY: plain syscall with integer arguments.
    if unsafe { libc::kill(pid, sig) } == 0 {
        Ok(())
    } else {
        Err(nix::errno::Errno::last())
    }
}

/// Parse a signal operand of `-s` / `-SIG`: a name (optionally
/// `SIG`-prefixed, case-insensitive), `0` (the null signal POSIX
/// requires), or the number of a signal this platform defines.
fn parse_sig_spec(spec: &str) -> Result<i32, String> {
    if !spec.is_empty() && spec.bytes().all(|b| b.is_ascii_digit()) {
        return match spec.parse::<i32>() {
            Ok(n) if n == 0 || crate::signal::signal_number_to_name(n).is_some() => Ok(n),
            _ => Err(format!("{}: invalid signal specification", spec)),
        };
    }
    crate::signal::signal_name_to_number(spec)
        .map_err(|_| format!("{}: invalid signal specification", spec))
}

/// Split `args` into the signal to send and the pid/jobspec operands.
/// An optional `--` after the signal option ends option parsing.
fn parse_kill_signal(args: &[String]) -> Result<(i32, &[String]), String> {
    let (sig, rest) = match args.first().map(String::as_str) {
        Some("-s") => {
            let Some(spec) = args.get(1) else {
                return Err("option requires an argument -- s".to_string());
            };
            (parse_sig_spec(spec)?, &args[2..])
        }
        Some("--") => return Ok((libc::SIGTERM, &args[1..])),
        Some(a) if a.starts_with('-') && a.len() > 1 => (parse_sig_spec(&a[1..])?, &args[1..]),
        _ => (libc::SIGTERM, args),
    };
    let rest = match rest.first().map(String::as_str) {
        Some("--") => &rest[1..],
        _ => rest,
    };
    Ok((sig, rest))
}

fn kill_list(args: &[String]) -> Result<i32, ShellError> {
    let mut out = String::new();
    if args.is_empty() {
        let names: Vec<&str> = crate::signal::SIGNAL_TABLE
            .iter()
            .map(|&(_, name)| name)
            .collect();
        out.push_str(&names.join(" "));
        out.push('\n');
    }
    for arg in args {
        if let Ok(num) = arg.parse::<i32>() {
            // An exit status of a signal-killed process (128+N) names N.
            let sig = if num > 128 { num - 128 } else { num };
            match crate::signal::signal_number_to_name(sig) {
                Some(name) => {
                    out.push_str(name);
                    out.push('\n');
                }
                None => {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::InvalidArgument,
                        format!("kill: {}: invalid signal number", arg),
                    ));
                }
            }
        } else {
            match crate::signal::signal_name_to_number(arg) {
                Ok(num) => {
                    out.push_str(&num.to_string());
                    out.push('\n');
                }
                Err(e) => {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::InvalidArgument,
                        format!("kill: {}", e),
                    ));
                }
            }
        }
    }
    Ok(write_stdout_or_report("kill", &out, false))
}

pub fn builtin_umask(args: &[String]) -> Result<i32, ShellError> {
    // Options: `-S` (symbolic output) and `--` (end of options). Any
    // other `-…` word is an invalid option (dash/bash), never a symbolic
    // mode — `umask -x` used to silently clear the x bits.
    let mut symbolic = false;
    let mut i = 0;
    while let Some(a) = args.get(i) {
        match a.as_str() {
            "--" => {
                i += 1;
                break;
            }
            "-S" => symbolic = true,
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidOption,
                    format!("umask: {}: invalid option", s),
                ));
            }
            _ => break,
        }
        i += 1;
    }

    // POSIX: the mask is written only when no mask operand is given;
    // `-S` merely selects the output format (dash: `umask -S 077` sets
    // the mask silently). Extra operands are ignored, as in dash/bash.
    let Some(mask) = args.get(i) else {
        let current = unsafe { libc::umask(0) };
        unsafe { libc::umask(current) };
        let line = if symbolic {
            umask_to_symbolic(current)
        } else {
            format!("{:04o}", current)
        };
        return Ok(write_stdout_or_report("umask", &line, true));
    };

    // Try octal parse first
    if mask.chars().all(|c| c.is_ascii_digit()) {
        return umask_set_octal(mask);
    }

    // Try symbolic parse
    umask_set_symbolic(mask)
}

fn umask_to_symbolic(mask: libc::mode_t) -> String {
    let perms = 0o777 & !mask;
    let fmt = |bits: libc::mode_t| -> String {
        let mut s = String::new();
        if bits & 4 != 0 {
            s.push('r');
        }
        if bits & 2 != 0 {
            s.push('w');
        }
        if bits & 1 != 0 {
            s.push('x');
        }
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

pub(crate) fn parse_cd_options(args: &[String]) -> Result<(CdMode, Option<String>), ShellError> {
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

/// Resolve a cd operand (or None for HOME) to the target path plus a
/// boolean indicating whether the result came from CDPATH (or was
/// `cd -`); when true the caller must print the new PWD.
pub(crate) fn resolve_target(
    operand: Option<&str>,
    env: &ShellEnv,
) -> Result<(String, bool), ShellError> {
    // Case 1: no operand -> HOME
    let op = match operand {
        None => {
            return match env.vars.get("HOME") {
                Some(h) if !h.is_empty() => Ok((h.to_string(), false)),
                _ => Err(ShellError::runtime(
                    RuntimeErrorKind::IoError,
                    "cd: HOME not set",
                )),
            };
        }
        Some(o) => o,
    };

    // Case 2: `cd -` -> OLDPWD, print the new PWD
    if op == "-" {
        return match env.vars.get("OLDPWD") {
            Some(p) if !p.is_empty() => Ok((p.to_string(), true)),
            _ => Err(ShellError::runtime(
                RuntimeErrorKind::IoError,
                "cd: OLDPWD not set",
            )),
        };
    }

    // Case 3: absolute path -> as-is, no CDPATH
    if op.starts_with('/') {
        return Ok((op.to_string(), false));
    }

    // Case 4: dot-prefixed -> skip CDPATH
    if op == "." || op == ".." || op.starts_with("./") || op.starts_with("../") {
        return Ok((op.to_string(), false));
    }

    // Case 5: CDPATH search
    if let Some(cdpath) = env.vars.get("CDPATH") {
        for entry in cdpath.split(':') {
            let dir = if entry.is_empty() { "." } else { entry };
            let candidate = format!("{}/{}", dir.trim_end_matches('/'), op);
            if let Ok(meta) = std::fs::metadata(os_path(&candidate))
                && meta.is_dir()
            {
                // POSIX cd: the new directory is written only when a
                // *non-empty* CDPATH entry was used; an empty entry (the
                // current directory) prints nothing, as in dash/bash.
                return Ok((candidate, !entry.is_empty()));
            }
        }
    }

    // No CDPATH match: return operand as-is (PWD prefix applied by caller)
    Ok((op.to_string(), false))
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

/// `resource` argument type of `getrlimit`/`setrlimit` (glibc uses an
/// enum-backed unsigned type, everyone else `c_int`).
#[cfg(all(target_os = "linux", target_env = "gnu"))]
type RlimResource = libc::__rlimit_resource_t;
#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
type RlimResource = libc::c_int;

/// One resource `ulimit` can query/set (POSIX.1-2024: `-c -d -f -n -s
/// -t -v`).
#[derive(Debug, PartialEq)]
struct UlimitResource {
    opt: char,
    resource: RlimResource,
    /// Bytes (or other native units) per reported unit.
    unit: libc::rlim_t,
    /// `ulimit -a` description and unit label (bash layout).
    desc: &'static str,
    unit_name: &'static str,
}

/// Reporting order of `ulimit -a` (alphabetical by option, as bash).
const ULIMIT_RESOURCES: &[UlimitResource] = &[
    UlimitResource {
        opt: 'c',
        resource: libc::RLIMIT_CORE,
        unit: 512,
        desc: "core file size",
        unit_name: "blocks",
    },
    UlimitResource {
        opt: 'd',
        resource: libc::RLIMIT_DATA,
        unit: 1024,
        desc: "data seg size",
        unit_name: "kbytes",
    },
    UlimitResource {
        opt: 'f',
        resource: libc::RLIMIT_FSIZE,
        unit: 512,
        desc: "file size",
        unit_name: "blocks",
    },
    UlimitResource {
        opt: 'n',
        resource: libc::RLIMIT_NOFILE,
        unit: 1,
        desc: "open files",
        unit_name: "",
    },
    UlimitResource {
        opt: 's',
        resource: libc::RLIMIT_STACK,
        unit: 1024,
        desc: "stack size",
        unit_name: "kbytes",
    },
    UlimitResource {
        opt: 't',
        resource: libc::RLIMIT_CPU,
        unit: 1,
        desc: "cpu time",
        unit_name: "seconds",
    },
    UlimitResource {
        opt: 'v',
        resource: libc::RLIMIT_AS,
        unit: 1024,
        desc: "virtual memory",
        unit_name: "kbytes",
    },
];

/// The new limit given as the `ulimit` operand.
#[derive(Debug, PartialEq)]
enum UlimitValue {
    /// A count of the resource's reporting units.
    Units(libc::rlim_t),
    Unlimited,
}

/// Parsed `ulimit` invocation.
#[derive(Debug, PartialEq)]
struct UlimitRequest {
    hard: bool,
    soft: bool,
    /// Selected resources, in option order; empty means `-f`.
    resources: Vec<char>,
    all: bool,
    value: Option<UlimitValue>,
}

/// Argument-parse failures for `ulimit`. All map to exit status 1.
#[derive(Debug, PartialEq)]
enum UlimitArgError {
    UnknownOption(String),
    InvalidNumber(String),
    TooManyArgs,
}

/// Parse `ulimit [-H|-S] [-a | -c|-d|-f|-n|-s|-t|-v ...] [limit]`.
/// Pure: no syscalls. Options may be clustered (`-Hn`); `--` ends them;
/// a word after the options that starts with `-` is an operand (so
/// `-f -5` is an invalid number, not an option). The resource count is
/// a count of units (`unlimited` for no limit).
fn parse_ulimit(args: &[String]) -> Result<UlimitRequest, UlimitArgError> {
    let mut req = UlimitRequest {
        hard: false,
        soft: false,
        resources: Vec::new(),
        all: false,
        value: None,
    };
    let mut i = 0;
    while let Some(a) = args.get(i) {
        if a == "--" {
            i += 1;
            break;
        }
        let Some(cluster) = a.strip_prefix('-').filter(|c| !c.is_empty()) else {
            break;
        };
        for c in cluster.chars() {
            match c {
                'H' => req.hard = true,
                'S' => req.soft = true,
                'a' => req.all = true,
                c if ULIMIT_RESOURCES.iter().any(|r| r.opt == c) => req.resources.push(c),
                _ => return Err(UlimitArgError::UnknownOption(a.clone())),
            }
        }
        i += 1;
    }
    match &args[i..] {
        [] => {}
        [op] => {
            req.value = Some(if op == "unlimited" {
                UlimitValue::Unlimited
            } else {
                op.parse::<libc::rlim_t>()
                    .map(UlimitValue::Units)
                    .map_err(|_| UlimitArgError::InvalidNumber(op.clone()))?
            });
        }
        _ => return Err(UlimitArgError::TooManyArgs),
    }
    Ok(req)
}

/// Render a limit in the resource's units, or `"unlimited"`.
fn format_limit(rlim: libc::rlim_t, unit: libc::rlim_t) -> String {
    if rlim == libc::RLIM_INFINITY {
        "unlimited".to_string()
    } else {
        (rlim / unit).to_string()
    }
}

/// One `ulimit -a` line in bash's layout: description, then the
/// `(unit, -X)` tag right-aligned to column 36, then the value.
fn ulimit_all_line(r: &UlimitResource, value: &str) -> String {
    let tag = if r.unit_name.is_empty() {
        format!("(-{})", r.opt)
    } else {
        format!("({}, -{})", r.unit_name, r.opt)
    };
    let pad = 36usize.saturating_sub(r.desc.len() + tag.len()).max(1);
    format!("{}{}{} {}", r.desc, " ".repeat(pad), tag, value)
}

fn get_rlimit(resource: RlimResource) -> std::io::Result<libc::rlimit> {
    let mut rl = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `rl` is a valid, aligned rlimit; getrlimit only writes it.
    if unsafe { libc::getrlimit(resource, &mut rl) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(rl)
}

/// `ulimit` — query or set resource limits (POSIX.1-2024 `-H -S -a -c
/// -d -f -n -s -t -v`). Without `-H`/`-S`, a query reports the soft
/// limit and a set changes both limits (bash, dash, ksh).
pub fn builtin_ulimit(args: &[String]) -> Result<i32, ShellError> {
    let req = match parse_ulimit(args) {
        Ok(r) => r,
        Err(UlimitArgError::UnknownOption(o)) => {
            eprintln!("yosh: ulimit: {o}: invalid option");
            return Ok(1);
        }
        Err(UlimitArgError::InvalidNumber(n)) => {
            eprintln!("yosh: ulimit: {n}: invalid number");
            return Ok(1);
        }
        Err(UlimitArgError::TooManyArgs) => {
            eprintln!("yosh: ulimit: too many arguments");
            return Ok(1);
        }
    };

    let selected: Vec<&UlimitResource> = if req.all {
        ULIMIT_RESOURCES.iter().collect()
    } else if req.resources.is_empty() {
        ULIMIT_RESOURCES.iter().filter(|r| r.opt == 'f').collect()
    } else {
        req.resources
            .iter()
            .filter_map(|&c| ULIMIT_RESOURCES.iter().find(|r| r.opt == c))
            .collect()
    };

    let Some(value) = req.value else {
        // Query. More than one resource (or -a) uses the labelled layout.
        let labelled = req.all || selected.len() > 1;
        let mut out = String::new();
        for r in &selected {
            let rl = match get_rlimit(r.resource) {
                Ok(rl) => rl,
                Err(e) => {
                    eprintln!("yosh: ulimit: {e}");
                    return Ok(1);
                }
            };
            // -H alone reports the hard limit; otherwise the soft one.
            let lim = if req.hard && !req.soft {
                rl.rlim_max
            } else {
                rl.rlim_cur
            };
            let v = format_limit(lim, r.unit);
            if labelled {
                out.push_str(&ulimit_all_line(r, &v));
            } else {
                out.push_str(&v);
            }
            out.push('\n');
        }
        return Ok(write_stdout_or_report("ulimit", &out, false));
    };

    if req.all {
        eprintln!("yosh: ulimit: -a: cannot be combined with a limit");
        return Ok(1);
    }
    // Neither -H nor -S: set both (as bash/dash/ksh do).
    let (set_hard, set_soft) = if req.hard || req.soft {
        (req.hard, req.soft)
    } else {
        (true, true)
    };
    for r in selected {
        let new = match value {
            UlimitValue::Unlimited => libc::RLIM_INFINITY,
            UlimitValue::Units(n) => match n.checked_mul(r.unit) {
                Some(v) if v != libc::RLIM_INFINITY => v,
                _ => {
                    eprintln!("yosh: ulimit: {n}: limit out of range");
                    return Ok(1);
                }
            },
        };
        let mut rl = match get_rlimit(r.resource) {
            Ok(rl) => rl,
            Err(e) => {
                eprintln!("yosh: ulimit: {e}");
                return Ok(1);
            }
        };
        if set_hard {
            rl.rlim_max = new;
        }
        if set_soft {
            rl.rlim_cur = new;
        }
        // SAFETY: `rl` is a valid rlimit; setrlimit only reads it.
        if unsafe { libc::setrlimit(r.resource, &rl) } != 0 {
            eprintln!(
                "yosh: ulimit: {}: cannot modify limit: {}",
                r.desc,
                std::io::Error::last_os_error()
            );
            return Ok(1);
        }
    }
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

    // ── resolve_target ───────────────────────────────────────────

    use crate::env::ShellEnv;

    fn make_env(pairs: &[(&str, &str)]) -> ShellEnv {
        let mut env = ShellEnv::new("yosh", vec![]);
        for name in &["HOME", "OLDPWD", "PWD", "CDPATH"] {
            let _ = env.vars.unset(name);
        }
        for (k, v) in pairs {
            let _ = env.vars.set(k, (*v).to_string());
        }
        env
    }

    #[test]
    fn resolve_none_uses_home() {
        let env = make_env(&[("HOME", "/home/x")]);
        let (target, from_cdpath) = resolve_target(None, &env).unwrap();
        assert_eq!(target, "/home/x");
        assert!(!from_cdpath);
    }

    #[test]
    fn resolve_none_home_unset_errors() {
        let env = make_env(&[]);
        let err = resolve_target(None, &env).unwrap_err();
        assert!(err.to_string().contains("HOME not set"));
    }

    #[test]
    fn resolve_dash_uses_oldpwd_and_sets_from_cdpath() {
        let env = make_env(&[("OLDPWD", "/prev")]);
        let (target, from_cdpath) = resolve_target(Some("-"), &env).unwrap();
        assert_eq!(target, "/prev");
        assert!(from_cdpath, "cd - must print the new PWD");
    }

    #[test]
    fn resolve_dash_oldpwd_unset_errors() {
        let env = make_env(&[]);
        let err = resolve_target(Some("-"), &env).unwrap_err();
        assert!(err.to_string().contains("OLDPWD not set"));
    }

    #[test]
    fn resolve_absolute_passes_through() {
        let env = make_env(&[("CDPATH", "/etc")]);
        let (target, from_cdpath) = resolve_target(Some("/tmp"), &env).unwrap();
        assert_eq!(target, "/tmp");
        assert!(!from_cdpath, "absolute paths skip CDPATH");
    }

    #[test]
    fn resolve_dot_prefix_skips_cdpath() {
        let env = make_env(&[("CDPATH", "/etc")]);
        let (target, from_cdpath) = resolve_target(Some("./foo"), &env).unwrap();
        assert_eq!(target, "./foo");
        assert!(!from_cdpath);
    }

    #[test]
    fn resolve_dotdot_prefix_skips_cdpath() {
        let env = make_env(&[("CDPATH", "/etc")]);
        let (target, from_cdpath) = resolve_target(Some("../foo"), &env).unwrap();
        assert_eq!(target, "../foo");
        assert!(!from_cdpath);
    }

    #[test]
    fn resolve_cdpath_hit() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let cdpath = tmp.path().to_string_lossy().to_string();

        let env = make_env(&[("CDPATH", cdpath.as_str())]);
        let (target, from_cdpath) = resolve_target(Some("sub"), &env).unwrap();
        assert_eq!(target, sub.to_string_lossy());
        assert!(from_cdpath);
    }

    #[test]
    fn resolve_cdpath_miss_falls_through() {
        let tmp = tempfile::tempdir().unwrap();
        let cdpath = tmp.path().to_string_lossy().to_string();
        let env = make_env(&[("CDPATH", cdpath.as_str())]);
        let (target, from_cdpath) = resolve_target(Some("nonexistent_xyz"), &env).unwrap();
        assert_eq!(target, "nonexistent_xyz");
        assert!(!from_cdpath);
    }

    #[test]
    fn resolve_cdpath_empty_entry_is_dot() {
        // Create the probe dir inside the existing cwd rather than
        // `set_current_dir`-ing into a tempdir. Mutating the process-wide
        // cwd races with any parallel test that spawns a subprocess
        // (e.g. `plugin::host::commands` invoking `/bin/sh`): the child
        // inherits the dangling cwd once the tempdir drops, and sh stalls
        // trying to recover.
        //
        // This test still *depends* on a stable cwd (the probe dir is
        // resolved via `./<name>`), so hold the cwd lock against tests
        // that legitimately chdir (see `crate::test_sync`).
        let _cwd = crate::test_sync::lock_cwd();
        let tmp = tempfile::tempdir_in(".").unwrap();
        let name = tmp
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();

        let env = make_env(&[("CDPATH", ":/nonexistent")]);
        let (target, print_pwd) = resolve_target(Some(&name), &env).unwrap();
        assert_eq!(target, format!("./{}", name));
        // A match through an empty (current-directory) entry is silent.
        assert!(!print_pwd);
    }

    #[test]
    fn resolve_cdpath_skips_non_directory_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("regular_file");
        std::fs::write(&file_path, "x").unwrap();

        let cdpath = tmp.path().to_string_lossy().to_string();
        let env = make_env(&[("CDPATH", cdpath.as_str())]);
        let (target, from_cdpath) = resolve_target(Some("regular_file"), &env).unwrap();
        assert_eq!(target, "regular_file");
        assert!(!from_cdpath);
    }

    // ── parse_ulimit ─────────────────────────────────────────────────

    fn req(
        hard: bool,
        soft: bool,
        res: &str,
        all: bool,
        value: Option<UlimitValue>,
    ) -> UlimitRequest {
        UlimitRequest {
            hard,
            soft,
            resources: res.chars().collect(),
            all,
            value,
        }
    }

    #[test]
    fn test_parse_ulimit_show() {
        assert_eq!(
            parse_ulimit(&s(&[])),
            Ok(req(false, false, "", false, None))
        );
        assert_eq!(
            parse_ulimit(&s(&["-f"])),
            Ok(req(false, false, "f", false, None))
        );
    }

    #[test]
    fn test_parse_ulimit_set_blocks() {
        assert_eq!(
            parse_ulimit(&s(&["-f", "100"])),
            Ok(req(false, false, "f", false, Some(UlimitValue::Units(100))))
        );
        assert_eq!(
            parse_ulimit(&s(&["100"])),
            Ok(req(false, false, "", false, Some(UlimitValue::Units(100))))
        );
    }

    #[test]
    fn test_parse_ulimit_unlimited() {
        assert_eq!(
            parse_ulimit(&s(&["-f", "unlimited"])),
            Ok(req(false, false, "f", false, Some(UlimitValue::Unlimited)))
        );
    }

    #[test]
    fn test_parse_ulimit_posix_2024_options() {
        assert_eq!(
            parse_ulimit(&s(&["-a"])),
            Ok(req(false, false, "", true, None))
        );
        assert_eq!(
            parse_ulimit(&s(&["-Hn"])),
            Ok(req(true, false, "n", false, None))
        );
        assert_eq!(
            parse_ulimit(&s(&["-S", "-c", "--", "0"])),
            Ok(req(false, true, "c", false, Some(UlimitValue::Units(0))))
        );
        assert_eq!(
            parse_ulimit(&s(&["-n", "-s", "-t", "-v", "-d"])),
            Ok(req(false, false, "nstvd", false, None))
        );
    }

    #[test]
    fn test_parse_ulimit_unknown_option() {
        assert_eq!(
            parse_ulimit(&s(&["-Z"])),
            Err(UlimitArgError::UnknownOption("-Z".to_string()))
        );
        assert_eq!(
            parse_ulimit(&s(&["-nZ"])),
            Err(UlimitArgError::UnknownOption("-nZ".to_string()))
        );
    }

    #[test]
    fn test_parse_ulimit_invalid_number() {
        assert_eq!(
            parse_ulimit(&s(&["-f", "abc"])),
            Err(UlimitArgError::InvalidNumber("abc".to_string()))
        );
        assert_eq!(
            parse_ulimit(&s(&["-f", "--", "-5"])),
            Err(UlimitArgError::InvalidNumber("-5".to_string()))
        );
    }

    #[test]
    fn test_parse_ulimit_too_many_args() {
        assert_eq!(
            parse_ulimit(&s(&["-f", "1", "2"])),
            Err(UlimitArgError::TooManyArgs)
        );
    }

    // ── formatting ───────────────────────────────────────────────────

    #[test]
    fn test_format_limit() {
        assert_eq!(format_limit(libc::RLIM_INFINITY, 512), "unlimited");
        assert_eq!(format_limit(51200, 512), "100"); // 51200 / 512
        assert_eq!(format_limit(8192 * 1024, 1024), "8192");
        assert_eq!(format_limit(0, 1), "0");
    }

    #[test]
    fn test_ulimit_all_line_matches_bash_layout() {
        let find = |c| ULIMIT_RESOURCES.iter().find(|r| r.opt == c).unwrap();
        assert_eq!(
            ulimit_all_line(find('c'), "0"),
            "core file size          (blocks, -c) 0"
        );
        assert_eq!(
            ulimit_all_line(find('n'), "256"),
            "open files                      (-n) 256"
        );
        assert_eq!(
            ulimit_all_line(find('t'), "unlimited"),
            "cpu time               (seconds, -t) unlimited"
        );
    }

    // ── builtin_ulimit ───────────────────────────────────────────────

    #[test]
    fn test_ulimit_show_returns_ok() {
        // Read-only paths: succeed without altering any limit.
        assert_eq!(builtin_ulimit(&s(&["-f"])), Ok(0));
        assert_eq!(builtin_ulimit(&s(&["-a"])), Ok(0));
        assert_eq!(builtin_ulimit(&s(&["-Hn"])), Ok(0));
        assert_eq!(
            builtin_ulimit(&s(&["-S", "-c", "-s", "-t", "-v", "-d"])),
            Ok(0)
        );
    }

    #[test]
    fn test_ulimit_unknown_option_returns_one() {
        assert_eq!(builtin_ulimit(&s(&["-Z"])), Ok(1));
    }

    #[test]
    fn test_ulimit_invalid_number_returns_one() {
        assert_eq!(builtin_ulimit(&s(&["-f", "abc"])), Ok(1));
    }

    #[test]
    fn test_ulimit_set_to_current_hard_is_safe() {
        // Setting the limit to the current HARD value is safe inside the shared
        // test process: soft only ever rises toward hard (never causes SIGXFSZ),
        // and it never fails with EPERM. In normal environments RLIMIT_FSIZE is
        // RLIM_INFINITY, so this is a true no-op; with a finite, non-512-aligned
        // hard limit it could round the hard limit down by <512 bytes, which is
        // harmless. This exercises the setrlimit success path.
        let rl = get_rlimit(libc::RLIMIT_FSIZE).unwrap();
        let operand = if rl.rlim_max == libc::RLIM_INFINITY {
            "unlimited".to_string()
        } else {
            (rl.rlim_max / 512).to_string()
        };
        assert_eq!(builtin_ulimit(&s(&["-f", &operand])), Ok(0));
    }
}
