//! POSIX `test` and `[` builtin implementation (XCU `test`).
//!
//! Evaluation dispatches by operand count. The POSIX.1-2024 binary
//! primaries `-nt`, `-ot`, `-ef`, `<` and `>` are supported; the
//! obsolescent `-a`/`-o` and deep `(` `)` nesting are deliberately not
//! — see the design doc for rationale.

use crate::builtin::path::os_path;

/// Error returned by `evaluate`. Always produces exit status 2 plus a
/// message prefixed by `yosh: {name}: ` in the caller.
///
/// The POSIX `test` utility spec specifies exit status 2 for every syntax / operator
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
            crate::byteenc::write_stderr_decoded_line(&format!("yosh: {}: {}", name, e.message));
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
            // POSIX `test`, 3-argument rules in order: a binary primary
            // in $2 wins over `!` negation and `( )` grouping, so
            // `[ ! = x ]` compares the strings `!` and `x`.
            if is_binary_primary(args[1]) {
                return eval_binary(args[0], args[1], args[2]);
            }
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

    // File operands are byteenc-encoded shell strings; decode so
    // non-UTF-8 path names reach the OS as their original raw bytes.
    let path = os_path(arg);
    let path = path.as_path();

    match op {
        "-n" => Ok(!arg.is_empty()),
        "-z" => Ok(arg.is_empty()),

        // -e follows symlinks (bash/dash semantics): dangling links → false.
        "-e" => Ok(std::fs::metadata(path).is_ok()),
        "-f" => Ok(std::fs::metadata(path)
            .map(|m| m.is_file())
            .unwrap_or(false)),
        "-d" => Ok(std::fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)),
        // -h / -L do NOT follow symlinks.
        "-h" | "-L" => Ok(std::fs::symlink_metadata(path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)),
        "-s" => Ok(std::fs::metadata(path)
            .map(|m| m.len() > 0)
            .unwrap_or(false)),
        "-p" => Ok(std::fs::metadata(path)
            .map(|m| m.file_type().is_fifo())
            .unwrap_or(false)),
        "-S" => Ok(std::fs::metadata(path)
            .map(|m| m.file_type().is_socket())
            .unwrap_or(false)),
        "-b" => Ok(std::fs::metadata(path)
            .map(|m| m.file_type().is_block_device())
            .unwrap_or(false)),
        "-c" => Ok(std::fs::metadata(path)
            .map(|m| m.file_type().is_char_device())
            .unwrap_or(false)),

        "-r" => Ok(nix::unistd::access(path, nix::unistd::AccessFlags::R_OK).is_ok()),
        "-w" => Ok(nix::unistd::access(path, nix::unistd::AccessFlags::W_OK).is_ok()),
        "-x" => Ok(nix::unistd::access(path, nix::unistd::AccessFlags::X_OK).is_ok()),
        "-t" => {
            let fd: i32 = arg
                .trim()
                .parse()
                .map_err(|_| TestError::syntax(format!("{}: integer expression expected", arg)))?;
            // A negative fd can't be open; BorrowedFd::borrow_raw requires a
            // valid fd, so short-circuit here rather than construct UB.
            if fd < 0 {
                return Ok(false);
            }
            let borrowed_fd = unsafe { std::os::unix::io::BorrowedFd::borrow_raw(fd) };
            Ok(nix::unistd::isatty(borrowed_fd).unwrap_or(false))
        }

        "-u" => Ok(std::fs::metadata(path)
            .map(|m| {
                use std::os::unix::fs::PermissionsExt;
                m.permissions().mode() & 0o4000 != 0
            })
            .unwrap_or(false)),
        "-g" => Ok(std::fs::metadata(path)
            .map(|m| {
                use std::os::unix::fs::PermissionsExt;
                m.permissions().mode() & 0o2000 != 0
            })
            .unwrap_or(false)),

        _ => Err(TestError::syntax(format!("{}: unknown operator", op))),
    }
}

/// True when `op` is one of the POSIX `test` binary primaries.
fn is_binary_primary(op: &str) -> bool {
    matches!(
        op,
        "=" | "!="
            | "<"
            | ">"
            | "-eq"
            | "-ne"
            | "-lt"
            | "-gt"
            | "-le"
            | "-ge"
            | "-nt"
            | "-ot"
            | "-ef"
    )
}

/// Last data-modification time `(sec, nsec)` of the file `arg`
/// resolves to (symlinks followed), or `None` if it cannot be resolved.
fn mtime_of(arg: &str) -> Option<(i64, i64)> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(os_path(arg)).ok()?;
    Some((m.mtime(), m.mtime_nsec()))
}

