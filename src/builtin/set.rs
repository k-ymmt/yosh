//! `set` and `shift` special builtins: shell options and the
//! positional parameters (POSIX XCU §2.15 `set`, `shift`).

use crate::builtin::output::write_stdout_decoded;
use crate::builtin::quote::quote_for_reinput;
use crate::env::ShellEnv;
use crate::error::{RuntimeErrorKind, ShellError};
use crate::exec::Executor;

/// `set` with its post-success monitor-mode bookkeeping
/// ([`set_after_success`]).
pub fn builtin_set(args: &[String], executor: &mut Executor) -> Result<i32, ShellError> {
    let was_monitor = executor.env.mode.options.monitor;
    let ret = apply_set(args, &mut executor.env)?;
    set_after_success(args, executor, was_monitor);
    Ok(ret)
}

/// Post-success bookkeeping of the `set` special built-in (monitor-mode
/// signal setup and the `$-` `m` snapshot).
fn set_after_success(args: &[String], executor: &mut Executor, was_monitor: bool) {
    let is_monitor = executor.env.mode.options.monitor;
    if was_monitor && !is_monitor {
        crate::signal::reset_job_control_signals();
    } else if !was_monitor && is_monitor {
        // Runtime `set -m` shares the invocation `-m`
        // terminal-ownership gate: without the controlling
        // terminal, monitor stays off (and `m` stays out of
        // `$-`), matching `yosh -m ...` and bash. `set`
        // itself still succeeds.
        if !crate::signal::try_enable_monitor_mode() {
            executor.env.mode.options.monitor = false;
        }
    }
    // An explicit `set -m`/`set +m` overrides the subshell
    // `flag_m` `$-` snapshot: a child that turned monitor off
    // internally still shows `m` in `$-` (bash-like) until the
    // user toggles it themselves.
    if set_args_mention_monitor(args) {
        executor.env.mode.flag_m = executor.env.mode.options.monitor;
    }
}

/// Whether a `set` invocation explicitly toggles the monitor option
/// (`-m`/`+m`, in a char cluster like `-em`, or `-o monitor`/`+o
/// monitor`). Scans the same leading-option region `builtin_set`
/// consumes: stops at `--`, `-`, or the first non-option argument.
/// Used by the `set` dispatch wrapper to re-sync the `flag_m` `$-`
/// snapshot on an explicit user toggle.
fn set_args_mention_monitor(args: &[String]) -> bool {
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--" || a == "-" || a == "+" {
            break;
        }
        let Some(rest) = a.strip_prefix('-').or_else(|| a.strip_prefix('+')) else {
            break;
        };
        // Each `o` in the cluster consumes the next word as an option
        // name (`-o monitor`, `-eo monitor`), mirroring apply_set.
        let mut consumed = 0;
        for c in rest.chars() {
            if c == 'o' {
                consumed += 1;
                if args.get(i + consumed).is_some_and(|n| n == "monitor") {
                    return true;
                }
            } else if c == 'm' {
                return true;
            }
        }
        i += 1 + consumed;
    }
    false
}

fn apply_set(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    if args.is_empty() {
        // Display all variables sorted
        // Names with attributes but no value (`export y`) are unset and
        // not listed (dash/bash agree).
        let mut vars: Vec<(String, String)> = env
            .vars
            .vars_iter()
            .filter_map(|(k, v)| v.value().map(|val| (k.to_string(), val.to_string())))
            .collect();
        vars.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in vars {
            write_stdout_decoded(&format!("{}={}", name, quote_for_reinput(&value)), true);
        }
        return Ok(0);
    }

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            env.vars.set_positional_params(args[i + 1..].to_vec());
            return Ok(0);
        }
        if arg == "-" {
            env.mode.options.xtrace = false;
            env.mode.options.verbose = false;
            if i + 1 < args.len() {
                env.vars.set_positional_params(args[i + 1..].to_vec());
            }
            return Ok(0);
        }
        if arg.starts_with('-') || arg.starts_with('+') {
            let on = arg.starts_with('-');
            for c in arg[1..].chars() {
                if c == 'o' {
                    // `-o name`, also inside a cluster (`set -eo noglob`,
                    // as in dash/bash): each `o` takes the next word.
                    i += 1;
                    let Some(name) = args.get(i) else {
                        if on {
                            env.mode.options.display_all();
                        } else {
                            env.mode.options.display_restorable();
                        }
                        return Ok(0);
                    };
                    if let Err(e) = env.mode.options.set_by_name(name, on) {
                        return Err(ShellError::runtime(RuntimeErrorKind::InvalidOption, e));
                    }
                } else if let Err(e) = env.mode.options.set_by_char(c, on) {
                    return Err(ShellError::runtime(RuntimeErrorKind::InvalidOption, e));
                }
            }
            i += 1;
            continue;
        }
        // Remaining args are positional params
        env.vars.set_positional_params(args[i..].to_vec());
        return Ok(0);
    }
    Ok(0)
}

pub fn builtin_shift(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    let n = if args.is_empty() {
        1usize
    } else {
        match args[0].parse::<usize>() {
            Ok(n) => n,
            Err(_) => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidArgument,
                    format!("shift: {}: numeric argument required", args[0]),
                ));
            }
        }
    };
    if !env.vars.shift_positional_params(n) {
        // POSIX shift: a count greater than `$#` "may be considered a
        // syntax error and a non-interactive shell may exit"; bash keeps
        // going with status 1 (dash exits). Report it as a plain failure
        // status, not a special-built-in utility error.
        eprintln!("yosh: shift: shift count out of range");
        return Ok(1);
    }
    Ok(0)
}
