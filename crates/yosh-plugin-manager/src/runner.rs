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
    /// The invocation deadline, kept for the timeout hint text.
    pub timeout_ms: u64,
    /// Keeps the epoch ticking until the invocation completes; stops
    /// and joins on drop.
    _tick: crate::tick::TickThread,
}

/// Category of a harness-level failure. `as_str()` is the JSON
/// `error.kind` value and the `RunOutcome.error_kind` string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Load,
    Metadata,
    Trap,
    Timeout,
    Memory,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Load => "load",
            ErrorKind::Metadata => "metadata",
            ErrorKind::Trap => "trap",
            ErrorKind::Timeout => "timeout",
            ErrorKind::Memory => "memory",
        }
    }
}

/// A harness-level failure: everything that is the harness's (or the
/// plugin binary's) fault rather than a legitimate plugin exit code.
/// Carries an optional one-line remediation hint. Replaces the old
/// `RunnerError`, whose `Trap`/`Timeout` variants were never
/// constructed (TODO ~L453).
#[derive(Debug)]
pub struct HarnessError {
    pub kind: ErrorKind,
    pub message: String,
    pub hint: Option<String>,
}

impl HarnessError {
    pub fn load(message: impl Into<String>) -> Self {
        HarnessError {
            kind: ErrorKind::Load,
            message: message.into(),
            hint: None,
        }
    }

    pub fn metadata(message: impl Into<String>) -> Self {
        HarnessError {
            kind: ErrorKind::Metadata,
            message: message.into(),
            hint: Some(
                "the `metadata` function must be side-effect-free (no host imports); \
                 see docs/yosh/plugin.md §Plugin Development Guide"
                    .into(),
            ),
        }
    }

    /// The `--format json` error object:
    /// `{"error":{"kind":...,"message":...,"hint":...}}` (hint null
    /// when absent).
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "error": {
                "kind": self.kind.as_str(),
                "message": self.message,
                "hint": self.hint,
            }
        })
    }
}

impl std::fmt::Display for HarnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind.as_str(), self.message)
    }
}

pub fn load_plugin(
    wasm_path: &Path,
    state: TestState,
    timeout: Duration,
) -> Result<LoadedPlugin, HarnessError> {
    let engine = make_engine().map_err(|e| HarnessError::load(e.to_string()))?;
    let wasm_bytes = std::fs::read(wasm_path)
        .map_err(|e| HarnessError::load(format!("read {}: {}", wasm_path.display(), e)))?;
    let component = Component::new(&engine, &wasm_bytes)
        .map_err(|e| HarnessError::load(format!("compile: {}", e)))?;

    let mut linker = build_linker(&engine).map_err(|e| HarnessError::load(e.to_string()))?;
    register_imports(&mut linker).map_err(|e| HarnessError::load(e.to_string()))?;

    let pre = PluginWorldPre::new(
        linker
            .instantiate_pre(&component)
            .map_err(|e| HarnessError::load(format!("instantiate_pre: {}", e)))?,
    )
    .map_err(|e| HarnessError::load(format!("bindings: {}", e)))?;

    let mut store = Store::new(&engine, TestCtx::new(state));
    // Deadline in ticks; the continuous tick thread bumps the epoch
    // every TICK_MS, so worst-case overshoot is one tick window.
    let ticks = (timeout.as_millis() as u64).div_ceil(crate::tick::TICK_MS).max(1);
    store.set_epoch_deadline(ticks);
    let tick = crate::tick::TickThread::spawn(engine.clone());

    let world = pre
        .instantiate(&mut store)
        .map_err(|e| HarnessError::load(format!("instantiate: {}", e)))?;
    Ok(LoadedPlugin {
        world,
        store,
        engine,
        timeout_ms: timeout.as_millis() as u64,
        _tick: tick,
    })
}

use crate::test_host::ExecRecord;

