//! Plugin runtime: wasmtime Component Model.
//!
//! Replaces the dlopen-era `libloading` implementation with a sandboxed
//! WebAssembly Component Model runtime. See
//! `docs/superpowers/specs/2026-04-27-wasm-plugin-runtime-design.md` for
//! the full design.
//!
//! Pipeline:
//!
//! 1. `PluginManager::new()` builds a shared `wasmtime::Engine`.
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

pub mod cache;
pub mod config;
mod host;
mod linker;

use std::path::Path;

use wasmtime::{Engine, Store};
use wasmtime::component::Component;

use yosh_plugin_api::{
    CAP_ALL, CAP_HOOK_ON_CD, CAP_HOOK_POST_EXEC, CAP_HOOK_PRE_EXEC, CAP_HOOK_PRE_PROMPT,
    Capability, parse_capability,
};

use crate::env::ShellEnv;

use self::cache::{CacheKey, CacheRejection, sha256_hex, sidecar_path, validate_cwasm};
use self::config::{PluginConfig, expand_tilde};
use self::host::HostContext;

// ── wasmtime bindgen for our WIT contract ───────────────────────────────
//
// The path is relative to the root yosh crate's `Cargo.toml`. Macros
// resolve paths from `CARGO_MANIFEST_DIR`, which for this crate is the
// repo root.
mod generated {
    wasmtime::component::bindgen!({
        path: "crates/yosh-plugin-api/wit",
        world: "plugin-world",
    });
}

use self::generated::{PluginWorld, PluginWorldPre};
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
    engine: Engine,
    /// Stable string fingerprint of the engine config; folded into the
    /// `engine_config_hash` field of the `CacheKey` tuple.
    engine_fingerprint: String,
    plugins: Vec<LoadedPlugin>,
}

impl PluginManager {
    pub fn new() -> Self {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(false);
        config.consume_fuel(false);
        // Best-effort: enable system cache. If unavailable we just proceed
        // without it. cwasm precompile is the durable cache; this is the
        // lower-level wasmtime cranelift cache.
        let _ = config.cache_config_load_default();

        // Stable fingerprint: covers the flags relevant to cwasm
        // compatibility. Any change to this string invalidates every
        // cached cwasm via `engine_config_hash`.
        let engine_fingerprint =
            "v1;component_model=true;async=false;fuel=false;cranelift".to_string();

        let engine = Engine::new(&config).expect("wasmtime Engine::new");

        PluginManager {
            engine,
            engine_fingerprint,
            plugins: Vec::new(),
        }
    }

    /// Load plugins listed in the config file. Errors are printed to stderr
    /// and the failing plugin is skipped.
    pub fn load_from_config(&mut self, config_path: &Path, env: &mut ShellEnv) {
        let config = match PluginConfig::load(config_path) {
            Ok(c) => c,
            Err(_) => return,
        };
        for entry in &config.plugin {
            if !entry.enabled {
                continue;
            }
            let path = expand_tilde(&entry.path);
            let config_caps = entry
                .capabilities
                .as_ref()
                .map(|strs| config::capabilities_from_strs(strs));
            if let Err(e) = self.load_one(
                &path,
                env,
                config_caps,
                entry.cwasm_path.as_deref(),
                entry.cache_key.as_ref(),
            ) {
                eprintln!("yosh: plugin: {}", e);
            }
        }
    }

    /// Load a single plugin from a wasm component path. Grants every
    /// capability the plugin's `plugin-info.required-capabilities` lists
    /// (no further restriction — equivalent to `plugins.toml` without a
    /// `capabilities = [...]` field). Always falls back to in-memory
    /// compile (no cwasm cache lookup).
    pub fn load_plugin(&mut self, path: &Path, env: &mut ShellEnv) -> Result<(), String> {
        self.load_one(path, env, None, None, None)
    }

