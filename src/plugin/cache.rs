//! cwasm cache key + sidecar metadata + 5-condition trust validation.
//!
//! See `docs/superpowers/specs/2026-04-27-wasm-plugin-runtime-design.md` §5
//! "cwasm trust model" for the full threat model. This module is purely
//! validation logic — it never decides whether to actually deserialize a
//! cwasm; the host (`mod.rs`) does that after the validator returns `Ok`.
//!
//! The four-tuple recorded in both `plugins.lock` and the `<basename>.cwasm.meta`
//! sidecar is `(wasm_sha256, wasmtime_version, target_triple, engine_config_hash)`.
//! See §5 of the spec for why each dimension matters.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Wasmtime version string used in the cache key. Hardcoded to match the
/// pinned `wasmtime = "27"` in `Cargo.toml`. Changing the pin REQUIRES
/// updating this constant — the resulting tuple mismatch invalidates all
/// existing cwasm caches, which is the desired behaviour on a wasmtime
/// upgrade (a precompiled artefact from version N is not safe to load on
/// version N+1).
pub const WASMTIME_VERSION: &str = "27";

/// Returns the target triple this binary was built for. Sourced from the
/// `TARGET` env var that cargo sets at build time, captured by the root
/// `build.rs` and re-emitted as the `TARGET_TRIPLE_OR_RUST_BUILT_IN`
/// `rustc-env` so it is available at runtime via `env!`.
pub fn target_triple() -> &'static str {
    // Fall back to "host" if the build script could not determine the target.
    option_env!("TARGET_TRIPLE_OR_RUST_BUILT_IN").unwrap_or("host")
}

/// Stable hash of the wasmtime `Config` flags that affect cwasm
/// compatibility. The hash is recomputed whenever the host's engine
/// construction changes; the host calls this with a string fingerprint of
/// the config it just built. Mismatches invalidate the cwasm.
///
/// We do not introspect `wasmtime::Config` directly — its fields are
/// private. Instead the host passes a small canonical string describing
/// the config flags it set (e.g. `"async=false;fuel=false;cranelift"`),
/// and we hash it here so the format is reusable across cache producers
/// (manager precompile path) and consumers (shell startup).
pub fn engine_config_hash(fingerprint: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(fingerprint.as_bytes());
    hex::encode(hasher.finalize())
}

/// SHA-256 of arbitrary bytes, hex-encoded. Used for the wasm file hash.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Convention: the sidecar metadata file lives next to the cwasm, with
/// suffix `.meta`. Used by both the manager (writes) and the host (reads).
pub fn sidecar_path(cwasm: &Path) -> std::path::PathBuf {
    let mut s = cwasm.as_os_str().to_owned();
    s.push(".meta");
    s.into()
}

/// The four-tuple cache key. Recorded both in `plugins.lock` (as fields
/// inside a `[[plugin]]` entry) and in the `<basename>.cwasm.meta`
/// sidecar next to each cwasm file. The lockfile is the manager's source
/// of truth; the sidecar makes orphan cwasm files self-describing for
/// `--prune` and integrity diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheKey {
    /// SHA-256 (hex) of the source `.wasm` file.
    pub wasm_sha256: String,
    /// Wasmtime version string. See `WASMTIME_VERSION`.
    pub wasmtime_version: String,
    /// Target triple, e.g. `aarch64-apple-darwin`.
    pub target_triple: String,
    /// Hex-encoded `engine_config_hash`. See `engine_config_hash()`.
    pub engine_config_hash: String,
}

impl CacheKey {
    /// Construct a cache key for the live runtime, using the given wasm
    /// file's SHA and engine config fingerprint.
    pub fn for_runtime(wasm_sha256: impl Into<String>, engine_fingerprint: &str) -> Self {
        CacheKey {
            wasm_sha256: wasm_sha256.into(),
            wasmtime_version: WASMTIME_VERSION.to_string(),
            target_triple: target_triple().to_string(),
            engine_config_hash: engine_config_hash(engine_fingerprint),
        }
    }

    /// True if every field matches `other`.
    pub fn matches(&self, other: &CacheKey) -> bool {
        self.wasm_sha256 == other.wasm_sha256
            && self.wasmtime_version == other.wasmtime_version
            && self.target_triple == other.target_triple
            && self.engine_config_hash == other.engine_config_hash
    }
}

/// Sidecar metadata layout. Stored as TOML next to each cwasm file as
/// `<basename>.cwasm.meta`. Includes a schema version so a future tuple
/// extension can detect old layouts.
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

    pub fn read_from(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read_to_string(path)
            .map_err(|e| format!("read cwasm sidecar {}: {}", path.display(), e))?;
        toml::from_str(&bytes).map_err(|e| format!("parse cwasm sidecar {}: {}", path.display(), e))
    }

    #[cfg(test)]
    pub fn write_to(&self, path: &Path) -> Result<(), String> {
        let s = toml::to_string(self)
            .map_err(|e| format!("serialize cwasm sidecar {}: {}", path.display(), e))?;
        std::fs::write(path, s)
            .map_err(|e| format!("write cwasm sidecar {}: {}", path.display(), e))
    }
}

