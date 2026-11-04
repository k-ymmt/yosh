//! `yosh -c 'cmd'` runs its last simple command in place (exec, no
//! fork) when nothing can run after it, as dash and bash do. These tests
//! observe the optimization through process identity: a command exec'd
//! in place has the shell's own PID, so `echo $$; /bin/sh -c 'echo $$'`
//! prints the same number twice. Every condition that must keep the
//! fork (a trap, a job, a function, a later command, ...) prints two
//! different numbers.

use std::process::{Command, Output};

fn yosh(args: &[&str]) -> Output {
    let home = std::env::temp_dir();
    Command::new(env!("CARGO_BIN_EXE_yosh"))
        .args(args)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &home)
        .output()
        .expect("run yosh")
}

fn stdout_lines(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

/// Run `script` with `yosh -c` and return (shell pid line, inner pid
/// line, all output lines).
fn pids(args: &[&str]) -> (String, String, Output) {
    let out = yosh(args);
    let lines = stdout_lines(&out);
    assert!(
        lines.len() >= 2,
        "expected two pid lines, got {:?} (stderr {:?})",
        lines,
        String::from_utf8_lossy(&out.stderr)
    );
    (lines[0].clone(), lines[1].clone(), out)
}

const INNER: &str = "/bin/sh -c 'echo $$'";

#[test]
fn last_simple_command_replaces_the_shell() {
    let (shell, inner, out) = pids(&["-c", &format!("echo $$; {INNER}")]);
    assert_eq!(shell, inner);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn trailing_blank_lines_still_count_as_last() {
    let (shell, inner, _) = pids(&["-c", &format!("echo $$\n{INNER}\n\n  \n")]);
    assert_eq!(shell, inner);
}

#[test]
fn exit_status_of_the_command_is_the_shell_status() {
    let out = yosh(&["-c", "/bin/sh -c 'exit 7'"]);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn exit_trap_keeps_the_fork() {
    let (shell, inner, out) = pids(&["-c", &format!("trap 'echo bye' EXIT; echo $$; {INNER}")]);
    assert_ne!(shell, inner);
    assert_eq!(stdout_lines(&out).last().map(String::as_str), Some("bye"));
}

#[test]
fn signal_trap_keeps_the_fork() {
    let (shell, inner, _) = pids(&["-c", &format!("trap 'echo int' INT; echo $$; {INNER}")]);
    assert_ne!(shell, inner);
}

#[test]
fn ignore_trap_keeps_the_fork() {
    let (shell, inner, _) = pids(&["-c", &format!("trap '' USR1; echo $$; {INNER}")]);
    assert_ne!(shell, inner);
}

#[test]
fn background_job_keeps_the_fork() {
    let (shell, inner, _) = pids(&["-c", &format!("sleep 1 >/dev/null 2>&1 & echo $$; {INNER}")]);
    assert_ne!(shell, inner);
}

#[test]
fn function_named_like_the_command_keeps_the_fork() {
    // The function wins over PATH; its body's external is not the
    // shell's tail and must not replace it.
    let (shell, inner, out) = pids(&[
        "-c",
        &format!("sh() {{ {INNER}; echo after; }}; echo $$; sh"),
    ]);
    assert_ne!(shell, inner);
    assert_eq!(stdout_lines(&out).last().map(String::as_str), Some("after"));
}

#[test]
fn later_command_keeps_the_fork() {
    let out = yosh(&["-c", &format!("echo $$; {INNER}; echo done")]);
    let lines = stdout_lines(&out);
    assert_ne!(lines[0], lines[1]);
    assert_eq!(lines[2], "done");
}

#[test]
fn later_line_keeps_the_fork() {
    let out = yosh(&["-c", &format!("echo $$; {INNER}\necho done")]);
    let lines = stdout_lines(&out);
    assert_ne!(lines[0], lines[1]);
    assert_eq!(lines[2], "done");
}

#[test]
fn and_or_list_keeps_the_fork_and_semantics() {
    let out = yosh(&["-c", &format!("echo $$; true && {INNER}")]);
    let lines = stdout_lines(&out);
    assert_eq!(lines.len(), 2);
    let out = yosh(&["-c", "/bin/sh -c 'exit 3' || echo rescued"]);
    assert_eq!(stdout_lines(&out), ["rescued"]);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn negated_pipeline_keeps_negation() {
    let out = yosh(&["-c", "! /bin/sh -c 'exit 3'"]);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn background_last_command_is_not_execed() {
    let out = yosh(&["-c", "echo $$; /bin/sh -c 'echo $$' &"]);
    // The shell does not wait for it; just make sure the shell itself
    // ran to completion with status 0.
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn redirections_apply_to_the_execed_command() {
    let (shell, inner, _) = pids(&["-c", "echo $$; /bin/sh -c 'echo $$ >&2' 2>&1"]);
    assert_eq!(shell, inner);
}

#[test]
fn redirection_failure_reports_and_exits_1() {
    let out = yosh(&["-c", "/bin/echo hi </nonexistent/p3b/file"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("/nonexistent/p3b/file"),
        "stderr: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn errexit_with_failing_tail_command() {
    let (shell, inner, out) = pids(&["-e", "-c", "echo $$; /bin/sh -c 'echo $$; exit 4'"]);
    assert_eq!(shell, inner);
    assert_eq!(out.status.code(), Some(4));
}

#[test]
fn buffered_builtin_output_is_flushed_before_exec() {
    let out = yosh(&["-c", "printf a; printf b; /bin/echo c"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "abc\n");
}

#[test]
fn prefix_assignments_reach_the_execed_command() {
    let out = yosh(&["-c", "FOO=bar /bin/sh -c 'echo $FOO'"]);
    assert_eq!(stdout_lines(&out), ["bar"]);
}

#[test]
fn not_found_tail_command_reports_127() {
    let out = yosh(&["-c", "p3b_no_such_command_xyz"]);
    assert_eq!(out.status.code(), Some(127));
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("yosh: p3b_no_such_command_xyz: command not found")
    );
}

#[test]
fn builtin_tail_is_unaffected() {
    let out = yosh(&["-c", "echo $$; echo builtin"]);
    assert_eq!(stdout_lines(&out)[1], "builtin");
}

#[test]
fn interactive_flag_keeps_the_fork() {
    let out = yosh(&["-i", "-c", &format!("echo $$; {INNER}")]);
    let lines = stdout_lines(&out);
    assert_eq!(lines.len(), 2, "{:?}", lines);
    assert_ne!(lines[0], lines[1]);
}

#[test]
fn script_files_are_unaffected() {
    let dir = std::env::temp_dir().join(format!("yosh-p3b-tail-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("s.sh");
    std::fs::write(&script, format!("echo $$; {INNER}\n")).unwrap();
    let out = yosh(&[script.to_str().unwrap()]);
    let lines = stdout_lines(&out);
    assert_ne!(lines[0], lines[1]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn signal_death_of_the_tail_command_is_the_shell_death() {
    // In place, the shell process *is* the command, so its parent sees
    // it killed by the signal (dash and bash -c behave the same).
    use std::os::unix::process::ExitStatusExt;
    let out = yosh(&["-c", "/bin/sh -c 'kill -TERM $$'"]);
    assert_eq!(out.status.signal(), Some(libc::SIGTERM));
}

#[cfg(unix)]
#[test]
fn signal_queued_during_expansion_still_ends_the_shell() {
    // The TERM arrives while the substitution runs; the exec must not
    // drop it from the self-pipe (b0ebc43, dash and bash all die of it).
    use std::os::unix::process::ExitStatusExt;
    let out = yosh(&["-c", "/bin/echo $(kill -TERM $$; echo sub)"]);
    assert_eq!(out.status.signal(), Some(libc::SIGTERM), "{:?}", out);
}

#[test]
fn descriptors_from_exec_redirections_reach_the_command() {
    // fd 11 may be a self-pipe fd number; the in-place exec must not
    // close it after `exec 11>&1` put the shell's stdout there.
    for fd in 10..=13 {
        let script = format!("exec {fd}>&1; /bin/sh -c 'echo hello >&{fd}'");
        let out = yosh(&["-c", &script]);
        assert_eq!(stdout_lines(&out), ["hello"], "fd {fd}: {:?}", out);
        assert_eq!(out.status.code(), Some(0));
        // The forked path (a redirection on the command) too.
        let script = format!("exec {fd}>&1; /bin/sh -c 'echo hello >&{fd}' 2>&2; :");
        let out = yosh(&["-c", &script]);
        assert_eq!(stdout_lines(&out), ["hello"], "fd {fd} forked: {:?}", out);
    }
}

#[test]
fn verbose_mode_still_echoes_trailing_blank_lines() {
    let out = yosh(&["-v", "-c", "/bin/echo hi\n\n"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hi\n");
    assert_eq!(String::from_utf8_lossy(&out.stderr), "/bin/echo hi\n\n");
}

#[test]
fn verbose_set_on_the_same_line_still_echoes_trailing_input() {
    let out = yosh(&["-c", "set -v; /bin/echo hi\n   \n"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hi\n");
    assert_eq!(String::from_utf8_lossy(&out.stderr), "   \n");
}

#[test]
fn redirection_from_the_signal_pipe_fds_fails() {
    // The shell's own descriptors (>= 10) are not the script's: the
    // command must not inherit one through `3>&N` (b0ebc43, bash, dash).
    for script in [
        "/bin/echo RAN 3>&11",
        "/bin/echo RAN 3>&11; :",
        "/bin/echo RAN 3>&10",
    ] {
        let out = yosh(&["-c", script]);
        assert_eq!(String::from_utf8_lossy(&out.stdout), "", "{script}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("Bad file descriptor"),
            "{script}"
        );
    }
}
