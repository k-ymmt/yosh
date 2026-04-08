use std::ffi::CString;
use std::path::PathBuf;

use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{execvp, fork, ForkResult, Pid};

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
            if let Ok(meta) = std::fs::metadata(&candidate) {
                if meta.permissions().mode() & 0o111 != 0 {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Fork + execvp the external command.
/// Returns the exit code:
///   - Normal exit: exit status (0-255)
///   - Killed by signal: 128 + signal number
///   - exec failed (not found): 127
///   - exec failed (not executable): 126
///   - fork failed: 1
pub fn exec_external(cmd: &str, args: &[String], env_vars: &[(String, String)]) -> i32 {
    // Build argv CStrings: [cmd, args...]
    let c_cmd = match CString::new(cmd) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("kish: {}: invalid command name", cmd);
            return 127;
        }
    };

    let mut c_args: Vec<CString> = Vec::with_capacity(args.len() + 1);
    c_args.push(c_cmd.clone());
    for a in args {
        match CString::new(a.as_str()) {
            Ok(s) => c_args.push(s),
            Err(_) => {
                eprintln!("kish: {}: invalid argument", a);
                return 1;
            }
        }
    }

    match unsafe { fork() } {
        Err(e) => {
            eprintln!("kish: fork: {}", e);
            1
        }
        Ok(ForkResult::Child) => {
            // Set environment variables
            for (k, v) in env_vars {
                // SAFETY: we are in the child process after fork(), single-threaded
                unsafe { std::env::set_var(k, v) };
            }

            let err = execvp(&c_cmd, &c_args).unwrap_err();
            use nix::errno::Errno;
            let exit_code = match err {
                Errno::ENOENT => {
                    eprintln!("kish: {}: command not found", cmd);
                    127
                }
                Errno::EACCES => {
                    eprintln!("kish: {}: permission denied", cmd);
                    126
                }
                _ => {
                    eprintln!("kish: {}: {}", cmd, err);
                    127
                }
            };
            std::process::exit(exit_code);
        }
        Ok(ForkResult::Parent { child }) => wait_child(child),
    }
}

/// Wait for a child process and return its exit code.
pub fn wait_child(child: Pid) -> i32 {
    match waitpid(child, None) {
        Ok(WaitStatus::Exited(_, code)) => code,
        Ok(WaitStatus::Signaled(_, sig, _)) => 128 + sig as i32,
        Ok(_) => 0,
        Err(e) => {
            eprintln!("kish: waitpid: {}", e);
            1
        }
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
    fn exec_external_true_returns_0() {
        let status = exec_external("true", &[], &[]);
        assert_eq!(status, 0);
    }

    #[test]
    fn exec_external_false_returns_1() {
        let status = exec_external("false", &[], &[]);
        assert_eq!(status, 1);
    }

    #[test]
    fn exec_external_nonexistent_returns_127() {
        let status = exec_external("nonexistent_cmd_12345", &[], &[]);
        assert_eq!(status, 127);
    }
}
