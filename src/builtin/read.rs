//! POSIX `read` builtin.
//!
//! `read [-r] var [var ...]` — read one logical line from stdin and
//! assign IFS-split fields to the named variables. With no `-r`,
//! backslash is the escape character (line continuation on `\<newline>`,
//! `\X` keeps `X` literally).

use crate::env::ShellEnv;
use crate::error::ShellError;

pub fn builtin_read(_args: &[String], _env: &mut ShellEnv) -> Result<i32, ShellError> {
    eprintln!("yosh: read: not implemented");
    Ok(1)
}
