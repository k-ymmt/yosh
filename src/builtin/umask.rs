//! `umask` regular builtin (POSIX XCU `umask`): print the file mode
//! creation mask (octal or `-S` symbolic) or set it from an octal or
//! symbolic mode.

use crate::builtin::options::invalid_option;
use crate::builtin::output::write_stdout_or_report;
use crate::error::{RuntimeErrorKind, ShellError};

pub fn builtin_umask(args: &[String]) -> Result<i32, ShellError> {
    // Options: `-S` (symbolic output) and `--` (end of options). Any
    // other `-…` word is an invalid option (dash/bash), never a symbolic
    // mode — `umask -x` used to silently clear the x bits.
    let mut symbolic = false;
    let mut i = 0;
    while let Some(a) = args.get(i) {
        match a.as_str() {
            "--" => {
                i += 1;
                break;
            }
            "-S" => symbolic = true,
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidOption,
                    invalid_option("umask", s),
                ));
            }
            _ => break,
        }
        i += 1;
    }

    // POSIX: the mask is written only when no mask operand is given;
    // `-S` merely selects the output format (dash: `umask -S 077` sets
    // the mask silently). Extra operands are ignored, as in dash/bash.
    let Some(mask) = args.get(i) else {
        let current = current_umask();
        let line = if symbolic {
            umask_to_symbolic(current)
        } else {
            format!("{:04o}", current)
        };
        return Ok(write_stdout_or_report("umask", &line, true));
    };

    // Try octal parse first
    if mask.chars().all(|c| c.is_ascii_digit()) {
        return umask_set_octal(mask);
    }

    // Try symbolic parse
    umask_set_symbolic(mask)
}

fn umask_to_symbolic(mask: libc::mode_t) -> String {
    let perms = 0o777 & !mask;
    let fmt = |bits: libc::mode_t| -> String {
        let mut s = String::new();
        if bits & 4 != 0 {
            s.push('r');
        }
        if bits & 2 != 0 {
            s.push('w');
        }
        if bits & 1 != 0 {
            s.push('x');
        }
        s
    };
    format!(
        "u={},g={},o={}",
        fmt((perms >> 6) & 7),
        fmt((perms >> 3) & 7),
        fmt(perms & 7),
    )
}

fn umask_set_octal(s: &str) -> Result<i32, ShellError> {
    let invalid_number = || {
        ShellError::runtime(
            RuntimeErrorKind::InvalidArgument,
            format!("umask: {}: invalid octal number", s),
        )
    };
    if !s.chars().all(|c| ('0'..='7').contains(&c)) {
        return Err(invalid_number());
    }
    let mode = libc::mode_t::from_str_radix(s, 8).map_err(|_| invalid_number())?;
    unsafe { libc::umask(mode) };
    Ok(0)
}

fn umask_set_symbolic(s: &str) -> Result<i32, ShellError> {
    let mask = apply_symbolic_mode(s, current_umask()).ok_or_else(|| {
        ShellError::runtime(
            RuntimeErrorKind::InvalidArgument,
            format!("umask: {}: invalid symbolic mode", s),
        )
    })?;
    unsafe { libc::umask(mask) };
    Ok(0)
}

