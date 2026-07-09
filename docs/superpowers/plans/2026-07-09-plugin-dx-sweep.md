# Plugin Author DX Sweep Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the 12 deferred DX items for the `yosh plugin run` / `yosh plugin test` harness: unified `HarnessError` with JSON routing and hints, denied tracking, harness memory cap, structured test-failure JSON, compile-once, `--watch`, a dependency-free trace channel, `files_write` content checks, a sandbox E2E test, and small alignments.

**Architecture:** All changes live in `crates/yosh-plugin-manager` (+ docs, TODO.md). A new `HarnessError { kind, message, hint }` replaces `RunnerError` and threads every harness-level failure to a single exit point in `cmd_run`. `TestState` gains `denied_log` (deterministic denial attribution) and `max_memory_mb` (a `TestLimiter` mirroring production's `MemoryLimiter`). `StepResult::Fail` becomes a struct carrying `step`/`check`/`expected`/`got`.

**Tech Stack:** Rust, wasmtime 27 components, serde/toml/serde_json. **Zero new dependencies** (settled at brainstorm: `--watch` = mtime polling, tracing = env-var-gated stderr).

**Spec:** `docs/superpowers/specs/2026-07-09-plugin-dx-sweep-design.md`

## Global Constraints

- Zero new dependencies. Do not add crates to any Cargo.toml.
- `src/plugin/` (yosh runtime) is untouched. All code changes are in `crates/yosh-plugin-manager` plus `docs/yosh/plugin.md`, `TODO.md`, and one new integration test file section.
- NEVER run `cargo build --workspace` / `cargo test --workspace` (wasm crates fail host builds).
- Integration tests in `crates/yosh-plugin-manager/tests/runner.rs` are artifact-gated: build fixtures first with `cargo component build -p test_plugin --target wasm32-wasip2 --release` (and `-p slow_plugin`, `-p hog_plugin` where a task says so). If the wasm is missing the tests skip silently — a "pass" without the artifact proves nothing.
- Manager crate error lines are prefixed `yosh-plugin: ` on stderr (NOT `yosh: ` — that is the shell binary's prefix).
- Exit-code policy is unchanged: plugin exit passes through, 99 = harness error, 2 = clap error.
- `cargo test -p yosh-plugin-manager` takes ~1–2 minutes with artifacts built; run in background if it exceeds foreground comfort. The full `cargo test --features test-helpers` suite takes minutes — always background it.
- Commit after every task with the task context in the message.

---

### Task 1: `HarnessError` / `ErrorKind` foundation in `runner.rs`

Replaces `RunnerError` (whose `Trap`/`Timeout` variants are dead code), adds hint plumbing to `RunOutcome`, and centralizes failure classification. `--format json` wiring into `cmd_run` happens in Task 2; the memory flag is wired in Task 7 (call sites pass `false` until then).

**Files:**
- Modify: `crates/yosh-plugin-manager/src/runner.rs`
- Modify: `crates/yosh-plugin-manager/src/test_host/mod.rs` (one new const)
- Modify: `crates/yosh-plugin-manager/src/scenario.rs` (test-helper literal only)
- Modify: `crates/yosh-plugin-manager/src/lib.rs` (`cmd_run`'s `load_plugin` error arm)

**Interfaces:**
- Produces: `runner::ErrorKind { Load, Metadata, Trap, Timeout, Memory }` with `as_str() -> &'static str`; `runner::HarnessError { pub kind, pub message, pub hint }` with `load()`, `metadata()`, `to_json()`, `Display`; `runner::classify_failure(err: &wasmtime::Error, memory_denied: bool, timeout_ms: u64, max_memory_mb: u64) -> HarnessError`; `RunOutcome.error_hint: Option<String>`; `LoadedPlugin.timeout_ms: u64`; `test_host::DEFAULT_MAX_MEMORY_MB: u64 = 256`.
- Consumes: existing `load_plugin`, `RunOutcome`, `classify_trap`.

- [ ] **Step 1: Write the failing unit tests**

Append to the `tests` module in `crates/yosh-plugin-manager/src/runner.rs`:

```rust
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p yosh-plugin-manager --lib runner`
Expected: FAIL to compile — `ErrorKind`, `HarnessError`, `classify_failure` not found.

- [ ] **Step 3: Implement the types**

In `crates/yosh-plugin-manager/src/test_host/mod.rs`, above `pub struct TestState`:

```rust
/// Default per-plugin linear-memory cap in MiB — parity with the
/// production host's `limits::DEFAULT_MAX_MEMORY_MB`.
pub const DEFAULT_MAX_MEMORY_MB: u64 = 256;
```

In `crates/yosh-plugin-manager/src/runner.rs`, replace the whole `RunnerError` enum + its `Display` impl with:

```rust
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
```

Change `classify_trap` to return `ErrorKind` and add `classify_failure` below it:

```rust
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
```

- [ ] **Step 4: Thread the new types through `LoadedPlugin`, `load_plugin`, `RunOutcome`, invocations, formatters**

Still in `runner.rs`:

1. `LoadedPlugin` gains a field (after `engine`):

```rust
    /// The invocation deadline, kept for the timeout hint text.
    pub timeout_ms: u64,
```

2. `load_plugin` returns `Result<LoadedPlugin, HarnessError>`; every `RunnerError::Load(...)` becomes `HarnessError::load(...)` (same message strings). Set the new field in the `Ok(...)` literal:

```rust
    Ok(LoadedPlugin {
        world,
        store,
        engine,
        timeout_ms: timeout.as_millis() as u64,
        _tick: tick,
    })
```

3. `RunOutcome` gains `pub error_hint: Option<String>` (after `error_kind`), and `from_state` takes the error as `Option<HarnessError>`:

```rust
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
```

4. `invoke_exec` and `invoke_hook` error arms go through `classify_failure` (shown for `invoke_exec`; `invoke_hook` is identical apart from the call):

```rust
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
```

5. `format_human` prints the hint after the error line:

```rust
    if let (Some(kind), Some(msg)) = (o.error_kind, &o.error) {
        let _ = writeln!(out, "[error] {}: {}", kind, msg);
        if let Some(h) = &o.error_hint {
            let _ = writeln!(out, "[hint]  {}", h);
        }
    }
```

6. `format_json`'s error object gains the hint:

```rust
        "error": o.error.as_ref().map(|m| serde_json::json!({
            "kind": o.error_kind, "message": m, "hint": o.error_hint
        })),
```

- [ ] **Step 5: Fix the compiler-reported fallout**

- `runner.rs` tests: `Err(RunnerError::Load(_))` matches become

```rust
        match result {
            Err(e) => assert_eq!(e.kind, ErrorKind::Load),
            Ok(_) => panic!("expected Load error, got Ok"),
        }
```

  and the two `RunOutcome` literals in `format_json_round_trip_fields` / `format_human_includes_sections` gain `error_hint: None,`.
- `scenario.rs` `outcome_with` helper (evaluator tests) gains `error_hint: None,` in its `RunOutcome` literal.
- `lib.rs` `cmd_run`'s `load_plugin` error arm keeps compiling because `HarnessError` implements `Display` — no edit needed beyond what the compiler demands (the full restructure is Task 2).

- [ ] **Step 6: Run the crate's unit tests**

Run: `cargo test -p yosh-plugin-manager --lib`
Expected: PASS, including the 6 new tests.

- [ ] **Step 7: Build fixtures and run integration tests (background)**

```bash
cargo component build -p test_plugin --target wasm32-wasip2 --release
cargo component build -p slow_plugin --target wasm32-wasip2 --release
```

Then in background: `cargo test -p yosh-plugin-manager --test runner`
Expected: PASS — `case_5` still sees `error_kind == Some("timeout")` (the string comes from `ErrorKind::as_str` now).

- [ ] **Step 8: Commit**

```bash
git add crates/yosh-plugin-manager/src/runner.rs crates/yosh-plugin-manager/src/test_host/mod.rs crates/yosh-plugin-manager/src/scenario.rs crates/yosh-plugin-manager/src/lib.rs
git commit -m "refactor(plugin-manager): unified HarnessError with kinds and hints

Task 1 of docs/superpowers/plans/2026-07-09-plugin-dx-sweep.md (plugin
author DX sweep). Replaces RunnerError (dead Trap/Timeout variants,
TODO ~L453); timeout failures now carry a budget hint."
```

---

### Task 2: Compile once + `cmd_run` restructure + JSON error routing

Closes TODO ~L446 (JSON callers get `{"error":{...}}`) and ~L447 (`--cap` fallback double-reads/compiles). `cmd_run` collapses to a single error exit; scenarios compile once per file instead of once per step.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/metadata_extract.rs` (split `extract`)
- Modify: `crates/yosh-plugin-manager/src/runner.rs` (`load_plugin_precompiled`)
- Modify: `crates/yosh-plugin-manager/src/lib.rs` (`run_once`, `emit_harness_error`, new `cmd_run`)
- Modify: `crates/yosh-plugin-manager/src/scenario.rs` (`run_scenario` compile-once)

**Interfaces:**
- Consumes: `HarnessError` / `ErrorKind` (Task 1).
- Produces: `metadata_extract::extract_component(engine: &Engine, component: &Component) -> Result<ExtractedMetadata, String>`; `runner::load_plugin_precompiled(engine: &wasmtime::Engine, component: &wasmtime::component::Component, state: TestState, timeout: Duration) -> Result<LoadedPlugin, HarnessError>`; `lib.rs::run_once(...) -> Result<i32, HarnessError>` and `lib.rs::emit_harness_error(&HarnessError, OutputFormat)` (Task 3 wraps them in the watch loop).

- [ ] **Step 1: Split `metadata_extract::extract`**

In `metadata_extract.rs`, replace the current `extract` body's first statement and signature area so the compile step lives in the byte-based wrapper:

```rust
/// Extract plugin metadata from raw wasm bytes. Compiles the component
/// and delegates to [`extract_component`].
pub fn extract(engine: &Engine, wasm_bytes: &[u8]) -> Result<ExtractedMetadata, String> {
    let component = Component::new(engine, wasm_bytes)
        .map_err(|e| format!("metadata: compile component: {}", e))?;
    extract_component(engine, &component)
}

/// Extract plugin metadata from an already-compiled component, so
/// callers that also instantiate the plugin (the `run` harness) compile
/// exactly once.
pub fn extract_component(
    engine: &Engine,
    component: &Component,
) -> Result<ExtractedMetadata, String> {
    let mut linker = Linker::<MetadataCtx>::new(engine);
    // ... (the rest of the old `extract` body, unchanged, operating on
    // `component`; `linker.instantiate_pre(component)` takes it by ref
    // already)
}
```

The old body from `let mut linker = ...` through the final `Ok(ExtractedMetadata { ... })` moves verbatim into `extract_component` (it already used `&component`).

- [ ] **Step 2: Add `load_plugin_precompiled` to `runner.rs`**

Replace `load_plugin` with a thin wrapper + the precompiled variant:

```rust
/// Path-based convenience wrapper: read + build engine + compile, then
/// delegate. Callers that already hold the compiled artifacts (the
/// `run` harness, the per-scenario loop) use
/// [`load_plugin_precompiled`] directly and compile exactly once.
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
    load_plugin_precompiled(&engine, &component, state, timeout)
}

pub fn load_plugin_precompiled(
    engine: &wasmtime::Engine,
    component: &Component,
    state: TestState,
    timeout: Duration,
) -> Result<LoadedPlugin, HarnessError> {
    let mut linker = build_linker(engine).map_err(|e| HarnessError::load(e.to_string()))?;
    register_imports(&mut linker).map_err(|e| HarnessError::load(e.to_string()))?;

    let pre = PluginWorldPre::new(
        linker
            .instantiate_pre(component)
            .map_err(|e| HarnessError::load(format!("instantiate_pre: {}", e)))?,
    )
    .map_err(|e| HarnessError::load(format!("bindings: {}", e)))?;

    let mut store = Store::new(engine, TestCtx::new(state));
    // Deadline in ticks; the continuous tick thread bumps the epoch
    // every TICK_MS, so worst-case overshoot is one tick window.
    let ticks = (timeout.as_millis() as u64)
        .div_ceil(crate::tick::TICK_MS)
        .max(1);
    store.set_epoch_deadline(ticks);
    let tick = crate::tick::TickThread::spawn(engine.clone());

    let world = pre
        .instantiate(&mut store)
        .map_err(|e| HarnessError::load(format!("instantiate: {}", e)))?;
    Ok(LoadedPlugin {
        world,
        store,
        engine: engine.clone(),
        timeout_ms: timeout.as_millis() as u64,
        _tick: tick,
    })
}
```

(`use crate::test_host::TestCtx;` is already imported in this file.)

- [ ] **Step 3: Rewrite `cmd_run` around `run_once` + `emit_harness_error`**

In `lib.rs`, replace the whole `cmd_run` function with:

```rust
#[allow(clippy::too_many_arguments)]
fn cmd_run(
    wasm: std::path::PathBuf,
    action: RunAction,
    cap: Vec<String>,
    vars: Vec<(String, String)>,
    exports: Vec<(String, String)>,
    cwd: std::path::PathBuf,
    allow_exec: Vec<String>,
    sandbox_root: Option<std::path::PathBuf>,
    timeout: u64,
    format: OutputFormat,
) -> i32 {
    match run_once(
        &wasm,
        &action,
        &cap,
        &vars,
        &exports,
        &cwd,
        &allow_exec,
        sandbox_root.as_deref(),
        timeout,
        format,
    ) {
        Ok(code) => code,
        Err(e) => {
            emit_harness_error(&e, format);
            99
        }
    }
}

/// Print a harness-level error: always the human line (+ hint) on
/// stderr; in JSON mode additionally a parseable `{"error":{...}}`
/// object on stdout so `--format json` consumers never have to scrape
/// stderr (spec §3.1).
fn emit_harness_error(e: &crate::runner::HarnessError, format: OutputFormat) {
    eprintln!("yosh-plugin: {}", e);
    if let Some(h) = &e.hint {
        eprintln!("yosh-plugin: hint: {}", h);
    }
    if matches!(format, OutputFormat::Json) {
        println!("{}", e.to_json());
    }
}

/// One complete `run` invocation: read + compile (once) + optional
/// metadata caps fallback + instantiate + invoke + print. Returns the
/// process exit code; every harness-level failure funnels to the
/// caller as `HarnessError`. Extracted so `--watch` (Task 3) can
/// re-run the same body.
#[allow(clippy::too_many_arguments)]
fn run_once(
    wasm: &std::path::Path,
    action: &RunAction,
    cap: &[String],
    vars: &[(String, String)],
    exports: &[(String, String)],
    cwd: &std::path::Path,
    allow_exec: &[String],
    sandbox_root: Option<&std::path::Path>,
    timeout: u64,
    format: OutputFormat,
) -> Result<i32, crate::runner::HarnessError> {
    use crate::runner::{
        HarnessError, HookCall, format_human, format_json, invoke_exec, invoke_hook,
        load_plugin_precompiled,
    };
    use crate::test_host::TestState;
    use wasmtime::component::Component;
    use yosh_plugin_api::pattern::CommandPattern;
    use yosh_plugin_api::{capabilities_to_bitflags, parse_capability};

    // Read + compile exactly once; the metadata fallback and
    // instantiation share the artifacts (was: 2x read + 2x compile).
    let bytes = std::fs::read(wasm)
        .map_err(|e| HarnessError::load(format!("read {}: {}", wasm.display(), e)))?;
    let engine = crate::precompile::make_engine()
        .map_err(|e| HarnessError::load(format!("engine: {}", e)))?;
    let component = Component::new(&engine, &bytes)
        .map_err(|e| HarnessError::load(format!("compile: {}", e)))?;

    let mut state = TestState::default();
    let parsed_caps: Vec<_> = cap.iter().filter_map(|s| parse_capability(s)).collect();
    state.caps = if cap.is_empty() {
        let m = crate::metadata_extract::extract_component(&engine, &component)
            .map_err(HarnessError::metadata)?;
        let caps: Vec<_> = m
            .required_capabilities
            .iter()
            .filter_map(|s| parse_capability(s))
            .collect();
        capabilities_to_bitflags(&caps)
    } else {
        capabilities_to_bitflags(&parsed_caps)
    };

    for (k, v) in vars {
        state.vars.insert(k.clone(), v.clone());
    }
    for (k, v) in exports {
        state.vars.insert(k.clone(), v.clone());
        state.exported.insert(k.clone());
    }
    state.cwd = cwd.to_path_buf();
    state.allow_exec = allow_exec
        .iter()
        .filter_map(|p| match CommandPattern::parse(p) {
            Ok(pat) => Some(pat),
            Err(e) => {
                eprintln!(
                    "yosh-plugin: ignoring invalid --allow-exec pattern {:?}: {}",
                    p, e
                );
                None
            }
        })
        .collect();
    state.sandbox_root =
        sandbox_root.map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()));

    let loaded = load_plugin_precompiled(
        &engine,
        &component,
        state,
        std::time::Duration::from_millis(timeout),
    )?;

    let outcome = match action {
        RunAction::Exec { command, args } => invoke_exec(loaded, command, args),
        RunAction::Hook { which } => {
            let call = match which {
                HookKind::PreExec { command_line } => HookCall::PreExec {
                    command_line: command_line.clone(),
                },
                HookKind::PostExec {
                    command_line,
                    exit_code,
                } => HookCall::PostExec {
                    command_line: command_line.clone(),
                    exit_code: *exit_code,
                },
                HookKind::OnCd { old, new } => HookCall::OnCd {
                    old: old.clone(),
                    new: new.clone(),
                },
                HookKind::PrePrompt => HookCall::PrePrompt,
            };
            invoke_hook(loaded, call)
        }
    };

    match format {
        OutputFormat::Human => print!("{}", format_human(&outcome)),
        OutputFormat::Json => println!("{}", format_json(&outcome)),
    }

    Ok(match outcome.error_kind {
        Some(_) => 99,
        None => outcome.exit_code.unwrap_or(0),
    })
}
```

- [ ] **Step 4: Compile once per scenario in `run_scenario`**

In `scenario.rs`, change the import line to use the precompiled loader:

```rust
use crate::runner::{HookCall, RunOutcome, invoke_exec, invoke_hook, load_plugin_precompiled};
```

and replace the top of `run_scenario` (through the `for` loop's `load` call) with:

```rust
pub fn run_scenario(path: &std::path::Path) -> Vec<StepResult> {
    let scenario = match parse(path) {
        Ok(s) => s,
        Err(e) => return vec![StepResult::Fail(format!("parse error: {}", e))],
    };

    let wasm_path = path
        .parent()
        .map(|p| p.join(&scenario.plugin))
        .unwrap_or(scenario.plugin.clone());

    // Compile once per scenario; each step still gets a fresh Store +
    // TestState (isolation), sharing only the immutable artifacts
    // (was: full re-read + recompile per step).
    let engine = match crate::precompile::make_engine() {
        Ok(e) => e,
        Err(e) => return vec![StepResult::Fail(format!("engine: {}", e))],
    };
    let wasm_bytes = match std::fs::read(&wasm_path) {
        Ok(b) => b,
        Err(e) => {
            return vec![StepResult::Fail(format!(
                "load: read {}: {}",
                wasm_path.display(),
                e
            ))];
        }
    };
    let component = match wasmtime::component::Component::new(&engine, &wasm_bytes) {
        Ok(c) => c,
        Err(e) => return vec![StepResult::Fail(format!("load: compile: {}", e))],
    };

    let mut results = Vec::new();
    for (idx, step) in scenario.steps.iter().enumerate() {
        let state = build_state(&scenario);
        let timeout = std::time::Duration::from_millis(scenario.env.timeout_ms);
        let loaded = match load_plugin_precompiled(&engine, &component, state, timeout) {
            Ok(l) => l,
            Err(e) => {
                results.push(StepResult::Fail(format!("step {}: load: {}", idx + 1, e)));
                continue;
            }
        };
        // ... (rest of the loop body unchanged)
```

- [ ] **Step 5: Run the crate suite**

Run: `cargo test -p yosh-plugin-manager --lib` then in background `cargo test -p yosh-plugin-manager --test runner`
Expected: all PASS (`run_dir_collects_toml_files` still fails-as-expected on the missing wasm, now via the scenario-level `load:` message; `case_6`/`case_7` behave identically).

- [ ] **Step 6: Commit**

```bash
git add crates/yosh-plugin-manager/src/metadata_extract.rs crates/yosh-plugin-manager/src/runner.rs crates/yosh-plugin-manager/src/lib.rs crates/yosh-plugin-manager/src/scenario.rs
git commit -m "feat(plugin-manager): JSON error routing; compile wasm exactly once

Task 2 of docs/superpowers/plans/2026-07-09-plugin-dx-sweep.md.
cmd_run funnels every harness error through one exit that also emits
{\"error\":{kind,message,hint}} in JSON mode (TODO ~L446); run and
per-scenario steps now share one compile (TODO ~L447)."
```

---

### Task 3: `yosh plugin run --watch`

Closes TODO ~L444. Dependency-free mtime polling per the brainstorm decision.

**Files:**
- Create: `crates/yosh-plugin-manager/src/watch.rs`
- Modify: `crates/yosh-plugin-manager/src/lib.rs` (`mod watch;`, `--watch` flag, watch loop in `cmd_run`)

**Interfaces:**
- Consumes: `run_once`, `emit_harness_error` (Task 2).
- Produces: `watch::wait_for_change(path: &Path, last: Option<SystemTime>) -> SystemTime`, `watch::WATCH_POLL_MS: u64 = 300`.

- [ ] **Step 1: Create `watch.rs` with its unit test**

```rust
//! mtime-polling change detection for `yosh plugin run --watch`.
//! Dependency-free by design (spec §3.6): 300 ms polling is plenty for
//! a rebuild-and-rerun dev loop and avoids platform FS-event backends.

use std::path::Path;
use std::time::SystemTime;

pub(crate) const WATCH_POLL_MS: u64 = 300;

/// Block until `path`'s mtime differs from `last`, then return the new
/// mtime. Polls every `WATCH_POLL_MS`; a vanished file (editors and
/// cargo unlink briefly during rebuild) just keeps polling. After the
/// first observed change, waits one extra poll interval so a compiler
/// mid-write doesn't hand us a torn wasm.
pub(crate) fn wait_for_change(path: &Path, last: Option<SystemTime>) -> SystemTime {
    loop {
        std::thread::sleep(std::time::Duration::from_millis(WATCH_POLL_MS));
        let Ok(md) = std::fs::metadata(path) else {
            continue;
        };
        let Ok(mtime) = md.modified() else { continue };
        if last != Some(mtime) {
            std::thread::sleep(std::time::Duration::from_millis(WATCH_POLL_MS));
            return mtime;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_for_change_sees_mtime_bump() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"v1").unwrap();
        let orig = std::fs::metadata(tmp.path()).unwrap().modified().unwrap();
        let path = tmp.path().to_path_buf();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(400));
            // Set an explicit future mtime so the test doesn't depend
            // on filesystem timestamp granularity.
            let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(10))
                .unwrap();
        });
        let start = std::time::Instant::now();
        let new = wait_for_change(tmp.path(), Some(orig));
        writer.join().unwrap();
        assert_ne!(new, orig);
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
    }
}
```

Register in `lib.rs` next to the other module declarations: `pub(crate) mod watch;`

- [ ] **Step 2: Run the unit test**

Run: `cargo test -p yosh-plugin-manager --lib watch`
Expected: PASS (takes ~1 s — two poll sleeps plus the 400 ms writer delay).

- [ ] **Step 3: Add the `--watch` flag and loop**

In `lib.rs` `Commands::Run`, after the `format` arg:

```rust
        /// Re-run the invocation whenever the wasm file changes
        /// (mtime-polled every 300 ms). Ctrl-C to stop.
        #[arg(long)]
        watch: bool,
