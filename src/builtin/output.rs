//! Stdout writing for builtins.
//!
//! Builtin output is byteenc-encoded shell text; it is decoded back to
//! the original raw bytes at the fd 1 boundary. Every builtin's stdout
//! (`echo`, `printf`, `pwd`, and the listings of `export -p`, `set`,
//! `trap`, `jobs`, `hash`, ...) goes through `write(2)` directly and
//! reports a failed write with status 1, because `std::io::Stdout`
//! silently swallows `EBADF` (`echo hi >&-` would otherwise succeed)
//! and `println!` panics on other errors; dash and bash report the
//! failure too.

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
/// codepoints become their original bytes), optionally newline-terminated,
/// reporting write failures (see the module docs for why this bypasses
/// `std::io::Stdout`).
pub(crate) fn try_write_stdout_decoded(s: &str, newline: bool) -> std::io::Result<()> {
    let mut buf = crate::byteenc::decode_bytes(s).into_owned();
    if newline {
        buf.push(b'\n');
    }
    write_stdout_bytes(&buf)
}

thread_local! {
    /// Set by [`report_write_error`]; see [`take_write_error_reported`].
    static WRITE_ERROR_REPORTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Report a stdout write failure for builtin `name` (`yosh: name: write
/// error: …`) and return the status the builtin must exit with.
pub(crate) fn report_write_error(name: &str, e: &std::io::Error) -> i32 {
    crate::byteenc::write_stderr_decoded_line(&format!(
        "yosh: {}: write error: {}",
        name,
        crate::exec::redirect::io_error_text(e)
    ));
    WRITE_ERROR_REPORTED.with(|f| f.set(true));
    1
}

/// Whether a builtin reported a stdout write error since the last call
/// (clears the flag). Lets the special-builtin dispatcher tell a failed
/// listing (`export -p >&-`: status 1, the shell keeps running — dash
/// agrees) from a special-built-in utility error that ends a
/// non-interactive shell.
pub(crate) fn take_write_error_reported() -> bool {
    WRITE_ERROR_REPORTED.with(|f| f.replace(false))
}

/// Write `lines`, each newline-terminated, to stdout with one `write(2)`
/// and return builtin `name`'s exit status: 0, or 1 after reporting the
/// write error. Used by the listing builtins (`export -p`, `set`,
/// `trap`, `jobs`, ...).
pub(crate) fn write_lines_or_report<S: AsRef<str>>(
    name: &str,
    lines: impl IntoIterator<Item = S>,
) -> i32 {
    let mut buf = String::new();
    for line in lines {
        buf.push_str(line.as_ref());
        buf.push('\n');
    }
    if buf.is_empty() {
        return 0;
    }
    write_stdout_or_report(name, &buf, false)
}

/// Write `s` like [`try_write_stdout_decoded`] and return builtin
/// `name`'s exit status: 0, or 1 after reporting the write error.
pub(crate) fn write_stdout_or_report(name: &str, s: &str, newline: bool) -> i32 {
    match try_write_stdout_decoded(s, newline) {
        Ok(()) => 0,
        Err(e) => report_write_error(name, &e),
    }
}
