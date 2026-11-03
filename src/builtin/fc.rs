//! POSIX `fc` builtin (history listing / edit / re-execute).
//!
//! `fc` is a *regular* built-in per POSIX (XCU §1.7 / §2.15 list only
//! `break : continue . eval exec exit export readonly return set shift
//! times trap unset` as special), so it is dispatched from the
//! Executor-hosted regular-builtin path: prefix assignments are
//! temporary and a function named `fc` shadows it.

use std::io::Write;

use crate::error::{RuntimeErrorKind, ShellError};
use crate::exec::Executor;

pub fn builtin_fc(args: &[String], executor: &mut Executor) -> Result<i32, ShellError> {
    if executor.env.history.entries().is_empty() {
        return Err(ShellError::runtime(
            RuntimeErrorKind::IoError,
            "fc: history is empty".to_string(),
        ));
    }

    let mut list_mode = false;
    let mut suppress_numbers = false;
    let mut reverse = false;
    let mut substitute_mode = false;
    let mut editor: Option<String> = None;
    let mut operands: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-e" {
            i += 1;
            if i >= args.len() {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::IoError,
                    "fc: -e: option requires an argument".to_string(),
                ));
            }
            editor = Some(args[i].clone());
        } else if arg.starts_with('-')
            && arg.len() > 1
            && arg.chars().nth(1).is_some_and(|c| c.is_ascii_alphabetic())
        {
            for ch in arg[1..].chars() {
                match ch {
                    'l' => list_mode = true,
                    'n' => suppress_numbers = true,
                    'r' => reverse = true,
                    's' => substitute_mode = true,
                    _ => {
                        return Err(ShellError::runtime(
                            RuntimeErrorKind::InvalidArgument,
                            format!("fc: -{}: invalid option", ch),
                        ));
                    }
                }
            }
        } else {
            operands.push(arg.clone());
        }
        i += 1;
    }

    if substitute_mode {
        return fc_substitute(&operands, executor);
    }

    // Clone history entries to release the immutable borrow on executor,
    // allowing fc_edit to take &mut Executor.
    let entries: Vec<String> = executor.env.history.entries().to_vec();
    let (start, end) = fc_resolve_range(&operands, list_mode, &entries)
        .map_err(|msg| ShellError::runtime(RuntimeErrorKind::IoError, msg))?;

    if list_mode {
        fc_list(&entries, start, end, suppress_numbers, reverse);
        Ok(0)
    } else {
        fc_edit(&entries, start, end, reverse, editor, executor)
    }
}

/// Resolve one `first`/`last` operand to a 0-based index into `entries`.
///
/// * a positive number `n` is history number `n` (clamped to the
///   existing range);
/// * `0` or a negative number `-n` is relative to the end (`-1` and `0`
///   are the most recent entry; offsets past the start clamp to the
///   oldest entry);
/// * anything else is a prefix matched against the most recent entry
///   first — `None` when no entry starts with it.
///
/// `entries` must be non-empty (callers bail out on an empty history).
fn fc_resolve_one(spec: &str, entries: &[String]) -> Option<usize> {
    let last = entries.len().saturating_sub(1);
    if let Ok(n) = spec.parse::<i64>() {
        if n > 0 {
            // n >= 1, so n - 1 cannot overflow; clamp before converting
            // so a huge history number on a 32-bit usize is safe too.
            let idx = (n - 1).min(last as i64);
            Some(idx as usize)
        } else {
            // unsigned_abs avoids the overflow `-i64::MIN` would hit.
            let back = usize::try_from(n.unsigned_abs())
                .unwrap_or(usize::MAX)
                .max(1);
            Some(entries.len().saturating_sub(back))
        }
    } else {
        (0..entries.len())
            .rev()
            .find(|&i| entries[i].starts_with(spec))
    }
}

