//! `alias` / `unalias` regular builtins (POSIX XCU `alias`, `unalias`).

use crate::builtin::options::strip_end_of_options;
use crate::builtin::output::{report_write_error, try_write_stdout_decoded};
use crate::env::ShellEnv;
use crate::error::{RuntimeErrorKind, ShellError};

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
    let args = strip_end_of_options(args);
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
