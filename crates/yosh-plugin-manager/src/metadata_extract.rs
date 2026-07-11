//! Extract `plugin-info` from a WebAssembly Component plugin without
//! granting it any host capability.
//!
//! See `docs/superpowers/specs/2026-04-27-wasm-plugin-runtime-design.md` §7
//! "Plugin manager pipeline" — the manager wants to display
//! `required-capabilities` and `implemented-hooks` in `yosh-plugin list`
//! without instantiating the plugin twice (once at sync, once at startup).
//! Extracting once here, caching in `plugins.lock`, lets `list` run
//! offline and lets the host trust the lockfile values.
//!
//! ## Sandboxing
//!
//! The metadata contract (WIT interface `plugin`) says "implementations
//! MUST NOT invoke any `yosh:plugin/*` host import from inside `metadata`."
//! The host enforces this with the same deny-stub pattern when its
//! `with_env` guard is not active (null env pointer). We enforce it here
//! by registering EVERY `yosh:plugin/*` import as a deny-stub returning
//! `Err(Denied)` regardless of input.
//!
//! For WASI we register the full Preview 2 sync surface (matching the
//! host's `build_linker`). Cargo-component-built plugins pull in
//! `wasi:io/*` and `wasi:cli/*` transitively through the Preview 1
//! adapter, regardless of whether their Rust source uses stdio. The
//! sandbox boundary is the empty `WasiCtx` (no preopens, no stdio, no
//! env, no args) — every WASI probe returns empty rather than failing
//! at link time. Selectively linking only clocks + random caused
//! issue #3: real plugins failed `instantiate_pre` and were silently
//! dropped from `plugins.lock`.
//!
//! ## Timeout
//!
//! The engine returned by `precompile::make_engine()` has
//! `epoch_interruption(true)`. A continuous tick thread
//! (`crate::tick::TickThread`) bumps the epoch every `TICK_MS` while the
//! call runs, so a hung `metadata()` call is interrupted within one tick
//! window of the configured deadline instead of the old one-shot
//! watchdog's multi-second worst case. A well-behaved plugin runs
//! `metadata` in microseconds.

use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

use crate::generated::yosh::plugin::commands::ExecOutput;
use crate::generated::yosh::plugin::files::{DirEntry, FileStat};
use crate::generated::yosh::plugin::types::{ErrorCode, HookName, IoStream};
use crate::generated::{PluginWorld, PluginWorldPre};

/// Per-store data for the metadata extraction sandbox. Carries a
/// fully-empty `WasiCtx` (no preopens, no env, no stdio mapping) so
/// every WASI probe returns empty data — the empty context, not
/// import-time link failure, is what isolates the plugin.
pub struct MetadataCtx {
    table: ResourceTable,
    wasi: WasiCtx,
}

impl Default for MetadataCtx {
    fn default() -> Self {
        // Defaults: no preopens, no env vars, no stdin/stdout/stderr, no
        // args. The full WASI Preview 2 surface is linked (see
        // `register_wasi`), but `wasi:cli/environment` returns an empty
        // list, `wasi:filesystem/preopens` is empty, `wasi:io` reads/
        // writes operate on no streams, etc. Only `wasi:clocks` and
        // `wasi:random` yield real values, which is harmless for a
        // metadata read.
        let wasi = WasiCtxBuilder::new().build();
        MetadataCtx {
            table: ResourceTable::new(),
            wasi,
        }
    }
}

impl WasiView for MetadataCtx {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

/// What the manager extracts from each plugin during sync.
#[derive(Debug, Clone)]
pub struct ExtractedMetadata {
    /// Plugin self-reported name. Useful sanity-check vs `plugins.toml`.
    pub name: String,
    pub version: String,
    pub commands: Vec<String>,
    pub required_capabilities: Vec<String>,
    pub implemented_hooks: Vec<String>,
}

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
    register_wasi(&mut linker).map_err(|e| format!("metadata: register WASI: {}", e))?;
    register_all_deny_imports(&mut linker)
        .map_err(|e| format!("metadata: register deny stubs: {}", e))?;

    let pre = PluginWorldPre::new(
        linker
            .instantiate_pre(component)
            .map_err(|e| format!("metadata: instantiate_pre: {}", e))?,
    )
    .map_err(|e| format!("metadata: bindings pre-init: {}", e))?;

    let mut store = Store::new(engine, MetadataCtx::default());
    // 100 ticks at 50ms ≈ 5s — same generous metadata budget as before,
    // now enforced by the continuous tick thread.
    store.set_epoch_deadline(100);
    let _tick = crate::tick::TickThread::spawn(engine.clone());

    let plugin_world: PluginWorld = pre
        .instantiate(&mut store)
        .map_err(|e| format!("metadata: instantiate: {}", e))?;

    let info = plugin_world
        .yosh_plugin_plugin()
        .call_metadata(&mut store)
        .map_err(|e| format!("metadata: call: {}", e))?;

    Ok(ExtractedMetadata {
        name: info.name,
        version: info.version,
        commands: info.commands,
        required_capabilities: info.required_capabilities,
        implemented_hooks: info
            .implemented_hooks
            .into_iter()
            .map(hook_name_to_string)
            .collect(),
    })
}

fn hook_name_to_string(h: HookName) -> String {
    match h {
        HookName::PreExec => "pre-exec".into(),
        HookName::PostExec => "post-exec".into(),
        HookName::OnCd => "on-cd".into(),
        HookName::PrePrompt => "pre-prompt".into(),
    }
}

