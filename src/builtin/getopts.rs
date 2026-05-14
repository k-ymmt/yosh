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
    Ok(ParsedArgs { optstring, var_name, operands })
}

pub fn builtin_getopts(_args: &[String], _env: &mut ShellEnv) -> Result<i32, ShellError> {
    // Filled in by Task 6.
    Ok(0)
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
        let next_optind = if rest_of_elt { optind_in } else { optind_in + 1 };
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
            optind: if rest_of_elt { optind_in } else { optind_in + 1 },
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
}
