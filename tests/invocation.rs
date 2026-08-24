//! Invocation-option tests for the `-i` (force interactive) flag and the
//! `$-` interactive-letter snapshot (POSIX XCU sh invocation + 2.5.2).
//!
//! These run the real binary with piped stdin, so stdin is NOT a terminal:
//! any `i` in `$-` comes from the `-i` flag, not from tty auto-detection.
//! The full-terminal path is covered by `tests/pty_interactive.rs`.

use std::io::Write;
use std::process::{Command, Stdio};

fn yosh_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_yosh"))
}

/// Run yosh with `args`, feeding `stdin` to it, and return (stdout, exit code).
fn run_with_stdin(args: &[&str], stdin: &str) -> (String, i32) {
    let mut child = yosh_bin()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn piped_stdin_without_i_omits_i_in_flags() {
    let (out, code) = run_with_stdin(&[], "echo flags-$-\n");
    assert_eq!(code, 0);
    let flags = out.trim().strip_prefix("flags-").unwrap().to_string();
    assert!(
        !flags.contains('i'),
        "no -i and no tty => no i, got {out:?}"
    );
}

#[test]
fn dash_i_piped_stdin_reports_i() {
    let (out, code) = run_with_stdin(&["-i"], "echo flags-$-\n");
    assert_eq!(code, 0);
    let flags = out.trim().strip_prefix("flags-").unwrap().to_string();
    assert!(flags.contains('i'), "-i must put i into $-, got {out:?}");
}

#[test]
fn dash_i_with_c_reports_i_and_c() {
    let output = yosh_bin()
        .args(["-i", "-c", "echo flags-$-"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    let flags = out.trim().strip_prefix("flags-").unwrap().to_string();
    assert!(flags.contains('i'), "got {out:?}");
    assert!(flags.contains('c'), "got {out:?}");
}

#[test]
fn dash_ic_cluster_equals_separate_flags() {
    let output = yosh_bin().args(["-ic", "echo flags-$-"]).output().unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    let flags = out.trim().strip_prefix("flags-").unwrap().to_string();
    assert!(flags.contains('i') && flags.contains('c'), "got {out:?}");
}

#[test]
fn dash_i_script_file_reports_i() {
    let dir = std::env::temp_dir().join(format!("yosh-inv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("flags.sh");
    std::fs::write(&script, "echo flags-$-\n").unwrap();

    let with_i = yosh_bin().arg("-i").arg(&script).output().unwrap();
    let out = String::from_utf8_lossy(&with_i.stdout);
    let flags = out.trim().strip_prefix("flags-").unwrap().to_string();
    assert!(flags.contains('i'), "got {out:?}");

    let without_i = yosh_bin().arg(&script).output().unwrap();
    let out = String::from_utf8_lossy(&without_i.stdout);
    let flags = out.trim().strip_prefix("flags-").unwrap().to_string();
    assert!(!flags.contains('i'), "got {out:?}");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn dash_i_ignores_untrapped_term() {
    // POSIX: an interactive shell ignores untrapped SIGTERM. With -i the
    // pending TERM drained at end-of-script must not kill the shell.
    let (out, code) = run_with_stdin(&["-i"], "kill -TERM $$\necho alive-$?\n");
    assert!(out.contains("alive-0"), "got {out:?}");
    assert_eq!(code, 0, "-i shell must survive untrapped TERM");

    // Sanity: without -i the same script dies with 128+15.
    let (_, code) = run_with_stdin(&[], "kill -TERM $$\necho alive-$?\n");
    assert_eq!(code, 143, "non-interactive shell must die on TERM");
}

#[test]
fn command_sub_dollar_dash_keeps_i() {
    // The command-sub child runs with is_interactive=false (behavioral)
    // but its $- must still report `i` via the flag_i snapshot.
    let output = yosh_bin()
        .args(["-i", "-c", "echo sub-$(echo $-)"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    let flags = out.trim().strip_prefix("sub-").unwrap().to_string();
    assert!(
        flags.contains('i'),
        "command-sub $- must keep i, got {out:?}"
    );

    // Nested substitution keeps propagating the snapshot.
    let output = yosh_bin()
        .args(["-i", "-c", "echo nest-$(echo $(echo $-))"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    let flags = out.trim().strip_prefix("nest-").unwrap().to_string();
    assert!(
        flags.contains('i'),
        "nested command-sub $- must keep i, got {out:?}"
    );
}

#[test]
fn command_sub_dollar_dash_omits_i_when_not_interactive() {
    let output = yosh_bin()
        .args(["-c", "echo sub-$(echo $-)"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    let flags = out.trim().strip_prefix("sub-").unwrap().to_string();
    assert!(!flags.contains('i'), "got {out:?}");
}

#[test]
fn double_dash_ends_option_parsing() {
    let dir = std::env::temp_dir().join(format!("yosh-inv-dd-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // A script literally named `-i` must be runnable behind `--`.
    let script = dir.join("-i");
    std::fs::write(&script, "echo ran-a-script-$-\n").unwrap();

    let output = yosh_bin()
        .current_dir(&dir)
        .args(["--", "-i"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("ran-a-script-"), "got {out:?}");
    let flags = out
        .trim()
        .strip_prefix("ran-a-script-")
        .unwrap()
        .to_string();
    assert!(
        !flags.contains('i'),
        "-i after -- is an operand, got {out:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn dash_c_without_argument_errors() {
    let output = yosh_bin().arg("-c").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("-c requires an argument"), "got {err:?}");
}

#[test]
fn dash_c_positional_params_still_work() {
    // Regression guard for the -c dispatch rewrite: $0 and $1 assignment
    // with and without the optional `--` separator.
    for args in [
        vec!["-c", "echo $0:$1", "name", "arg1"],
        vec!["-c", "echo $0:$1", "--", "name", "arg1"],
    ] {
        let output = yosh_bin().args(&args).output().unwrap();
        let out = String::from_utf8_lossy(&output.stdout);
        assert_eq!(out.trim(), "name:arg1", "args {args:?} gave {out:?}");
    }
}
