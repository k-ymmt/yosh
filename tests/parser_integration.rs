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

fn kish_exec_with_args(input: &str, args: &[&str]) -> std::process::Output {
    // POSIX: sh -c cmd [name [arg...]]
    // We pass "kish" as $0 (name), then the test args as $1, $2, ...
    let mut cmd_args = vec!["-c", input, "--", "kish"];
    cmd_args.extend_from_slice(args);
    Command::new(env!("CARGO_BIN_EXE_kish"))
        .args(cmd_args)
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

// ── command substitution tests ───────────────────────────────────────────────

#[test]
fn test_command_substitution() {
    let out = kish_exec("echo $(echo hello)");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_command_sub_strips_trailing_newlines() {
    let out = kish_exec("echo \"x$(echo hello)x\"");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "xhellox\n");
}

#[test]
fn test_command_sub_exit_status() {
    let out = kish_exec("x=$(false); echo $?");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
}

#[test]
fn test_command_sub_in_assignment() {
    let out = kish_exec("x=$(echo hello); echo $x");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

// ── arithmetic expansion tests ───────────────────────────────────────────────

#[test]
fn test_arithmetic_expansion() {
    let out = kish_exec("echo $((2 + 3 * 4))");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "14\n");
}

#[test]
fn test_arithmetic_with_variables() {
    let out = kish_exec("x=10; y=3; echo $((x + y))");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "13\n");
}

// --- Phase 3: Full expansion integration tests ---

#[test]
fn test_arithmetic_hex_full() {
    let out = kish_exec("echo $((0xFF))");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "255\n");
}

#[test]
fn test_param_assign_full() {
    let out = kish_exec("echo ${x:=hello}; echo $x");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\nhello\n");
}

