//! In-memory `yosh:plugin/commands` host import — spawns real
//! subprocesses, gated by CAP_COMMANDS_EXEC and an allowlist of
//! `CommandPattern` (reused from yosh-plugin-api).
//!
//! Spawn / timeout logic duplicates `src/plugin/host/commands.rs::spawn_with_timeout`
//! intentionally; consolidation onto a shared helper is tracked as a
//! TODO (spec §11).

use std::time::Duration;

use super::{ExecRecord, TestState};
use crate::generated::yosh::plugin::commands::ExecOutput;
use crate::generated::yosh::plugin::types::ErrorCode;
use yosh_plugin_api::CAP_COMMANDS_EXEC;

pub fn host_exec(
    state: &mut TestState,
    program: &str,
    args: &[String],
) -> Result<ExecOutput, ErrorCode> {
    if state.caps & CAP_COMMANDS_EXEC == 0 {
        return Err(super::deny(state, "commands:exec", program));
    }
    if program.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }

    let argv: Vec<&str> = std::iter::once(program)
        .chain(args.iter().map(|s| s.as_str()))
        .collect();

    if !state.allow_exec.iter().any(|p| p.matches(&argv)) {
        state
            .denied_log
            .push(format!("commands:exec: {}", argv.join(" ")));
        return Err(ErrorCode::PatternNotAllowed);
    }

    let out = spawn_with_timeout(program, &argv[1..], Duration::from_millis(1000))?;
    state.exec_log.push(ExecRecord {
        program: program.to_string(),
        args: args.to_vec(),
        exit_code: out.exit_code,
        stdout_len: out.stdout.len(),
        stderr_len: out.stderr.len(),
    });
    Ok(out)
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
/// bytes were captured. Never blocks past `until`.
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
    timeout: Duration,
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

    let stdout_pipe = child.stdout.take().expect("piped stdout");
    let stderr_pipe = child.stderr.take().expect("piped stderr");
    let (out_buf, out_done) = spawn_pipe_reader(stdout_pipe);
    let (err_buf, err_done) = spawn_pipe_reader(stderr_pipe);

    let deadline = Instant::now() + timeout;
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {}
            Err(_) => return Err(ErrorCode::IoFailed),
        }
        if Instant::now() >= deadline {
            let pid = nix::unistd::Pid::from_raw(child.id() as i32);
            let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM);
            let grace = Instant::now() + Duration::from_millis(100);
            loop {
                if let Ok(Some(_)) = child.try_wait() {
                    break;
                }
                if Instant::now() >= grace {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            // Bounded drain: a surviving grandchild can hold the pipes
            // open past the child's death.
            let drain = Instant::now() + Duration::from_millis(100);
            let _ = take_captured(&out_buf, &out_done, drain);
            let _ = take_captured(&err_buf, &err_done, drain);
            return Err(ErrorCode::Timeout);
        }
        thread::sleep(Duration::from_millis(10));
    };

    // The child has exited. Wait for EOF up to the remaining exec
    // budget (with a small floor if the child used it all), then take
    // whatever was captured — never block indefinitely on a
    // grandchild-held pipe.
    let floor = Instant::now() + Duration::from_millis(100);
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
    use super::*;
    use yosh_plugin_api::pattern::CommandPattern;

    fn state_with_allow(patterns: &[&str]) -> TestState {
        let mut s = TestState::with_caps(CAP_COMMANDS_EXEC);
        s.allow_exec = patterns
            .iter()
            .map(|p| CommandPattern::parse(p).unwrap())
            .collect();
        s
    }

    #[test]
    fn exec_denied_without_cap() {
        let mut s = TestState::default();
        assert!(matches!(
            host_exec(&mut s, "/bin/echo", &[]),
            Err(ErrorCode::Denied)
        ));
    }

    #[test]
    fn exec_rejects_pattern_mismatch() {
        let mut s = state_with_allow(&["ls:*"]);
        assert!(matches!(
            host_exec(&mut s, "/bin/echo", &["hi".to_string()]),
            Err(ErrorCode::PatternNotAllowed)
        ));
    }

    #[test]
    fn pattern_not_allowed_recorded_in_denied_log() {
        let mut s = TestState::with_caps(CAP_COMMANDS_EXEC);
        assert!(matches!(
            host_exec(&mut s, "echo", &["hi".to_string()]),
            Err(ErrorCode::PatternNotAllowed)
        ));
        assert_eq!(s.denied_log, vec!["commands:exec: echo hi".to_string()]);
    }

    #[test]
    fn exec_runs_when_pattern_matches() {
        let mut s = state_with_allow(&["/bin/echo:*"]);
        let out = host_exec(&mut s, "/bin/echo", &["hello".to_string()]).unwrap();
        assert_eq!(out.exit_code, 0);
        assert_eq!(out.stdout, b"hello\n");
        assert_eq!(s.exec_log.len(), 1);
        assert_eq!(s.exec_log[0].program, "/bin/echo");
    }

    /// Regression: a grandchild that inherits the pipe write ends
    /// (`sh -c 'daemon & exit 0'`) must not hang `yosh-plugin run`/
    /// `test` — the drain after child exit is deadline-bounded.
    #[test]
    fn exec_returns_when_grandchild_keeps_pipe_open() {
        let mut s = state_with_allow(&["/bin/sh:*"]);
        let start = std::time::Instant::now();
        let result = host_exec(
            &mut s,
            "/bin/sh",
            &["-c".to_string(), "echo out; sleep 5 & exit 7".to_string()],
        );
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_millis(3000),
            "call blocked on grandchild-held pipe for {:?}",
            elapsed
        );
        let out = result.expect("child exited normally");
        assert_eq!(out.exit_code, 7);
        assert_eq!(out.stdout, b"out\n");
    }

    #[test]
    fn exec_returns_not_found_for_missing_binary() {
        let mut s = state_with_allow(&["/nope/binary-xyz:*"]);
        assert!(matches!(
            host_exec(&mut s, "/nope/binary-xyz", &[]),
            Err(ErrorCode::NotFound)
        ));
    }

    /// Mirror of `src/plugin/host/commands.rs::host_commands_exec_timeout_after_1000ms`
    /// for this module's duplicated spawn helper. Upper bound follows the
    /// grandchild test above (3000ms): generous enough for loaded parallel
    /// runs, small enough to catch a wait-forever hang.
    #[test]
    fn exec_timeout_after_1000ms() {
        let mut s = state_with_allow(&["/bin/sleep:*"]);
        let start = std::time::Instant::now();
        let result = host_exec(&mut s, "/bin/sleep", &["5".to_string()]);
        let elapsed = start.elapsed();
        assert!(matches!(result, Err(ErrorCode::Timeout)));
        assert!(
            elapsed < std::time::Duration::from_millis(3000),
            "timeout took {:?}, expected <3000ms",
            elapsed
        );
    }

    /// Stronger than the production mirror: the child ignores SIGTERM, so
    /// a bounded Err(Timeout) return proves the SIGKILL fallback and the
    /// final reap actually ran (a TERM-obeying child like bare `sleep`
    /// dies in the grace period and never reaches `child.kill()`).
    #[test]
    fn exec_kills_term_ignoring_child_on_timeout() {
        let mut s = state_with_allow(&["/bin/sh:*"]);
        let start = std::time::Instant::now();
        let result = host_exec(
            &mut s,
            "/bin/sh",
            // `exec` so the tracked child PID *is* the TERM-ignoring
            // sleep (ignored dispositions survive exec) — killing a
            // wrapper sh would orphan a still-sleeping grandchild. The
            // odd duration is a unique process-table marker for the
            // survivor check below.
            &["-c".to_string(), "trap '' TERM; exec sleep 4.917".to_string()],
        );
        let elapsed = start.elapsed();
        assert!(matches!(result, Err(ErrorCode::Timeout)));
        // Lower bound: SIGTERM only fires after the 1000ms deadline.
        assert!(
            elapsed >= std::time::Duration::from_millis(900),
            "elapsed {:?} too small — timeout fired before deadline",
            elapsed
        );
        // Upper bound: deadline + grace + SIGKILL + scheduling slack. A
        // hang here would mean the TERM-ignoring child was never killed.
        assert!(
            elapsed < std::time::Duration::from_millis(3000),
            "elapsed {:?} too large — SIGKILL fallback or reap missing",
            elapsed
        );
        // Survivor check: the bounded drain could return Timeout even
        // with the SIGKILL step deleted; the unique marker proves the
        // TERM-ignoring child is actually gone from the process table.
        let survivors = std::process::Command::new("pgrep")
            .args(["-f", "sleep 4.917"])
            .output()
            .expect("pgrep runs");
        assert!(
            survivors.stdout.is_empty(),
            "TERM-ignoring child survived the timeout kill: {:?}",
            String::from_utf8_lossy(&survivors.stdout)
        );
    }
}
