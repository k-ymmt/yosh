//! Invocation-option tests: the `-i` (force interactive) flag, the `$-`
//! interactive-letter snapshot (POSIX XCU sh invocation + 2.5.2), and
//! invocation-time set options (-e/-x/-u/..., -o/+o, -s).
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
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("flags.sh");
    std::fs::write(&script, "echo flags-$-\n").unwrap();

    let with_i = yosh_bin().arg("-i").arg(&script).output().unwrap();
    let out = String::from_utf8_lossy(&with_i.stdout);
    let flags = out.trim().strip_prefix("flags-").unwrap().to_string();
    assert!(flags.contains('i'), "got {out:?}");

    let without_i = yosh_bin().arg(&script).output().unwrap();
    let out = String::from_utf8_lossy(&without_i.stdout);
    let flags = out.trim().strip_prefix("flags-").unwrap().to_string();
    assert!(!flags.contains('i'), "got {out:?}");
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
    let dir = tempfile::tempdir().unwrap();
    // A script literally named `-i` must be runnable behind `--`.
    let script = dir.path().join("-i");
    std::fs::write(&script, "echo ran-a-script-$-\n").unwrap();

    let output = yosh_bin()
        .current_dir(dir.path())
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
}

#[test]
fn dash_c_without_argument_errors() {
    let output = yosh_bin().arg("-c").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("-c requires an argument"), "got {err:?}");
}

#[test]
fn dash_e_enables_errexit() {
    for args in [
        vec!["-e", "-c", "false; echo after"],
        vec!["-ec", "false; echo after"], // clustered
        vec!["-o", "errexit", "-c", "false; echo after"],
        vec!["-oerrexit", "-c", "false; echo after"], // attached -o argument
    ] {
        let output = yosh_bin().args(&args).output().unwrap();
        let out = String::from_utf8_lossy(&output.stdout);
        assert!(!out.contains("after"), "args {args:?} gave {out:?}");
        assert_eq!(output.status.code(), Some(1), "args {args:?}");
    }
}

#[test]
fn plus_e_unsets_earlier_dash_e() {
    let output = yosh_bin()
        .args(["-e", "+e", "-c", "false; echo after"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("after"), "+e must undo -e, got {out:?}");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn dash_x_traces_commands() {
    let output = yosh_bin().args(["-x", "-c", "echo hi"]).output().unwrap();
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("+ echo hi"), "got stderr {err:?}");
}

#[test]
fn dash_u_errors_on_unset_parameter() {
    let output = yosh_bin()
        .args(["-u", "-c", "echo $undefined_var_xyz"])
        .output()
        .unwrap();
    assert_ne!(output.status.code(), Some(0));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("parameter not set"), "got stderr {err:?}");
}

#[test]
fn invocation_flags_appear_in_dollar_dash() {
    let output = yosh_bin()
        .args(["-ef", "-c", "echo flags-$-"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    let flags = out.trim().strip_prefix("flags-").unwrap().to_string();
    for f in ['c', 'e', 'f'] {
        assert!(flags.contains(f), "missing {f} in {out:?}");
    }
}

#[test]
fn dash_e_script_file() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("errexit.sh");
    std::fs::write(&script, "false\necho after\n").unwrap();

    let output = yosh_bin().arg("-e").arg(&script).output().unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(!out.contains("after"), "got {out:?}");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn invalid_option_is_usage_error() {
    for args in [vec!["-z"], vec!["+c", "echo x"], vec!["+s"]] {
        let output = yosh_bin().args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "args {args:?}");
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(err.contains("invalid option"), "args {args:?} gave {err:?}");
        assert!(err.contains("Usage:"), "args {args:?} gave {err:?}");
    }
}

#[test]
fn dash_o_without_argument_errors() {
    let output = yosh_bin().arg("-o").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("option requires an argument"), "got {err:?}");
}

#[test]
fn dash_o_unknown_name_errors() {
    let output = yosh_bin()
        .args(["-o", "bogus", "-c", "echo x"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(!out.contains('x'), "must not execute, got {out:?}");
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("unknown option: bogus"), "got {err:?}");
}

#[test]
fn dash_s_reads_stdin_with_positional_params() {
    let (out, code) = run_with_stdin(&["-s", "one", "two"], "echo p-$1:$2:$#\n");
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "p-one:two:2");
}

