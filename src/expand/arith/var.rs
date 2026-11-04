//! Shell variables as arithmetic operands and assignment targets.

use super::{MAX_RECURSION_DEPTH, arith_err, eval_text};
use crate::env::ShellEnv;
use crate::error::{ExpansionErrorKind, ShellError};

/// Look up a bare identifier (`name` in `$((name))`, without a leading
/// `$`) for the tokenizer's variable-reference path. Intercepts `LINENO`
/// as a computed pseudo-variable (see `ExecState::lineno`); unset regular
/// variables default to "0".
fn arith_name_lookup(env: &ShellEnv, name: &str) -> String {
    if name == "LINENO" {
        return env.exec.lineno.to_string();
    }
    env.vars.get(name).unwrap_or("0").to_string()
}

/// The value of variable `name` as an operand at variable recursion
/// depth `depth` and nesting `nest`; 0 without evaluation in a skipped
/// operand (`noeval`). A plain decimal value (the common case) is read in
/// place, without copying the value string.
pub(super) fn var_operand(
    env: &mut ShellEnv,
    name: &str,
    depth: u32,
    nest: u32,
    noeval: bool,
) -> crate::error::Result<i64> {
    if noeval {
        return Ok(0);
    }
    if name != "LINENO" {
        let Some(raw) = env.vars.get(name) else {
            return Ok(0);
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(0);
        }
        if is_plain_decimal(trimmed)
            && let Ok(v) = trimmed.parse::<i64>()
        {
            return Ok(v);
        }
    }
    let raw = arith_name_lookup(env, name);
    eval_var_value(env, name, &raw, depth, nest)
}

/// Evaluate a variable's raw string value as an operand. A plain
/// integer parses directly; empty/blank is 0 (matches bash: `x=""`
/// gives `$((x))` == 0); anything else is recursively evaluated as an
/// arithmetic expression (`x=1+2; $((x))` == 3, matching bash/dash),
/// with a depth cap so self-referential values (`x=x`) terminate.
fn eval_var_value(
    env: &mut ShellEnv,
    name: &str,
    raw: &str,
    depth: u32,
    nest: u32,
) -> crate::error::Result<i64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    // Fast path: a plain decimal integer. Values with a leading zero
    // (octal `010`, hex `0x1f`, invalid `09`) take the full path so
    // they are read as integer constants with the C rules a literal
    // gets (`x=010; $((x))` is 8, `x=09` is an error; dash/bash).
    if is_plain_decimal(trimmed)
        && let Ok(v) = trimmed.parse::<i64>()
    {
        return Ok(v);
    }
    if depth >= MAX_RECURSION_DEPTH || crate::exec::stack_guard::descent_stack_exhausted() {
        return Err(arith_err(format!(
            "{}: expression recursion level exceeded",
            name
        )));
    }
    // Never from a cached tree: values recurse through each other
    // (`v0='1 && v1'`, …), and stacking tree walks per level would use
    // more native stack than the one-pass evaluator, so only the
    // outermost text — at most `cache::MAX_CACHED_NEST` levels — is
    // walked from a tree.
    eval_text(env, trimmed, depth + 1, nest, false)
}

/// Store an arithmetic assignment's result. LINENO is a computed
/// pseudo-variable (see `ExecState::lineno`): an assignment reads back
/// the value but does not persist (matches bash: `$((LINENO+=1))`
/// does not "stick"). `assign_var` (not `vars.set`) invalidates the
/// utility hash when PATH is assigned. A readonly target is an
/// expansion error (dash: `x: is read only`; bash: `x: readonly
/// variable`), never silently ignored.
pub(super) fn assign(env: &mut ShellEnv, name: &str, val: i64) -> crate::error::Result<()> {
    if name == "LINENO" {
        return Ok(());
    }
    env.assign_var(name, val.to_string())
        .map_err(|msg| ShellError::expansion(ExpansionErrorKind::InvalidArithmetic, msg))
}

/// True for an optionally `-`-signed run of ASCII digits with no
/// redundant leading zero (`0`, `42`, `-7`): the strings whose `i64`
/// parse agrees with C integer-constant rules. Anything else (`010`,
/// `0x1f`, `09`, `+5`, `1+2`) is evaluated as an expression.
pub(super) fn is_plain_decimal(s: &str) -> bool {
    let digits = s.strip_prefix('-').unwrap_or(s).as_bytes();
    match digits {
        [] => false,
        [b'0'] => true,
        [first, ..] => *first != b'0' && digits.iter().all(u8::is_ascii_digit),
    }
}
