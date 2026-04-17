//! POSIX default PATH discovery via `confstr(_CS_PATH)`.

/// Hardcoded fallback PATH used when `confstr(_CS_PATH)` is unavailable or fails.
///
/// Chosen to be minimal and work on any POSIX-like system without depending
/// on `/usr/local/bin` (absent on many minimal Linux containers) or `.`
/// (classic security foot-gun).
pub fn fallback_default_path() -> String {
    "/bin:/usr/bin".to_string()
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
}
