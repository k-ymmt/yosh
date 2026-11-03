//! `eval` and `.` (dot) special builtins: run shell code in the
//! current execution environment (POSIX XCU §2.15).

use crate::builtin::path::os_path;
use crate::error::{RuntimeErrorKind, ShellError};
use crate::exec::Executor;

pub fn builtin_eval(args: &[String], executor: &mut Executor) -> Result<i32, ShellError> {
    if args.is_empty() {
        return Ok(0);
    }
    if executor.recursion_too_deep("eval") {
        return Ok(1);
    }
    let input = args.join(" ");
    match executor.exec_text(&input) {
        Ok(status) => Ok(status),
        Err(e) => {
            // ShellError's Display already starts with `yosh: `.
            let msg = e.to_string();
            eprintln!("yosh: eval: {}", msg.strip_prefix("yosh: ").unwrap_or(&msg));
            Ok(2)
        }
    }
}

pub fn builtin_source(args: &[String], executor: &mut Executor) -> Result<i32, ShellError> {
    if args.is_empty() {
        return Err(ShellError::runtime(
            RuntimeErrorKind::InvalidArgument,
            ".: filename argument required".to_string(),
        ));
    }
    let filename = &args[0];
    // Decode byteenc-escaped bytes so non-UTF-8 script paths resolve to
    // the real on-disk names.
    let path = if filename.contains('/') {
        os_path(filename)
    } else {
        if let Some(path_var) = executor.env.vars.get("PATH") {
            let mut found = None;
            for dir in path_var.split(':') {
                let candidate = os_path(dir).join(os_path(filename));
                if candidate.is_file() {
                    found = Some(candidate);
                    break;
                }
            }
            match found {
                Some(p) => p,
                None => {
                    return Err(ShellError::runtime(
                        RuntimeErrorKind::IoError,
                        format!(".: {}: not found", filename),
                    ));
                }
            }
        } else {
            os_path(filename)
        }
    };
    if executor.recursion_too_deep(".") {
        return Ok(1);
    }
    match executor.source_file(&path) {
        Some(status) => Ok(status),
        None => Err(ShellError::runtime(
            RuntimeErrorKind::IoError,
            format!(".: {}: No such file or directory", path.display()),
        )),
    }
}
