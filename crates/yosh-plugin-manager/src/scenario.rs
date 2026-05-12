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

fn default_timeout_ms() -> u64 { 5000 }

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
    Struct { #[serde(default)] len: Option<usize>, #[serde(default)] bytes_eq: Option<String> },
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
    let parsed: Scenario = toml::from_str(&s).map_err(|e| format!("parse {}: {}", path.display(), e))?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(s: &str) -> Result<Scenario, String> {
        toml::from_str(s).map_err(|e| e.to_string())
    }

    #[test]
    fn minimal_scenario_parses() {
        let sc = parse_str(r#"
            plugin = "a.wasm"
            [[step]]
            call = "exec"
            args = ["echo", "hi"]
        "#).unwrap();
        assert_eq!(sc.plugin.to_str().unwrap(), "a.wasm");
        assert_eq!(sc.steps.len(), 1);
        match &sc.steps[0] {
            Step::Exec { args, .. } => assert_eq!(args, &vec!["echo".to_string(), "hi".to_string()]),
            _ => panic!("expected exec step"),
        }
    }

    #[test]
    fn unknown_expect_key_rejected() {
        let err = parse_str(r#"
            plugin = "a.wasm"
            [[step]]
            call = "exec"
            args = ["x"]
            [step.expect]
            mystery = "boom"
        "#).unwrap_err();
        assert!(err.contains("mystery") || err.contains("unknown field"));
    }

    #[test]
    fn hook_step_parses() {
        let sc = parse_str(r#"
            plugin = "a.wasm"
            [[step]]
            call = "hook"
            name = "on-cd"
            args = ["/old", "/new"]
        "#).unwrap();
        match &sc.steps[0] {
            Step::Hook { name, args, .. } => {
                assert_eq!(*name, HookName::OnCd);
                assert_eq!(args.len(), 2);
            }
            _ => panic!("expected hook step"),
        }
    }
}
