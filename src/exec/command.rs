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
}
