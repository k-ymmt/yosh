//! `yosh:plugin/commands` host import — execute external commands
//! against a per-plugin allowlist of CommandPattern. Granted via
//! CAP_COMMANDS_EXEC.

use super::super::generated::yosh::plugin::commands::ExecOutput;
use super::super::generated::yosh::plugin::types::ErrorCode;
use super::HostContext;

pub fn host_commands_exec(
    ctx: &HostContext,
    program: &str,
    args: &[std::borrow::Cow<'_, str>],
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

    spawn_with_timeout(
        &exec_program,
        &argv[1..],
        std::time::Duration::from_millis(1000),
    )
}

pub fn deny_commands_exec() -> Result<ExecOutput, ErrorCode> {
    Err(ErrorCode::Denied)
}

fn spawn_with_timeout(
    program: &str,
    args: &[&str],
    timeout: std::time::Duration,
) -> Result<ExecOutput, ErrorCode> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
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
    // not deadlock waiting on us. Each thread reads to EOF, which only
    // happens after the child exits or its pipe is closed.
    let mut stdout_pipe = child.stdout.take().expect("piped stdout");
    let mut stderr_pipe = child.stderr.take().expect("piped stderr");
    let (out_tx, out_rx) = mpsc::channel::<std::io::Result<Vec<u8>>>();
    let (err_tx, err_rx) = mpsc::channel::<std::io::Result<Vec<u8>>>();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let r = stdout_pipe.read_to_end(&mut buf).map(|_| buf);
        let _ = out_tx.send(r);
    });
    thread::spawn(move || {
        let mut buf = Vec::new();
        let r = stderr_pipe.read_to_end(&mut buf).map(|_| buf);
        let _ = err_tx.send(r);
    });

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
            // Drain pipes: the child is dead (SIGKILL + wait), so the
            // pipe fds are closed and the reader threads will EOF and
            // terminate. Blocking recv() is safe here — it cannot hang.
            let _ = out_rx.recv();
            let _ = err_rx.recv();
            return Err(ErrorCode::Timeout);
        }
        thread::sleep(std::time::Duration::from_millis(10));
    };

    // The child has exited (try_wait returned Some(_)), so the pipe fds
    // are closed and the reader threads are guaranteed to terminate.
    // Blocking recv() is safe — it cannot hang.
    let stdout = out_rx.recv().ok().and_then(|r| r.ok()).unwrap_or_default();
    let stderr = err_rx.recv().ok().and_then(|r| r.ok()).unwrap_or_default();

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
        let result = host_commands_exec(&ctx, "yosh-exec-probe", &[])
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
