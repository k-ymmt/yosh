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

#[derive(Debug, Deserialize)]
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
    #[serde(default = "default_max_memory_mb")]
    pub max_memory_mb: u64,
}

fn default_timeout_ms() -> u64 {
    5000
}

fn default_max_memory_mb() -> u64 {
    crate::test_host::DEFAULT_MAX_MEMORY_MB
}

// Manual `Default` (rather than `#[derive(Default)]`) so that an
// entirely-absent `[env]` section in a scenario TOML — which makes
// serde fall back to `EnvConfig::default()` wholesale instead of
// applying each field's `#[serde(default = "...")]` — still gets the
// real timeout/memory defaults instead of `0`. A `max_memory_mb` of 0
// would deny the plugin's initial linear memory outright.
impl Default for EnvConfig {
    fn default() -> Self {
        EnvConfig {
            caps: Vec::new(),
            vars: BTreeMap::new(),
            exported: Vec::new(),
            cwd: String::new(),
            allow_exec: Vec::new(),
            sandbox_root: String::new(),
            timeout_ms: default_timeout_ms(),
            max_memory_mb: default_max_memory_mb(),
        }
    }
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
    pub denied: Option<bool>,
}

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

use crate::runner::{HookCall, RunOutcome, invoke_exec, invoke_hook, load_plugin_precompiled};
use crate::test_host::TestState;
use yosh_plugin_api::pattern::CommandPattern;
use yosh_plugin_api::{capabilities_to_bitflags, parse_capability};

#[derive(Debug)]
pub enum StepResult {
    Pass,
    Fail {
        /// 1-based step index; 0 = scenario-level failure (parse /
        /// engine / read / compile), which precedes any step.
        step: usize,
        /// Which expectation (or phase) failed: an `Expect` key name
        /// ("exit", "stdout", "vars_set", ...) or "parse" / "load" /
        /// "args" for non-expectation failures.
        check: &'static str,
        /// The expected value, where the check has one.
        expected: Option<serde_json::Value>,
        /// The actual value, where the check has one.
        got: Option<serde_json::Value>,
        /// Human sentence — same wording the human summary prints.
        reason: String,
    },
}

fn scenario_fail(check: &'static str, reason: String) -> StepResult {
    StepResult::Fail {
        step: 0,
        check,
        expected: None,
        got: None,
        reason,
    }
}