/// Resolve the `[first [last]]` operands to an index range. `Err` carries
/// the diagnostic for an unmatched string operand.
fn fc_resolve_range(
    operands: &[String],
    is_list: bool,
    entries: &[String],
) -> Result<(usize, usize), String> {
    let last = entries.len().saturating_sub(1);
    let resolve = |spec: &str| {
        fc_resolve_one(spec, entries).ok_or_else(|| {
            if is_list {
                "fc: history specification out of range".to_string()
            } else {
                "fc: no command found".to_string()
            }
        })
    };
    match operands.len() {
        0 => {
            if is_list {
                Ok((entries.len().saturating_sub(16), last))
            } else {
                Ok((last, last))
            }
        }
        1 => {
            let idx = resolve(&operands[0])?;
            if is_list {
                Ok((idx, last))
            } else {
                Ok((idx, idx))
            }
        }
        _ => Ok((resolve(&operands[0])?, resolve(&operands[1])?)),
    }
}

fn fc_list(entries: &[String], start: usize, end: usize, suppress_numbers: bool, reverse: bool) {
    let (lo, hi) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    let range: Vec<usize> = if reverse ^ (start > end) {
        (lo..=hi).rev().collect()
    } else {
        (lo..=hi).collect()
    };
    for i in range {
        if suppress_numbers {
            println!("\t{}", entries[i]);
        } else {
            println!("{}\t{}", i + 1, entries[i]);
        }
    }
}

fn fc_edit(
    entries: &[String],
    start: usize,
    end: usize,
    reverse: bool,
    editor: Option<String>,
    executor: &mut Executor,
) -> Result<i32, ShellError> {
    let editor_cmd = editor
        .or_else(|| executor.env.vars.get("FCEDIT").map(|s| s.to_string()))
        .or_else(|| executor.env.vars.get("EDITOR").map(|s| s.to_string()))
        .unwrap_or_else(|| "/bin/ed".to_string());

    let (lo, hi) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    let mut commands: Vec<&str> = (lo..=hi).map(|i| entries[i].as_str()).collect();
    if reverse {
        commands.reverse();
    }

    let tmp_path = match create_secure_tempfile("yosh_fc") {
        Ok(path) => path,
        Err(e) => {
            return Err(ShellError::runtime(
                RuntimeErrorKind::IoError,
                format!("fc: {}", e),
            ));
        }
    };
    {
        use std::fs::OpenOptions;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = match OpenOptions::new().write(true).mode(0o600).open(&tmp_path) {
            Ok(f) => f,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_path);
                return Err(ShellError::runtime(
                    RuntimeErrorKind::IoError,
                    format!("fc: cannot open temp file: {}", e),
                ));
            }
        };
        for cmd in &commands {
            let _ = writeln!(file, "{}", cmd);
        }
    }

    use std::process::Command;
    let status = Command::new(&editor_cmd).arg(&tmp_path).status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            let _ = std::fs::remove_file(&tmp_path);
            return Ok(s.code().unwrap_or(1));
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(ShellError::runtime(
                RuntimeErrorKind::CommandNotFound,
                format!("fc: {}: {}", editor_cmd, e),
            ));
        }
    }

    let content = match std::fs::read(&tmp_path) {
        Ok(c) => crate::byteenc::encode_bytes(&c).into_owned(),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(ShellError::runtime(
                RuntimeErrorKind::IoError,
                format!("fc: cannot read temp file: {}", e),
            ));
        }
    };
    let _ = std::fs::remove_file(&tmp_path);

    if content.trim().is_empty() {
        return Ok(0);
    }

    executor.eval_string(&content);
    Ok(executor.env.exec.last_exit_status)
}

fn fc_substitute(operands: &[String], executor: &mut Executor) -> Result<i32, ShellError> {
    let entries = executor.env.history.entries();
    if entries.is_empty() {
        return Err(ShellError::runtime(
            RuntimeErrorKind::IoError,
            "fc: history is empty".to_string(),
        ));
    }

    let mut replacement: Option<(&str, &str)> = None;
    let mut target_spec: Option<&str> = None;

    for op in operands {
        if let Some(eq_pos) = op.find('=') {
            replacement = Some((&op[..eq_pos], &op[eq_pos + 1..]));
        } else {
            target_spec = Some(op.as_str());
        }
    }

    let idx = match target_spec {
        Some(spec) => match fc_resolve_one(spec, entries) {
            Some(idx) => idx,
            None => {
                // bash: `fc -s zzz` with no match is an error; it must
                // never fall back to re-running the last command.
                return Err(ShellError::runtime(
                    RuntimeErrorKind::IoError,
                    "fc: no command found".to_string(),
                ));
            }
        },
        None => entries.len().saturating_sub(1),
    };

    let mut cmd = entries[idx].clone();
    if let Some((old, new)) = replacement {
        cmd = cmd.replacen(old, new, 1);
    }

    // Informational output — not an error
    eprintln!("{}", cmd);

    let histsize: usize = executor
        .env
        .vars
        .get("HISTSIZE")
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);
    let histcontrol = executor
        .env
        .vars
        .get("HISTCONTROL")
        .unwrap_or("ignoreboth")
        .to_string();
    executor.env.history.add(&cmd, histsize, &histcontrol);

    executor.eval_string(&cmd);
    Ok(executor.env.exec.last_exit_status)
}

