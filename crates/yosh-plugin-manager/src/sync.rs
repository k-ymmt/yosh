use std::path::PathBuf;

use crate::config::{self, PluginDecl, PluginSource};
use crate::github::GitHubClient;
use crate::lockfile::{LockEntry, LockFile, load_lockfile, save_lockfile};
use crate::metadata_extract::{self, ExtractedMetadata};
use crate::precompile::{self, PrecompileOutput};
use crate::resolve::asset_filename;
use crate::verify::{sha256_file, verify_checksum};

fn plugin_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".yosh/plugins")
    } else {
        PathBuf::from("/tmp/yosh/plugins")
    }
}

fn config_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config/yosh")
    } else {
        PathBuf::from("/tmp/yosh")
    }
}

/// Per-plugin cwasm cache root. Mirrors `<HOME>/.yosh/plugins/<name>/`
/// — the `.cwasm` and sidecar live next to the source `.wasm`. The host
/// cache validator checks that the directory is mode 0700 and uid-owned,
/// so we co-locate them under the plugin dir which we already control.
fn cache_dir_for(plugin_name: &str) -> PathBuf {
    plugin_dir().join(plugin_name)
}

pub fn config_path() -> PathBuf {
    config_dir().join("plugins.toml")
}

pub fn lock_path() -> PathBuf {
    config_dir().join("plugins.lock")
}

pub struct SyncResult {
    pub succeeded: Vec<String>,
    pub failed: Vec<(String, String)>, // (name, error)
}

/// Run the sync flow: read config, diff against lock, download/verify, write lock.
pub fn sync(prune: bool) -> Result<SyncResult, String> {
    let config_path = config_path();
    let lock_path = lock_path();

    let decls = config::load_config(&config_path)?;

    let existing_lock = match load_lockfile(&lock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("yosh-plugin: warning: {}", e);
            LockFile { plugin: Vec::new() }
        }
    };

    let client = GitHubClient::new();
    // One engine each, shared across plugins. Building engines is non-trivial
    // (cranelift initialisation), so reusing them for the whole sync run
    // amortises the cost.
    let precompile_engine = precompile::make_engine()?;
    let metadata_engine = precompile::make_metadata_engine()?;

    let mut new_entries: Vec<LockEntry> = Vec::new();
    let mut succeeded: Vec<String> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();

    for decl in &decls {
        match sync_one(
            &client,
            decl,
            &existing_lock,
            &precompile_engine,
            &metadata_engine,
        ) {
            Ok(entry) => {
                succeeded.push(decl.name.clone());
                new_entries.push(entry);
            }
            Err(e) => {
                eprintln!("yosh-plugin: {}: {}", decl.name, e);
                failed.push((decl.name.clone(), e));
            }
        }
    }

    // Prune: delete binaries for plugins removed from config
    if prune {
        for old in &existing_lock.plugin {
            if !decls.iter().any(|d| d.name == old.name) {
                let path = config::expand_tilde_path(&old.path);
                if path.exists() {
                    if let Err(e) = std::fs::remove_file(&path) {
                        eprintln!("yosh-plugin: prune {}: {}", old.name, e);
                    } else {
                        eprintln!("yosh-plugin: pruned {}", old.name);
                    }
                }
                // Also drop any stale cwasm + sidecar.
                if let Some(cwasm) = &old.cwasm_path {
                    let p = config::expand_tilde_path(cwasm);
                    let _ = std::fs::remove_file(&p);
                    let meta = p.with_extension("cwasm.meta");
                    let _ = std::fs::remove_file(&meta);
                }
                // Best-effort: remove the now-empty per-plugin directory.
                // Manager-managed layout co-locates wasm + cwasm under
                // `<root>/<name>/`, so once both files are gone the dir
                // is typically empty. `remove_dir` fails fast if not
                // empty (e.g. user dropped a stray file there); we
                // ignore the error in that case.
                if let Some(parent) = path.parent() {
                    let _ = std::fs::remove_dir(parent);
                }
                if let Some(cwasm) = &old.cwasm_path {
                    let p = config::expand_tilde_path(cwasm);
                    if let Some(parent) = p.parent() {
                        let _ = std::fs::remove_dir(parent);
                    }
                }
            }
        }
    }

    let new_lock = LockFile {
        plugin: new_entries,
    };
    save_lockfile(&lock_path, &new_lock)?;

    Ok(SyncResult { succeeded, failed })
}