/// Reasons a cwasm cache file was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheRejection {
    /// The cwasm file or sidecar does not exist on disk.
    Missing,
    /// `cwasm` or sidecar is not owned by the current uid.
    UidMismatch,
    /// `cwasm` is not mode 0600 / sidecar wrong perms / dir wrong perms.
    PermissionsTooOpen,
    /// Sidecar parse failure or schema mismatch.
    SidecarUnreadable(String),
    /// Cache key tuple does not match the live runtime.
    KeyMismatch,
    /// Source wasm is missing or its SHA-256 does not match the lockfile.
    WasmShaMismatch,
}

impl CacheRejection {
    /// Human-readable reason for the startup warning.
    pub fn as_str(&self) -> &'static str {
        match self {
            CacheRejection::Missing => "cwasm missing",
            CacheRejection::UidMismatch => "cwasm not owned by current user",
            CacheRejection::PermissionsTooOpen => "cwasm or cache dir permissions too permissive",
            CacheRejection::SidecarUnreadable(_) => "cwasm sidecar unreadable",
            CacheRejection::KeyMismatch => "cwasm cache key mismatch",
            CacheRejection::WasmShaMismatch => "wasm SHA-256 mismatch",
        }
    }
}

/// Validate the 5 trust conditions for a cwasm file. Returns `Ok(())` only
/// if every condition is satisfied. The host treats any rejection as
/// "cache absent" and falls back to in-memory precompile.
///
/// Conditions (from spec §5):
///  1. cwasm file owned by current uid.
///  2. cwasm file mode is 0600.
///  3. Containing cache directory owned by current uid, mode 0700.
///  4. Sidecar `.cwasm.meta` parses, schema matches, key tuple matches `runtime_key`.
///  5. Source `.wasm` exists at `wasm_path` and its SHA-256 matches `runtime_key.wasm_sha256`.
pub fn validate_cwasm(
    cwasm_path: &Path,
    sidecar_path: &Path,
    wasm_path: &Path,
    runtime_key: &CacheKey,
) -> Result<(), CacheRejection> {
    // Condition 1+2+3: filesystem trust.
    check_filesystem_trust(cwasm_path)?;

    // Condition 4: sidecar key tuple match.
    let meta = SidecarMeta::read_from(sidecar_path)
        .map_err(CacheRejection::SidecarUnreadable)?;
    if meta.schema != SidecarMeta::SCHEMA_VERSION {
        return Err(CacheRejection::SidecarUnreadable(format!(
            "schema {} != {}",
            meta.schema,
            SidecarMeta::SCHEMA_VERSION
        )));
    }
    if !meta.key.matches(runtime_key) {
        return Err(CacheRejection::KeyMismatch);
    }

    // Condition 5: wasm SHA-256 still matches.
    let wasm_bytes = std::fs::read(wasm_path).map_err(|_| CacheRejection::WasmShaMismatch)?;
    let actual = sha256_hex(&wasm_bytes);
    if actual != runtime_key.wasm_sha256 {
        return Err(CacheRejection::WasmShaMismatch);
    }

    Ok(())
}

/// Conditions 1-3: same-uid, mode 0600 file, mode 0700 parent dir.
#[cfg(unix)]
fn check_filesystem_trust(cwasm_path: &Path) -> Result<(), CacheRejection> {
    use std::os::unix::fs::MetadataExt;

    let cwasm_meta = match std::fs::metadata(cwasm_path) {
        Ok(m) => m,
        Err(_) => return Err(CacheRejection::Missing),
    };
    let parent = cwasm_path.parent().ok_or(CacheRejection::Missing)?;
    let parent_meta = match std::fs::metadata(parent) {
        Ok(m) => m,
        Err(_) => return Err(CacheRejection::Missing),
    };

    let current_uid = unsafe { libc::getuid() };
    if cwasm_meta.uid() != current_uid || parent_meta.uid() != current_uid {
        return Err(CacheRejection::UidMismatch);
    }

    let cwasm_mode = cwasm_meta.mode() & 0o777;
    let parent_mode = parent_meta.mode() & 0o777;
    if cwasm_mode != 0o600 || parent_mode != 0o700 {
        return Err(CacheRejection::PermissionsTooOpen);
    }

    Ok(())
}

