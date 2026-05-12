//! In-memory host context for `yosh-plugin run` / `yosh-plugin test`.
//!
//! Mirrors the precedent of `metadata_extract::MetadataCtx`: a
//! self-contained `wasmtime` store data type with an empty `WasiCtx`
//! plus per-capability `yosh:plugin/*` import implementations backed
//! by `TestState`. Per-capability impls live in submodules.
//!
//! See `docs/superpowers/specs/2026-05-12-plugin-dev-test-runner-design.md`
//! §3 for the architectural rationale (third host context alongside
//! `HostContext` and `MetadataCtx`).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

use yosh_plugin_api::pattern::CommandPattern;

pub mod commands;
pub mod files;
pub mod filesystem;
pub mod io;
pub mod variables;

/// Record of one external command spawn (commands:exec). One entry
/// per host call, in invocation order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecRecord {
    pub program: String,
    pub args: Vec<String>,
    pub exit_code: i32,
    pub stdout_len: usize,
    pub stderr_len: usize,
}

/// In-memory state behind every TestCtx host import. The runner
/// constructs this from CLI flags or a scenario file, then reads it
/// back after the guest call to format results / evaluate expectations.
#[derive(Debug, Default, Clone)]
pub struct TestState {
    /// Granted capability bitmask. Same shape as `HostContext.capabilities`.
    pub caps: u32,
    pub vars: HashMap<String, String>,
    pub exported: HashSet<String>,
    pub cwd: PathBuf,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// Virtual filesystem contents (when `sandbox_root` is `None`).
    pub files: HashMap<PathBuf, Vec<u8>>,
    /// If set, files:* host imports operate on the real FS scoped to
    /// this canonicalised root. Otherwise they operate on `files`.
    pub sandbox_root: Option<PathBuf>,
    /// commands:exec allowlist. Empty = all denied (PatternNotAllowed).
    pub allow_exec: Vec<CommandPattern>,
    pub exec_log: Vec<ExecRecord>,
    /// (key, value) pairs the plugin wrote via variables::set during
    /// the current step. Reset by the scenario runner between steps.
    pub set_log: Vec<(String, String)>,
    pub export_log: Vec<(String, String)>,
    /// (path, bytes-written) for each files::{write,append}-file call.
    pub write_log: Vec<(PathBuf, usize)>,
}

/// Per-store wrapper. `state` is the shared in-memory backend; `wasi`
/// is an empty `WasiCtx` to absorb cargo-component's transitive
/// WASI imports (same rationale as `MetadataCtx` §Sandboxing).
pub struct TestCtx {
    pub state: TestState,
    pub(crate) table: ResourceTable,
    pub(crate) wasi: WasiCtx,
}

impl Default for TestCtx {
    fn default() -> Self {
        // Same rationale as MetadataCtx: no preopens, no stdio, no env.
        // Plugins use yosh:plugin/io, not wasi:cli/stdout.
        let wasi = WasiCtxBuilder::new().build();
        TestCtx {
            state: TestState::default(),
            table: ResourceTable::new(),
            wasi,
        }
    }
}

impl TestCtx {
    /// Build from an existing TestState (set up by the CLI / scenario).
    pub fn new(state: TestState) -> Self {
        let mut ctx = TestCtx::default();
        ctx.state = state;
        ctx
    }
}

impl WasiView for TestCtx {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_default_is_empty() {
        let s = TestState::default();
        assert!(s.vars.is_empty());
        assert!(s.exported.is_empty());
        assert_eq!(s.cwd.as_os_str(), "");
        assert!(s.stdout.is_empty());
        assert!(s.stderr.is_empty());
        assert_eq!(s.caps, 0);
    }

    #[test]
    fn test_ctx_default_constructs() {
        let _ctx = TestCtx::default();
    }
}
