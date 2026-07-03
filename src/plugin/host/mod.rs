//! HostContext + WasiView impl + the real / deny implementations of the
//! `yosh:plugin/*` host imports.
//!
//! See `docs/superpowers/specs/2026-04-27-wasm-plugin-runtime-design.md`
//! §5 "Execution Model" — `HostContext`, the metadata contract, and the
//! relationship to `EnvGuard` / the free `with_env` in `src/plugin/mod.rs`.
//!
//! Layout (per SP1 redesign,
//! docs/superpowers/specs/2026-05-06-sp1-plugin-host-redesign-design.md):
//! - this `mod.rs` owns `HostContext`, its `WasiView` impl, the
//!   `ensure_bound` / `bound_env_ref` / `bound_env_with` helpers, and
//!   helper tests.
//! - Per-capability submodules (`variables.rs`, `filesystem.rs`, `io.rs`,
//!   `files.rs`, `commands.rs`) own every `host_*` and `deny_*` function.

use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

use crate::env::ShellEnv;

use super::generated::yosh::plugin::types::ErrorCode;
use super::pattern::CommandPattern;

mod commands;
mod files;
mod filesystem;
mod io;
mod variables;

pub(super) use commands::{deny_commands_exec, host_commands_exec};
pub(super) use files::{
    deny_files_append_file, deny_files_create_dir, deny_files_metadata, deny_files_read_dir,
    deny_files_read_file, deny_files_remove_dir, deny_files_remove_file, deny_files_write_file,
    host_files_append_file, host_files_create_dir, host_files_metadata, host_files_read_dir,
    host_files_read_file, host_files_remove_dir, host_files_remove_file, host_files_write_file,
};
pub(super) use filesystem::{
    deny_filesystem_cwd, deny_filesystem_set_cwd, host_filesystem_cwd, host_filesystem_set_cwd,
};
pub(super) use io::{deny_io_write, host_io_write};
pub(super) use variables::{
    deny_variables_export_env, deny_variables_get, deny_variables_set, host_variables_export_env,
    host_variables_get, host_variables_set,
};

/// Per-plugin store data. See module docstring for invariants.
pub struct HostContext {
    /// Raw pointer to the live `ShellEnv`. Dereferenced only inside the
    /// `bound_env_ref` / `bound_env_with` helpers — these are the only
    /// `unsafe` sites in the host binding layer.
    pub(super) env: *mut ShellEnv,
    #[allow(dead_code)]
    pub(super) plugin_name: String,
    #[allow(dead_code)]
    pub(super) capabilities: u32,

    pub(super) wasi: WasiCtx,
    pub(super) resource_table: ResourceTable,
    pub(super) allowed_commands: Vec<CommandPattern>,
    /// Optional confinement root for the `files` capability
    /// (`plugins.toml` `files_root`). `None` means the capability is a
    /// full-filesystem grant; `Some(root)` confines every `files` host
    /// call to paths inside `root` (canonicalized, symlink-escape safe).
    pub(super) files_root: Option<std::path::PathBuf>,
}

