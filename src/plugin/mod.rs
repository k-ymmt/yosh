//! Plugin runtime: wasmtime Component Model.
//!
//! Replaces the dlopen-era `libloading` implementation with a sandboxed
//! WebAssembly Component Model runtime. See
//! `docs/superpowers/specs/2026-04-27-wasm-plugin-runtime-design.md` for
//! the full design.
//!
//! Pipeline:
//!
//! 1. `PluginManager::new()` is cheap; the shared `wasmtime::Engine` (and
//!    its epoch tick thread) is built lazily on the first plugin load, so
//!    plugin-free shells never pay for it.
//! 2. For each enabled `plugins.toml` entry, `load_plugin` either uses the
//!    `.cwasm` cache (if all 5 trust conditions hold) or precompiles in-memory.
//! 3. Per-plugin `Store<HostContext>` is created once and reused for every
//!    `exec` / hook dispatch.
//! 4. `with_env` is the single dispatch wrapper. An `EnvGuard` RAII guard
//!    binds a raw `*mut ShellEnv` for the duration of the callback and
//!    resets to null on every exit path (Ok/Err/panic). The pointer is the
//!    only `unsafe` site in the binding layer.
//! 5. `exec_command` returns a 3-valued `PluginExec` so callers in
//!    `src/exec/` cannot accidentally fall through to PATH lookup when a
//!    plugin handler exists but failed.
//!
//! Submodules: `engine` (lazy engine + epoch tick thread), `deadline`
//! (epoch-deadline constants, pre-prompt timeout), `load` (plugin
//! loading), `capabilities` (capability negotiation), `invoke`
//! (`EnvGuard` / `with_env` guest-call wrappers), `dispatch` (command
//! and hook dispatch).

pub mod cache;
mod capabilities;
pub mod config;
mod deadline;
mod dispatch;
mod engine;
mod host;
mod invoke;
pub mod limits;
mod linker;
mod load;
pub mod pattern;
#[cfg(any(test, feature = "test-helpers"))]
#[allow(dead_code)]
// exercised from integration tests in tests/plugin.rs and benches/plugin_bench.rs
pub mod test_helpers;

use wasmtime::component::Linker;
use wasmtime::{Engine, Store};

use self::engine::TickThread;
use self::host::HostContext;

// `limits` reaches the tick interval as `super::TICK_MS`.
use self::deadline::TICK_MS;

// ── wasmtime bindgen for our WIT contract ───────────────────────────────
//
// The path is `wit/` inside the yosh crate. The canonical source is
// `crates/yosh-plugin-api/wit/yosh-plugin.wit`; `build.rs` verifies the
// bundled copy matches when built inside the workspace. The copy is
// required because `cargo install yosh` extracts the yosh crate
// standalone, with no `crates/` subtree alongside it.
mod generated {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "plugin-world",
    });
}

use self::generated::PluginWorld;
use self::generated::yosh::plugin::types::{HookName, PluginInfo};

// ── Public types ────────────────────────────────────────────────────────

/// Result of attempting to dispatch a command to the plugin layer.
///
/// Distinguishes "no plugin claimed the name" (caller should fall through
/// to PATH lookup) from "a plugin claimed it but failed" (caller must NOT
/// fall through — the plugin owned the command). See spec §5.
#[derive(Debug)]
pub enum PluginExec {
    /// No plugin provides this command. The caller falls back to PATH.
    NotHandled,
    /// A plugin handled the command and returned this exit status.
    Handled(i32),
    /// A plugin claimed the command but failed (trap, host error, invalidated).
    Failed,
}

/// A loaded plugin: its persistent store, bindings handle, and metadata.
struct LoadedPlugin {
    pub(super) name: String,
    store: Store<HostContext>,
    bindings: PluginWorld,
    plugin_info: PluginInfo,
    /// Granted capability bitfield (after allowlist intersection with
    /// `required-capabilities`).
    capabilities: u32,
    /// Resolved runtime resource limits (memory cap + per-entry-point
    /// timeouts). Fixed at load time.
    limits: limits::PluginLimits,
    /// Set by `with_env` on guest trap. All subsequent dispatches for this
    /// plugin short-circuit with a single skip warning.
    invalidated: bool,
}

