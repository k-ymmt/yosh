use crate::env::ShellEnv;
use crate::parser::ast::{ParamExpr, SpecialParam};
use super::expand_word_to_string;

/// Expand a `ParamExpr` to a String.
/// Phase 3 stub: forwards to the legacy logic (will be replaced in Task 2).
pub fn expand(env: &mut ShellEnv, param: &ParamExpr) -> String {
    let mut out = String::new();
    expand_param_inner(env, param, &mut out);
    out
}

fn expand_param_inner(env: &mut ShellEnv, param: &ParamExpr, out: &mut String) {
    match param {
        ParamExpr::Simple(name) => {
            if let Some(val) = env.vars.get(name) {
                out.push_str(val);
            }
        }
        ParamExpr::Positional(n) => {
            if *n > 0 {
                if let Some(val) = env.positional_params.get(n - 1) {
                    out.push_str(val);
                }
            }
        }
        ParamExpr::Special(sp) => expand_special(env, sp, out),
        ParamExpr::Length(name) => {
            let len = env.vars.get(name).map(|v| v.len()).unwrap_or(0);
            out.push_str(&len.to_string());
        }
        ParamExpr::Default {
            name,
            word,
            null_check,
        } => {
            let val = env.vars.get(name).map(|s| s.to_string());
            let is_unset_or_null = match &val {
                None => true,
                Some(v) if *null_check && v.is_empty() => true,
                _ => false,
            };
            if is_unset_or_null {
                if let Some(default_word) = word {
                    let expanded = expand_word_to_string(env, default_word);
                    out.push_str(&expanded);
                }
            } else if let Some(v) = val {
                out.push_str(&v);
            }
        }
        // Remaining cases — to be implemented fully in Task 2
        _ => {}
    }
}

fn expand_special(env: &ShellEnv, sp: &SpecialParam, out: &mut String) {
    match sp {
        SpecialParam::Question => out.push_str(&env.last_exit_status.to_string()),
        SpecialParam::Dollar => out.push_str(&env.shell_pid.as_raw().to_string()),
        SpecialParam::Zero => out.push_str(&env.shell_name),
        SpecialParam::Hash => out.push_str(&env.positional_params.len().to_string()),
        SpecialParam::At | SpecialParam::Star => {
            out.push_str(&env.positional_params.join(" "));
        }
        SpecialParam::Bang => {}
        SpecialParam::Dash => {}
    }
}