/// Outcome of one guest invocation. Includes everything the formatters
/// and scenario evaluator need.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub exit_code: Option<i32>, // Some for exec, None for hooks
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub set_log: Vec<(String, String)>,
    pub export_log: Vec<(String, String)>,
    pub write_log: Vec<(std::path::PathBuf, usize)>,
    pub exec_log: Vec<ExecRecord>,
    pub error: Option<String>, // populated on trap/denied/timeout
    pub error_kind: Option<&'static str>,
    pub error_hint: Option<String>,
}

impl RunOutcome {
    fn from_state(
        state: TestState,
        exit_code: Option<i32>,
        error: Option<HarnessError>,
    ) -> Self {
        let (kind, msg, hint) = match error {
            Some(e) => (Some(e.kind.as_str()), Some(e.message), e.hint),
            None => (None, None, None),
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
            error_hint: hint,
        }
    }
}

pub fn invoke_exec(mut loaded: LoadedPlugin, command: &str, args: &[String]) -> RunOutcome {
    let plugin = loaded.world.yosh_plugin_plugin();
    let res = plugin.call_exec(&mut loaded.store, command, args);
    let timeout_ms = loaded.timeout_ms;
    let state = loaded.store.into_data().state;
    match res {
        Ok(code) => RunOutcome::from_state(state, Some(code), None),
        Err(e) => {
            let err =
                classify_failure(&e, false, timeout_ms, crate::test_host::DEFAULT_MAX_MEMORY_MB);
            RunOutcome::from_state(state, None, Some(err))
        }
    }
}

/// Bucket a wasmtime call failure into `Timeout` (epoch deadline
/// interrupt) vs `Trap` (everything else). Epoch traps are detected
/// structurally by downcasting to `wasmtime::Trap::Interrupt`; the
/// substring fallback covers other wasmtime versions / future error
/// shapes where the trap is nested inside an anyhow chain.
fn classify_trap(err: &wasmtime::Error) -> ErrorKind {
    if let Some(trap) = err.downcast_ref::<wasmtime::Trap>()
        && matches!(trap, wasmtime::Trap::Interrupt)
    {
        return ErrorKind::Timeout;
    }
    let msg = err.to_string();
    if msg.contains("epoch") || msg.contains("deadline") || msg.contains("interrupt") {
        ErrorKind::Timeout
    } else {
        ErrorKind::Trap
    }
}

/// Classify a failed guest call into a `HarnessError` with a hint.
/// `memory_denied` is the store limiter's flag — a refused
/// `memory.grow` surfaces as a guest allocator abort whose trap text
/// says nothing about memory, so the flag wins over trap/timeout
/// classification. (The limiter lands in Task 7; until then callers
/// pass `false`.)
fn classify_failure(
    err: &wasmtime::Error,
    memory_denied: bool,
    timeout_ms: u64,
    max_memory_mb: u64,
) -> HarnessError {
    if memory_denied {
        return HarnessError {
            kind: ErrorKind::Memory,
            message: format!("{} (memory limit {} MiB exceeded)", err, max_memory_mb),
            hint: Some(format!(
                "raise --max-memory-mb (or env.max_memory_mb in scenarios) above {} \
                 if the plugin legitimately needs more",
                max_memory_mb
            )),
        };
    }
    let kind = classify_trap(err);
    let hint = match kind {
        ErrorKind::Timeout => Some(format!(
            "the invocation exceeded the {} ms budget; raise --timeout \
             (or env.timeout_ms in scenarios)",
            timeout_ms
        )),
        _ => None,
    };
    HarnessError {
        kind,
        message: err.to_string(),
        hint,
    }
}

pub enum HookCall {
    PreExec {
        command_line: String,
    },
    PostExec {
        command_line: String,
        exit_code: i32,
    },
    OnCd {
        old: String,
        new: String,
    },
    PrePrompt,
}