#[test]
fn dash_s_keeps_shell_name_as_dollar_zero() {
    // POSIX sh -s: operands are $1..., $0 stays the shell name — an
    // operand must NOT be opened as a script file.
    let (out, code) = run_with_stdin(&["-s", "one"], "echo zero-$0\n");
    assert_eq!(code, 0);
    assert!(out.trim().ends_with("yosh"), "got {out:?}");
}

#[test]
fn dash_s_clusters_with_set_options() {
    let (out, code) = run_with_stdin(&["-es"], "false\necho after\n");
    assert!(!out.contains("after"), "got {out:?}");
    assert_eq!(code, 1);
}

#[test]
fn dash_n_ignored_for_interactive_shells() {
    // POSIX XCU set: "-n ... This option is ignored by interactive
    // shells". Without the guard, -n + interactive made every command
    // (including `exit` and `set +n`) a silent no-op.
    let output = yosh_bin()
        .args(["-i", "-n", "-c", "exit 3"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(3),
        "-n must be ignored when the shell is interactive"
    );

    // Non-interactive -n still means noexec (syntax-check mode).
    let output = yosh_bin()
        .args(["-n", "-c", "echo should-not-print"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.is_empty(), "noexec must not run commands, got {out:?}");
}

#[test]
fn dash_h_with_operand_is_posix_noop() {
    // `-h` alone is the yosh help alias, but with anything after it,
    // it is the POSIX locate-utilities no-op: the script must run.
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("hello.sh");
    std::fs::write(&script, "echo from-script\n").unwrap();

    let output = yosh_bin().arg("-h").arg(&script).output().unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert_eq!(out.trim(), "from-script", "got {out:?}");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn long_option_error_names_whole_argument() {
    let output = yosh_bin().arg("--verbose").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        err.contains("--verbose: invalid option"),
        "must name the whole word, got {err:?}"
    );
}

#[test]
fn dash_s_reports_s_in_dollar_dash() {
    // Explicit -s puts `s` into $- (bash/dash agree); the implicit
    // stdin-pipe path does not (matches bash).
    let (out, _) = run_with_stdin(&["-s"], "echo flags-$-\n");
    let flags = out.trim().strip_prefix("flags-").unwrap().to_string();
    assert!(flags.contains('s'), "-s must put s into $-, got {out:?}");

    let (out, _) = run_with_stdin(&[], "echo flags-$-\n");
    let flags = out.trim().strip_prefix("flags-").unwrap().to_string();
    assert!(!flags.contains('s'), "implicit pipe has no s, got {out:?}");
}

#[test]
fn noexec_ignored_in_command_sub_of_interactive_shell() {
    // Command-substitution children run with is_interactive=false but
    // flag_i=true; -n must stay ignored there too, or every $(...) in
    // an interactive -n shell silently expands to nothing.
    let output = yosh_bin()
        .args(["-i", "-n", "-c", "echo A$(echo B)"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert_eq!(out.trim(), "AB", "command sub must run under -i -n");
}

#[test]
fn dash_sc_reports_both_c_and_s_in_dollar_dash() {
    let output = yosh_bin().args(["-sc", "echo got:$-"]).output().unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    let flags = out.trim().strip_prefix("got:").unwrap().to_string();
    assert!(flags.contains('c'), "got {out:?}");
    assert!(
        flags.contains('s'),
        "-s must survive alongside -c, got {out:?}"
    );
}

/// Run yosh detached from any controlling terminal (`setsid` in the
/// forked child) with piped stdio, feeding `stdin`; returns (stdout,
/// exit code). The monitor-mode ownership gate probes stderr and
/// `/dev/tty` — not just stdin — so piped stdio alone does not
/// simulate "no terminal" when the test harness itself runs on one.
fn run_detached_with_stdin(args: &[&str], stdin: &str) -> (String, i32) {
    use std::os::unix::process::CommandExt;
    let mut cmd = yosh_bin();
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: setsid(2) is async-signal-safe and allocation-free; the
    // forked child is never a process-group leader, so it cannot fail
    // with EPERM in practice.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().unwrap();
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
fn dash_m_without_terminal_disables_monitor() {
    // A shell that does not own a controlling terminal cannot do job
    // control; -m is dropped (and with it the `m` in $-), matching
    // bash. Prevents a background `yosh -m -c ...` from stealing the
    // invoking shell's terminal or being stopped by SIGTTOU.
    let (out, code) = run_detached_with_stdin(&["-m", "-s"], "echo m-$-\n");
    assert_eq!(code, 0);
    let flags = out.trim().strip_prefix("m-").unwrap().to_string();
    assert!(
        !flags.contains('m'),
        "monitor must be disabled without a terminal, got {out:?}"
    );
}

#[test]
fn runtime_set_m_without_terminal_stays_off() {
    // Runtime `set -m` shares the invocation -m terminal-ownership
    // gate: without a controlling terminal the monitor flag reverts,
    // `m` stays out of $-, and `set` itself still succeeds — matching
    // `yosh -m ...` so the two spellings cannot diverge.
    let (out, code) = run_detached_with_stdin(&["-s"], "set -m\necho rc-$?-m-$-\n");
    assert_eq!(code, 0);
    let trimmed = out.trim();
    let rest = trimmed.strip_prefix("rc-0-m-").unwrap_or_else(|| {
        panic!("set -m must succeed even without a terminal, got {out:?}")
    });
    assert!(
        !rest.contains('m'),
        "monitor must stay off without a terminal, got {out:?}"
    );
}

#[test]
fn plus_i_cancels_dash_i_and_last_one_wins() {
    let (out, code) = run_with_stdin(&["-i", "+i"], "echo flags-$-\n");
    assert_eq!(code, 0);
    let flags = out.trim().strip_prefix("flags-").unwrap().to_string();
    assert!(!flags.contains('i'), "+i must cancel -i, got {out:?}");

    let (out, code) = run_with_stdin(&["+i", "-i"], "echo flags-$-\n");
    assert_eq!(code, 0);
    let flags = out.trim().strip_prefix("flags-").unwrap().to_string();
    assert!(flags.contains('i'), "-i after +i must win, got {out:?}");
}

#[test]
fn noexec_stubs_negation_and_async_lists() {
    // The noexec stub must sit above the AND-OR machinery: a trailing
    // `! cmd` must not negate the stub status into exit 1, and `cmd &`
    // must not fork or print a job line (bash -n does neither).
    let (out, code) = run_with_stdin(&["-n", "-s"], "echo a\n! false\n");
    assert_eq!(code, 0, "-n with trailing ! pipeline must exit 0");
    assert!(out.is_empty(), "noexec must not run commands, got {out:?}");

    let output = yosh_bin().args(["-n", "-c", "sleep 0 &"]).output().unwrap();
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        err.is_empty(),
        "-n must not fork/announce jobs, got {err:?}"
    );
}

#[test]
fn background_job_notice_only_with_job_control() {
    // POSIX §2.9.3.1: the "[n] pid" notice belongs to job control;
    // plain non-interactive scripts stay silent (bash/dash agree).
    let output = yosh_bin()
        .args(["-c", "/bin/sleep 0 & wait"])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        !err.contains("[1]"),
        "non-interactive shell must not print job notice, got {err:?}"
    );

    // The interactive flag brings the notice back.
    let output = yosh_bin()
        .args(["-i", "-c", "/bin/sleep 0 & wait"])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        err.contains("[1]"),
        "interactive shell prints the job notice, got {err:?}"
    );
}

#[test]
fn lone_plus_is_consumed_as_noop() {
    // A lone `+` is an empty option cluster (bash agrees): consumed,
    // and option parsing continues with the next argument.
    let (out, code) = run_with_stdin(&["+"], "echo plus-ok\n");
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "plus-ok");

    let (out, code) = run_with_stdin(&["+", "-e"], "false\necho after\n");
    assert!(
        !out.contains("after"),
        "-e after lone + must apply, got {out:?}"
    );
    assert_eq!(code, 1);
}

#[test]
fn invalid_option_error_decodes_raw_bytes() {
    // Raw non-UTF-8 argv bytes are byteenc-escaped internally; the
    // usage error must decode them back instead of leaking the
    // U+10FExx escape codepoints to stderr.
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let output = yosh_bin()
        .arg(OsStr::from_bytes(b"-\xff"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let first_line = output.stderr.split(|&b| b == b'\n').next().unwrap();
    assert!(
        first_line.windows(2).any(|w| w == b"-\xff"),
        "stderr must contain the raw byte, got {:x?}",
        first_line
    );
}

#[test]
fn lone_dash_ends_option_parsing() {
    // POSIX sh: a lone `-` is an obsolescent synonym for `--`.
    let (out, code) = run_with_stdin(&["-"], "echo lone-ok\n");
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "lone-ok");
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
