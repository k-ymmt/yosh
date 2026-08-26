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
    // Use TERM, not INT: async children ignore SIGINT/SIGQUIT when job
    // control is off (POSIX §2.11), so INT would never kill the sleep.
    let (stdout, _stderr, code) =
        yosh_exec_timeout("exec sleep 100 & kill -s TERM $!; wait $!; echo $?", 10);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "143"); // 128 + 15 (SIGTERM)
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
    // TERM instead of INT — see test_kill_dash_s.
    let (stdout, _stderr, code) =
        yosh_exec_timeout("exec sleep 100 & kill -TERM $!; wait $!; echo $?", 10);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "143");
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

// SIGINT trap ordering

#[test]
fn sigint_trap_fires_between_commands() {
    let script =
        "trap 'echo caught' INT\nkill -INT $$ 2>/dev/null\nsleep 0.05 2>/dev/null\necho after\n";
    let out = yosh_exec(script);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let caught_idx = stdout.find("caught").expect("stdout must contain 'caught'");
    let after_idx = stdout.find("after").expect("stdout must contain 'after'");
    assert!(
        caught_idx < after_idx,
        "trap output must precede 'after' line; got stdout = {:?}",
        stdout
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

// `command` must dispatch the Executor-hosted builtins (wait/fg/bg/jobs),
// not fall through to a PATH binary (audit M4).

#[test]
fn test_command_wait_waits_for_background_job() {
    let start = std::time::Instant::now();
    let (_stdout, _stderr, code) = yosh_exec_timeout("sleep 0.4 & command wait", 10);
    assert_eq!(code, Some(0));
    assert!(
        start.elapsed() >= Duration::from_millis(300),
        "`command wait` must actually wait for the background job; returned after {:?}",
        start.elapsed()
    );
}

#[test]
fn test_command_wait_reports_operand_status() {
    let (stdout, _stderr, code) =
        yosh_exec_timeout("sh -c 'exit 3' & command wait $!; echo st=$?", 10);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "st=3");
}

#[test]
fn test_command_jobs_is_builtin() {
    let (stdout, stderr, code) = yosh_exec_timeout("sleep 0.3 & command jobs; wait", 10);
    assert_eq!(code, Some(0), "stderr: {}", stderr);
    assert!(
        stdout.contains("Running"),
        "`command jobs` must run the jobs builtin; stdout = {:?}, stderr = {:?}",
        stdout,
        stderr
    );
}

// Signal-trap deferral fixes (audit M7 / P4): repeated signals between
// commands must still be observed with the pending-flag fast path.

#[test]
fn test_repeated_signals_all_observed_between_commands() {
    let script = "trap 'echo T' USR1\nkill -USR1 $$\necho a\nkill -USR1 $$\necho b\n";
    let out = yosh_exec(script);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let t_count = stdout.matches('T').count();
    assert!(
        t_count >= 2,
        "both USR1 deliveries must fire the trap; stdout = {:?}",
        stdout
    );
    // Ordering: first T before 'a', second T before 'b'.
    let a_idx = stdout.find('a').expect("stdout must contain 'a'");
    let first_t = stdout.find('T').expect("stdout must contain 'T'");
    assert!(first_t < a_idx, "trap must fire before next command; stdout = {:?}", stdout);
}

// Background reaping still works with the live-jobs fast path (audit P4a).

#[test]
fn test_background_job_reaped_and_waitable_after_exit() {
    let (stdout, _stderr, code) = yosh_exec_timeout(
        "sh -c 'exit 5' & p=$!; sleep 0.2; :; wait $p; echo st=$?",
        10,
    );
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "st=5");
}

// SIGPIPE reaches externally-run children at default disposition (audit H1).

#[test]
fn test_child_sigpipe_default_disposition() {
    let (stdout, _stderr, code) =
        yosh_exec_timeout("sh -c 'kill -PIPE $$; echo survived'; echo st=$?", 10);
    assert_eq!(code, Some(0));
    assert_eq!(
        stdout.trim(),
        "st=141",
        "child must die of SIGPIPE (128+13), not survive with it ignored"
    );
}

// Signal trap actions must not clobber `$?` (POSIX §2.12: on completion
// of a trap action, `$?` is restored to its pre-trap value).

