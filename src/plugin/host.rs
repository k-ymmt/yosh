//! HostContext + WasiView impl + the real / deny implementations of the
//! `yosh:plugin/*` host imports.
//!
//! See `docs/superpowers/specs/2026-04-27-wasm-plugin-runtime-design.md`
//! §5 "Execution Model" — `HostContext`, `with_env`, and metadata contract
//! enforcement (host imports return `Err(Denied)` whenever the live
//! `ShellEnv` pointer is null, which is exactly the state during the
//! single `metadata()` call at startup).

use std::io::Write;

use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

use crate::env::ShellEnv;

use super::generated::yosh::plugin::types::{ErrorCode, IoStream};

/// Per-plugin store data. Carries:
///
/// * A raw `*mut ShellEnv` updated by `with_env` immediately before each
///   guest-bound call and reset to `null` on every exit path (see the
///   `EnvGuard` RAII guard in `mod.rs`). NULL during the single
///   `metadata()` call — enforces the metadata contract by short-circuiting
///   every host import to `Err(Denied)`.
/// * Plugin name and granted capability bitfield, used by deny-stubs and
///   diagnostics.
/// * `WasiCtx` + `ResourceTable` for the `wasi:clocks` / `wasi:random`
///   linker subset; the `WasiView` impl below exposes them per-store.
pub struct HostContext {
    /// Raw pointer to the live `ShellEnv`. Confined to a single `unsafe`
    /// helper (`env_mut`); this is the only `unsafe` site in the new host
    /// binding layer (vs. eight `unsafe extern "C" fn` callbacks in the
    /// dlopen version).
    pub(super) env: *mut ShellEnv,
    pub(super) plugin_name: String,
    pub(super) capabilities: u32,

    pub(super) wasi: WasiCtx,
    pub(super) resource_table: ResourceTable,
}

// SAFETY: `*mut ShellEnv` is `!Send` by default, but the pointer is only
// ever dereferenced via `with_env` which holds an `&mut ShellEnv` for the
// duration of the call. The shell is single-threaded for plugin dispatch
// (matches dlopen), and the pointer is null when no call is in progress.
unsafe impl Send for HostContext {}
// SAFETY: same rationale; we never share a `&HostContext` across threads
// in practice (per-store, single-threaded shell).
unsafe impl Sync for HostContext {}

impl HostContext {
    pub fn new_for_plugin(plugin_name: impl Into<String>, capabilities: u32) -> Self {
        // wasmtime-wasi 27 builder: defaults are sufficient (clocks use the
        // host clock, random is seeded; stdout/stderr are eaten — plugins
        // do their own host-side I/O via the `yosh:plugin/io` interface).
        let wasi = WasiCtxBuilder::new().build();
        HostContext {
            env: std::ptr::null_mut(),
            plugin_name: plugin_name.into(),
            capabilities,
            wasi,
            resource_table: ResourceTable::new(),
        }
    }

    /// Borrow the live `ShellEnv` if currently bound. Returns `None` when
    /// `env` is null (during `metadata()` calls or between `with_env`
    /// invocations).
    ///
    /// SAFETY: callers must hold exclusive access to the `Store<HostContext>`,
    /// which is implied by `&mut self` here. The pointer's lifetime is
    /// managed by `EnvGuard` in `mod.rs` and is guaranteed to be valid
    /// for the duration of any `with_env` callback.
    pub(super) fn env_mut(&mut self) -> Option<&mut ShellEnv> {
        if self.env.is_null() {
            None
        } else {
            // SAFETY: `EnvGuard::bind` set this pointer from a live
            // `&mut ShellEnv`; it is reset to null on guard drop. The
            // shell is single-threaded for plugin dispatch.
            Some(unsafe { &mut *self.env })
        }
    }
}

impl WasiView for HostContext {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.resource_table
    }
}

// ── yosh:plugin/variables host imports ──────────────────────────────────

/// `variables.get` — granted: read from `ShellEnv::vars`. Denied: log nothing,
/// return `Err(Denied)` (the WIT error value is the canonical signal).
pub(super) fn host_variables_get(
    ctx: &mut HostContext,
    name: String,
) -> Result<Option<String>, ErrorCode> {
    let Some(env) = ctx.env_mut() else {
        // metadata-contract enforcement OR null between calls.
        return Err(ErrorCode::Denied);
    };
    Ok(env.vars.get(&name).map(|s| s.to_string()))
}