```

Add `watch` to the `Commands::Run { ... }` destructuring in `run()` and pass it to `cmd_run` as a new trailing parameter. Then change `cmd_run`'s body (signature gains `watch: bool` before `format`... keep it LAST for clarity: `format: OutputFormat, watch: bool`):

```rust
    if !watch {
        return match run_once(
            &wasm,
            &action,
            &cap,
            &vars,
            &exports,
            &cwd,
            &allow_exec,
            sandbox_root.as_deref(),
            timeout,
            format,
        ) {
            Ok(code) => code,
            Err(e) => {
                emit_harness_error(&e, format);
                99
            }
        };
    }
    // --watch: re-run on every wasm mtime change until Ctrl-C (default
    // SIGINT disposition kills the process — no handler needed). Errors
    // don't end the loop: a broken build prints its error, then the
    // next successful build re-runs.
    let mut last = std::fs::metadata(&wasm).and_then(|m| m.modified()).ok();
    loop {
        match run_once(
            &wasm,
            &action,
            &cap,
            &vars,
            &exports,
            &cwd,
            &allow_exec,
            sandbox_root.as_deref(),
            timeout,
            format,
        ) {
            Ok(_) => {}
            Err(e) => emit_harness_error(&e, format),
        }
        if matches!(format, OutputFormat::Human) {
            eprintln!("--- watching {} (Ctrl-C to stop) ---", wasm.display());
        }
        last = Some(crate::watch::wait_for_change(&wasm, last));
        if matches!(format, OutputFormat::Human) {
            eprintln!("--- change detected, re-running ---");
        }
    }
