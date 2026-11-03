//! Stdout writing for builtins.
//!
//! Builtin output is byteenc-encoded shell text; it is decoded back to
//! the original raw bytes at the fd 1 boundary. Output builtins whose
//! exit status must reflect a failed write (`echo`, `printf`, `type`,
//! `alias`, `umask`, `ulimit`, `kill -l`) go through `write(2)` directly
//! because `std::io::Stdout` silently swallows `EBADF`, so `echo hi >&-`
//! would otherwise succeed; POSIX/dash/bash report the failure with
//! status 1.

/// Write `buf` to fd 1 with `write(2)` directly. `std::io::Stdout`
/// silently swallows `EBADF` (a closed stdout, `printf x >&-`), but
/// POSIX requires output builtins to report a write failure.
pub(crate) fn write_stdout_bytes(buf: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut lock = std::io::stdout().lock();
    // Anything still sitting in std's buffer must go out first so the
    // raw write below cannot overtake it.
    let _ = lock.flush();
    let mut rest = buf;
    while !rest.is_empty() {
        let n = unsafe { libc::write(1, rest.as_ptr().cast(), rest.len()) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        rest = &rest[n as usize..];
    }
    Ok(())
}

/// Write a byteenc-encoded string to stdout as raw bytes (escape
/// codepoints become their original bytes), optionally newline-terminated.
/// Write errors are ignored; output builtins whose exit status must
/// reflect a failed write use [`try_write_stdout_decoded`].
pub(crate) fn write_stdout_decoded(s: &str, newline: bool) {
    let _ = try_write_stdout_decoded(s, newline);
}

/// Like [`write_stdout_decoded`] but reports write failures (see the
/// module docs for why this bypasses `std::io::Stdout`).
pub(crate) fn try_write_stdout_decoded(s: &str, newline: bool) -> std::io::Result<()> {
    let mut buf = crate::byteenc::decode_bytes(s).into_owned();
    if newline {
        buf.push(b'\n');
    }
    write_stdout_bytes(&buf)
}

/// Report a stdout write failure for builtin `name` (`yosh: name: write
/// error: …`) and return the status the builtin must exit with.
pub(crate) fn report_write_error(name: &str, e: &std::io::Error) -> i32 {
    eprintln!("yosh: {}: write error: {}", name, e);
    1
}

/// Write `s` like [`try_write_stdout_decoded`] and return builtin
/// `name`'s exit status: 0, or 1 after reporting the write error.
pub(crate) fn write_stdout_or_report(name: &str, s: &str, newline: bool) -> i32 {
    match try_write_stdout_decoded(s, newline) {
        Ok(()) => 0,
        Err(e) => report_write_error(name, &e),
    }
}
