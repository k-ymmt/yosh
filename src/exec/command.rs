use std::path::PathBuf;

use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::Pid;

use crate::error::{ShellError, RuntimeErrorKind};

/// Search each directory in `path_var` for `cmd`.
/// Returns the full path if found and executable, otherwise None.
pub fn find_in_path(cmd: &str, path_var: &str) -> Option<PathBuf> {
    for dir in path_var.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = PathBuf::from(dir).join(cmd);
        if candidate.is_file() {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&candidate)
                && meta.permissions().mode() & 0o111 != 0
            {
                return Some(candidate);
            }
        }
    }
    None
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
/// POSIX exit status (126 vs 127).
pub fn lookup_in_path(cmd: &str, path_var: &str) -> PathLookup {
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
                return PathLookup::Executable(candidate);
            }
            Ok(_) => {
                // File exists but no exec bit; remember the first such hit.
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
        Err(e) => Err(ShellError::runtime(RuntimeErrorKind::IoError, format!("waitpid: {}", e))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn find_in_path_finds_sh() {
        let path_var = env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".to_string());
        let result = find_in_path("sh", &path_var);
        assert!(result.is_some(), "should find 'sh' in PATH");
    }

    #[test]
    fn find_in_path_returns_none_for_nonexistent() {
        let path_var = "/bin:/usr/bin";
        let result = find_in_path("nonexistent_cmd_12345", path_var);
        assert!(result.is_none());
    }

    #[test]
    fn lookup_in_path_finds_executable() {
        use super::PathLookup;
        let path_var = env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".to_string());
        match lookup_in_path("sh", &path_var) {
            PathLookup::Executable(p) => assert!(p.ends_with("sh")),
            other => panic!("expected Executable, got {:?}", other),
        }
    }

    #[test]
    fn lookup_in_path_reports_not_found_for_missing() {
        use super::PathLookup;
        let path_var = "/bin:/usr/bin";
        match lookup_in_path("definitely_not_a_real_cmd_xyz", path_var) {
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
        match lookup_in_path("cmd_no_exec", path_var) {
            PathLookup::NotExecutable(found) => {
                assert!(found.ends_with("cmd_no_exec"), "got: {}", found.display());
            }
            other => panic!("expected NotExecutable, got {:?}", other),
        }
    }
}
