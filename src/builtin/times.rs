//! `times` special builtin: print the accumulated user and system times
//! of the shell and its children (POSIX XCU §2.15 `times`).

use crate::error::{RuntimeErrorKind, ShellError};

pub fn builtin_times(args: &[String]) -> Result<i32, ShellError> {
    if !args.is_empty() {
        return Err(ShellError::runtime(
            RuntimeErrorKind::InvalidArgument,
            format!("times: unexpected operand: {}", args[0]),
        ));
    }
    let mut tms: libc::tms = unsafe { std::mem::zeroed() };
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    if unsafe { libc::times(&mut tms) } == u64::MAX {
        return Err(ShellError::runtime(
            RuntimeErrorKind::IoError,
            "times: failed".to_string(),
        ));
    }
    let fmt = |t: libc::clock_t| -> String {
        let secs = t as f64 / ticks;
        let m = (secs / 60.0) as u64;
        let s = secs - (m as f64 * 60.0);
        format!("{}m{:.3}s", m, s)
    };
    println!("{} {}", fmt(tms.tms_utime), fmt(tms.tms_stime));
    println!("{} {}", fmt(tms.tms_cutime), fmt(tms.tms_cstime));
    Ok(0)
}
