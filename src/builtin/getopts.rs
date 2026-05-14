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
}