```

- [ ] **Step 4: Smoke-test manually**

```bash
cargo build -p yosh-plugin-manager 2>/dev/null || cargo build
./target/debug/yosh plugin run target/wasm32-wasip2/release/test_plugin.wasm exec test_cmd hello --watch &
sleep 2
touch target/wasm32-wasip2/release/test_plugin.wasm
sleep 2
kill %1
```

Expected: two `[stdout] test_cmd args=...` blocks separated by the `--- watching ... ---` / `--- change detected ---` lines. (Confirm how the manager binary is invoked in this repo — if `yosh plugin` subcommands are routed through the main `yosh` binary, use `./target/debug/yosh plugin run ...` as shown; adjust to the actual binary name the compiler/`Cargo.toml` reveals.)

- [ ] **Step 5: Run the lib tests, then commit**

Run: `cargo test -p yosh-plugin-manager --lib`
Expected: PASS.

```bash
git add crates/yosh-plugin-manager/src/watch.rs crates/yosh-plugin-manager/src/lib.rs
git commit -m "feat(plugin-manager): yosh plugin run --watch (mtime polling)

Task 3 of docs/superpowers/plans/2026-07-09-plugin-dx-sweep.md.
Dependency-free 300ms polling; errors keep the loop alive (TODO ~L444)."
```

---

### Task 4: Structured step failures (`step` / `check` / `expected` / `got`)

Closes TODO ~L448: `test --format json` failure lines get the spec-§4.2 structured fields instead of only a freeform `reason`.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/scenario.rs`

**Interfaces:**
- Produces: `StepResult::Fail { step: usize, check: &'static str, expected: Option<serde_json::Value>, got: Option<serde_json::Value>, reason: String }` (step 0 = scenario-level failure). Tasks 5/6 add arms to `evaluate` using the same `fail!` macro.
- Consumes: `RunOutcome` (Task 1 shape).

- [ ] **Step 1: Write the failing tests**

Append to the `evaluator_tests` module:

```rust
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p yosh-plugin-manager --lib scenario`
Expected: FAIL to compile — `Fail` is a tuple variant.

- [ ] **Step 3: Reshape `StepResult` and rewrite `evaluate`**

