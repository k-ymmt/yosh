//! Eager precompile of `.wasm` Component Model plugins to `.cwasm` artifacts
//! plus a sidecar `<basename>.cwasm.meta` describing the four-tuple cache
//! key the host validates at startup.
//!
//! See `docs/superpowers/specs/2026-04-27-wasm-plugin-runtime-design.md` §5
//! "cwasm trust model" / §7 "Plugin manager pipeline" for the full design.
//!
//! The four-tuple recorded both in `plugins.lock` and the sidecar is:
//!
//! ```text
//! (wasm_sha256, wasmtime_version, target_triple, engine_config_hash)
//! ```
//!
//! At shell startup the host computes the same tuple from its live engine
//! and rejects the cwasm if any field differs (`src/plugin/cache.rs` →
//! `validate_cwasm`). When that happens the host falls back to in-memory
//! `Component::new`, so a stale cwasm is never a hard failure — just a
//! perf regression until `yosh-plugin sync` re-runs.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Wasmtime version used for precompile. Hardcoded to match the pin in
/// `Cargo.toml`. MUST equal `src/plugin/cache.rs::WASMTIME_VERSION` so the
/// host's cache validator accepts cwasm files written here. Bumping the
/// wasmtime dep requires bumping this constant in lockstep.
///
/// `wasmtime::VERSION` is private in the 27.x crate, so we cannot derive
/// this at compile time without a build script that runs cargo metadata.
pub const WASMTIME_VERSION: &str = "27";

/// Target triple this binary was built for. Sourced from cargo's `TARGET`
/// env var captured by `build.rs` and re-emitted as a `rustc-env` entry.
/// Falls back to `"host"` if the build script could not determine it
/// (mirrors the host's same-name fallback).
pub fn target_triple() -> &'static str {
    option_env!("TARGET").unwrap_or("host")
}

/// Canonical engine-config fingerprint. MUST match what the shell's
/// `PluginManager::new()` computes (`src/plugin/mod.rs`) for the manager
/// to write a cwasm the host will accept. Both sides use this string
/// verbatim and feed it through `engine_config_hash`.
pub const ENGINE_FINGERPRINT: &str = "v1;component_model=true;async=false;fuel=false;cranelift";

/// Hex-encoded SHA-256 of the engine fingerprint string. Same algorithm
/// the host uses; same input string => same hash. Reusing the canonical
/// digest here keeps both producers in lockstep without sharing code.
pub fn engine_config_hash(fingerprint: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(fingerprint.as_bytes());
    hex::encode(hasher.finalize())
}

/// SHA-256 of arbitrary bytes, hex-encoded.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Four-tuple cache key. Same shape as `src/plugin/cache.rs::CacheKey`;
/// duplicated here because the manager and the host live in different
/// crates and we want neither to depend on the other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheKey {
    /// SHA-256 (hex) of the source `.wasm` file.
    pub wasm_sha256: String,
    /// Wasmtime version string. See `WASMTIME_VERSION`.
    pub wasmtime_version: String,
    /// Target triple, e.g. `aarch64-apple-darwin`.
    pub target_triple: String,
    /// Hex-encoded `engine_config_hash`.
    pub engine_config_hash: String,
}

impl CacheKey {
    /// Construct a cache key for the manager's precompile path.
    pub fn for_precompile(wasm_sha256: impl Into<String>) -> Self {
        CacheKey {
            wasm_sha256: wasm_sha256.into(),
            wasmtime_version: WASMTIME_VERSION.to_string(),
            target_triple: target_triple().to_string(),
            engine_config_hash: engine_config_hash(ENGINE_FINGERPRINT),
        }
    }
}

/// Sidecar layout. Identical to `src/plugin/cache.rs::SidecarMeta` so the
/// host's `read_from` parses files written here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarMeta {
    pub schema: u32,
    pub key: CacheKey,
}

impl SidecarMeta {
    pub const SCHEMA_VERSION: u32 = 1;

    pub fn new(key: CacheKey) -> Self {
        SidecarMeta {
            schema: Self::SCHEMA_VERSION,
            key,
        }
    }

    pub fn write_to(&self, path: &Path) -> Result<(), String> {
        let s = toml::to_string(self)
            .map_err(|e| format!("serialize cwasm sidecar {}: {}", path.display(), e))?;
        std::fs::write(path, s)
            .map_err(|e| format!("write cwasm sidecar {}: {}", path.display(), e))
    }
}

/// Build a wasmtime `Engine` configured the same way the shell does at
/// startup. Used by callers of `precompile()` so the produced cwasm
/// matches the host's expectations exactly.
///
/// `epoch_interruption` here is OFF — the metadata extraction sub-step in
/// `sync.rs` builds its own engine with epoch_interruption ON for the
/// 5-second watchdog. Mixing them would change the engine config hash and
/// invalidate every precompile, so we keep them as two separate engines.
pub fn make_engine() -> Result<wasmtime::Engine, String> {
    let mut config = wasmtime::Config::new();
    config.wasm_component_model(true);
    config.async_support(false);
    config.consume_fuel(false);
    wasmtime::Engine::new(&config).map_err(|e| format!("wasmtime Engine::new: {}", e))
}

