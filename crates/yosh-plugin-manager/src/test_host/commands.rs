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

fn spawn_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
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
            let _ = out_rx.recv();
            let _ = err_rx.recv();
            return Err(ErrorCode::Timeout);
        }
        thread::sleep(Duration::from_millis(10));
    };

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

    #[test]
    fn exec_returns_not_found_for_missing_binary() {
        let mut s = state_with_allow(&["/nope/binary-xyz:*"]);
        assert!(matches!(
            host_exec(&mut s, "/nope/binary-xyz", &[]),
            Err(ErrorCode::NotFound)
        ));
    }
}
