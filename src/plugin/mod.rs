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
pub mod pattern;

use std::path::Path;

use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store};

use yosh_plugin_api::{
    CAP_ALL, CAP_HOOK_ON_CD, CAP_HOOK_POST_EXEC, CAP_HOOK_PRE_EXEC, CAP_HOOK_PRE_PROMPT,
    Capability, parse_capability,
};

use crate::env::ShellEnv;

use self::cache::{CacheKey, sha256_hex, sidecar_path, validate_cwasm};
use self::config::{PluginConfig, expand_tilde};
use self::host::HostContext;

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

use self::generated::yosh::plugin::types::{HookName, PluginInfo};
use self::generated::{PluginWorld, PluginWorldPre};

// ── Pre-prompt timeout constants ───────────────────────────────────────

/// Default pre-prompt hook timeout in milliseconds. A plugin's
/// `pre_prompt` hook is interrupted with `Trap::Interrupt` if it has not
/// returned within this budget, then permanently invalidated for the
/// session.
const DEFAULT_PRE_PROMPT_TIMEOUT_MS: u64 = 500;

/// Hard upper bound on the configurable pre-prompt timeout. Values above
/// this clamp back to the default with a stderr warning.
const MAX_PRE_PROMPT_TIMEOUT_MS: u64 = 60_000;

/// Tick interval for the `PluginManager` epoch-bumping thread. Worst-case
/// overshoot of any deadline is one tick window.
const TICK_MS: u64 = 50;

/// Environment variable that overrides `DEFAULT_PRE_PROMPT_TIMEOUT_MS`.
const PRE_PROMPT_TIMEOUT_ENV: &str = "YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS";

/// Effectively-never epoch deadline. Set as the persistent baseline on
/// every `LoadedPlugin.store` so non-`pre_prompt` hooks never trap on
/// the engine epoch. `call_pre_prompt` overrides this with a tight bound
/// per invocation and restores it after the call.
const STORE_BASELINE_DEADLINE_TICKS: u64 = u64::MAX / 2;

/// Epoch deadline applied to the one-shot scratch store used during
/// `metadata()` extraction at plugin load. With `epoch_interruption(true)`
/// every store needs an explicit deadline (default 0 traps immediately);
/// 100 ticks (~5 s at `TICK_MS = 50`) is a generous bound that never
/// trips for well-behaved plugins while still catching infinite loops.
const METADATA_SCRATCH_DEADLINE_TICKS: u64 = 100;

/// Pure parser for the `YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS` env var.
///
/// * `Ok(n)` — caller should use `n` directly. `None` → default,
///   `Some(valid integer in [1, 60000])` → that integer.
/// * `Err(raw)` — input was present but invalid (non-numeric, 0, or
///   > 60000). The raw input is returned so the caller can phrase a
///   warning that quotes what the user typed. The caller decides
///   whether to fall back to the default.
fn parse_pre_prompt_timeout(input: Option<&str>) -> Result<u64, String> {
    let Some(s) = input else {
        return Ok(DEFAULT_PRE_PROMPT_TIMEOUT_MS);
    };
    match s.parse::<u64>() {
        Ok(n) if (1..=MAX_PRE_PROMPT_TIMEOUT_MS).contains(&n) => Ok(n),
        _ => Err(s.to_string()),
    }
}

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
    /// Resolved pre-prompt timeout in milliseconds, captured once at
    /// construction from `YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS`.
    pre_prompt_timeout_ms: u64,
    /// Permissive (`CAP_ALL`) linker reused for the metadata probe step
    /// of every `load_one`. Built lazily on the first plugin load and
    /// then shared across subsequent loads — the metadata-contract
    /// (host imports return `Err(Denied)` on null env) makes the
    /// permissive linker safe to reuse regardless of the negotiated
    /// capability mask. Eliminates one full `Linker<HostContext>`
    /// rebuild per plugin after the first. See report §4.2 in
    /// `docs/superpowers/specs/2026-05-08-plugin-perf-report.md`.
    scratch_linker: Option<Linker<HostContext>>,
    /// Background epoch-tick thread. `Some` while the manager is alive;
    /// joined on `Drop`.
    tick_thread: Option<TickThread>,
}

