//! Declarative scenarios for `yosh plugin test`. One TOML file per
//! scenario; each scenario is a sequence of `step` entries, each step
//! is one exec / hook invocation plus an `expect` block.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Scenario {
    pub plugin: PathBuf,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub env: EnvConfig,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    #[serde(rename = "step", default)]
    pub steps: Vec<Step>,
}

#[derive(Debug, Default, Deserialize)]
pub struct EnvConfig {
    #[serde(default)]
    pub caps: Vec<String>,
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    #[serde(default)]
    pub exported: Vec<String>,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub allow_exec: Vec<String>,
    #[serde(default)]
    pub sandbox_root: String,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_timeout_ms() -> u64 {
    5000
}

#[derive(Debug, Deserialize)]
#[serde(tag = "call", rename_all = "lowercase")]
pub enum Step {
    Exec {
        args: Vec<String>,
        #[serde(default)]
        expect: Expect,
    },
    Hook {
        name: HookName,
        args: Vec<toml::Value>,
        #[serde(default)]
        expect: Expect,
    },
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum HookName {
    PreExec,
    PostExec,
    OnCd,
    PrePrompt,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    pub exit: Option<i32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub stdout_contains: Option<String>,
    pub stderr_contains: Option<String>,
    pub stdout_regex: Option<String>,
    pub stderr_regex: Option<String>,
    pub vars_set: Option<BTreeMap<String, String>>,
    pub vars_export: Option<BTreeMap<String, String>>,
    pub files_write: Option<BTreeMap<String, FileExpect>>,
    pub exec_called: Option<Vec<ExecCallExpect>>,
    pub trap: Option<bool>,
}

// Note: `denied: bool` (listed in spec §5 as a future expect key) is
// intentionally not implemented here. Observing capability-denied
// errors from the harness requires plumbing a counter through every
// host import (each `Err(Denied)` increments). Deferred — for now,
// authors detect denial via `stdout_regex` on guest-side error
// handling or via specific `exit` codes the guest returns on
// `Err(ErrorCode::Denied)`.

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum FileExpect {
    Bytes(String),
    Struct {
        #[serde(default)]
        len: Option<usize>,
        #[serde(default)]
        bytes_eq: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
pub struct ExecCallExpect {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub exit: Option<i32>,
}

pub fn parse(path: &std::path::Path) -> Result<Scenario, String> {
    let s = std::fs::read_to_string(path).map_err(|e| format!("read {}: {}", path.display(), e))?;
    let parsed: Scenario =
        toml::from_str(&s).map_err(|e| format!("parse {}: {}", path.display(), e))?;
    Ok(parsed)
}

use crate::runner::{HookCall, RunOutcome, invoke_exec, invoke_hook, load_plugin};
use crate::test_host::TestState;
use yosh_plugin_api::pattern::CommandPattern;
use yosh_plugin_api::{capabilities_to_bitflags, parse_capability};

#[derive(Debug)]
pub enum StepResult {
    Pass,
    Fail(String),
}

pub fn run_scenario(path: &std::path::Path) -> Vec<StepResult> {
    let scenario = match parse(path) {
        Ok(s) => s,
        Err(e) => return vec![StepResult::Fail(format!("parse error: {}", e))],
    };

    let wasm_path = path
        .parent()
        .map(|p| p.join(&scenario.plugin))
        .unwrap_or(scenario.plugin.clone());
    let mut results = Vec::new();

    for (idx, step) in scenario.steps.iter().enumerate() {
        let state = build_state(&scenario);
        let timeout = std::time::Duration::from_millis(scenario.env.timeout_ms);
        let loaded = match load_plugin(&wasm_path, state, timeout) {
            Ok(l) => l,
            Err(e) => {
                results.push(StepResult::Fail(format!("step {}: load: {}", idx + 1, e)));
                continue;
            }
        };

        let (outcome, expect) = match step {
            Step::Exec { args, expect } => {
                if args.is_empty() {
                    results.push(StepResult::Fail(format!(
                        "step {}: exec needs at least 1 arg",
                        idx + 1
                    )));
                    continue;
                }
                let (cmd, rest) = (&args[0], &args[1..]);
                (invoke_exec(loaded, cmd, rest), expect)
            }
            Step::Hook { name, args, expect } => {
                let call = match build_hook_call(*name, args) {
                    Ok(c) => c,
                    Err(e) => {
                        results.push(StepResult::Fail(format!(
                            "step {}: hook args: {}",
                            idx + 1,
                            e
                        )));
                        continue;
                    }
                };
                (invoke_hook(loaded, call), expect)
            }
        };

        results.push(evaluate(idx + 1, &outcome, expect));
    }

    if results.is_empty() {
        results.push(StepResult::Pass);
    }
    results
}

fn build_state(scenario: &Scenario) -> TestState {
    let mut state = TestState::default();
    let parsed_caps: Vec<_> = scenario
        .env
        .caps
        .iter()
        .filter_map(|s| parse_capability(s))
        .collect();
    state.caps = capabilities_to_bitflags(&parsed_caps);
    for (k, v) in &scenario.env.vars {
        state.vars.insert(k.clone(), v.clone());
    }
    for k in &scenario.env.exported {
        state.exported.insert(k.clone());
    }
    if !scenario.env.cwd.is_empty() {
        state.cwd = scenario.env.cwd.clone().into();
    }
    state.allow_exec = scenario
        .env
        .allow_exec
        .iter()
        .filter_map(|p| CommandPattern::parse(p).ok())
        .collect();
    if !scenario.env.sandbox_root.is_empty() {
        state.sandbox_root = Some(std::path::PathBuf::from(&scenario.env.sandbox_root));
    } else {
        for (k, v) in &scenario.files {
            state
                .files
                .insert(std::path::PathBuf::from(k), v.as_bytes().to_vec());
        }
    }
    state
}

fn build_hook_call(name: HookName, args: &[toml::Value]) -> Result<HookCall, String> {
    fn s(v: &toml::Value) -> Result<String, String> {
        v.as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "expected string".into())
    }
    fn i(v: &toml::Value) -> Result<i32, String> {
        v.as_integer()
            .map(|i| i as i32)
            .ok_or_else(|| "expected integer".into())
    }
    match name {
        HookName::PreExec => Ok(HookCall::PreExec {
            command_line: s(args.first().ok_or("missing arg")?)?,
        }),
        HookName::PostExec => {
            let cl = s(args.first().ok_or("missing command_line")?)?;
            let ec = i(args.get(1).ok_or("missing exit_code")?)?;
            Ok(HookCall::PostExec {
                command_line: cl,
                exit_code: ec,
            })
        }
        HookName::OnCd => {
            let old = s(args.first().ok_or("missing old")?)?;
            let new = s(args.get(1).ok_or("missing new")?)?;
            Ok(HookCall::OnCd { old, new })
        }
        HookName::PrePrompt => Ok(HookCall::PrePrompt),
    }
}

fn evaluate(step_idx: usize, o: &RunOutcome, e: &Expect) -> StepResult {
    macro_rules! fail {
        ($($t:tt)*) => {{ return StepResult::Fail(format!("step {}: {}", step_idx, format_args!($($t)*))) }};
    }

    if let Some(want) = e.exit {
        match o.exit_code {
            Some(got) if got == want => {}
            Some(got) => fail!("exit: want {}, got {}", want, got),
            None => fail!("exit: want {}, got (no exit code — hook?)", want),
        }
    }

    let stdout_str = String::from_utf8_lossy(&o.stdout);
    let stderr_str = String::from_utf8_lossy(&o.stderr);

    if let Some(want) = &e.stdout
        && stdout_str != *want
    {
        fail!("stdout mismatch: want {:?}, got {:?}", want, stdout_str);
    }
    if let Some(want) = &e.stderr
        && stderr_str != *want
    {
        fail!("stderr mismatch: want {:?}, got {:?}", want, stderr_str);
    }
    if let Some(sub) = &e.stdout_contains
        && !stdout_str.contains(sub.as_str())
    {
        fail!("stdout_contains {:?} not found in {:?}", sub, stdout_str);
    }
    if let Some(sub) = &e.stderr_contains
        && !stderr_str.contains(sub.as_str())
    {
        fail!("stderr_contains {:?} not found in {:?}", sub, stderr_str);
    }
    if let Some(re) = &e.stdout_regex {
        let rx = regex::Regex::new(re).map_err(|err| err.to_string());
        match rx {
            Ok(rx) if !rx.is_match(&stdout_str) => {
                fail!("stdout_regex {:?} did not match {:?}", re, stdout_str)
            }
            Err(err) => fail!("stdout_regex invalid: {}", err),
            _ => {}
        }
    }
    if let Some(re) = &e.stderr_regex {
        let rx = regex::Regex::new(re).map_err(|err| err.to_string());
        match rx {
            Ok(rx) if !rx.is_match(&stderr_str) => {
                fail!("stderr_regex {:?} did not match {:?}", re, stderr_str)
            }
            Err(err) => fail!("stderr_regex invalid: {}", err),
            _ => {}
        }
    }

    if let Some(want) = &e.vars_set {
        let got: BTreeMap<String, String> = o.set_log.iter().cloned().collect();
        if got != *want {
            fail!("vars_set: want {:?}, got {:?}", want, got);
        }
    }
    if let Some(want) = &e.vars_export {
        let got: BTreeMap<String, String> = o.export_log.iter().cloned().collect();
        if got != *want {
            fail!("vars_export: want {:?}, got {:?}", want, got);
        }
    }

    if let Some(want) = &e.files_write {
        let got: BTreeMap<String, usize> = o
            .write_log
            .iter()
            .map(|(p, n)| (p.display().to_string(), *n))
            .collect();
        for (path, expectation) in want {
            match expectation {
                FileExpect::Bytes(b) => {
                    let want_len = b.len();
                    match got.get(path) {
                        Some(actual) if *actual == want_len => {}
                        Some(actual) => fail!(
                            "files_write[{}] len: want {}, got {}",
                            path,
                            want_len,
                            actual
                        ),
                        None => fail!("files_write[{}] not written", path),
                    }
                }
                FileExpect::Struct { len, bytes_eq } => {
                    if let Some(l) = len {
                        match got.get(path) {
                            Some(actual) if *actual == *l => {}
                            Some(actual) => {
                                fail!("files_write[{}] len: want {}, got {}", path, l, actual)
                            }
                            None => fail!("files_write[{}] not written", path),
                        }
                    }
                    if let Some(b) = bytes_eq {
                        let want_len = b.len();
                        match got.get(path) {
                            Some(actual) if *actual == want_len => {}
                            Some(actual) => fail!(
                                "files_write[{}] bytes_eq len: want {}, got {}",
                                path,
                                want_len,
                                actual
                            ),
                            None => fail!("files_write[{}] not written", path),
                        }
                    }
                }
            }
        }
    }

    if let Some(want_seq) = &e.exec_called {
        if want_seq.len() != o.exec_log.len() {
            fail!(
                "exec_called: want {} calls, got {}",
                want_seq.len(),
                o.exec_log.len()
            );
        }
        for (i, (w, g)) in want_seq.iter().zip(o.exec_log.iter()).enumerate() {
            if w.program != g.program {
                fail!(
                    "exec_called[{}].program: want {}, got {}",
                    i,
                    w.program,
                    g.program
                );
            }
            if w.args != g.args {
                fail!(
                    "exec_called[{}].args: want {:?}, got {:?}",
                    i,
                    w.args,
                    g.args
                );
            }
            if let Some(exit) = w.exit
                && exit != g.exit_code
            {
                fail!(
                    "exec_called[{}].exit: want {}, got {}",
                    i,
                    exit,
                    g.exit_code
                );
            }
        }
    }

    if let Some(want) = e.trap {
        let got = o.error_kind == Some("trap");
        if got != want {
            fail!("trap: want {}, got {}", want, got);
        }
    }

    StepResult::Pass
}

#[cfg(test)]
mod evaluator_tests {
    use super::*;
    use crate::runner::RunOutcome;

    fn outcome_with(exit: Option<i32>, stdout: &[u8]) -> RunOutcome {
        RunOutcome {
            exit_code: exit,
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
            set_log: Vec::new(),
            export_log: Vec::new(),
            write_log: Vec::new(),
            exec_log: Vec::new(),
            error: None,
            error_kind: None,
        }
    }

    #[test]
    fn expect_exit_match_passes() {
        let o = outcome_with(Some(0), b"");
        let e = Expect {
            exit: Some(0),
            ..Default::default()
        };
        assert!(matches!(evaluate(1, &o, &e), StepResult::Pass));
    }

    #[test]
    fn expect_exit_mismatch_fails() {
        let o = outcome_with(Some(2), b"");
        let e = Expect {
            exit: Some(0),
            ..Default::default()
        };
        match evaluate(1, &o, &e) {
            StepResult::Fail(s) => assert!(s.contains("exit")),
            _ => panic!("expected fail"),
        }
    }

    #[test]
    fn expect_stdout_contains_works() {
        let o = outcome_with(Some(0), b"hello world\n");
        let e = Expect {
            stdout_contains: Some("world".into()),
            ..Default::default()
        };
        assert!(matches!(evaluate(1, &o, &e), StepResult::Pass));
    }

    #[test]
    fn run_dir_collects_toml_files() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.toml");
        std::fs::write(
            &a,
            r#"
            plugin = "missing.wasm"
            [[step]]
            call = "exec"
            args = ["x"]
        "#,
        )
        .unwrap();
        let reports = run_dir(tmp.path(), None);
        assert_eq!(reports.len(), 1);
        assert!(!reports[0].passed()); // wasm missing
    }

    #[test]
    fn format_summary_json_has_summary_line() {
        let reports = vec![];
        let s = format_summary_json(&reports);
        assert!(s.contains("\"summary\""));
    }
}

#[derive(Debug)]
pub struct ScenarioReport {
    pub file: std::path::PathBuf,
    pub steps: Vec<StepResult>,
}

impl ScenarioReport {
    pub fn passed(&self) -> bool {
        self.steps.iter().all(|r| matches!(r, StepResult::Pass))
    }
}

pub fn run_dir(path: &std::path::Path, filter: Option<&str>) -> Vec<ScenarioReport> {
    let mut reports = Vec::new();
    let filter_rx = filter.and_then(|f| match regex::Regex::new(f) {
        Ok(rx) => Some(rx),
        Err(e) => {
            eprintln!(
                "yosh-plugin: ignoring invalid --filter regex {:?}: {}",
                f, e
            );
            None
        }
    });

    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("toml") {
                out.push(p);
            }
        }
    }

