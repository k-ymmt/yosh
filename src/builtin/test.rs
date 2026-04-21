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
        3 => {
            if args[0] == "!" {
                return Ok(!evaluate(&args[1..])?);
            }
            if args[0] == "(" && args[2] == ")" {
                return evaluate(&args[1..2]);
            }
            eval_binary(args[0], args[1], args[2])
        }
        4 => {
            if args[0] == "!" {
                return Ok(!evaluate(&args[1..])?);
            }
            if args[0] == "(" && args[3] == ")" {
                return evaluate(&args[1..3]);
            }
            Err(TestError::syntax(format!(
                "{}: unexpected operator",
                args[1]
            )))
        }
        _ => Err(TestError::syntax("too many arguments".to_string())),
    }
}

fn eval_unary(op: &str, arg: &str) -> Result<bool, TestError> {
    use std::os::unix::fs::FileTypeExt;

    match op {
        "-n" => Ok(!arg.is_empty()),
        "-z" => Ok(arg.is_empty()),

        // -e follows symlinks (bash/dash semantics): dangling links → false.
        "-e" => Ok(std::fs::metadata(arg).is_ok()),
        "-f" => Ok(std::fs::metadata(arg).map(|m| m.is_file()).unwrap_or(false)),
        "-d" => Ok(std::fs::metadata(arg).map(|m| m.is_dir()).unwrap_or(false)),
        // -h / -L do NOT follow symlinks.
        "-h" | "-L" => Ok(std::fs::symlink_metadata(arg)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)),
        "-s" => Ok(std::fs::metadata(arg).map(|m| m.len() > 0).unwrap_or(false)),
        "-p" => Ok(std::fs::metadata(arg)
            .map(|m| m.file_type().is_fifo())
            .unwrap_or(false)),
        "-S" => Ok(std::fs::metadata(arg)
            .map(|m| m.file_type().is_socket())
            .unwrap_or(false)),
        "-b" => Ok(std::fs::metadata(arg)
            .map(|m| m.file_type().is_block_device())
            .unwrap_or(false)),
        "-c" => Ok(std::fs::metadata(arg)
            .map(|m| m.file_type().is_char_device())
            .unwrap_or(false)),

        "-r" => Ok(nix::unistd::access(arg, nix::unistd::AccessFlags::R_OK).is_ok()),
        "-w" => Ok(nix::unistd::access(arg, nix::unistd::AccessFlags::W_OK).is_ok()),
        "-x" => Ok(nix::unistd::access(arg, nix::unistd::AccessFlags::X_OK).is_ok()),
        "-t" => {
            let fd: i32 = arg
                .trim()
                .parse()
                .map_err(|_| TestError::syntax(format!("{}: integer expression expected", arg)))?;
            let borrowed_fd = unsafe { std::os::unix::io::BorrowedFd::borrow_raw(fd) };
            Ok(nix::unistd::isatty(borrowed_fd).unwrap_or(false))
        }

        "-u" => Ok(std::fs::metadata(arg)
            .map(|m| {
                use std::os::unix::fs::PermissionsExt;
                m.permissions().mode() & 0o4000 != 0
            })
            .unwrap_or(false)),
        "-g" => Ok(std::fs::metadata(arg)
            .map(|m| {
                use std::os::unix::fs::PermissionsExt;
                m.permissions().mode() & 0o2000 != 0
            })
            .unwrap_or(false)),

        _ => Err(TestError::syntax(format!("{}: unknown operator", op))),
    }
}

fn eval_binary(lhs: &str, op: &str, rhs: &str) -> Result<bool, TestError> {
    match op {
        "=" => Ok(lhs == rhs),
        "!=" => Ok(lhs != rhs),
        "-eq" | "-ne" | "-lt" | "-gt" | "-le" | "-ge" => {
            let l = parse_integer(lhs)?;
            let r = parse_integer(rhs)?;
            Ok(match op {
                "-eq" => l == r,
                "-ne" => l != r,
                "-lt" => l < r,
                "-gt" => l > r,
                "-le" => l <= r,
                "-ge" => l >= r,
                _ => unreachable!(),
            })
        }
        _ => Err(TestError::syntax(format!("{}: unknown operator", op))),
    }
}

