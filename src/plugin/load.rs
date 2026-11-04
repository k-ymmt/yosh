//! Plugin loading: lockfile walk, wasm verification, component
//! compilation (cwasm cache or in-memory), metadata probe, capability
//! negotiation, and real instantiation + `on_load`.

use std::path::Path;

use wasmtime::Store;
use wasmtime::component::{Component, Linker};

use yosh_plugin_api::CAP_ALL;

use crate::env::ShellEnv;

use super::cache::{CacheKey, sha256_hex, sidecar_path, validate_cwasm};
use super::capabilities::negotiate_capabilities;
use super::config::{self, expand_tilde, settings_path_for};
use super::deadline::{METADATA_SCRATCH_DEADLINE_TICKS, STORE_BASELINE_DEADLINE_TICKS};
use super::generated::PluginWorldPre;
use super::generated::yosh::plugin::types::PluginInfo;
use super::host::HostContext;
use super::invoke::EnvGuard;
use super::pattern::CommandPattern;
use super::{LoadedPlugin, PluginManager, limits, linker};

impl PluginManager {
    /// Load plugins listed in the config file. Errors are printed to stderr
    /// and the failing plugin is skipped. A missing config file is the
    /// normal no-plugins case and stays silent; a corrupted/unreadable
    /// one is reported — otherwise every plugin would vanish with zero
    /// diagnostics.
    pub fn load_from_config(&mut self, config_path: &Path, env: &mut ShellEnv, interactive: bool) {
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
            // Non-interactive shells never dispatch hooks, so a plugin
            // that provides no custom commands has nothing a script can
            // reach: skip its instantiation entirely. A lockfile written
            // before `commands` was recorded (`None`) is treated as
            // "may provide commands" and still loads.
            if !interactive && entry.commands.as_ref().is_some_and(|c| c.is_empty()) {
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
            let l = linker::build_linker(self.engine(), caps)
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
    pub(crate) fn load_one(
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
        // 0. First plugin load: bring up the wasmtime Engine and the
        //    epoch tick thread. Everything below (component compile,
        //    linkers, stores) needs the engine.
        self.ensure_engine();

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
                    actual,
                ));
            }
        }

        // 3. Build the component. Try the cwasm cache first when the
        //    lockfile points at one; fall back to in-memory compile on any
        //    trust-condition failure.
        let component = self.compile_component(path, &wasm_bytes, cwasm_path, expected_key)?;

        // 4. Parse the allowed_commands patterns up front so we can fail
        //     fast before wasting time on the scratch linker.
        let parsed_allowed_commands = parse_allowed_commands(path, allowed_commands)?;

        // 5. Probe the plugin's metadata to learn its requested
        //    capabilities.
        let plugin_info = self.probe_metadata(path, &component)?;

        // 6. Negotiate capabilities. Parse the strings from `plugin-info`,
        //    intersect with the config allowlist, log denied bits.
        let effective_capabilities = negotiate_capabilities(&plugin_info, config_capabilities);

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
        let mut store = Store::new(self.engine(), host_ctx);
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

    /// Build the component for `path`: deserialize the trusted cwasm
    /// cache when the lockfile points at one and all 5 trust conditions
    /// hold, otherwise precompile `wasm_bytes` in memory with a warning.
    fn compile_component(
        &self,
        path: &Path,
        wasm_bytes: &[u8],
        cwasm_path: Option<&Path>,
        expected_key: Option<&CacheKey>,
    ) -> Result<Component, String> {
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
                        unsafe { Component::deserialize(self.engine(), &cwasm_bytes) }.map_err(
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
                        Component::new(self.engine(), wasm_bytes).map_err(|e| {
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
                Component::new(self.engine(), wasm_bytes)
                    .map_err(|e| format!("{}: component compile failed: {}", path.display(), e))?
            }
        };
        Ok(component)
    }

    /// Instantiate `component` against a permissive linker in a scratch
    /// store and call `metadata` to learn the plugin's requested
    /// capabilities. The metadata contract (host imports return
    /// `Err(Denied)` on null env) makes this safe — even a permissive
    /// linker rejects host calls during `metadata`. The linker is fetched
    /// from `linker_cache` (per-cap-mask cache); the `CAP_ALL` entry is
    /// shared across all metadata probes, and any plugin granted
    /// `CAP_ALL` shares this same cached linker for its
    /// real-instantiation step.
    fn probe_metadata(&mut self, path: &Path, component: &Component) -> Result<PluginInfo, String> {
        let scratch_linker = self.get_or_build_linker(CAP_ALL, path)?;
        let scratch_pre = PluginWorldPre::new(
            scratch_linker
                .instantiate_pre(component)
                .map_err(|e| format!("{}: instantiate_pre failed: {}", path.display(), e))?,
        )
        .map_err(|e| format!("{}: bindings pre-init failed: {}", path.display(), e))?;

        let mut scratch_store = Store::new(
            self.engine(),
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
        scratch_world
            .yosh_plugin_plugin()
            .call_metadata(&mut scratch_store)
            .map_err(|e| format!("{}: metadata trap: {}", path.display(), e))
    }
}

/// Parse the `allowed_commands` patterns, naming the plugin path and the
/// offending pattern on failure.
fn parse_allowed_commands(
    path: &Path,
    allowed_commands: &[String],
) -> Result<Vec<CommandPattern>, String> {
    allowed_commands
        .iter()
        .map(|s| {
            CommandPattern::parse(s).map_err(|e| {
                format!(
                    "{}: invalid allowed_commands pattern '{}': {}",
                    path.display(),
                    s,
                    e
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_interactive_load_skips_plugins_without_commands() {
        // A lockfile entry that records `commands = []` has nothing a
        // script can reach (hooks are interactive-only), so a
        // non-interactive load must not even build the engine. The
        // interactive load of the same entry attempts the load (and
        // fails on the missing wasm, which is fine for this test).
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("plugins.lock");
        std::fs::write(
            &lock,
            r#"[[plugin]]
name = "noop"
path = "/nonexistent/noop.wasm"
sha256 = "00"
commands = []
"#,
        )
        .unwrap();
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut mgr = PluginManager::new();
        mgr.load_from_config(&lock, &mut env, false);
        assert!(
            mgr.engine.is_none(),
            "non-interactive must skip a command-less plugin"
        );
        assert!(mgr.plugins.is_empty());

        // Without the `commands` field (older lockfile) the entry is
        // treated as "may provide commands" and the load is attempted.
        std::fs::write(
            &lock,
            r#"[[plugin]]
name = "noop"
path = "/nonexistent/noop.wasm"
sha256 = "00"
"#,
        )
        .unwrap();
        let mut mgr = PluginManager::new();
        mgr.load_from_config(&lock, &mut env, false);
        assert!(
            mgr.engine.is_some(),
            "unknown command list must still attempt the load"
        );
    }
}
