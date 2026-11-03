use std::ffi::CString;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ExecCStringError {
    CommandName,
    Argument(String),
}

/// Build C-string argv for `execv` on an already-resolved
/// absolute path: the first element returned is the path to pass as the
/// syscall's file argument, while `argv[0]` (the first entry of the
/// returned Vec) stays `display_name` (the original, unresolved command
/// name) — matching what `execvp(cmd, ...)` would have set argv[0] to.
/// Overlay prefix-assignment `overrides` on the cached exported envp:
/// entries whose key is overridden are dropped, then the overrides are
/// appended (decoded from byteenc). Only built when overrides exist.
/// `argv`/`envp`-style pointer array for `posix_spawn`.
pub(super) fn nul_terminated_ptrs(strings: &[CString]) -> Vec<*mut libc::c_char> {
    strings
        .iter()
        .map(|c| c.as_ptr() as *mut libc::c_char)
        .chain(std::iter::once(std::ptr::null_mut()))
        .collect()
}

pub(super) fn merge_envp(base: &[CString], overrides: &[(String, String)]) -> Vec<CString> {
    let keys: Vec<Vec<u8>> = overrides
        .iter()
        .map(|(k, _)| {
            let mut b = crate::byteenc::decode_bytes(k).into_owned();
            b.push(b'=');
            b
        })
        .collect();
    let mut out: Vec<CString> = base
        .iter()
        .filter(|c| !keys.iter().any(|k| c.as_bytes().starts_with(k)))
        .cloned()
        .collect();
    // `A=one A=two cmd`: the last assignment for a name wins, and the
    // name must appear once (a duplicate entry makes `getenv` return
    // whichever comes first).
    for (idx, ((_, v), key)) in overrides.iter().zip(&keys).enumerate() {
        if keys[idx + 1..].contains(key) {
            continue;
        }
        let mut bytes = key.clone();
        bytes.extend_from_slice(&crate::byteenc::decode_bytes(v));
        if let Ok(c) = CString::new(bytes) {
            out.push(c);
        }
    }
    out
}

pub(super) fn build_exec_cstrings_for_path(
    path: &std::path::Path,
    display_name: &str,
    args: &[String],
) -> Result<(CString, Vec<CString>), ExecCStringError> {
    // display_name and args are byteenc-encoded shell strings; decode them
    // so the child receives the original raw bytes (invalid UTF-8 included).
    // Interior NUL is still rejected — it cannot cross the execv boundary.
    let c_path = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| ExecCStringError::CommandName)?;
    let c_argv0 = CString::new(crate::byteenc::decode_bytes(display_name).into_owned())
        .map_err(|_| ExecCStringError::CommandName)?;
    let mut c_args = Vec::with_capacity(args.len() + 1);
    c_args.push(c_argv0);
    for arg in args {
        let c_arg = CString::new(crate::byteenc::decode_bytes(arg).into_owned())
            .map_err(|_| ExecCStringError::Argument(arg.clone()))?;
        c_args.push(c_arg);
    }
    Ok((c_path, c_args))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_envp_last_duplicate_prefix_assignment_wins() {
        let base = vec![
            CString::new("A=base").unwrap(),
            CString::new("AB=keep").unwrap(),
            CString::new("Z=1").unwrap(),
        ];
        let overrides = vec![
            ("A".to_string(), "one".to_string()),
            ("B".to_string(), "b".to_string()),
            ("A".to_string(), "two".to_string()),
        ];
        let got: Vec<String> = merge_envp(&base, &overrides)
            .into_iter()
            .map(|c| c.into_string().unwrap())
            .collect();
        assert_eq!(got, vec!["AB=keep", "Z=1", "B=b", "A=two"]);
    }

    #[test]
    fn build_exec_cstrings_for_path_accepts_valid_utf8_args() {
        let args = vec!["arg".to_string(), "日本".to_string()];
        let path = std::path::Path::new("/bin/echo");
        let (c_path, argv) = build_exec_cstrings_for_path(path, "echo", &args).expect("valid args");

        assert_eq!(c_path.as_bytes(), b"/bin/echo");
        assert_eq!(argv.len(), 3);
        // argv[0] stays the display name, not the resolved path.
        assert_eq!(argv[0].as_bytes(), b"echo");
        assert_eq!(argv[1].as_bytes(), b"arg");
        assert_eq!(argv[2].as_bytes(), "日本".as_bytes());
    }

    #[test]
    fn build_exec_cstrings_for_path_rejects_nul_in_display_name() {
        let args = Vec::new();
        let path = std::path::Path::new("/bin/echo");
        let err = build_exec_cstrings_for_path(path, "bad\0cmd", &args).unwrap_err();
        assert_eq!(err, ExecCStringError::CommandName);
    }

    #[test]
    fn build_exec_cstrings_for_path_rejects_nul_in_argument() {
        let args = vec!["bad\0arg".to_string()];
        let path = std::path::Path::new("/bin/echo");
        let err = build_exec_cstrings_for_path(path, "echo", &args).unwrap_err();
        assert_eq!(err, ExecCStringError::Argument("bad\0arg".to_string()));
    }
}