pub fn invoke_hook(mut loaded: LoadedPlugin, hook: HookCall) -> RunOutcome {
    let hooks = loaded.world.yosh_plugin_hooks();
    let res = match &hook {
        HookCall::PreExec { command_line } => hooks.call_pre_exec(&mut loaded.store, command_line),
        HookCall::PostExec {
            command_line,
            exit_code,
        } => hooks.call_post_exec(&mut loaded.store, command_line, *exit_code),
        HookCall::OnCd { old, new } => hooks.call_on_cd(&mut loaded.store, old, new),
        HookCall::PrePrompt => hooks.call_pre_prompt(&mut loaded.store),
    };
    let timeout_ms = loaded.timeout_ms;
    let state = loaded.store.into_data().state;
    match res {
        Ok(()) => RunOutcome::from_state(state, None, None),
        Err(e) => {
            let err =
                classify_failure(&e, false, timeout_ms, crate::test_host::DEFAULT_MAX_MEMORY_MB);
            RunOutcome::from_state(state, None, Some(err))
        }
    }
}

use std::fmt::Write as _;

pub fn format_human(o: &RunOutcome) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "[stdout]\n{}", String::from_utf8_lossy(&o.stdout));
    let _ = writeln!(out, "[stderr]\n{}", String::from_utf8_lossy(&o.stderr));
    match o.exit_code {
        Some(c) => {
            let _ = writeln!(out, "[exit] {}", c);
        }
        None => {
            let _ = writeln!(out, "[exit] (hook — no exit code)");
        }
    }
    for (k, v) in &o.set_log {
        let _ = writeln!(out, "[vars set]    {}={}", k, v);
    }
    for (k, v) in &o.export_log {
        let _ = writeln!(out, "[vars export] {}={}", k, v);
    }
    for (p, n) in &o.write_log {
        let _ = writeln!(out, "[files write] {} ({} bytes)", p.display(), n);
    }
    for r in &o.exec_log {
        let _ = writeln!(
            out,
            "[exec]        {} {} → exit {} ({} bytes stdout)",
            r.program,
            r.args.join(" "),
            r.exit_code,
            r.stdout_len
        );
    }
    if let (Some(kind), Some(msg)) = (o.error_kind, &o.error) {
        let _ = writeln!(out, "[error] {}: {}", kind, msg);
        if let Some(h) = &o.error_hint {
            let _ = writeln!(out, "[hint]  {}", h);
        }
    }
    out
}