/// Register the full WASI Preview 2 sync surface, matching the host's
/// `build_linker`. Cargo-component-built plugins transitively import
/// `wasi:io/*` and `wasi:cli/*` through the Preview 1 adapter, so a
/// narrower allowlist breaks `instantiate_pre`. Isolation is provided
/// by the empty `WasiCtx` constructed in `MetadataCtx::default`: every
/// probe returns empty data instead of failing at link time.
fn register_wasi(linker: &mut Linker<MetadataCtx>) -> wasmtime::Result<()> {
    wasmtime_wasi::add_to_linker_sync(linker)
}

/// Register every `yosh:plugin/*` import as a stub returning
/// `Err(Denied)`. The metadata contract forbids host calls during
/// `metadata()`; this is the active enforcement vs. the host's "null env
/// pointer" enforcement (both produce the same WIT result).
fn register_all_deny_imports(linker: &mut Linker<MetadataCtx>) -> wasmtime::Result<()> {
    let mut vars = linker.instance("yosh:plugin/variables@0.2.1")?;
    vars.func_wrap(
        "get",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>, (_,): (String,)| {
            Ok::<_, wasmtime::Error>((Err::<Option<String>, ErrorCode>(ErrorCode::Denied),))
        },
    )?;
    vars.func_wrap(
        "set",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>, (_, _): (String, String)| {
            Ok::<_, wasmtime::Error>((Err::<(), ErrorCode>(ErrorCode::Denied),))
        },
    )?;
    vars.func_wrap(
        "export-env",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>, (_, _): (String, String)| {
            Ok::<_, wasmtime::Error>((Err::<(), ErrorCode>(ErrorCode::Denied),))
        },
    )?;

    let mut fs = linker.instance("yosh:plugin/filesystem@0.2.1")?;
    fs.func_wrap(
        "cwd",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>, (): ()| {
            Ok::<_, wasmtime::Error>((Err::<String, ErrorCode>(ErrorCode::Denied),))
        },
    )?;
    fs.func_wrap(
        "set-cwd",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>, (_,): (String,)| {
            Ok::<_, wasmtime::Error>((Err::<(), ErrorCode>(ErrorCode::Denied),))
        },
    )?;

    let mut io = linker.instance("yosh:plugin/io@0.2.1")?;
    io.func_wrap(
        "write",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>, (_, _): (IoStream, Vec<u8>)| {
            Ok::<_, wasmtime::Error>((Err::<(), ErrorCode>(ErrorCode::Denied),))
        },
    )?;

    let mut files = linker.instance("yosh:plugin/files@0.2.1")?;
    files.func_wrap(
        "read-file",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>, (_,): (String,)| {
            Ok::<_, wasmtime::Error>((Err::<Vec<u8>, ErrorCode>(ErrorCode::Denied),))
        },
    )?;
    files.func_wrap(
        "read-dir",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>, (_,): (String,)| {
            Ok::<_, wasmtime::Error>((Err::<Vec<DirEntry>, ErrorCode>(ErrorCode::Denied),))
        },
    )?;
    files.func_wrap(
        "metadata",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>, (_,): (String,)| {
            Ok::<_, wasmtime::Error>((Err::<FileStat, ErrorCode>(ErrorCode::Denied),))
        },
    )?;
    files.func_wrap(
        "write-file",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>, (_, _): (String, Vec<u8>)| {
            Ok::<_, wasmtime::Error>((Err::<(), ErrorCode>(ErrorCode::Denied),))
        },
    )?;
    files.func_wrap(
        "append-file",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>, (_, _): (String, Vec<u8>)| {
            Ok::<_, wasmtime::Error>((Err::<(), ErrorCode>(ErrorCode::Denied),))
        },
    )?;
    files.func_wrap(
        "create-dir",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>, (_, _): (String, bool)| {
            Ok::<_, wasmtime::Error>((Err::<(), ErrorCode>(ErrorCode::Denied),))
        },
    )?;
    files.func_wrap(
        "remove-file",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>, (_,): (String,)| {
            Ok::<_, wasmtime::Error>((Err::<(), ErrorCode>(ErrorCode::Denied),))
        },
    )?;
    files.func_wrap(
        "remove-dir",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>, (_, _): (String, bool)| {
            Ok::<_, wasmtime::Error>((Err::<(), ErrorCode>(ErrorCode::Denied),))
        },
    )?;

    let mut commands = linker.instance("yosh:plugin/commands@0.2.1")?;
    commands.func_wrap(
        "exec",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>, (_, _): (String, Vec<String>)| {
            Ok::<_, wasmtime::Error>((Err::<ExecOutput, ErrorCode>(ErrorCode::Denied),))
        },
    )?;

    let mut settings = linker.instance("yosh:plugin/settings@0.2.1")?;
    settings.func_wrap(
        "read",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>, (): ()| {
            Ok::<_, wasmtime::Error>((Err::<Option<String>, ErrorCode>(ErrorCode::Denied),))
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_ctx_default_constructs() {
        let _c = MetadataCtx::default();
    }

    #[test]
    fn linker_registration_smoke() {
        let engine = crate::precompile::make_engine().unwrap();
        let mut linker = Linker::<MetadataCtx>::new(&engine);
        register_wasi(&mut linker).expect("wasi");
        register_all_deny_imports(&mut linker).expect("deny stubs");
    }
}
