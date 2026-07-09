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

/// Default per-plugin linear-memory cap in MiB — parity with the
/// production host's `limits::DEFAULT_MAX_MEMORY_MB`.
pub const DEFAULT_MAX_MEMORY_MB: u64 = 256;

/// In-memory state behind every TestCtx host import. The runner
/// constructs this from CLI flags or a scenario file, then reads it
/// back after the guest call to format results / evaluate expectations.
///
/// Note: the in-memory write paths intentionally skip the validation
/// that the production host's `VarTable::set` (and similar) performs
/// (readonly-var rejection, integer-attribute checks, etc.). Test
/// scenarios verifying those guard paths must run against the real
/// shell, not this harness.
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
    /// (path, written bytes) for each files::{write,append}-file call.
    /// Bytes are captured (not just the length) so scenarios can assert
    /// content; test-plugin-scale payloads make the copy cost moot.
    pub write_log: Vec<(PathBuf, Vec<u8>)>,
    /// One entry per capability denial (`Err(Denied)` or
    /// `PatternNotAllowed`), e.g. `"files:read: /etc/passwd"`. Recorded
    /// state — the harness never guesses denials from error text.
    pub denied_log: Vec<String>,
    /// Linear-memory cap in MiB. `None` = `DEFAULT_MAX_MEMORY_MB`.
    /// `Option` so `#[derive(Default)]` doesn't silently mean 0.
    pub max_memory_mb: Option<u64>,
}

impl TestState {
    /// Construct a `TestState` with the given capability bitmask and
    /// every other field at its `Default` value. Used by per-module
    /// unit tests so each module doesn't have to repeat a local
    /// `state_with` helper.
    pub fn with_caps(caps: u32) -> Self {
        TestState {
            caps,
            ..TestState::default()
        }
    }
}

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

/// Per-store wrapper. `state` is the shared in-memory backend; `wasi`
/// is an empty `WasiCtx` to absorb cargo-component's transitive
/// WASI imports (same rationale as `MetadataCtx` §Sandboxing).
pub struct TestCtx {
    pub state: TestState,
    pub(crate) table: ResourceTable,
    pub(crate) wasi: WasiCtx,
    pub(crate) limiter: TestLimiter,
}

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

