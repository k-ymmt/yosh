//! `pwd` regular builtin (POSIX XCU `pwd`).
//!
//! It must be built in: the logical working directory (`-L`, the
//! default) is shell state — `$PWD` as maintained by `cd` — which an
//! external `/bin/pwd` only sees when `PWD` happens to be exported, and
//! `pwd` must keep working with an empty or broken `PATH`.

use crate::builtin::options::{invalid_flag, scan_flag_words};
use crate::builtin::output::write_stdout_or_report;
use crate::env::ShellEnv;
use crate::env::startup_pwd::{logical_cwd, physical_cwd};
use crate::error::{RuntimeErrorKind, ShellError};

pub fn builtin_pwd(args: &[String], env: &ShellEnv) -> Result<i32, ShellError> {
    // `-L` / `-P` may repeat or cluster; the last one wins. Operands are
    // ignored (unspecified by POSIX; dash ignores them too).
    let mut physical = false;
    scan_flag_words(args, |ch| {
        match ch {
            'L' => physical = false,
            'P' => physical = true,
            _ => return false,
        }
        true
    })
    .map_err(|e| {
        ShellError::runtime(
            RuntimeErrorKind::InvalidArgument,
            invalid_flag("pwd", e.flag),
        )
    })?;

    let cwd = if physical {
        physical_cwd()
    } else {
        logical_cwd(env.vars.get("PWD"))
    };
    match cwd {
        Some(dir) => Ok(write_stdout_or_report("pwd", &dir, true)),
        None => {
            // getcwd failed (e.g. the working directory was removed);
            // ask once more for the reason.
            let reason = std::env::current_dir()
                .err()
                .map_or_else(|| "unknown error".to_string(), |e| e.to_string());
            eprintln!("yosh: pwd: cannot determine the current directory: {reason}");
            Ok(1)
        }
    }
}
