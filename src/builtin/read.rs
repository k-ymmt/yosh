//! POSIX `read` builtin.
//!
//! `read [-r] var [var ...]` — read one logical line from stdin and
//! assign IFS-split fields to the named variables. With no `-r`,
//! backslash is the escape character (line continuation on `\<newline>`,
//! `\X` keeps `X` literally).

use crate::env::ShellEnv;
use crate::error::ShellError;
use crate::parser::word::is_valid_name;

pub fn builtin_read(_args: &[String], _env: &mut ShellEnv) -> Result<i32, ShellError> {
    eprintln!("yosh: read: not implemented");
    Ok(1)
}

#[derive(Debug, PartialEq)]
struct ParsedArgs {
    raw: bool,
    var_names: Vec<String>,
}

#[derive(Debug, PartialEq)]
enum ArgError {
    NoVarName,
    UnknownFlag(char),
    InvalidIdentifier(String),
}

fn parse_args(args: &[String]) -> Result<ParsedArgs, ArgError> {
    let mut raw = false;
    let mut idx = 0;
    while idx < args.len() {
        let a = &args[idx];
        if a == "--" {
            idx += 1;
            break;
        }
        if !a.starts_with('-') || a == "-" {
            break;
        }
        for ch in a[1..].chars() {
            match ch {
                'r' => raw = true,
                other => return Err(ArgError::UnknownFlag(other)),
            }
        }
        idx += 1;
    }

    let var_names: Vec<String> = args[idx..].to_vec();
    if var_names.is_empty() {
        return Err(ArgError::NoVarName);
    }
    for name in &var_names {
        if !is_valid_name(name) {
            return Err(ArgError::InvalidIdentifier(name.clone()));
        }
    }
    Ok(ParsedArgs { raw, var_names })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parse_args_no_args_is_error() {
        assert_eq!(parse_args(&[]), Err(ArgError::NoVarName));
    }

    #[test]
    fn parse_args_single_var() {
        assert_eq!(
            parse_args(&s(&["line"])),
            Ok(ParsedArgs { raw: false, var_names: vec!["line".into()] })
        );
    }

    #[test]
    fn parse_args_dash_r_sets_raw() {
        assert_eq!(
            parse_args(&s(&["-r", "line"])),
            Ok(ParsedArgs { raw: true, var_names: vec!["line".into()] })
        );
    }

    #[test]
    fn parse_args_double_dash_terminates_options() {
        // After `--`, even `-r` is treated as a (invalid) variable name.
        assert_eq!(
            parse_args(&s(&["--", "line"])),
            Ok(ParsedArgs { raw: false, var_names: vec!["line".into()] })
        );
    }

    #[test]
    fn parse_args_double_dash_then_dash_r_treats_as_invalid_ident() {
        // `--` terminates options; subsequent `-r` is a name and fails validation.
        assert_eq!(
            parse_args(&s(&["--", "-r"])),
            Err(ArgError::InvalidIdentifier("-r".into()))
        );
    }

    #[test]
    fn parse_args_unknown_flag_errors() {
        assert_eq!(parse_args(&s(&["-x", "line"])), Err(ArgError::UnknownFlag('x')));
    }

    #[test]
    fn parse_args_invalid_identifier_errors() {
        assert_eq!(
            parse_args(&s(&["1foo"])),
            Err(ArgError::InvalidIdentifier("1foo".into()))
        );
    }

    #[test]
    fn parse_args_multiple_vars() {
        assert_eq!(
            parse_args(&s(&["-r", "x", "y", "z"])),
            Ok(ParsedArgs {
                raw: true,
                var_names: vec!["x".into(), "y".into(), "z".into()],
            })
        );
    }
}
