//! Test helpers.
//!
//! Tests in Task 6 call into the manager from tests/plugin.rs. Expose
//! what they need behind a feature gate so production code never sees the
//! internals.

use std::path::Path;

use crate::env::ShellEnv;

use super::{PluginManager, limits};

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
pub fn set_settings_path_for_tests(manager: &mut PluginManager, path: Option<std::path::PathBuf>) {
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
