mod helpers;

use std::process::Command;

fn kish_parse(input: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kish"))
        .args(["--parse", input])
        .output()
        .expect("failed to execute kish")
}

#[test]
fn test_parse_simple_pipeline() {
    let out = kish_parse("echo hello | grep h");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn test_parse_and_or_list() {
    let out = kish_parse("true && echo yes || echo no");
    assert!(out.status.success());
}

#[test]
fn test_parse_if_statement() {
    let out = kish_parse("if true; then echo yes; elif false; then echo maybe; else echo no; fi");
    assert!(out.status.success());
}

#[test]
fn test_parse_for_loop() {
    let out = kish_parse("for i in a b c; do echo $i; done");
    assert!(out.status.success());
}

#[test]
fn test_parse_while_loop() {
    let out = kish_parse("while true; do echo loop; break; done");
    assert!(out.status.success());
}

#[test]
fn test_parse_case() {
    let out = kish_parse("case $x in\na) echo a;;\nb|c) echo bc;;\nesac");
    assert!(out.status.success());
}

#[test]
fn test_parse_function_def() {
    let out = kish_parse("myfunc() { echo hello; }");
    assert!(out.status.success());
}

#[test]
fn test_parse_subshell() {
    let out = kish_parse("(echo hello; echo world)");
    assert!(out.status.success());
}

#[test]
fn test_parse_brace_group() {
    let out = kish_parse("{ echo hello; echo world; }");
    assert!(out.status.success());
}

#[test]
fn test_parse_complex_redirects() {
    let out = kish_parse("cmd < in > out 2>&1 >>log");
    assert!(out.status.success());
}

#[test]
fn test_parse_assignments_and_command() {
    let out = kish_parse("FOO=bar BAZ=qux echo hello");
    assert!(out.status.success());
}

#[test]
fn test_parse_command_substitution() {
    let out = kish_parse("echo $(echo hello)");
    assert!(out.status.success());
}

#[test]
fn test_parse_arithmetic_expansion() {
    let out = kish_parse("echo $((1 + 2 * 3))");
    assert!(out.status.success());
}

#[test]
fn test_parse_parameter_expansion() {
    let out = kish_parse("echo ${name:-default} ${#name} ${path%%/*}");
    assert!(out.status.success());
}

#[test]
fn test_parse_nested_structures() {
    let out = kish_parse("if true; then for i in a b; do case $i in a) echo yes;; esac; done; fi");
    assert!(out.status.success());
}

#[test]
fn test_parse_semicolons_and_async() {
    let out = kish_parse("cmd1; cmd2 & cmd3");
    assert!(out.status.success());
}

#[test]
fn test_parse_error_unmatched_quote() {
    let out = kish_parse("echo 'hello");
    assert!(!out.status.success());
}

#[test]
fn test_parse_script_file() {
    let tmp = helpers::TempDir::new();
    let script = tmp.write_file(
        "test.sh",
        "#!/bin/kish\necho hello\nfor i in 1 2 3; do\n  echo $i\ndone\n",
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_kish"))
        .arg(script.to_str().unwrap())
        .output()
        .expect("failed to execute kish");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