Replace the `StepResult` enum:

```rust
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
```

Add a helper for scenario-level failures next to it:

```rust
fn scenario_fail(check: &'static str, reason: String) -> StepResult {
    StepResult::Fail {
        step: 0,
        check,
        expected: None,
        got: None,
        reason,
    }
}
```

Update `run_scenario`'s non-evaluate failure constructions:
- `parse error: {}` → `scenario_fail("parse", format!("parse error: {}", e))`
- `engine: {}` → `scenario_fail("load", format!("engine: {}", e))`
- `load: read ...` → `scenario_fail("load", format!("load: read {}: {}", wasm_path.display(), e))`
- `load: compile: {}` → `scenario_fail("load", format!("load: compile: {}", e))`
- per-step load failure →

```rust
                results.push(StepResult::Fail {
                    step: idx + 1,
                    check: "load",
                    expected: None,
                    got: None,
                    reason: format!("step {}: load: {}", idx + 1, e),
                });
```

- `exec needs at least 1 arg` and `hook args: {}` → same struct form with `check: "args"`, `step: idx + 1`, `expected: None`, `got: None`, keeping the existing reason strings.

Rewrite `evaluate` with the structured `fail!` macro (full function; the checks are the same, each now naming its `check` and values):

```rust
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
                            "files_write",
                            Some(serde_json::json!(want_len)),
                            Some(serde_json::json!(actual)),
                            "files_write[{}] len: want {}, got {}",
                            path,
                            want_len,
                            actual
                        ),
                        None => fail!(
                            "files_write",
                            Some(serde_json::json!(path)),
                            None,
                            "files_write[{}] not written",
                            path
                        ),
                    }
                }
                FileExpect::Struct { len, bytes_eq } => {
                    if let Some(l) = len {
                        match got.get(path) {
                            Some(actual) if *actual == *l => {}
                            Some(actual) => fail!(
                                "files_write",
                                Some(serde_json::json!(l)),
                                Some(serde_json::json!(actual)),
                                "files_write[{}] len: want {}, got {}",
                                path,
                                l,
                                actual
                            ),
                            None => fail!(
                                "files_write",
                                Some(serde_json::json!(path)),
                                None,
                                "files_write[{}] not written",
                                path
                            ),
                        }
                    }
                    if let Some(b) = bytes_eq {
                        let want_len = b.len();
                        match got.get(path) {
                            Some(actual) if *actual == want_len => {}
                            Some(actual) => fail!(
                                "files_write",
                                Some(serde_json::json!(want_len)),
                                Some(serde_json::json!(actual)),
                                "files_write[{}] bytes_eq len: want {}, got {}",
                                path,
                                want_len,
                                actual
                            ),
                            None => fail!(
                                "files_write",
                                Some(serde_json::json!(path)),
                                None,
                                "files_write[{}] not written",
                                path
                            ),
                        }
                    }
                }
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

    StepResult::Pass
}
```

(The `files_write` arm keeps length-only semantics here; Task 5 upgrades it to content comparison.)

- [ ] **Step 4: Update the formatters and remaining matchers**

`format_summary_human`'s inner loop:

```rust
            for s in &r.steps {
                if let StepResult::Fail { reason, .. } = s {
                    let _ = writeln!(out, "      {}", reason);
                }
            }
```

`format_summary_json`'s fail branch:

```rust
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
            let (step, check, expected, got, reason) = first.expect("failed report has a Fail step");
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
```

Update `expect_exit_mismatch_fails` in `evaluator_tests`:

```rust
        match evaluate(1, &o, &e) {
            StepResult::Fail { reason, .. } => assert!(reason.contains("exit")),
            _ => panic!("expected fail"),
        }
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p yosh-plugin-manager --lib scenario` then in background `cargo test -p yosh-plugin-manager --test runner`
Expected: all PASS, including the two new tests.

- [ ] **Step 6: Commit**

```bash
git add crates/yosh-plugin-manager/src/scenario.rs
git commit -m "feat(plugin-manager): structured step/check/expected/got in test JSON

Task 4 of docs/superpowers/plans/2026-07-09-plugin-dx-sweep.md.
Failure lines now carry the spec-4.2 structured fields alongside the
freeform reason (TODO ~L448)."
```

---

### Task 5: `files_write` content capture

