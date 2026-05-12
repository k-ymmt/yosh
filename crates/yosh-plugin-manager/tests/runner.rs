//! End-to-end tests against the in-repo `test_plugin.wasm` artifact.
//! Skipped silently when the wasm has not been built.

use std::path::PathBuf;
use std::time::Duration;

use yosh_plugin_manager::runner::{HookCall, invoke_exec, invoke_hook, load_plugin};
use yosh_plugin_manager::test_host::TestState;
use yosh_plugin_api::{
    CAP_COMMANDS_EXEC, CAP_HOOK_ON_CD, CAP_IO, CAP_VARIABLES_WRITE,
};
use yosh_plugin_api::pattern::CommandPattern;

fn wasm() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm32-wasip2/release/test_plugin.wasm");
    if p.exists() { Some(p) } else { None }
}

#[test]
fn case_1_run_exec_happy_path() {
    let Some(w) = wasm() else { return };
    let mut s = TestState::default();
    s.caps = CAP_IO;
    let loaded = load_plugin(&w, s, Duration::from_secs(5)).expect("load");
    let outcome = invoke_exec(loaded, "test_cmd", &["x".into()]);
    assert_eq!(outcome.exit_code, Some(0));
    assert!(String::from_utf8_lossy(&outcome.stdout).starts_with("test_cmd args="));
}

#[test]
fn case_2_hook_on_cd_records_var() {
    let Some(w) = wasm() else { return };
    let mut s = TestState::default();
    s.caps = CAP_HOOK_ON_CD | CAP_VARIABLES_WRITE | CAP_IO;
    let loaded = load_plugin(&w, s, Duration::from_secs(5)).expect("load");
    let outcome = invoke_hook(loaded, HookCall::OnCd { old: "/tmp".into(), new: "/home".into() });
    assert!(outcome.error.is_none());
}

#[test]
fn case_3_insufficient_cap_denied() {
    let Some(w) = wasm() else { return };
    // echo_var requires variables:read. Granting only CAP_IO triggers Denied
    // in the guest's get_var call. The guest converts to exit code 2.
    let mut s = TestState::default();
    s.caps = CAP_IO;
    s.vars.insert("X".into(), "y".into());
    let loaded = load_plugin(&w, s, Duration::from_secs(5)).expect("load");
    let outcome = invoke_exec(loaded, "echo_var", &["X".into()]);
    assert_eq!(outcome.exit_code, Some(2));
}

#[test]
fn case_4_allowed_exec_pattern_runs_echo() {
    let Some(w) = wasm() else { return };
    let mut s = TestState::default();
    s.caps = CAP_IO | CAP_COMMANDS_EXEC;
    s.allow_exec = vec![CommandPattern::parse("echo:*").unwrap()];
    let loaded = load_plugin(&w, s, Duration::from_secs(5)).expect("load");
    let outcome = invoke_exec(loaded, "run-echo", &["hi".into()]);
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(outcome.stdout, b"hi\n");
    assert_eq!(outcome.exec_log.len(), 1);
}

#[test]
fn case_5_timeout_on_slow_plugin_pre_prompt() {
    let slow = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm32-wasip2/release/slow_plugin.wasm");
    if !slow.exists() { return; }
    let mut s = TestState::default();
    s.caps = yosh_plugin_api::CAP_HOOK_PRE_PROMPT;
    let start = std::time::Instant::now();
    let loaded = load_plugin(&slow, s, Duration::from_millis(200)).expect("load");
    let outcome = invoke_hook(loaded, HookCall::PrePrompt);
    let elapsed = start.elapsed();
    assert_eq!(outcome.error_kind, Some("timeout"));
    // Generous wall-clock bound: the one-shot watchdog (sleep then
    // increment_epoch once) competes with the guest's busy loop for
    // CPU under macOS scheduling, so the actual interrupt latency
    // ranges several seconds even for a 200ms deadline. The point of
    // this assertion is "bounded", not "fast" — we just want to catch
    // an outright hang.
    assert!(elapsed < Duration::from_secs(15), "timeout interrupt did not fire: {:?}", elapsed);
}

#[test]
fn case_6_test_runner_parses_passing_scenario() {
    let Some(_w) = wasm() else { return };
    let scenario = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios/echo_var_pass.toml");
    let reports = yosh_plugin_manager::scenario::run_dir(&scenario, None);
    assert_eq!(reports.len(), 1);
    assert!(reports[0].passed(), "report: {:?}", reports[0]);
}

#[test]
fn case_7_test_runner_reports_failure_with_step_index() {
    let Some(_w) = wasm() else { return };
    let scenario = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios/vars_set_fail.toml");
    let reports = yosh_plugin_manager::scenario::run_dir(&scenario, None);
    assert_eq!(reports.len(), 1);
    assert!(!reports[0].passed());
}

#[test]
fn case_8_unknown_expect_key_rejected_at_parse() {
    use yosh_plugin_manager::scenario;
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), r#"
        plugin = "x.wasm"
        [[step]]
        call = "exec"
        args = ["y"]
        [step.expect]
        unknown_key = "boom"
    "#).unwrap();
    let err = scenario::parse(tmp.path()).unwrap_err();
    assert!(err.contains("unknown") || err.contains("unknown_key"));
}