pub fn run_scenario(path: &std::path::Path) -> Vec<StepResult> {
    let scenario = match parse(path) {
        Ok(s) => s,
        Err(e) => return vec![scenario_fail("parse", format!("parse error: {}", e))],
    };

    // Fail fast on unparsable env entries — the rest of the schema is
    // strict (deny_unknown_fields), so a typo'd capability or exec
    // pattern must not silently grant nothing.
    if let Err(e) = validate_env(&scenario.env) {
        return vec![scenario_fail("env", format!("env: {}", e))];
    }

    let wasm_path = path
        .parent()
        .map(|p| p.join(&scenario.plugin))
        .unwrap_or(scenario.plugin.clone());

    // Compile once per scenario; each step still gets a fresh Store +
    // TestState (isolation), sharing only the immutable artifacts
    // (was: full re-read + recompile per step).
    let engine = match crate::precompile::make_engine() {
        Ok(e) => e,
        Err(e) => return vec![scenario_fail("load", format!("engine: {}", e))],
    };
    let wasm_bytes = match std::fs::read(&wasm_path) {
        Ok(b) => b,
        Err(e) => {
            return vec![scenario_fail(
                "load",
                format!("load: read {}: {}", wasm_path.display(), e),
            )];
        }
    };
    crate::trace::trace!("read {} ({} bytes)", wasm_path.display(), wasm_bytes.len());
    let component = match wasmtime::component::Component::new(&engine, &wasm_bytes) {
        Ok(c) => c,
        Err(e) => return vec![scenario_fail("load", format!("load: compile: {}", e))],
    };
    crate::trace::trace!("compiled component");

    let mut results = Vec::new();
    for (idx, step) in scenario.steps.iter().enumerate() {
        crate::trace::trace!("scenario {} step {}", path.display(), idx + 1);
        let state = build_state(&scenario);
        let timeout = std::time::Duration::from_millis(scenario.env.timeout_ms);
        let loaded = match load_plugin_precompiled(&engine, &component, state, timeout) {
            Ok(l) => l,
            Err(e) => {
                results.push(StepResult::Fail {
                    step: idx + 1,
                    check: "load",
                    expected: None,
                    got: None,
                    reason: format!("step {}: load: {}", idx + 1, e),
                });
                continue;
            }
        };

        let (outcome, expect) = match step {
            Step::Exec { args, expect } => {
                if args.is_empty() {
                    results.push(StepResult::Fail {
                        step: idx + 1,
                        check: "args",
                        expected: None,
                        got: None,
                        reason: format!("step {}: exec needs at least 1 arg", idx + 1),
                    });
                    continue;
                }
                let (cmd, rest) = (&args[0], &args[1..]);
                (invoke_exec(loaded, cmd, rest), expect)
            }
            Step::Hook { name, args, expect } => {
                let call = match build_hook_call(*name, args) {
                    Ok(c) => c,
                    Err(e) => {
                        results.push(StepResult::Fail {
                            step: idx + 1,
                            check: "args",
                            expected: None,
                            got: None,
                            reason: format!("step {}: hook args: {}", idx + 1, e),
                        });
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

/// Reject unparsable `env.caps` / `env.allow_exec` entries up front.
/// `build_state` can then assume every entry parses.
fn validate_env(env: &EnvConfig) -> Result<(), String> {
    for s in &env.caps {
        if parse_capability(s).is_none() {
            return Err(format!(
                "unknown capability string '{}' in env.caps (expected e.g. \"io\", \"hooks:pre_exec\")",
                s
            ));
        }
    }
    for p in &env.allow_exec {
        if let Err(e) = CommandPattern::parse(p) {
            return Err(format!("invalid env.allow_exec pattern '{}': {}", p, e));
        }
    }
    Ok(())
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
        // Canonicalize to match the CLI path (`lib.rs` run_once) and
        // `test_host::files::resolve`, which canonicalizes each
        // candidate before the starts_with check — a symlinked or
        // relative root would otherwise deny every files op.
        let root = std::path::PathBuf::from(&scenario.env.sandbox_root);
        state.sandbox_root = Some(std::fs::canonicalize(&root).unwrap_or(root));
    } else {
        for (k, v) in &scenario.files {
            state
                .files
                .insert(std::path::PathBuf::from(k), v.as_bytes().to_vec());
        }
    }
    state.max_memory_mb = Some(scenario.env.max_memory_mb);
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

/// Lossy-UTF-8 preview of written bytes for failure messages, capped
/// at 200 chars so a large payload doesn't flood the summary.
fn preview(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    if s.chars().count() > 200 {
        let head: String = s.chars().take(200).collect();
        format!("{}…", head)
    } else {
        s.into_owned()
    }
}

fn evaluate(step_idx: usize, o: &RunOutcome, e: &Expect) -> StepResult {
    macro_rules! fail {
        ($check:expr, $expected:expr, $got:expr, $($t:tt)*) => {{
            return StepResult::Fail {
                step: step_idx,
                check: $check,
                expected: $expected,
                got: $got,
                reason: format!("step {}: {}", step_idx, format_args!($($t)*)),
            }
        }};
    }

    // A harness-level error (trap / timeout / memory / load) fails the
    // step unless the scenario explicitly opts into traps with a
    // `trap = ...` key. Spec §5: a step without an expect block must at
    // least complete without trap/timeout.
    if let Some(kind) = o.error_kind {
        let handled_by_trap_key = e.trap.is_some() && kind == "trap";
        if !handled_by_trap_key {
            fail!(
                "error",
                None,
                Some(serde_json::json!(kind)),
                "step errored ({}): {}",
                kind,
                o.error.as_deref().unwrap_or("")
            );
        }
    }

    if let Some(want) = e.exit {
        match o.exit_code {
            Some(got) if got == want => {}
            Some(got) => fail!(
                "exit",
                Some(serde_json::json!(want)),
                Some(serde_json::json!(got)),
                "exit: want {}, got {}",
                want,
                got
            ),
            None => fail!(
                "exit",
                Some(serde_json::json!(want)),
                None,
                "exit: want {}, got (no exit code — hook?)",
                want
            ),
        }
    }

    let stdout_str = String::from_utf8_lossy(&o.stdout);
    let stderr_str = String::from_utf8_lossy(&o.stderr);

    if let Some(want) = &e.stdout
        && stdout_str != *want
    {
        fail!(
            "stdout",
            Some(serde_json::json!(want)),
            Some(serde_json::json!(stdout_str)),
            "stdout mismatch: want {:?}, got {:?}",
            want,
            stdout_str
        );
    }
    if let Some(want) = &e.stderr
        && stderr_str != *want
    {
        fail!(
            "stderr",
            Some(serde_json::json!(want)),
            Some(serde_json::json!(stderr_str)),
            "stderr mismatch: want {:?}, got {:?}",
            want,
            stderr_str
        );
    }
    if let Some(sub) = &e.stdout_contains
        && !stdout_str.contains(sub.as_str())
    {
        fail!(
            "stdout_contains",
            Some(serde_json::json!(sub)),
            Some(serde_json::json!(stdout_str)),
            "stdout_contains {:?} not found in {:?}",
            sub,
            stdout_str
        );
    }
    if let Some(sub) = &e.stderr_contains
        && !stderr_str.contains(sub.as_str())
    {
        fail!(
            "stderr_contains",
            Some(serde_json::json!(sub)),
            Some(serde_json::json!(stderr_str)),
            "stderr_contains {:?} not found in {:?}",
            sub,
            stderr_str
        );
    }
    if let Some(re) = &e.stdout_regex {
        match regex::Regex::new(re) {
            Ok(rx) if !rx.is_match(&stdout_str) => fail!(
                "stdout_regex",
                Some(serde_json::json!(re)),
                Some(serde_json::json!(stdout_str)),
                "stdout_regex {:?} did not match {:?}",
                re,
                stdout_str
            ),
            Err(err) => fail!(
                "stdout_regex",
                Some(serde_json::json!(re)),
                None,
                "stdout_regex invalid: {}",
                err
            ),
            _ => {}
        }
    }
    if let Some(re) = &e.stderr_regex {
        match regex::Regex::new(re) {
            Ok(rx) if !rx.is_match(&stderr_str) => fail!(
                "stderr_regex",
                Some(serde_json::json!(re)),
                Some(serde_json::json!(stderr_str)),
                "stderr_regex {:?} did not match {:?}",
                re,
                stderr_str
            ),
            Err(err) => fail!(
                "stderr_regex",
                Some(serde_json::json!(re)),
                None,
                "stderr_regex invalid: {}",
                err
            ),
            _ => {}
        }
    }

    if let Some(want) = &e.vars_set {
        let got: BTreeMap<String, String> = o.set_log.iter().cloned().collect();
        if got != *want {
            fail!(
                "vars_set",
                Some(serde_json::json!(want)),
                Some(serde_json::json!(got)),
                "vars_set: want {:?}, got {:?}",
                want,
                got
            );
        }
    }
    if let Some(want) = &e.vars_export {
        let got: BTreeMap<String, String> = o.export_log.iter().cloned().collect();
        if got != *want {
            fail!(
                "vars_export",
                Some(serde_json::json!(want)),
                Some(serde_json::json!(got)),
                "vars_export: want {:?}, got {:?}",
                want,
                got
            );
        }
    }

    if let Some(want) = &e.files_write {
        let got: BTreeMap<String, &Vec<u8>> = o
            .write_log
            .iter()
            .map(|(p, b)| (p.display().to_string(), b))
            .collect();
        for (path, expectation) in want {
            let (want_len, want_bytes): (Option<usize>, Option<&str>) = match expectation {
                FileExpect::Bytes(b) => (None, Some(b.as_str())),
                FileExpect::Struct { len, bytes_eq } => (*len, bytes_eq.as_deref()),
            };
            let Some(actual) = got.get(path) else {
                fail!(
                    "files_write",
                    Some(serde_json::json!(path)),
                    None,
                    "files_write[{}] not written",
                    path
                );
            };
            if let Some(l) = want_len
                && actual.len() != l
            {
                fail!(
                    "files_write",
                    Some(serde_json::json!(l)),
                    Some(serde_json::json!(actual.len())),
                    "files_write[{}] len: want {}, got {}",
                    path,
                    l,
                    actual.len()
                );
            }
            if let Some(b) = want_bytes
                && actual.as_slice() != b.as_bytes()
            {
                fail!(
                    "files_write",
                    Some(serde_json::json!(preview(b.as_bytes()))),
                    Some(serde_json::json!(preview(actual))),
                    "files_write[{}] content: want {:?}, got {:?}",
                    path,
                    preview(b.as_bytes()),
                    preview(actual)
                );
            }
        }
    }

    if let Some(want_seq) = &e.exec_called {
        if want_seq.len() != o.exec_log.len() {
            fail!(
                "exec_called",
                Some(serde_json::json!(want_seq.len())),
                Some(serde_json::json!(o.exec_log.len())),
                "exec_called: want {} calls, got {}",
                want_seq.len(),
                o.exec_log.len()
            );
        }
        for (i, (w, g)) in want_seq.iter().zip(o.exec_log.iter()).enumerate() {
            if w.program != g.program {
                fail!(
                    "exec_called",
                    Some(serde_json::json!(w.program)),
                    Some(serde_json::json!(g.program)),
                    "exec_called[{}].program: want {}, got {}",
                    i,
                    w.program,
                    g.program
                );
            }
            if w.args != g.args {
                fail!(
                    "exec_called",
                    Some(serde_json::json!(w.args)),
                    Some(serde_json::json!(g.args)),
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
                    "exec_called",
                    Some(serde_json::json!(exit)),
                    Some(serde_json::json!(g.exit_code)),
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
            fail!(
                "trap",
                Some(serde_json::json!(want)),
                Some(serde_json::json!(got)),
                "trap: want {}, got {}",
                want,
                got
            );
        }
    }

    if let Some(want) = e.denied {
        let got = !o.denied.is_empty();
        if got != want {
            fail!(
                "denied",
                Some(serde_json::json!(want)),
                Some(serde_json::json!(got)),
                "denied: want {}, got {} (denied log: {:?})",
                want,
                got,
                o.denied
            );
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
            denied: Vec::new(),
            error: None,
            error_kind: None,
            error_hint: None,
        }
    }

    fn outcome_with_error(kind: &'static str) -> RunOutcome {
        let mut o = outcome_with(None, b"");
        o.error = Some("boom".into());
        o.error_kind = Some(kind);
        o
    }

    /// Spec §5: a step with no (or an empty) expect block must at least
    /// complete without trap/timeout — a harness-level error fails it.
    #[test]
    fn step_without_expect_fails_on_trap() {
        let o = outcome_with_error("trap");
        let e = Expect::default();
        match evaluate(1, &o, &e) {
            StepResult::Fail { check, .. } => assert_eq!(check, "error"),
            StepResult::Pass => panic!("trapped step with empty expect must fail"),
        }
    }

    #[test]
    fn step_without_expect_fails_on_timeout() {
        let o = outcome_with_error("timeout");
        let e = Expect::default();
        assert!(
            matches!(evaluate(1, &o, &e), StepResult::Fail { .. }),
            "timed-out step with empty expect must fail"
        );
    }

    #[test]
    fn step_without_expect_fails_on_memory_error() {
        let o = outcome_with_error("memory");
        let e = Expect::default();
        assert!(
            matches!(evaluate(1, &o, &e), StepResult::Fail { .. }),
            "memory-limit step with empty expect must fail"
        );
    }

    /// `trap = true` explicitly sanctions the trap — must keep passing.
    #[test]
    fn expected_trap_still_passes() {
        let o = outcome_with_error("trap");
        let e = Expect {
            trap: Some(true),
            ..Default::default()
        };
        assert!(matches!(evaluate(1, &o, &e), StepResult::Pass));
    }

    /// `trap = false` + an actual trap keeps failing via the trap check.
    #[test]
    fn trap_false_with_trap_fails() {
        let o = outcome_with_error("trap");
        let e = Expect {
            trap: Some(false),
            ..Default::default()
        };
        assert!(matches!(evaluate(1, &o, &e), StepResult::Fail { .. }));
    }

    /// The scenario path must canonicalize `env.sandbox_root` exactly
    /// like the CLI path does — on macOS `/tmp/...` roots otherwise
    /// resolve candidates to `/private/tmp/...` and every files op is
    /// denied as an escape.
    #[test]
    fn build_state_canonicalizes_sandbox_root() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let toml_src = format!(
            "plugin = \"x.wasm\"\n[env]\nsandbox_root = \"{}\"\n",
            link.display()
        );
        let scenario: Scenario = toml::from_str(&toml_src).unwrap();
        let state = build_state(&scenario);
        assert_eq!(
            state.sandbox_root,
            Some(std::fs::canonicalize(&real).unwrap())
        );
    }

    /// Unknown cap strings (e.g. the kebab-case hook spelling) must be
    /// a hard scenario error, not silently grant nothing.
    #[test]
    fn validate_env_rejects_unknown_cap() {
        let scenario: Scenario =
            toml::from_str("plugin = \"x.wasm\"\n[env]\ncaps = [\"hooks:pre-exec\"]\n").unwrap();
        let err = validate_env(&scenario.env).unwrap_err();
        assert!(err.contains("hooks:pre-exec"), "{}", err);
    }

    #[test]
    fn validate_env_rejects_bad_allow_exec() {
        let scenario: Scenario =
            toml::from_str("plugin = \"x.wasm\"\n[env]\nallow_exec = [\":*\"]\n").unwrap();
        let err = validate_env(&scenario.env).unwrap_err();
        assert!(err.contains(":*"), "{}", err);
    }

    #[test]
    fn validate_env_accepts_valid_entries() {
        let scenario: Scenario = toml::from_str(
            "plugin = \"x.wasm\"\n[env]\ncaps = [\"io\", \"hooks:pre_exec\"]\nallow_exec = [\"git status:*\"]\n",
        )
        .unwrap();
        assert!(validate_env(&scenario.env).is_ok());
    }

    /// A bad env must fail the scenario before any wasm is read, so the
    /// plugin path here can be bogus.
    #[test]
    fn run_scenario_fails_fast_on_bad_env() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad-env.toml");
        std::fs::write(
            &path,
            "plugin = \"nonexistent.wasm\"\n[env]\ncaps = [\"hooks:pre-exec\"]\n",
        )
        .unwrap();
        let results = run_scenario(&path);
        match &results[0] {
            StepResult::Fail { check, reason, .. } => {
                assert_eq!(*check, "env");
                assert!(reason.contains("hooks:pre-exec"), "{}", reason);
            }
            StepResult::Pass => panic!("bad env must fail the scenario"),
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
            StepResult::Fail { reason, .. } => assert!(reason.contains("exit")),
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
    fn files_write_content_match_passes() {
        let mut o = outcome_with(Some(0), b"");
        o.write_log
            .push((std::path::PathBuf::from("/out"), b"hello".to_vec()));
        let e = Expect {
            files_write: Some(
                [("/out".to_string(), FileExpect::Bytes("hello".into()))]
                    .into_iter()
                    .collect(),
            ),
            ..Default::default()
        };
        assert!(matches!(evaluate(1, &o, &e), StepResult::Pass));
    }

    #[test]
    fn files_write_same_length_different_content_fails() {
        // The pre-content-capture bug: any 5-byte write matched "hello".
        let mut o = outcome_with(Some(0), b"");
        o.write_log
            .push((std::path::PathBuf::from("/out"), b"xxxxx".to_vec()));
        let e = Expect {
            files_write: Some(
                [("/out".to_string(), FileExpect::Bytes("hello".into()))]
                    .into_iter()
                    .collect(),
            ),
            ..Default::default()
        };
        match evaluate(1, &o, &e) {
            StepResult::Fail { check, .. } => assert_eq!(check, "files_write"),
            _ => panic!("expected content mismatch to fail"),
        }
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

    #[test]
    fn fail_carries_structured_fields() {
        let o = outcome_with(Some(2), b"");
        let e = Expect {
            exit: Some(0),
            ..Default::default()
        };
        match evaluate(3, &o, &e) {
            StepResult::Fail {
                step,
                check,
                expected,
                got,
                reason,
            } => {
                assert_eq!(step, 3);
                assert_eq!(check, "exit");
                assert_eq!(expected, Some(serde_json::json!(0)));
                assert_eq!(got, Some(serde_json::json!(2)));
                assert!(reason.contains("exit"));
            }
            _ => panic!("expected fail"),
        }
    }

    #[test]
    fn format_summary_json_fail_line_has_structured_fields() {
        let reports = vec![ScenarioReport {
            file: std::path::PathBuf::from("t.toml"),
            steps: vec![StepResult::Fail {
                step: 2,
                check: "vars_set",
                expected: Some(serde_json::json!({"K": "v"})),
                got: Some(serde_json::json!({})),
                reason: "step 2: vars_set: want ..., got ...".into(),
            }],
        }];
        let s = format_summary_json(&reports);
        let first_line = s.lines().next().unwrap();
        let v: serde_json::Value = serde_json::from_str(first_line).unwrap();
        assert_eq!(v["status"], serde_json::json!("fail"));
        assert_eq!(v["step"], serde_json::json!(2));
        assert_eq!(v["check"], serde_json::json!("vars_set"));
        assert_eq!(v["expected"], serde_json::json!({"K": "v"}));
        assert_eq!(v["got"], serde_json::json!({}));
    }

    #[test]
    fn expect_denied_true_requires_a_denial() {
        let o = outcome_with(Some(13), b"");
        let e = Expect {
            denied: Some(true),
            ..Default::default()
        };
        match evaluate(1, &o, &e) {
            StepResult::Fail { check, .. } => assert_eq!(check, "denied"),
            _ => panic!("no denial recorded, expect denied=true must fail"),
        }
        let mut o2 = outcome_with(Some(13), b"");
        o2.denied.push("files:read: /x".into());
        assert!(matches!(evaluate(1, &o2, &e), StepResult::Pass));
    }

    #[test]
    fn expect_denied_false_rejects_denials() {
        let mut o = outcome_with(Some(0), b"");
        o.denied.push("io:write".into());
        let e = Expect {
            denied: Some(false),
            ..Default::default()
        };
        assert!(matches!(evaluate(1, &o, &e), StepResult::Fail { .. }));
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
                if let StepResult::Fail { reason, .. } = s {
                    let _ = writeln!(out, "      {}", reason);
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
            let first = r.steps.iter().find_map(|s| match s {
                StepResult::Fail {
                    step,
                    check,
                    expected,
                    got,
                    reason,
                } => Some((*step, *check, expected.clone(), got.clone(), reason.clone())),
                _ => None,
            });
            let (step, check, expected, got, reason) =
                first.expect("failed report has a Fail step");
            let _ = writeln!(
                out,
                "{}",
                serde_json::json!({
                    "file": r.file.display().to_string(),
                    "status": "fail",
                    "step": step,
                    "check": check,
                    "expected": expected,
                    "got": got,
                    "reason": reason,
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
