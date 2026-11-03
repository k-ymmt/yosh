mod helpers;

use std::process::Command;

fn yosh_exec(input: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_yosh"))
        .args(["-c", input])
        .output()
        .expect("failed to execute yosh")
}

#[test]
fn test_errexit_basic() {
    let out = yosh_exec("set -e; false; echo unreachable");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_errexit_if_condition_suppressed() {
    let out = yosh_exec("set -e; if false; then echo no; fi; echo reached");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "reached\n");
}

#[test]
fn test_errexit_elif_condition_suppressed() {
    let out =
        yosh_exec("set -e; if false; then echo no; elif false; then echo no2; fi; echo reached");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "reached\n");
}

#[test]
fn test_errexit_while_condition_suppressed() {
    let out = yosh_exec("set -e; while false; do :; done; echo reached");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "reached\n");
}

#[test]
fn test_errexit_until_condition_suppressed() {
    let out = yosh_exec("set -e; until true; do :; done; echo reached");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "reached\n");
}

#[test]
fn test_errexit_negated_pipeline_suppressed() {
    let out = yosh_exec("set -e; ! false; echo reached");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "reached\n");
}

#[test]
fn test_errexit_and_or_suppressed() {
    let out = yosh_exec("set -e; false || true; echo reached");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "reached\n");
}

#[test]
fn test_errexit_and_or_final_exits() {
    let out = yosh_exec("set -e; true && false; echo unreachable");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_errexit_nested_suppression() {
    let out = yosh_exec("set -e; if ! false; then echo ok; fi; echo reached");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ok\nreached\n");
}

#[test]
fn test_errexit_subshell() {
    let out = yosh_exec("set -e; (false); echo unreachable");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_errexit_function() {
    let out = yosh_exec("set -e; f() { false; }; f; echo unreachable");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

/// `set -e` applies inside the EXIT trap action (dash and bash agree):
/// the failing command ends the shell with its status, and the rest of
/// the action does not run.
#[test]
fn test_errexit_applies_in_exit_trap_action() {
    let out = yosh_exec("set -e; trap 'false; echo trap' EXIT; exit 0");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
    assert_eq!(out.status.code(), Some(1));
    let out = yosh_exec("set -e; trap 'false; echo intrap' EXIT; true");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
    assert_eq!(out.status.code(), Some(1));
}

/// A trap action is a fresh errexit context: errexit suppression of the
/// interrupted code (`f || true`) does not leak into it.
#[test]
fn test_errexit_trap_action_not_suppressed_by_interrupted_context() {
    let out = yosh_exec("set -e; trap 'false; echo trap' EXIT; f() { exit 0; }; f || true");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
    assert_eq!(out.status.code(), Some(1));
    let out = yosh_exec(
        "set -e; trap 'false; echo trap' USR1; f() { kill -USR1 $$; echo f; }; f || true; echo end",
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
    assert_eq!(out.status.code(), Some(1));
}