impl WasiView for TestCtx {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

use wasmtime::Engine;
use wasmtime::component::Linker;

/// Construct a `Linker<TestCtx>` with WASI registered. Per-capability
/// `yosh:plugin/*` imports are added by `register_imports` (Task 9).
pub fn build_linker(engine: &Engine) -> wasmtime::Result<Linker<TestCtx>> {
    let mut linker = Linker::<TestCtx>::new(engine);
    register_wasi(&mut linker)?;
    Ok(linker)
}

/// Same rationale as `metadata_extract::register_wasi`: cargo-component
/// plugins pull in `wasi:io` / `wasi:cli` transitively. Isolation is
/// provided by the empty `WasiCtx` in `TestCtx::default`.
fn register_wasi(linker: &mut Linker<TestCtx>) -> wasmtime::Result<()> {
    wasmtime_wasi::add_to_linker_sync(linker)
}

use crate::generated::yosh::plugin::commands::ExecOutput;
use crate::generated::yosh::plugin::files::{DirEntry, FileStat};
use crate::generated::yosh::plugin::types::{ErrorCode, IoStream};

/// Register every `yosh:plugin/*` import. The per-capability host
/// functions enforce their own capability checks; the linker
/// unconditionally points each WIT name at its real implementation.
pub fn register_imports(linker: &mut Linker<TestCtx>) -> wasmtime::Result<()> {
    // variables
    let mut vars = linker.instance("yosh:plugin/variables@0.2.1")?;
    vars.func_wrap(
        "get",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (name,): (String,)| {
            Ok::<_, wasmtime::Error>((variables::host_get(&mut store.data_mut().state, &name),))
        },
    )?;
    vars.func_wrap(
        "set",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (name, value): (String, String)| {
            Ok::<_, wasmtime::Error>((variables::host_set(
                &mut store.data_mut().state,
                &name,
                &value,
            ),))
        },
    )?;
    vars.func_wrap(
        "export-env",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (name, value): (String, String)| {
            Ok::<_, wasmtime::Error>((variables::host_export_env(
                &mut store.data_mut().state,
                &name,
                &value,
            ),))
        },
    )?;

    // filesystem
    let mut fs = linker.instance("yosh:plugin/filesystem@0.2.1")?;
    fs.func_wrap(
        "cwd",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (): ()| {
            Ok::<_, wasmtime::Error>((filesystem::host_cwd(&mut store.data_mut().state),))
        },
    )?;
    fs.func_wrap(
        "set-cwd",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (path,): (String,)| {
            Ok::<_, wasmtime::Error>(
                (filesystem::host_set_cwd(&mut store.data_mut().state, &path),),
            )
        },
    )?;

    // io
    let mut io_inst = linker.instance("yosh:plugin/io@0.2.1")?;
    io_inst.func_wrap(
        "write",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (target, data): (IoStream, Vec<u8>)| {
            Ok::<_, wasmtime::Error>((io::host_write(&mut store.data_mut().state, target, &data),))
        },
    )?;

    // files
    let mut f = linker.instance("yosh:plugin/files@0.2.1")?;
    f.func_wrap(
        "read-file",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (path,): (String,)| {
            Ok::<_, wasmtime::Error>((files::host_read_file(&mut store.data_mut().state, &path),))
        },
    )?;
    f.func_wrap(
        "read-dir",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (path,): (String,)| {
            Ok::<_, wasmtime::Error>((files::host_read_dir(&mut store.data_mut().state, &path),))
        },
    )?;
    f.func_wrap(
        "metadata",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (path,): (String,)| {
            Ok::<_, wasmtime::Error>((files::host_metadata(&mut store.data_mut().state, &path),))
        },
    )?;
    f.func_wrap(
        "write-file",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (path, data): (String, Vec<u8>)| {
            Ok::<_, wasmtime::Error>((files::host_write_file(
                &mut store.data_mut().state,
                &path,
                &data,
            ),))
        },
    )?;
    f.func_wrap(
        "append-file",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (path, data): (String, Vec<u8>)| {
            Ok::<_, wasmtime::Error>((files::host_append_file(
                &mut store.data_mut().state,
                &path,
                &data,
            ),))
        },
    )?;
    f.func_wrap(
        "create-dir",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (path, recursive): (String, bool)| {
            Ok::<_, wasmtime::Error>((files::host_create_dir(
                &mut store.data_mut().state,
                &path,
                recursive,
            ),))
        },
    )?;
    f.func_wrap(
        "remove-file",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (path,): (String,)| {
            Ok::<_, wasmtime::Error>((files::host_remove_file(&mut store.data_mut().state, &path),))
        },
    )?;
    f.func_wrap(
        "remove-dir",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (path, recursive): (String, bool)| {
            Ok::<_, wasmtime::Error>((files::host_remove_dir(
                &mut store.data_mut().state,
                &path,
                recursive,
            ),))
        },
    )?;

    // commands
    let mut cmds = linker.instance("yosh:plugin/commands@0.2.1")?;
    cmds.func_wrap(
        "exec",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>,
         (program, args): (String, Vec<String>)| {
            Ok::<_, wasmtime::Error>((commands::host_exec(
                &mut store.data_mut().state,
                &program,
                &args,
            ),))
        },
    )?;

    // Silence unused-import warnings if a future WIT addition removes
    // any of these constructor calls.
    let _ = (
        std::marker::PhantomData::<ExecOutput>,
        std::marker::PhantomData::<DirEntry>,
        std::marker::PhantomData::<FileStat>,
        std::marker::PhantomData::<ErrorCode>,
    );
    Ok(())
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

    #[test]
    fn linker_construction_smoke() {
        let engine = crate::precompile::make_engine().expect("engine");
        let _linker = build_linker(&engine).expect("linker");
    }

    #[test]
    fn linker_with_yosh_imports_constructs() {
        let engine = crate::precompile::make_engine().unwrap();
        let mut linker = build_linker(&engine).unwrap();
        register_imports(&mut linker).expect("yosh imports");
    }

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
}
