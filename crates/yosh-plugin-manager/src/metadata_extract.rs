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
//! `Err(Denied)` regardless of input. WASI is restricted to clocks +
//! random (matching the host's permanent allowlist).
//!
//! ## Watchdog
//!
//! The engine in `precompile::make_metadata_engine()` has
//! `epoch_interruption(true)`. We bump the epoch from a detached thread
//! after 5 seconds to interrupt a hung `metadata()` call. A well-behaved
//! plugin runs `metadata` in microseconds.

use std::time::Duration;

use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

use crate::generated::yosh::plugin::types::{ErrorCode, HookName, IoStream};
use crate::generated::{PluginWorld, PluginWorldPre};

/// Per-store data for the metadata extraction sandbox. Carries a
/// fully-empty `WasiCtx` (no preopens, no env, no stdio mapping) so the
/// limited WASI surface still works but yields nothing useful — exactly
/// what we want for a metadata read.
pub struct MetadataCtx {
    table: ResourceTable,
    wasi: WasiCtx,
}

impl Default for MetadataCtx {
    fn default() -> Self {
        // Defaults: no preopens, no env vars, no stdin/stdout/stderr.
        // Plugins that try to read clocks/random get real values; anything
        // else from `wasi:cli`, `wasi:filesystem`, `wasi:sockets`, etc.
        // hits an unsatisfied import at link time.
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

/// Extract plugin metadata. Compiles the wasm with the given engine,
/// builds an all-deny linker, instantiates, calls `metadata()`, returns
/// the result. A 5-second epoch watchdog interrupts the call if the
/// plugin hangs.
pub fn extract(engine: &Engine, wasm_bytes: &[u8]) -> Result<ExtractedMetadata, String> {
    let component = Component::new(engine, wasm_bytes)
        .map_err(|e| format!("metadata: compile component: {}", e))?;

    let mut linker = Linker::<MetadataCtx>::new(engine);
    register_limited_wasi(&mut linker)
        .map_err(|e| format!("metadata: register WASI: {}", e))?;
    register_all_deny_imports(&mut linker)
        .map_err(|e| format!("metadata: register deny stubs: {}", e))?;

    let pre = PluginWorldPre::new(
        linker
            .instantiate_pre(&component)
            .map_err(|e| format!("metadata: instantiate_pre: {}", e))?,
    )
    .map_err(|e| format!("metadata: bindings pre-init: {}", e))?;

    let mut store = Store::new(engine, MetadataCtx::default());
    // Trip on the next epoch increment. The watchdog bumps after 5s.
    store.set_epoch_deadline(1);

    // Detached watchdog. We hold an Arc-clone of the engine so even if
    // the parent function returns first, the thread can still call
    // `increment_epoch` safely (no-op effect post-extraction).
    let watchdog_engine: Engine = engine.clone();
    let _watchdog = std::thread::Builder::new()
        .name("yosh-plugin-metadata-watchdog".to_string())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(5));
            // Engine::increment_epoch is cheap and idempotent. Calling it
            // after the host call has finished is harmless.
            watchdog_engine.increment_epoch();
        });

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

/// Register the same limited WASI surface the host allows: clocks +
/// random. NO `wasi:cli`, `wasi:filesystem`, `wasi:sockets` — a plugin
/// importing them will fail to link, which is the desired sandbox
/// behaviour.
fn register_limited_wasi(linker: &mut Linker<MetadataCtx>) -> wasmtime::Result<()> {
    use wasmtime_wasi::WasiImpl;
    use wasmtime_wasi::bindings::{clocks, random};

    let closure = type_annotate::<MetadataCtx, _>(|t| WasiImpl(t));
    clocks::wall_clock::add_to_linker_get_host(linker, closure)?;
    clocks::monotonic_clock::add_to_linker_get_host(linker, closure)?;
    random::random::add_to_linker_get_host(linker, closure)?;
    Ok(())
}

/// Pin the closure type for `add_to_linker_get_host`'s generic argument.
/// Same pattern as `src/plugin/linker.rs::type_annotate` in the host.
fn type_annotate<T, F>(val: F) -> F
where
    F: Fn(&mut T) -> wasmtime_wasi::WasiImpl<&mut T>,
{
    val
}

/// Register every `yosh:plugin/*` import as a stub returning
/// `Err(Denied)`. The metadata contract forbids host calls during
/// `metadata()`; this is the active enforcement vs. the host's "null env
/// pointer" enforcement (both produce the same WIT result).
fn register_all_deny_imports(linker: &mut Linker<MetadataCtx>) -> wasmtime::Result<()> {
    let mut vars = linker.instance("yosh:plugin/variables@0.1.0")?;
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

    let mut fs = linker.instance("yosh:plugin/filesystem@0.1.0")?;
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

    let mut io = linker.instance("yosh:plugin/io@0.1.0")?;
    io.func_wrap(
        "write",
        |_store: wasmtime::StoreContextMut<'_, MetadataCtx>,
         (_, _): (IoStream, Vec<u8>)| {
            Ok::<_, wasmtime::Error>((Err::<(), ErrorCode>(ErrorCode::Denied),))
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
        let engine = crate::precompile::make_metadata_engine().unwrap();
        let mut linker = Linker::<MetadataCtx>::new(&engine);
        register_limited_wasi(&mut linker).expect("limited wasi");
        register_all_deny_imports(&mut linker).expect("deny stubs");
    }
}