/// Build a wasmtime `Engine` for the metadata-extraction sub-step. Same
/// flags as `make_engine` PLUS `epoch_interruption(true)` so an
/// out-of-band thread can interrupt a hung `metadata()` call by bumping
/// the engine epoch.
///
/// NOTE: epoch_interruption changes the engine fingerprint conceptually
/// but the cwasm produced by the precompile engine is what gets cached —
/// metadata extraction works off a fresh in-memory `Component::new`, not
/// the cwasm. So the two engines never need to share artefacts.
pub fn make_metadata_engine() -> Result<wasmtime::Engine, String> {
    let mut config = wasmtime::Config::new();
    config.wasm_component_model(true);
    config.async_support(false);
    config.consume_fuel(false);
    config.epoch_interruption(true);
    wasmtime::Engine::new(&config).map_err(|e| format!("wasmtime metadata Engine::new: {}", e))
}

/// Result of a successful precompile.
#[derive(Debug, Clone)]
pub struct PrecompileOutput {
    /// Absolute path to the written `.cwasm` file.
    pub cwasm_path: PathBuf,
    /// Absolute path to the written `<basename>.cwasm.meta` sidecar.
    pub sidecar_path: PathBuf,
    /// Cache key tuple. Caller persists this in `plugins.lock`.
    pub cache_key: CacheKey,
}

/// Precompile a `.wasm` component to `<cache_dir>/<stem>.cwasm` plus a
/// `<stem>.cwasm.meta` sidecar.
///
/// `cache_dir` is created with mode 0700 if missing (matches the host's
/// trust check). `cwasm` and sidecar files are written with mode 0600.
///
/// On any failure the cwasm or sidecar may not exist; callers should
/// treat that as "no cache" — the host will re-precompile in-memory at
/// startup.
pub fn precompile(
    wasm_path: &Path,
    cache_dir: &Path,
    engine: &wasmtime::Engine,
) -> Result<PrecompileOutput, String> {
    let wasm_bytes = std::fs::read(wasm_path)
        .map_err(|e| format!("read {}: {}", wasm_path.display(), e))?;
    let wasm_sha = sha256_hex(&wasm_bytes);

    // Ensure the cache directory exists with the right permissions.
    ensure_cache_dir(cache_dir)?;

    let stem = wasm_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("invalid wasm filename: {}", wasm_path.display()))?;
    let cwasm_path = cache_dir.join(format!("{}.cwasm", stem));
    let sidecar_path = cache_dir.join(format!("{}.cwasm.meta", stem));

    // `precompile_component` returns the same byte stream the host's
    // `Component::deserialize` consumes. The host re-validates the
    // four-tuple before deserialize, so a stale cwasm is rejected without
    // crossing the unsafe boundary.
    let serialized = engine
        .precompile_component(&wasm_bytes)
        .map_err(|e| format!("precompile_component {}: {}", wasm_path.display(), e))?;

    write_with_mode(&cwasm_path, &serialized, 0o600)?;

    let cache_key = CacheKey::for_precompile(wasm_sha);
    SidecarMeta::new(cache_key.clone()).write_to(&sidecar_path)?;
    set_mode(&sidecar_path, 0o600)?;

    Ok(PrecompileOutput {
        cwasm_path,
        sidecar_path,
        cache_key,
    })
}

/// Create the cache directory if missing and ensure mode 0700 (Unix only;
/// other platforms fall back to existence check).
fn ensure_cache_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create cache dir {}: {}", dir.display(), e))?;
    set_mode(dir, 0o700)?;
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("chmod {}: {}", path.display(), e))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn write_with_mode(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(path)
        .map_err(|e| format!("open {}: {}", path.display(), e))?;
    f.write_all(bytes)
        .map_err(|e| format!("write {}: {}", path.display(), e))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_with_mode(path: &Path, bytes: &[u8], _mode: u32) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("write {}: {}", path.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_config_hash_is_deterministic() {
        let a = engine_config_hash(ENGINE_FINGERPRINT);
        let b = engine_config_hash(ENGINE_FINGERPRINT);
        assert_eq!(a, b);
    }

    #[test]
    fn engine_config_hash_differs_for_different_fingerprints() {
        let a = engine_config_hash("a");
        let b = engine_config_hash("b");
        assert_ne!(a, b);
    }

    #[test]
    fn cache_key_for_precompile_uses_pinned_constants() {
        let k = CacheKey::for_precompile("abc");
        assert_eq!(k.wasm_sha256, "abc");
        assert_eq!(k.wasmtime_version, WASMTIME_VERSION);
        assert_eq!(k.engine_config_hash, engine_config_hash(ENGINE_FINGERPRINT));
    }

    #[test]
    fn sidecar_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugin.cwasm.meta");
        let key = CacheKey::for_precompile("deadbeef");
        let meta = SidecarMeta::new(key.clone());
        meta.write_to(&path).unwrap();
        let bytes = std::fs::read_to_string(&path).unwrap();
        let parsed: SidecarMeta = toml::from_str(&bytes).unwrap();
        assert_eq!(parsed.schema, SidecarMeta::SCHEMA_VERSION);
        assert_eq!(parsed.key, key);
    }

    #[test]
    fn make_engine_succeeds() {
        let _engine = make_engine().expect("engine");
    }

    #[test]
    fn make_metadata_engine_succeeds() {
        let _engine = make_metadata_engine().expect("metadata engine");
    }
}
