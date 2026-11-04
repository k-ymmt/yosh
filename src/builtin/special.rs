//! Dispatch of the POSIX special builtins (see
//! [`super::classify_builtin`]) and classification of their failures as
//! special-built-in utility errors (POSIX §2.8.1). Each utility is
//! implemented in its own module.

use super::{eval, exec, flow, set, times, trap, vars};
use crate::error::{RuntimeErrorKind, ShellError};
use crate::exec::Executor;

/// Result of running a special built-in.
pub struct SpecialOutcome {
    pub status: i32,
    /// True when the built-in failed with a "special built-in utility
    /// error" (POSIX §2.8.1), which makes a non-interactive shell exit
    /// when the built-in was invoked directly (not via `command`).
    pub utility_error: bool,
}

/// Run a special built-in and return its status (diagnostics printed).
/// Used where the special-built-in error consequences do not apply
/// (`command name ...`).
pub fn exec_special_builtin(name: &str, args: &[String], executor: &mut Executor) -> i32 {
    run_special_builtin(name, args, executor).status
}

/// Run a special built-in, classifying a failure as a utility error.
///
/// An `Err` from the implementation is always a utility error. For the
/// built-ins whose nonzero status can only mean failure (they print the
/// diagnostic and return `Ok(n)` so the remaining operands are still
/// processed), a nonzero status counts too. The others (`eval`, `.`,
/// `fc`, `exit`, `return`, `break`, `continue`, `exec`) pass through the
/// status of what they ran, so a nonzero status is not by itself an error.
/// `trap` is left out on purpose: dash and bash both keep running after
/// `trap : BADSIG`.
pub fn run_special_builtin(name: &str, args: &[String], executor: &mut Executor) -> SpecialOutcome {
    crate::builtin::output::take_write_error_reported();
    let result = match name {
        ":" => Ok(0),
        "exit" => flow::builtin_exit(args, executor),
        "export" => vars::builtin_export(args, &mut executor.env),
        "unset" => vars::builtin_unset(args, &mut executor.env),
        "readonly" => vars::builtin_readonly(args, &mut executor.env),
        "return" => flow::builtin_return(args, executor),
        "break" => flow::builtin_break(args, &mut executor.env),
        "continue" => flow::builtin_continue(args, &mut executor.env),
        "set" => set::builtin_set(args, executor),
        "eval" => eval::builtin_eval(args, executor),
        "exec" => exec::builtin_exec(args, executor),
        "trap" => trap::builtin_trap(args, &mut executor.env),
        "." => eval::builtin_source(args, executor),
        "shift" => set::builtin_shift(args, &mut executor.env),
        "times" => times::builtin_times(args),
        _ => Err(ShellError::runtime(
            RuntimeErrorKind::InvalidArgument,
            format!("{}: not a special builtin", name),
        )),
    };
    // A failed write of a listing (`export -p >&-`) is reported with
    // status 1 but does not end the shell (dash agrees).
    let write_failed = crate::builtin::output::take_write_error_reported();
    match result {
        Ok(status) => SpecialOutcome {
            status,
            utility_error: status != 0
                && !write_failed
                && (matches!(name, "export" | "readonly" | "unset" | "set" | "times")
                    // A bad trap condition is not fatal (dash, bash); a
                    // bad option is.
                    || (name == "trap" && status == 2)),
        },
        Err(e) => {
            crate::byteenc::write_stderr_decoded_line(&e.to_string());
            SpecialOutcome {
                status: e.exit_code(),
                // `fc` is dispatched here but is a regular built-in in
                // POSIX, so its errors never terminate the shell. `return`
                // outside a function/dot script is tolerated by dash
                // (silently) and bash (diagnostic only); keep going too.
                utility_error: !matches!(name, "fc" | "return"),
            }
        }
    }
}
