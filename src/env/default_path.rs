//! POSIX default PATH discovery via `confstr(_CS_PATH)`.

use std::ptr;

/// Hardcoded fallback PATH used when `confstr(_CS_PATH)` is unavailable or fails.
///
/// Chosen to be minimal and work on any POSIX-like system without depending
/// on `/usr/local/bin` (absent on many minimal Linux containers) or `.`
/// (classic security foot-gun).
pub fn fallback_default_path() -> String {
    "/bin:/usr/bin".to_string()
}

/// Call `libc::confstr(_CS_PATH, ...)` to retrieve the POSIX-recommended
/// default PATH. Returns `None` if `confstr` is unsupported on this OS,
/// returns 0, or produces invalid UTF-8.
///
/// This is a thin unsafe FFI wrapper — the unsafety is limited to the two
/// libc calls. The returned String is safe to pass around freely.
pub fn call_confstr() -> Option<String> {
    // Step 1: query required buffer size (NUL included).
    // Safety: passing null_mut + 0 is explicitly allowed by POSIX for size
    // queries. No memory is written.
    let needed = unsafe { libc::confstr(libc::_CS_PATH, ptr::null_mut(), 0) };
    if needed == 0 {
        return None;
    }

    // Step 2: allocate and fill the buffer.
    let mut buf = vec![0u8; needed];
    // Safety: buf is exactly `needed` bytes long, matching the size confstr
    // asked for on the previous call. confstr writes up to `needed` bytes
    // including NUL.
    let written = unsafe { libc::confstr(libc::_CS_PATH, buf.as_mut_ptr().cast(), needed) };
    if written == 0 || written > needed {
        return None;
    }

    // Drop the trailing NUL.
    buf.truncate(written.saturating_sub(1));
    String::from_utf8(buf).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_is_bin_usr_bin() {
        assert_eq!(fallback_default_path(), "/bin:/usr/bin");
    }

    #[test]
    fn fallback_does_not_contain_cwd_or_empty() {
        let p = fallback_default_path();
        assert!(!p.split(':').any(|d| d == "." || d.is_empty()));
    }

    #[test]
    fn call_confstr_returns_something_usable() {
        // macOS and Linux both implement _CS_PATH; failure here would mean
        // the OS is genuinely non-POSIX (CI sanity check).
        let p = call_confstr().expect("confstr(_CS_PATH) should succeed on POSIX systems");
        assert!(!p.is_empty());
        // Must contain at least one of /bin or /usr/bin: true on both macOS
        // and Linux default values, without asserting the exact string.
        assert!(
            p.split(':').any(|d| d == "/bin" || d == "/usr/bin"),
            "expected /bin or /usr/bin in confstr PATH, got: {p}"
        );
    }

    #[test]
    fn call_confstr_has_no_cwd_or_empty_entries() {
        // POSIX _CS_PATH never includes "." or empty segments.
        let p = call_confstr().expect("confstr should succeed");
        assert!(!p.split(':').any(|d| d == "." || d.is_empty()));
    }
}
