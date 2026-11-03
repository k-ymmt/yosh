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
    let invalid_mode = || {
        ShellError::runtime(
            RuntimeErrorKind::InvalidArgument,
            format!("umask: {}: invalid symbolic mode", s),
        )
    };
    let mut mask = current_umask();

    for clause in s.split(',') {
        let bytes = clause.as_bytes();
        if bytes.is_empty() {
            return Err(invalid_mode());
        }

        let mut i = 0;
        let mut who_mask: libc::mode_t = 0;

        // Parse who (u/g/o/a)
        while i < bytes.len() && matches!(bytes[i], b'u' | b'g' | b'o' | b'a') {
            match bytes[i] {
                b'u' => who_mask |= 0o700,
                b'g' => who_mask |= 0o070,
                b'o' => who_mask |= 0o007,
                b'a' => who_mask |= 0o777,
                _ => unreachable!(),
            }
            i += 1;
        }

        // Default to 'a' if no who specified
        if who_mask == 0 {
            who_mask = 0o777;
        }

        // Parse operator (=, +, -)
        if i >= bytes.len() || !matches!(bytes[i], b'=' | b'+' | b'-') {
            return Err(invalid_mode());
        }
        let op = bytes[i] as char;
        i += 1;

        // Parse permissions (r/w/x)
        let mut perm_bits: libc::mode_t = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'r' => perm_bits |= 0o444,
                b'w' => perm_bits |= 0o222,
                b'x' => perm_bits |= 0o111,
                _ => {
                    return Err(invalid_mode());
                }
            }
            i += 1;
        }

        // Apply within the who mask
        let effective_perms = perm_bits & who_mask;

        match op {
            '=' => {
                // Clear who bits, then set umask to deny everything NOT in perm
                mask = (mask & !who_mask) | (who_mask & !effective_perms);
            }
            '+' => {
                // Adding permissions = clearing umask bits
                mask &= !effective_perms;
            }
            '-' => {
                // Removing permissions = setting umask bits
                mask |= effective_perms;
            }
            _ => unreachable!(),
        }
    }

    unsafe { libc::umask(mask) };
    Ok(0)
}

/// The current file mode creation mask. `umask(2)` can only be read by
/// setting it, so the old value is restored immediately.
fn current_umask() -> libc::mode_t {
    let current = unsafe { libc::umask(0) };
    unsafe { libc::umask(current) };
    current
}
