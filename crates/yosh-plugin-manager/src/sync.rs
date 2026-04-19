use std::path::PathBuf;

use crate::config::{self, PluginDecl, PluginSource};
use crate::github::GitHubClient;
use crate::lockfile::{LockEntry, LockFile, load_lockfile, save_lockfile};
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
    let mut new_entries: Vec<LockEntry> = Vec::new();
    let mut succeeded: Vec<String> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();

    for decl in &decls {
        match sync_one(&client, decl, &existing_lock) {
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
) -> Result<LockEntry, String> {
    let existing = existing_lock.plugin.iter().find(|e| e.name == decl.name);

    match &decl.source {
        PluginSource::GitHub { owner, repo } => {
            let version = decl.version.as_deref().unwrap(); // validated in config
            let asset_name = asset_filename(&decl.name, decl.asset.as_deref());
            let dest_dir = plugin_dir().join(&decl.name);
            let dest_path = dest_dir.join(&asset_name);

            // Check if unchanged (same version, already in lock, file exists)
            if let Some(existing) = existing {
                if existing.version.as_deref() == Some(version) && dest_path.exists() {
                    match verify_checksum(&dest_path, &existing.sha256) {
                        Ok(true) => {
                            return Ok(LockEntry {
                                name: decl.name.clone(),
                                path: format!("~/.yosh/plugins/{}/{}", decl.name, asset_name),
                                enabled: decl.enabled,
                                capabilities: decl.capabilities.clone(),
                                sha256: existing.sha256.clone(),
                                source: format!("github:{}/{}", owner, repo),
                                version: Some(version.to_string()),
                            });
                        }
                        Ok(false) => {
                            eprintln!(
                                "yosh-plugin: {}: local binary checksum mismatch, re-downloading",
                                decl.name
                            );
                        }
                        Err(e) => {
                            eprintln!("yosh-plugin: {}: verify failed: {}", decl.name, e);
                        }
                    }
                }
            }

            // Download
            let url = client.find_asset_url(owner, repo, version, &asset_name)?;
            std::fs::create_dir_all(&dest_dir)
                .map_err(|e| format!("create dir {}: {}", dest_dir.display(), e))?;
            client.download(&url, &dest_path)?;
            let sha256 = sha256_file(&dest_path)?;

            // If re-downloading same version and hash changed, that's suspicious
            if let Some(existing) = existing {
                if existing.version.as_deref() == Some(version) && sha256 != existing.sha256 {
                    let _ = std::fs::remove_file(&dest_path);
                    return Err(format!(
                        "re-downloaded binary has different checksum (expected {}, got {}). \
                         The upstream release asset may have been replaced.",
                        existing.sha256, sha256
                    ));
                }
            }

            Ok(LockEntry {
                name: decl.name.clone(),
                path: format!("~/.yosh/plugins/{}/{}", decl.name, asset_name),
                enabled: decl.enabled,
                capabilities: decl.capabilities.clone(),
                sha256,
                source: format!("github:{}/{}", owner, repo),
                version: Some(version.to_string()),
            })
        }
        PluginSource::Local { path } => {
            let resolved = config::expand_tilde_path(path);
            if !resolved.exists() {
                return Err(format!("file not found: {}", resolved.display()));
            }
            let sha256 = sha256_file(&resolved)?;
            Ok(LockEntry {
                name: decl.name.clone(),
                path: path.clone(),
                enabled: decl.enabled,
                capabilities: decl.capabilities.clone(),
                sha256,
                source: format!("local:{}", path),
                version: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn expand_tilde_via_config() {
        let result = config::expand_tilde_path("~/.yosh/plugins/lib.dylib");
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
        let entry = sync_one(&client, &decl, &empty_lock).unwrap();
        assert_eq!(entry.name, "local-test");
        assert_eq!(entry.path, path);
        assert!(!entry.sha256.is_empty());
        assert!(entry.version.is_none());
    }

    #[test]
    fn sync_one_local_plugin_missing_file() {
        let decl = PluginDecl {
            name: "missing".into(),
            source: PluginSource::Local {
                path: "/nonexistent/lib.dylib".into(),
            },
            version: None,
            enabled: true,
            capabilities: None,
            asset: None,
        };
        let client = GitHubClient::new();
        let empty_lock = LockFile { plugin: vec![] };
        let result = sync_one(&client, &decl, &empty_lock);
        assert!(result.is_err());
    }
}
