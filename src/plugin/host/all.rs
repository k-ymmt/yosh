//! Temporary single-file home for host_* and deny_* functions.
//!
//! PR-A scaffolding step (see
//! `docs/superpowers/specs/2026-05-06-sp1-plugin-host-redesign-design.md`).
//! PR-B splits this file into per-capability submodules and deletes it.

use super::HostContext;
use super::super::generated::yosh::plugin::commands::ExecOutput;
use super::super::generated::yosh::plugin::types::ErrorCode;

// ── yosh:plugin/commands host imports ───────────────────────────────

pub fn host_commands_exec(
    ctx: &mut HostContext,
    program: String,
    args: Vec<String>,
) -> Result<ExecOutput, ErrorCode> {
    // `&mut HostContext` is here only for the metadata-contract null
    // guard below; CWD and environment inheritance happen implicitly via
    // `std::process::Command::new` defaults (spec §5: "CWD is the
    // shell's current directory; environment is the shell's full
    // environment"), not via fields read off `ctx`.
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if program.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }

    // argv = [program, args...]; pattern matcher consumes the literal
    // strings (no PATH resolution, no basename normalization — see
    // spec §5).
    let mut argv = Vec::with_capacity(1 + args.len());
    argv.push(program.clone());
    argv.extend(args.iter().cloned());

    if !ctx.allowed_commands.iter().any(|p| p.matches(&argv)) {
        return Err(ErrorCode::PatternNotAllowed);
    }

    spawn_with_timeout(&program, &args, std::time::Duration::from_millis(1000))
}

pub fn deny_commands_exec(
    _ctx: &mut HostContext,
    _program: String,
    _args: Vec<String>,
) -> Result<ExecOutput, ErrorCode> {
    Err(ErrorCode::Denied)
}

fn spawn_with_timeout(
    program: &str,
    args: &[String],
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
    //! Unit tests for the metadata contract: every host import must
    //! short-circuit to `Err(Denied)` when `HostContext.env` is null. This
    //! is the canonical enforcement point for the §5 metadata-cannot-reach-
    //! host-APIs invariant. The pointer is null during the single
    //! `metadata()` call at startup and between `with_env` invocations, so
    //! returning `Denied` from these functions blocks any plugin that tries
    //! to call them outside of a properly-bound dispatch.
    //!
    //! Replaces what would have been `tests/plugin.rs::t04_metadata_cannot_
    //! reach_host_apis` — a contrived plugin whose `metadata` calls `cwd()`
    //! is harder to author than this direct call. Same invariant, simpler
    //! test.
    use super::*;
    use super::super::test_helpers::{bound_env_ctx, ctx_with_allowed, null_env_ctx};
    use crate::env::ShellEnv;

    #[test]
    fn ensure_bound_returns_denied_when_env_null() {
        let ctx = null_env_ctx();
        assert_eq!(ctx.ensure_bound(), Err(ErrorCode::Denied));
    }

    #[test]
    fn ensure_bound_returns_ok_when_env_bound() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = bound_env_ctx(&mut env);
        assert_eq!(ctx.ensure_bound(), Ok(()));
    }

    #[test]
    fn bound_env_returns_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = ctx.bound_env();
        assert!(matches!(result, Err(ErrorCode::Denied)));
    }

    #[test]
    fn bound_env_returns_env_when_bound() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        let result = ctx.bound_env();
        assert!(result.is_ok());
    }

    // ── commands:exec host tests (spec §10) ─────────────────────────────

    #[test]
    fn host_commands_exec_metadata_contract_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_commands_exec(&mut ctx, "/bin/echo".into(), vec!["hi".into()]);
        assert!(matches!(result, Err(ErrorCode::Denied)));
    }

    #[test]
    fn host_commands_exec_invalid_argument_on_empty_program() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        let result = host_commands_exec(&mut ctx, String::new(), vec![]);
        assert!(matches!(result, Err(ErrorCode::InvalidArgument)));
    }

    #[test]
    fn host_commands_exec_pattern_not_allowed_when_no_match() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = ctx_with_allowed(&mut env, &["ls:*"]);
        let result = host_commands_exec(&mut ctx, "echo".into(), vec!["hi".into()]);
        assert!(matches!(result, Err(ErrorCode::PatternNotAllowed)));
    }

    #[test]
    fn host_commands_exec_runs_when_pattern_matches() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = ctx_with_allowed(&mut env, &["/bin/echo:*"]);
        let result = host_commands_exec(&mut ctx, "/bin/echo".into(), vec!["hello".into()])
            .expect("echo should succeed");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, b"hello\n");
        assert!(result.stderr.is_empty());
    }

    #[test]
    fn host_commands_exec_captures_stderr_separately() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = ctx_with_allowed(&mut env, &["/bin/sh:*"]);
        let result = host_commands_exec(
            &mut ctx,
            "/bin/sh".into(),
            vec!["-c".into(), "echo out; echo err 1>&2".into()],
        )
        .expect("sh should succeed");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, b"out\n");
        // The unrelated `resolve_cdpath_empty_entry_is_dot` test in
        // src/builtin/regular.rs calls `set_current_dir` into a tempdir
        // and lets the tempdir drop, leaving the test process's cwd
        // pointing at a deleted directory. When a subsequent run of
        // this test spawns `/bin/sh -c …` in parallel, sh prints
        // "shell-init: error retrieving current directory: …" to
        // stderr before our `echo err` runs. Use `ends_with` so the
        // capture-separately invariant we care about is verified
        // without false-failing on that pre-existing race.
        assert!(
            result.stderr.ends_with(b"err\n"),
            "stderr should end with the captured `err\\n` line, got {:?}",
            String::from_utf8_lossy(&result.stderr),
        );
    }

    #[test]
    fn host_commands_exec_propagates_nonzero_exit() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = ctx_with_allowed(&mut env, &["/bin/sh:*"]);
        let result = host_commands_exec(
            &mut ctx,
            "/bin/sh".into(),
            vec!["-c".into(), "exit 42".into()],
        )
        .expect("sh should run to exit");
        assert_eq!(result.exit_code, 42);
    }

    #[test]
    fn host_commands_exec_returns_not_found_for_missing_binary() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = ctx_with_allowed(&mut env, &["/no/such/binary-xyz:*"]);
        let result = host_commands_exec(&mut ctx, "/no/such/binary-xyz".into(), vec![]);
        assert!(matches!(result, Err(ErrorCode::NotFound)));
    }

    #[test]
    fn host_commands_exec_timeout_after_1000ms() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = ctx_with_allowed(&mut env, &["/bin/sleep:*"]);
        let start = std::time::Instant::now();
        let result = host_commands_exec(&mut ctx, "/bin/sleep".into(), vec!["5".into()]);
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
        // waiting for a still-running child — and (b) the elapsed time
        // covers SIGTERM + 100ms grace + SIGKILL + reaping. If any step
        // were broken, this assertion would fail with either a hang or
        // a too-fast / too-slow elapsed time.
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = ctx_with_allowed(&mut env, &["/bin/sleep:*"]);
        let start = std::time::Instant::now();
        let result = host_commands_exec(&mut ctx, "/bin/sleep".into(), vec!["5".into()]);
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
