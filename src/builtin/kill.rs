//! `kill` regular builtin (POSIX XCU `kill`): send a signal to pids or
//! `%jobspec` process groups, or list signal names with `kill -l`.

use crate::builtin::options::strip_end_of_options;
use crate::builtin::output::write_stdout_or_report;
use crate::env::ShellEnv;
use crate::error::{RuntimeErrorKind, ShellError};

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
        return kill_list(strip_end_of_options(&args[1..]));
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
                        crate::byteenc::write_stderr_decoded_line(&format!(
                            "yosh: kill: ({}) - {}",
                            pid_str, e
                        ));
                        status = 1;
                    }
                }
                None => {
                    crate::byteenc::write_stderr_decoded_line(&format!(
                        "yosh: kill: {}: no such job",
                        pid_str
                    ));
                    status = 1;
                }
            }
            continue;
        }
        let pid: i32 = match pid_str.parse() {
            Ok(n) => n,
            Err(_) => {
                crate::byteenc::write_stderr_decoded_line(&format!(
                    "yosh: kill: {}: invalid pid",
                    pid_str
                ));
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
            crate::byteenc::write_stderr_decoded_line(&format!(
                "yosh: kill: ({}) - {}",
                pid_str, e
            ));
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
    Ok((sig, strip_end_of_options(rest)))
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
