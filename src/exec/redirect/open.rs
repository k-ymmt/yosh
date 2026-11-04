//! Opening redirection targets and the fd-level primitives the
//! redirections are built from, with their diagnostics.

use std::os::fd::RawFd;
use std::os::unix::io::IntoRawFd;

use nix::fcntl::{OFlag, open};
use nix::sys::stat::Mode;

/// Open a byteenc-encoded redirect target, decoding escape codepoints so
/// non-UTF-8 paths reach the OS as their original raw bytes.
fn open_decoded(path: &str, flags: OFlag, mode: Mode) -> nix::Result<std::os::fd::OwnedFd> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = crate::byteenc::decode_bytes(path);
    open(std::ffi::OsStr::from_bytes(&bytes), flags, mode)
}

/// The C library's message for `e` (`No such file or directory`), for
/// redirection diagnostics: nix's own `Display` adds the symbolic name
/// (`ENOENT: ...`) and has its own wording for some codes.
pub(crate) fn errno_text(e: nix::errno::Errno) -> String {
    io_error_text(&std::io::Error::from_raw_os_error(e as i32))
}

/// An I/O error as the shell reports it: the C library's message alone
/// (`No such file or directory`), without the ` (os error 2)` suffix
/// Rust's `Display` appends for OS errors.
pub(crate) fn io_error_text(e: &std::io::Error) -> String {
    let msg = e.to_string();
    match (e.raw_os_error(), msg.rfind(" (os error ")) {
        (Some(_), Some(pos)) => msg[..pos].to_string(),
        _ => msg,
    }
}

/// [`open_decoded`] as a raw fd, with the error formatted as the
/// redirection diagnostic (`path: reason`).
pub(super) fn open_path(path: &str, flags: OFlag, mode: Mode) -> Result<RawFd, String> {
    open_decoded(path, flags, mode)
        .map(|fd| fd.into_raw_fd())
        .map_err(|e| format!("{}: {}", path, errno_text(e)))
}

/// Mode for files created by `>`, `>>` and `<>`.
pub(super) const CREATE_MODE: Mode = Mode::from_bits_truncate(0o644);

/// Open `path` for `>` output under `set -C` (noclobber) without the
/// TOCTOU race of a separate exists() check: O_CREAT|O_EXCL atomically
/// fails with EEXIST if anything (including a symlink) already occupies
/// the path. Existing *non-regular* files (e.g. /dev/null, FIFOs) are
/// then reopened without O_CREAT/O_TRUNC, per POSIX — noclobber only
/// protects regular files.
pub(super) fn open_noclobber(path: &str) -> Result<RawFd, String> {
    let create_flags = OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL;
    match open_decoded(path, create_flags, CREATE_MODE) {
        Ok(fd) => Ok(fd.into_raw_fd()),
        Err(nix::errno::Errno::EEXIST) => {
            // stat() follows symlinks: a dangling link errors here, and a
            // link to a regular file is treated as that regular file.
            match std::fs::metadata({
                use std::os::unix::ffi::OsStrExt;
                std::path::PathBuf::from(std::ffi::OsStr::from_bytes(
                    &crate::byteenc::decode_bytes(path),
                ))
            }) {
                Ok(md) if !md.is_file() => open_path(path, OFlag::O_WRONLY, Mode::empty()),
                _ => Err(format!("{}: cannot overwrite existing file", path)),
            }
        }
        Err(e) => Err(format!("{}: {}", path, errno_text(e))),
    }
}

/// Perform a low-level dup2(oldfd, newfd) via libc.
pub(super) fn raw_dup2(oldfd: RawFd, newfd: RawFd) -> nix::Result<()> {
    let res = unsafe { libc::dup2(oldfd, newfd) };
    if res == -1 {
        Err(nix::errno::Errno::last())
    } else {
        Ok(())
    }
}