/// Background thread that increments the wasmtime epoch counter at a
/// fixed interval (`TICK_MS`). One per `PluginManager`. The handle and
/// stop flag are wrapped so `Drop for PluginManager` can request a
/// clean shutdown.
struct TickThread {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl TickThread {
    fn spawn(engine: wasmtime::Engine) -> Self {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::thread;
        use std::time::Duration;

        let stop = Arc::new(AtomicBool::new(false));
        let stop_inner = stop.clone();
        let handle = thread::Builder::new()
            .name("yosh-plugin-epoch-tick".to_string())
            .spawn(move || {
                while !stop_inner.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(TICK_MS));
                    // Cheap and safe to call concurrently with guest
                    // execution; wasmtime is designed for this exact
                    // pattern.
                    engine.increment_epoch();
                }
            })
            .expect("spawn yosh-plugin-epoch-tick thread");
        TickThread {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for TickThread {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(h) = self.handle.take() {
            // The tick thread sleeps up to TICK_MS between flag checks,
            // so worst-case join wait is ~TICK_MS. We do not impose a
            // join timeout here because the loop is tight and bounded.
            let _ = h.join();
        }
    }
}

impl PluginManager {
    pub fn new() -> Self {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(false);
        config.consume_fuel(false);
        config.epoch_interruption(true);
        // Best-effort: enable system cache. If unavailable we just proceed
        // without it. cwasm precompile is the durable cache; this is the
        // lower-level wasmtime cranelift cache.
        let _ = config.cache_config_load_default();

        // Stable fingerprint: covers the flags relevant to cwasm
        // compatibility. Any change to this string invalidates every
        // cached cwasm via `engine_config_hash`. The canonical literal
        // lives in `yosh_plugin_manager::precompile::ENGINE_FINGERPRINT`
        // so both sides cannot drift.
        let engine_fingerprint = yosh_plugin_manager::precompile::ENGINE_FINGERPRINT.to_string();

        let engine = Engine::new(&config).expect("wasmtime Engine::new");

        // Resolve the pre-prompt timeout once. Invalid values warn and
        // fall back to the default. The pure parser stays unit-testable;
        // the warning lives here because it is I/O.
        let raw = std::env::var(PRE_PROMPT_TIMEOUT_ENV).ok();
        let pre_prompt_timeout_ms = match parse_pre_prompt_timeout(raw.as_deref()) {
            Ok(n) => n,
            Err(invalid) => {
                let display: &str = if invalid.is_empty() {
                    "<empty>"
                } else {
                    &invalid
                };
                eprintln!(
                    "yosh: plugin: {}={} invalid (must be 1..={} ms); using default {}ms",
                    PRE_PROMPT_TIMEOUT_ENV,
                    display,
                    MAX_PRE_PROMPT_TIMEOUT_MS,
                    DEFAULT_PRE_PROMPT_TIMEOUT_MS
                );
                DEFAULT_PRE_PROMPT_TIMEOUT_MS
            }
        };

        let tick_thread = Some(TickThread::spawn(engine.clone()));

        PluginManager {
            engine,
            engine_fingerprint,
            plugins: Vec::new(),
            pre_prompt_timeout_ms,
            scratch_linker: None,
            tick_thread,
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
                entry.allowed_commands.as_deref().unwrap_or_default(),
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
    #[allow(dead_code)] // public manager API; production loads go through load_from_config
    pub fn load_plugin(&mut self, path: &Path, env: &mut ShellEnv) -> Result<(), String> {
        self.load_one(path, env, None, None, None, &[])
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
        allowed_commands: &[String],
    ) -> Result<(), String> {
        // 1. Read the wasm bytes (needed for SHA verify and/or in-memory compile).
        let wasm_bytes = std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;

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
                        unsafe { Component::deserialize(&self.engine, &cwasm_bytes) }.map_err(
                            |e| format!("{}: cwasm deserialize failed: {}", cwasm.display(), e),
                        )?
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
            _ => {
                eprintln!(
                    "yosh: plugin '{}': no cwasm cache; \
                     precompiling in memory (one-time; run 'yosh-plugin sync' to cache)",
                    path.display(),
                );
                Component::new(&self.engine, &wasm_bytes)
                    .map_err(|e| format!("{}: component compile failed: {}", path.display(), e))?
            }
        };

        // 4. Parse the allowed_commands patterns up front so we can fail
        //     fast before wasting time on the scratch linker.
        let parsed_allowed_commands: Vec<self::pattern::CommandPattern> = allowed_commands
            .iter()
            .map(|s| {
                self::pattern::CommandPattern::parse(s).map_err(|e| {
                    format!(
                        "{}: invalid allowed_commands pattern '{}': {}",
                        path.display(),
                        s,
                        e
                    )
                })
            })
            .collect::<Result<_, _>>()?;

        // 5. Build a permissive linker first so we can call `metadata` to
        //    learn the plugin's requested capabilities. The metadata
        //    contract (host imports return `Err(Denied)` on null env) makes
        //    this safe — even a permissive linker rejects host calls during
        //    `metadata`. The scratch linker is cached on `self` because it
        //    is plugin-independent (always `CAP_ALL`); subsequent loads
        //    reuse it, eliminating one full `Linker` rebuild per plugin.
        if self.scratch_linker.is_none() {
            let l = linker::build_linker(&self.engine, CAP_ALL)
                .map_err(|e| format!("{}: linker init failed: {}", path.display(), e))?;
            self.scratch_linker = Some(l);
        }
        let scratch_linker = self
            .scratch_linker
            .as_ref()
            .expect("scratch_linker initialized just above");
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
        // With epoch_interruption(true) on the engine, every store needs a
        // deadline; the default is 0, which traps on the first instruction.
        // metadata() should be microseconds; the scratch deadline is a
        // generous bound that never trips for well-behaved plugins.
        scratch_store.set_epoch_deadline(METADATA_SCRATCH_DEADLINE_TICKS);
        let scratch_world = scratch_pre
            .instantiate(&mut scratch_store)
            .map_err(|e| format!("{}: instantiate failed: {}", path.display(), e))?;
        // env pointer is null in scratch_store — the deny short-circuit on
        // null env is what enforces the metadata contract.
        let plugin_info = scratch_world
            .yosh_plugin_plugin()
            .call_metadata(&mut scratch_store)
            .map_err(|e| format!("{}: metadata trap: {}", path.display(), e))?;

        // 6. Negotiate capabilities. Parse the strings from `plugin-info`,
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

        // 7. Build the real linker with the negotiated capability mask,
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

        let mut host_ctx =
            HostContext::new_for_plugin(plugin_info.name.clone(), effective_capabilities);
        host_ctx.allowed_commands = parsed_allowed_commands;
        let mut store = Store::new(&self.engine, host_ctx);
        // Default-effectively-never deadline. `call_pre_prompt` overrides
        // this with a tight bound on each invocation (Task 4); other
        // hooks and `exec_command` keep this baseline so they don't trap
        // unexpectedly.
        store.set_epoch_deadline(STORE_BASELINE_DEADLINE_TICKS);
        let bindings = real_pre
            .instantiate(&mut store)
            .map_err(|e| format!("{}: real instantiate: {}", path.display(), e))?;

        // call on_load under with_env (host imports available).
        let on_load_result = {
            let mut guard = EnvGuard::bind(&mut store, env);
            bindings.yosh_plugin_plugin().call_on_load(guard.store())
        };
        match on_load_result {
            Ok(Ok(())) => {}
            Ok(Err(msg)) => {
                return Err(format!(
                    "{}: on_load returned error: {}",
                    plugin_info.name, msg
                ));
            }
            Err(e) => {
                return Err(format!("{}: on_load trap: {}", plugin_info.name, e));
            }
        }

        // 8. Stash.
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
    pub fn exec_command(&mut self, env: &mut ShellEnv, name: &str, args: &[String]) -> PluginExec {
        let Some(idx) = self.plugins.iter().position(|p| p.provides_command(name)) else {
            return PluginExec::NotHandled;
        };
        let plugin = &mut self.plugins[idx];
        match with_env(plugin, env, |bindings, store| {
            bindings.yosh_plugin_plugin().call_exec(store, name, args)
        }) {
            Ok(exit) => PluginExec::Handled(exit),
            Err(e) => {
                log_with_env_failure(&plugin.name, &e);
                PluginExec::Failed
            }
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
            if let Err(e) = with_env(plugin, env, |bindings, store| {
                bindings.yosh_plugin_hooks().call_pre_exec(store, cmd)
            }) {
                log_with_env_failure(&plugin.name, &e);
            }
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
            if let Err(e) = with_env(plugin, env, |bindings, store| {
                bindings
                    .yosh_plugin_hooks()
                    .call_post_exec(store, cmd, exit_code)
            }) {
                log_with_env_failure(&plugin.name, &e);
            }
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
            if let Err(e) = with_env(plugin, env, |bindings, store| {
                bindings
                    .yosh_plugin_hooks()
                    .call_on_cd(store, old_dir, new_dir)
            }) {
                log_with_env_failure(&plugin.name, &e);
            }
        }
    }

    pub fn call_pre_prompt(&mut self, env: &mut ShellEnv) {
        let ticks = self.pre_prompt_timeout_ms.div_ceil(TICK_MS);
        let timeout_ms = self.pre_prompt_timeout_ms;

        for plugin in &mut self.plugins {
            if plugin.capabilities & CAP_HOOK_PRE_PROMPT == 0 {
                continue;
            }
            if !plugin.implements_hook(HookName::PrePrompt) {
                continue;
            }
            plugin.store.set_epoch_deadline(ticks);
            let result = with_env(plugin, env, |bindings, store| {
                bindings.yosh_plugin_hooks().call_pre_prompt(store)
            });
            // Restore the baseline so subsequent non-pre_prompt hooks
            // (pre_exec, post_exec, on_cd, exec_command) on the same
            // store retain their full budget. Skip on Trapped because
            // the plugin is now invalidated and its deadline is moot.
            if !matches!(&result, Err(WithEnvError::Trapped { .. })) {
                plugin
                    .store
                    .set_epoch_deadline(STORE_BASELINE_DEADLINE_TICKS);
            }
            if let Err(e) = result {
                match &e {
                    WithEnvError::Skipped => {}
                    WithEnvError::Trapped {
                        is_interrupt: true, ..
                    } => {
                        eprintln!(
                            "yosh: plugin '{}': pre_prompt exceeded {}ms timeout — disabling for the rest of this session",
                            plugin.name, timeout_ms
                        );
                    }
                    _ => log_with_env_failure(&plugin.name, &e),
                }
            }
        }
    }

    /// Call `on_unload` on every plugin and drop them. Best-effort: a trap
    /// in `on_unload` is logged and the plugin is dropped anyway.
    #[allow(dead_code)] // public manager API; called by host shutdown paths
    pub fn unload_all(&mut self, env: &mut ShellEnv) {
        // Drain so the borrow checker lets us call `with_env` on each.
        let mut plugins = std::mem::take(&mut self.plugins);
        for plugin in &mut plugins {
            if plugin.invalidated {
                continue;
            }
            if let Err(e) = with_env(plugin, env, |bindings, store| {
                bindings.yosh_plugin_plugin().call_on_unload(store)
            }) {
                log_with_env_failure(&plugin.name, &e);
            }
        }
        // plugins drops here, releasing every Store and underlying instance.
        drop(plugins);
    }

    /// Check if any plugin provides the given command.
    #[allow(dead_code)] // public manager API; used by completion / dispatch lookups
    pub fn has_command(&self, name: &str) -> bool {
        self.plugins.iter().any(|p| p.provides_command(name))
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

/// Failure mode of a `with_env` dispatch. The caller decides how to
/// phrase the user-visible message — pre_prompt timeouts in particular
/// want hook-specific wording.
enum WithEnvError {
    /// Plugin instance was already invalidated by an earlier failure.
    /// `with_env` has already printed a "skipped" line.
    Skipped,
    /// Guest trap. `is_interrupt` is true for `Trap::Interrupt` (epoch
    /// deadline exceeded). The plugin has been marked invalidated.
    Trapped {
        is_interrupt: bool,
        trap: wasmtime::Trap,
    },
    /// Non-trap host-side error (e.g. type mismatch). The plugin is NOT
    /// invalidated; the failure is one-off.
    Other(wasmtime::Error),
}

/// Canonical dispatch wrapper for any guest-bound call that needs host
/// API access. Sets up `EnvGuard`, runs the callback, and converts
/// `wasmtime::Error` into a `WithEnvError`. Direct callers never observe
/// `wasmtime::Error` themselves; they pattern-match on `WithEnvError` and
/// pick their own message phrasing.
fn with_env<R>(
    plugin: &mut LoadedPlugin,
    env: &mut ShellEnv,
    f: impl FnOnce(&PluginWorld, &mut Store<HostContext>) -> Result<R, wasmtime::Error>,
) -> Result<R, WithEnvError> {
    if plugin.invalidated {
        eprintln!(
            "yosh: plugin '{}': skipped (instance invalidated by earlier trap)",
            plugin.name
        );
        return Err(WithEnvError::Skipped);
    }

    let bindings = &plugin.bindings;
    let result = {
        let mut guard = EnvGuard::bind(&mut plugin.store, env);
        f(bindings, guard.store())
    };

    match result {
        Ok(r) => Ok(r),
        Err(e) => {
            if let Some(trap) = e.downcast_ref::<wasmtime::Trap>() {
                let trap = *trap;
                let is_interrupt = matches!(trap, wasmtime::Trap::Interrupt);
                plugin.invalidated = true;
                Err(WithEnvError::Trapped { is_interrupt, trap })
            } else {
                Err(WithEnvError::Other(e))
            }
        }
    }
}

/// Generic "with_env failure" logger for hooks that don't need
/// hook-specific phrasing. Reproduces the pre-refactor messages exactly.
fn log_with_env_failure(plugin_name: &str, err: &WithEnvError) {
    match err {
        WithEnvError::Skipped => {}
        WithEnvError::Trapped { trap, .. } => {
            eprintln!(
                "yosh: plugin '{}': trapped: {} — disabling for the rest of this session",
                plugin_name, trap
            );
        }
        WithEnvError::Other(e) => {
            eprintln!("yosh: plugin '{}': call failed: {}", plugin_name, e);
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
#[allow(dead_code)] // exercised from integration tests in tests/plugin.rs and benches/plugin_bench.rs
pub mod test_helpers {
    use super::*;

    /// Load a single plugin with an explicit capability allowlist, for
    /// integration tests. Returns the granted bitfield on success.
    pub fn load_plugin_with_caps(
        manager: &mut PluginManager,
        path: &Path,
        env: &mut ShellEnv,
        caps: u32,
        allowed_commands: &[String],
    ) -> Result<(), String> {
        manager.load_one(path, env, Some(caps), None, None, allowed_commands)
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
        allowed_commands: &[String],
    ) -> Result<(), String> {
        manager.load_one(
            path,
            env,
            Some(caps),
            Some(cwasm_path),
            Some(expected_key),
            allowed_commands,
        )
    }

    /// Returns true if the most-recently-loaded plugin's `Store` has a
    /// null env pointer (i.e. no `with_env` is currently active). Used by
    /// the env-leak regression test.
    pub fn env_pointer_is_null_in_store(manager: &PluginManager) -> Option<bool> {
        let plugin = manager.plugins.last()?;
        Some(plugin.store.data().env.is_null())
    }

    /// Number of `Linker<HostContext>` entries currently cached on the
    /// manager. Used by §4.2 fix#2 cache-reuse / cache-separation tests.
    pub fn linker_cache_len(_manager: &PluginManager) -> usize {
        0 // STUB — replaced in Task 2
    }

    /// Override the resolved pre-prompt timeout for this manager. Tests
    /// use this instead of mutating `YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS`
    /// in the process environment, which is `unsafe` in Rust 2024 and
    /// races across parallel tests.
    pub fn set_pre_prompt_timeout_for_tests(manager: &mut PluginManager, ms: u64) {
        debug_assert!(
            ms >= 1,
            "set_pre_prompt_timeout_for_tests(0) would trap on the first epoch tick — use a positive deadline"
        );
        manager.pre_prompt_timeout_ms = ms;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pre_prompt_timeout_unset_returns_ok_default() {
        assert_eq!(
            parse_pre_prompt_timeout(None),
            Ok(DEFAULT_PRE_PROMPT_TIMEOUT_MS)
        );
    }

    #[test]
    fn parse_pre_prompt_timeout_valid_in_range() {
        assert_eq!(parse_pre_prompt_timeout(Some("250")), Ok(250));
        assert_eq!(parse_pre_prompt_timeout(Some("1")), Ok(1));
        assert_eq!(parse_pre_prompt_timeout(Some("60000")), Ok(60_000));
    }

    #[test]
    fn parse_pre_prompt_timeout_zero_returns_invalid() {
        assert_eq!(parse_pre_prompt_timeout(Some("0")), Err("0".to_string()));
    }

    #[test]
    fn parse_pre_prompt_timeout_above_max_returns_invalid() {
        assert_eq!(
            parse_pre_prompt_timeout(Some("60001")),
            Err("60001".to_string())
        );
        assert_eq!(
            parse_pre_prompt_timeout(Some("999999")),
            Err("999999".to_string())
        );
    }

    #[test]
    fn parse_pre_prompt_timeout_non_numeric_returns_invalid() {
        assert_eq!(
            parse_pre_prompt_timeout(Some("abc")),
            Err("abc".to_string())
        );
        assert_eq!(parse_pre_prompt_timeout(Some("")), Err("".to_string()));
        assert_eq!(parse_pre_prompt_timeout(Some("-1")), Err("-1".to_string()));
    }

    #[test]
    fn tick_thread_stops_when_manager_drops() {
        use std::time::Instant;

        let manager = PluginManager::new();
        // Capture a clone of the stop flag to observe post-drop state.
        let stop_flag = manager
            .tick_thread
            .as_ref()
            .expect("tick thread must be running while manager is alive")
            .stop
            .clone();
        assert!(
            !stop_flag.load(std::sync::atomic::Ordering::Acquire),
            "stop flag must be false while manager is alive"
        );

        let t0 = Instant::now();
        drop(manager);
        let elapsed = t0.elapsed();

        assert!(
            stop_flag.load(std::sync::atomic::Ordering::Acquire),
            "stop flag must be true after PluginManager drops"
        );
        // Drop must join within a generous bound (one tick + slack). If
        // the tick thread sleeps for the full TICK_MS without checking
        // the flag, worst case is ~TICK_MS. 500ms is generous.
        assert!(
            elapsed.as_millis() < 500,
            "Drop took {:?} (>500ms); tick thread is not exiting promptly",
            elapsed
        );
    }
}