/// Apply a POSIX `chmod`-style symbolic mode (XCU `chmod` grammar, which
/// XCU `umask` reuses) to `mask` and return the new mask, or `None` when
/// `s` is malformed. The mode edits the permissions the mask allows
/// (`!mask & 0o777`):
///
/// ```text
/// clause     : [who...] action...      (clauses separated by `,`)
/// who        : u | g | o | a           (omitted = a)
/// action     : op [permlist | permcopy]
/// op         : + | - | =
/// permlist   : r | w | x | X | s | t ...
/// permcopy   : u | g | o               (copy that class's current bits)
/// ```
///
/// `X` means `x` when some execute bit is already allowed; `s` and `t`
/// name bits outside the permission bits and so change nothing (dash).
fn apply_symbolic_mode(s: &str, mask: libc::mode_t) -> Option<libc::mode_t> {
    let mut perms: libc::mode_t = !mask & 0o777;

    for clause in s.split(',') {
        let bytes = clause.as_bytes();
        let mut i = 0;
        let mut who_mask: libc::mode_t = 0;
        while let Some(&b) = bytes.get(i) {
            who_mask |= match b {
                b'u' => 0o700,
                b'g' => 0o070,
                b'o' => 0o007,
                b'a' => 0o777,
                _ => break,
            };
            i += 1;
        }
        if who_mask == 0 {
            who_mask = 0o777;
        }

        // At least one action per clause.
        if !matches!(bytes.get(i), Some(b'=' | b'+' | b'-')) {
            return None;
        }
        while let Some(&op) = bytes.get(i) {
            if !matches!(op, b'=' | b'+' | b'-') {
                return None;
            }
            i += 1;

            // A permcopy is a single class letter; otherwise a permlist.
            let bits = match bytes.get(i) {
                Some(&c @ (b'u' | b'g' | b'o')) => {
                    i += 1;
                    let shift = match c {
                        b'u' => 6,
                        b'g' => 3,
                        _ => 0,
                    };
                    ((perms >> shift) & 0o7) * 0o111
                }
                _ => {
                    let mut bits: libc::mode_t = 0;
                    while let Some(&c) = bytes.get(i) {
                        bits |= match c {
                            b'r' => 0o444,
                            b'w' => 0o222,
                            b'x' => 0o111,
                            b'X' if perms & 0o111 != 0 => 0o111,
                            b'X' | b's' | b't' => 0,
                            _ => break,
                        };
                        i += 1;
                    }
                    bits
                }
            };

            let bits = bits & who_mask;
            perms = match op {
                b'=' => (perms & !who_mask) | bits,
                b'+' => perms | bits,
                _ => perms & !bits,
            };
        }
    }

    Some(!perms & 0o777)
}

/// The current file mode creation mask. `umask(2)` can only be read by
/// setting it, so the old value is restored immediately.
fn current_umask() -> libc::mode_t {
    let current = unsafe { libc::umask(0) };
    unsafe { libc::umask(current) };
    current
}

#[cfg(test)]
mod tests {
    use super::apply_symbolic_mode;

    #[test]
    fn single_actions() {
        assert_eq!(apply_symbolic_mode("u=rwx,g+w", 0o022), Some(0o002));
        assert_eq!(apply_symbolic_mode("ug=", 0o022), Some(0o772));
        assert_eq!(apply_symbolic_mode("=", 0o022), Some(0o777));
        assert_eq!(apply_symbolic_mode("+x", 0o022), Some(0o022));
        assert_eq!(apply_symbolic_mode("o-r", 0o022), Some(0o026));
    }

    #[test]
    fn several_actions_in_one_clause() {
        // XCU chmod: `actionlist : action | actionlist action`.
        assert_eq!(apply_symbolic_mode("u+r-w", 0o022), Some(0o222));
        assert_eq!(apply_symbolic_mode("a=r+x", 0o000), Some(0o222));
    }

    #[test]
    fn permcopy_copies_the_current_class_bits() {
        assert_eq!(apply_symbolic_mode("u=g", 0o022), Some(0o222));
        assert_eq!(apply_symbolic_mode("g=u,o=", 0o222), Some(0o227));
        assert_eq!(apply_symbolic_mode("go=u", 0o077), Some(0o000));
    }

    #[test]
    fn capital_x_depends_on_existing_execute_bits() {
        assert_eq!(apply_symbolic_mode("a=rX", 0o022), Some(0o222));
        assert_eq!(apply_symbolic_mode("a+X", 0o777), Some(0o777));
    }

    #[test]
    fn set_id_and_sticky_change_nothing() {
        assert_eq!(apply_symbolic_mode("u+s", 0o022), Some(0o022));
        assert_eq!(apply_symbolic_mode("o+t", 0o022), Some(0o022));
    }

    #[test]
    fn malformed_modes_are_rejected() {
        for bad in ["", "u", "u+q", "u=gw", "x", "u+r,", ",u+r", "uq+r"] {
            assert_eq!(apply_symbolic_mode(bad, 0o022), None, "{bad:?}");
        }
    }
}
