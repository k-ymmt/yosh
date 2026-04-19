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

#[test]
fn test_errexit_trap_action_suppressed() {
    let out = yosh_exec("set -e; trap 'false; echo trap' EXIT; exit 0");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "trap\n");
}
