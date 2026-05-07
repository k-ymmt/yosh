//! POSIX tilde expansion: `~` (HOME) and `~user` (getpwnam).
//!
//! Used by `expand::pipeline` for inline word-part tilde, by `expand::heredoc`
//! for unquoted heredoc bodies, and re-exported from `expand` for use by
//! `interactive::mod` during ENV preprocessing.

/// Expand a tilde prefix in a string: `~` uses `home_dir`, `~user` uses getpwnam.
/// Returns the original string unchanged if the prefix doesn't start with `~`
/// or expansion fails.
pub fn expand_tilde_prefix(home_dir: Option<&str>, s: &str) -> String {
    let rest = match s.strip_prefix('~') {
        Some(r) => r,
        None => return s.to_string(),
    };
    let (user, suffix) = match rest.find('/') {
        Some(pos) => (&rest[..pos], &rest[pos..]),
        None => (rest, ""),
    };
    if user.is_empty() {
        // ~ alone: use provided home directory
        match home_dir {
            Some(home) if !home.is_empty() => format!("{}{}", home, suffix),
            _ => s.to_string(),
        }
    } else {
        // ~user: resolve via getpwnam
        let expanded = expand_tilde_user(user);
        if expanded.starts_with('~') {
            s.to_string() // unknown user, keep original
        } else {
            format!("{}{}", expanded, suffix)
        }
    }
}

/// Expand `~username` using `getpwnam`.
pub fn expand_tilde_user(user: &str) -> String {
    use std::ffi::CString;
    let c_user = match CString::new(user) {
        Ok(s) => s,
        Err(_) => return format!("~{}", user),
    };
    // SAFETY: getpwnam is reentrant enough for single-threaded shell use.
    let pw = unsafe { libc::getpwnam(c_user.as_ptr()) };
    if pw.is_null() {
        return format!("~{}", user);
    }
    let dir = unsafe { std::ffi::CStr::from_ptr((*pw).pw_dir) };
    dir.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde_prefix_home() {
        assert_eq!(
            expand_tilde_prefix(Some("/home/user"), "~/docs"),
            "/home/user/docs"
        );
    }

    #[test]
    fn test_expand_tilde_prefix_home_only() {
        assert_eq!(expand_tilde_prefix(Some("/home/user"), "~"), "/home/user");
    }

    #[test]
    fn test_expand_tilde_prefix_no_home() {
        assert_eq!(expand_tilde_prefix(None, "~/docs"), "~/docs");
    }

    #[test]
    fn test_expand_tilde_prefix_no_tilde() {
        assert_eq!(
            expand_tilde_prefix(Some("/home/user"), "/abs/path"),
            "/abs/path"
        );
    }

    #[test]
    fn test_expand_tilde_prefix_empty_home() {
        assert_eq!(expand_tilde_prefix(Some(""), "~/docs"), "~/docs");
    }
}
