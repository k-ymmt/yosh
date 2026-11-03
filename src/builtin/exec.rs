//! `exec` special builtin: replace the shell with a utility (POSIX XCU
//! §2.15 `exec`). Redirection-only `exec` (no operands) is handled by
//! the executor.

use std::ffi::CString;

use crate::builtin::path::os_path;
use crate::error::ShellError;
use crate::exec::Executor;

pub fn builtin_exec(args: &[String], executor: &mut Executor) -> Result<i32, ShellError> {
    if args.is_empty() {
        return Ok(0);
    }

    // POSIX §2.14 exec: when the utility cannot be executed, a
    // NON-interactive shell exits — 127 if not found, 126 if found but
    // not executable (matches bash/dash errno semantics). An interactive
    // shell prints the diagnostic and continues.
    fn exec_failure(executor: &mut Executor, msg: String, code: i32) -> Result<i32, ShellError> {
        eprintln!("yosh: {}", msg);
        executor.env.exec.last_exit_status = code;
        if !executor.env.mode.is_interactive {
            executor.exit_shell(code);
        }
        Ok(code)
    }

    let cmd = args[0].clone();

    // Resolve the executable path. If the command contains `/`, treat as
    // a relative or absolute path (byteenc-decoded so non-UTF-8 paths
    // reach the OS as their original bytes). Otherwise walk $PATH.
    let resolved_path: std::path::PathBuf = if cmd.contains('/') {
        os_path(&cmd)
    } else {
        let env = &mut executor.env;
        let path_var = env
            .vars
            .get("PATH")
            .map(|s| s.to_string())
            .unwrap_or_default();
        // lookup_in_path (not find_in_path): the 126/127 distinction
        // matters here — a matching regular file that is not executable
        // is "permission denied" / 126, exactly as the execve EACCES
        // branch below reports it (bash/dash agree for
        // `PATH=. exec Cargo.toml`).
        use crate::exec::command::PathLookup;
        match crate::exec::command::lookup_in_path(&cmd, &path_var, &mut env.utility_hash) {
            PathLookup::Executable(p) => p,
            PathLookup::NotExecutable(_) => {
                return exec_failure(executor, format!("exec: {}: permission denied", cmd), 126);
            }
            PathLookup::NotFound => {
                return exec_failure(executor, format!("exec: {}: not found", cmd), 127);
            }
        }
    };

    let c_path = match CString::new(resolved_path.as_os_str().as_encoded_bytes()) {
        Ok(s) => s,
        Err(_) => {
            return exec_failure(executor, format!("exec: {}: invalid path", cmd), 126);
        }
    };

    // argv crosses execve as raw bytes: decode byteenc escapes (interior
    // NUL is still rejected — it cannot cross the boundary).
    let mut c_args: Vec<CString> = Vec::with_capacity(args.len());
    for a in args {
        match CString::new(crate::byteenc::decode_bytes(a.as_str()).into_owned()) {
            Ok(s) => c_args.push(s),
            Err(_) => {
                return exec_failure(executor, format!("exec: {}: invalid argument", a), 126);
            }
        }
    }

    // Build envp from currently-exported variables (byteenc-decoded).
    let envp: Vec<CString> = executor
        .env
        .vars
        .environ()
        .iter()
        .filter_map(|(k, v)| {
            let mut bytes = crate::byteenc::decode_bytes(k).into_owned();
            bytes.push(b'=');
            bytes.extend_from_slice(&crate::byteenc::decode_bytes(v));
            CString::new(bytes).ok()
        })
        .collect();

    let err = nix::unistd::execve(&c_path, &c_args, &envp).unwrap_err();
    use nix::errno::Errno;
    match err {
        Errno::ENOENT => exec_failure(executor, format!("exec: {}: not found", cmd), 127),
        Errno::EACCES => exec_failure(executor, format!("exec: {}: permission denied", cmd), 126),
        Errno::ENOEXEC => {
            // POSIX execvp fallback (mirrors exec_external_with_redirects):
            // an executable file the kernel refuses to exec is re-run as a
            // shell script via /bin/sh, passing the resolved file path.
            let sh = CString::new("/bin/sh").expect("/bin/sh has no NUL");
            let mut sh_args = Vec::with_capacity(c_args.len() + 1);
            sh_args.push(sh.clone());
            sh_args.push(c_path.clone());
            sh_args.extend_from_slice(&c_args[1..]);
            let err2 = nix::unistd::execve(&sh, &sh_args, &envp).unwrap_err();
            exec_failure(
                executor,
                format!("exec: {}: {}", cmd, crate::exec::redirect::errno_text(err2)),
                126,
            )
        }
        _ => exec_failure(
            executor,
            format!("exec: {}: {}", cmd, crate::exec::redirect::errno_text(err)),
            126,
        ),
    }
}
