mod helpers;

use std::process::Command;

fn kish_parse(input: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kish"))
        .args(["--parse", input])
        .output()
        .expect("failed to execute kish")
}

fn kish_exec(input: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kish"))
        .args(["-c", input])
        .output()
        .expect("failed to execute kish")
}

// ── execution tests ──────────────────────────────────────────────────────────

#[test]
fn test_exec_echo() {
    let out = kish_exec("echo hello world");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello world\n");
}

#[test]
fn test_exec_true_false() {
    assert!(kish_exec("true").status.success());
    assert!(!kish_exec("false").status.success());
}

#[test]
fn test_exec_exit_code() {
    assert_eq!(kish_exec("exit 42").status.code(), Some(42));
}

#[test]
fn test_exec_pipeline() {
    let out = kish_exec("echo hello | tr h H");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "Hello\n");
}

#[test]
fn test_exec_pipeline_exit_status() {
    assert!(kish_exec("false | true").status.success());
    assert!(!kish_exec("true | false").status.success());
}

#[test]
fn test_exec_and_list() {
    assert_eq!(String::from_utf8_lossy(&kish_exec("true && echo yes").stdout), "yes\n");
    assert_eq!(String::from_utf8_lossy(&kish_exec("false && echo yes").stdout), "");
}

#[test]
fn test_exec_or_list() {
    assert_eq!(String::from_utf8_lossy(&kish_exec("false || echo fallback").stdout), "fallback\n");
    assert_eq!(String::from_utf8_lossy(&kish_exec("true || echo fallback").stdout), "");
}

#[test]
fn test_exec_semicolon_list() {
    let out = kish_exec("echo first; echo second");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "first\nsecond\n");
}

#[test]
fn test_exec_negated_pipeline() {
    assert!(kish_exec("! false").status.success());
    assert!(!kish_exec("! true").status.success());
}

#[test]
fn test_exec_variable_expansion() {
    let out = kish_exec("FOO=hello; echo $FOO");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_exec_exit_status_variable() {
    let out = kish_exec("false; echo $?");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
}

#[test]
fn test_exec_export() {
    let out = kish_exec("export FOO=bar; echo $FOO");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "bar\n");
}

#[test]
fn test_exec_output_redirect() {
    let tmp = helpers::TempDir::new();
    let outfile = tmp.path().join("out.txt");
    kish_exec(&format!("echo hello > {}", outfile.display()));
    assert_eq!(std::fs::read_to_string(&outfile).unwrap(), "hello\n");
}

#[test]
fn test_exec_append_redirect() {
    let tmp = helpers::TempDir::new();
    let outfile = tmp.path().join("out.txt");
    kish_exec(&format!("echo first > {}; echo second >> {}", outfile.display(), outfile.display()));
    assert_eq!(std::fs::read_to_string(&outfile).unwrap(), "first\nsecond\n");
}

#[test]
fn test_exec_input_redirect() {
    let tmp = helpers::TempDir::new();
    let infile = tmp.write_file("in.txt", "hello from file\n");
    let out = kish_exec(&format!("cat < {}", infile.display()));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello from file\n");
}

#[test]
fn test_exec_command_not_found() {
    assert_eq!(kish_exec("nonexistent_cmd_12345").status.code(), Some(127));
}

#[test]
fn test_exec_script_file() {
    let tmp = helpers::TempDir::new();
    let script = tmp.write_file("test.sh", "echo hello\necho world\n");
    let output = Command::new(env!("CARGO_BIN_EXE_kish"))
        .arg(script.to_str().unwrap())
        .output()
        .expect("failed");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hello\nworld\n");
}

#[test]
fn test_exec_complex_pipeline() {
    let out = kish_exec("echo 'hello world' | tr ' ' '\\n' | sort");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("hello"));
    assert!(stdout.contains("world"));
}

// ── parse tests ──────────────────────────────────────────────────────────────

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
    // Use --parse to verify the script file is syntactically valid
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_kish"))
        .args(["--parse", script.to_str().unwrap()])
        .output()
        .expect("failed to execute kish");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