#[cfg(not(unix))]
fn check_filesystem_trust(cwasm_path: &Path) -> Result<(), CacheRejection> {
    // Non-unix platforms: only existence check. Cache trust is best-effort
    // here; the spec's trust model assumes unix permission semantics.
    if !cwasm_path.exists() {
        return Err(CacheRejection::Missing);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn write_cwasm(dir: &Path, name: &str, content: &[u8]) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        path
    }

    fn make_cache_dir(parent: &TempDir) -> std::path::PathBuf {
        let dir = parent.path().join("cache");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    fn write_wasm(parent: &TempDir, name: &str, content: &[u8]) -> std::path::PathBuf {
        let path = parent.path().join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn make_runtime_key(wasm_bytes: &[u8]) -> CacheKey {
        CacheKey::for_runtime(sha256_hex(wasm_bytes), "test-fingerprint")
    }

    #[test]
    fn validate_happy_path() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = make_cache_dir(&tmp);
        let wasm_bytes = b"fake wasm bytes";
        let wasm_path = write_wasm(&tmp, "plugin.wasm", wasm_bytes);
        let runtime_key = make_runtime_key(wasm_bytes);

        let cwasm_path = write_cwasm(&cache_dir, "plugin.cwasm", b"fake cwasm");
        let sidecar_path = cache_dir.join("plugin.cwasm.meta");
        SidecarMeta::new(runtime_key.clone())
            .write_to(&sidecar_path)
            .unwrap();
        std::fs::set_permissions(&sidecar_path, std::fs::Permissions::from_mode(0o600)).unwrap();

        validate_cwasm(&cwasm_path, &sidecar_path, &wasm_path, &runtime_key)
            .expect("validation should pass");
    }

    #[test]
    fn rejects_missing_cwasm() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = make_cache_dir(&tmp);
        let wasm_bytes = b"fake wasm";
        let wasm_path = write_wasm(&tmp, "plugin.wasm", wasm_bytes);
        let runtime_key = make_runtime_key(wasm_bytes);

        let cwasm_path = cache_dir.join("does-not-exist.cwasm");
        let sidecar_path = cache_dir.join("does-not-exist.cwasm.meta");

        let res = validate_cwasm(&cwasm_path, &sidecar_path, &wasm_path, &runtime_key);
        assert_eq!(res.unwrap_err(), CacheRejection::Missing);
    }

    #[test]
    fn rejects_world_readable_cwasm() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = make_cache_dir(&tmp);
        let wasm_bytes = b"fake wasm";
        let wasm_path = write_wasm(&tmp, "plugin.wasm", wasm_bytes);
        let runtime_key = make_runtime_key(wasm_bytes);

        let cwasm_path = cache_dir.join("plugin.cwasm");
        std::fs::write(&cwasm_path, b"cwasm bytes").unwrap();
        // 0644 — too open.
        std::fs::set_permissions(&cwasm_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let sidecar_path = cache_dir.join("plugin.cwasm.meta");
        SidecarMeta::new(runtime_key.clone())
            .write_to(&sidecar_path)
            .unwrap();

        let res = validate_cwasm(&cwasm_path, &sidecar_path, &wasm_path, &runtime_key);
        assert_eq!(res.unwrap_err(), CacheRejection::PermissionsTooOpen);
    }

    #[test]
    fn rejects_key_tuple_mismatch() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = make_cache_dir(&tmp);
        let wasm_bytes = b"fake wasm";
        let wasm_path = write_wasm(&tmp, "plugin.wasm", wasm_bytes);
        let runtime_key = make_runtime_key(wasm_bytes);

        // Sidecar records a DIFFERENT wasmtime version.
        let mut stale_key = runtime_key.clone();
        stale_key.wasmtime_version = "26".to_string();

        let cwasm_path = write_cwasm(&cache_dir, "plugin.cwasm", b"fake cwasm");
        let sidecar_path = cache_dir.join("plugin.cwasm.meta");
        SidecarMeta::new(stale_key).write_to(&sidecar_path).unwrap();

        let res = validate_cwasm(&cwasm_path, &sidecar_path, &wasm_path, &runtime_key);
        assert_eq!(res.unwrap_err(), CacheRejection::KeyMismatch);
    }

    #[test]
    fn rejects_wasm_sha_mismatch() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = make_cache_dir(&tmp);
        let wasm_bytes = b"original wasm";
        let wasm_path = write_wasm(&tmp, "plugin.wasm", wasm_bytes);

        // Lockfile-recorded SHA matches the original bytes.
        let runtime_key = make_runtime_key(wasm_bytes);

        let cwasm_path = write_cwasm(&cache_dir, "plugin.cwasm", b"fake cwasm");
        let sidecar_path = cache_dir.join("plugin.cwasm.meta");
        SidecarMeta::new(runtime_key.clone())
            .write_to(&sidecar_path)
            .unwrap();

        // Tamper with the wasm AFTER the lockfile was recorded.
        std::fs::write(&wasm_path, b"tampered wasm content").unwrap();

        let res = validate_cwasm(&cwasm_path, &sidecar_path, &wasm_path, &runtime_key);
        assert_eq!(res.unwrap_err(), CacheRejection::WasmShaMismatch);
    }

    #[test]
    fn engine_config_hash_is_deterministic() {
        let a = engine_config_hash("async=false;fuel=false");
        let b = engine_config_hash("async=false;fuel=false");
        let c = engine_config_hash("async=true;fuel=false");
        assert_eq!(a, b, "same fingerprint must hash to the same digest");
        assert_ne!(a, c, "different fingerprints must produce different digests");
    }
}
