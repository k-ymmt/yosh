//! POSIX `getopts` builtin.
//!
//! `getopts optstring var [arg ...]` — parse one option from the
//! positional parameters (or explicit `arg`s) on each call, advancing
//! `OPTIND` and setting `OPTARG`. Stacked options (`-abc`) are returned
//! one per call. A `:` prefix on `optstring` enables silent error mode.

use crate::env::ShellEnv;
use crate::error::ShellError;
use crate::parser::word::is_valid_name;

#[derive(Debug, PartialEq)]
enum ArgError {
    MissingOperands,
    InvalidVarName(String),
}

#[derive(Debug, PartialEq)]
struct ParsedArgs<'a> {
    optstring: &'a str,
    var_name: &'a str,
    operands: Vec<&'a str>,
}

fn parse_args<'a>(args: &'a [String]) -> Result<ParsedArgs<'a>, ArgError> {
    if args.len() < 2 {
        return Err(ArgError::MissingOperands);
    }
    let optstring = args[0].as_str();
    let var_name = args[1].as_str();
    if !is_valid_name(var_name) {
        return Err(ArgError::InvalidVarName(var_name.to_string()));
    }
    let operands: Vec<&str> = args[2..].iter().map(String::as_str).collect();
    Ok(ParsedArgs {
        optstring,
        var_name,
        operands,
    })
}

pub fn builtin_getopts(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    let parsed = match parse_args(args) {
        Ok(p) => p,
        Err(ArgError::MissingOperands) => {
            eprintln!("yosh: getopts: usage: getopts optstring name [arg ...]");
            return Ok(2);
        }
        Err(ArgError::InvalidVarName(name)) => {
            eprintln!("yosh: getopts: `{}': not a valid identifier", name);
            return Ok(2);
        }
    };

    let silent = parsed.optstring.starts_with(':');
    let spec = if silent {
        &parsed.optstring[1..]
    } else {
        parsed.optstring
    };

    // Resolve operands: explicit args[2..] if non-empty, else positional params.
    let positional_owned: Vec<String>;
    let operands_refs: Vec<&str> = if parsed.operands.is_empty() {
        positional_owned = env.vars.positional_params().to_vec();
        positional_owned.iter().map(String::as_str).collect()
    } else {
        parsed.operands.clone()
    };

    let optind_in = env
        .vars
        .get("OPTIND")
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(1);
    let subindex_in = env.vars.getopts_subindex();

    let step = step_getopts(spec, &operands_refs, optind_in, subindex_in, silent);

    // Pre-check all readonly targets so a failure aborts BEFORE any
    // assign_var / subindex write — otherwise a readonly `opt` would
    // silently advance OPTIND, leaving callers with inconsistent state.
    // bash returns rc=1 with `<name>: readonly variable` on this path.
    for target in [parsed.var_name, "OPTARG", "OPTIND"] {
        if env.vars.is_readonly(target) {
            eprintln!("yosh: getopts: {}: readonly variable", target);
            return Ok(1);
        }
    }

    env.assign_var(parsed.var_name, step.var_value)
        .expect("var_name readonly was pre-checked");
    let optarg_value = step.optarg.unwrap_or_default();
    env.assign_var("OPTARG", optarg_value)
        .expect("OPTARG readonly was pre-checked");
    env.assign_var("OPTIND", step.optind.to_string())
        .expect("OPTIND readonly was pre-checked");
    env.vars.set_getopts_subindex(step.subindex);

    if let Some(msg) = step.stderr {
        eprintln!("yosh: getopts: {}", msg);
    }

    Ok(step.exit)
}

#[derive(Debug, PartialEq)]
struct GetoptsStep {
    var_value: String,
    optarg: Option<String>,
    optind: usize,
    subindex: usize,
    exit: i32,
    stderr: Option<String>,
}

fn end_of_options(optind: usize) -> GetoptsStep {
    GetoptsStep {
        var_value: "?".to_string(),
        optarg: None,
        optind,
        subindex: 0,
        exit: 1,
        stderr: None,
    }
}

