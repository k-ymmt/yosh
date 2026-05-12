//! Drive `yosh:plugin/*` exports against a `TestCtx`. Used by both
//! `yosh plugin run` (single invocation) and `yosh plugin test`
//! (scenario stepping).

use std::path::Path;
use std::time::Duration;

use wasmtime::Store;
use wasmtime::component::Component;

use crate::generated::{PluginWorld, PluginWorldPre};
use crate::precompile::make_engine;
use crate::test_host::{TestCtx, TestState, build_linker, register_imports};

pub struct LoadedPlugin {
    pub world: PluginWorld,
    pub store: Store<TestCtx>,
    pub engine: wasmtime::Engine,
}

#[derive(Debug)]
pub enum RunnerError {
    Load(String),
    Trap(String),
    Timeout(String),
}

impl std::fmt::Display for RunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunnerError::Load(s) => write!(f, "load: {}", s),
            RunnerError::Trap(s) => write!(f, "trap: {}", s),
            RunnerError::Timeout(s) => write!(f, "timeout: {}", s),
        }
    }
}

pub fn load_plugin(
    wasm_path: &Path,
    state: TestState,
    timeout: Duration,
) -> Result<LoadedPlugin, RunnerError> {
    let engine = make_engine().map_err(|e| RunnerError::Load(e.to_string()))?;
    let wasm_bytes = std::fs::read(wasm_path)
        .map_err(|e| RunnerError::Load(format!("read {}: {}", wasm_path.display(), e)))?;
    let component = Component::new(&engine, &wasm_bytes)
        .map_err(|e| RunnerError::Load(format!("compile: {}", e)))?;

    let mut linker = build_linker(&engine).map_err(|e| RunnerError::Load(e.to_string()))?;
    register_imports(&mut linker).map_err(|e| RunnerError::Load(e.to_string()))?;

    let pre = PluginWorldPre::new(
        linker
            .instantiate_pre(&component)
            .map_err(|e| RunnerError::Load(format!("instantiate_pre: {}", e)))?,
    )
    .map_err(|e| RunnerError::Load(format!("bindings: {}", e)))?;

    let mut store = Store::new(&engine, TestCtx::new(state));
    store.set_epoch_deadline(1);

    let watchdog_engine = engine.clone();
    let _watchdog = std::thread::Builder::new()
        .name("yosh-plugin-test-watchdog".into())
        .spawn(move || {
            std::thread::sleep(timeout);
            watchdog_engine.increment_epoch();
        });

    let world = pre
        .instantiate(&mut store)
        .map_err(|e| RunnerError::Load(format!("instantiate: {}", e)))?;
    Ok(LoadedPlugin {
        world,
        store,
        engine,
    })
}

use crate::test_host::ExecRecord;

/// Outcome of one guest invocation. Includes everything the formatters
/// and scenario evaluator need.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub exit_code: Option<i32>,        // Some for exec, None for hooks
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub set_log: Vec<(String, String)>,
    pub export_log: Vec<(String, String)>,
    pub write_log: Vec<(std::path::PathBuf, usize)>,
    pub exec_log: Vec<ExecRecord>,
    pub error: Option<String>,         // populated on trap/denied/timeout
    pub error_kind: Option<&'static str>,
}

impl RunOutcome {
    fn from_state(state: TestState, exit_code: Option<i32>, error: Option<(&'static str, String)>) -> Self {
        let (kind, msg) = match error {
            Some((k, m)) => (Some(k), Some(m)),
            None => (None, None),
        };
        RunOutcome {
            exit_code,
            stdout: state.stdout,
            stderr: state.stderr,
            set_log: state.set_log,
            export_log: state.export_log,
            write_log: state.write_log,
            exec_log: state.exec_log,
            error: msg,
            error_kind: kind,
        }
    }
}

pub fn invoke_exec(
    mut loaded: LoadedPlugin,
    command: &str,
    args: &[String],
) -> RunOutcome {
    let plugin = loaded.world.yosh_plugin_plugin();
    let res = plugin.call_exec(&mut loaded.store, command, args);
    let state = loaded.store.into_data().state;
    match res {
        Ok(code) => RunOutcome::from_state(state, Some(code), None),
        Err(e) => {
            let msg = e.to_string();
            let kind = classify_trap(&msg);
            RunOutcome::from_state(state, None, Some((kind, msg)))
        }
    }
}

fn classify_trap(msg: &str) -> &'static str {
    if msg.contains("epoch") || msg.contains("deadline") {
        "timeout"
    } else {
        "trap"
    }
}