    /// Load one plugin.
    ///   * `config_capabilities`: `None` → grant every requested capability;
    ///     `Some(bits)` → intersect requested with `bits`.
    ///   * `cwasm_path` + `expected_key`: when both are present, attempt to
    ///     `Component::deserialize` from the trusted cache instead of
    ///     re-compiling the wasm bytes. Any of the 5 trust conditions
    ///     failing falls back to in-memory compile with a warning.
    pub(super) fn load_one(
        &mut self,
        path: &Path,
        env: &mut ShellEnv,
        config_capabilities: Option<u32>,
        cwasm_path: Option<&Path>,
        expected_key: Option<&CacheKey>,
    ) -> Result<(), String> {
        // 1. Read the wasm bytes (needed for SHA verify and/or in-memory compile).
        let wasm_bytes = std::fs::read(path)
            .map_err(|e| format!("{}: {}", path.display(), e))?;

        // 2. If the lockfile pinned a SHA, verify the on-disk wasm matches
        //    BEFORE trusting any cwasm. Per spec §5 step 1: this check is
        //    unconditional. A mismatch refuses the load (does NOT silently
        //    fall back to a cached cwasm).
        if let Some(key) = expected_key {
            let actual = sha256_hex(&wasm_bytes);
            if actual != key.wasm_sha256 {
                return Err(format!(
                    "{}: wasm SHA-256 mismatch (lockfile {}, actual {}); \
                     refusing to load. Run 'yosh-plugin sync' to refresh.",
                    path.display(),
                    &key.wasm_sha256,
                    &actual,
                ));
            }
        }

        // 3. Build the component. Try the cwasm cache first when the
        //    lockfile points at one; fall back to in-memory compile on any
        //    trust-condition failure.
        let component = match (cwasm_path, expected_key) {
            (Some(cwasm), Some(lockfile_key)) => {
                let sidecar = sidecar_path(cwasm);
                let runtime_key = CacheKey::for_runtime(
                    lockfile_key.wasm_sha256.clone(),
                    &self.engine_fingerprint,
                );
                match validate_cwasm(cwasm, &sidecar, path, &runtime_key) {
                    Ok(()) => {
                        let cwasm_bytes = std::fs::read(cwasm).map_err(|e| {
                            format!("{}: cwasm read failed: {}", cwasm.display(), e)
                        })?;
                        // SAFETY: validate_cwasm returned Ok, which enforces
                        // all 5 spec §5 trust conditions: same-uid ownership,
                        // file mode 0600, parent-dir mode 0700, sidecar key
                        // tuple match, and source wasm SHA-256 still matches.
                        // Together these establish that the cwasm bytes were
                        // produced by THIS user's previous yosh-plugin sync
                        // for THIS wasm, on this same host with this same
                        // wasmtime version. That is the trust boundary
                        // Component::deserialize requires.
                        unsafe { Component::deserialize(&self.engine, &cwasm_bytes) }
                            .map_err(|e| {
                                format!("{}: cwasm deserialize failed: {}", cwasm.display(), e)
                            })?
                    }
                    Err(reason) => {
                        eprintln!(
                            "yosh: plugin '{}': cwasm cache stale ({}); \
                             precompiling in memory (run 'yosh-plugin sync' to refresh)",
                            path.display(),
                            reason.as_str(),
                        );
                        Component::new(&self.engine, &wasm_bytes).map_err(|e| {
                            format!("{}: component compile failed: {}", path.display(), e)
                        })?
                    }
                }
            }
            _ => Component::new(&self.engine, &wasm_bytes)
                .map_err(|e| format!("{}: component compile failed: {}", path.display(), e))?,
        };

        // 3. Build a permissive linker first so we can call `metadata` to
        //    learn the plugin's requested capabilities. The metadata
        //    contract (host imports return `Err(Denied)` on null env) makes
        //    this safe — even a permissive linker rejects host calls during
        //    `metadata`.
        let scratch_linker = linker::build_linker(&self.engine, CAP_ALL)
            .map_err(|e| format!("{}: linker init failed: {}", path.display(), e))?;
        let scratch_pre = PluginWorldPre::new(
            scratch_linker
                .instantiate_pre(&component)
                .map_err(|e| format!("{}: instantiate_pre failed: {}", path.display(), e))?,
        )
        .map_err(|e| format!("{}: bindings pre-init failed: {}", path.display(), e))?;

        let mut scratch_store = Store::new(
            &self.engine,
            HostContext::new_for_plugin("<probing>", CAP_ALL),
        );
        let scratch_world = scratch_pre
            .instantiate(&mut scratch_store)
            .map_err(|e| format!("{}: instantiate failed: {}", path.display(), e))?;
        // env pointer is null in scratch_store — the deny short-circuit on
        // null env is what enforces the metadata contract.
        let plugin_info = scratch_world
            .yosh_plugin_plugin()
            .call_metadata(&mut scratch_store)
            .map_err(|e| format!("{}: metadata trap: {}", path.display(), e))?;

        // 4. Negotiate capabilities. Parse the strings from `plugin-info`,
        //    intersect with the config allowlist, log denied bits.
        let requested_capabilities = parse_required_capabilities(&plugin_info, &plugin_info.name);
        let effective_capabilities = match config_capabilities {
            None => requested_capabilities,
            Some(allow) => {
                let effective = requested_capabilities & allow;
                let denied = requested_capabilities & !effective;
                if denied != 0 {
                    log_denied_capabilities(&plugin_info.name, denied);
                }
                effective
            }
        };

        // 5. Build the real linker with the negotiated capability mask,
        //    create a fresh store, instantiate, and call on_load under
        //    with_env so the plugin can use its granted host imports.
        let real_linker = linker::build_linker(&self.engine, effective_capabilities)
            .map_err(|e| format!("{}: linker build failed: {}", path.display(), e))?;
        let real_pre = PluginWorldPre::new(
            real_linker
                .instantiate_pre(&component)
                .map_err(|e| format!("{}: real instantiate_pre: {}", path.display(), e))?,
        )
        .map_err(|e| format!("{}: real bindings pre-init: {}", path.display(), e))?;

        let mut store = Store::new(
            &self.engine,
            HostContext::new_for_plugin(plugin_info.name.clone(), effective_capabilities),
        );
        let bindings = real_pre
            .instantiate(&mut store)
            .map_err(|e| format!("{}: real instantiate: {}", path.display(), e))?;

        // 6. on_load under with_env (host imports available).
        let on_load_result = {
            let mut guard = EnvGuard::bind(&mut store, env);
            bindings.yosh_plugin_plugin().call_on_load(guard.store())
        };
        match on_load_result {
            Ok(Ok(())) => {}
            Ok(Err(msg)) => {
                return Err(format!("{}: on_load returned error: {}", plugin_info.name, msg));
            }
            Err(e) => {
                return Err(format!("{}: on_load trap: {}", plugin_info.name, e));
            }
        }

        // 7. Stash.
        self.plugins.push(LoadedPlugin {
            name: plugin_info.name.clone(),
            store,
            bindings,
            plugin_info,
            capabilities: effective_capabilities,
            invalidated: false,
        });

        Ok(())
    }