/// Create a temporary file with a random name and restrictive permissions (0o600).
/// Uses `O_CREAT | O_EXCL` to atomically create the file, preventing TOCTOU races.
fn create_secure_tempfile(prefix: &str) -> Result<String, String> {
    use std::collections::hash_map::RandomState;
    use std::fs::OpenOptions;
    use std::hash::{BuildHasher, Hasher};
    use std::os::unix::fs::OpenOptionsExt;

    let tmp_dir = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());

    for _ in 0..16 {
        let s = RandomState::new();
        let mut hasher = s.build_hasher();
        hasher.write_u64(std::process::id() as u64);
        let rand_hex = format!("{:016x}", hasher.finish());
        let path = format!("{}/{}_{}", tmp_dir, prefix, rand_hex);

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(_) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("cannot create temp file: {}", e)),
        }
    }

    Err("cannot create temp file: too many collisions".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hist(n: usize) -> Vec<String> {
        (1..=n).map(|i| format!("cmd{i}")).collect()
    }

    #[test]
    fn resolve_positive_number_is_history_number_clamped() {
        let h = hist(5);
        assert_eq!(fc_resolve_one("1", &h), Some(0));
        assert_eq!(fc_resolve_one("5", &h), Some(4));
        assert_eq!(fc_resolve_one("99", &h), Some(4));
        assert_eq!(fc_resolve_one("9223372036854775807", &h), Some(4));
    }

    #[test]
    fn resolve_zero_is_most_recent_not_out_of_bounds() {
        let h = hist(5);
        assert_eq!(fc_resolve_one("0", &h), Some(4));
        assert_eq!(fc_resolve_one("-0", &h), Some(4));
    }

    #[test]
    fn resolve_negative_is_relative_and_clamped() {
        let h = hist(5);
        assert_eq!(fc_resolve_one("-1", &h), Some(4));
        assert_eq!(fc_resolve_one("-5", &h), Some(0));
        assert_eq!(fc_resolve_one("-6", &h), Some(0));
        // `-i64::MIN` would overflow a plain negation.
        assert_eq!(fc_resolve_one("-9223372036854775808", &h), Some(0));
    }

    #[test]
    fn resolve_string_prefix_most_recent_first_or_none() {
        let h = vec!["echo a".to_string(), "ls".to_string(), "echo b".to_string()];
        assert_eq!(fc_resolve_one("echo", &h), Some(2));
        assert_eq!(fc_resolve_one("l", &h), Some(1));
        assert_eq!(fc_resolve_one("zzz", &h), None);
    }

    #[test]
    fn resolve_range_errors_on_unmatched_string() {
        let h = hist(3);
        let ops = vec!["zzz".to_string()];
        assert_eq!(
            fc_resolve_range(&ops, false, &h),
            Err("fc: no command found".to_string())
        );
        assert_eq!(
            fc_resolve_range(&ops, true, &h),
            Err("fc: history specification out of range".to_string())
        );
    }

    #[test]
    fn resolve_range_list_defaults_and_zero_operand() {
        let h = hist(20);
        assert_eq!(fc_resolve_range(&[], true, &h), Ok((4, 19)));
        assert_eq!(fc_resolve_range(&[], false, &h), Ok((19, 19)));
        assert_eq!(fc_resolve_range(&["0".to_string()], true, &h), Ok((19, 19)));
        assert_eq!(
            fc_resolve_range(&["0".to_string()], false, &h),
            Ok((19, 19))
        );
    }
}
