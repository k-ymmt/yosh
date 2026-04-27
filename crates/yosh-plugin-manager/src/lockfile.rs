use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockFile {
    #[serde(default)]
    pub plugin: Vec<LockEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockEntry {
    pub name: String,
    pub path: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,
    /// SHA-256 of the on-disk `.wasm` file. With the v0.2.0 component
    /// model migration the file is no longer a Mach-O / ELF binary, so
    /// this hash is identical to `upstream_sha256` (no local re-signing
    /// step). Both fields are kept for round-trip compatibility with
    /// older lockfiles.
    pub sha256: String,
    /// SHA-256 of the asset as served by the upstream source. Stable
    /// across machines; detects silent upstream replacement at
    /// re-download time. `None` for local plugins or lock entries
    /// written before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_sha256: Option<String>,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Path to the precompiled `.cwasm` next to the source `.wasm`.
    /// Used by the host's cache validator to skip re-precompiling at
    /// startup. `None` when precompile failed during sync (host falls
    /// back to in-memory `Component::new` in that case).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwasm_path: Option<String>,
    /// Wasmtime version that produced the `cwasm_path` artifact. Part
    /// of the four-tuple cache key; the host rejects the cwasm if its
    /// own pin differs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasmtime_version: Option<String>,
    /// Target triple the cwasm was precompiled for. Cwasm files are
    /// not portable across triples, so the host rejects mismatches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_triple: Option<String>,
    /// Hex-encoded SHA-256 of the engine config fingerprint. Lets the
    /// host detect when its `wasmtime::Config` changed since the cwasm
    /// was written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_config_hash: Option<String>,
    /// Plugin-self-reported capability strings extracted at sync time.
    /// Cached so `yosh-plugin list` can show capabilities without
    /// instantiating each plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_capabilities: Option<Vec<String>>,
    /// Plugin-self-reported hook names extracted at sync time. Same
    /// caching rationale as `required_capabilities`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implemented_hooks: Option<Vec<String>>,
}

fn default_true() -> bool {
    true
}

pub fn load_lockfile(path: &Path) -> Result<LockFile, String> {
    if !path.exists() {
        return Ok(LockFile { plugin: Vec::new() });
    }
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    toml::from_str(&content).map_err(|e| format!("{}: {}", path.display(), e))
}

pub fn save_lockfile(path: &Path, lockfile: &LockFile) -> Result<(), String> {
    let content =
        toml::to_string_pretty(lockfile).map_err(|e| format!("serialize lock file: {}", e))?;
    let parent = path
        .parent()
        .ok_or_else(|| format!("{}: no parent directory", path.display()))?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {}", parent.display(), e))?;
    let mut tmp =
        NamedTempFile::new_in(parent).map_err(|e| format!("{}: {}", path.display(), e))?;
    tmp.write_all(content.as_bytes())
        .map_err(|e| format!("{}: {}", path.display(), e))?;
    tmp.persist(path)
        .map_err(|e| format!("{}: {}", path.display(), e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> LockEntry {
        LockEntry {
            name: "git-status".into(),
            path: "~/.yosh/plugins/git-status/git_status.wasm".into(),
            enabled: true,
            capabilities: Some(vec!["variables:read".into(), "io".into()]),
            sha256: "abc123".into(),
            upstream_sha256: Some("upstream123".into()),
            source: "github:user/repo".into(),
            version: Some("1.2.3".into()),
            cwasm_path: None,
            wasmtime_version: None,
            target_triple: None,
            engine_config_hash: None,
            required_capabilities: None,
            implemented_hooks: None,
        }
    }

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("plugins.lock");
        let original = LockFile {
            plugin: vec![sample_entry()],
        };
        save_lockfile(&lock_path, &original).unwrap();
        let loaded = load_lockfile(&lock_path).unwrap();
        assert_eq!(original, loaded);
    }

    #[test]
    fn load_nonexistent_returns_empty() {
        let lf = load_lockfile(Path::new("/nonexistent/plugins.lock")).unwrap();
        assert!(lf.plugin.is_empty());
    }

    #[test]
    fn save_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("sub/dir/plugins.lock");
        let lf = LockFile {
            plugin: vec![sample_entry()],
        };
        save_lockfile(&lock_path, &lf).unwrap();
        assert!(lock_path.exists());
    }

    #[test]
    fn local_entry_without_version() {
        let entry = LockEntry {
            name: "local-tool".into(),
            path: "~/.yosh/plugins/local.wasm".into(),
            enabled: true,
            capabilities: Some(vec!["io".into()]),
            sha256: "def456".into(),
            upstream_sha256: None,
            source: "local:~/.yosh/plugins/local.wasm".into(),
            version: None,
            cwasm_path: None,
            wasmtime_version: None,
            target_triple: None,
            engine_config_hash: None,
            required_capabilities: None,
            implemented_hooks: None,
        };
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("plugins.lock");
        let original = LockFile {
            plugin: vec![entry],
        };
        save_lockfile(&lock_path, &original).unwrap();
        let loaded = load_lockfile(&lock_path).unwrap();
        assert_eq!(original, loaded);
        assert!(loaded.plugin[0].version.is_none());
    }

    #[test]
    fn partial_write_only_successful_entries() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("plugins.lock");
        let lf = LockFile {
            plugin: vec![sample_entry()],
        };
        save_lockfile(&lock_path, &lf).unwrap();
        let loaded = load_lockfile(&lock_path).unwrap();
        assert_eq!(loaded.plugin.len(), 1);
    }

    #[test]
    fn legacy_lockfile_without_upstream_sha256_loads() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("plugins.lock");
        // Lock file written by an older yosh-plugin-manager that did not know
        // about upstream_sha256. Loader must accept it and default the field
        // to None.
        let legacy = "\
[[plugin]]
name = \"old\"
path = \"~/.yosh/plugins/old/old.wasm\"
enabled = true
sha256 = \"deadbeef\"
source = \"github:u/r\"
version = \"0.1.0\"
";
        std::fs::write(&lock_path, legacy).unwrap();
        let loaded = load_lockfile(&lock_path).unwrap();
        assert_eq!(loaded.plugin.len(), 1);
        assert!(loaded.plugin[0].upstream_sha256.is_none());
    }

    #[test]
    fn empty_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("plugins.lock");
        let lf = LockFile { plugin: vec![] };
        save_lockfile(&lock_path, &lf).unwrap();
        let loaded = load_lockfile(&lock_path).unwrap();
        assert!(loaded.plugin.is_empty());
    }
}
