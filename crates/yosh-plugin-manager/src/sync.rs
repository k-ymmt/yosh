use std::path::{Path, PathBuf};

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
    sync_with_paths(
        &config_path(),
        &lock_path(),
        &plugin_dir(),
        prune,
        &GitHubClient::new(),
    )
}

fn sync_with_paths(
    config_path: &Path,
    lock_path: &Path,
    plugin_root: &Path,
    prune: bool,
    client: &GitHubClient,
) -> Result<SyncResult, String> {
    let decls = config::load_config(config_path)?;

    let existing_lock = match load_lockfile(lock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("yosh-plugin: warning: {}", e);
            LockFile { plugin: Vec::new() }
        }
    };

    // One engine each, shared across plugins. Building engines is non-trivial
    // (cranelift initialisation), so reusing them for the whole sync run
    // amortises the cost. precompile and metadata engines are flag-equivalent
    // but kept as separate handles so each call site documents its semantic
    // intent (cwasm production vs one-shot metadata watchdog).
    let precompile_engine = precompile::make_engine()?;
    let metadata_engine = precompile::make_engine()?;

    let mut new_entries: Vec<LockEntry> = Vec::new();
    let mut succeeded: Vec<String> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();

    for decl in &decls {
        match sync_one(
            client,
            decl,
            &existing_lock,
            plugin_root,
            &precompile_engine,
            &metadata_engine,
        ) {
            Ok(entry) => {
                succeeded.push(decl.name.clone());
                new_entries.push(entry);
            }
            Err(e) => {
                eprintln!("yosh-plugin: {}: {}", decl.name, e);
                // A transient per-plugin failure must not drop the
                // plugin's previously-good lock entry — the shell loads
                // from plugins.lock, and the valid install is still on
                // disk. Carry the old entry forward.
                if let Some(prev) = existing_lock.plugin.iter().find(|p| p.name == decl.name) {
                    eprintln!("yosh-plugin: {}: keeping previous lock entry", decl.name);
                    new_entries.push(prev.clone());
                }
                failed.push((decl.name.clone(), e));
            }
        }
    }

    // Prune: delete binaries for plugins removed from config. Only the
    // manager's own copies are touched: a `local:` plugin's `path` is
    // the user's artifact (e.g. a build output inside their project),
    // so it is left alone — only the lock entry and the manager-managed
    // cwasm cache go away.
    if prune {
        for old in &existing_lock.plugin {
            if !decls.iter().any(|d| d.name == old.name) {
                let is_local = old.source.starts_with("local:");
                let path = config::expand_tilde_path(&old.path);
                if !is_local && path.exists() {
                    if let Err(e) = std::fs::remove_file(&path) {
                        eprintln!("yosh-plugin: prune {}: {}", old.name, e);
                    } else {
                        eprintln!("yosh-plugin: pruned {}", old.name);
                    }
                }
                if is_local {
                    eprintln!(
                        "yosh-plugin: pruned {} (local artifact kept at {})",
                        old.name,
                        path.display()
                    );
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
                if !is_local && let Some(parent) = path.parent() {
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
    save_lockfile(lock_path, &new_lock)?;

    Ok(SyncResult { succeeded, failed })
}

/// `plugin_root` is the manager-managed install root (production:
/// `~/.yosh/plugins`). Each plugin's wasm, cwasm, and sidecar live
/// under `<plugin_root>/<name>/`; the host cache validator checks that
/// the directory is mode 0700 and uid-owned.
fn sync_one(
    client: &GitHubClient,
    decl: &PluginDecl,
    existing_lock: &LockFile,
    plugin_root: &Path,
    precompile_engine: &wasmtime::Engine,
    metadata_engine: &wasmtime::Engine,
) -> Result<LockEntry, String> {
    let existing = existing_lock.plugin.iter().find(|e| e.name == decl.name);

    match &decl.source {
        PluginSource::GitHub { owner, repo } => {
            let version = decl.version.as_deref().unwrap(); // validated in config
            let asset_name = asset_filename(&decl.name, decl.asset.as_deref());
            let dest_dir = plugin_root.join(&decl.name);
            let dest_path = dest_dir.join(&asset_name);

            // Verify the local file against the lock entry whenever we
            // could otherwise skip the download (same version, file
            // present). A mismatch or verify error marks the file
            // untrusted so the download below repairs it — never
            // re-hash and re-pin tampered content.
            let same_version = existing
                .map(|e| e.version.as_deref() == Some(version))
                .unwrap_or(false);
            let mut local_file_trusted = false;
            if let Some(existing) = existing
                && same_version
                && dest_path.exists()
            {
                match verify_checksum(&dest_path, &existing.sha256) {
                    Ok(true) => {
                        local_file_trusted = true;
                        // Fast path: checksum ok AND cwasm + metadata
                        // already cached in the lock. cwasm sidecar might
                        // still be stale on disk (e.g. prior `prune`
                        // removed it); if so fall through to re-run
                        // precompile + metadata (skip download).
                        let cwasm_present = existing
                            .cwasm_path
                            .as_deref()
                            .map(config::expand_tilde_path)
                            .map(|p| p.exists())
                            .unwrap_or(false);
                        if existing.required_capabilities.is_some() && cwasm_present {
                            return Ok(with_decl_limits(existing, decl));
                        }
                    }
                    Ok(false) => {
                        eprintln!(
                            "yosh-plugin: {}: local checksum mismatch, re-downloading",
                            decl.name
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "yosh-plugin: {}: verify failed: {}; re-downloading",
                            decl.name, e
                        );
                    }
                }
            }

            // Download if the file is missing, stale, or untrusted.
            let need_download = !dest_path.exists() || !same_version || !local_file_trusted;
            let upstream_sha256 = if need_download {
                let url = client.find_asset_url(owner, repo, version, &asset_name)?;
                std::fs::create_dir_all(&dest_dir)
                    .map_err(|e| format!("create dir {}: {}", dest_dir.display(), e))?;
                client.download(&url, &dest_path)?;
                let sha = sha256_file(&dest_path)?;

                // Re-download integrity check vs prior lock entry.
                if let Some(existing) = existing
                    && existing.version.as_deref() == Some(version)
                    && let Some(prev_upstream) = existing.upstream_sha256.as_deref()
                    && sha != prev_upstream
                {
                    let _ = std::fs::remove_file(&dest_path);
                    return Err(format!(
                        "re-downloaded asset has different checksum \
                         (expected {}, got {}). \
                         The upstream release asset may have been replaced.",
                        prev_upstream, sha
                    ));
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

            let cache_dir = plugin_root.join(&decl.name);
            let pre = precompile::precompile(&dest_path, &cache_dir, precompile_engine)
                .map_err(|e| format!("precompile: {}", e))?;
            let cwasm_rel = format!(
                "~/.yosh/plugins/{}/{}.cwasm",
                decl.name,
                asset_stem(&asset_name)
            );
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
                max_memory_mb: decl.max_memory_mb,
                hook_timeout_ms: decl.hook_timeout_ms,
                command_timeout_ms: decl.command_timeout_ms,
                pre_prompt_timeout_ms: decl.pre_prompt_timeout_ms,
                allowed_commands: decl.allowed_commands.clone(),
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
            let cache_dir = plugin_root.join(&decl.name);
            let pre_result = precompile::precompile(&resolved, &cache_dir, precompile_engine);

            // Local-plugin tolerance: if precompile or metadata fails (e.g.
            // the user pointed at a non-component file), we record the entry
            // without the cached fields so the host can still try to load
            // it the slow path. Tests exercise the no-metadata case with
            // throwaway "fake binary" content.
            let (cwasm_fields, meta_fields): (Option<PrecompileOutput>, Option<ExtractedMetadata>) =
                match (pre_result, metadata_result) {
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
            let implemented_hooks = meta_fields.as_ref().map(|m| m.implemented_hooks.clone());

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
                max_memory_mb: decl.max_memory_mb,
                hook_timeout_ms: decl.hook_timeout_ms,
                command_timeout_ms: decl.command_timeout_ms,
                pre_prompt_timeout_ms: decl.pre_prompt_timeout_ms,
                allowed_commands: decl.allowed_commands.clone(),
            })
        }
    }
}

/// Clone `entry` but refresh the four per-plugin resource-limit fields
/// and the `commands:exec` allowlist from `decl`. Used on the GitHub
/// "already synced" fast path so that editing a limit or
/// `allowed_commands` in `plugins.toml` and re-running `sync` takes
/// effect without forcing a re-download/re-precompile. Note: `enabled`
/// and `capabilities` are deliberately left untouched here — their
/// identical staleness on this fast path is pre-existing behaviour
/// tracked separately.
fn with_decl_limits(entry: &LockEntry, decl: &PluginDecl) -> LockEntry {
    let mut refreshed = entry.clone();
    refreshed.max_memory_mb = decl.max_memory_mb;
    refreshed.hook_timeout_ms = decl.hook_timeout_ms;
    refreshed.command_timeout_ms = decl.command_timeout_ms;
    refreshed.pre_prompt_timeout_ms = decl.pre_prompt_timeout_ms;
    refreshed.allowed_commands = decl.allowed_commands.clone();
    refreshed
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
    s.strip_prefix(&home).map(|rest| format!("~{}", rest))
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
            max_memory_mb: None,
            hook_timeout_ms: None,
            command_timeout_ms: None,
            pre_prompt_timeout_ms: None,
            allowed_commands: None,
        };
        let client = GitHubClient::new();
        let empty_lock = LockFile { plugin: vec![] };
        let root = tempfile::tempdir().unwrap();
        let pre_engine = precompile::make_engine().unwrap();
        let meta_engine = precompile::make_engine().unwrap();
        let entry = sync_one(
            &client,
            &decl,
            &empty_lock,
            root.path(),
            &pre_engine,
            &meta_engine,
        )
        .unwrap();
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
            max_memory_mb: None,
            hook_timeout_ms: None,
            command_timeout_ms: None,
            pre_prompt_timeout_ms: None,
            allowed_commands: None,
        };
        let client = GitHubClient::new();
        let empty_lock = LockFile { plugin: vec![] };
        let root = tempfile::tempdir().unwrap();
        let pre_engine = precompile::make_engine().unwrap();
        let meta_engine = precompile::make_engine().unwrap();
        let result = sync_one(
            &client,
            &decl,
            &empty_lock,
            root.path(),
            &pre_engine,
            &meta_engine,
        );
        assert!(result.is_err());
    }

    /// Regression: a local file whose checksum no longer matches the
    /// lock entry (tampered or corrupted) must be re-downloaded, not
    /// silently re-hashed and re-pinned into the lockfile. The mock
    /// server 404s, so an attempted re-download surfaces as a
    /// "failed to fetch release" error — whereas the old buggy path
    /// never contacted the network and failed later in metadata
    /// extraction (or worse, re-pinned a valid tampered component).
    #[test]
    fn sync_one_github_checksum_mismatch_triggers_redownload() {
        let root = tempfile::tempdir().unwrap();
        let dest_dir = root.path().join("gh-test");
        std::fs::create_dir_all(&dest_dir).unwrap();
        // Content hash differs from the lock entry's pinned sha256.
        std::fs::write(dest_dir.join("gh_test.wasm"), b"tampered content").unwrap();

        let mut server = mockito::Server::new();
        let _m1 = server
            .mock("GET", "/repos/owner/repo/releases/tags/v1.0.0")
            .with_status(404)
            .create();
        let _m2 = server
            .mock("GET", "/repos/owner/repo/releases/tags/1.0.0")
            .with_status(404)
            .create();
        let client = crate::github::GitHubClientWithBase::new(&server.url()).into_client();

        let existing = sample_lock_entry(); // sha256 "aaa" != actual hash
        let lock = LockFile {
            plugin: vec![existing],
        };
        let decl = sample_decl_with_limits(None, None, None, None); // github owner/repo @1.0.0
        let pre_engine = precompile::make_engine().unwrap();
        let meta_engine = precompile::make_engine().unwrap();
        let err = sync_one(
            &client,
            &decl,
            &lock,
            root.path(),
            &pre_engine,
            &meta_engine,
        )
        .expect_err("mismatched checksum must attempt re-download, which 404s here");
        assert!(
            err.contains("release not found"),
            "expected a re-download attempt, got: {}",
            err
        );
    }

    /// Regression: a per-plugin sync failure (here: upstream 404) must
    /// not drop the plugin's previously-good lock entry — the shell
    /// loads from plugins.lock and the valid install is still on disk.
    #[test]
    fn sync_keeps_previous_lock_entry_when_plugin_fails() {
        let home = tempfile::tempdir().unwrap();
        let config_path = home.path().join("plugins.toml");
        let lock_path = home.path().join("plugins.lock");
        let plugin_root = home.path().join("plugins");
        std::fs::write(
            &config_path,
            "[[plugin]]\nname = \"gh-test\"\nsource = \"github:owner/repo\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        let prev = sample_lock_entry();
        save_lockfile(
            &lock_path,
            &LockFile {
                plugin: vec![prev.clone()],
            },
        )
        .unwrap();

        // No local file installed and the mock upstream 404s → this
        // plugin's sync fails.
        let mut server = mockito::Server::new();
        let _m1 = server
            .mock("GET", "/repos/owner/repo/releases/tags/v1.0.0")
            .with_status(404)
            .create();
        let _m2 = server
            .mock("GET", "/repos/owner/repo/releases/tags/1.0.0")
            .with_status(404)
            .create();
        let client = crate::github::GitHubClientWithBase::new(&server.url()).into_client();

        let result =
            sync_with_paths(&config_path, &lock_path, &plugin_root, false, &client).unwrap();
        assert_eq!(result.failed.len(), 1);

        let lock = load_lockfile(&lock_path).unwrap();
        assert_eq!(
            lock.plugin.len(),
            1,
            "failed plugin's previous lock entry must be preserved"
        );
        assert_eq!(lock.plugin[0], prev);
    }

    /// Regression: `sync --prune` must not delete the artifact of a
    /// `local:` plugin — that path is the user's own build output, not
    /// a manager-managed copy under the plugin root. Manager-managed
    /// cwasm sidecars are still removed.
    #[test]
    fn prune_keeps_local_plugin_artifact() {
        let home = tempfile::tempdir().unwrap();
        let config_path = home.path().join("plugins.toml");
        let lock_path = home.path().join("plugins.lock");
        let plugin_root = home.path().join("plugins");
        // Plugin removed from config → prune target.
        std::fs::write(&config_path, "").unwrap();

        let artifact_dir = home.path().join("user-project/target");
        std::fs::create_dir_all(&artifact_dir).unwrap();
        let artifact = artifact_dir.join("my_plugin.wasm");
        std::fs::write(&artifact, b"user build output").unwrap();

        // Manager-managed cwasm for the same plugin — this one SHOULD go.
        let cwasm_dir = plugin_root.join("my_plugin");
        std::fs::create_dir_all(&cwasm_dir).unwrap();
        let cwasm = cwasm_dir.join("my_plugin.cwasm");
        std::fs::write(&cwasm, b"cwasm").unwrap();

        let artifact_str = artifact.to_string_lossy().to_string();
        let entry = LockEntry {
            name: "my_plugin".into(),
            path: artifact_str.clone(),
            enabled: true,
            capabilities: None,
            sha256: "abc".into(),
            upstream_sha256: None,
            source: format!("local:{}", artifact_str),
            version: None,
            cwasm_path: Some(cwasm.to_string_lossy().to_string()),
            wasmtime_version: None,
            target_triple: None,
            engine_config_hash: None,
            required_capabilities: None,
            implemented_hooks: None,
            max_memory_mb: None,
            hook_timeout_ms: None,
            command_timeout_ms: None,
            pre_prompt_timeout_ms: None,
            allowed_commands: None,
        };
        save_lockfile(
            &lock_path,
            &LockFile {
                plugin: vec![entry],
            },
        )
        .unwrap();

        let client = GitHubClient::new();
        sync_with_paths(&config_path, &lock_path, &plugin_root, true, &client).unwrap();

        assert!(
            artifact.exists(),
            "local: plugin artifact must survive --prune"
        );
        assert!(!cwasm.exists(), "manager-managed cwasm must be pruned");
        let lock = load_lockfile(&lock_path).unwrap();
        assert!(lock.plugin.is_empty());
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

    fn sample_lock_entry() -> LockEntry {
        LockEntry {
            name: "gh-test".into(),
            path: "~/.yosh/plugins/gh-test/gh-test.wasm".into(),
            enabled: false,
            capabilities: Some(vec!["io".into()]),
            sha256: "aaa".into(),
            upstream_sha256: Some("aaa".into()),
            source: "github:owner/repo".into(),
            version: Some("1.0.0".into()),
            cwasm_path: Some("~/.yosh/plugins/gh-test/gh-test.cwasm".into()),
            wasmtime_version: Some("1.2.3".into()),
            target_triple: Some("aarch64-apple-darwin".into()),
            engine_config_hash: Some("hash".into()),
            required_capabilities: Some(vec!["io".into()]),
            implemented_hooks: Some(vec!["pre_exec".into()]),
            max_memory_mb: Some(32),
            hook_timeout_ms: Some(500),
            command_timeout_ms: Some(10_000),
            pre_prompt_timeout_ms: Some(100),
            allowed_commands: Some(vec!["stale-cmd".into()]),
        }
    }

    fn sample_decl_with_limits(
        max_memory_mb: Option<u64>,
        hook_timeout_ms: Option<u64>,
        command_timeout_ms: Option<u64>,
        pre_prompt_timeout_ms: Option<u64>,
    ) -> PluginDecl {
        PluginDecl {
            name: "gh-test".into(),
            source: PluginSource::GitHub {
                owner: "owner".into(),
                repo: "repo".into(),
            },
            version: Some("1.0.0".into()),
            // Deliberately different from the existing entry so the
            // "left untouched" assertions below are meaningful.
            enabled: true,
            capabilities: None,
            asset: None,
            max_memory_mb,
            hook_timeout_ms,
            command_timeout_ms,
            pre_prompt_timeout_ms,
            allowed_commands: Some(vec!["whoami".into(), "git status:*".into()]),
        }
    }

    #[test]
    fn with_decl_limits_refreshes_all_four_fields() {
        let existing = sample_lock_entry();
        let decl = sample_decl_with_limits(Some(64), Some(1_000), Some(30_000), Some(250));

        let refreshed = with_decl_limits(&existing, &decl);

        assert_eq!(refreshed.max_memory_mb, Some(64));
        assert_eq!(refreshed.hook_timeout_ms, Some(1_000));
        assert_eq!(refreshed.command_timeout_ms, Some(30_000));
        assert_eq!(refreshed.pre_prompt_timeout_ms, Some(250));
    }

    /// Regression: `allowed_commands` from `plugins.toml` must reach the
    /// lock entry, both on a fresh sync and on the already-synced fast
    /// path — the shell host reads the exec allowlist from the lock, so
    /// dropping it here silently denies every `commands:exec` call.
    #[test]
    fn sync_one_local_propagates_allowed_commands() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"fake binary content").unwrap();
        let path = f.path().to_string_lossy().to_string();

        let decl = PluginDecl {
            name: "local-test".into(),
            source: PluginSource::Local { path },
            version: None,
            enabled: true,
            capabilities: None,
            asset: None,
            max_memory_mb: None,
            hook_timeout_ms: None,
            command_timeout_ms: None,
            pre_prompt_timeout_ms: None,
            allowed_commands: Some(vec!["whoami".into(), "hostname".into()]),
        };
        let client = GitHubClient::new();
        let empty_lock = LockFile { plugin: vec![] };
        let root = tempfile::tempdir().unwrap();
        let pre_engine = precompile::make_engine().unwrap();
        let meta_engine = precompile::make_engine().unwrap();
        let entry = sync_one(
            &client,
            &decl,
            &empty_lock,
            root.path(),
            &pre_engine,
            &meta_engine,
        )
        .unwrap();
        assert_eq!(
            entry.allowed_commands,
            Some(vec!["whoami".to_string(), "hostname".to_string()])
        );
    }

    #[test]
    fn with_decl_limits_refreshes_allowed_commands() {
        let existing = sample_lock_entry(); // allowed_commands: ["stale-cmd"]
        let decl = sample_decl_with_limits(None, None, None, None);

        let refreshed = with_decl_limits(&existing, &decl);

        assert_eq!(
            refreshed.allowed_commands,
            Some(vec!["whoami".to_string(), "git status:*".to_string()])
        );
    }

    #[test]
    fn with_decl_limits_can_clear_a_previously_set_limit() {
        let existing = sample_lock_entry();
        // decl now omits all limits (user deleted the lines from
        // plugins.toml) — the refreshed entry must drop them too.
        let decl = sample_decl_with_limits(None, None, None, None);

        let refreshed = with_decl_limits(&existing, &decl);

        assert_eq!(refreshed.max_memory_mb, None);
        assert_eq!(refreshed.hook_timeout_ms, None);
        assert_eq!(refreshed.command_timeout_ms, None);
        assert_eq!(refreshed.pre_prompt_timeout_ms, None);
    }

    #[test]
    fn with_decl_limits_leaves_enabled_and_capabilities_untouched() {
        let existing = sample_lock_entry();
        let decl = sample_decl_with_limits(Some(64), Some(1_000), Some(30_000), Some(250));

        let refreshed = with_decl_limits(&existing, &decl);

        // `enabled` is false on `existing` and true on `decl` — the
        // fast path's pre-existing staleness for this field must be
        // preserved (tracked separately, not in scope for this fix).
        assert_eq!(refreshed.enabled, existing.enabled);
        assert_eq!(refreshed.capabilities, existing.capabilities);
    }

    #[test]
    fn with_decl_limits_preserves_non_limit_fields() {
        let existing = sample_lock_entry();
        let decl = sample_decl_with_limits(Some(64), Some(1_000), Some(30_000), Some(250));

        let refreshed = with_decl_limits(&existing, &decl);

        assert_eq!(refreshed.name, existing.name);
        assert_eq!(refreshed.path, existing.path);
        assert_eq!(refreshed.sha256, existing.sha256);
        assert_eq!(refreshed.upstream_sha256, existing.upstream_sha256);
        assert_eq!(refreshed.source, existing.source);
        assert_eq!(refreshed.version, existing.version);
        assert_eq!(refreshed.cwasm_path, existing.cwasm_path);
        assert_eq!(refreshed.wasmtime_version, existing.wasmtime_version);
        assert_eq!(refreshed.target_triple, existing.target_triple);
        assert_eq!(refreshed.engine_config_hash, existing.engine_config_hash);
        assert_eq!(
            refreshed.required_capabilities,
            existing.required_capabilities
        );
        assert_eq!(refreshed.implemented_hooks, existing.implemented_hooks);
    }
}
