//! `yosh:plugin/commands` host import — execute external commands
//! against a per-plugin allowlist of CommandPattern. Granted via
//! CAP_COMMANDS_EXEC.

use super::super::generated::yosh::plugin::commands::ExecOutput;
use super::super::generated::yosh::plugin::types::ErrorCode;
use super::HostContext;

/// Per-exec wall-clock budget for plugin-initiated commands (spec §5).
const EXEC_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1000);

pub fn host_commands_exec(
    ctx: &HostContext,
    program: &str,
    args: &[std::borrow::Cow<'_, str>],
) -> Result<ExecOutput, ErrorCode> {
    host_commands_exec_with_timeout(ctx, program, args, EXEC_TIMEOUT)
}

/// [`host_commands_exec`] with an explicit timeout. Split out so tests
/// that assert on *semantics* (allowlisting, PATH resolution, stream
/// capture) can use a budget generous enough to survive parallel-suite
/// load — the 1s production budget has its own dedicated timing tests
/// and is otherwise an unrelated failure mode for those tests.
fn host_commands_exec_with_timeout(
    ctx: &HostContext,
    program: &str,
    args: &[std::borrow::Cow<'_, str>],
    timeout: std::time::Duration,
) -> Result<ExecOutput, ErrorCode> {
    // The metadata-contract guard runs first. CWD and environment
    // inheritance happen implicitly via std::process::Command::new
    // defaults (spec §5: "CWD is the shell's current directory;
    // environment is the shell's full environment") — `ctx` is read
    // here only for `allowed_commands`, not for ShellEnv state.
    ctx.ensure_bound()?;
    if program.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }

    // argv = [program, args...]; pattern matcher consumes &str slices
    // (no PATH resolution, no basename normalization — see spec §5).
    // One Vec<&str> allocation, reused for both the matcher and spawn.
    let argv: Vec<&str> = std::iter::once(program)
        .chain(args.iter().map(|c| c.as_ref()))
        .collect();

    if !ctx.allowed_commands.iter().any(|p| p.matches(&argv)) {
        return Err(ErrorCode::PatternNotAllowed);
    }

    // Resolve relative names against the *shell's* $PATH (ShellEnv) in
    // the parent, matching how the shell executes a command typed at
    // the prompt. Without this, Command::new would PATH-walk the
    // process-inherited environment, which shell-level PATH assignments
    // never update — so the allowlist would match one name while exec
    // resolved it through a different, stale PATH.
    let exec_program: String = if program.contains('/') {
        program.to_string()
    } else {
        let found = ctx.bound_env_with(|env| {
            let path_var = env
                .vars
                .get("PATH")
                .map(|s| s.to_string())
                .unwrap_or_default();
            crate::exec::command::find_in_path(program, &path_var, &mut env.utility_hash)
        })?;
        match found {
            Some(p) => p.to_string_lossy().into_owned(),
            None => return Err(ErrorCode::NotFound),
        }
    };

    spawn_with_timeout(&exec_program, &argv[1..], timeout)
}

pub fn deny_commands_exec() -> Result<ExecOutput, ErrorCode> {
    Err(ErrorCode::Denied)
}

/// Spawn a reader thread that drains `pipe` incrementally into a shared
/// buffer, signalling EOF (or read error) on the returned channel. The
/// buffer is shared so the caller can take whatever has been captured
/// even when EOF never arrives — a grandchild that inherited the pipe
/// write end keeps it open past the direct child's exit.
fn spawn_pipe_reader(
    mut pipe: impl std::io::Read + Send + 'static,
) -> (
    std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    std::sync::mpsc::Receiver<()>,
) {
    let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let shared = buf.clone();
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => shared
                    .lock()
                    .expect("pipe buffer mutex")
                    .extend_from_slice(&chunk[..n]),
            }
        }
        let _ = tx.send(());
    });
    (buf, rx)
}

/// Wait for the reader's EOF signal until `until`, then take whatever
/// bytes were captured. Never blocks past `until` — the reader thread
/// is left to finish (or stay blocked on a grandchild-held pipe) on
/// its own; it only holds an Arc to the buffer.
fn take_captured(
    buf: &std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    done: &std::sync::mpsc::Receiver<()>,
    until: std::time::Instant,
) -> Vec<u8> {
    let wait = until.saturating_duration_since(std::time::Instant::now());
    let _ = done.recv_timeout(wait);
    std::mem::take(&mut *buf.lock().expect("pipe buffer mutex"))
}