fn step_getopts(
    spec: &str,
    operands: &[&str],
    optind_in: usize,
    subindex_in: usize,
    silent: bool,
) -> GetoptsStep {
    if optind_in == 0 || optind_in > operands.len() {
        return end_of_options(optind_in.max(1));
    }

    let elt = operands[optind_in - 1];

    let cursor = if subindex_in == 0 {
        if elt == "--" {
            return GetoptsStep {
                var_value: "?".to_string(),
                optarg: None,
                optind: optind_in + 1,
                subindex: 0,
                exit: 1,
                stderr: None,
            };
        }
        if !elt.starts_with('-') || elt == "-" {
            return end_of_options(optind_in);
        }
        1
    } else {
        subindex_in
    };

    let bytes = elt.as_bytes();
    let ch = bytes[cursor] as char;
    let next_cursor = cursor + 1;
    let rest_of_elt = next_cursor < bytes.len();

    let pos = spec.bytes().position(|b| b == ch as u8);

    // Unknown option
    if pos.is_none() {
        let next_optind = if rest_of_elt {
            optind_in
        } else {
            optind_in + 1
        };
        let next_sub = if rest_of_elt { next_cursor } else { 0 };
        if silent {
            return GetoptsStep {
                var_value: "?".to_string(),
                optarg: Some(ch.to_string()),
                optind: next_optind,
                subindex: next_sub,
                exit: 0,
                stderr: None,
            };
        }
        return GetoptsStep {
            var_value: "?".to_string(),
            optarg: None,
            optind: next_optind,
            subindex: next_sub,
            exit: 0,
            stderr: Some(format!("-{}: illegal option", ch)),
        };
    }

    let pos = pos.unwrap();
    let takes_arg = matches!(spec.as_bytes().get(pos + 1), Some(b':'));

    // Known, no-arg option
    if !takes_arg {
        return GetoptsStep {
            var_value: ch.to_string(),
            optarg: None,
            optind: if rest_of_elt {
                optind_in
            } else {
                optind_in + 1
            },
            subindex: if rest_of_elt { next_cursor } else { 0 },
            exit: 0,
            stderr: None,
        };
    }

    // Known, takes argument — argument inside same element
    if rest_of_elt {
        let arg = &elt[next_cursor..];
        return GetoptsStep {
            var_value: ch.to_string(),
            optarg: Some(arg.to_string()),
            optind: optind_in + 1,
            subindex: 0,
            exit: 0,
            stderr: None,
        };
    }

    // Argument in next element
    if optind_in + 1 > operands.len() {
        // Missing
        if silent {
            return GetoptsStep {
                var_value: ":".to_string(),
                optarg: Some(ch.to_string()),
                optind: optind_in + 1,
                subindex: 0,
                exit: 0,
                stderr: None,
            };
        }
        return GetoptsStep {
            var_value: "?".to_string(),
            optarg: None,
            optind: optind_in + 1,
            subindex: 0,
            exit: 0,
            stderr: Some(format!("option requires an argument -- {}", ch)),
        };
    }

    let arg = operands[optind_in];
    GetoptsStep {
        var_value: ch.to_string(),
        optarg: Some(arg.to_string()),
        optind: optind_in + 2,
        subindex: 0,
        exit: 0,
        stderr: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parse_args_minimum_two_operands() {
        assert_eq!(parse_args(&s(&[])), Err(ArgError::MissingOperands));
        assert_eq!(parse_args(&s(&["a"])), Err(ArgError::MissingOperands));
    }

    #[test]
    fn parse_args_invalid_var_name_rejected() {
        assert_eq!(
            parse_args(&s(&["a", "1foo"])),
            Err(ArgError::InvalidVarName("1foo".into()))
        );
    }

    #[test]
    fn parse_args_no_operands_means_empty_vec() {
        let args = s(&["a:", "opt"]);
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.optstring, "a:");
        assert_eq!(parsed.var_name, "opt");
        assert!(parsed.operands.is_empty());
    }

    #[test]
    fn parse_args_explicit_operands_captured() {
        let args = s(&["a:", "opt", "-a", "value"]);
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.operands, vec!["-a", "value"]);
    }

    #[test]
    fn step_single_option() {
        let step = step_getopts("a", &["-a"], 1, 0, false);
        assert_eq!(step.var_value, "a");
        assert_eq!(step.optarg, None);
        assert_eq!(step.optind, 2);
        assert_eq!(step.subindex, 0);
        assert_eq!(step.exit, 0);
        assert!(step.stderr.is_none());
    }

    #[test]
    fn step_end_of_options_when_index_past_operands() {
        let step = step_getopts("a", &["-a"], 2, 0, false);
        assert_eq!(step.var_value, "?");
        assert_eq!(step.optind, 2);
        assert_eq!(step.subindex, 0);
        assert_eq!(step.exit, 1);
    }

    #[test]
    fn step_end_of_options_on_non_dash_operand() {
        let step = step_getopts("a", &["arg"], 1, 0, false);
        assert_eq!(step.var_value, "?");
        assert_eq!(step.optind, 1);
        assert_eq!(step.exit, 1);
    }

    #[test]
    fn step_end_of_options_on_lone_dash() {
        let step = step_getopts("a", &["-"], 1, 0, false);
        assert_eq!(step.var_value, "?");
        assert_eq!(step.optind, 1);
        assert_eq!(step.exit, 1);
    }

    #[test]
    fn step_option_with_arg_same_element() {
        let step = step_getopts("a:", &["-aval"], 1, 0, false);
        assert_eq!(step.var_value, "a");
        assert_eq!(step.optarg, Some("val".into()));
        assert_eq!(step.optind, 2);
        assert_eq!(step.subindex, 0);
        assert_eq!(step.exit, 0);
    }

    #[test]
    fn step_option_with_arg_next_element() {
        let step = step_getopts("a:", &["-a", "val"], 1, 0, false);
        assert_eq!(step.var_value, "a");
        assert_eq!(step.optarg, Some("val".into()));
        assert_eq!(step.optind, 3);
        assert_eq!(step.subindex, 0);
        assert_eq!(step.exit, 0);
    }

    #[test]
    fn step_stacked_first() {
        let step = step_getopts("ab", &["-ab"], 1, 0, false);
        assert_eq!(step.var_value, "a");
        assert_eq!(step.optind, 1);
        assert_eq!(step.subindex, 2);
        assert_eq!(step.exit, 0);
    }

    #[test]
    fn step_stacked_second() {
        let step = step_getopts("ab", &["-ab"], 1, 2, false);
        assert_eq!(step.var_value, "b");
        assert_eq!(step.optind, 2);
        assert_eq!(step.subindex, 0);
        assert_eq!(step.exit, 0);
    }

    #[test]
    fn step_unknown_option_normal_mode() {
        let step = step_getopts("a", &["-x"], 1, 0, false);
        assert_eq!(step.var_value, "?");
        assert_eq!(step.optarg, None);
        assert_eq!(step.optind, 2);
        assert_eq!(step.subindex, 0);
        assert_eq!(step.exit, 0);
        assert!(step.stderr.is_some());
        let msg = step.stderr.unwrap();
        assert!(msg.contains("-x"), "stderr msg = {msg}");
        assert!(msg.contains("illegal option"), "stderr msg = {msg}");
    }

    #[test]
    fn step_unknown_option_silent_mode() {
        let step = step_getopts("a", &["-x"], 1, 0, true);
        assert_eq!(step.var_value, "?");
        assert_eq!(step.optarg, Some("x".into()));
        assert_eq!(step.optind, 2);
        assert_eq!(step.exit, 0);
        assert!(step.stderr.is_none());
    }

    #[test]
    fn step_missing_arg_normal_mode() {
        let step = step_getopts("a:", &["-a"], 1, 0, false);
        assert_eq!(step.var_value, "?");
        assert_eq!(step.optarg, None);
        assert_eq!(step.optind, 2);
        assert_eq!(step.subindex, 0);
        assert_eq!(step.exit, 0);
        assert!(step.stderr.is_some());
        let msg = step.stderr.unwrap();
        assert!(msg.contains("requires an argument"), "stderr msg = {msg}");
        assert!(msg.contains("a"), "stderr msg = {msg}");
    }

    #[test]
    fn step_missing_arg_silent_mode() {
        let step = step_getopts("a:", &["-a"], 1, 0, true);
        assert_eq!(step.var_value, ":");
        assert_eq!(step.optarg, Some("a".into()));
        assert_eq!(step.optind, 2);
        assert_eq!(step.exit, 0);
        assert!(step.stderr.is_none());
    }

    #[test]
    fn step_double_dash_advances_optind() {
        let step = step_getopts("a", &["--"], 1, 0, false);
        assert_eq!(step.var_value, "?");
        assert_eq!(step.optind, 2);
        assert_eq!(step.exit, 1);
    }

    use crate::env::ShellEnv;

    fn make_env() -> ShellEnv {
        ShellEnv::new("yosh", vec![])
    }

    #[test]
    fn builtin_dispatches_simple_option_from_positional() {
        let mut env = make_env();
        env.vars.set_positional_params(vec!["-a".into()]);
        let rc = super::builtin_getopts(&s(&["a", "opt"]), &mut env).unwrap();
        assert_eq!(rc, 0);
        assert_eq!(env.vars.get("opt"), Some("a"));
        assert_eq!(env.vars.get("OPTIND"), Some("2"));
    }

    #[test]
    fn builtin_sets_optarg_for_takes_arg() {
        let mut env = make_env();
        env.vars
            .set_positional_params(vec!["-a".into(), "value".into()]);
        let rc = super::builtin_getopts(&s(&["a:", "opt"]), &mut env).unwrap();
        assert_eq!(rc, 0);
        assert_eq!(env.vars.get("opt"), Some("a"));
        assert_eq!(env.vars.get("OPTARG"), Some("value"));
        assert_eq!(env.vars.get("OPTIND"), Some("3"));
    }

    #[test]
    fn builtin_explicit_operands_override_positional() {
        let mut env = make_env();
        env.vars.set_positional_params(vec!["-x".into()]);
        let rc = super::builtin_getopts(&s(&["a", "opt", "-a"]), &mut env).unwrap();
        assert_eq!(rc, 0);
        assert_eq!(env.vars.get("opt"), Some("a"));
    }

    #[test]
    fn builtin_stacked_two_calls() {
        let mut env = make_env();
        env.vars.set_positional_params(vec!["-ab".into()]);
        let args = s(&["ab", "opt"]);

        let rc1 = super::builtin_getopts(&args, &mut env).unwrap();
        assert_eq!(rc1, 0);
        assert_eq!(env.vars.get("opt"), Some("a"));
        assert_eq!(env.vars.get("OPTIND"), Some("1"));

        let rc2 = super::builtin_getopts(&args, &mut env).unwrap();
        assert_eq!(rc2, 0);
        assert_eq!(env.vars.get("opt"), Some("b"));
        assert_eq!(env.vars.get("OPTIND"), Some("2"));
    }

    #[test]
    fn builtin_end_of_options_returns_one() {
        let mut env = make_env();
        env.vars.set_positional_params(vec!["arg".into()]);
        let rc = super::builtin_getopts(&s(&["a", "opt"]), &mut env).unwrap();
        assert_eq!(rc, 1);
        assert_eq!(env.vars.get("opt"), Some("?"));
    }

    #[test]
    fn builtin_missing_operands_returns_two() {
        let mut env = make_env();
        let rc = super::builtin_getopts(&s(&[]), &mut env).unwrap();
        assert_eq!(rc, 2);
    }

    #[test]
    fn builtin_invalid_var_name_returns_two() {
        let mut env = make_env();
        let rc = super::builtin_getopts(&s(&["a", "1foo"]), &mut env).unwrap();
        assert_eq!(rc, 2);
    }

    #[test]
    fn builtin_readonly_var_name_emits_diagnostic_and_rc_one() {
        let mut env = make_env();
        env.vars.set_positional_params(vec!["-a".into()]);
        env.vars.set("opt", "initial").unwrap();
        env.vars.set_readonly("opt");
        let rc = super::builtin_getopts(&s(&["a", "opt"]), &mut env).unwrap();
        assert_eq!(rc, 1);
        // var_name preserves its prior value, no partial write.
        assert_eq!(env.vars.get("opt"), Some("initial"));
    }

    #[test]
    fn builtin_readonly_var_name_does_not_advance_optind() {
        let mut env = make_env();
        env.vars.set_positional_params(vec!["-a".into()]);
        // ShellEnv::new seeds OPTIND="1"; after a readonly-var_name
        // abort the value must stay "1" so a script can fix `opt` and
        // re-call getopts from the same logical position.
        assert_eq!(env.vars.get("OPTIND"), Some("1"));
        env.vars.set_readonly("opt");
        let rc = super::builtin_getopts(&s(&["a", "opt"]), &mut env).unwrap();
        assert_eq!(rc, 1);
        assert_eq!(env.vars.get("OPTIND"), Some("1"));
    }

    #[test]
    fn builtin_readonly_optind_emits_diagnostic_and_rc_one() {
        let mut env = make_env();
        env.vars.set_positional_params(vec!["-a".into()]);
        env.vars.set("OPTIND", "1").unwrap();
        env.vars.set_readonly("OPTIND");
        let rc = super::builtin_getopts(&s(&["a", "opt"]), &mut env).unwrap();
        assert_eq!(rc, 1);
        // opt must not be written when OPTIND would fail downstream.
        assert_eq!(env.vars.get("opt"), None);
        assert_eq!(env.vars.get("OPTIND"), Some("1"));
    }

    #[test]
    fn builtin_readonly_optarg_emits_diagnostic_and_rc_one() {
        let mut env = make_env();
        env.vars
            .set_positional_params(vec!["-a".into(), "value".into()]);
        env.vars.set("OPTARG", "prev").unwrap();
        env.vars.set_readonly("OPTARG");
        let rc = super::builtin_getopts(&s(&["a:", "opt"]), &mut env).unwrap();
        assert_eq!(rc, 1);
        assert_eq!(env.vars.get("opt"), None);
        assert_eq!(env.vars.get("OPTARG"), Some("prev"));
    }
}