pub fn format_json(o: &RunOutcome) -> serde_json::Value {
    serde_json::json!({
        "exit": o.exit_code,
        "stdout": String::from_utf8_lossy(&o.stdout),
        "stderr": String::from_utf8_lossy(&o.stderr),
        "vars_set":    o.set_log.iter().map(|(k,v)| serde_json::json!({"key":k,"value":v})).collect::<Vec<_>>(),
        "vars_export": o.export_log.iter().map(|(k,v)| serde_json::json!({"key":k,"value":v})).collect::<Vec<_>>(),
        "files_write": o.write_log.iter().map(|(p,n)| serde_json::json!({"path": p.display().to_string(),"bytes": n})).collect::<Vec<_>>(),
        "exec":        o.exec_log.iter().map(|r| serde_json::json!({
            "program": r.program, "args": r.args, "exit": r.exit_code, "stdout_bytes": r.stdout_len
        })).collect::<Vec<_>>(),
        "error": o.error.as_ref().map(|m| serde_json::json!({
            "kind": o.error_kind, "message": m, "hint": o.error_hint
        })),
    })
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
            Err(e) => assert_eq!(e.kind, ErrorKind::Load),
            Ok(_) => panic!("expected Load error, got Ok"),
        }
    }

    #[test]
    fn load_non_wasm_file_returns_load_error() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"not wasm").unwrap();
        let result = load_plugin(tmp.path(), TestState::default(), Duration::from_secs(1));
        match result {
            Err(e) => assert_eq!(e.kind, ErrorKind::Load),
            Ok(_) => panic!("expected Load error, got Ok"),
        }
    }

    #[test]
    fn invoke_exec_runs_test_plugin_test_cmd() {
        let wasm = match plugin_artifact() {
            Some(p) => p,
            None => return, // wasm not built; skip silently
        };
        let state = TestState {
            caps: yosh_plugin_api::CAP_IO,
            ..Default::default()
        };
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
        let state = TestState {
            caps: yosh_plugin_api::CAP_HOOK_PRE_EXEC
                | yosh_plugin_api::CAP_VARIABLES_WRITE
                | yosh_plugin_api::CAP_IO,
            ..Default::default()
        };
        let loaded = load_plugin(&wasm, state, Duration::from_secs(5)).expect("load");
        let outcome = invoke_hook(
            loaded,
            HookCall::PreExec {
                command_line: "ls -l".into(),
            },
        );
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

    #[test]
    fn format_json_round_trip_fields() {
        let mut o = RunOutcome {
            exit_code: Some(0),
            stdout: b"hi\n".to_vec(),
            stderr: Vec::new(),
            set_log: vec![("X".into(), "y".into())],
            export_log: Vec::new(),
            write_log: Vec::new(),
            exec_log: Vec::new(),
            error: None,
            error_kind: None,
            error_hint: None,
        };
        let j = format_json(&o);
        assert_eq!(j["exit"], serde_json::json!(0));
        assert_eq!(j["stdout"], serde_json::json!("hi\n"));
        assert_eq!(j["vars_set"][0]["key"], serde_json::json!("X"));
        o.error = Some("boom".into());
        o.error_kind = Some("trap");
        let j2 = format_json(&o);
        assert_eq!(j2["error"]["kind"], serde_json::json!("trap"));
    }

    #[test]
    fn format_human_includes_sections() {
        let o = RunOutcome {
            exit_code: Some(0),
            stdout: b"hi\n".to_vec(),
            stderr: Vec::new(),
            set_log: vec![("X".into(), "y".into())],
            export_log: Vec::new(),
            write_log: Vec::new(),
            exec_log: Vec::new(),
            error: None,
            error_kind: None,
            error_hint: None,
        };
        let s = format_human(&o);
        assert!(s.contains("[stdout]"));
        assert!(s.contains("[exit] 0"));
        assert!(s.contains("[vars set]    X=y"));
    }

    #[test]
    fn error_kind_serializes_lowercase() {
        assert_eq!(ErrorKind::Load.as_str(), "load");
        assert_eq!(ErrorKind::Metadata.as_str(), "metadata");
        assert_eq!(ErrorKind::Trap.as_str(), "trap");
        assert_eq!(ErrorKind::Timeout.as_str(), "timeout");
        assert_eq!(ErrorKind::Memory.as_str(), "memory");
    }

    #[test]
    fn harness_error_to_json_shape() {
        let e = HarnessError::load("boom");
        let j = e.to_json();
        assert_eq!(j["error"]["kind"], serde_json::json!("load"));
        assert_eq!(j["error"]["message"], serde_json::json!("boom"));
        assert!(j["error"]["hint"].is_null());
    }

    #[test]
    fn metadata_error_carries_hint() {
        let e = HarnessError::metadata("metadata: call: denied");
        assert_eq!(e.kind, ErrorKind::Metadata);
        assert!(e.hint.as_deref().unwrap().contains("side-effect-free"));
        let j = e.to_json();
        assert!(j["error"]["hint"].as_str().unwrap().contains("side-effect-free"));
    }

    #[test]
    fn classify_failure_memory_wins_and_hints() {
        let err = wasmtime::Error::msg("wasm trap: unreachable");
        let h = classify_failure(&err, true, 5000, 8);
        assert_eq!(h.kind, ErrorKind::Memory);
        assert!(h.message.contains("memory limit 8 MiB exceeded"));
        assert!(h.hint.as_deref().unwrap().contains("max-memory-mb"));
    }

    #[test]
    fn classify_failure_timeout_hint_names_budget() {
        let err = wasmtime::Error::msg("epoch deadline reached");
        let h = classify_failure(&err, false, 5000, 256);
        assert_eq!(h.kind, ErrorKind::Timeout);
        assert!(h.hint.as_deref().unwrap().contains("5000 ms"));
    }

    #[test]
    fn classify_failure_plain_trap_has_no_hint() {
        let err = wasmtime::Error::msg("wasm trap: unreachable");
        let h = classify_failure(&err, false, 5000, 256);
        assert_eq!(h.kind, ErrorKind::Trap);
        assert!(h.hint.is_none());
    }
}