pub enum HookCall {
    PreExec { command_line: String },
    PostExec { command_line: String, exit_code: i32 },
    OnCd { old: String, new: String },
    PrePrompt,
}

pub fn invoke_hook(mut loaded: LoadedPlugin, hook: HookCall) -> RunOutcome {
    let hooks = loaded.world.yosh_plugin_hooks();
    let res = match &hook {
        HookCall::PreExec { command_line } => hooks.call_pre_exec(&mut loaded.store, command_line),
        HookCall::PostExec { command_line, exit_code } => {
            hooks.call_post_exec(&mut loaded.store, command_line, *exit_code)
        }
        HookCall::OnCd { old, new } => hooks.call_on_cd(&mut loaded.store, old, new),
        HookCall::PrePrompt => hooks.call_pre_prompt(&mut loaded.store),
    };
    let state = loaded.store.into_data().state;
    match res {
        Ok(()) => RunOutcome::from_state(state, None, None),
        Err(e) => {
            let msg = e.to_string();
            let kind = classify_trap(&msg);
            RunOutcome::from_state(state, None, Some((kind, msg)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_wasm_returns_load_error() {
        let result = load_plugin(
            Path::new("/no/such/file.wasm"),
            TestState::default(),
            Duration::from_secs(1),
        );
        match result {
            Err(RunnerError::Load(_)) => {}
            Err(other) => panic!("expected Load error, got {:?}", other),
            Ok(_) => panic!("expected Load error, got Ok"),
        }
    }

    #[test]
    fn load_non_wasm_file_returns_load_error() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"not wasm").unwrap();
        let result = load_plugin(tmp.path(), TestState::default(), Duration::from_secs(1));
        match result {
            Err(RunnerError::Load(_)) => {}
            Err(other) => panic!("expected Load error, got {:?}", other),
            Ok(_) => panic!("expected Load error, got Ok"),
        }
    }

    #[test]
    fn invoke_exec_runs_test_plugin_test_cmd() {
        let wasm = match plugin_artifact() {
            Some(p) => p,
            None => return, // wasm not built; skip silently
        };
        let mut state = TestState::default();
        state.caps = yosh_plugin_api::CAP_IO;
        let loaded = load_plugin(&wasm, state, Duration::from_secs(5)).expect("load");
        let outcome = invoke_exec(loaded, "test_cmd", &["arg1".to_string()]);
        assert_eq!(outcome.exit_code, Some(0));
        assert!(outcome.stdout.starts_with(b"test_cmd args=["));
        assert!(outcome.error.is_none());
    }

    #[test]
    fn invoke_hook_pre_exec_records_event() {
        let wasm = match plugin_artifact() {
            Some(p) => p,
            None => return,
        };
        let mut state = TestState::default();
        state.caps = yosh_plugin_api::CAP_HOOK_PRE_EXEC
            | yosh_plugin_api::CAP_VARIABLES_WRITE
            | yosh_plugin_api::CAP_IO;
        let loaded = load_plugin(&wasm, state, Duration::from_secs(5)).expect("load");
        let outcome = invoke_hook(loaded, HookCall::PreExec { command_line: "ls -l".into() });
        assert!(outcome.error.is_none());
        // test_plugin records pre_exec:ls -l in its internal log; the
        // dump-events command flushes that log to a shell var, but we
        // don't drive it from here — we only need to confirm the hook
        // dispatched without trap.
    }

    fn plugin_artifact() -> Option<std::path::PathBuf> {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/wasm32-wasip2/release/test_plugin.wasm");
        if p.exists() { Some(p) } else { None }
    }
}
