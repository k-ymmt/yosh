use std::path::Path;

use serde::{Deserialize, Serialize};

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
    pub sha256: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

fn default_true() -> bool {
    true
}

pub fn load_lockfile(path: &Path) -> Result<LockFile, String> {
    if !path.exists() {
        return Ok(LockFile { plugin: Vec::new() });
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("{}: {}", path.display(), e))?;
    toml::from_str(&content)
        .map_err(|e| format!("{}: {}", path.display(), e))
}

pub fn save_lockfile(path: &Path, lockfile: &LockFile) -> Result<(), String> {
    let content = toml::to_string_pretty(lockfile)
        .map_err(|e| format!("serialize lock file: {}", e))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("{}: {}", parent.display(), e))?;
    }
    std::fs::write(path, content)
        .map_err(|e| format!("{}: {}", path.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> LockEntry {
        LockEntry {
            name: "git-status".into(),
            path: "~/.kish/plugins/git-status/libgit_status.dylib".into(),
            enabled: true,
            capabilities: Some(vec!["variables:read".into(), "io".into()]),
            sha256: "abc123".into(),
            source: "github:user/repo".into(),
            version: Some("1.2.3".into()),
        }
    }

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("plugins.lock");
        let original = LockFile { plugin: vec![sample_entry()] };
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
        let lf = LockFile { plugin: vec![sample_entry()] };
        save_lockfile(&lock_path, &lf).unwrap();
        assert!(lock_path.exists());
    }

    #[test]
    fn local_entry_without_version() {
        let entry = LockEntry {
            name: "local-tool".into(),
            path: "~/.kish/plugins/liblocal.dylib".into(),
            enabled: true,
            capabilities: Some(vec!["io".into()]),
            sha256: "def456".into(),
            source: "local:~/.kish/plugins/liblocal.dylib".into(),
            version: None,
        };
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("plugins.lock");
        let original = LockFile { plugin: vec![entry] };
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
    fn empty_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("plugins.lock");
        let lf = LockFile { plugin: vec![] };
        save_lockfile(&lock_path, &lf).unwrap();
        let loaded = load_lockfile(&lock_path).unwrap();
        assert!(loaded.plugin.is_empty());
    }
}
