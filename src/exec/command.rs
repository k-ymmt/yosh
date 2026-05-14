use std::path::PathBuf;

use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::Pid;

use crate::error::{RuntimeErrorKind, ShellError};

fn is_executable_file(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    if !p.is_file() {
        return false;
    }
    matches!(
        std::fs::metadata(p),
        Ok(meta) if meta.permissions().mode() & 0o111 != 0
    )
}

fn walk_path(cmd: &str, path_var: &str) -> Option<PathBuf> {
    for dir in path_var.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = PathBuf::from(dir).join(cmd);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Search each directory in `path_var` for `cmd`, consulting a cache first.
///
/// If `cmd` contains '/', the cache is bypassed (POSIX: pathnames with
/// '/' are not subject to PATH search). On a cache hit whose path still
/// exists and is executable, the cached path is returned without
/// re-walking PATH. On miss or stale cache entry, falls through to the
/// PATH walk and inserts a fresh entry on success (auto-hash).
pub fn find_in_path(
    cmd: &str,
    path_var: &str,
    cache: &mut std::collections::HashMap<String, std::path::PathBuf>,
) -> Option<PathBuf> {
    if cmd.contains('/') {
        return walk_path(cmd, path_var);
    }
    if let Some(cached) = cache.get(cmd)
        && is_executable_file(cached)
    {
        return Some(cached.clone());
    }
    let found = walk_path(cmd, path_var)?;
    cache.insert(cmd.to_string(), found.clone());
    Some(found)
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
    if !cmd.contains('/')
        && let Some(cached) = cache.get(cmd)
        && is_executable_file(cached)
    {
        return PathLookup::Executable(cached.clone());
    }
    let mut seen_non_exec: Option<PathBuf> = None;
    for dir in path_var.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = PathBuf::from(dir).join(cmd);
        if !candidate.is_file() {
            continue;
        }
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(&candidate) {
            Ok(meta) if meta.permissions().mode() & 0o111 != 0 => {
                if !cmd.contains('/') {
                    cache.insert(cmd.to_string(), candidate.clone());
                }
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
