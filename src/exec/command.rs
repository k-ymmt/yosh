use std::path::PathBuf;

use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::Pid;

use crate::error::{RuntimeErrorKind, ShellError};

pub(crate) fn is_executable_file(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    if !p.is_file() {
        return false;
    }
    matches!(
        std::fs::metadata(p),
        Ok(meta) if meta.permissions().mode() & 0o111 != 0
    )
}

/// Resolve one `:`-separated `$PATH` component to a search directory.
/// POSIX XBD §8.3: a zero-length prefix (leading `:`, trailing `:`, or
/// `::` between two colons) is a legacy synonym for the current working
/// directory.
fn path_component_dir(dir: &str) -> &str {
    if dir.is_empty() { "." } else { dir }
}

/// Search each directory in `path_var` for `cmd`, consulting a cache first.
///
/// Thin wrapper over [`lookup_in_path`] for exec-only callers that do not
/// need the 126/127 (not executable / not found) distinction: both
/// non-`Executable` outcomes collapse to `None`.
pub fn find_in_path(
    cmd: &str,
    path_var: &str,
    cache: &mut std::collections::HashMap<String, std::path::PathBuf>,
) -> Option<PathBuf> {
    match lookup_in_path(cmd, path_var, cache) {
        PathLookup::Executable(p) => Some(p),
        PathLookup::NotExecutable(_) | PathLookup::NotFound => None,
    }
}

/// Result of looking up a command name in `$PATH`.
#[derive(Debug)]
pub enum PathLookup {
    /// Found an executable file at this absolute path.
    Executable(PathBuf),
    /// Found a regular file, but it is not executable.
    NotExecutable(PathBuf),
    /// No matching file in any PATH entry.
    NotFound,
}

/// Walk each directory in `path_var`, without touching any cache, and
/// report whether `cmd` exists and is executable.
fn walk_path_lookup(cmd: &str, path_var: &str) -> PathLookup {
    use std::os::unix::ffi::OsStrExt;
    let mut seen_non_exec: Option<PathBuf> = None;
    // Decode byteenc-escaped bytes so non-UTF-8 PATH entries / command
    // names resolve against the real on-disk names.
    let cmd_bytes = crate::byteenc::decode_bytes(cmd);
    let cmd_os = std::ffi::OsStr::from_bytes(&cmd_bytes);
    for dir in path_var.split(':') {
        let dir_bytes = crate::byteenc::decode_bytes(path_component_dir(dir));
        let candidate = PathBuf::from(std::ffi::OsStr::from_bytes(&dir_bytes)).join(cmd_os);
        if !candidate.is_file() {
            continue;
        }
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(&candidate) {
            Ok(meta) if meta.permissions().mode() & 0o111 != 0 => {
                return PathLookup::Executable(candidate);
            }
            Ok(_) => {
                if seen_non_exec.is_none() {
                    seen_non_exec = Some(candidate);
                }
            }
            Err(_) => continue,
        }
    }
    match seen_non_exec {
        Some(p) => PathLookup::NotExecutable(p),
        None => PathLookup::NotFound,
    }
}

/// Walk each directory in `path_var` and report whether `cmd` exists and
/// is executable. Unlike [`find_in_path`], this distinguishes the
/// "exists but not executable" case so callers can return the correct
/// POSIX exit status (126 vs 127). Cache is consulted only for the
/// `Executable` case; non-executable hits are not cached.
pub fn lookup_in_path(
    cmd: &str,
    path_var: &str,
    cache: &mut std::collections::HashMap<String, std::path::PathBuf>,
) -> PathLookup {
    // POSIX: pathnames containing '/' are not subject to PATH search, so
    // they bypass the cache entirely (never read, never inserted).
    let use_cache = !cmd.contains('/');
    if use_cache
        && let Some(cached) = cache.get(cmd)
        && is_executable_file(cached)
    {
        return PathLookup::Executable(cached.clone());
    }
    let result = walk_path_lookup(cmd, path_var);
    if use_cache && let PathLookup::Executable(p) = &result {
        cache.insert(cmd.to_string(), p.clone());
    }
    result
}

/// Like [`lookup_in_path`], but never reads or writes the `utility_hash`
/// cache. For use when `path_var` is a one-off override (e.g. a command's
/// own `PATH=dir cmd` prefix assignment) rather than the shell's own
/// `$PATH` that the cache is keyed against — a cache hit here could
/// return a path found under a *different* PATH, and a cache insert here
/// would poison lookups for the shell's own (uncorrelated) `$PATH`.
pub fn lookup_in_path_uncached(cmd: &str, path_var: &str) -> PathLookup {
    walk_path_lookup(cmd, path_var)
}