Closes TODO ~L451: expectations compare content, not just byte length.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/test_host/mod.rs` (`write_log` type)
- Modify: `crates/yosh-plugin-manager/src/test_host/files.rs` (two push sites)
- Modify: `crates/yosh-plugin-manager/src/runner.rs` (`RunOutcome.write_log` type, formatters)
- Modify: `crates/yosh-plugin-manager/src/scenario.rs` (evaluator + `preview`)

**Interfaces:**
- Produces: `TestState.write_log: Vec<(PathBuf, Vec<u8>)>`, `RunOutcome.write_log: Vec<(PathBuf, Vec<u8>)>`; JSON `files_write` entries gain `"content"`; `scenario::preview(bytes: &[u8]) -> String` (private).
- Consumes: `StepResult::Fail` struct + `fail!` macro (Task 4).

- [ ] **Step 1: Write the failing tests**

In `scenario.rs` `evaluator_tests` (note `use std::path::PathBuf;` if not present in the module):

```rust
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
```

In `runner.rs` tests:

```rust
    #[test]
    fn format_json_files_write_includes_content() {
        let mut o = RunOutcome {
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
            set_log: Vec::new(),
            export_log: Vec::new(),
            write_log: Vec::new(),
            exec_log: Vec::new(),
            error: None,
            error_kind: None,
            error_hint: None,
        };
        o.write_log
            .push((std::path::PathBuf::from("/out"), b"hi".to_vec()));
        let j = format_json(&o);
        assert_eq!(j["files_write"][0]["bytes"], serde_json::json!(2));
        assert_eq!(j["files_write"][0]["content"], serde_json::json!("hi"));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p yosh-plugin-manager --lib`
Expected: FAIL to compile — `write_log` holds `usize`, not `Vec<u8>`.

- [ ] **Step 3: Widen the log**

`test_host/mod.rs`:

```rust
    /// (path, written bytes) for each files::{write,append}-file call.
    /// Bytes are captured (not just the length) so scenarios can assert
    /// content; test-plugin-scale payloads make the copy cost moot.
    pub write_log: Vec<(PathBuf, Vec<u8>)>,
```

`test_host/files.rs`, both push sites (`host_write_file`, `host_append_file`):

```rust
    state.write_log.push((resolved, data.to_vec()));
```

(In `host_write_file`'s virtual branch `resolved` is cloned into the map first, so the move into the log still compiles; same for `host_append_file`.)

`runner.rs`: `RunOutcome.write_log: Vec<(std::path::PathBuf, Vec<u8>)>`; `format_human`:

```rust
    for (p, b) in &o.write_log {
        let _ = writeln!(out, "[files write] {} ({} bytes)", p.display(), b.len());
    }
```

`format_json`:

```rust
        "files_write": o.write_log.iter().map(|(p, b)| serde_json::json!({
            "path": p.display().to_string(),
            "bytes": b.len(),
            "content": String::from_utf8_lossy(b),
        })).collect::<Vec<_>>(),
```

- [ ] **Step 4: Content comparison in the evaluator**

In `scenario.rs`, add near `evaluate`:

```rust
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
```

Replace the whole `files_write` arm of `evaluate` (from Task 4) with:

```rust
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
```

Fix the compiler fallout: `files.rs` unit test `virtual_write_then_read_roundtrips` asserts `write_log.len() == 1` — still fine; anywhere else that reads the `usize` will be reported by the compiler (the `runner.rs` `format_human` loop is covered above).

- [ ] **Step 5: Run the tests**

Run: `cargo test -p yosh-plugin-manager --lib`
Expected: PASS, including the 3 new tests.

- [ ] **Step 6: Commit**

```bash
git add crates/yosh-plugin-manager/src/test_host/mod.rs crates/yosh-plugin-manager/src/test_host/files.rs crates/yosh-plugin-manager/src/runner.rs crates/yosh-plugin-manager/src/scenario.rs
git commit -m "feat(plugin-manager): files_write expectations compare content

Task 5 of docs/superpowers/plans/2026-07-09-plugin-dx-sweep.md.
write_log captures bytes; JSON output gains a content field; a 5-byte
write no longer matches any 5-byte expectation (TODO ~L451)."
```

---

### Task 6: Denied tracking, hints, and the `Expect::denied` key

Closes TODO ~L450 (troubleshooting hints) and ~L457 (spec-§5 `denied` key). Denials are recorded state, not string sniffing.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/test_host/mod.rs` (`denied_log` field, `deny` helper)
- Modify: `crates/yosh-plugin-manager/src/test_host/{variables,filesystem,io,files,commands}.rs`
- Modify: `crates/yosh-plugin-manager/src/runner.rs` (`RunOutcome.denied`, `denied_hint`, formatters)
- Modify: `crates/yosh-plugin-manager/src/scenario.rs` (`Expect.denied` + evaluate arm)
- Modify: `crates/yosh-plugin-manager/tests/runner.rs` (case_9, case_10)

**Interfaces:**
- Produces: `TestState.denied_log: Vec<String>` (entries like `"files:read: /etc/passwd"`); `test_host::deny(state: &mut TestState, interface: &str, detail: &str) -> ErrorCode` (`pub(crate)`); `RunOutcome.denied: Vec<String>`; `runner::denied_hint(entry: &str) -> Option<String>` (pub); `Expect.denied: Option<bool>`.
- Consumes: `fail!` macro (Task 4). **Signature change:** `host_get`, `host_cwd`, `host_read_file`, `host_read_dir`, `host_metadata`, and `resolve` switch from `&TestState` to `&mut TestState` so denials can be recorded.

- [ ] **Step 1: Write the failing unit tests**

`test_host/variables.rs` tests — update + add:

```rust
    #[test]
    fn get_denied_without_cap() {
        let mut s = TestState::with_caps(0);
        assert_eq!(host_get(&mut s, "FOO"), Err(ErrorCode::Denied));
        assert_eq!(s.denied_log, vec!["variables:get: FOO".to_string()]);
    }
```

(Also mechanically add `&mut` to the other `host_get` calls in this module's tests.)

`test_host/files.rs` tests:

```rust
    #[test]
    fn read_denied_without_cap() {
        let mut s = TestState::default();
        assert_eq!(host_read_file(&mut s, "/a"), Err(ErrorCode::Denied));
        assert_eq!(s.denied_log, vec!["files:read: /a".to_string()]);
    }
```

`test_host/commands.rs` tests:

```rust
    #[test]
    fn pattern_not_allowed_recorded_in_denied_log() {
        let mut s = TestState::with_caps(CAP_COMMANDS_EXEC);
        assert_eq!(
            host_exec(&mut s, "echo", &["hi".to_string()]),
            Err(ErrorCode::PatternNotAllowed)
        );
        assert_eq!(s.denied_log, vec!["commands:exec: echo hi".to_string()]);
    }
```

`runner.rs` tests:

```rust
    #[test]
    fn denied_hint_suggests_allow_exec_pattern() {
        let h = denied_hint("commands:exec: git status --short").unwrap();
        assert!(h.contains("--allow-exec 'git:*'"));
        assert!(h.contains("--cap commands:exec"));
    }

    #[test]
    fn denied_hint_covers_each_interface_prefix() {
        assert!(denied_hint("files:read: /x").unwrap().contains("files:read"));
        assert!(denied_hint("variables:get: FOO").unwrap().contains("variables:read"));
        assert!(denied_hint("filesystem:cwd").unwrap().contains("filesystem"));
        assert!(denied_hint("io:write").unwrap().contains("io"));
        assert!(denied_hint("something:else").is_none());
    }
```

`scenario.rs` `evaluator_tests`:

```rust
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p yosh-plugin-manager --lib`
Expected: FAIL to compile — `denied_log`, `deny`, `denied`, `denied_hint` not found.

- [ ] **Step 3: Add the field and helper**

`test_host/mod.rs` — `TestState` gains (after `write_log`):

```rust
    /// One entry per capability denial (`Err(Denied)` or
    /// `PatternNotAllowed`), e.g. `"files:read: /etc/passwd"`. Recorded
    /// state — the harness never guesses denials from error text.
    pub denied_log: Vec<String>,
```

and below `TestState`'s impl:

```rust
/// Record a capability denial (for `[denied]` reporting and the
/// scenario `denied` expectation) and return `ErrorCode::Denied`.
/// `interface` is the WIT-ish name (e.g. "files:read"); `detail` names
/// the operand and may be empty.
pub(crate) fn deny(state: &mut TestState, interface: &str, detail: &str) -> ErrorCode {
    let entry = if detail.is_empty() {
        interface.to_string()
    } else {
        format!("{}: {}", interface, detail)
    };
    state.denied_log.push(entry);
    ErrorCode::Denied
}
```

- [ ] **Step 4: Record denials in every host import**

`variables.rs`:

```rust
pub fn host_get(state: &mut TestState, name: &str) -> Result<Option<String>, ErrorCode> {
    if state.caps & CAP_VARIABLES_READ == 0 {
        return Err(super::deny(state, "variables:get", name));
    }
    Ok(state.vars.get(name).cloned())
}
```

`host_set` → `super::deny(state, "variables:set", name)`; `host_export_env` → `super::deny(state, "variables:export-env", name)`.

`filesystem.rs` — `host_cwd(state: &mut TestState)` with `super::deny(state, "filesystem:cwd", "")`; `host_set_cwd` cap arm → `super::deny(state, "filesystem:set-cwd", path)`.

`io.rs` — `host_write` cap arm → `return Err(super::deny(state, "io:write", ""));`

`files.rs` — the gates take the path so the entry names the operand:

```rust
fn require_read(state: &mut TestState, path: &str) -> Result<(), ErrorCode> {
    if state.caps & CAP_FILES_READ == 0 {
        return Err(super::deny(state, "files:read", path));
    }
    Ok(())
}

fn require_write(state: &mut TestState, path: &str) -> Result<(), ErrorCode> {
    if state.caps & CAP_FILES_WRITE == 0 {
        return Err(super::deny(state, "files:write", path));
    }
    Ok(())
}
```

Every caller passes the path: `require_read(state, path)?;` / `require_write(state, path)?;`. `host_read_file`, `host_read_dir`, `host_metadata` change to `state: &mut TestState`. `resolve` becomes `&mut` and records sandbox escapes:

```rust
/// In sandbox mode, return the canonicalised real path or a recorded
/// `Denied` if it escapes `root`. Virtual mode returns the path as-is.
fn resolve(state: &mut TestState, path: &str) -> Result<PathBuf, ErrorCode> {
    let Some(root) = state.sandbox_root.clone() else {
        return Ok(PathBuf::from(path));
    };
    let candidate = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        root.join(path)
    };
    // Canonicalise lazily: if the file doesn't exist yet
    // (write/create), canonicalise the parent and re-join.
    let canon = match std::fs::canonicalize(&candidate) {
        Ok(p) => p,
        Err(_) => {
            let Some(parent) = candidate.parent() else {
                return Err(super::deny(state, "files:sandbox-escape", path));
            };
            let Ok(parent_canon) = std::fs::canonicalize(parent) else {
                return Err(super::deny(state, "files:sandbox-escape", path));
            };
            let Some(file_name) = candidate.file_name() else {
                return Err(super::deny(state, "files:sandbox-escape", path));
            };
            parent_canon.join(file_name)
        }
    };
    if canon.starts_with(&root) {
        Ok(canon)
    } else {
        Err(super::deny(state, "files:sandbox-escape", path))
    }
}
```

`commands.rs`:

```rust
    if state.caps & CAP_COMMANDS_EXEC == 0 {
        return Err(super::deny(state, "commands:exec", program));
    }
```

and the allowlist miss (`PatternNotAllowed` is not `Denied`, so record manually):

```rust
    if !state.allow_exec.iter().any(|p| p.matches(&argv)) {
        state.denied_log.push(format!("commands:exec: {}", argv.join(" ")));
        return Err(ErrorCode::PatternNotAllowed);
    }
```

`test_host/mod.rs` `register_imports` — the five read-only closures switch `store.data()` → `store.data_mut()` (variables `get`, filesystem `cwd`, files `read-file`, `read-dir`, `metadata`), e.g.:

```rust
    vars.func_wrap(
        "get",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (name,): (String,)| {
            Ok::<_, wasmtime::Error>((variables::host_get(&mut store.data_mut().state, &name),))
        },
    )?;
```

- [ ] **Step 5: Surface in `RunOutcome`, formatters, and the scenario evaluator**

`runner.rs`:

1. `RunOutcome` gains `pub denied: Vec<String>` (after `exec_log`); `from_state` sets `denied: state.denied_log`.
2. Public hint function:

```rust
/// One-line remediation for a denied-log entry, keyed on the interface
/// prefix. `None` when there is nothing actionable to suggest.
pub fn denied_hint(entry: &str) -> Option<String> {
    if let Some(rest) = entry.strip_prefix("commands:exec: ") {
        let program = rest.split_whitespace().next().unwrap_or(rest);
        return Some(format!(
            "re-run with --allow-exec '{}:*' (or add it to env.allow_exec) \
             and grant --cap commands:exec",
            program
        ));
    }
    if entry.starts_with("files:") {
        return Some(
            "add files:read / files:write to --cap (or env.caps); seed [files] \
             for the virtual FS or pass --sandbox-root"
                .into(),
        );
    }
    if entry.starts_with("variables:") {
        return Some("add variables:read / variables:write to --cap (or env.caps)".into());
    }
    if entry.starts_with("filesystem:") {
        return Some("add filesystem to --cap (or env.caps)".into());
    }
    if entry.starts_with("io:") {
        return Some("add io to --cap (or env.caps)".into());
    }
    None
}
```

3. `format_human`, after the exec-log loop and before the error block:

```rust
    for d in &o.denied {
        let _ = writeln!(out, "[denied]      {}", d);
        if let Some(h) = denied_hint(d) {
            let _ = writeln!(out, "              hint: {}", h);
        }
    }
```

4. `format_json` gains `"denied": o.denied,` alongside the other arrays.

`scenario.rs`:

1. `Expect` gains `pub denied: Option<bool>,` (after `trap`); DELETE the whole "Note: `denied: bool` … is intentionally not implemented" comment block above `FileExpect`.
2. `evaluate`, after the `trap` arm:

```rust
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
```

3. `outcome_with` test helper gains `denied: Vec::new(),` in its `RunOutcome` literal, and likewise every `RunOutcome` literal in `runner.rs` tests (three at this point — `format_json_round_trip_fields`, `format_human_includes_sections`, `format_json_files_write_includes_content`; the compiler lists them).

- [ ] **Step 6: Integration tests**

Append to `crates/yosh-plugin-manager/tests/runner.rs`:

```rust
#[test]
fn case_9_denied_read_recorded_with_entry() {
    let Some(w) = wasm() else { return };
    // read-file needs files:read; grant nothing. The guest maps Denied
    // to exit 13; the harness independently records the denial.
    let s = TestState::default();
    let loaded = load_plugin(&w, s, Duration::from_secs(5)).expect("load");
    let outcome = invoke_exec(loaded, "read-file", &["/x".into()]);
    assert_eq!(outcome.exit_code, Some(13));
    assert_eq!(outcome.denied, vec!["files:read: /x".to_string()]);
}

#[test]
fn case_10_scenario_denied_key() {
    let Some(w) = wasm() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let scenario_path = tmp.path().join("denied.toml");
    std::fs::write(
        &scenario_path,
        format!(
            r#"
plugin = "{}"

[[step]]
call = "exec"
args = ["read-file", "/x"]

  [step.expect]
  exit = 13
  denied = true
"#,
            w.canonicalize().unwrap().display()
        ),
    )
    .unwrap();
    let results = yosh_plugin_manager::scenario::run_scenario(&scenario_path);
    assert!(
        results
            .iter()
            .all(|r| matches!(r, yosh_plugin_manager::scenario::StepResult::Pass)),
        "results: {:?}",
        results
    );
}
```

- [ ] **Step 7: Run everything**

Run: `cargo test -p yosh-plugin-manager --lib` then in background `cargo test -p yosh-plugin-manager --test runner`
Expected: all PASS (fix any remaining `&s` → `&mut s` in module tests the compiler flags).

- [ ] **Step 8: Commit**

```bash
git add crates/yosh-plugin-manager/src/test_host crates/yosh-plugin-manager/src/runner.rs crates/yosh-plugin-manager/src/scenario.rs crates/yosh-plugin-manager/tests/runner.rs
git commit -m "feat(plugin-manager): record denials, print hints, add Expect::denied

Task 6 of docs/superpowers/plans/2026-07-09-plugin-dx-sweep.md.
denied_log gives deterministic attribution; [denied] entries carry
per-capability remediation hints (TODO ~L450, ~L457)."
```

---

### Task 7: Memory cap in the harness

Fulfils spec §7's mandate to surface the phase-1 memory-cap error kind: `--max-memory-mb`, `env.max_memory_mb`, a `TestLimiter` mirroring production, and `memory` classification via the limiter's flag.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/test_host/mod.rs` (`TestLimiter`, `TestState.max_memory_mb`, `TestCtx.limiter`)
- Modify: `crates/yosh-plugin-manager/src/runner.rs` (limiter install, real `classify_failure` args)
- Modify: `crates/yosh-plugin-manager/src/lib.rs` (`--max-memory-mb`)
- Modify: `crates/yosh-plugin-manager/src/scenario.rs` (`env.max_memory_mb`)
- Modify: `crates/yosh-plugin-manager/tests/runner.rs` (case_11)

**Interfaces:**
- Consumes: `classify_failure(err, memory_denied, timeout_ms, max_memory_mb)` (Task 1 — this task supplies the real `memory_denied` / `max_memory_mb` arguments), `load_plugin_precompiled` (Task 2), `DEFAULT_MAX_MEMORY_MB` (Task 1), `hog_plugin` fixture (phase 1).
- Produces: `test_host::TestLimiter` (`new(max_bytes: usize)`, `pub denied: bool`); `TestState.max_memory_mb: Option<u64>` (None = 256); `TestCtx.limiter: TestLimiter`; `LoadedPlugin.max_memory_mb: u64`.

- [ ] **Step 1: Write the failing unit test**

`test_host/mod.rs` tests:

```rust
    #[test]
    fn test_limiter_denies_over_cap_and_sets_flag() {
        use wasmtime::ResourceLimiter;
        let mib = 1024 * 1024;
        let mut l = TestLimiter::new(8 * mib);
        assert!(l.memory_growing(0, 4 * mib, None).unwrap());
        assert!(!l.denied);
        assert!(!l.memory_growing(4 * mib, 16 * mib, None).unwrap());
        assert!(l.denied);
        assert!(l.table_growing(0, 10_000, None).unwrap());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p yosh-plugin-manager --lib test_host`
Expected: FAIL to compile — `TestLimiter` not found.

- [ ] **Step 3: Implement `TestLimiter` and thread the cap**

`test_host/mod.rs`:

1. Below the `deny` helper:

```rust
/// Per-store linear-memory limiter — the same shape as the production
/// host's `limits::MemoryLimiter` (`src/plugin/limits.rs`). Duplicated
/// (~30 lines) because the manager crate cannot depend on the yosh
/// binary crate. Denies any growth beyond the cap and records the
/// denial so the runner can attribute the guest's allocator abort.
pub struct TestLimiter {
    max_memory_bytes: usize,
    pub denied: bool,
}

impl TestLimiter {
    pub fn new(max_memory_bytes: usize) -> Self {
        TestLimiter {
            max_memory_bytes,
            denied: false,
        }
    }
}

impl wasmtime::ResourceLimiter for TestLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.max_memory_bytes {
            self.denied = true;
            Ok(false)
        } else {
            Ok(true)
        }
    }

    fn table_growing(
        &mut self,
        _current: usize,
        _desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(true)
    }
}
```

(If the wasmtime 27 trait signature differs, match what the compiler shows — production's `src/plugin/limits.rs::MemoryLimiter` is the reference.)

2. `TestState` gains (after `denied_log`):

```rust
    /// Linear-memory cap in MiB. `None` = `DEFAULT_MAX_MEMORY_MB`.
    /// `Option` so `#[derive(Default)]` doesn't silently mean 0.
    pub max_memory_mb: Option<u64>,
```

3. `TestCtx` gains `pub(crate) limiter: TestLimiter,`; replace `TestCtx::new` and `Default`:

```rust
impl Default for TestCtx {
    fn default() -> Self {
        TestCtx::new(TestState::default())
    }
}

impl TestCtx {
    /// Build from an existing TestState (set up by the CLI / scenario).
    pub fn new(state: TestState) -> Self {
        let cap_bytes =
            (state.max_memory_mb.unwrap_or(DEFAULT_MAX_MEMORY_MB) as usize) * 1024 * 1024;
        TestCtx {
            state,
            table: ResourceTable::new(),
            wasi: WasiCtxBuilder::new().build(),
            limiter: TestLimiter::new(cap_bytes),
        }
    }
}
```

`runner.rs`:

4. `LoadedPlugin` gains `pub max_memory_mb: u64,`; in `load_plugin_precompiled`, before the store is built:

```rust
    let max_memory_mb = state
        .max_memory_mb
        .unwrap_or(crate::test_host::DEFAULT_MAX_MEMORY_MB);
    let mut store = Store::new(engine, TestCtx::new(state));
    store.limiter(|ctx| &mut ctx.limiter);
```

and set `max_memory_mb` in the `Ok(LoadedPlugin { ... })` literal.

5. `invoke_exec` / `invoke_hook` read the flag (shown for exec; hook identical):

```rust
    let timeout_ms = loaded.timeout_ms;
    let max_memory_mb = loaded.max_memory_mb;
    let data = loaded.store.into_data();
    let memory_denied = data.limiter.denied;
    let state = data.state;
    match res {
        Ok(code) => RunOutcome::from_state(state, Some(code), None),
        Err(e) => {
            let err = classify_failure(&e, memory_denied, timeout_ms, max_memory_mb);
            RunOutcome::from_state(state, None, Some(err))
        }
    }
```

`lib.rs`:

6. `Commands::Run` gains (after `timeout`):

```rust
        /// Linear-memory cap for the plugin store, in MiB.
        #[arg(long = "max-memory-mb", default_value_t = 256)]
        max_memory_mb: u64,
```

destructure + pass through `run()` → `cmd_run` → `run_once` (parameter `max_memory_mb: u64` after `timeout`); in `run_once`: `state.max_memory_mb = Some(max_memory_mb);` next to the other state fields.

`scenario.rs`:

7. `EnvConfig` gains:

```rust
    #[serde(default = "default_max_memory_mb")]
    pub max_memory_mb: u64,
```

with `fn default_max_memory_mb() -> u64 { crate::test_host::DEFAULT_MAX_MEMORY_MB }` next to `default_timeout_ms`; `build_state` sets `state.max_memory_mb = Some(scenario.env.max_memory_mb);`.

- [ ] **Step 4: Integration test against `hog_plugin`**

Build the fixture: `cargo component build -p hog_plugin --target wasm32-wasip2 --release`

Append to `tests/runner.rs`:

```rust
#[test]
fn case_11_memory_cap_kills_hog() {
    let hog = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm32-wasip2/release/hog_plugin.wasm");
    if !hog.exists() {
        return;
    }
    let mut s = TestState {
        caps: yosh_plugin_api::CAP_HOOK_PRE_EXEC,
        ..Default::default()
    };
    s.max_memory_mb = Some(8);
    // Generous 30s deadline so the epoch timeout can't fire first —
    // the 8 MiB cap must be what kills the unbounded allocator.
    let start = std::time::Instant::now();
    let loaded = load_plugin(&hog, s, Duration::from_secs(30)).expect("load");
    let outcome = invoke_hook(
        loaded,
        HookCall::PreExec {
            command_line: "ls".into(),
        },
    );
    assert_eq!(outcome.error_kind, Some("memory"), "error: {:?}", outcome.error);
    assert!(
        outcome
            .error_hint
            .as_deref()
            .unwrap_or("")
            .contains("max-memory-mb")
    );
    assert!(start.elapsed() < Duration::from_secs(10));
}
```

- [ ] **Step 5: Run everything**

Run: `cargo test -p yosh-plugin-manager --lib` then in background `cargo test -p yosh-plugin-manager --test runner`
Expected: all PASS, including `case_11` (hog is killed by the cap, classified `memory`, hint names the flag).

- [ ] **Step 6: Commit**

```bash
git add crates/yosh-plugin-manager/src/test_host/mod.rs crates/yosh-plugin-manager/src/runner.rs crates/yosh-plugin-manager/src/lib.rs crates/yosh-plugin-manager/src/scenario.rs crates/yosh-plugin-manager/tests/runner.rs
git commit -m "feat(plugin-manager): per-run memory cap with deterministic attribution

Task 7 of docs/superpowers/plans/2026-07-09-plugin-dx-sweep.md.
--max-memory-mb / env.max_memory_mb (default 256, production parity);
limiter denied flag classifies the trap as kind=memory with a hint
(runtime-limits spec 7 mandate)."
```

---

### Task 8: Dependency-free trace channel (`YOSH_PLUGIN_TRACE`)

Closes TODO ~L449. Supersedes the 2026-05-12 spec §6 `RUST_LOG`/`log`-crate promise (recorded in the DX-sweep spec §3.7).

**Files:**
- Create: `crates/yosh-plugin-manager/src/trace.rs`
- Modify: `crates/yosh-plugin-manager/src/lib.rs` (`pub(crate) mod trace;`)
- Modify: `crates/yosh-plugin-manager/src/runner.rs`, `crates/yosh-plugin-manager/src/scenario.rs`, `crates/yosh-plugin-manager/src/test_host/mod.rs` (call sites)

**Interfaces:**
- Produces: `trace::enabled() -> bool`; `trace::trace!(...)` macro (`pub(crate)`), printing `yosh-plugin[trace]: ...` to stderr when `YOSH_PLUGIN_TRACE` is set to anything but empty/`0`.

- [ ] **Step 1: Create `trace.rs` with tests**

```rust
//! Dependency-free trace channel for the run/test harness, enabled by
//! setting `YOSH_PLUGIN_TRACE` to anything but empty or `0`. This
//! supersedes the 2026-05-12 spec §6 `RUST_LOG` (log-crate) promise —
//! see docs/superpowers/specs/2026-07-09-plugin-dx-sweep-design.md
//! §3.7 for the zero-dependency rationale.

use std::sync::OnceLock;

static ENABLED: OnceLock<bool> = OnceLock::new();

/// Whether tracing is on. Reads `YOSH_PLUGIN_TRACE` once per process.
pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| parse_enabled(std::env::var("YOSH_PLUGIN_TRACE").ok().as_deref()))
}