#[test]
fn test_param_alt_full() {
    let out = kish_exec("x=set; echo ${x:+alt}; echo ${y:+alt}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "alt\n\n");
}

#[test]
fn test_param_strip_suffix_full() {
    let out = kish_exec("f=/path/to/file.txt; echo ${f%.txt}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "/path/to/file\n");
}

#[test]
fn test_param_strip_long_prefix_full() {
    let out = kish_exec("f=/path/to/file.txt; echo ${f##*/}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "file.txt\n");
}

#[test]
fn test_param_length_full() {
    let out = kish_exec("x=hello; echo ${#x}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n");
}

#[test]
fn test_quoted_glob_no_expansion_full() {
    let out = kish_exec("echo 'src/*.rs'");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "src/*.rs");
}

#[test]
fn test_tilde_expansion_full() {
    let out = kish_exec("echo ~");
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(stdout.starts_with('/'), "tilde should expand to home dir, got: {}", stdout);
}

#[test]
fn test_dollar_at_in_script_full() {
    let tmp = helpers::TempDir::new();
    let script = tmp.write_file("args.sh", "echo \"$@\"\n");
    let output = Command::new(env!("CARGO_BIN_EXE_kish"))
        .args([script.to_str().unwrap(), "a", "b", "c"])
        .output().expect("failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.trim() == "a b c", "got: {}", stdout);
}

#[test]
fn test_param_default_full() {
    let out = kish_exec("echo ${UNSET_XYZ:-fallback}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "fallback\n");

    let out = kish_exec("X=value; echo ${X:-fallback}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "value\n");
}

#[test]
fn test_param_error_full() {
    // Use a quoted word so the lexer accepts the space inside ${...:?...}
    let out = kish_exec("echo ${UNSET_XYZ:?\"custom error\"}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("custom error"), "stderr: {}", stderr);
}

#[test]
fn test_nested_expansion() {
    // Variable in command substitution
    let out = kish_exec("x=hello; echo $(echo $x)");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_arithmetic_assign_persists() {
    let out = kish_exec("echo $((x = 42)); echo $x");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n42\n");
}

#[test]
fn test_complex_expansion_pipeline() {
    // Combines multiple expansion types
    let out = kish_exec("x=hello; echo \"$x $(echo world) $((1+2))\"");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello world 3\n");
}

#[test]
fn test_script_with_expansions() {
    let tmp = helpers::TempDir::new();
    let script = tmp.write_file("test.sh",
        "x=hello\ny=$(echo world)\necho \"$x $y $((2+2))\"\n");
    let output = Command::new(env!("CARGO_BIN_EXE_kish"))
        .arg(script.to_str().unwrap())
        .output().expect("failed");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hello world 4\n");
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

// ── Phase 4: Here-document I/O tests ─────────────────────────────────────────

#[test]
fn test_heredoc_basic() {
    let out = kish_exec("cat <<EOF\nhello world\nEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello world\n");
}

#[test]
fn test_heredoc_multiline() {
    let out = kish_exec("cat <<EOF\nline1\nline2\nline3\nEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "line1\nline2\nline3\n");
}

#[test]
fn test_heredoc_with_variable_expansion() {
    let out = kish_exec("FOO=hello; cat <<EOF\nvalue is $FOO\nEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "value is hello\n");
}

#[test]
fn test_heredoc_quoted_delimiter_no_expansion() {
    let out = kish_exec("FOO=hello; cat <<'EOF'\nvalue is $FOO\nEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "value is $FOO\n");
}

#[test]
fn test_heredoc_strip_tabs() {
    let out = kish_exec("cat <<-EOF\n\thello\n\tworld\n\tEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\nworld\n");
}

#[test]
fn test_heredoc_with_command_sub() {
    let out = kish_exec("x=$(cat <<EOF\nhello\nEOF\n); echo $x");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_heredoc_empty_body() {
    let out = kish_exec("cat <<EOF\nEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_heredoc_pipeline() {
    let out = kish_exec("cat <<EOF | tr a-z A-Z\nhello world\nEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "HELLO WORLD\n");
}

#[test]
fn test_heredoc_pipeline_three_stages() {
    let out = kish_exec("cat <<EOF | tr a-z A-Z | sed 's/HELLO/HI/'\nhello world\nEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "HI WORLD\n");
}

#[test]
fn test_heredoc_pipeline_middle_command() {
    let out = kish_exec("echo start | cat <<EOF | tr a-z A-Z\nmiddle\nEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "MIDDLE\n");
}

#[test]
fn test_heredoc_pipeline_strip_tabs() {
    let out = kish_exec("cat <<-EOF | tr a-z A-Z\n\thello\n\tEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "HELLO\n");
}

#[test]
fn test_heredoc_pipeline_variable_expansion() {
    let out = kish_exec("X=test; cat <<EOF | tr a-z A-Z\nvalue is $X\nEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "VALUE IS TEST\n");
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

// ── Phase 5: Control structure execution tests ──────────────────────────────

// ── brace group ──

#[test]
fn test_exec_brace_group() {
    let out = kish_exec("{ echo hello; echo world; }");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\nworld\n");
}

#[test]
fn test_exec_brace_group_exit_status() {
    assert!(kish_exec("{ true; }").status.success());
    assert!(!kish_exec("{ false; }").status.success());
}

#[test]
fn test_exec_brace_group_shares_env() {
    let out = kish_exec("x=hello; { x=world; }; echo $x");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "world\n");
}

// ── if/elif/else ──

#[test]
fn test_exec_if_true() {
    let out = kish_exec("if true; then echo yes; fi");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "yes\n");
}

#[test]
fn test_exec_if_false() {
    let out = kish_exec("if false; then echo yes; fi");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_exec_if_else() {
    let out = kish_exec("if false; then echo no; else echo yes; fi");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "yes\n");
}

#[test]
fn test_exec_if_elif() {
    let out = kish_exec("if false; then echo 1; elif true; then echo 2; elif true; then echo 3; fi");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n");
}

#[test]
fn test_exec_if_elif_else() {
    let out = kish_exec("if false; then echo 1; elif false; then echo 2; else echo 3; fi");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n");
}

#[test]
fn test_exec_if_exit_status() {
    assert!(kish_exec("if false; then echo no; fi").status.success());
}

#[test]
fn test_exec_nested_if() {
    let out = kish_exec("if true; then if false; then echo no; else echo yes; fi; fi");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "yes\n");
}

// ── while/until ──

#[test]
fn test_exec_while_loop() {
    let out = kish_exec("x=0; while test $x -lt 3; do echo $x; x=$((x + 1)); done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n1\n2\n");
}

#[test]
fn test_exec_while_false_no_exec() {
    let out = kish_exec("while false; do echo never; done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_exec_until_loop() {
    let out = kish_exec("x=0; until test $x -ge 3; do echo $x; x=$((x + 1)); done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n1\n2\n");
}

#[test]
fn test_exec_until_true_no_exec() {
    let out = kish_exec("until true; do echo never; done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

// ── for loop ──

#[test]
fn test_exec_for_loop() {
    let out = kish_exec("for i in a b c; do echo $i; done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\nb\nc\n");
}

#[test]
fn test_exec_for_empty_list() {
    let out = kish_exec("for i in; do echo $i; done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_exec_for_with_expansion() {
    let out = kish_exec("items='x y z'; for i in $items; do echo $i; done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "x\ny\nz\n");
}

#[test]
fn test_exec_for_default_positional_params() {
    let tmp = helpers::TempDir::new();
    let script = tmp.write_file("test.sh", "for i; do echo $i; done\n");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_kish"))
        .args([script.to_str().unwrap(), "hello", "world"])
        .output()
        .expect("failed");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hello\nworld\n");
}

#[test]
fn test_exec_nested_for() {
    let out = kish_exec("for i in 1 2; do for j in a b; do echo $i$j; done; done");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "1a\n1b\n2a\n2b\n"
    );
}

// ── break/continue ──

#[test]
fn test_exec_break() {
    let out = kish_exec("for i in 1 2 3; do if test $i = 2; then break; fi; echo $i; done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
}

#[test]
fn test_exec_continue() {
    let out = kish_exec("for i in 1 2 3; do if test $i = 2; then continue; fi; echo $i; done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n3\n");
}

#[test]
fn test_exec_break_nested() {
    let out = kish_exec(
        "for i in 1 2; do for j in a b c; do if test $j = b; then break 2; fi; echo $i$j; done; done",
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1a\n");
}

#[test]
fn test_exec_continue_nested() {
    let out = kish_exec(
        "for i in 1 2; do for j in a b; do if test $j = b; then continue 2; fi; echo $i$j; done; done",
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1a\n2a\n");
}

#[test]
fn test_exec_break_while() {
    let out = kish_exec("x=0; while true; do x=$((x+1)); if test $x = 3; then break; fi; echo $x; done");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n2\n");
}

// ── case ──

#[test]
fn test_exec_case_basic() {
    let out = kish_exec("case foo in foo) echo yes;; bar) echo no;; esac");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "yes\n");
}

#[test]
fn test_exec_case_no_match() {
    let out = kish_exec("case baz in foo) echo no;; bar) echo no;; esac");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_exec_case_glob_pattern() {
    let out = kish_exec("case hello in h*) echo matched;; esac");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "matched\n");
}

#[test]
fn test_exec_case_multiple_patterns() {
    let out = kish_exec("case bar in foo|bar|baz) echo matched;; esac");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "matched\n");
}

#[test]
fn test_exec_case_default() {
    let out = kish_exec("case xyz in foo) echo no;; *) echo default;; esac");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "default\n");
}

#[test]
fn test_exec_case_with_variable() {
    let out = kish_exec("x=hello; case $x in hello) echo yes;; esac");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "yes\n");
}

#[test]
fn test_exec_case_fallthrough() {
    let out = kish_exec("case a in a) echo first;& b) echo second;; c) echo third;; esac");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "first\nsecond\n");
}

// ── functions ──

#[test]
fn test_exec_function_basic() {
    let out = kish_exec("greet() { echo hello; }; greet");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_exec_function_args() {
    let out = kish_exec("greet() { echo \"hello $1\"; }; greet world");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello world\n");
}

#[test]
fn test_exec_function_dollar_at() {
    let out = kish_exec("show() { echo \"$@\"; }; show a b c");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a b c\n");
}

#[test]
fn test_exec_function_return() {
    let out = kish_exec("myfn() { return 42; echo never; }; myfn; echo $?");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn test_exec_function_return_default() {
    let out = kish_exec("myfn() { true; }; myfn; echo $?");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n");
}

#[test]
fn test_exec_function_recursion() {
    // Note: use temp variable before arithmetic because $N inside $((...)) is a known limitation
    let out = kish_exec(
        "countdown() { if test $1 -gt 0; then echo $1; x=$1; countdown $((x - 1)); fi; }; countdown 3",
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n2\n1\n");
}

#[test]
fn test_exec_function_global_vars() {
    let out = kish_exec("x=before; setx() { x=after; }; setx; echo $x");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "after\n");
}

#[test]
fn test_exec_function_restores_positional_params() {
    let tmp = helpers::TempDir::new();
    let script = tmp.write_file(
        "test.sh",
        "show() { echo \"func: $1\"; }; show inner; echo \"script: $1\"\n",
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_kish"))
        .args([script.to_str().unwrap(), "outer"])
        .output()
        .expect("failed");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "func: inner\nscript: outer\n"
    );
}

// ── subshell ──

#[test]
fn test_exec_subshell_basic() {
    let out = kish_exec("(echo hello)");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_exec_subshell_isolation() {
    let out = kish_exec("x=before; (x=after; echo $x); echo $x");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "after\nbefore\n"
    );
}

#[test]
fn test_exec_subshell_exit_status() {
    assert!(kish_exec("(true)").status.success());
    assert!(!kish_exec("(false)").status.success());
}

// ── compound command redirects ──

#[test]
fn test_exec_brace_group_redirect() {
    let tmp = helpers::TempDir::new();
    let outfile = tmp.path().join("out.txt");
    kish_exec(&format!("{{ echo hello; echo world; }} > {}", outfile.display()));
    assert_eq!(
        std::fs::read_to_string(&outfile).unwrap(),
        "hello\nworld\n"
    );
}

#[test]
fn test_exec_if_redirect() {
    let tmp = helpers::TempDir::new();
    let outfile = tmp.path().join("out.txt");
    kish_exec(&format!(
        "if true; then echo yes; fi > {}",
        outfile.display()
    ));
    assert_eq!(std::fs::read_to_string(&outfile).unwrap(), "yes\n");
}

#[test]
fn test_exec_for_redirect() {
    let tmp = helpers::TempDir::new();
    let outfile = tmp.path().join("out.txt");
    kish_exec(&format!(
        "for i in a b; do echo $i; done > {}",
        outfile.display()
    ));
    assert_eq!(std::fs::read_to_string(&outfile).unwrap(), "a\nb\n");
}

// ── complex / combined tests ──

#[test]
fn test_exec_if_with_pipeline_condition() {
    let out = kish_exec("if echo hello | grep -q hello; then echo found; fi");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "found\n");
}

#[test]
fn test_exec_for_in_function() {
    let out = kish_exec("each() { for i in \"$@\"; do echo $i; done; }; each x y z");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "x\ny\nz\n");
}

#[test]
fn test_exec_case_in_loop() {
    let out = kish_exec(
        "for f in a.txt b.rs c.txt; do case $f in *.txt) echo $f;; esac; done",
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a.txt\nc.txt\n");
}

#[test]
fn test_exec_nested_control_structures() {
    let out = kish_exec(
        "if true; then for i in 1 2 3; do case $i in 2) echo two;; *) echo other;; esac; done; fi",
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "other\ntwo\nother\n"
    );
}

#[test]
fn test_exec_function_with_control() {
    let out = kish_exec(
        "first_match() { for i in \"$@\"; do if test $i = target; then echo found; return 0; fi; done; return 1; }; first_match a b target c; echo $?",
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "found\n0\n");
}

#[test]
fn test_exec_sum_with_for() {
    let out = kish_exec(
        "sum=0; for i in 1 2 3 4 5; do sum=$((sum + i)); done; echo $sum",
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "15\n");
}

#[test]
fn test_exec_script_with_functions() {
    let tmp = helpers::TempDir::new();
    let script = tmp.write_file(
        "test.sh",
        "greet() {\n  echo \"Hello, $1!\"\n}\nfor name in Alice Bob; do\n  greet $name\ndone\n",
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_kish"))
        .arg(script.to_str().unwrap())
        .output()
        .expect("failed");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Hello, Alice!\nHello, Bob!\n"
    );
}

// ── Phase 6: Special builtins tests ─────────────────────────────────────────

// ── set ──

#[test]
fn test_set_positional_params() {
    let out = kish_exec("set -- a b c; echo $1 $2 $3");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a b c\n");
}

#[test]
fn test_set_enable_option() {
    let out = kish_exec("set -f; echo *");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "*\n");
}

#[test]
fn test_set_dash_o_display() {
    let out = kish_exec("set -o");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("allexport"));
    assert!(stdout.contains("off"));
}

#[test]
fn test_set_no_args_displays_vars() {
    let out = kish_exec("X=hello; set");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("X=hello"));
}

// ── eval ──

#[test]
fn test_eval_simple() {
    let out = kish_exec("eval 'echo hello'");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_eval_variable_expansion() {
    let out = kish_exec("CMD='echo world'; eval $CMD");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "world\n");
}

#[test]
fn test_eval_empty() {
    let out = kish_exec("eval; echo $?");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n");
}

// ── exec ──

#[test]
fn test_exec_replaces_process() {
    // Use /bin/echo which is available on macOS and Linux
    let echo_path = if std::path::Path::new("/bin/echo").exists() {
        "/bin/echo"
    } else {
        "/usr/bin/echo"
    };
    let cmd = format!("exec {} replaced", echo_path);
    let out = kish_exec(&cmd);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "replaced\n");
}

#[test]
fn test_exec_no_args() {
    let out = kish_exec("exec; echo still here");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "still here\n");
}

// ── trap ──

#[test]
fn test_trap_exit() {
    let out = kish_exec("trap 'echo goodbye' EXIT; echo hello");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout, "hello\ngoodbye\n");
}

#[test]
fn test_trap_display() {
    let out = kish_exec("trap 'echo bye' EXIT; trap");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("trap -- 'echo bye' EXIT"));
}

#[test]
fn test_trap_reset() {
    let out = kish_exec("trap 'echo bye' EXIT; trap - EXIT; echo hello");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout, "hello\n");
}

// ── source ──

#[test]
fn test_source_file() {
    let dir = helpers::TempDir::new();
    let script = dir.write_file("lib.sh", "MY_SOURCE_VAR=sourced\n");
    let cmd = format!(". {}; echo $MY_SOURCE_VAR", script.display());
    let out = kish_exec(&cmd);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "sourced\n");
}

#[test]
fn test_source_not_found() {
    let out = kish_exec(". /nonexistent/file.sh");
    assert!(!out.status.success());
}

// ── shift ──

#[test]
fn test_shift_default() {
    let out = kish_exec_with_args("shift; echo $1 $2", &["a", "b", "c"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "b c\n");
}

#[test]
fn test_shift_n() {
    let out = kish_exec_with_args("shift 2; echo $1", &["a", "b", "c"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "c\n");
}

#[test]
fn test_shift_too_many() {
    let out = kish_exec_with_args("shift 5; echo $?", &["a", "b"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout, "1\n");
}

// ── times ──

#[test]
fn test_times() {
    let out = kish_exec("times");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("m"));
}

// ── shell option behaviors (phase 6) ────────────────────────────────────────

#[test]
fn test_dash_parameter() {
    let out = kish_exec("set -x; echo $-");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.trim().contains('x'));
}

#[test]
fn test_nounset() {
    let out = kish_exec("set -u; echo $UNDEFINED_VAR_XYZ");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("UNDEFINED_VAR_XYZ"));
}

#[test]
fn test_noclobber() {
    let dir = helpers::TempDir::new();
    let file = dir.write_file("existing.txt", "original");
    let cmd = format!("set -C; echo new > {}", file.display());
    let out = kish_exec(&cmd);
    assert!(!out.status.success());
    let content = std::fs::read_to_string(&file).unwrap();
    assert_eq!(content, "original");
}

#[test]
fn test_noclobber_override() {
    let dir = helpers::TempDir::new();
    let file = dir.write_file("existing.txt", "original");
    let cmd = format!("set -C; echo new >| {}", file.display());
    let out = kish_exec(&cmd);
    assert!(out.status.success());
    let content = std::fs::read_to_string(&file).unwrap();
    assert_eq!(content, "new\n");
}

#[test]
fn test_xtrace() {
    let out = kish_exec("set -x; echo hello");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("+ echo hello"));
}

#[test]
fn test_allexport() {
    let out = kish_exec("set -a; MY_AE_VAR=exported; /usr/bin/env | grep MY_AE_VAR");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("MY_AE_VAR=exported"));
}

// ── Alias expansion tests ───────────────────────────────────────────────────

#[test]
fn test_alias_basic() {
    let out = kish_exec("alias greet='echo hello'\ngreet");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_alias_with_args() {
    let out = kish_exec("alias say='echo'\nsay world");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "world\n");
}

#[test]
fn test_alias_recursive_prevention() {
    let out = kish_exec("alias ls='echo ls called'\nls");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ls called\n");
}

#[test]
fn test_alias_trailing_space_chain() {
    let out = kish_exec("alias run='echo '\nalias world='hello'\nrun world");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_alias_display() {
    let out = kish_exec("alias ll='ls -l'\nalias ll");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("alias ll='ls -l'"));
}

#[test]
fn test_unalias() {
    let out = kish_exec("alias greet='echo hi'\nunalias greet\nalias greet");
    assert!(!out.status.success());
}

#[test]
fn test_unalias_all() {
    let out = kish_exec("alias a='echo a'\nalias b='echo b'\nunalias -a\nalias");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.is_empty());
}

#[test]
fn test_alias_in_pipeline() {
    let out = kish_exec("alias greet='echo hello world'\ngreet | tr ' ' '\\n' | sort");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("hello"));
    assert!(stdout.contains("world"));
}

#[test]
fn test_alias_after_semicolon() {
    // Alias should be expanded after ; (new command position)
    let out = kish_exec("alias greet='echo hello'\necho start; greet");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "start\nhello\n");
}

#[test]
fn test_alias_not_in_second_word() {
    // Alias should NOT expand in non-command position
    let out = kish_exec("alias world='EXPANDED'\necho world");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "world\n");
}

#[test]
fn test_alias_via_eval() {
    let out = kish_exec("alias greet='echo hello'\neval greet");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_alias_multiword_value() {
    let out = kish_exec("alias greet='echo hello world'\ngreet");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello world\n");
}

#[test]
fn test_alias_with_redirect() {
    let tmp = helpers::TempDir::new();
    let outfile = tmp.path().join("out.txt");
    let cmd = format!("alias greet='echo hello'\ngreet > {}", outfile.display());
    let out = kish_exec(&cmd);
    assert!(out.status.success());
    let content = std::fs::read_to_string(&outfile).unwrap();
    assert_eq!(content, "hello\n");
}

// ── prefix assignment POSIX compliance ──────────────────────────────────────

#[test]
fn test_special_builtin_assignment_persists() {
    // VAR=val on a special builtin should persist
    let out = kish_exec("MY_SP_VAR=hello :; echo $MY_SP_VAR");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_assignment_only_sets_var() {
    let out = kish_exec("MY_ASSIGN_VAR=world; echo $MY_ASSIGN_VAR");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "world\n");
}

#[test]
fn test_external_cmd_assignment_does_not_persist() {
    let out = kish_exec("MY_EXT_VAR=hello /usr/bin/true; echo \"MY_EXT_VAR=$MY_EXT_VAR\"");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout, "MY_EXT_VAR=\n");
}