#[test]
fn test_signal_trap_action_restores_status() {
    let out = yosh_exec("trap 'false' USR1; kill -USR1 $$; echo $?");
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "0",
        "$? after the trap action must be kill's status (0), not the action's (1)"
    );
}

#[test]
fn test_signal_trap_action_output_then_restored_status() {
    let out = yosh_exec("trap 'echo t' USR1; kill -USR1 $$; echo $?");
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"),
        "t\n0\n",
        "trap output must precede the restored (0) status"
    );
}

#[test]
fn test_signal_trap_action_exit_keeps_trap_context_status() {
    // `exit` without an operand inside a trap action uses the value $?
    // had when the action started (POSIX §2.12) — the post-action $?
    // restore must not interfere with the requested exit.
    let out = yosh_exec("trap 'exit' USR1; sh -c 'kill -USR1 $PPID; exit 5'; echo unreached");
    assert_eq!(out.status.code(), Some(5));
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("unreached"),
        "exit inside the trap action must terminate the shell"
    );
}

#[test]
fn test_errexit_applies_in_signal_trap_action() {
    // bash/dash/zsh: `set -e` is NOT suspended inside a signal trap
    // action — the failing `false` exits the shell with status 1 and
    // `echo ok` never runs.
    let out = yosh_exec("set -e; trap 'false' USR1; kill -USR1 $$; echo ok");
    assert_eq!(out.status.code(), Some(1));
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("ok"),
        "errexit must abort the shell from inside the trap action"
    );
}

// A command-substitution child resets the parent's traps; the OS signal
// dispositions installed for parent Command traps must be restored to
// baseline too, or the leftover self-pipe handler (SA_RESTART) catches
// the signal and the child only dies at the next command boundary —
// i.e. after the blocking `sleep 3` completes instead of immediately.

#[test]
fn test_command_sub_child_dies_immediately_on_parent_trapped_signal() {
    let start = std::time::Instant::now();
    let (stdout, _stderr, code) = yosh_exec_timeout(
        r#"trap ":" ABRT; x=$(sh -c 'kill -ABRT $PPID; sleep 3' >/dev/null; echo survived); echo "[$x]""#,
        10,
    );
    assert_eq!(code, Some(0));
    assert_eq!(
        stdout.trim(),
        "[]",
        "the command-sub child must die from ABRT before echoing (bash agrees)"
    );
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "child must be killed by ABRT mid-wait, not survive until sleep 3 finishes; took {:?}",
        start.elapsed()
    );
}

// Fork children that keep running shell code (command substitution,
// subshells, pipeline members, async lists) must not inherit the
// parent's self-pipe handlers for HANDLED_SIGNALS either: POSIX §2.12
// resets traps caught by the parent to default in a subshell, so a
// USR1/TERM/… whose only trap belonged to the parent must kill the
// child immediately, mid-blocking-syscall — not be caught by the
// leftover SA_RESTART handler and deferred to the next command boundary
// (i.e. after a long `sleep` completes). The `sh -c 'kill … $PPID'`
// runs in the foreground, so $PPID is exactly the substitution/subshell
// child. (bash/dash agree on all of these.)

#[test]
fn test_command_sub_child_dies_immediately_on_parent_trapped_handled_signal() {
    // USR1 is in HANDLED_SIGNALS (unlike ABRT above), so the shell's
    // always-registered self-pipe handler must be reset too.
    let start = std::time::Instant::now();
    let (stdout, _stderr, code) = yosh_exec_timeout(
        r#"trap ":" USR1; x=$(sh -c 'kill -USR1 $PPID; sleep 3' >/dev/null; echo survived); echo "[$x]""#,
        10,
    );
    assert_eq!(code, Some(0));
    assert_eq!(
        stdout.trim(),
        "[]",
        "the command-sub child must die from USR1 before echoing (bash agrees)"
    );
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "child must be killed by USR1 mid-wait, not survive until sleep 3 finishes; took {:?}",
        start.elapsed()
    );
}