fn parse_enabled(v: Option<&str>) -> bool {
    matches!(v, Some(x) if !x.is_empty() && x != "0")
}

/// `eprintln!` with a `yosh-plugin[trace]:` prefix, compiled to a
/// cheap branch when tracing is off (arguments are only evaluated
/// inside the branch).
macro_rules! trace {
    ($($t:tt)*) => {
        if $crate::trace::enabled() {
            eprintln!("yosh-plugin[trace]: {}", format_args!($($t)*));
        }
    };
}
pub(crate) use trace;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_enabled_truth_table() {
        assert!(!parse_enabled(None));
        assert!(!parse_enabled(Some("")));
        assert!(!parse_enabled(Some("0")));
        assert!(parse_enabled(Some("1")));
        assert!(parse_enabled(Some("true")));
    }
}
```

Register in `lib.rs`: `pub(crate) mod trace;`

- [ ] **Step 2: Run the unit test**

Run: `cargo test -p yosh-plugin-manager --lib trace`
Expected: PASS.

- [ ] **Step 3: Add call sites**

`runner.rs`:
- `load_plugin`, after the read: `crate::trace::trace!("read {} ({} bytes)", wasm_path.display(), wasm_bytes.len());`
- `load_plugin_precompiled`, after `pre.instantiate(...)` succeeds: `crate::trace::trace!("instantiated (timeout {} ms, memory cap {} MiB)", timeout.as_millis(), max_memory_mb);`
- `invoke_exec`, right after `let res = ...`:

```rust
    crate::trace::trace!(
        "exec {:?} -> {}",
        command,
        match &res {
            Ok(c) => format!("exit {}", c),
            Err(e) => format!("error: {}", e),
        }
    );
