//! HostContext + WasiView impl + the real / deny implementations of the
//! `yosh:plugin/*` host imports.
//!
//! See `docs/superpowers/specs/2026-04-27-wasm-plugin-runtime-design.md`
//! §5 "Execution Model" — `HostContext`, the metadata contract, and the
//! relationship to `EnvGuard` / the free `with_env` in `src/plugin/mod.rs`.
//!
//! Layout (per SP1 redesign,
//! docs/superpowers/specs/2026-05-06-sp1-plugin-host-redesign-design.md):
//! - this `mod.rs` owns `HostContext`, its `WasiView` impl, and the
//!   `ensure_bound` / `bound_env` helpers.
//! - During PR-A, `all.rs` is a temporary single-file home for every
//!   `host_*` and `deny_*` function with bodies preserved bit-for-bit.
//! - PR-B will split `all.rs` into per-capability submodules
//!   (`variables.rs`, `filesystem.rs`, `io.rs`, `files.rs`, `commands.rs`).

use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

use crate::env::ShellEnv;

use super::generated::yosh::plugin::types::ErrorCode;
use super::pattern::CommandPattern;

mod all;

pub(super) use all::{
    deny_commands_exec, deny_files_append_file, deny_files_create_dir, deny_files_metadata,
    deny_files_read_dir, deny_files_read_file, deny_files_remove_dir, deny_files_remove_file,
    deny_files_write_file, deny_filesystem_cwd, deny_filesystem_set_cwd, deny_io_write,
    deny_variables_export_env, deny_variables_get, deny_variables_set,
};
pub(super) use all::{
    host_commands_exec, host_files_append_file, host_files_create_dir, host_files_metadata,
    host_files_read_dir, host_files_read_file, host_files_remove_dir, host_files_remove_file,
    host_files_write_file, host_filesystem_cwd, host_filesystem_set_cwd, host_io_write,
    host_variables_export_env, host_variables_get, host_variables_set,
};

/// Per-plugin store data. See module docstring for invariants.
pub struct HostContext {
    /// Raw pointer to the live `ShellEnv`. Confined to a single `unsafe`
    /// helper (`env_mut`); this is the only `unsafe` site in the new host
    /// binding layer.
    pub(super) env: *mut ShellEnv,
    #[allow(dead_code)]
    pub(super) plugin_name: String,
    #[allow(dead_code)]
    pub(super) capabilities: u32,

    pub(super) wasi: WasiCtx,
    pub(super) resource_table: ResourceTable,
    pub(super) allowed_commands: Vec<CommandPattern>,
}

// SAFETY: `*mut ShellEnv` is `!Send` by default, but the pointer is only
// ever dereferenced via `env_mut` / `bound_env`, both of which require
// `&mut HostContext`. The shell is single-threaded for plugin dispatch
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
            allowed_commands: Vec::new(),
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

    /// Metadata-contract guard: returns `Err(Denied)` if env is null
    /// (during `metadata()` or between `with_env` invocations), `Ok(())`
    /// otherwise. Used by host functions that do not need to read or
    /// write `ShellEnv` but still must reject calls during the
    /// metadata phase.
    pub(super) fn ensure_bound(&self) -> Result<(), ErrorCode> {
        if self.env.is_null() {
            Err(ErrorCode::Denied)
        } else {
            Ok(())
        }
    }

    /// Metadata-contract guard that also returns the bound `&mut ShellEnv`.
    /// Returns `Err(Denied)` if env is null. Used by host functions that
    /// need to read or write shell state.
    pub(super) fn bound_env(&mut self) -> Result<&mut ShellEnv, ErrorCode> {
        self.env_mut().ok_or(ErrorCode::Denied)
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
