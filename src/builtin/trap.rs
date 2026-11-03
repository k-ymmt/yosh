//! `trap` special builtin (POSIX XCU §2.15 `trap`).

use crate::builtin::options::strip_end_of_options;
use crate::builtin::output::write_lines_or_report;
use crate::env::{ShellEnv, TrapAction};
use crate::error::ShellError;

/// Install the OS disposition matching a trap-store change (POSIX: the
/// trap must take effect even for signals the shell has no standing
/// handler for, e.g. `trap 'cmd' ABRT`). No-ops for EXIT (0), unknown
/// conditions, and ignored-on-entry signals (§2.12 — the store already
/// no-oped those).
fn apply_trap_os_disposition(env: &ShellEnv, condition: &str, action: &TrapAction) {
    let Some(num) = crate::env::TrapStore::signal_name_to_number(condition) else {
        return;
    };
    if num == 0 || crate::signal::is_ignored_on_entry(num) {
        return;
    }
    let disposition = match action {
        TrapAction::Command(_) => crate::signal::TrapDisposition::Command,
        TrapAction::Ignore => crate::signal::TrapDisposition::Ignore,
        TrapAction::Default => crate::signal::TrapDisposition::Default,
    };
    crate::signal::apply_trap_disposition(num, disposition, env.mode.options.monitor);
}

/// Write the re-input trap listing (all traps, or just `conditions`;
/// see [`crate::env::TrapStore::listing`]) to stdout and return the
/// status (1 after a write error).
fn print_traps(env: &ShellEnv, conditions: Option<&[String]>) -> i32 {
    write_lines_or_report("trap", env.traps.listing(conditions))
}

pub fn builtin_trap(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // POSIX XBD §12.2 Guideline 10: a leading `--` marks the end of
    // options — required for the save/restore idiom `t=$(trap); eval
    // "$t"` to work against our own `trap -- '…' NAME` output.
    let saw_end_of_options = args.first().is_some_and(|a| a == "--");
    let args = strip_end_of_options(args);
    if args.is_empty() {
        return Ok(print_traps(env, None));
    }
    if !saw_end_of_options && args[0] == "-p" {
        // POSIX 2024: with condition operands, print only the named
        // conditions; with none, print all traps. `--` may still follow.
        let conds = strip_end_of_options(&args[1..]);
        return Ok(print_traps(env, (!conds.is_empty()).then_some(conds)));
    }
    // POSIX trap: "If the first operand is an unsigned decimal integer,
    // the shell shall treat all operands as conditions, and shall reset
    // each condition to the default value." A lone operand is likewise
    // a condition to reset (`trap INT`).
    let first_is_condition =
        args.len() == 1 || (!args[0].is_empty() && args[0].bytes().all(|b| b.is_ascii_digit()));
    let (action, signals) = if first_is_condition {
        (TrapAction::Default, args)
    } else {
        let action_str = &args[0];
        let action = if action_str == "-" {
            TrapAction::Default
        } else if action_str.is_empty() {
            TrapAction::Ignore
        } else {
            TrapAction::Command(action_str.to_string())
        };
        (action, &args[1..])
    };
    let mut status = 0;
    for sig in signals {
        let stored = if matches!(action, TrapAction::Default) {
            env.traps.remove_trap(sig)
        } else {
            env.traps.set_trap(sig, action.clone())
        };
        match stored {
            Ok(()) => apply_trap_os_disposition(env, sig, &action),
            Err(e) => {
                crate::byteenc::write_stderr_decoded_line(&format!("yosh: trap: {}", e));
                status = 1;
            }
        }
    }
    Ok(status)
}
