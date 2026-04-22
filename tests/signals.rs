mod helpers;

use helpers::reset_trap_signals;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Atomic counter to ensure unique temp file names across parallel tests.
static TIMEOUT_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn yosh_exec(input: &str) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_yosh"));
    cmd.args(["-c", input]);
    reset_trap_signals(&mut cmd);
    cmd.output().expect("failed to execute yosh")
}

/// Run a yosh command with a timeout, using temp files for output to avoid
/// pipe-inheritance issues with background processes.
/// Returns (stdout, stderr, exit_code).
fn yosh_exec_timeout(input: &str, timeout_secs: u64) -> (String, String, Option<i32>) {
    let id = std::process::id();
    let seq = TIMEOUT_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let stdout_path = format!("/tmp/yosh-test-{}-{}-out", id, seq);
    let stderr_path = format!("/tmp/yosh-test-{}-{}-err", id, seq);

    let stdout_file = std::fs::File::create(&stdout_path).expect("create stdout file");
    let stderr_file = std::fs::File::create(&stderr_path).expect("create stderr file");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_yosh"));
    cmd.args(["-c", input])
        .stdin(Stdio::null())
        .stdout(stdout_file)
        .stderr(stderr_file)
        .process_group(0);
    reset_trap_signals(&mut cmd);
    let mut child = cmd.spawn().expect("failed to spawn yosh");

    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(timeout_secs);

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if start.elapsed() > timeout {
                    // Kill the process group
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGKILL);
                    }
                    let _ = child.wait();
                    let _ = std::fs::remove_file(&stdout_path);
                    let _ = std::fs::remove_file(&stderr_path);
                    panic!("yosh timed out after {}s for: {}", timeout_secs, input);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&stdout_path);
                let _ = std::fs::remove_file(&stderr_path);
                panic!("error waiting for yosh: {}", e);
            }
        }
    };

    // Kill any remaining background processes in the process group
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    // Brief wait for zombies to be cleaned up
    std::thread::sleep(Duration::from_millis(10));

    let stdout = std::fs::read_to_string(&stdout_path).unwrap_or_default();
    let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
    let _ = std::fs::remove_file(&stdout_path);
    let _ = std::fs::remove_file(&stderr_path);

    (stdout, stderr, status.code())
}

// Signal trap tests

#[test]
fn test_trap_int_execution() {
    let out = yosh_exec("trap 'echo caught' INT; kill -INT $$; echo after");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("caught"));
    assert!(stdout.contains("after"));
}

#[test]
fn test_trap_reset() {
    let out = yosh_exec("trap 'echo x' INT; trap - INT; trap");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("INT"));
}

#[test]
fn test_subshell_trap_reset() {
    let out = yosh_exec("trap 'echo x' INT; (trap)");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("INT"));
}

#[test]
fn test_subshell_ignore_preserved() {
    let out = yosh_exec("trap '' INT; (trap -p INT)");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("INT"));
}

// kill tests

#[test]
fn test_kill_default_sigterm() {
    let (stdout, _stderr, code) =
        yosh_exec_timeout("exec sleep 100 & kill $!; wait $!; echo $?", 10);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "143"); // 128 + 15 (SIGTERM)
}

#[test]
fn test_kill_dash_s() {
    let (stdout, _stderr, code) =
        yosh_exec_timeout("exec sleep 100 & kill -s INT $!; wait $!; echo $?", 10);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "130"); // 128 + 2 (SIGINT)
}

#[test]
fn test_kill_dash_9() {
    let (stdout, _stderr, code) =
        yosh_exec_timeout("exec sleep 100 & kill -9 $!; wait $!; echo $?", 10);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "137"); // 128 + 9 (SIGKILL)
}

#[test]
fn test_kill_dash_signal_name() {
    let (stdout, _stderr, code) =
        yosh_exec_timeout("exec sleep 100 & kill -INT $!; wait $!; echo $?", 10);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "130");
}

#[test]
fn test_kill_list() {
    let out = yosh_exec("kill -l");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("HUP"));
    assert!(stdout.contains("INT"));
    assert!(stdout.contains("TERM"));
}

#[test]
fn test_kill_list_status() {
    let out = yosh_exec("kill -l 130");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "INT");
}

// wait tests

#[test]
fn test_wait_basic() {
    let (stdout, _stderr, code) = yosh_exec_timeout("exec sleep 0.1 & wait; echo done", 10);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "done");
}

#[test]
fn test_wait_pid() {
    let (stdout, _stderr, code) =
        yosh_exec_timeout("exec sleep 0.1 & pid=$!; wait $pid; echo $?", 10);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "0");
}

#[test]
fn test_wait_nonexistent_pid() {
    let out = yosh_exec("wait 99999");
    assert_eq!(out.status.code(), Some(127));
}

#[test]
fn test_kill_0_targets_shell_pgid() {
    // In a pipeline, `kill 0` should target the shell's process group,
    // not the pipeline's process group. We verify by using a trap + kill 0 in
    // a pipeline command — if kill 0 incorrectly targets only the pipeline group,
    // the trap on the shell won't fire.
    let (stdout, _stderr, code) = yosh_exec_timeout(
        "trap 'echo trapped' TERM; true | kill -TERM 0; echo after",
        5,
    );
    assert_eq!(code, Some(0));
    let stdout_str = stdout.trim();
    // The trap should fire because kill 0 targets the shell's process group
    assert!(
        stdout_str.contains("trapped"),
        "expected trap to fire, got: {}",
        stdout_str
    );
    assert!(
        stdout_str.contains("after"),
        "expected execution to continue, got: {}",
        stdout_str
    );
}

// Background job tracking

#[test]
fn test_background_job_last_pid() {
    let (stdout, _stderr, code) = yosh_exec_timeout("true & echo $!", 5);
    assert_eq!(code, Some(0));
    let pid: i32 = stdout.trim().parse().expect("$! should be a number");
    assert!(pid > 0);
}
