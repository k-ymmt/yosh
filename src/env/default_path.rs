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

use crate::env::ShellEnv;

/// Return the POSIX default PATH, cached per `ShellEnv`.
///
/// Computed once via `call_confstr()`; falls back to `fallback_default_path()`
/// if `confstr` fails. Never panics.
pub fn default_path(env: &ShellEnv) -> &str {
    env.default_path_cache
        .get_or_init(|| call_confstr().unwrap_or_else(fallback_default_path))
        .as_str()
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

    use crate::env::ShellEnv;

    #[test]
    fn default_path_is_non_empty() {
        let env = ShellEnv::new("yosh", vec![]);
        assert!(!default_path(&env).is_empty());
    }

    #[test]
    fn default_path_contains_bin_or_usr_bin() {
        let env = ShellEnv::new("yosh", vec![]);
        let dp = default_path(&env);
        assert!(
            dp.split(':').any(|d| d == "/bin" || d == "/usr/bin"),
            "expected /bin or /usr/bin in default path, got: {dp}"
        );
    }

    #[test]
    fn default_path_finds_sh() {
        // /bin/sh is POSIX-mandatory on every conforming system (macOS + Linux).
        use crate::exec::command::find_in_path;
        let env = ShellEnv::new("yosh", vec![]);
        let dp = default_path(&env);
        assert!(find_in_path("sh", dp).is_some(), "expected to find sh in: {dp}");
    }

    #[test]
    fn default_path_is_cached() {
        // Two calls return the same slice — proves OnceLock caches.
        let env = ShellEnv::new("yosh", vec![]);
        let a = default_path(&env).as_ptr();
        let b = default_path(&env).as_ptr();
        assert_eq!(a, b, "default_path should return the same cached string");
    }
}