    /// Dispatch a command name to the plugin layer.
    ///
    /// See `PluginExec` for the three-valued return semantics.
    pub fn exec_command(
        &mut self,
        env: &mut ShellEnv,
        name: &str,
        args: &[String],
    ) -> PluginExec {
        let Some(idx) = self.plugins.iter().position(|p| p.provides_command(name)) else {
            return PluginExec::NotHandled;
        };
        let plugin = &mut self.plugins[idx];
        match with_env(plugin, env, |bindings, store| {
            bindings.yosh_plugin_plugin().call_exec(store, name, args)
        }) {
            Some(exit) => PluginExec::Handled(exit),
            None => PluginExec::Failed,
        }
    }

    pub fn call_pre_exec(&mut self, env: &mut ShellEnv, cmd: &str) {
        for plugin in &mut self.plugins {
            if plugin.capabilities & CAP_HOOK_PRE_EXEC == 0 {
                continue;
            }
            if !plugin.implements_hook(HookName::PreExec) {
                continue;
            }
            let _ = with_env(plugin, env, |bindings, store| {
                bindings.yosh_plugin_hooks().call_pre_exec(store, cmd)
            });
        }
    }

    pub fn call_post_exec(&mut self, env: &mut ShellEnv, cmd: &str, exit_code: i32) {
        for plugin in &mut self.plugins {
            if plugin.capabilities & CAP_HOOK_POST_EXEC == 0 {
                continue;
            }
            if !plugin.implements_hook(HookName::PostExec) {
                continue;
            }
            let _ = with_env(plugin, env, |bindings, store| {
                bindings
                    .yosh_plugin_hooks()
                    .call_post_exec(store, cmd, exit_code)
            });
        }
    }

