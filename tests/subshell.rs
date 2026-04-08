mod helpers;

use std::process::Command;

fn kish_exec(input: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kish"))
        .args(["-c", input])
        .output()
        .expect("failed to execute kish")
}

// =============================================================================
// Category 1: ( ... ) Subshell isolation
// =============================================================================

#[test]
fn test_subshell_variable_isolation() {
    let out = kish_exec("X=original; (X=changed); echo $X");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "original");
}

#[test]
fn test_subshell_new_variable_isolation() {
    let out = kish_exec("(Y=new; echo $Y); echo \"${Y:-unset}\"");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "new");
    assert_eq!(lines[1], "unset");
}

#[test]
fn test_subshell_function_isolation() {
    let out = kish_exec("f() { echo original; }; (f() { echo changed; }; f); f");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "changed");
    assert_eq!(lines[1], "original");
}

#[test]
fn test_subshell_new_function_isolation() {
    let out = kish_exec("(g() { echo inside; }; g); g 2>/dev/null; echo $?");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "inside");
    assert_eq!(lines[1], "127");
}

#[test]
fn test_subshell_alias_isolation() {
    let out = kish_exec("alias ll='echo parent'; (alias ll='echo child'; alias ll); alias ll");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "alias ll='echo child'");
    assert_eq!(lines[1], "alias ll='echo parent'");
}

#[test]
fn test_subshell_trap_command_reset() {
    let out = kish_exec("trap 'echo trapped' INT; (trap)");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("INT"));
}

#[test]
fn test_subshell_trap_ignore_inherited() {
    let out = kish_exec("trap '' INT; (trap)");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("INT"));
}

#[test]
fn test_subshell_option_isolation() {
    let out = kish_exec("set +x; (set -x); echo \"$-\"");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.trim().contains('x'));
}

#[test]
fn test_subshell_dollar_dollar_is_parent_pid() {
    let out = kish_exec("echo $$; (echo $$)");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], lines[1]);
}

#[test]
fn test_subshell_exit_status_propagation() {
    let out = kish_exec("(exit 42); echo $?");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "42");
}

#[test]
fn test_subshell_readonly_inherited() {
    let out = kish_exec("X=hello; readonly X; (echo $X)");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello");
}

#[test]
fn test_subshell_positional_params_isolation() {
    let out = kish_exec("set -- a b c; (set -- x y; echo $# $1); echo $# $1");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "2 x");
    assert_eq!(lines[1], "3 a");
}

#[test]
fn test_subshell_cwd_inheritance() {
    let out = kish_exec("cd /tmp; (pwd)");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.trim().ends_with("/tmp"));
}

#[test]
fn test_subshell_cwd_isolation() {
    let out = kish_exec("cd /tmp; (cd /); pwd");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.trim().ends_with("/tmp"));
}
