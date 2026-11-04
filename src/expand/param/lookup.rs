//! Resolving a parameter name to its value: variables, the computed
//! `LINENO`, positional and special parameters, and the predicates the
//! `${name<op>word}` forms test the result with.

use std::borrow::Cow;

use super::parameter_not_set;
use crate::env::ShellEnv;
use crate::parser::ast::{ParamExpr, SpecialParam};

/// Resolve a variable by name, intercepting `LINENO` as a computed
/// pseudo-variable backed by `env.exec.lineno` rather than a real
/// `VarStore` entry (see `ExecState::lineno` doc comment for rationale).
/// Digit-string names are positional parameters (POSIX §2.5.1 — leading
/// zeros are decimal, so `01` ≡ `1`); an index past `$#` is unset (`None`).
/// All other names delegate to `VarStore::get`.
///
/// The value is borrowed from `env` where it is stored there, so a
/// caller that only reads it does not copy it.
pub(in crate::expand) fn lookup_var<'a>(env: &'a ShellEnv, name: &str) -> Option<Cow<'a, str>> {
    if name == "LINENO" {
        return Some(Cow::Owned(env.exec.lineno.to_string()));
    }
    // Special parameters reach here via the modifier forms (`${?:-x}`,
    // `${!+y}`, …); bare `$?` et al. dispatch through `ParamExpr::Special`
    // instead. `$!` is unset until an asynchronous list has run, and
    // `$*`/`$@` are unset without positional parameters (POSIX §2.5.2);
    // the rest are always set.
    if let Some(sp) = special_param(name) {
        return match sp {
            SpecialParam::Bang if env.process.jobs.last_bg_pid().is_none() => None,
            SpecialParam::At | SpecialParam::Star if env.vars.positional_params().is_empty() => {
                None
            }
            // `$*` joins with IFS[0] (POSIX §2.5.2); in unquoted contexts
            // field splitting on IFS then recovers the separate fields.
            SpecialParam::Star => Some(Cow::Owned(
                env.vars
                    .positional_params()
                    .join(&crate::expand::pipeline::ifs_join_separator(env)),
            )),
            _ => Some(Cow::Owned(expand_special(env, &sp))),
        };
    }
    if !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()) {
        let n: usize = name.parse().unwrap_or(0);
        if n == 0 {
            return Some(Cow::Borrowed(&env.shell_name));
        }
        return env
            .vars
            .positional_params()
            .get(n - 1)
            .map(|s| Cow::Borrowed(s.as_str()));
    }
    env.vars.get(name).map(Cow::Borrowed)
}

/// The value of a `$name` / `${name}` / `$1` expansion when it is set,
/// borrowed where possible: exactly what [`expand`] returns for it, and
/// without side effects. `None` for any other expression, or an unset
/// parameter (which [`expand`] handles, `set -u` included).
pub(in crate::expand) fn set_scalar_value<'a>(
    env: &'a ShellEnv,
    param: &ParamExpr,
) -> Option<Cow<'a, str>> {
    match param {
        ParamExpr::Simple(name) => lookup_var(env, name),
        ParamExpr::Positional(n) if *n > 0 => env
            .vars
            .positional_params()
            .get(n - 1)
            .map(|s| Cow::Borrowed(s.as_str())),
        _ => None,
    }
}

/// `lookup_var` for the expansion forms that `set -u` covers beyond a
/// bare `$name` — `${#name}` and the four strip forms (POSIX §2.6.2 /
/// `set -u`: expanding an unset parameter other than `@`/`*` is an
/// error; dash/bash/ksh agree). An unset `name` raises the expansion
/// error and yields `Err(())` so the caller skips the rest of the
/// expansion (no pattern side effects). `@`/`*` stay exempt, as for
/// bare `$@`.
pub(super) fn lookup_var_nounset<'a>(
    env: &'a mut ShellEnv,
    name: &str,
) -> Result<Option<Cow<'a, str>>, ()> {
    if env.mode.options.nounset && !matches!(name, "@" | "*") && lookup_var(env, name).is_none() {
        parameter_not_set(env, name);
        return Err(());
    }
    Ok(lookup_var(env, name))
}

/// True when `name` denotes a positional or special parameter, which
/// `${name=word}` may not assign to (POSIX §2.6.2).
pub(in crate::expand) fn is_unassignable_param(name: &str) -> bool {
    !name.is_empty()
        && !name
            .bytes()
            .next()
            .is_some_and(|b| b == b'_' || b.is_ascii_alphabetic())
}

pub(in crate::expand) fn is_unset_or_null_inner(val: Option<&str>, null_check: bool) -> bool {
    match val {
        None => true,
        Some(v) if null_check && v.is_empty() => true,
        _ => false,
    }
}

/// Map a single-character special-parameter name (as produced by the
/// lexer's `read_param_name`) to its `SpecialParam`. `0` is not included:
/// the digit-string branch of `lookup_var` already resolves it.
fn special_param(name: &str) -> Option<SpecialParam> {
    match name.as_bytes() {
        [b] if *b != b'0' => SpecialParam::from_byte(*b),
        _ => None,
    }
}

pub(super) fn expand_special(env: &ShellEnv, sp: &SpecialParam) -> String {
    match sp {
        SpecialParam::Question => env.exec.last_exit_status.to_string(),
        SpecialParam::Dollar => env.process.shell_pid.as_raw().to_string(),
        SpecialParam::Zero => env.shell_name.clone(),
        SpecialParam::Hash => env.vars.positional_params().len().to_string(),
        SpecialParam::At | SpecialParam::Star => env.vars.positional_params().join(" "),
        SpecialParam::Bang => env
            .process
            .jobs
            .last_bg_pid()
            .map(|p| p.as_raw().to_string())
            .unwrap_or_default(),
        SpecialParam::Dash => env.mode.flag_string(),
    }
}