/// Wait for a child process and return its exit code.
pub fn wait_child(child: Pid) -> Result<i32, ShellError> {
    match waitpid(child, None) {
        Ok(WaitStatus::Exited(_, code)) => Ok(code),
        Ok(WaitStatus::Signaled(_, sig, _)) => Ok(128 + sig as i32),
        Ok(_) => Ok(0),
        Err(e) => Err(ShellError::runtime(
            RuntimeErrorKind::IoError,
            format!("waitpid: {}", e),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::env;

    #[test]
    fn find_in_path_finds_sh() {
        let path_var = env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".to_string());
        let mut cache = HashMap::new();
        let result = find_in_path("sh", &path_var, &mut cache);
        assert!(result.is_some(), "should find 'sh' in PATH");
    }

    #[test]
    fn find_in_path_returns_none_for_nonexistent() {
        let path_var = "/bin:/usr/bin";
        let mut cache = HashMap::new();
        let result = find_in_path("nonexistent_cmd_12345", path_var, &mut cache);
        assert!(result.is_none());
    }

    /// POSIX XBD §8.3: a zero-length PATH prefix (leading `:`, trailing
    /// `:`, or `::`) means "the current working directory." Verified via
    /// the pure `path_component_dir` helper rather than an actual `chdir`
    /// (which would race with other tests running in parallel threads).
    #[test]
    fn path_component_dir_empty_means_dot() {
        assert_eq!(path_component_dir(""), ".");
        assert_eq!(path_component_dir("/usr/bin"), "/usr/bin");
    }

    #[test]
    fn lookup_in_path_empty_component_means_cwd() {
        // lookup_in_path never chdirs; this proves the empty PATH
        // component is actually searched (not skipped) by using a
        // relative name that resolves under "." from the crate root
        // (wherever `cargo test` runs from) — Cargo.toml exists there
        // but is not executable, so PathLookup::NotExecutable proves the
        // empty component's directory (".") was checked, whereas the
        // pre-fix behavior (skipping empty components entirely) would
        // have produced NotFound instead.
        let mut cache = HashMap::new();
        match lookup_in_path("Cargo.toml", ":/nonexistent_dir_xyz", &mut cache) {
            PathLookup::NotExecutable(_) => {}
            other => panic!(
                "expected NotExecutable (proves empty component searched cwd), got {:?}",
                other
            ),
        }
    }

    #[test]
    fn lookup_in_path_finds_executable() {
        use super::PathLookup;
        let path_var = env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".to_string());
        let mut cache = HashMap::new();
        match lookup_in_path("sh", &path_var, &mut cache) {
            PathLookup::Executable(p) => assert!(p.ends_with("sh")),
            other => panic!("expected Executable, got {:?}", other),
        }
    }

    #[test]
    fn lookup_in_path_reports_not_found_for_missing() {
        use super::PathLookup;
        let path_var = "/bin:/usr/bin";
        let mut cache = HashMap::new();
        match lookup_in_path("definitely_not_a_real_cmd_xyz", path_var, &mut cache) {
            PathLookup::NotFound => {}
            other => panic!("expected NotFound, got {:?}", other),
        }
    }

    #[test]
    fn lookup_in_path_reports_not_executable() {
        use super::PathLookup;
        use std::io::Write;
        // Create a regular file without the exec bit in a fresh temp dir.
        let tmp = tempfile::tempdir().expect("tempdir");
        let p = tmp.path().join("cmd_no_exec");
        let mut f = std::fs::File::create(&p).expect("create file");
        f.write_all(b"#!/bin/sh\n").expect("write file");
        drop(f);
        // Explicitly strip exec bits just in case.
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&p).unwrap().permissions();
        perms.set_mode(0o644);
        std::fs::set_permissions(&p, perms).unwrap();

        let path_var = tmp.path().to_str().unwrap();
        let mut cache = HashMap::new();
        match lookup_in_path("cmd_no_exec", path_var, &mut cache) {
            PathLookup::NotExecutable(found) => {
                assert!(found.ends_with("cmd_no_exec"), "got: {}", found.display());
            }
            other => panic!("expected NotExecutable, got {:?}", other),
        }
    }

    #[test]
    fn find_in_path_cache_hit_returns_cached_path() {
        let path_var = env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".to_string());
        let mut cache = HashMap::new();
        let canonical_sh = find_in_path("sh", &path_var, &mut cache).unwrap();
        // Cache should now contain "sh".
        assert_eq!(cache.get("sh"), Some(&canonical_sh));

        // Subsequent call must return the same path.
        let again = find_in_path("sh", &path_var, &mut cache).unwrap();
        assert_eq!(again, canonical_sh);
    }

    #[test]
    fn find_in_path_skips_cache_for_slash_paths() {
        let mut cache = HashMap::new();
        // /bin/sh exists on macOS and Linux; the slash form bypasses cache.
        let _ = find_in_path("/bin/sh", "/bin:/usr/bin", &mut cache);
        assert!(cache.is_empty());
    }

    #[test]
    fn find_in_path_falls_back_when_cached_file_missing() {
        use std::path::PathBuf;
        let mut cache = HashMap::new();
        cache.insert(
            "sh".to_string(),
            PathBuf::from("/nonexistent/fake_sh_12345"),
        );
        let path_var = env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".to_string());
        let result = find_in_path("sh", &path_var, &mut cache);
        // Must fall through to PATH walk and find real sh.
        assert!(result.is_some());
        let p = result.unwrap();
        assert!(p.exists());
        // Cache should be refreshed to the real path.
        assert_eq!(cache.get("sh"), Some(&p));
    }
}