fn sync_one(
    client: &GitHubClient,
    decl: &PluginDecl,
    existing_lock: &LockFile,
    precompile_engine: &wasmtime::Engine,
    metadata_engine: &wasmtime::Engine,
) -> Result<LockEntry, String> {
    let existing = existing_lock.plugin.iter().find(|e| e.name == decl.name);

    match &decl.source {
        PluginSource::GitHub { owner, repo } => {
            let version = decl.version.as_deref().unwrap(); // validated in config
            let asset_name = asset_filename(&decl.name, decl.asset.as_deref());
            let dest_dir = plugin_dir().join(&decl.name);
            let dest_path = dest_dir.join(&asset_name);

            // Fast path: existing entry, same version, file present and
            // checksum matches, AND we already have cwasm + metadata cached
            // in the lock. If anything is missing fall through to the
            // download / precompile / metadata path so we can repair.
            if let Some(existing) = existing {
                if existing.version.as_deref() == Some(version)
                    && dest_path.exists()
                    && existing.cwasm_path.is_some()
                    && existing.required_capabilities.is_some()
                {
                    match verify_checksum(&dest_path, &existing.sha256) {
                        Ok(true) => {
                            // cwasm sidecar might still be stale on disk
                            // (e.g. prior `prune` removed it). Verify the
                            // file is present; if not, re-precompile only
                            // (skip download).
                            let cwasm_present = existing
                                .cwasm_path
                                .as_deref()
                                .map(config::expand_tilde_path)
                                .map(|p| p.exists())
                                .unwrap_or(false);
                            if cwasm_present {
                                return Ok(existing.clone());
                            }
                            // Fall through to re-run precompile + metadata.
                        }
                        Ok(false) => {
                            eprintln!(
                                "yosh-plugin: {}: local checksum mismatch, re-downloading",
                                decl.name
                            );
                        }
                        Err(e) => {
                            eprintln!("yosh-plugin: {}: verify failed: {}", decl.name, e);
                        }
                    }
                }
            }

            // Download (only if file is missing or stale).
            let need_download = !dest_path.exists()
                || existing
                    .map(|e| e.version.as_deref() != Some(version))
                    .unwrap_or(true);
            let upstream_sha256 = if need_download {
                let url = client.find_asset_url(owner, repo, version, &asset_name)?;
                std::fs::create_dir_all(&dest_dir)
                    .map_err(|e| format!("create dir {}: {}", dest_dir.display(), e))?;
                client.download(&url, &dest_path)?;
                let sha = sha256_file(&dest_path)?;

                // Re-download integrity check vs prior lock entry.
                if let Some(existing) = existing {
                    if existing.version.as_deref() == Some(version) {
                        if let Some(prev_upstream) = existing.upstream_sha256.as_deref() {
                            if sha != prev_upstream {
                                let _ = std::fs::remove_file(&dest_path);
                                return Err(format!(
                                    "re-downloaded asset has different checksum \
                                     (expected {}, got {}). \
                                     The upstream release asset may have been replaced.",
                                    prev_upstream, sha
                                ));
                            }
                        }
                    }
                }
                sha
            } else {
                sha256_file(&dest_path)?
            };

            // Precompile + metadata extraction. The wasm bytes are the same
            // input; we read them once and pass to both.
            let wasm_bytes = std::fs::read(&dest_path)
                .map_err(|e| format!("read {}: {}", dest_path.display(), e))?;

            let metadata = metadata_extract::extract(metadata_engine, &wasm_bytes)
                .map_err(|e| format!("metadata extract: {}", e))?;

            let cache_dir = cache_dir_for(&decl.name);
            let pre = precompile::precompile(&dest_path, &cache_dir, precompile_engine)
                .map_err(|e| format!("precompile: {}", e))?;
            let cwasm_rel = format!("~/.yosh/plugins/{}/{}.cwasm", decl.name, asset_stem(&asset_name));
            // Use the literal precompile output path for the lock entry
            // (which encodes the absolute path) so the host can find it
            // verbatim. If HOME is set, use the ~-prefixed form for
            // portability.
            let cwasm_path_str = tildify(&pre.cwasm_path).unwrap_or(cwasm_rel);

            // sha256 == upstream_sha256 in v0.2.0+ since we no longer
            // re-sign. Keep both fields populated for compatibility.
            Ok(LockEntry {
                name: decl.name.clone(),
                path: format!("~/.yosh/plugins/{}/{}", decl.name, asset_name),
                enabled: decl.enabled,
                capabilities: decl.capabilities.clone(),
                sha256: upstream_sha256.clone(),
                upstream_sha256: Some(upstream_sha256),
                source: format!("github:{}/{}", owner, repo),
                version: Some(version.to_string()),
                cwasm_path: Some(cwasm_path_str),
                wasmtime_version: Some(pre.cache_key.wasmtime_version.clone()),
                target_triple: Some(pre.cache_key.target_triple.clone()),
                engine_config_hash: Some(pre.cache_key.engine_config_hash.clone()),
                required_capabilities: Some(metadata.required_capabilities),
                implemented_hooks: Some(metadata.implemented_hooks),
            })
        }
        PluginSource::Local { path } => {
            let resolved = config::expand_tilde_path(path);
            if !resolved.exists() {
                return Err(format!("file not found: {}", resolved.display()));
            }
            let sha256 = sha256_file(&resolved)?;

            // Local plugins also benefit from precompile + metadata caching.
            let wasm_bytes = std::fs::read(&resolved)
                .map_err(|e| format!("read {}: {}", resolved.display(), e))?;

            let metadata_result = metadata_extract::extract(metadata_engine, &wasm_bytes);
            let cache_dir = cache_dir_for(&decl.name);
            let pre_result = precompile::precompile(&resolved, &cache_dir, precompile_engine);

            // Local-plugin tolerance: if precompile or metadata fails (e.g.
            // the user pointed at a non-component file), we record the entry
            // without the cached fields so the host can still try to load
            // it the slow path. Tests exercise the no-metadata case with
            // throwaway "fake binary" content.
            let (cwasm_fields, meta_fields): (
                Option<PrecompileOutput>,
                Option<ExtractedMetadata>,
            ) = match (pre_result, metadata_result) {
                (Ok(pre), Ok(meta)) => (Some(pre), Some(meta)),
                (Ok(pre), Err(_)) => (Some(pre), None),
                (Err(_), Ok(meta)) => (None, Some(meta)),
                (Err(_), Err(_)) => (None, None),
            };

            let cwasm_path = cwasm_fields.as_ref().and_then(|p| tildify(&p.cwasm_path));
            let wasmtime_version = cwasm_fields
                .as_ref()
                .map(|p| p.cache_key.wasmtime_version.clone());
            let target_triple = cwasm_fields
                .as_ref()
                .map(|p| p.cache_key.target_triple.clone());
            let engine_config_hash = cwasm_fields
                .as_ref()
                .map(|p| p.cache_key.engine_config_hash.clone());
            let required_capabilities = meta_fields
                .as_ref()
                .map(|m| m.required_capabilities.clone());
            let implemented_hooks = meta_fields
                .as_ref()
                .map(|m| m.implemented_hooks.clone());

            Ok(LockEntry {
                name: decl.name.clone(),
                path: path.clone(),
                enabled: decl.enabled,
                capabilities: decl.capabilities.clone(),
                sha256,
                upstream_sha256: None,
                source: format!("local:{}", path),
                version: None,
                cwasm_path,
                wasmtime_version,
                target_triple,
                engine_config_hash,
                required_capabilities,
                implemented_hooks,
            })
        }
    }
}

