use std::path::{Path, PathBuf};

use crate::config::{self, PluginDecl, PluginSource};
use crate::github::GitHubClient;
use crate::lockfile::{LockEntry, LockFile, load_lockfile, save_lockfile};
use crate::resolve::asset_filename;
use crate::verify::{sha256_file, verify_checksum};

/// Re-apply an ad-hoc code signature to a freshly-downloaded Mach-O so that
/// its embedded `cs_mtime` matches the file's filesystem mtime.
///
/// macOS XNU rejects pages whose `cs_mtime != mtime` for ad-hoc / linker-signed
/// binaries (`cs_invalid_page`) and SIGKILLs the loading process. The mismatch
/// is unavoidable for any dylib that is signed in CI, transported through
/// artifact upload/download/release, and finally fetched over HTTP — every hop
/// rewrites the file's mtime while the signature's `cs_mtime` is frozen at
/// build time.
///
/// `codesign --force --sign -` replaces the signature with a fresh ad-hoc one
/// whose `cs_mtime` is aligned with the file's current mtime, mirroring the
/// approach Homebrew uses when pouring arm64 bottles.
///
/// On non-macOS targets this is a no-op (other kernels do not enforce
/// `cs_mtime`).
#[cfg(target_os = "macos")]
fn ad_hoc_resign(path: &Path) -> Result<(), String> {
    let output = std::process::Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(path)
        .output()
        .map_err(|e| {
            format!(
                "failed to invoke codesign for {}: {} \
                 (install Xcode Command Line Tools: 'xcode-select --install')",
                path.display(),
                e
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "codesign --force --sign - {} failed: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn ad_hoc_resign(_path: &Path) -> Result<(), String> {
    Ok(())
}

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
                                upstream_sha256: existing.upstream_sha256.clone(),
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
            // Hash the upstream bytes BEFORE re-signing so we can detect silent
            // upstream replacement across machines (the post-resign sha256 is
            // machine-specific on macOS).
            let upstream_sha256 = sha256_file(&dest_path)?;

            // If re-downloading same version and the upstream hash drifted,
            // that's suspicious. Skip the check when the previous lock entry
            // predates upstream_sha256 (legacy entry) — we have nothing to
            // compare against and re-recording the value is the recovery path.
            if let Some(existing) = existing {
                if existing.version.as_deref() == Some(version) {
                    if let Some(prev_upstream) = existing.upstream_sha256.as_deref() {
                        if upstream_sha256 != prev_upstream {
                            let _ = std::fs::remove_file(&dest_path);
                            return Err(format!(
                                "re-downloaded binary has different checksum \
                                 (expected {}, got {}). \
                                 The upstream release asset may have been replaced.",
                                prev_upstream, upstream_sha256
                            ));
                        }
                    }
                }
            }

            // Re-sign on macOS so cs_mtime matches the file's mtime; otherwise
            // dlopen will be SIGKILLed by XNU's code-signing enforcement.
            ad_hoc_resign(&dest_path)?;
            let sha256 = sha256_file(&dest_path)?;

            Ok(LockEntry {
                name: decl.name.clone(),
                path: format!("~/.yosh/plugins/{}/{}", decl.name, asset_name),
                enabled: decl.enabled,
                capabilities: decl.capabilities.clone(),
                sha256,
                upstream_sha256: Some(upstream_sha256),
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
                upstream_sha256: None,
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

    #[cfg(target_os = "macos")]
    #[test]
    fn ad_hoc_resign_succeeds_on_macho_and_aligns_mtime() {
        // Copy the test binary itself (a real Mach-O) and re-sign it. After
        // re-signing the file's mtime should be very close to the moment of
        // signing — the same condition the kernel uses to validate
        // ad-hoc-signed binaries at load time. We can't read cs_mtime from
        // userspace easily, but `codesign --verify` exercising the full check
        // is the real assertion.
        let exe = std::env::current_exe().expect("current_exe");
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("resign_target");
        std::fs::copy(&exe, &dest).expect("copy test binary");
        ad_hoc_resign(&dest).expect("ad_hoc_resign should succeed on a Mach-O");
        let verify = std::process::Command::new("codesign")
            .args(["--verify", "--strict"])
            .arg(&dest)
            .output()
            .expect("invoke codesign --verify");
        assert!(
            verify.status.success(),
            "codesign --verify failed after resign: {}",
            String::from_utf8_lossy(&verify.stderr)
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn ad_hoc_resign_is_noop_off_macos() {
        let f = tempfile::NamedTempFile::new().unwrap();
        // No content needed — the helper must not touch the file at all.
        let before = std::fs::metadata(f.path()).unwrap().modified().unwrap();
        ad_hoc_resign(f.path()).expect("no-op must succeed");
        let after = std::fs::metadata(f.path()).unwrap().modified().unwrap();
        assert_eq!(before, after, "no-op must not modify the file");
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