```

- `invoke_hook`, right after its `let res = ...` (the `hook` binding is still live):

```rust
    crate::trace::trace!(
        "hook {} -> {}",
        match &hook {
            HookCall::PreExec { .. } => "pre-exec",
            HookCall::PostExec { .. } => "post-exec",
            HookCall::OnCd { .. } => "on-cd",
            HookCall::PrePrompt => "pre-prompt",
        },
        match &res {
            Ok(()) => "ok".to_string(),
            Err(e) => format!("error: {}", e),
        }
    );
```

`scenario.rs`, first line inside the step loop: `crate::trace::trace!("scenario {} step {}", path.display(), idx + 1);`

`test_host/mod.rs` `register_imports` — every closure binds its result to `r`, traces, then returns it. Fully worked example (variables `get`; apply the same restructure to all 15):

```rust
    vars.func_wrap(
        "get",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (name,): (String,)| {
            let r = variables::host_get(&mut store.data_mut().state, &name);
            crate::trace::trace!("variables:get {:?} -> {:?}", name, r);
            Ok::<_, wasmtime::Error>((r,))
        },
    )?;
```

Exact trace statement per closure (results that carry payloads are summarized so a big file read doesn't flood stderr):

| Closure | Trace statement |
|---|---|
| variables `set` | `crate::trace::trace!("variables:set {:?}={:?} -> {:?}", name, value, r);` |
| variables `export-env` | `crate::trace::trace!("variables:export-env {:?}={:?} -> {:?}", name, value, r);` |
| filesystem `cwd` | `crate::trace::trace!("filesystem:cwd -> {:?}", r);` |
| filesystem `set-cwd` | `crate::trace::trace!("filesystem:set-cwd {:?} -> {:?}", path, r);` |
| io `write` | `crate::trace::trace!("io:write {:?} {} bytes -> {:?}", target, data.len(), r);` |
| files `read-file` | `crate::trace::trace!("files:read-file {:?} -> {:?}", path, r.as_ref().map(\|b\| b.len()));` |
| files `read-dir` | `crate::trace::trace!("files:read-dir {:?} -> {:?}", path, r.as_ref().map(\|v\| v.len()));` |
| files `metadata` | `crate::trace::trace!("files:metadata {:?} -> {:?}", path, r.as_ref().map(\|s\| s.size));` |
| files `write-file` | `crate::trace::trace!("files:write-file {:?} {} bytes -> {:?}", path, data.len(), r);` |
| files `append-file` | `crate::trace::trace!("files:append-file {:?} {} bytes -> {:?}", path, data.len(), r);` |
| files `create-dir` | `crate::trace::trace!("files:create-dir {:?} recursive={} -> {:?}", path, recursive, r);` |
| files `remove-file` | `crate::trace::trace!("files:remove-file {:?} -> {:?}", path, r);` |
| files `remove-dir` | `crate::trace::trace!("files:remove-dir {:?} recursive={} -> {:?}", path, recursive, r);` |
| commands `exec` | `crate::trace::trace!("commands:exec {:?} {:?} -> {:?}", program, args, r.as_ref().map(\|o\| o.exit_code));` |

(Write the `|b|` closures normally — the pipes are escaped above only for the Markdown table. If a generated WIT type lacks `Debug`, summarize the value another way; `ErrorCode` derives `Debug` today.)

- [ ] **Step 4: Verify by eye and by test**

```bash
YOSH_PLUGIN_TRACE=1 ./target/debug/yosh plugin run target/wasm32-wasip2/release/test_plugin.wasm exec test_cmd hi 2>&1 | grep '\[trace\]' | head
```

Expected: `read ...`, `instantiated ...`, `io:write ...`, `exec "test_cmd" -> exit 0` lines. (Rebuild first: `cargo build`.) Then run `cargo test -p yosh-plugin-manager --lib` — Expected: PASS (tracing off by default, output unchanged).

- [ ] **Step 5: Commit**

```bash
git add crates/yosh-plugin-manager/src/trace.rs crates/yosh-plugin-manager/src/lib.rs crates/yosh-plugin-manager/src/runner.rs crates/yosh-plugin-manager/src/scenario.rs crates/yosh-plugin-manager/src/test_host/mod.rs
git commit -m "feat(plugin-manager): YOSH_PLUGIN_TRACE stderr trace channel