impl LoadedPlugin {
    fn provides_command(&self, name: &str) -> bool {
        self.plugin_info.commands.iter().any(|c| c == name)
    }

    fn implements_hook(&self, hook: HookName) -> bool {
        self.plugin_info.implemented_hooks.contains(&hook)
    }
}

/// Manages loaded plugins and dispatches commands/hooks.
pub struct PluginManager {
    /// Lazily-initialized wasmtime `Engine`. `None` until the first
    /// plugin load: a shell with no plugins configured (the common case,
    /// including every command-substitution child) must not pay for
    /// engine construction or spawn the epoch tick thread — the Drop
    /// join of an eagerly-spawned tick thread alone added ~50ms to every
    /// clean shell exit.
    engine: Option<Engine>,
    /// Stable string fingerprint of the engine config; folded into the
    /// `engine_config_hash` field of the `CacheKey` tuple.
    engine_fingerprint: String,
    plugins: Vec<LoadedPlugin>,
    /// Resolved pre-prompt timeout in milliseconds, captured once at
    /// construction from `YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS`.
    pre_prompt_timeout_ms: u64,
    /// Linkers keyed by negotiated `effective_capabilities`. Both the
    /// metadata-probe scratch linker (always `CAP_ALL`) and per-plugin
    /// real linkers share this cache; an entry is built lazily on first
    /// load that needs it. The Linker is plugin-independent: it depends
    /// only on the engine and the cap bitfield, so two plugins granted
    /// the same caps share one cached linker. See report §4.2 fix#2 and
    /// `docs/superpowers/specs/2026-05-09-plugin-real-linker-cache-design.md`.
    pub(super) linker_cache: std::collections::HashMap<u32, Linker<HostContext>>,
    /// Background epoch-tick thread. Spawned together with the lazy
    /// engine on first plugin load; joined on `Drop`.
    tick_thread: Option<TickThread>,
}

impl PluginManager {
    /// Construct an empty manager. Deliberately cheap: no wasmtime
    /// `Engine` and no tick thread are created until the first plugin
    /// load (`ensure_engine`), so the no-plugins case — every plain
    /// `yosh -c` invocation and every command-substitution child — pays
    /// nothing here beyond an env-var parse.
    pub fn new() -> Self {
        // Stable fingerprint: covers the flags relevant to cwasm
        // compatibility. Any change to this string invalidates every
        // cached cwasm via `engine_config_hash`. The canonical literal
        // lives in `yosh_plugin_manager::precompile::ENGINE_FINGERPRINT`
        // so both sides cannot drift.
        let engine_fingerprint = yosh_plugin_manager::precompile::ENGINE_FINGERPRINT.to_string();

        // Resolve the pre-prompt timeout once (invalid values warn and
        // fall back to the default).
        let pre_prompt_timeout_ms = deadline::pre_prompt_timeout_from_env();

        PluginManager {
            engine: None,
            engine_fingerprint,
            plugins: Vec::new(),
            pre_prompt_timeout_ms,
            linker_cache: std::collections::HashMap::new(),
            tick_thread: None,
        }
    }

    /// Engine fingerprint used in cache key tuples. Exposed for the manager
    /// in Task 5 so it precompiles into a key matching the host's runtime.
    #[allow(dead_code)] // public manager API; consumed by yosh-plugin sync
    pub fn engine_fingerprint(&self) -> &str {
        &self.engine_fingerprint
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

// `Drop for PluginManager` joins the tick thread. The note about
// `unload_all` requiring `&mut ShellEnv` still applies — plugin
// `on_unload` callbacks must be invoked from the shell main loop
// before the manager is dropped. Drop here only cleans up the
// tick-thread background resource, which has no `ShellEnv` dependency.
impl Drop for PluginManager {
    fn drop(&mut self) {
        // Setting tick_thread to None drops the TickThread, which
        // signals stop and joins the worker. We do this explicitly
        // (rather than relying on the field's natural drop order) so
        // the join is the first thing that happens when this impl
        // runs — keeps the intent local to the body.
        self.tick_thread = None;
    }
}