    pub fn call_on_cd(&mut self, env: &mut ShellEnv, old_dir: &str, new_dir: &str) {
        for plugin in &mut self.plugins {
            if plugin.capabilities & CAP_HOOK_ON_CD == 0 {
                continue;
            }
            if !plugin.implements_hook(HookName::OnCd) {
                continue;
            }
            let _ = with_env(plugin, env, |bindings, store| {
                bindings
                    .yosh_plugin_hooks()
                    .call_on_cd(store, old_dir, new_dir)
            });
        }
    }

    pub fn call_pre_prompt(&mut self, env: &mut ShellEnv) {
        for plugin in &mut self.plugins {
            if plugin.capabilities & CAP_HOOK_PRE_PROMPT == 0 {
                continue;
            }
            if !plugin.implements_hook(HookName::PrePrompt) {
                continue;
            }
            let _ = with_env(plugin, env, |bindings, store| {
                bindings.yosh_plugin_hooks().call_pre_prompt(store)
            });
        }
    }

    /// Call `on_unload` on every plugin and drop them. Best-effort: a trap
    /// in `on_unload` is logged and the plugin is dropped anyway.
    pub fn unload_all(&mut self, env: &mut ShellEnv) {
        // Drain so the borrow checker lets us call `with_env` on each.
        let mut plugins = std::mem::take(&mut self.plugins);
        for plugin in &mut plugins {
            if plugin.invalidated {
                continue;
            }
            let _ = with_env(plugin, env, |bindings, store| {
                bindings.yosh_plugin_plugin().call_on_unload(store)
            });
        }
        // plugins drops here, releasing every Store and underlying instance.
        drop(plugins);
    }

    /// Check if any plugin provides the given command.
    pub fn has_command(&self, name: &str) -> bool {
        self.plugins.iter().any(|p| p.provides_command(name))
    }

    /// Engine fingerprint used in cache key tuples. Exposed for the manager
    /// in Task 5 so it precompiles into a key matching the host's runtime.
    pub fn engine_fingerprint(&self) -> &str {
        &self.engine_fingerprint
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

// Note: no `Drop` impl — `unload_all` requires `&mut ShellEnv`, which we
// can't synthesize at drop time. The shell's main loop must call
// `unload_all` explicitly before dropping the manager. The `Store`s drop
// without calling `on_unload`, which matches a hard process exit.

// ── EnvGuard + with_env ────────────────────────────────────────────────

/// RAII guard that binds a raw `*mut ShellEnv` into the `Store`'s
/// `HostContext` and resets it to null on drop. Drop runs on every exit
/// path: normal return, `Err`, host-side panic, trap unwind.
struct EnvGuard<'a> {
    store: &'a mut Store<HostContext>,
}

impl<'a> EnvGuard<'a> {
    fn bind(store: &'a mut Store<HostContext>, env: &mut ShellEnv) -> Self {
        store.data_mut().env = env as *mut _;
        EnvGuard { store }
    }

    fn store(&mut self) -> &mut Store<HostContext> {
        self.store
    }
}

impl Drop for EnvGuard<'_> {
    fn drop(&mut self) {
        // Always restores env to null, even during unwinding. Drop itself
        // cannot panic because pointer assignment is infallible.
        self.store.data_mut().env = std::ptr::null_mut();
    }
}

