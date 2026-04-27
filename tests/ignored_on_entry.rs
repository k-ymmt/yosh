//! Integration tests for POSIX §2.11 ignored-on-entry signal inheritance.
//!
//! Each test spawns the yosh binary in a subprocess with specific signals
//! pre-set to SIG_IGN via `pre_exec`, then asserts yosh's observable
//! behaviour (stdout, stderr, exit code). This verifies the end-to-end
//! flow from `capture_ignored_on_entry` through `TrapStore::set_trap`
//! and `reset_child_signals`.

use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};

/// Spawn yosh with the given signal numbers pre-ignored (SIG_IGN) in the child,
/// feeding the `script` to `yosh -c`. Returns (stdout, stderr, exit_code).
///
/// Isolates the child from the developer's `~/.config/yosh/plugins.lock`
/// so a stale plugin entry there can't pollute stderr — these tests assert
/// on the shell's own diagnostic output and must not see plugin loader
/// warnings. We point HOME at a freshly-created tempdir for the duration.
fn spawn_yosh_with_ignored(signals: &[i32], script: &str) -> (String, String, i32) {
    let isolated_home = tempfile::tempdir().expect("tempdir for isolated HOME");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_yosh"));
    cmd.arg("-c").arg(script);
    cmd.env("HOME", isolated_home.path());
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let sigs = signals.to_vec();
    unsafe {
        cmd.pre_exec(move || {
            let sa = SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty());
            for &num in &sigs {
                let sig = Signal::try_from(num)
                    .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
                sigaction(sig, &sa)
                    .map_err(|e| std::io::Error::from_raw_os_error(e as i32))?;
            }
            Ok(())
        });
    }

    let out = cmd.output().expect("yosh binary should be buildable and runnable");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn trap_set_on_ignored_on_entry_sigint_is_silent() {
    // Parent sets SIGINT=SIG_IGN, then yosh runs `trap 'echo caught' INT; echo $?`.
    // POSIX §2.11: the trap set must silently no-op and $? must be 0.
    let (stdout, stderr, code) = spawn_yosh_with_ignored(
        &[libc::SIGINT],
        "trap 'echo caught' INT; echo $?",
    );
    assert_eq!(code, 0, "exit code; stderr={}", stderr);
    assert_eq!(stdout.trim(), "0", "stdout should be just '0'; got {:?}", stdout);
    assert!(stderr.is_empty(), "no stderr expected, got {:?}", stderr);
}

#[test]
fn trap_reset_on_ignored_on_entry_sigint_is_silent() {
    // `trap - INT` on an ignored-on-entry signal must also silent no-op.
    let (stdout, stderr, code) = spawn_yosh_with_ignored(
        &[libc::SIGINT],
        "trap - INT; echo $?",
    );
    assert_eq!(code, 0, "exit code; stderr={}", stderr);
    assert_eq!(stdout.trim(), "0", "stdout={:?}", stdout);
}

#[test]
fn trap_display_shows_ignored_on_entry() {
    // `trap` with no args should list SIGTERM as `trap -- '' SIGTERM`.
    let (stdout, _stderr, code) = spawn_yosh_with_ignored(
        &[libc::SIGTERM],
        "trap",
    );
    assert_eq!(code, 0);
    assert!(
        stdout.contains("trap -- '' SIGTERM"),
        "expected 'trap -- \\'\\' SIGTERM' in stdout; got {:?}",
        stdout
    );
}

#[test]
fn subshell_inherits_ignored_on_entry() {
    // In a subshell `( trap )`, SIGTERM should still appear as ignored.
    let (stdout, _stderr, code) = spawn_yosh_with_ignored(
        &[libc::SIGTERM],
        "( trap )",
    );
    assert_eq!(code, 0);
    assert!(
        stdout.contains("trap -- '' SIGTERM"),
        "subshell should inherit ignored-on-entry; stdout={:?}",
        stdout
    );
}

#[test]
fn external_cmd_inherits_ignored_on_entry() {
    // When yosh execs an external command, SIG_IGN must be preserved
    // across exec (POSIX guarantee, reinforced by reset_child_signals union).
    // We verify that SIGINT remains SIG_IGN in the child by sending it to the
    // child's own PID: if the signal is ignored the child survives and prints
    // "still_alive"; if SIG_IGN was lost the child is terminated and no output
    // appears.
    let (stdout, _stderr, code) = spawn_yosh_with_ignored(
        &[libc::SIGINT],
        "sh -c 'kill -INT $$; echo still_alive'",
    );
    assert_eq!(code, 0, "external sh should survive SIGINT (SIG_IGN preserved); stdout={:?}", stdout);
    assert!(
        stdout.contains("still_alive"),
        "external sh should inherit SIGINT SIG_IGN and survive kill; stdout={:?}",
        stdout
    );
}

#[test]
fn non_ignored_signal_trap_still_works() {
    // Sanity: a signal NOT ignored-on-entry still accepts trap actions.
    // We trap SIGUSR1 then send it to the shell's PID and check output.
    let (stdout, stderr, code) = spawn_yosh_with_ignored(
        &[], // no signals pre-ignored
        "trap 'echo caught' USR1; kill -USR1 $$; wait; echo done",
    );
    assert_eq!(code, 0, "stderr={}", stderr);
    assert!(
        stdout.contains("caught"),
        "USR1 trap should fire; stdout={:?} stderr={:?}",
        stdout,
        stderr
    );
    assert!(stdout.contains("done"), "shell should continue; stdout={:?}", stdout);
}
