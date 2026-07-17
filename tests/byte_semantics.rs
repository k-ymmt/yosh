//! End-to-end POSIX byte-semantics tests: invalid UTF-8 must be preserved
//! byte-identically through source input, argv, environment values, `read`,
//! command substitution, and `$'\xHH'` escapes.
//!
//! These spawn the built `yosh` binary so the full ingress → internal
//! byteenc encoding → egress pipeline is exercised. Non-UTF-8 *file names*
//! are not covered here because macOS APFS rejects them at creation
//! (EILSEQ); the glob matching path is unit-tested in `src/expand/pathname.rs`.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::process::{Command, Stdio};

fn yosh() -> Command {
    Command::new(env!("CARGO_BIN_EXE_yosh"))
}

fn run_stdout(cmd: &mut Command) -> Vec<u8> {
    let out = cmd.output().expect("spawn yosh");
    assert!(
        out.status.success(),
        "yosh failed: status={:?} stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

#[test]
fn dollar_single_quote_yields_raw_byte() {
    let stdout = run_stdout(yosh().args(["-c", r#"printf '%s' $'\xe9'"#]));
    assert_eq!(stdout, b"\xe9");
}

#[test]
fn dollar_single_quote_mixed_bytes_round_trip() {
    let stdout = run_stdout(yosh().args(["-c", r#"printf '%s' $'a\xff\xe6\x97\xa5b'"#]));
    assert_eq!(stdout, b"a\xff\xe6\x97\xa5b");
}

#[test]
fn argv_positional_param_preserves_bytes() {
    let arg = OsString::from_vec(b"p\xe9q".to_vec());
    let stdout = run_stdout(yosh().args(["-c", r#"printf '%s' "$1""#, "sh"]).arg(&arg));
    assert_eq!(stdout, b"p\xe9q");
}

#[test]
fn inherited_env_value_preserves_bytes() {
    let stdout = run_stdout(
        yosh()
            .args(["-c", r#"printf '%s' "$FOO""#])
            .env("FOO", OsString::from_vec(b"v\xe9\xffw".to_vec())),
    );
    assert_eq!(stdout, b"v\xe9\xffw");
}

#[test]
fn exported_var_reaches_child_process_as_raw_bytes() {
    let stdout = run_stdout(yosh().args([
        "-c",
        r#"FOO=$'\xe9'; export FOO; /bin/sh -c 'printf %s "$FOO"'"#,
    ]));
    assert_eq!(stdout, b"\xe9");
}

#[test]
fn command_substitution_preserves_invalid_bytes() {
    let stdout = run_stdout(yosh().args([
        "-c",
        r#"x=$(printf '%s' $'a\xe9b'); printf '%s' "$x""#,
    ]));
    assert_eq!(stdout, b"a\xe9b");
}

#[test]
fn command_substitution_strips_trailing_newlines_after_invalid_bytes() {
    let stdout = run_stdout(yosh().args([
        "-c",
        r#"x=$(printf '%s\n\n' $'ab\xff'); printf '%s' "$x""#,
    ]));
    assert_eq!(stdout, b"ab\xff");
}

#[test]
fn read_preserves_invalid_bytes() {
    use std::io::Write;
    let mut child = yosh()
        .args(["-c", r#"read x; printf '%s' "$x""#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn yosh");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"a\xe9b\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout, b"a\xe9b");
}

#[test]
fn script_file_with_invalid_bytes_is_preserved() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("inv.sh");
    let mut f = std::fs::File::create(&script).unwrap();
    // The word itself contains a raw invalid byte in the source file.
    f.write_all(b"x=a\xe9b\nprintf '%s' \"$x\"\n").unwrap();
    drop(f);
    let stdout = run_stdout(yosh().arg(&script));
    assert_eq!(stdout, b"a\xe9b");
}

#[test]
fn stdin_script_with_invalid_bytes_is_preserved() {
    use std::io::Write;
    let mut child = yosh()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn yosh");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"printf '%s' a\xe9b\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout, b"a\xe9b");
}

#[test]
fn heredoc_preserves_invalid_bytes() {
    let stdout = run_stdout(yosh().args([
        "-c",
        "cat <<EOF\n$(printf '%s' $'\\xe9')\nEOF",
    ]));
    assert_eq!(stdout, b"\xe9\n");
}

#[test]
fn echo_writes_raw_bytes() {
    let stdout = run_stdout(yosh().args(["-c", r#"echo $'ab\xff'"#]));
    assert_eq!(stdout, b"ab\xff\n");
}

#[test]
fn pattern_question_mark_matches_single_invalid_byte() {
    let stdout = run_stdout(yosh().args([
        "-c",
        r#"x=$'a\xe9'; case $x in a?) echo one;; *) echo other;; esac"#,
    ]));
    assert_eq!(stdout, b"one\n");
}

#[test]
fn length_counts_invalid_byte_as_one_char() {
    let stdout = run_stdout(yosh().args(["-c", r#"x=$'\xe9'; echo ${#x}"#]));
    assert_eq!(stdout, b"1\n");
}

#[test]
fn strip_prefix_works_across_invalid_bytes() {
    let stdout = run_stdout(yosh().args(["-c", r#"x=$'a\xe9b'; printf '%s' "${x#a}""#]));
    assert_eq!(stdout, b"\xe9b");
}

#[test]
fn redirect_round_trips_invalid_bytes_through_file_content() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("out.txt");
    let script = format!(
        r#"printf '%s' $'\xe9\xff' > {p}; cat {p}"#,
        p = file.display()
    );
    let stdout = run_stdout(yosh().args(["-c", &script]));
    assert_eq!(stdout, b"\xe9\xff");
}
