//! `echo` regular builtin. Only a leading `-n` (suppress the trailing
//! newline) is recognized; every other word is printed literally.

use crate::builtin::output::write_stdout_or_report;
use crate::error::ShellError;

pub fn builtin_echo(args: &[String]) -> Result<i32, ShellError> {
    Ok(if args.first().map(|a| a.as_str()) == Some("-n") {
        write_stdout_or_report("echo", &args[1..].join(" "), false)
    } else {
        write_stdout_or_report("echo", &args.join(" "), true)
    })
}