fn spawn_with_timeout(
    program: &str,
    args: &[&str],
    timeout: std::time::Duration,
) -> Result<ExecOutput, ErrorCode> {
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::Instant;

    let mut child = match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(ErrorCode::NotFound),
        Err(_) => return Err(ErrorCode::IoFailed),
    };

    // Drain stdout and stderr concurrently so a buffer-full child does
    // not deadlock waiting on us. The reader threads normally hit EOF
    // right after the child exits, but a grandchild that inherited the
    // write ends keeps the pipe open — so every wait below is bounded
    // by the deadline instead of blocking on EOF.
    let stdout_pipe = child.stdout.take().expect("piped stdout");
    let stderr_pipe = child.stderr.take().expect("piped stderr");
    let (out_buf, out_done) = spawn_pipe_reader(stdout_pipe);
    let (err_buf, err_done) = spawn_pipe_reader(stderr_pipe);

    let deadline = Instant::now() + timeout;
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(_) => return Err(ErrorCode::IoFailed),
        }
        if Instant::now() >= deadline {
            // Timeout: SIGTERM, 100ms grace, then SIGKILL.
            let pid = nix::unistd::Pid::from_raw(child.id() as i32);
            let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM);
            let grace = Instant::now() + std::time::Duration::from_millis(100);
            loop {
                if let Ok(Some(_)) = child.try_wait() {
                    break;
                }
                if Instant::now() >= grace {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                thread::sleep(std::time::Duration::from_millis(10));
            }
            // Bounded drain: the killed child's pipes usually EOF at
            // once, but a surviving grandchild can hold them open.
            let drain = Instant::now() + std::time::Duration::from_millis(100);
            let _ = take_captured(&out_buf, &out_done, drain);
            let _ = take_captured(&err_buf, &err_done, drain);
            return Err(ErrorCode::Timeout);
        }
        thread::sleep(std::time::Duration::from_millis(10));
    };

    // The child has exited. Wait for EOF up to the remaining exec
    // budget (with a small floor if the child used it all), then take
    // whatever was captured — never block indefinitely on a
    // grandchild-held pipe.
    let floor = Instant::now() + std::time::Duration::from_millis(100);
    let drain = deadline.max(floor);
    let stdout = take_captured(&out_buf, &out_done, drain);
    let stderr = take_captured(&err_buf, &err_done, drain);

    Ok(ExecOutput {
        exit_code: exit_status.code().unwrap_or(-1),
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    //! Metadata-contract spot test plus the eight spec-§10 behavioral
    //! tests for commands:exec.

    use super::super::test_helpers::{bound_env_ctx, ctx_with_allowed, null_env_ctx};
    use super::*;
    use crate::env::ShellEnv;
    use std::borrow::Cow;

    #[test]
    fn commands_exec_denied_when_env_null() {
        let ctx = null_env_ctx();
        let result = host_commands_exec(&ctx, "/bin/echo", &[Cow::Borrowed("hi")]);
        assert!(matches!(result, Err(ErrorCode::Denied)));
    }

    #[test]
    fn host_commands_exec_invalid_argument_on_empty_program() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = bound_env_ctx(&mut env);
        let result = host_commands_exec(&ctx, "", &[]);
        assert!(matches!(result, Err(ErrorCode::InvalidArgument)));
    }

    #[test]
    fn host_commands_exec_pattern_not_allowed_when_no_match() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_allowed(&mut env, &["ls:*"]);
        let result = host_commands_exec(&ctx, "echo", &[Cow::Borrowed("hi")]);
        assert!(matches!(result, Err(ErrorCode::PatternNotAllowed)));
    }

    #[test]
    fn host_commands_exec_runs_when_pattern_matches() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_allowed(&mut env, &["/bin/echo:*"]);
        let result = host_commands_exec(&ctx, "/bin/echo", &[Cow::Borrowed("hello")])
            .expect("echo should succeed");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, b"hello\n");
        assert!(result.stderr.is_empty());
    }

    #[test]
    fn host_commands_exec_captures_stderr_separately() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_allowed(&mut env, &["/bin/sh:*"]);
        let result = host_commands_exec(
            &ctx,
            "/bin/sh",
            &[
                Cow::Borrowed("-c"),
                Cow::Borrowed("echo out; echo err 1>&2"),
            ],
        )
        .expect("sh should succeed");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, b"out\n");
        // `ends_with` rather than `==` guards against a sibling test
        // dangling the process cwd (sh would prepend a "shell-init"
        // line to stderr). The known offender was fixed, but the
        // looser check costs nothing and prevents recurrences.
        assert!(
            result.stderr.ends_with(b"err\n"),
            "stderr should end with the captured `err\\n` line, got {:?}",
            String::from_utf8_lossy(&result.stderr),
        );
    }

    #[test]
    fn host_commands_exec_propagates_nonzero_exit() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_allowed(&mut env, &["/bin/sh:*"]);
        let result = host_commands_exec(
            &ctx,
            "/bin/sh",
            &[Cow::Borrowed("-c"), Cow::Borrowed("exit 42")],
        )
        .expect("sh should run to exit");
        assert_eq!(result.exit_code, 42);
    }

    #[test]
    fn host_commands_exec_resolves_relative_program_via_shell_path() {
        // Relative program names must resolve against the *shell's*
        // $PATH (ShellEnv), not the process-inherited environment —
        // yosh never writes shell PATH assignments back to the process
        // env, so the two can diverge.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("yosh-exec-probe");
        std::fs::write(&bin, "#!/bin/sh\necho probe-ok\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars
            .set("PATH", dir.path().to_string_lossy().into_owned())
            .unwrap();
        let ctx = ctx_with_allowed(&mut env, &["yosh-exec-probe:*"]);
        // Generous budget: this test asserts PATH-resolution semantics,
        // not the 1s production budget (which has its own timing tests).
        // Under full parallel-suite load, /bin/sh spawn latency alone
        // exceeded 1s deterministically (4/4 release runs, 2026-07-11).
        let result = host_commands_exec_with_timeout(
            &ctx,
            "yosh-exec-probe",
            &[],
            std::time::Duration::from_secs(10),
        )
        .expect("relative name should resolve via the shell's PATH");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, b"probe-ok\n");
    }

    #[test]
    fn host_commands_exec_relative_not_in_shell_path_is_not_found() {
        // A relative name absent from the shell's $PATH must fail with
        // NotFound even if some process-env PATH would have found it.
        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars.set("PATH", "/nonexistent-dir-yosh").unwrap();
        let ctx = ctx_with_allowed(&mut env, &["echo:*"]);
        let result = host_commands_exec(&ctx, "echo", &[Cow::Borrowed("hi")]);
        assert!(matches!(result, Err(ErrorCode::NotFound)));
    }

    #[test]
    fn host_commands_exec_returns_not_found_for_missing_binary() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_allowed(&mut env, &["/no/such/binary-xyz:*"]);
        let result = host_commands_exec(&ctx, "/no/such/binary-xyz", &[]);
        assert!(matches!(result, Err(ErrorCode::NotFound)));
    }

    /// Regression: a grandchild that inherits the stdout/stderr pipe
    /// write ends (`sh -c 'daemon & exit 0'` pattern) must not hang the
    /// call — the reader threads never see EOF while the grandchild
    /// lives, so the final drain has to be deadline-bounded rather
    /// than a blocking recv().
    #[test]
    fn host_commands_exec_returns_when_grandchild_keeps_pipe_open() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_allowed(&mut env, &["/bin/sh:*"]);
        let start = std::time::Instant::now();
        let result = host_commands_exec(
            &ctx,
            "/bin/sh",
            &[
                Cow::Borrowed("-c"),
                Cow::Borrowed("echo out; sleep 5 & exit 7"),
            ],
        );
        let elapsed = start.elapsed();
        // The child exits immediately; only the 5s grandchild holds the
        // pipes. The call must come back within the ~1s exec budget
        // (plus slack), not after the grandchild dies.
        assert!(
            elapsed < std::time::Duration::from_millis(3000),
            "call blocked on grandchild-held pipe for {:?}",
            elapsed
        );
        let out = result.expect("child exited normally");
        assert_eq!(out.exit_code, 7);
        assert_eq!(out.stdout, b"out\n", "output written before exit is kept");
    }

    #[test]
    fn host_commands_exec_timeout_after_1000ms() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_allowed(&mut env, &["/bin/sleep:*"]);
        let start = std::time::Instant::now();
        let result = host_commands_exec(&ctx, "/bin/sleep", &[Cow::Borrowed("5")]);
        let elapsed = start.elapsed();
        assert!(matches!(result, Err(ErrorCode::Timeout)));
        // Hard cap is 1000ms + 100ms grace + a generous slack for thread
        // scheduling. Anything past 2 seconds means the timeout enforcement
        // is broken, not just slow.
        assert!(
            elapsed < std::time::Duration::from_millis(2000),
            "timeout took {:?}, expected <2000ms",
            elapsed
        );
    }

    #[test]
    fn host_commands_exec_kills_child_on_timeout() {
        // Spec §10: after a timeout-triggered call returns, the child must
        // be reaped (no zombie). spawn_with_timeout calls `child.wait()`
        // after SIGKILL, so a successful return implies the child PID has
        // been reaped. The test verifies (a) the function returns within
        // a bounded window — meaning child.wait() did NOT block forever
        // waiting on a still-running child — and (b) the elapsed time
        // covers SIGTERM + 100ms grace + SIGKILL + reaping. If any step
        // were broken, this assertion would fail with either a hang or
        // a too-fast / too-slow elapsed time.
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_allowed(&mut env, &["/bin/sleep:*"]);
        let start = std::time::Instant::now();
        let result = host_commands_exec(&ctx, "/bin/sleep", &[Cow::Borrowed("5")]);
        let elapsed = start.elapsed();
        assert!(matches!(result, Err(ErrorCode::Timeout)));
        // Lower bound: SIGTERM only fires after the 1000ms deadline.
        assert!(
            elapsed >= std::time::Duration::from_millis(900),
            "elapsed {:?} too small — timeout fired before deadline",
            elapsed
        );
        // Upper bound: deadline + grace + reasonable scheduling slack.
        // If child.wait() blocked indefinitely waiting on an unkilled
        // child, this would hang past 2000ms.
        assert!(
            elapsed < std::time::Duration::from_millis(2000),
            "elapsed {:?} too large — child may not have been reaped",
            elapsed
        );
    }
}