    let mut paths = Vec::new();
    if path.is_dir() {
        walk(path, &mut paths);
    } else if path.exists() {
        paths.push(path.to_path_buf());
    }
    paths.sort();

    for p in paths {
        if let Some(rx) = &filter_rx
            && !rx.is_match(&p.to_string_lossy())
        {
            continue;
        }
        let results = run_scenario(&p);
        reports.push(ScenarioReport {
            file: p,
            steps: results,
        });
    }
    reports
}

pub fn format_summary_human(reports: &[ScenarioReport]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "running {} scenarios", reports.len());
    let mut passed = 0;
    let mut failed = 0;
    for r in reports {
        if r.passed() {
            passed += 1;
            let _ = writeln!(out, "  \u{2713} {}", r.file.display());
        } else {
            failed += 1;
            let _ = writeln!(out, "  \u{2717} {}", r.file.display());
            for s in &r.steps {
                if let StepResult::Fail(msg) = s {
                    let _ = writeln!(out, "      {}", msg);
                }
            }
        }
    }
    let _ = writeln!(out, "{} passed, {} failed", passed, failed);
    out
}

pub fn format_summary_json(reports: &[ScenarioReport]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let mut passed = 0;
    let mut failed = 0;
    for r in reports {
        if r.passed() {
            passed += 1;
            let _ = writeln!(
                out,
                "{}",
                serde_json::json!({
                    "file": r.file.display().to_string(),
                    "status": "pass",
                    "steps": r.steps.len()
                })
            );
        } else {
            failed += 1;
            let reason = r
                .steps
                .iter()
                .find_map(|s| match s {
                    StepResult::Fail(m) => Some(m.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "{}",
                serde_json::json!({
                    "file": r.file.display().to_string(),
                    "status": "fail",
                    "reason": reason
                })
            );
        }
    }
    let _ = writeln!(
        out,
        "{}",
        serde_json::json!({
            "summary": { "passed": passed, "failed": failed, "total": reports.len() }
        })
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(s: &str) -> Result<Scenario, String> {
        toml::from_str(s).map_err(|e| e.to_string())
    }

    #[test]
    fn minimal_scenario_parses() {
        let sc = parse_str(
            r#"
            plugin = "a.wasm"
            [[step]]
            call = "exec"
            args = ["echo", "hi"]
        "#,
        )
        .unwrap();
        assert_eq!(sc.plugin.to_str().unwrap(), "a.wasm");
        assert_eq!(sc.steps.len(), 1);
        match &sc.steps[0] {
            Step::Exec { args, .. } => {
                assert_eq!(args, &vec!["echo".to_string(), "hi".to_string()])
            }
            _ => panic!("expected exec step"),
        }
    }

    #[test]
    fn unknown_expect_key_rejected() {
        let err = parse_str(
            r#"
            plugin = "a.wasm"
            [[step]]
            call = "exec"
            args = ["x"]
            [step.expect]
            mystery = "boom"
        "#,
        )
        .unwrap_err();
        assert!(err.contains("mystery") || err.contains("unknown field"));
    }

    #[test]
    fn hook_step_parses() {
        let sc = parse_str(
            r#"
            plugin = "a.wasm"
            [[step]]
            call = "hook"
            name = "on-cd"
            args = ["/old", "/new"]
        "#,
        )
        .unwrap();
        match &sc.steps[0] {
            Step::Hook { name, args, .. } => {
                assert_eq!(*name, HookName::OnCd);
                assert_eq!(args.len(), 2);
            }
            _ => panic!("expected hook step"),
        }
    }
}
