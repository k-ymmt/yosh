//! `trap` special builtin (POSIX XCU §2.15 `trap`).

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

pub fn builtin_trap(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // POSIX XBD §12.2 Guideline 10: a leading `--` marks the end of
    // options — required for the save/restore idiom `t=$(trap); eval
    // "$t"` to work against our own `trap -- '…' NAME` output.
    let (args, saw_end_of_options) = if args.first().map(String::as_str) == Some("--") {
        (&args[1..], true)
    } else {
        (args, false)
    };
    if args.is_empty() {
        env.traps.display_all();
        return Ok(0);
    }
    if !saw_end_of_options && args[0] == "-p" {
        // POSIX 2024: with condition operands, print only the named
        // conditions; with none, print all traps. `--` may still follow.
        let conds = &args[1..];
        let conds = if conds.first().map(String::as_str) == Some("--") {
            &conds[1..]
        } else {
            conds
        };
        if conds.is_empty() {
            env.traps.display_all();
        } else {
            env.traps.display_conditions(conds);
        }
        return Ok(0);
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
        if matches!(action, TrapAction::Default) {
            if let Err(e) = env.traps.remove_trap(sig) {
                eprintln!("yosh: trap: {}", e);
                status = 1;
            } else {
                apply_trap_os_disposition(env, sig, &action);
            }
        } else if let Err(e) = env.traps.set_trap(sig, action.clone()) {
            eprintln!("yosh: trap: {}", e);
            status = 1;
        } else {
            apply_trap_os_disposition(env, sig, &action);
        }
    }
    Ok(status)
}