Task 8 of docs/superpowers/plans/2026-07-09-plugin-dx-sweep.md.
Dependency-free tracer over every host import and runner phase;
supersedes the old spec's RUST_LOG promise (TODO ~L449)."
```

---

### Task 9: Sandbox-mode E2E scenario test

Closes TODO ~L452. `test_plugin` already ships `write-file` (writes `YOSH_TEST_CONTENT\n` via `files:write`) — no fixture changes.

**Files:**
- Modify: `crates/yosh-plugin-manager/tests/runner.rs` (case_12)

**Interfaces:**
- Consumes: `run_scenario`, `StepResult` (Task 4 shape), content-comparing `files_write` (Task 5), `env.sandbox_root` (existing).

- [ ] **Step 1: Write the test**

```rust
#[test]
fn case_12_sandbox_write_scenario_e2e() {
    let Some(w) = wasm() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    let scenario_path = root.join("sandbox_write.toml");
    // The write resolves to <root>/out.txt (relative paths join the
    // sandbox root), so the files_write expectation keys that path.
    let toml = format!(
        r#"
plugin = "{plugin}"
description = "write-file lands on the real FS under sandbox_root"

[env]
caps = ["files:write"]
sandbox_root = "{root}"

[[step]]
call = "exec"
args = ["write-file", "out.txt"]

  [step.expect]
  exit = 0
  files_write = {{ "{root}/out.txt" = "YOSH_TEST_CONTENT\n" }}
"#,
        plugin = w.canonicalize().unwrap().display(),
        root = root.display(),
    );
    std::fs::write(&scenario_path, toml).unwrap();

    let results = yosh_plugin_manager::scenario::run_scenario(&scenario_path);
    assert!(
        results
            .iter()
            .all(|r| matches!(r, yosh_plugin_manager::scenario::StepResult::Pass)),
        "results: {:?}",
        results
    );
    // The write must exist on the real filesystem, not just in a log.
    let on_disk = std::fs::read(root.join("out.txt")).unwrap();
    assert_eq!(on_disk, b"YOSH_TEST_CONTENT\n");
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p yosh-plugin-manager --test runner case_12`
Expected: PASS (feature-complete from Tasks 4–5 + existing sandbox mode; if the `files_write` key mismatches, print `results` — the resolved path in the failure message shows the actual key to expect).

Caveat: `run_scenario` on a single file works because `run_dir` isn't involved; the scenario TOML lives inside the tempdir so `\n` inside the TOML basic string is the escape sequence TOML itself defines — the guest writes a real newline and TOML `"YOSH_TEST_CONTENT\n"` parses to the same bytes.

- [ ] **Step 3: Commit**

```bash
git add crates/yosh-plugin-manager/tests/runner.rs
git commit -m "test(plugin-manager): sandbox-mode E2E scenario writes the real FS

Task 9 of docs/superpowers/plans/2026-07-09-plugin-dx-sweep.md.
Closes the virtual-FS-only coverage gap (TODO ~L452)."
```

---

### Task 10: Alignments, docs, TODO cleanup, full verification

Closes TODO ~L454 (`pub(crate)` CLI types) and ~L456 (`set_cwd` drift); documents everything; deletes the 12 swept TODO items.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/lib.rs`
- Modify: `crates/yosh-plugin-manager/src/test_host/filesystem.rs`
- Modify: `docs/yosh/plugin.md`
- Modify: `TODO.md`

- [ ] **Step 1: Tighten CLI-type visibility**

In `lib.rs`, change exactly four items from `pub` to `pub(crate)`: `pub enum RunAction` → `pub(crate) enum RunAction`, `pub enum HookKind` → `pub(crate) enum HookKind`, `pub enum OutputFormat` → `pub(crate) enum OutputFormat`, `pub fn parse_kv` → `pub(crate) fn parse_kv` (keep the `fn parse_kv` body unchanged). Run `cargo build -p yosh-plugin-manager` — if any external consumer (the shell binary's CLI dispatch) fails, revert ONLY the specific item it needs back to `pub` and note it in the commit message.

- [ ] **Step 2: Align `set_cwd` empty-path error with production**

In `test_host/filesystem.rs`:

```rust
    if path.is_empty() {
        // Production's host maps the empty path to IoFailed (not
        // InvalidArgument); match it so error-mapping tests written
        // against this harness hold in the real shell.
        return Err(ErrorCode::IoFailed);
    }
```

and update the unit test:

```rust
    #[test]
    fn set_cwd_rejects_empty() {
        let mut s = TestState::with_caps(CAP_FILESYSTEM);
        assert_eq!(host_set_cwd(&mut s, ""), Err(ErrorCode::IoFailed));
    }
```

Run: `cargo test -p yosh-plugin-manager --lib filesystem` — Expected: PASS.

- [ ] **Step 3: Document in `docs/yosh/plugin.md`**

1. In the `#### One-shot: yosh plugin run` flags table, add after the `--timeout` row:

```markdown
| `--max-memory-mb <N>` | Linear-memory cap for the plugin store in MiB (default 256) |
| `--watch` | Re-run the invocation whenever the wasm changes (300 ms mtime polling; Ctrl-C to stop) |
```

2. After that table (before "Hooks are invoked similarly:"), add:

```markdown
Harness-level failures (load, metadata, trap, timeout, memory) exit 99
and print a `yosh-plugin: <kind>: <message>` line — plus a `hint:` line
when there is an obvious fix. With `--format json` the same object is
also emitted on stdout as `{"error":{"kind":...,"message":...,"hint":...}}`,
so CI never scrapes stderr. Capability denials are not errors (the
plugin decides how to react); they are listed in a `[denied]` section
(JSON: `"denied"` array) with per-capability remediation hints.

Set `YOSH_PLUGIN_TRACE=1` to trace every host-import call and runner
phase on stderr (`yosh-plugin[trace]: ...`).
```

3. In the `#### Declarative: yosh plugin test` section, in the scenario example's `[env]` block add `max_memory_mb = 256` under `timeout_ms = 5000`, and replace the "Supported `[step.expect]` keys" paragraph with:

```markdown
Supported `[step.expect]` keys: `exit`, `stdout`, `stderr`,
`stdout_contains`, `stderr_contains`, `stdout_regex`, `stderr_regex`,
`vars_set`, `vars_export`, `files_write`, `exec_called`, `trap`,
`denied`. `files_write = { "/path" = "bytes" }` compares the written
content (use `{ len = N }` to assert length only); `denied = true`
passes iff at least one host call was capability-denied during the
step. Failing scenarios in `--format json` carry structured `step`,
`check`, `expected`, and `got` fields alongside the freeform `reason`.
```

- [ ] **Step 4: Delete the 12 swept TODO items**

In `TODO.md` "Future: Plugin System Enhancements", delete these whole bullets (identified by their opening text; per project convention completed items are deleted, never checked off):

1. "`yosh plugin run --watch` mode to re-run on wasm file change."
2. "Harness-level error paths in `yosh plugin run` (`load`/`engine`/`metadata`/runner) currently print stderr-only human text"
3. "`--cap` empty fallback in `yosh plugin run` re-reads the wasm"
4. "`yosh plugin test --format json` summary lines omit spec §4.2 fields"
5. "Spec §6 last paragraph promised `log` crate wiring"
6. "Spec §6 troubleshooting hint strings not implemented in `cmd_run`"
7. "`Expect::files_write = { path = \"bytes-string\" }` only checks byte *length*"
8. "`tests/runner.rs` covers virtual-FS scenarios only."
9. "`RunnerError::{Trap, Timeout}` variants are dead code"
10. "CLI-only types in `lib.rs` (`RunAction`, `HookKind`, `OutputFormat`, `parse_kv`) are `pub`"
11. "`set_cwd` empty-path error-code drift"
12. "`Expect::denied: bool` scenario key (spec §5)"

Do NOT delete the multi-plugin-scenario item, the `host_commands_exec` timeout-test item, the WASI-lockdown deviation item, or the `HostBackend` consolidation item — they remain open.

- [ ] **Step 5: Full verification (background — takes minutes)**

```bash
cargo component build -p test_plugin --target wasm32-wasip2 --release
cargo component build -p slow_plugin --target wasm32-wasip2 --release
cargo component build -p hog_plugin --target wasm32-wasip2 --release
cargo test -p yosh-plugin-manager 2>&1 | tail -15
cargo test --features test-helpers 2>&1 | tail -30
./e2e/run_tests.sh 2>&1 | tail -10
```

Expected: all green (known env-specific exceptions per project memory: LC_NUMERIC e2e flake, wasm32-wasip2 target gap on machines without the target, host_commands_exec subprocess-timing flake).

- [ ] **Step 6: Commit**

```bash
git add crates/yosh-plugin-manager/src/lib.rs crates/yosh-plugin-manager/src/test_host/filesystem.rs docs/yosh/plugin.md TODO.md
git commit -m "docs(plugin): document DX sweep; align set_cwd; tighten CLI types

Task 10 of docs/superpowers/plans/2026-07-09-plugin-dx-sweep.md.
Closes the 12 swept TODO items (watch, JSON errors, hints, denied key,
structured test JSON, compile-once, files_write content, sandbox E2E,
trace channel, RunnerError dead code, pub(crate), set_cwd drift)."
```
