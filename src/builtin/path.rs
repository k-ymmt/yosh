//! Conversions between the shell's byteenc-encoded path strings and OS
//! paths carrying the original raw bytes.

use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// Convert a byteenc-encoded shell path string into an `OsStr`-backed path
/// carrying the original raw bytes for OS calls.
pub(crate) fn os_path(s: &str) -> PathBuf {
    PathBuf::from(std::ffi::OsStr::from_bytes(&crate::byteenc::decode_bytes(
        s,
    )))
}

/// Encode an OS path back into the shell's internal (byteenc) string form.
pub(crate) fn encode_path(p: &Path) -> String {
    crate::byteenc::encode_bytes(p.as_os_str().as_bytes()).into_owned()
}