pub(super) fn deny_variables_get(
    _ctx: &mut HostContext,
    _name: String,
) -> Result<Option<String>, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn host_variables_set(
    ctx: &mut HostContext,
    name: String,
    value: String,
) -> Result<(), ErrorCode> {
    let Some(env) = ctx.env_mut() else {
        return Err(ErrorCode::Denied);
    };
    env.vars
        .set(&name, &value)
        .map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn deny_variables_set(
    _ctx: &mut HostContext,
    _name: String,
    _value: String,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

/// `variables.export-env` — name in WIT is `export-env` (because `export`
/// is a reserved WIT keyword); the wit-bindgen-generated Rust function
/// is `export_env`.
pub(super) fn host_variables_export_env(
    ctx: &mut HostContext,
    name: String,
    value: String,
) -> Result<(), ErrorCode> {
    let Some(env) = ctx.env_mut() else {
        return Err(ErrorCode::Denied);
    };
    env.vars
        .set(&name, &value)
        .map_err(|_| ErrorCode::IoFailed)?;
    env.vars.export(&name);
    Ok(())
}

pub(super) fn deny_variables_export_env(
    _ctx: &mut HostContext,
    _name: String,
    _value: String,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

// ── yosh:plugin/filesystem host imports ─────────────────────────────────

pub(super) fn host_filesystem_cwd(ctx: &mut HostContext) -> Result<String, ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn deny_filesystem_cwd(_ctx: &mut HostContext) -> Result<String, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn host_filesystem_set_cwd(
    ctx: &mut HostContext,
    path: String,
) -> Result<(), ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    std::env::set_current_dir(&path).map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn deny_filesystem_set_cwd(
    _ctx: &mut HostContext,
    _path: String,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

// ── yosh:plugin/io host imports ─────────────────────────────────────────

pub(super) fn host_io_write(
    ctx: &mut HostContext,
    target: IoStream,
    data: Vec<u8>,
) -> Result<(), ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    let result = match target {
        IoStream::Stdout => std::io::stdout().write_all(&data),
        IoStream::Stderr => std::io::stderr().write_all(&data),
    };
    result.map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn deny_io_write(
    _ctx: &mut HostContext,
    _target: IoStream,
    _data: Vec<u8>,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

// ── yosh:plugin/files host imports (deny stubs only — real impls in Task 4) ──

use super::generated::yosh::plugin::files::{DirEntry, FileStat};

pub(super) fn deny_files_read_file(
    _ctx: &mut HostContext,
    _path: String,
) -> Result<Vec<u8>, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_read_dir(
    _ctx: &mut HostContext,
    _path: String,
) -> Result<Vec<DirEntry>, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_metadata(
    _ctx: &mut HostContext,
    _path: String,
) -> Result<FileStat, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_write_file(
    _ctx: &mut HostContext,
    _path: String,
    _data: Vec<u8>,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_append_file(
    _ctx: &mut HostContext,
    _path: String,
    _data: Vec<u8>,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_create_dir(
    _ctx: &mut HostContext,
    _path: String,
    _recursive: bool,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_remove_file(
    _ctx: &mut HostContext,
    _path: String,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_remove_dir(
    _ctx: &mut HostContext,
    _path: String,
    _recursive: bool,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

#[cfg(test)]
mod tests {
    //! Unit tests for the metadata contract: every host import must
    //! short-circuit to `Err(Denied)` when `HostContext.env` is null. This
    //! is the canonical enforcement point for the §5 metadata-cannot-reach-
    //! host-APIs invariant. The pointer is null during the single
    //! `metadata()` call at startup and between `with_env` invocations, so
    //! returning `Denied` from these functions blocks any plugin that tries
    //! to call them outside of a properly-bound dispatch.
    //!
    //! Replaces what would have been `tests/plugin.rs::t04_metadata_cannot_
    //! reach_host_apis` — a contrived plugin whose `metadata` calls `cwd()`
    //! is harder to author than this direct call. Same invariant, simpler
    //! test.
    use super::*;
    use yosh_plugin_api::CAP_ALL;

    fn null_env_ctx() -> HostContext {
        // Capabilities are deliberately CAP_ALL — the deny short-circuit
        // we are testing fires regardless of granted capabilities, because
        // it is enforced inside the *real* implementations. The deny stubs
        // would also return `Denied` but for a different reason.
        HostContext::new_for_plugin("<test>", CAP_ALL)
    }

    #[test]
    fn metadata_contract_real_cwd_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_filesystem_cwd(&mut ctx);
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_set_cwd_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_filesystem_set_cwd(&mut ctx, "/tmp".into());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_io_write_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_io_write(&mut ctx, IoStream::Stdout, b"hi".to_vec());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_variables_get_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_variables_get(&mut ctx, "PATH".into());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_variables_set_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_variables_set(&mut ctx, "FOO".into(), "bar".into());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_variables_export_env_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_variables_export_env(&mut ctx, "FOO".into(), "bar".into());
        assert_eq!(result, Err(ErrorCode::Denied));
    }
}