fn parse_integer(s: &str) -> Result<i64, TestError> {
    s.trim()
        .parse::<i64>()
        .map_err(|_| TestError::syntax(format!("{}: integer expression expected", s)))
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

    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn dash_e_existing_file_is_true() {
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        assert_eq!(t(&["-e", &path]), 0);
    }

    #[test]
    fn dash_e_missing_file_is_false() {
        assert_eq!(t(&["-e", "/no/such/path/__yosh_test__"]), 1);
    }

    #[test]
    fn dash_f_regular_file_is_true() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "data").unwrap();
        let path = f.path().to_str().unwrap().to_string();
        assert_eq!(t(&["-f", &path]), 0);
    }

    #[test]
    fn dash_f_directory_is_false() {
        assert_eq!(t(&["-f", "/tmp"]), 1);
    }

    #[test]
    fn dash_d_directory_is_true() {
        assert_eq!(t(&["-d", "/tmp"]), 0);
    }

    #[test]
    fn dash_d_regular_file_is_false() {
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        assert_eq!(t(&["-d", &path]), 1);
    }

    #[test]
    fn dash_h_and_l_detect_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, b"x").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let link_str = link.to_str().unwrap().to_string();
        assert_eq!(t(&["-h", &link_str]), 0);
        assert_eq!(t(&["-L", &link_str]), 0);
        let target_str = target.to_str().unwrap().to_string();
        assert_eq!(t(&["-h", &target_str]), 1);
        assert_eq!(t(&["-L", &target_str]), 1);
    }

    #[test]
    fn dash_s_nonempty_file() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "data").unwrap();
        let path = f.path().to_str().unwrap().to_string();
        assert_eq!(t(&["-s", &path]), 0);
    }

    #[test]
    fn dash_s_empty_file_is_false() {
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        assert_eq!(t(&["-s", &path]), 1);
    }

    #[test]
    fn dash_r_readable_file_is_true() {
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        assert_eq!(t(&["-r", &path]), 0);
    }

    #[test]
    fn dash_r_missing_file_is_false() {
        assert_eq!(t(&["-r", "/no/such/__yosh_test__"]), 1);
    }

    #[test]
    fn dash_w_writable_file_is_true() {
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        assert_eq!(t(&["-w", &path]), 0);
    }

    #[test]
    fn dash_x_executable_is_true_for_chmod_bit() {
        use std::os::unix::fs::PermissionsExt;
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(t(&["-x", &path]), 0);
    }

    #[test]
    fn dash_x_nonexecutable_is_false() {
        use std::os::unix::fs::PermissionsExt;
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(t(&["-x", &path]), 1);
    }

    #[test]
    fn dash_t_non_tty_fd_is_false() {
        // FD 99 is almost certainly not open, so isatty returns false.
        assert_eq!(t(&["-t", "99"]), 1);
    }

    #[test]
    fn dash_t_non_integer_errors() {
        assert_eq!(t(&["-t", "abc"]), 2);
    }

    #[test]
    fn dash_u_setuid_bit() {
        use std::os::unix::fs::PermissionsExt;
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o4755)).unwrap();
        assert_eq!(t(&["-u", &path]), 0);
        std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o0755)).unwrap();
        assert_eq!(t(&["-u", &path]), 1);
    }

    #[test]
    fn dash_g_setgid_bit() {
        use std::os::unix::fs::PermissionsExt;
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_str().unwrap().to_string();
        std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o2755)).unwrap();
        assert_eq!(t(&["-g", &path]), 0);
        std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o0755)).unwrap();
        assert_eq!(t(&["-g", &path]), 1);
    }

    #[test]
    fn binary_string_eq() {
        assert_eq!(t(&["abc", "=", "abc"]), 0);
        assert_eq!(t(&["abc", "=", "xyz"]), 1);
    }

    #[test]
    fn binary_string_neq() {
        assert_eq!(t(&["abc", "!=", "xyz"]), 0);
        assert_eq!(t(&["abc", "!=", "abc"]), 1);
    }

    #[test]
    fn binary_integer_eq() {
        assert_eq!(t(&["3", "-eq", "3"]), 0);
        assert_eq!(t(&["3", "-eq", "4"]), 1);
    }

    #[test]
    fn binary_integer_ne_lt_gt_le_ge() {
        assert_eq!(t(&["3", "-ne", "4"]), 0);
        assert_eq!(t(&["3", "-lt", "4"]), 0);
        assert_eq!(t(&["4", "-gt", "3"]), 0);
        assert_eq!(t(&["3", "-le", "3"]), 0);
        assert_eq!(t(&["4", "-ge", "4"]), 0);
    }

    #[test]
    fn binary_integer_strips_whitespace() {
        assert_eq!(t(&[" 42 ", "-eq", "42"]), 0);
    }

    #[test]
    fn binary_integer_signed() {
        assert_eq!(t(&["-3", "-lt", "0"]), 0);
        assert_eq!(t(&["+3", "-eq", "3"]), 0);
    }

    #[test]
    fn binary_integer_parse_error() {
        assert_eq!(t(&["abc", "-eq", "0"]), 2);
        assert_eq!(t(&["0", "-eq", "abc"]), 2);
    }

    #[test]
    fn negation_of_2op_form() {
        assert_eq!(t(&["!", "-z", ""]), 1); // -z "" is true, negation is false
        assert_eq!(t(&["!", "-n", ""]), 0); // -n "" is false, negation is true
    }

    #[test]
    fn paren_grouping_1op() {
        assert_eq!(t(&["(", "x", ")"]), 0);
        assert_eq!(t(&["(", "", ")"]), 1);
    }

    #[test]
    fn unknown_binary_operator_errors() {
        assert_eq!(t(&["a", "-Z", "b"]), 2);
    }

    #[test]
    fn four_operand_negation_of_binary() {
        assert_eq!(t(&["!", "a", "=", "b"]), 0); // not (a = b)
        assert_eq!(t(&["!", "a", "=", "a"]), 1);
    }

    #[test]
    fn four_operand_paren_wraps_unary() {
        assert_eq!(t(&["(", "-n", "x", ")"]), 0);
        assert_eq!(t(&["(", "-n", "", ")"]), 1);
    }

    #[test]
    fn four_operand_invalid_shape() {
        // Not starting with ! and not wrapped in ( ).
        assert_eq!(t(&["a", "b", "c", "d"]), 2);
    }

    #[test]
    fn five_or_more_operands_is_error() {
        assert_eq!(t(&["a", "b", "c", "d", "e"]), 2);
    }
}
