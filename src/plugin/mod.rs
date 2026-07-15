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
pub mod limits;
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
use self::config::{expand_tilde, settings_path_for};
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
///   greater than 60000). The raw input is returned so the caller can
///   phrase a warning that quotes what the user typed. The caller
///   decides whether to fall back to the default.
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
    engine: Engine,
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
            linker_cache: std::collections::HashMap::new(),
            tick_thread,
        }
    }

    /// Load plugins listed in the config file. Errors are printed to stderr
    /// and the failing plugin is skipped. A missing config file is the
    /// normal no-plugins case and stays silent; a corrupted/unreadable
    /// one is reported — otherwise every plugin would vanish with zero
    /// diagnostics.
    pub fn load_from_config(&mut self, config_path: &Path, env: &mut ShellEnv) {
        let config = match config::read_config_for_load(config_path) {
            Ok(Some(c)) => c,
            Ok(None) => return,
            Err(e) => {
                eprintln!("yosh: plugin: {}", e);
                return;
            }
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
            let cache_key = entry.cache_key();
            let files_root = entry.files_root.as_deref().map(expand_tilde);
            let cwasm_path = entry
                .cwasm_path
                .as_deref()
                .map(|p| expand_tilde(&p.to_string_lossy()));
            if let Err(e) = self.load_one(
                &path,
                env,
                config_caps,
                Some(&entry.sha256),
                cwasm_path.as_deref(),
                cache_key.as_ref(),
                entry.allowed_commands.as_deref().unwrap_or_default(),
                files_root.as_deref(),
                entry.limits_config(),
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
        self.load_one(
            path,
            env,
            None,
            None,
            None,
            None,
            &[],
            None,
            limits::LimitsConfig::default(),
        )
    }

    /// Look up a cached `Linker<HostContext>` for the given capability
    /// bitfield, or build and cache one. Returns a borrowed reference
    /// suitable for `Linker::instantiate_pre(&self, &Component)`. On
    /// `build_linker` failure the cache is not modified, so a
    /// subsequent load for the same caps retries from scratch.
    fn get_or_build_linker(
        &mut self,
        caps: u32,
        path: &Path,
    ) -> Result<&Linker<HostContext>, String> {
        if !self.linker_cache.contains_key(&caps) {
            let l = linker::build_linker(&self.engine, caps)
                .map_err(|e| format!("{}: linker build failed: {}", path.display(), e))?;
            self.linker_cache.insert(caps, l);
        }
        Ok(self
            .linker_cache
            .get(&caps)
            .expect("inserted on the line above if missing"))
    }

    /// Load one plugin.
    ///   * `config_capabilities`: `None` → grant every requested capability;
    ///     `Some(bits)` → intersect requested with `bits`.
    ///   * `expected_sha256`: lockfile-pinned SHA-256 of the wasm bytes.
    ///     Always `Some` for lockfile loads (the field is required in
    ///     `plugins.lock`); a mismatch refuses the load. `None` only for
    ///     the manual `load_plugin` API, which has no lockfile pin.
    ///   * `cwasm_path` + `expected_key`: when both are present, attempt to
    ///     `Component::deserialize` from the trusted cache instead of
    ///     re-compiling the wasm bytes. Any of the 5 trust conditions
    ///     failing falls back to in-memory compile with a warning.
    ///   * `files_root`: when present, confines every `files:read` /
    ///     `files:write` host call to paths inside this directory
    ///     (canonicalized at load time). `None` leaves the capability
    ///     as a full-filesystem grant.
    ///   * `limits_cfg`: raw optional resource-limit fields from the
    ///     `plugins.lock` entry. Resolved into `PluginLimits` once
    ///     capability negotiation has produced `plugin_info.name`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn load_one(
        &mut self,
        path: &Path,
        env: &mut ShellEnv,
        config_capabilities: Option<u32>,
        expected_sha256: Option<&str>,
        cwasm_path: Option<&Path>,
        expected_key: Option<&CacheKey>,
        allowed_commands: &[String],
        files_root: Option<&Path>,
        limits_cfg: limits::LimitsConfig,
    ) -> Result<(), String> {
        // 1. Read the wasm bytes (needed for SHA verify and/or in-memory compile).
        let wasm_bytes = std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;

        // 2. Verify the on-disk wasm against the lockfile-pinned SHA
        //    BEFORE trusting any cwasm. Per spec §5 step 1: this check is
        //    unconditional for lockfile loads — it does NOT depend on the
        //    cwasm cache tuple being present. A mismatch refuses the load
        //    (does NOT silently fall back to a cached cwasm).
        if let Some(expected) = expected_sha256 {
            let actual = sha256_hex(&wasm_bytes);
            if actual != expected {
                return Err(format!(
                    "{}: wasm SHA-256 mismatch (lockfile {}, actual {}); \
                     refusing to load. Run 'yosh-plugin sync' to refresh.",
                    path.display(),
                    expected,
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
        //    `metadata`. The linker is fetched from `linker_cache`
        //    (per-cap-mask cache); the `CAP_ALL` entry is shared across
        //    all metadata probes, and any plugin granted `CAP_ALL` shares
        //    this same cached linker for its real-instantiation step.
        let scratch_linker = self.get_or_build_linker(CAP_ALL, path)?;
        let scratch_pre = PluginWorldPre::new(
            scratch_linker
                .instantiate_pre(&component)
                .map_err(|e| format!("{}: instantiate_pre failed: {}", path.display(), e))?,
        )
        .map_err(|e| format!("{}: bindings pre-init failed: {}", path.display(), e))?;

        let mut scratch_store = Store::new(
            &self.engine,
            HostContext::new_for_plugin(
                "<probing>",
                CAP_ALL,
                (limits::DEFAULT_MAX_MEMORY_MB * limits::MIB) as usize,
            ),
        );
        scratch_store.limiter(|ctx| &mut ctx.mem_limiter);
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

        // Resolve runtime resource limits now that `plugin_info.name` is
        // known. Pure resolution + warnings happen once, here, at load
        // time; Tasks 3/5 read the resulting `PluginLimits` off
        // `LoadedPlugin` without re-resolving.
        let (plugin_limits, limit_warnings) =
            limits::resolve_limits(&limits_cfg, self.pre_prompt_timeout_ms, &plugin_info.name);
        for w in &limit_warnings {
            eprintln!("yosh: {}", w);
        }

        // 7. Fetch the real linker from `linker_cache` (built lazily on
        //    first use of this cap mask), create a fresh store,
        //    instantiate, and call on_load under with_env so the plugin
        //    can use its granted host imports. Plugins sharing a cap
        //    mask reuse the same cached linker.
        let real_linker = self.get_or_build_linker(effective_capabilities, path)?;
        let real_pre = PluginWorldPre::new(
            real_linker
                .instantiate_pre(&component)
                .map_err(|e| format!("{}: real instantiate_pre: {}", path.display(), e))?,
        )
        .map_err(|e| format!("{}: real bindings pre-init: {}", path.display(), e))?;

        let mut host_ctx = HostContext::new_for_plugin(
            plugin_info.name.clone(),
            effective_capabilities,
            plugin_limits.max_memory_bytes,
        );
        host_ctx.allowed_commands = parsed_allowed_commands;
        // Canonicalize once at load time so the per-call starts_with
        // confinement check compares canonical paths. A root that does
        // not exist yet is kept verbatim (every canonicalized candidate
        // will then fail starts_with — deny-by-default).
        host_ctx.files_root =
            files_root.map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()));
        host_ctx.settings_path = settings_path_for(&plugin_info.name);
        let mut store = Store::new(&self.engine, host_ctx);
        store.limiter(|ctx| &mut ctx.mem_limiter);
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
            limits: plugin_limits,
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
        let ticks = plugin.limits.command_deadline_ticks();
        let timeout_ms = plugin.limits.command_timeout_ms;
        let max_mb = plugin.limits.max_memory_mb();
        match with_deadline(plugin, env, ticks, |bindings, store| {
            bindings.yosh_plugin_plugin().call_exec(store, name, args)
        }) {
            Ok(exit) => PluginExec::Handled(exit),
            Err(e) => {
                log_entry_failure(
                    &plugin.name,
                    &format!("command '{}'", name),
                    timeout_ms,
                    max_mb,
                    &e,
                );
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
            let ticks = plugin.limits.hook_deadline_ticks();
            let timeout_ms = plugin.limits.hook_timeout_ms;
            let max_mb = plugin.limits.max_memory_mb();
            if let Err(e) = with_deadline(plugin, env, ticks, |bindings, store| {
                bindings.yosh_plugin_hooks().call_pre_exec(store, cmd)
            }) {
                log_entry_failure(&plugin.name, "pre_exec", timeout_ms, max_mb, &e);
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
            let ticks = plugin.limits.hook_deadline_ticks();
            let timeout_ms = plugin.limits.hook_timeout_ms;
            let max_mb = plugin.limits.max_memory_mb();
            if let Err(e) = with_deadline(plugin, env, ticks, |bindings, store| {
                bindings
                    .yosh_plugin_hooks()
                    .call_post_exec(store, cmd, exit_code)
            }) {
                log_entry_failure(&plugin.name, "post_exec", timeout_ms, max_mb, &e);
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
            let ticks = plugin.limits.hook_deadline_ticks();
            let timeout_ms = plugin.limits.hook_timeout_ms;
            let max_mb = plugin.limits.max_memory_mb();
            if let Err(e) = with_deadline(plugin, env, ticks, |bindings, store| {
                bindings
                    .yosh_plugin_hooks()
                    .call_on_cd(store, old_dir, new_dir)
            }) {
                log_entry_failure(&plugin.name, "on_cd", timeout_ms, max_mb, &e);
            }
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
            let ticks = Some(plugin.limits.pre_prompt_ticks());
            let timeout_ms = plugin.limits.pre_prompt_timeout_ms;
            let max_mb = plugin.limits.max_memory_mb();
            if let Err(e) = with_deadline(plugin, env, ticks, |bindings, store| {
                bindings.yosh_plugin_hooks().call_pre_prompt(store)
            }) {
                log_entry_failure(&plugin.name, "pre_prompt", timeout_ms, max_mb, &e);
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
                log_with_env_failure(&plugin.name, &e, plugin.limits.max_memory_mb());
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

    /// True if any loaded plugin both holds the pre-exec or post-exec
    /// capability and implements the corresponding hook. Callers use this
    /// to skip building the joined command string passed to
    /// `call_pre_exec` / `call_post_exec` when no plugin would ever
    /// receive it (the common case: no plugins loaded, or none register
    /// these hooks).
    pub fn has_exec_hooks(&self) -> bool {
        self.plugins.iter().any(|p| {
            (p.capabilities & CAP_HOOK_PRE_EXEC != 0 && p.implements_hook(HookName::PreExec))
                || (p.capabilities & CAP_HOOK_POST_EXEC != 0
                    && p.implements_hook(HookName::PostExec))
        })
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
        /// The store's memory limiter denied a grow during this call —
        /// the trap is (almost certainly) the guest allocator aborting
        /// on the failed allocation.
        memory_denied: bool,
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

    // Clear any stale `denied` flag left over from a prior grow that the
    // guest survived without trapping (e.g. `try_reserve`, or a
    // non-Rust guest that handles allocation failure gracefully), or
    // from a denial during `on_load` (which runs via a raw `EnvGuard`,
    // not `with_env`, so this is the first chance to clear it). Without
    // this, a later unrelated trap would read a stale `true` here and
    // mis-attribute itself as a memory-limit trap.
    plugin.store.data_mut().mem_limiter.denied = false;

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
                let memory_denied =
                    std::mem::replace(&mut plugin.store.data_mut().mem_limiter.denied, false);
                plugin.invalidated = true;
                Err(WithEnvError::Trapped {
                    is_interrupt,
                    memory_denied,
                    trap,
                })
            } else {
                Err(WithEnvError::Other(e))
            }
        }
    }
}

/// Run a guest call under an optional epoch deadline. `None` leaves the
/// store at the baseline (effectively unlimited). The baseline is
/// restored after the call unless the plugin trapped (it is then
/// invalidated and its deadline is moot).
fn with_deadline<R>(
    plugin: &mut LoadedPlugin,
    env: &mut ShellEnv,
    deadline_ticks: Option<u64>,
    f: impl FnOnce(&PluginWorld, &mut Store<HostContext>) -> Result<R, wasmtime::Error>,
) -> Result<R, WithEnvError> {
    if let Some(ticks) = deadline_ticks {
        plugin.store.set_epoch_deadline(ticks);
    }
    let result = with_env(plugin, env, f);
    if deadline_ticks.is_some() && !matches!(&result, Err(WithEnvError::Trapped { .. })) {
        plugin
            .store
            .set_epoch_deadline(STORE_BASELINE_DEADLINE_TICKS);
    }
    result
}

/// Failure logger for deadline-bounded entry points: names the entry
/// point and its budget on an epoch interrupt; defers to
/// `log_with_env_failure` for everything else.
fn log_entry_failure(
    plugin_name: &str,
    entry: &str,
    timeout_ms: u64,
    max_memory_mb: u64,
    err: &WithEnvError,
) {
    match err {
        WithEnvError::Trapped {
            is_interrupt: true, ..
        } => {
            eprintln!(
                "yosh: plugin '{}': {} exceeded {}ms timeout — disabling for the rest of this session",
                plugin_name, entry, timeout_ms
            );
        }
        other => log_with_env_failure(plugin_name, other, max_memory_mb),
    }
}

/// Generic "with_env failure" logger for hooks that don't need
/// hook-specific phrasing. Reproduces the pre-refactor messages exactly.
fn log_with_env_failure(plugin_name: &str, err: &WithEnvError, max_memory_mb: u64) {
    match err {
        WithEnvError::Skipped => {}
        // Must precede the generic `Trapped` arm below: both patterns
        // match `Trapped { .. }`, and Rust picks the first arm whose
        // pattern matches, so this more-specific `memory_denied: true`
        // arm has to come first or it would be unreachable.
        WithEnvError::Trapped {
            memory_denied: true,
            trap,
            ..
        } => {
            eprintln!(
                "yosh: plugin '{}': trapped: {} (memory limit {} MiB exceeded) — disabling for the rest of this session",
                plugin_name, trap, max_memory_mb
            );
        }
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

/// Names of the capabilities present in the `denied` bitfield. Must
/// enumerate every `Capability` variant so no denial goes unreported.
fn denied_capability_names(denied: u32) -> Vec<&'static str> {
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
        Capability::CommandsExec,
    ];
    caps.into_iter()
        .filter(|cap| denied & cap.to_bitflag() != 0)
        .map(|cap| cap.as_str())
        .collect()
}

/// Log requested-but-not-granted capabilities in the same shape as the
/// dlopen-era `log_denied_capabilities` — preserves user-visible behaviour.
fn log_denied_capabilities(plugin_name: &str, denied: u32) {
    for name in denied_capability_names(denied) {
        eprintln!(
            "yosh: plugin '{}': capability '{}' requested but not granted",
            plugin_name, name
        );
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
        manager.load_one(
            path,
            env,
            Some(caps),
            None,
            None,
            None,
            allowed_commands,
            None,
            limits::LimitsConfig::default(),
        )
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
            Some(&expected_key.wasm_sha256),
            Some(cwasm_path),
            Some(expected_key),
            allowed_commands,
            None,
            limits::LimitsConfig::default(),
        )
    }

    /// Load a plugin with explicit runtime limits, for the timeout /
    /// memory-cap integration tests.
    pub fn load_plugin_with_limits(
        manager: &mut PluginManager,
        path: &Path,
        env: &mut ShellEnv,
        caps: u32,
        limits_cfg: limits::LimitsConfig,
    ) -> Result<(), String> {
        manager.load_one(
            path,
            env,
            Some(caps),
            None,
            None,
            None,
            &[],
            None,
            limits_cfg,
        )
    }

    /// Returns true if the most-recently-loaded plugin's `Store` has a
    /// null env pointer (i.e. no `with_env` is currently active). Used by
    /// the env-leak regression test.
    pub fn env_pointer_is_null_in_store(manager: &PluginManager) -> Option<bool> {
        let plugin = manager.plugins.last()?;
        Some(plugin.store.data().env.is_null())
    }

    /// Override the most-recently-loaded plugin's resolved settings
    /// path, so integration tests can point `settings.read` at a
    /// tempdir file without mutating the process HOME.
    pub fn set_settings_path_for_tests(
        manager: &mut PluginManager,
        path: Option<std::path::PathBuf>,
    ) {
        if let Some(plugin) = manager.plugins.last_mut() {
            plugin.store.data_mut().settings_path = path;
        }
    }

    /// Force the most-recently-loaded plugin's `mem_limiter.denied` flag,
    /// for the stale-flag regression test.
    pub fn set_mem_denied_for_tests(manager: &mut PluginManager, denied: bool) {
        if let Some(plugin) = manager.plugins.last_mut() {
            plugin.store.data_mut().mem_limiter.denied = denied;
        }
    }

    /// Returns the most-recently-loaded plugin's `mem_limiter.denied`
    /// flag. Used by the stale-flag regression test to confirm
    /// `with_env` clears it at dispatch entry.
    pub fn mem_denied_for_tests(manager: &PluginManager) -> Option<bool> {
        let plugin = manager.plugins.last()?;
        Some(plugin.store.data().mem_limiter.denied)
    }

    /// Number of `Linker<HostContext>` entries currently cached on the
    /// manager. Used by §4.2 fix#2 cache-reuse / cache-separation tests.
    pub fn linker_cache_len(manager: &PluginManager) -> usize {
        manager.linker_cache.len()
    }

    /// Override the resolved pre-prompt timeout for this manager. Tests
    /// use this instead of mutating `YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS`
    /// in the process environment, which is `unsafe` in Rust 2024 and
    /// races across parallel tests.
    ///
    /// Must be called BEFORE loading any plugins: per-plugin limits
    /// (including the pre_prompt timeout fallback) are resolved once at
    /// load time via `resolve_limits`, so changing this afterward has no
    /// effect on already-loaded plugins.
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
    fn has_exec_hooks_false_with_no_plugins_loaded() {
        // The common case this gate targets: no plugins loaded at all, so
        // callers should skip building the joined command string passed
        // to call_pre_exec / call_post_exec.
        let mgr = PluginManager::new();
        assert!(!mgr.has_exec_hooks());
    }

    /// Regression: `commands:exec` was missing from the denial
    /// enumeration, so a denied exec capability was the only one that
    /// produced no "requested but not granted" warning. Every
    /// capability bit must map to a name here.
    #[test]
    fn denied_capability_names_covers_every_capability() {
        use yosh_plugin_api::*;
        let all = [
            (CAP_VARIABLES_READ, "variables:read"),
            (CAP_VARIABLES_WRITE, "variables:write"),
            (CAP_FILESYSTEM, "filesystem"),
            (CAP_IO, "io"),
            (CAP_HOOK_PRE_EXEC, "hooks:pre_exec"),
            (CAP_HOOK_POST_EXEC, "hooks:post_exec"),
            (CAP_HOOK_ON_CD, "hooks:on_cd"),
            (CAP_HOOK_PRE_PROMPT, "hooks:pre_prompt"),
            (CAP_FILES_READ, "files:read"),
            (CAP_FILES_WRITE, "files:write"),
            (CAP_COMMANDS_EXEC, "commands:exec"),
        ];
        for (bit, name) in all {
            assert_eq!(
                denied_capability_names(bit),
                vec![name],
                "capability {:#x} must be reported as '{}'",
                bit,
                name
            );
        }
    }

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
