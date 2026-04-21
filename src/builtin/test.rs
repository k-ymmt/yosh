//! POSIX `test` and `[` builtin implementation (§2.14).
//!
//! Evaluation dispatches by operand count. Operators outside POSIX
//! (e.g. `<`, `>`, `-a`, `-o`, deep `(` `)` nesting) are deliberately
//! not supported — see the design doc for rationale.

/// Error returned by `evaluate`. Always produces exit status 2 plus a
/// message prefixed by `yosh: {name}: ` in the caller.
///
/// POSIX §2.14 specifies exit status 2 for every syntax / operator
/// error in `test`, so no `exit_code` field is needed — the caller
/// always returns 2.
struct TestError {
    message: String,
}

impl TestError {
    fn syntax(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

/// Implements POSIX `test` and `[` builtins. Returns exit status directly;
/// `test` failures are normal exit statuses, not flow-control errors.
pub fn builtin_test(name: &str, args: &[String]) -> i32 {
    // `[` requires a closing `]` as the last argument.
    let operand_slice: &[String] = if name == "[" {
        match args.last() {
            Some(s) if s == "]" => &args[..args.len() - 1],
            _ => {
                eprintln!("yosh: [: missing ']'");
                return 2;
            }
        }
    } else {
        args
    };

    let operands: Vec<&str> = operand_slice.iter().map(|s| s.as_str()).collect();
    match evaluate(&operands) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(e) => {
            eprintln!("yosh: {}: {}", name, e.message);
            2
        }
    }
}

fn evaluate(args: &[&str]) -> Result<bool, TestError> {
    match args.len() {
        0 => Ok(false),
        1 => Ok(!args[0].is_empty()),
        2 => {
            if args[0] == "!" {
                return Ok(!evaluate(&args[1..])?);
            }
            eval_unary(args[0], args[1])
        }
        _ => Err(TestError::syntax(format!(
            "unsupported operand count: {}",
            args.len()
        ))),
    }
}

fn eval_unary(op: &str, arg: &str) -> Result<bool, TestError> {
    match op {
        "-n" => Ok(!arg.is_empty()),
        "-z" => Ok(arg.is_empty()),
        _ => Err(TestError::syntax(format!("{}: unknown operator", op))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(args: &[&str]) -> i32 {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        builtin_test("test", &owned)
    }

    fn b(args: &[&str]) -> i32 {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        builtin_test("[", &owned)
    }

    #[test]
    fn zero_operands_is_false() {
        assert_eq!(t(&[]), 1);
    }

    #[test]
    fn one_empty_operand_is_false() {
        assert_eq!(t(&[""]), 1);
    }

    #[test]
    fn one_nonempty_operand_is_true() {
        assert_eq!(t(&["x"]), 0);
        assert_eq!(t(&["false"]), 0); // string "false" is nonempty → true
    }

    #[test]
    fn bracket_requires_closing() {
        // No closing `]` — exit 2 regardless of which operands precede.
        assert_eq!(b(&["x"]), 2);
    }

    #[test]
    fn bracket_with_closing_matches_test() {
        assert_eq!(b(&["x", "]"]), 0); // 1-operand nonempty → true
        assert_eq!(b(&["", "]"]), 1); // 1-operand empty → false
    }

    #[test]
    fn negation_of_empty_is_true() {
        assert_eq!(t(&["!", ""]), 0);
    }

    #[test]
    fn negation_of_nonempty_is_false() {
        assert_eq!(t(&["!", "x"]), 1);
    }

    #[test]
    fn dash_n_nonempty_is_true() {
        assert_eq!(t(&["-n", "x"]), 0);
    }

    #[test]
    fn dash_n_empty_is_false() {
        assert_eq!(t(&["-n", ""]), 1);
    }

    #[test]
    fn dash_z_empty_is_true() {
        assert_eq!(t(&["-z", ""]), 0);
    }

    #[test]
    fn dash_z_nonempty_is_false() {
        assert_eq!(t(&["-z", "x"]), 1);
    }

    #[test]
    fn unknown_unary_operator_errors() {
        // An unknown unary operator produces exit 2.
        assert_eq!(t(&["-Z", "x"]), 2);
    }
}