/// Extract `<stem>` from `<stem>.wasm`, or fall back to the whole name.
fn asset_stem(asset_name: &str) -> &str {
    asset_name.strip_suffix(".wasm").unwrap_or(asset_name)
}

/// Best-effort `~/...` rewrite for paths under `$HOME`. Returns `None`
/// when the path is not under HOME or HOME is unset; callers fall back
/// to the absolute string.
fn tildify(p: &std::path::Path) -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let s = p.to_string_lossy();
    s.strip_prefix(&home)
        .map(|rest| format!("~{}", rest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn expand_tilde_via_config() {
        let result = config::expand_tilde_path("~/.yosh/plugins/plugin.wasm");
        assert!(!result.to_string_lossy().starts_with("~"));
    }

    #[test]
    fn expand_tilde_absolute_path() {
        let result = config::expand_tilde_path("/absolute/path");
        assert_eq!(result, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn sync_one_local_plugin() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"fake binary content").unwrap();
        let path = f.path().to_string_lossy().to_string();

        let decl = PluginDecl {
            name: "local-test".into(),
            source: PluginSource::Local { path: path.clone() },
            version: None,
            enabled: true,
            capabilities: Some(vec!["io".into()]),
            asset: None,
        };
        let client = GitHubClient::new();
        let empty_lock = LockFile { plugin: vec![] };
        let pre_engine = precompile::make_engine().unwrap();
        let meta_engine = precompile::make_metadata_engine().unwrap();
        let entry = sync_one(&client, &decl, &empty_lock, &pre_engine, &meta_engine).unwrap();
        assert_eq!(entry.name, "local-test");
        assert_eq!(entry.path, path);
        assert!(!entry.sha256.is_empty());
        assert!(entry.version.is_none());
        // "fake binary content" is not a real component; precompile +
        // metadata extraction both fail and we fall through with all
        // cwasm/metadata fields unset. The lock entry is still recorded.
        assert!(entry.cwasm_path.is_none());
        assert!(entry.required_capabilities.is_none());
    }

    #[test]
    fn sync_one_local_plugin_missing_file() {
        let decl = PluginDecl {
            name: "missing".into(),
            source: PluginSource::Local {
                path: "/nonexistent/plugin.wasm".into(),
            },
            version: None,
            enabled: true,
            capabilities: None,
            asset: None,
        };
        let client = GitHubClient::new();
        let empty_lock = LockFile { plugin: vec![] };
        let pre_engine = precompile::make_engine().unwrap();
        let meta_engine = precompile::make_metadata_engine().unwrap();
        let result = sync_one(&client, &decl, &empty_lock, &pre_engine, &meta_engine);
        assert!(result.is_err());
    }

    #[test]
    fn asset_stem_strips_wasm_suffix() {
        assert_eq!(asset_stem("plugin.wasm"), "plugin");
        assert_eq!(asset_stem("my-plugin.wasm"), "my-plugin");
        assert_eq!(asset_stem("noext"), "noext");
    }

    #[test]
    fn tildify_under_home() {
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            return;
        }
        let p = std::path::PathBuf::from(&home).join("foo/bar.wasm");
        assert_eq!(tildify(&p), Some("~/foo/bar.wasm".to_string()));
    }

    #[test]
    fn tildify_outside_home_returns_none() {
        let p = std::path::PathBuf::from("/tmp/foo");
        assert_eq!(tildify(&p), None);
    }
}