// SAFETY: `*mut ShellEnv` is `!Send` by default, but the pointer is only
// ever dereferenced inside `bound_env_ref` / `bound_env_with`, which gate
// access on a non-null check and rely on `EnvGuard` to bound the
// pointer's lifetime. The shell is single-threaded for plugin dispatch
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
            files_root: None,
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

    /// Read-only borrow of the bound `ShellEnv`. Returns `Err(Denied)` if
    /// env is null. Used by host functions that only need to read shell
    /// state and want to keep the underlying `Store` borrow shared (e.g.
    /// so a `WasmStr` borrowed from the same store can coexist with the
    /// env borrow).
    ///
    /// SAFETY: the pointer is non-null only while `EnvGuard` keeps the
    /// bound `&mut ShellEnv` alive, and plugin dispatch is
    /// single-threaded.
    pub(super) fn bound_env_ref(&self) -> Result<&ShellEnv, ErrorCode> {
        if self.env.is_null() {
            Err(ErrorCode::Denied)
        } else {
            // SAFETY: `EnvGuard::bind` set this pointer from a live
            // `&mut ShellEnv`; it is reset to null on guard drop. The
            // shell is single-threaded for plugin dispatch.
            Ok(unsafe { &*self.env })
        }
    }

    /// Closure-style mutable env access. Used by host functions that
    /// must mutate `ShellEnv` while a wasmtime store borrow (e.g. from
    /// a `WasmStr::to_str` `Cow`) is held immutably. The mutation goes
    /// through the raw `*mut ShellEnv` so the wasmtime store's borrow
    /// state is unaffected.
    ///
    /// SAFETY: same invariants as `bound_env_ref` — pointer is non-null
    /// only while `EnvGuard` keeps the bound `&mut ShellEnv` alive, and
    /// plugin dispatch is single-threaded.
    pub(super) fn bound_env_with<R, F>(&self, f: F) -> Result<R, ErrorCode>
    where
        F: FnOnce(&mut ShellEnv) -> R,
    {
        if self.env.is_null() {
            Err(ErrorCode::Denied)
        } else {
            // SAFETY: `EnvGuard::bind` set this pointer from a live
            // `&mut ShellEnv`; it is reset to null on guard drop.
            // Plugin dispatch is single-threaded.
            Ok(f(unsafe { &mut *self.env }))
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

#[cfg(test)]
pub(super) mod test_helpers {
    //! Test fixtures shared by every capability submodule.
    //!
    //! `null_env_ctx` produces a `HostContext` with `env = null` to
    //! exercise the metadata-contract guard. `bound_env_ctx` binds a
    //! real `ShellEnv` so happy-path tests can proceed past the guard.
    //! `ctx_with_allowed` adds command-pattern allowlists for
    //! `commands_exec` tests.

    use super::super::pattern::CommandPattern;
    use super::HostContext;
    use crate::env::ShellEnv;
    use yosh_plugin_api::CAP_ALL;

    pub fn null_env_ctx() -> HostContext {
        // CAP_ALL is intentional — the deny short-circuit we test
        // fires regardless of granted capabilities, because it lives
        // inside the *real* implementations.
        HostContext::new_for_plugin("<test>", CAP_ALL)
    }

    pub fn bound_env_ctx(env: &mut ShellEnv) -> HostContext {
        let mut ctx = HostContext::new_for_plugin("<test>", CAP_ALL);
        ctx.env = env as *mut ShellEnv;
        ctx
    }

    pub fn ctx_with_allowed(env: &mut ShellEnv, patterns: &[&str]) -> HostContext {
        let mut ctx = bound_env_ctx(env);
        ctx.allowed_commands = patterns
            .iter()
            .map(|s| CommandPattern::parse(s).expect("valid pattern"))
            .collect();
        ctx
    }

    pub fn ctx_with_files_root(env: &mut ShellEnv, root: &std::path::Path) -> HostContext {
        let mut ctx = bound_env_ctx(env);
        ctx.files_root = Some(root.to_path_buf());
        ctx
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the metadata-contract guard (`ensure_bound`). This
    //! is the canonical enforcement point for the §5
    //! metadata-cannot-reach-host-APIs invariant. Per-capability
    //! submodules also keep one spot test confirming the deny path is
    //! reachable through the host function, but the structural guarantee
    //! is verified here.

    use super::super::generated::yosh::plugin::types::ErrorCode;
    use super::test_helpers::{bound_env_ctx, null_env_ctx};
    use crate::env::ShellEnv;

    #[test]
    fn ensure_bound_returns_denied_when_env_null() {
        let ctx = null_env_ctx();
        assert_eq!(ctx.ensure_bound(), Err(ErrorCode::Denied));
    }

    #[test]
    fn ensure_bound_returns_ok_when_env_bound() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = bound_env_ctx(&mut env);
        assert_eq!(ctx.ensure_bound(), Ok(()));
    }
}