fn eval_binary(lhs: &str, op: &str, rhs: &str) -> Result<bool, TestError> {
    match op {
        // String comparison uses bytewise ordering (LC_COLLATE=C
        // semantics). Non-C locale values are currently treated as C
        // per yosh's POSIX-compliance doc (XBD §7.2 implementation-
        // defined).
        "=" => Ok(lhs == rhs),
        "!=" => Ok(lhs != rhs),
        // POSIX.1-2024 `<` / `>`: collation order. Compare the decoded
        // raw bytes (C-locale order) — the byteenc escape codepoints do
        // not sort like the bytes they stand for.
        "<" | ">" => {
            let l = crate::byteenc::decode_bytes(lhs);
            let r = crate::byteenc::decode_bytes(rhs);
            Ok(if op == "<" { l < r } else { l > r })
        }
        // POSIX.1-2024: `a -nt b` is true if a resolves to an existing
        // file and b does not, or both exist and a's mtime is newer
        // (nanosecond resolution); `-ot` is the mirror image.
        "-nt" => Ok(match (mtime_of(lhs), mtime_of(rhs)) {
            (Some(l), Some(r)) => l > r,
            (Some(_), None) => true,
            _ => false,
        }),
        "-ot" => Ok(match (mtime_of(lhs), mtime_of(rhs)) {
            (Some(l), Some(r)) => l < r,
            (None, Some(_)) => true,
            _ => false,
        }),
        // Both resolve to existing files that are the same file.
        "-ef" => {
            use std::os::unix::fs::MetadataExt;
            Ok(
                match (
                    std::fs::metadata(os_path(lhs)),
                    std::fs::metadata(os_path(rhs)),
                ) {
                    (Ok(l), Ok(r)) => l.dev() == r.dev() && l.ino() == r.ino(),
                    _ => false,
                },
            )
        }
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

    /// The byteenc encoding is injective, so `=`/`!=` on encoded operands
    /// is exactly raw-byte equality — invalid UTF-8 bytes compare
    /// byte-faithfully with no decode step needed. (The ordering
    /// primaries `<`/`>` decode first; see `string_order_is_raw_bytes`.)
    #[test]
    fn string_equality_is_byte_exact_for_invalid_utf8() {
        let e9 = crate::byteenc::encode_bytes(b"a\xe9b").into_owned();
        let e9_again = crate::byteenc::encode_bytes(b"a\xe9b").into_owned();
        let ff = crate::byteenc::encode_bytes(b"a\xffb").into_owned();
        assert_eq!(t(&[&e9, "=", &e9_again]), 0);
        assert_eq!(t(&[&e9, "!=", &ff]), 0);
        assert_eq!(t(&[&e9, "=", &ff]), 1);
        // A raw 0xe9 is distinct from both U+00E9 (é) and U+FFFD.
        assert_eq!(t(&[&crate::byteenc::encode_bytes(b"\xe9"), "=", "é"]), 1);
        assert_eq!(
            t(&[&crate::byteenc::encode_bytes(b"\xe9"), "=", "\u{FFFD}"]),
            1
        );
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
    fn three_arg_binary_primary_beats_negation() {
        // POSIX `test`: `[ ! = x ]` compares the strings `!` and `x`.
        assert_eq!(t(&["!", "=", "x"]), 1);
        assert_eq!(t(&["!", "=", "!"]), 0);
        assert_eq!(t(&["!", "!=", "x"]), 0);
    }

    #[test]
    fn three_arg_negation_still_applies_without_binary_primary() {
        assert_eq!(t(&["!", "-n", ""]), 0);
        assert_eq!(t(&["!", "-n", "x"]), 1);
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
    fn dash_t_negative_fd_is_false() {
        // Negative fd can't be open; result is false (no UB).
        assert_eq!(t(&["-t", "-1"]), 1);
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

    #[test]
    fn string_order_primaries() {
        assert_eq!(t(&["a", "<", "b"]), 0);
        assert_eq!(t(&["b", "<", "a"]), 1);
        assert_eq!(t(&["b", ">", "a"]), 0);
        assert_eq!(t(&["a", ">", "a"]), 1);
        assert_eq!(t(&["B", "<", "a"]), 0); // C-locale byte order
        assert_eq!(t(&["", "<", "a"]), 0);
        assert_eq!(b(&["!", "a", "<", "b", "]"]), 1);
    }

    #[test]
    fn string_order_is_raw_bytes() {
        // Raw 0xff sorts after the UTF-8 lead byte of U+00E9 (0xc3);
        // the byteenc escape for 0xff must not change that.
        let ff = crate::byteenc::encode_bytes(b"\xff").into_owned();
        assert_eq!(t(&["\u{e9}", "<", &ff]), 0);
        assert_eq!(t(&[&ff, ">", "\u{e9}"]), 0);
    }

    #[test]
    fn file_time_and_identity_primaries() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old");
        let new = dir.path().join("new");
        let link = dir.path().join("link");
        std::fs::write(&old, "o").unwrap();
        std::fs::write(&new, "n").unwrap();
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(100);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(past)
            .unwrap();
        std::os::unix::fs::symlink(&old, &link).unwrap();
        let (o, n, l) = (
            old.to_str().unwrap(),
            new.to_str().unwrap(),
            link.to_str().unwrap(),
        );
        let missing = dir.path().join("missing");
        let m = missing.to_str().unwrap();

        assert_eq!(t(&[n, "-nt", o]), 0);
        assert_eq!(t(&[o, "-nt", n]), 1);
        assert_eq!(t(&[o, "-ot", n]), 0);
        assert_eq!(t(&[n, "-ot", o]), 1);
        assert_eq!(t(&[o, "-nt", o]), 1);
        // Existing vs missing.
        assert_eq!(t(&[o, "-nt", m]), 0);
        assert_eq!(t(&[m, "-nt", o]), 1);
        assert_eq!(t(&[m, "-ot", o]), 0);
        assert_eq!(t(&[o, "-ot", m]), 1);
        assert_eq!(t(&[m, "-nt", m]), 1);
        // -ef follows symlinks and needs both files to exist.
        assert_eq!(t(&[o, "-ef", l]), 0);
        assert_eq!(t(&[o, "-ef", n]), 1);
        assert_eq!(t(&[m, "-ef", m]), 1);
    }
}
