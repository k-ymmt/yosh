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

// =============================================================================
// Category 2: Pipeline subshell isolation
// =============================================================================

#[test]
fn test_pipeline_variable_isolation() {
    let out = kish_exec("X=original; echo hello | { X=changed; cat >/dev/null; }; echo $X");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "original");
}

#[test]
fn test_pipeline_trap_reset() {
    // Command traps should be reset in pipeline subshell (fixed in Task 1)
    let out = kish_exec("trap 'echo trapped' INT; echo hello | trap; cat >/dev/null");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("trapped"));
    assert!(!stdout.contains("echo trapped"));
}

#[test]
fn test_pipeline_trap_ignore_preserved() {
    let out = kish_exec("trap '' INT; echo hello | trap; cat >/dev/null");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("INT"));
}

#[test]
fn test_pipeline_function_isolation() {
    let out = kish_exec("f() { echo original; }; echo x | f() { echo changed; }; f");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "original");
}

#[test]
fn test_pipeline_cwd_isolation() {
    let out = kish_exec("cd /tmp; echo x | cd /; pwd");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.trim().ends_with("/tmp"));
}

#[test]
fn test_pipeline_option_isolation() {
    let out = kish_exec("set +x; echo x | set -x; echo \"$-\"");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.trim().contains('x'));
}

#[test]
fn test_pipeline_exit_status() {
    let out = kish_exec("false | true; echo $?");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0");
}

#[test]
fn test_pipeline_pipefail() {
    let out = kish_exec("set -o pipefail; false | true; echo $?");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "1");
}

// =============================================================================
// Category 3: Command substitution $(...) isolation
// =============================================================================

#[test]
fn test_cmdsub_variable_isolation() {
    let out = kish_exec("X=original; Y=$(X=changed; echo $X); echo $X $Y");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "original changed");
}

#[test]
fn test_cmdsub_exit_status() {
    let out = kish_exec("X=$(exit 42); echo $?");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "42");
}

#[test]
fn test_cmdsub_nested_isolation() {
    let out = kish_exec("X=outer; Y=$(X=mid; Z=$(X=inner; echo $X); echo $X $Z); echo $X $Y");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "outer mid inner");
}

#[test]
fn test_cmdsub_trap_isolation() {
    let out = kish_exec("trap 'echo parent' INT; X=$(trap); echo \"${X}\"");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // POSIX: $(trap) shows parent traps, not the reset subshell traps
    assert!(stdout.contains("parent"));
}

#[test]
fn test_cmdsub_function_isolation() {
    let out = kish_exec("f() { echo original; }; X=$(f() { echo changed; }; f); f; echo $X");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "original");
    assert_eq!(lines[1], "changed");
}

#[test]
fn test_cmdsub_positional_params_isolation() {
    let out = kish_exec("set -- a b c; X=$(set -- x y; echo $# $1); echo $# $1 $X");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "3 a 2 x");
}

#[test]
fn test_cmdsub_cwd_isolation() {
    let out = kish_exec("cd /tmp; X=$(cd /; pwd); pwd; echo $X");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert!(lines[0].ends_with("/tmp"));
    assert_eq!(lines[1], "/");
}

// =============================================================================
// Category 4: Edge cases
// =============================================================================

#[test]
fn test_nested_subshell() {
    let out = kish_exec("X=1; (X=2; (X=3; echo $X); echo $X); echo $X");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "3");
    assert_eq!(lines[1], "2");
    assert_eq!(lines[2], "1");
}

#[test]
fn test_subshell_exit_no_parent() {
    let out = kish_exec("(exit 1); echo still_running");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "still_running");
}

#[test]
fn test_subshell_errexit() {
    // set -e in subshell should not affect parent
    let out = kish_exec("(set -e; false); echo $?");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "1");
}

#[test]
fn test_subshell_errexit_inherited() {
    // Parent's set -e should be inherited by subshell
    let out = kish_exec("set -e; (false); echo unreachable");
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("unreachable"));
}

#[test]
fn test_umask_inheritance() {
    let out = kish_exec("umask 027; (umask)");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0027");
}

#[test]
fn test_umask_isolation() {
    let out = kish_exec("umask 022; (umask 077); umask");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0022");
}

#[test]
#[ignore = "exec N>file fd persistence not yet implemented in kish"]
fn test_fd_inheritance() {
    // exec 3>file should open fd 3 persistently so subshells can write to it.
    // kish currently restores redirects applied to builtins, so exec 3>file
    // does not persist the fd.
    let out = kish_exec("exec 3>/tmp/kish-fd-test-$$; (echo hello >&3); cat /tmp/kish-fd-test-$$; rm -f /tmp/kish-fd-test-$$");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello");
}

#[test]
fn test_export_and_non_export_in_subshell() {
    let out = kish_exec("A=exported; export A; B=local; (echo $A $B)");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "exported local");
}

#[test]
fn test_last_bg_pid_inheritance() {
    let out = kish_exec("true & PARENT_BG=$!; CHILD_BG=$(echo $!); echo \"$PARENT_BG $CHILD_BG\"");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parts: Vec<&str> = stdout.trim().split_whitespace().collect();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0], parts[1]);
}

#[test]
fn test_deeply_nested_isolation() {
    let out = kish_exec("X=0; (X=1; (X=2; (X=3; echo $X); echo $X); echo $X); echo $X");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines, vec!["3", "2", "1", "0"]);
}

#[test]
fn test_background_command_trap_reset() {
    // Background commands run in subshell; command traps should be reset
    let out = kish_exec("trap 'echo trapped' INT; true & wait; trap");
    assert!(out.status.success());
    // Parent's trap should still be set (background didn't affect it)
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("echo trapped"));
}

#[test]
fn test_umask_octal_display() {
    let out = kish_exec("umask 027; umask");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0027");
}

#[test]
fn test_umask_set_octal() {
    let out = kish_exec("umask 077; umask");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0077");
}