#[test]
fn test_command_sub_child_dies_immediately_on_untrapped_handled_signal() {
    // Same as above with NO parent trap: the plain HANDLED_SIGNALS
    // self-pipe handler alone must not keep the child alive either.
    let start = std::time::Instant::now();
    let (stdout, _stderr, code) = yosh_exec_timeout(
        r#"x=$(sh -c 'kill -USR1 $PPID; sleep 3' >/dev/null; echo survived); echo "[$x]""#,
        10,
    );
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "[]");
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn test_subshell_child_dies_immediately_on_parent_trapped_handled_signal() {
    let start = std::time::Instant::now();
    let (stdout, _stderr, code) = yosh_exec_timeout(
        r#"trap ":" USR1; (sh -c 'kill -USR1 $PPID; sleep 3' >/dev/null; echo survived); echo after=$?"#,
        10,
    );
    assert_eq!(code, Some(0));
    assert_eq!(
        stdout.trim(),
        "after=158",
        "the subshell must die from USR1 (128+30) without echoing"
    );
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "took {:?}",
        start.elapsed()
    );
}

// A trap set INSIDE the fork child must still work after the reset:
// the child's own `trap` builtin reinstalls the self-pipe handler, and
// the child gets a FRESH self-pipe (sharing the parent's pipe would let
// one process drain the other's signal bytes; closing it — the old
// subshell behavior — silently lost the child's own traps).

#[test]
fn test_command_sub_child_own_trap_still_works() {
    let (stdout, _stderr, code) = yosh_exec_timeout(
        r#"x=$(trap "echo t" USR1; sh -c 'kill -USR1 $PPID'; echo done); echo "[$x]""#,
        10,
    );
    assert_eq!(code, Some(0));
    assert_eq!(
        stdout.trim(),
        "[t\ndone]",
        "a trap set inside the command substitution must fire (bash agrees)"
    );
}

#[test]
fn test_subshell_child_own_trap_still_works() {
    // Pre-fix, reset_child_signals closed the inherited self-pipe fds in
    // subshell children, so the reinstalled handler wrote into a closed
    // pipe and the trap (and the signal itself) vanished silently.
    let (stdout, _stderr, code) = yosh_exec_timeout(
        r#"(trap "echo t" USR1; sh -c 'kill -USR1 $PPID'; echo done)"#,
        10,
    );
    assert_eq!(code, Some(0));
    assert_eq!(
        stdout.trim(),
        "t\ndone",
        "a trap set inside a ( ) subshell must fire (bash agrees)"
    );
}

#[test]
fn test_pipeline_member_own_trap_still_works() {
    let (stdout, _stderr, code) = yosh_exec_timeout(
        r#"{ trap "echo t" USR1; sh -c 'kill -USR1 $PPID'; echo done; } | cat"#,
        10,
    );
    assert_eq!(code, Some(0));
    assert_eq!(
        stdout.trim(),
        "t\ndone",
        "a trap set inside a pipeline member must fire (bash agrees)"
    );
}

#[test]
fn test_async_child_own_trap_still_works() {
    let (stdout, _stderr, code) = yosh_exec_timeout(
        r#"{ trap "echo t" USR1; sh -c 'kill -USR1 $PPID'; echo done; } > /tmp/yosh-async-trap-$$.out & wait $!; cat /tmp/yosh-async-trap-$$.out; rm -f /tmp/yosh-async-trap-$$.out"#,
        10,
    );
    assert_eq!(code, Some(0));
    assert_eq!(
        stdout.trim(),
        "t\ndone",
        "a trap set inside an async list must fire (bash agrees)"
    );
}

// `command <external>` must go through the standard fork/exec path
// (job control, signal reset, wait bookkeeping), not a bespoke spawn.

#[test]
fn test_command_external_signal_exit_code() {
    let (stdout, _stderr, code) = yosh_exec_timeout("command sh -c 'kill -TERM $$'; echo st=$?", 10);
    assert_eq!(code, Some(0));
    assert_eq!(
        stdout.trim(),
        "st=143",
        "a signal-killed `command` external must report 128+signo"
    );
}

#[test]
fn test_command_external_argv0_and_args() {
    let (stdout, _stderr, code) =
        yosh_exec_timeout("command sh -c 'echo argv0=$0 first=$1' zero one", 10);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "argv0=zero first=one");
}

#[test]
fn test_pipeline_member_sigpipe_default() {
    // Killing the whole pipeline writer with EPIPE: `yes | head -1`
    // terminates promptly only if the writer dies on SIGPIPE.
    let (stdout, _stderr, code) = yosh_exec_timeout("yes 2>/dev/null | head -1", 10);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "y");
}