/// Canonical dispatch wrapper for any guest-bound call that needs host
/// API access. Sets up `EnvGuard`, runs the callback, and converts
/// `wasmtime::Error` into either a logged-and-invalidated `None` (trap)
/// or a logged `None` (other error). Direct callers never observe
/// `wasmtime::Error` themselves.
///
/// The callback receives both `&PluginWorld` (the bindings handle, used
/// for accessor methods like `yosh_plugin_plugin()`) and `&mut Store`
/// (passed into the typed call functions). We pass them as separate
/// arguments so the closure does not need to move out of `plugin.bindings`
/// — `PluginWorld` is not `Clone` in wasmtime 27's bindgen output.
fn with_env<R>(
    plugin: &mut LoadedPlugin,
    env: &mut ShellEnv,
    f: impl FnOnce(&PluginWorld, &mut Store<HostContext>) -> Result<R, wasmtime::Error>,
) -> Option<R> {
    if plugin.invalidated {
        eprintln!(
            "yosh: plugin '{}': skipped (instance invalidated by earlier trap)",
            plugin.name
        );
        return None;
    }

    // Split-borrow: `&plugin.bindings` and `&mut plugin.store` are
    // disjoint fields, so we can hold both simultaneously.
    let bindings = &plugin.bindings;
    let result = {
        let mut guard = EnvGuard::bind(&mut plugin.store, env);
        f(bindings, guard.store())
        // guard drops here, restoring env to null whether `f` returned
        // Ok, Err, or unwound via panic.
    };

    match result {
        Ok(r) => Some(r),
        Err(e) => {
            if let Some(trap) = e.downcast_ref::<wasmtime::Trap>() {
                eprintln!(
                    "yosh: plugin '{}': trapped: {} — disabling for the rest of this session",
                    plugin.name, trap
                );
                plugin.invalidated = true;
            } else {
                eprintln!("yosh: plugin '{}': call failed: {}", plugin.name, e);
            }
            None
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

/// Parse `plugin-info.required-capabilities` into a bitfield. Unknown
/// strings produce a single warning line each but do NOT block the plugin
/// (matches the §6 "unknown capabilities are warnings, not errors" rule).
fn parse_required_capabilities(plugin_info: &PluginInfo, plugin_name: &str) -> u32 {
    let mut bits: u32 = 0;
    for s in &plugin_info.required_capabilities {
        match parse_capability(s) {
            Some(cap) => bits |= cap.to_bitflag(),
            None => {
                eprintln!(
                    "yosh: plugin '{}': unknown capability string '{}' (ignored)",
                    plugin_name, s
                );
            }
        }
    }
    bits
}

/// Log requested-but-not-granted capabilities in the same shape as the
/// dlopen-era `log_denied_capabilities` — preserves user-visible behaviour.
fn log_denied_capabilities(plugin_name: &str, denied: u32) {
    let caps = [
        Capability::VariablesRead,
        Capability::VariablesWrite,
        Capability::Filesystem,
        Capability::Io,
        Capability::HookPreExec,
        Capability::HookPostExec,
        Capability::HookOnCd,
        Capability::HookPrePrompt,
        Capability::FilesRead,
        Capability::FilesWrite,
    ];
    for cap in caps {
        if denied & cap.to_bitflag() != 0 {
            eprintln!(
                "yosh: plugin '{}': capability '{}' requested but not granted",
                plugin_name,
                cap.as_str()
            );
        }
    }
}

// ── test helpers ───────────────────────────────────────────────────────
//
// Tests in Task 6 call into the manager from tests/plugin.rs. Expose
// what they need behind a feature gate so production code never sees the
// internals.
#[cfg(any(test, feature = "test-helpers"))]
pub mod test_helpers {
    use super::*;

    /// Load a single plugin with an explicit capability allowlist, for
    /// integration tests. Returns the granted bitfield on success.
    pub fn load_plugin_with_caps(
        manager: &mut PluginManager,
        path: &Path,
        env: &mut ShellEnv,
        caps: u32,
    ) -> Result<(), String> {
        manager.load_one(path, env, Some(caps), None, None)
    }

    /// Load a plugin with an explicit cwasm cache + key. The host
    /// validates the cache key tuple and falls back to in-memory compile
    /// on mismatch. Used by §8.6–§8.9 cwasm-invalidation tests.
    pub fn load_plugin_with_cache(
        manager: &mut PluginManager,
        path: &Path,
        env: &mut ShellEnv,
        caps: u32,
        cwasm_path: &Path,
        expected_key: &super::cache::CacheKey,
    ) -> Result<(), String> {
        manager.load_one(path, env, Some(caps), Some(cwasm_path), Some(expected_key))
    }

    /// Returns true if the most-recently-loaded plugin's `Store` has a
    /// null env pointer (i.e. no `with_env` is currently active). Used by
    /// the env-leak regression test.
    pub fn env_pointer_is_null_in_store(manager: &PluginManager) -> Option<bool> {
        let plugin = manager.plugins.last()?;
        Some(plugin.store.data().env.is_null())
    }
}
