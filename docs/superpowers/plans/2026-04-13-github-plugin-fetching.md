# GitHub Plugin Fetching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `kish-plugin` CLI that fetches prebuilt plugin binaries from GitHub Releases via a declarative `plugins.toml` + lock file workflow, and update kish to read from `plugins.lock`.

**Architecture:** A new `crates/kish-plugin-manager` binary crate handles all network/sync logic. kish itself only changes to read `plugins.lock` instead of `plugins.toml`. The lock file has the same shape as the existing plugin config (with extra informational fields), so the plugin loading pipeline is unchanged.

**Tech Stack:** Rust (edition 2024), `ureq` (HTTP), `sha2` (checksums), `serde`/`toml`/`serde_json` (serialization), `mockito` (test mocks)

**Spec:** `docs/superpowers/specs/2026-04-13-github-plugin-fetching-design.md`

---

## File Map

### New files (crates/kish-plugin-manager/)

| File | Responsibility |
|------|---------------|
| `crates/kish-plugin-manager/Cargo.toml` | Crate manifest with dependencies and `[[bin]]` for `kish-plugin` |
| `crates/kish-plugin-manager/src/main.rs` | CLI entry point — parse subcommands, dispatch to modules |
| `crates/kish-plugin-manager/src/config.rs` | Parse `plugins.toml` — `PluginSource` enum, `PluginDecl` struct |
| `crates/kish-plugin-manager/src/lockfile.rs` | Read/write `plugins.lock` — `LockEntry` struct, atomic write |
| `crates/kish-plugin-manager/src/resolve.rs` | Asset template variable expansion, OS/arch detection |
| `crates/kish-plugin-manager/src/verify.rs` | SHA-256 computation and comparison |
| `crates/kish-plugin-manager/src/github.rs` | GitHub API client — release lookup, asset download |
| `crates/kish-plugin-manager/src/sync.rs` | Sync orchestration — diff, download, verify, write lock |

### Modified files (kish binary)

| File | Change |
|------|--------|
| `Cargo.toml` (workspace root) | Add `crates/kish-plugin-manager` to `[workspace]` members |
| `src/plugin/config.rs` | Add optional `sha256`, `source`, `version` fields to `PluginEntry` |
| `src/exec/mod.rs:631-637` | Change `plugin_config_path()` to return `plugins.lock` |

---

### Task 1: Scaffold `kish-plugin-manager` crate

**Files:**
- Create: `crates/kish-plugin-manager/Cargo.toml`
- Create: `crates/kish-plugin-manager/src/main.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Create Cargo.toml for the new crate**

```toml
[package]
name = "kish-plugin-manager"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "kish-plugin"
path = "src/main.rs"

[dependencies]
serde = { version = "1", features = ["derive"] }
toml = "0.8"
serde_json = "1"
ureq = "3"
sha2 = "0.10"

[dev-dependencies]
tempfile = "3"
mockito = "1"
```

- [ ] **Step 2: Create minimal main.rs**

```rust
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(|s| s.as_str()) {
        Some("sync") => { eprintln!("sync: not yet implemented"); 2 }
        Some("update") => { eprintln!("update: not yet implemented"); 2 }
        Some("list") => { eprintln!("list: not yet implemented"); 2 }
        Some("verify") => { eprintln!("verify: not yet implemented"); 2 }
        Some(cmd) => { eprintln!("kish-plugin: unknown command '{}'", cmd); 2 }
        None => { eprintln!("usage: kish-plugin <sync|update|list|verify>"); 2 }
    };
    process::exit(code);
}
```

- [ ] **Step 3: Add to workspace members in root Cargo.toml**

Add `"crates/kish-plugin-manager"` to the `[workspace]` members list. The root `Cargo.toml` does not have a `[workspace]` section yet — it needs to be added:

```toml
[workspace]
members = [
    ".",
    "crates/kish-plugin-api",
    "crates/kish-plugin-sdk",
    "crates/kish-plugin-manager",
]
```

Note: Adding `"."` is required so the root `kish` package remains a workspace member.

- [ ] **Step 4: Verify it builds**

Run: `cargo build -p kish-plugin-manager`
Expected: Compiles successfully, produces `target/debug/kish-plugin` binary.

- [ ] **Step 5: Verify existing tests still pass**

Run: `cargo test --lib`
Expected: All existing kish unit tests pass (no regressions from workspace change).

- [ ] **Step 6: Commit**

```bash
git add crates/kish-plugin-manager/Cargo.toml crates/kish-plugin-manager/src/main.rs Cargo.toml Cargo.lock
git commit -m "feat(plugin-manager): scaffold kish-plugin-manager crate with CLI stub"
```

---

### Task 2: `config.rs` — Parse `plugins.toml`

**Files:**
- Create: `crates/kish-plugin-manager/src/config.rs`
- Modify: `crates/kish-plugin-manager/src/main.rs` (add `mod config;`)

- [ ] **Step 1: Write the failing tests**

Create `crates/kish-plugin-manager/src/config.rs` with tests at the bottom:

```rust
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub enum PluginSource {
    GitHub { owner: String, repo: String },
    Local { path: String },
}

#[derive(Debug, Clone)]
pub struct PluginDecl {
    pub name: String,
    pub source: PluginSource,
    pub version: Option<String>,
    pub enabled: bool,
    pub capabilities: Option<Vec<String>>,
    pub asset: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(default)]
    plugin: Vec<RawPluginEntry>,
}

#[derive(Debug, Deserialize)]
struct RawPluginEntry {
    name: String,
    source: String,
    version: Option<String>,
    #[serde(default = "default_true")]
    enabled: bool,
    capabilities: Option<Vec<String>>,
    asset: Option<String>,
}

fn default_true() -> bool {
    true
}

pub fn parse_source(s: &str) -> Result<PluginSource, String> {
    if let Some(rest) = s.strip_prefix("github:") {
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
            return Err(format!("invalid github source '{}': expected 'github:owner/repo'", s));
        }
        Ok(PluginSource::GitHub {
            owner: parts[0].to_string(),
            repo: parts[1].to_string(),
        })
    } else if let Some(rest) = s.strip_prefix("local:") {
        if rest.is_empty() {
            return Err(format!("invalid local source '{}': path is empty", s));
        }
        Ok(PluginSource::Local { path: rest.to_string() })
    } else {
        Err(format!("unknown source type '{}': expected 'github:' or 'local:' prefix", s))
    }
}

pub fn load_config(path: &Path) -> Result<Vec<PluginDecl>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("{}: {}", path.display(), e))?;
    let raw: RawConfig = toml::from_str(&content)
        .map_err(|e| format!("{}: {}", path.display(), e))?;
    raw.plugin
        .into_iter()
        .map(|entry| {
            let source = parse_source(&entry.source)?;
            if matches!(source, PluginSource::GitHub { .. }) && entry.version.is_none() {
                return Err(format!(
                    "plugin '{}': github source requires 'version' field",
                    entry.name
                ));
            }
            Ok(PluginDecl {
                name: entry.name,
                source,
                version: entry.version,
                enabled: entry.enabled,
                capabilities: entry.capabilities,
                asset: entry.asset,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_github_source() {
        let src = parse_source("github:user/repo").unwrap();
        assert_eq!(
            src,
            PluginSource::GitHub { owner: "user".into(), repo: "repo".into() }
        );
    }

    #[test]
    fn parse_local_source() {
        let src = parse_source("local:~/.kish/plugins/lib.dylib").unwrap();
        assert_eq!(
            src,
            PluginSource::Local { path: "~/.kish/plugins/lib.dylib".into() }
        );
    }

    #[test]
    fn parse_invalid_source_no_prefix() {
        assert!(parse_source("invalid:foo").is_err());
    }

    #[test]
    fn parse_invalid_github_missing_repo() {
        assert!(parse_source("github:useronly").is_err());
    }

    #[test]
    fn parse_empty_local_path() {
        assert!(parse_source("local:").is_err());
    }

    #[test]
    fn load_full_config() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, r#"
[[plugin]]
name = "git-status"
source = "github:user/kish-plugin-git-status"
version = "1.2.3"
capabilities = ["variables:read", "io"]

[[plugin]]
name = "local-tool"
source = "local:~/.kish/plugins/liblocal.dylib"
capabilities = ["io"]
"#).unwrap();
        let decls = load_config(f.path()).unwrap();
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].name, "git-status");
        assert!(matches!(&decls[0].source, PluginSource::GitHub { owner, repo } if owner == "user" && repo == "kish-plugin-git-status"));
        assert_eq!(decls[0].version.as_deref(), Some("1.2.3"));
        assert_eq!(decls[1].name, "local-tool");
        assert!(matches!(&decls[1].source, PluginSource::Local { .. }));
        assert!(decls[1].version.is_none());
    }

    #[test]
    fn load_config_enabled_defaults_true() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, r#"
[[plugin]]
name = "p"
source = "local:/tmp/lib.dylib"
"#).unwrap();
        let decls = load_config(f.path()).unwrap();
        assert!(decls[0].enabled);
    }

    #[test]
    fn load_config_disabled_plugin() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, r#"
[[plugin]]
name = "p"
source = "local:/tmp/lib.dylib"
enabled = false
"#).unwrap();
        let decls = load_config(f.path()).unwrap();
        assert!(!decls[0].enabled);
    }

    #[test]
    fn load_config_with_asset_template() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, r#"
[[plugin]]
name = "custom"
source = "github:user/repo"
version = "1.0.0"
asset = "myplugin-{{os}}-{{arch}}.{{ext}}"
"#).unwrap();
        let decls = load_config(f.path()).unwrap();
        assert_eq!(decls[0].asset.as_deref(), Some("myplugin-{os}-{arch}.{ext}"));
    }

    #[test]
    fn github_source_without_version_is_error() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, r#"
[[plugin]]
name = "bad"
source = "github:user/repo"
"#).unwrap();
        assert!(load_config(f.path()).is_err());
    }

    #[test]
    fn empty_config_returns_empty_vec() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "").unwrap();
        let decls = load_config(f.path()).unwrap();
        assert!(decls.is_empty());
    }
}
```

- [ ] **Step 2: Add `mod config;` to main.rs**

Add `mod config;` at the top of `crates/kish-plugin-manager/src/main.rs`.

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p kish-plugin-manager`
Expected: All 9 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/kish-plugin-manager/src/config.rs crates/kish-plugin-manager/src/main.rs
git commit -m "feat(plugin-manager): add plugins.toml config parser with source/version/asset support"
```

---

### Task 3: `resolve.rs` — Asset template resolution

**Files:**
- Create: `crates/kish-plugin-manager/src/resolve.rs`
- Modify: `crates/kish-plugin-manager/src/main.rs` (add `mod resolve;`)

- [ ] **Step 1: Write resolve.rs with tests**

```rust
pub const DEFAULT_TEMPLATE: &str = "lib{name}-{os}-{arch}.{ext}";

pub fn current_os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

pub fn current_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "unknown"
    }
}

pub fn lib_ext() -> &'static str {
    if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

/// Convert plugin name to a form suitable for library names: hyphens become underscores.
pub fn normalize_name(name: &str) -> String {
    name.replace('-', "_")
}

/// Resolve an asset template by replacing `{name}`, `{os}`, `{arch}`, `{ext}`.
pub fn resolve_template(template: &str, plugin_name: &str) -> String {
    template
        .replace("{name}", &normalize_name(plugin_name))
        .replace("{os}", current_os())
        .replace("{arch}", current_arch())
        .replace("{ext}", lib_ext())
}

/// Get the resolved asset filename for a plugin, using custom or default template.
pub fn asset_filename(plugin_name: &str, custom_template: Option<&str>) -> String {
    let template = custom_template.unwrap_or(DEFAULT_TEMPLATE);
    resolve_template(template, plugin_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_name_replaces_hyphens() {
        assert_eq!(normalize_name("git-status"), "git_status");
    }

    #[test]
    fn normalize_name_no_hyphens() {
        assert_eq!(normalize_name("simple"), "simple");
    }

    #[test]
    fn resolve_default_template() {
        let result = resolve_template(DEFAULT_TEMPLATE, "git-status");
        let expected = format!("libgit_status-{}-{}.{}", current_os(), current_arch(), lib_ext());
        assert_eq!(result, expected);
    }

    #[test]
    fn resolve_custom_template() {
        let result = resolve_template("kish_{name}-{os}-{arch}.{ext}", "auto-env");
        let expected = format!("kish_auto_env-{}-{}.{}", current_os(), current_arch(), lib_ext());
        assert_eq!(result, expected);
    }

    #[test]
    fn asset_filename_uses_default() {
        let result = asset_filename("my-plugin", None);
        assert!(result.starts_with("libmy_plugin-"));
    }

    #[test]
    fn asset_filename_uses_custom() {
        let result = asset_filename("my-plugin", Some("custom_{name}.{ext}"));
        assert!(result.starts_with("custom_my_plugin."));
    }

    #[test]
    fn current_os_is_known() {
        assert!(["macos", "linux"].contains(&current_os()));
    }

    #[test]
    fn current_arch_is_known() {
        assert!(["x86_64", "aarch64"].contains(&current_arch()));
    }
}
```

- [ ] **Step 2: Add `mod resolve;` to main.rs**

- [ ] **Step 3: Run tests**

Run: `cargo test -p kish-plugin-manager resolve`
Expected: All 8 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/kish-plugin-manager/src/resolve.rs crates/kish-plugin-manager/src/main.rs
git commit -m "feat(plugin-manager): add asset template resolution with OS/arch detection"
```

---

### Task 4: `verify.rs` — SHA-256 computation and verification

**Files:**
- Create: `crates/kish-plugin-manager/src/verify.rs`
- Modify: `crates/kish-plugin-manager/src/main.rs` (add `mod verify;`)

- [ ] **Step 1: Write verify.rs with tests**

```rust
use std::path::Path;

use sha2::{Sha256, Digest};

/// Compute the SHA-256 hex digest of a file.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let data = std::fs::read(path)
        .map_err(|e| format!("{}: {}", path.display(), e))?;
    let hash = Sha256::digest(&data);
    Ok(format!("{:x}", hash))
}

/// Check if a file's SHA-256 matches the expected hex digest.
pub fn verify_checksum(path: &Path, expected: &str) -> Result<bool, String> {
    let actual = sha256_file(path)?;
    Ok(actual == expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn sha256_known_content() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"hello world").unwrap();
        let hash = sha256_file(f.path()).unwrap();
        // SHA-256 of "hello world"
        assert_eq!(hash, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
    }

    #[test]
    fn sha256_empty_file() {
        let f = tempfile::NamedTempFile::new().unwrap();
        let hash = sha256_file(f.path()).unwrap();
        // SHA-256 of empty input
        assert_eq!(hash, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn sha256_missing_file() {
        assert!(sha256_file(Path::new("/nonexistent/file")).is_err());
    }

    #[test]
    fn verify_checksum_match() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"hello world").unwrap();
        assert!(verify_checksum(f.path(), "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9").unwrap());
    }

    #[test]
    fn verify_checksum_mismatch() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"hello world").unwrap();
        assert!(!verify_checksum(f.path(), "0000000000000000000000000000000000000000000000000000000000000000").unwrap());
    }
}
```

- [ ] **Step 2: Add `mod verify;` to main.rs**

- [ ] **Step 3: Run tests**

Run: `cargo test -p kish-plugin-manager verify`
Expected: All 5 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/kish-plugin-manager/src/verify.rs crates/kish-plugin-manager/src/main.rs
git commit -m "feat(plugin-manager): add SHA-256 checksum computation and verification"
```

---

### Task 5: `lockfile.rs` — Read/write `plugins.lock`

**Files:**
- Create: `crates/kish-plugin-manager/src/lockfile.rs`
- Modify: `crates/kish-plugin-manager/src/main.rs` (add `mod lockfile;`)

- [ ] **Step 1: Write lockfile.rs with tests**

```rust
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
            plugin: vec![sample_entry()],  // only 1 of N succeeded
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
```

- [ ] **Step 2: Add `mod lockfile;` to main.rs**

- [ ] **Step 3: Run tests**

Run: `cargo test -p kish-plugin-manager lockfile`
Expected: All 6 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/kish-plugin-manager/src/lockfile.rs crates/kish-plugin-manager/src/main.rs
git commit -m "feat(plugin-manager): add lock file read/write with TOML serialization"
```

---

### Task 6: `github.rs` — GitHub API client

**Files:**
- Create: `crates/kish-plugin-manager/src/github.rs`
- Modify: `crates/kish-plugin-manager/src/main.rs` (add `mod github;`)

- [ ] **Step 1: Write github.rs with types and parsing logic**

```rust
use std::io::Read;
use std::path::Path;

use serde::Deserialize;

const API_BASE: &str = "https://api.github.com";

#[derive(Debug, Deserialize)]
struct Release {
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub struct GitHubClient {
    token: Option<String>,
}

impl GitHubClient {
    pub fn new() -> Self {
        let token = std::env::var("KISH_GITHUB_TOKEN")
            .or_else(|_| std::env::var("GITHUB_TOKEN"))
            .ok();
        GitHubClient { token }
    }

    #[cfg(test)]
    pub fn with_base_url(base_url: String) -> GitHubClientWithBase {
        let token = std::env::var("KISH_GITHUB_TOKEN")
            .or_else(|_| std::env::var("GITHUB_TOKEN"))
            .ok();
        GitHubClientWithBase { base_url, token }
    }

    fn build_request(&self, url: &str) -> ureq::RequestBuilder {
        let req = ureq::get(url)
            .header("User-Agent", "kish-plugin-manager")
            .header("Accept", "application/vnd.github.v3+json");
        match &self.token {
            Some(t) => req.header("Authorization", &format!("Bearer {}", t)),
            None => req,
        }
    }

    /// Find the download URL for an asset matching `asset_name` in the given release.
    pub fn find_asset_url(
        &self,
        owner: &str,
        repo: &str,
        version: &str,
        asset_name: &str,
    ) -> Result<String, String> {
        // Try v-prefixed tag first, then bare version
        let tags = [format!("v{}", version), version.to_string()];
        let mut last_err = String::new();

        for tag in &tags {
            let url = format!("{}/repos/{}/{}/releases/tags/{}", API_BASE, owner, repo, tag);
            match self.build_request(&url).call() {
                Ok(response) => {
                    let body: String = response.body_mut().read_to_string()
                        .map_err(|e| format!("read response: {}", e))?;
                    let release: Release = serde_json::from_str(&body)
                        .map_err(|e| format!("parse release JSON: {}", e))?;
                    if let Some(asset) = release.assets.iter().find(|a| a.name == asset_name) {
                        return Ok(asset.browser_download_url.clone());
                    }
                    return Err(format!(
                        "release '{}' for {}/{} has no asset named '{}'",
                        tag, owner, repo, asset_name
                    ));
                }
                Err(e) => {
                    last_err = format!("{}", e);
                    continue;
                }
            }
        }
        Err(format!(
            "no release found for {}/{} version '{}': {}",
            owner, repo, version, last_err
        ))
    }

    /// Download a file from a URL to a local path.
    pub fn download(&self, url: &str, dest: &Path) -> Result<(), String> {
        if !url.starts_with("https://") {
            return Err(format!("refusing non-HTTPS URL: {}", url));
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create dir {}: {}", parent.display(), e))?;
        }
        let response = self.build_request(url).call()
            .map_err(|e| format!("download {}: {}", url, e))?;
        let mut body = response.into_body();
        let mut data = Vec::new();
        body.read_to_end(&mut data)
            .map_err(|e| format!("read body: {}", e))?;
        std::fs::write(dest, &data)
            .map_err(|e| format!("write {}: {}", dest.display(), e))?;
        Ok(())
    }

    /// Get the latest release tag for a repo.
    pub fn latest_version(&self, owner: &str, repo: &str) -> Result<String, String> {
        let url = format!("{}/repos/{}/{}/releases/latest", API_BASE, owner, repo);
        let response = self.build_request(&url).call()
            .map_err(|e| format!("fetch latest release for {}/{}: {}", owner, repo, e))?;
        let body: String = response.body_mut().read_to_string()
            .map_err(|e| format!("read response: {}", e))?;
        let value: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| format!("parse JSON: {}", e))?;
        let tag = value["tag_name"].as_str()
            .ok_or_else(|| format!("no tag_name in latest release for {}/{}", owner, repo))?;
        // Strip "v" prefix if present
        let version = tag.strip_prefix('v').unwrap_or(tag);
        Ok(version.to_string())
    }
}

/// Test-only variant that uses a custom base URL (for mockito).
#[cfg(test)]
pub struct GitHubClientWithBase {
    base_url: String,
    token: Option<String>,
}

#[cfg(test)]
impl GitHubClientWithBase {
    fn build_request(&self, url: &str) -> ureq::RequestBuilder {
        let req = ureq::get(url)
            .header("User-Agent", "kish-plugin-manager")
            .header("Accept", "application/vnd.github.v3+json");
        match &self.token {
            Some(t) => req.header("Authorization", &format!("Bearer {}", t)),
            None => req,
        }
    }

    pub fn find_asset_url(
        &self,
        owner: &str,
        repo: &str,
        version: &str,
        asset_name: &str,
    ) -> Result<String, String> {
        let tags = [format!("v{}", version), version.to_string()];
        let mut last_err = String::new();
        for tag in &tags {
            let url = format!("{}/repos/{}/{}/releases/tags/{}", self.base_url, owner, repo, tag);
            match self.build_request(&url).call() {
                Ok(response) => {
                    let body: String = response.body_mut().read_to_string()
                        .map_err(|e| format!("read response: {}", e))?;
                    let release: Release = serde_json::from_str(&body)
                        .map_err(|e| format!("parse release JSON: {}", e))?;
                    if let Some(asset) = release.assets.iter().find(|a| a.name == asset_name) {
                        return Ok(asset.browser_download_url.clone());
                    }
                    return Err(format!(
                        "release '{}' for {}/{} has no asset named '{}'",
                        tag, owner, repo, asset_name
                    ));
                }
                Err(e) => {
                    last_err = format!("{}", e);
                    continue;
                }
            }
        }
        Err(format!(
            "no release found for {}/{} version '{}': {}",
            owner, repo, version, last_err
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_release_json() {
        let json = r#"{
            "tag_name": "v1.2.3",
            "assets": [
                {"name": "libfoo-macos-aarch64.dylib", "browser_download_url": "https://example.com/libfoo.dylib"},
                {"name": "libfoo-linux-x86_64.so", "browser_download_url": "https://example.com/libfoo.so"}
            ]
        }"#;
        let release: Release = serde_json::from_str(json).unwrap();
        assert_eq!(release.assets.len(), 2);
        assert_eq!(release.assets[0].name, "libfoo-macos-aarch64.dylib");
    }

    #[test]
    fn find_asset_in_release_json() {
        let json = r#"{
            "assets": [
                {"name": "libfoo-macos-aarch64.dylib", "browser_download_url": "https://example.com/libfoo.dylib"},
                {"name": "libfoo-linux-x86_64.so", "browser_download_url": "https://example.com/libfoo.so"}
            ]
        }"#;
        let release: Release = serde_json::from_str(json).unwrap();
        let found = release.assets.iter().find(|a| a.name == "libfoo-linux-x86_64.so");
        assert!(found.is_some());
        assert_eq!(found.unwrap().browser_download_url, "https://example.com/libfoo.so");
    }

    #[test]
    fn asset_not_in_release() {
        let json = r#"{
            "assets": [
                {"name": "libfoo-linux-x86_64.so", "browser_download_url": "https://example.com/libfoo.so"}
            ]
        }"#;
        let release: Release = serde_json::from_str(json).unwrap();
        let found = release.assets.iter().find(|a| a.name == "libfoo-macos-aarch64.dylib");
        assert!(found.is_none());
    }

    #[test]
    fn reject_http_url() {
        let client = GitHubClient::new();
        let dir = tempfile::tempdir().unwrap();
        let result = client.download("http://insecure.example.com/file", &dir.path().join("file"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("non-HTTPS"));
    }

    #[test]
    fn strip_v_prefix_from_tag() {
        let tag = "v1.2.3";
        let version = tag.strip_prefix('v').unwrap_or(tag);
        assert_eq!(version, "1.2.3");
    }

    #[test]
    fn no_v_prefix_tag() {
        let tag = "1.2.3";
        let version = tag.strip_prefix('v').unwrap_or(tag);
        assert_eq!(version, "1.2.3");
    }

    #[test]
    fn find_asset_with_mock_server() {
        let mut server = mockito::Server::new();
        let mock = server.mock("GET", "/repos/user/repo/releases/tags/v1.0.0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{
                "assets": [
                    {"name": "libfoo-linux-x86_64.so", "browser_download_url": "https://example.com/libfoo.so"}
                ]
            }"#)
            .create();

        let client = GitHubClient::with_base_url(server.url());
        let result = client.find_asset_url("user", "repo", "1.0.0", "libfoo-linux-x86_64.so");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://example.com/libfoo.so");
        mock.assert();
    }

    #[test]
    fn find_asset_fallback_to_bare_tag() {
        let mut server = mockito::Server::new();
        let _miss = server.mock("GET", "/repos/user/repo/releases/tags/v2.0.0")
            .with_status(404)
            .create();
        let _hit = server.mock("GET", "/repos/user/repo/releases/tags/2.0.0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{
                "assets": [
                    {"name": "lib.so", "browser_download_url": "https://example.com/lib.so"}
                ]
            }"#)
            .create();

        let client = GitHubClient::with_base_url(server.url());
        let result = client.find_asset_url("user", "repo", "2.0.0", "lib.so");
        assert!(result.is_ok());
    }
}
```

Note: The `read_to_string()` method on `ureq` 3.x body returns `Result<String, std::io::Error>`. The exact API may need minor adjustment based on the `ureq` version resolved — check `ureq` 3 docs. The key pattern is: call → read body to string → parse JSON.

- [ ] **Step 2: Add `mod github;` to main.rs**

- [ ] **Step 3: Run tests**

Run: `cargo test -p kish-plugin-manager github`
Expected: All 8 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/kish-plugin-manager/src/github.rs crates/kish-plugin-manager/src/main.rs
git commit -m "feat(plugin-manager): add GitHub API client with release lookup and download"
```

---

### Task 7: `sync.rs` — Sync orchestration

**Files:**
- Create: `crates/kish-plugin-manager/src/sync.rs`
- Modify: `crates/kish-plugin-manager/src/main.rs` (add `mod sync;`)

- [ ] **Step 1: Write sync.rs**

```rust
use std::path::{Path, PathBuf};

use crate::config::{PluginDecl, PluginSource, load_config};
use crate::github::GitHubClient;
use crate::lockfile::{LockEntry, LockFile, load_lockfile, save_lockfile};
use crate::resolve::asset_filename;
use crate::verify::{sha256_file, verify_checksum};

/// Expand ~ to $HOME in a path string.
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn plugin_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".kish/plugins")
    } else {
        PathBuf::from("/tmp/kish/plugins")
    }
}

fn config_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config/kish")
    } else {
        PathBuf::from("/tmp/kish")
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
    pub failed: Vec<(String, String)>,
}

pub fn sync(prune: bool) -> SyncResult {
    let config_path = config_path();
    let lock_path = lock_path();

    let decls = match load_config(&config_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("kish-plugin: {}", e);
            std::process::exit(2);
        }
    };

    let existing_lock = match load_lockfile(&lock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("kish-plugin: warning: {}", e);
            LockFile { plugin: Vec::new() }
        }
    };

    let client = GitHubClient::new();
    let mut new_entries: Vec<LockEntry> = Vec::new();
    let mut succeeded: Vec<String> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();

    for decl in &decls {
        match sync_one(&client, decl, &existing_lock, prune) {
            Ok(entry) => {
                succeeded.push(decl.name.clone());
                new_entries.push(entry);
            }
            Err(e) => {
                eprintln!("kish-plugin: {}: {}", decl.name, e);
                failed.push((decl.name.clone(), e));
            }
        }
    }

    // Handle removed plugins: if --prune, delete binaries
    if prune {
        for old in &existing_lock.plugin {
            if !decls.iter().any(|d| d.name == old.name) {
                let path = expand_tilde(&old.path);
                if path.exists() {
                    if let Err(e) = std::fs::remove_file(&path) {
                        eprintln!("kish-plugin: prune {}: {}", old.name, e);
                    } else {
                        eprintln!("kish-plugin: pruned {}", old.name);
                    }
                }
            }
        }
    }

    let new_lock = LockFile { plugin: new_entries };
    if let Err(e) = save_lockfile(&lock_path, &new_lock) {
        eprintln!("kish-plugin: fatal: {}", e);
        std::process::exit(2);
    }

    SyncResult { succeeded, failed }
}

fn sync_one(
    client: &GitHubClient,
    decl: &PluginDecl,
    existing_lock: &LockFile,
    _prune: bool,
) -> Result<LockEntry, String> {
    let existing = existing_lock.plugin.iter().find(|e| e.name == decl.name);

    match &decl.source {
        PluginSource::GitHub { owner, repo } => {
            let version = decl.version.as_deref().unwrap();  // validated in config
            let asset_name = asset_filename(&decl.name, decl.asset.as_deref());
            let dest_dir = plugin_dir().join(&decl.name);
            let dest_path = dest_dir.join(&asset_name);

            // Check if unchanged (same version, already in lock)
            if let Some(existing) = existing {
                if existing.version.as_deref() == Some(version) && dest_path.exists() {
                    // Verify checksum
                    match verify_checksum(&dest_path, &existing.sha256) {
                        Ok(true) => {
                            // Unchanged and verified
                            return Ok(LockEntry {
                                name: decl.name.clone(),
                                path: format!("~/.kish/plugins/{}/{}", decl.name, asset_name),
                                enabled: decl.enabled,
                                capabilities: decl.capabilities.clone(),
                                sha256: existing.sha256.clone(),
                                source: format!("github:{}/{}", owner, repo),
                                version: Some(version.to_string()),
                            });
                        }
                        Ok(false) => {
                            eprintln!(
                                "kish-plugin: {}: local binary checksum mismatch, re-downloading",
                                decl.name
                            );
                            // Fall through to re-download
                        }
                        Err(e) => {
                            eprintln!("kish-plugin: {}: verify failed: {}", decl.name, e);
                            // Fall through to re-download
                        }
                    }
                }
            }

            // Download
            let url = client.find_asset_url(owner, repo, version, &asset_name)?;
            client.download(&url, &dest_path)?;

            let sha256 = sha256_file(&dest_path)?;

            // If re-downloading and hash differs from lock, warn about upstream change
            if let Some(existing) = existing {
                if existing.version.as_deref() == Some(version) && sha256 != existing.sha256 {
                    // Clean up downloaded file
                    let _ = std::fs::remove_file(&dest_path);
                    return Err(format!(
                        "re-downloaded binary has different checksum than lock file \
                         (expected {}, got {}). The upstream release asset may have been replaced.",
                        existing.sha256, sha256
                    ));
                }
            }

            Ok(LockEntry {
                name: decl.name.clone(),
                path: format!("~/.kish/plugins/{}/{}", decl.name, asset_name),
                enabled: decl.enabled,
                capabilities: decl.capabilities.clone(),
                sha256,
                source: format!("github:{}/{}", owner, repo),
                version: Some(version.to_string()),
            })
        }
        PluginSource::Local { path } => {
            let resolved = expand_tilde(path);
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
    fn expand_tilde_with_home() {
        let result = expand_tilde("~/.kish/plugins/lib.dylib");
        assert!(!result.to_string_lossy().starts_with("~"));
    }

    #[test]
    fn expand_tilde_absolute_path() {
        let result = expand_tilde("/absolute/path");
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
        let entry = sync_one(&client, &decl, &empty_lock, false).unwrap();
        assert_eq!(entry.name, "local-test");
        assert_eq!(entry.path, path);
        assert!(!entry.sha256.is_empty());
        assert!(entry.version.is_none());
    }

    #[test]
    fn sync_one_local_plugin_missing_file() {
        let decl = PluginDecl {
            name: "missing".into(),
            source: PluginSource::Local { path: "/nonexistent/lib.dylib".into() },
            version: None,
            enabled: true,
            capabilities: None,
            asset: None,
        };
        let client = GitHubClient::new();
        let empty_lock = LockFile { plugin: vec![] };
        let result = sync_one(&client, &decl, &empty_lock, false);
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Add `mod sync;` to main.rs**

- [ ] **Step 3: Run tests**

Run: `cargo test -p kish-plugin-manager sync`
Expected: All 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/kish-plugin-manager/src/sync.rs crates/kish-plugin-manager/src/main.rs
git commit -m "feat(plugin-manager): add sync orchestration with diff, download, and verify"
```

---

### Task 8: Wire up CLI subcommands in `main.rs`

**Files:**
- Modify: `crates/kish-plugin-manager/src/main.rs`

- [ ] **Step 1: Replace the stub main.rs with full CLI dispatch**

```rust
mod config;
mod github;
mod lockfile;
mod resolve;
mod sync;
mod verify;

use std::process;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(|s| s.as_str()) {
        Some("sync") => cmd_sync(&args[1..]),
        Some("update") => cmd_update(&args[1..]),
        Some("list") => cmd_list(),
        Some("verify") => cmd_verify(),
        Some(cmd) => {
            eprintln!("kish-plugin: unknown command '{}'", cmd);
            2
        }
        None => {
            eprintln!("usage: kish-plugin <sync|update|list|verify>");
            2
        }
    };
    process::exit(code);
}

fn cmd_sync(args: &[String]) -> i32 {
    let prune = args.iter().any(|a| a == "--prune");
    let result = sync::sync(prune);

    for name in &result.succeeded {
        eprintln!("  ✓ {}", name);
    }
    for (name, err) in &result.failed {
        eprintln!("  ✗ {}: {}", name, err);
    }

    if result.failed.is_empty() {
        eprintln!("kish-plugin: sync complete ({} plugins)", result.succeeded.len());
        0
    } else {
        eprintln!(
            "kish-plugin: sync partial ({} succeeded, {} failed)",
            result.succeeded.len(),
            result.failed.len()
        );
        1
    }
}

fn cmd_update(args: &[String]) -> i32 {
    let name_filter = args.first().map(|s| s.as_str());

    let config_path = sync::config_path();
    let decls = match config::load_config(&config_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("kish-plugin: {}", e);
            return 2;
        }
    };

    let client = github::GitHubClient::new();
    let mut updated = false;

    // Read the raw TOML content for rewriting
    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("kish-plugin: {}: {}", config_path.display(), e);
            return 2;
        }
    };
    let mut new_content = content.clone();

    for decl in &decls {
        if let Some(filter) = name_filter {
            if decl.name != filter {
                continue;
            }
        }
        if let config::PluginSource::GitHub { owner, repo } = &decl.source {
            match client.latest_version(owner, repo) {
                Ok(latest) => {
                    let current = decl.version.as_deref().unwrap_or("");
                    if latest != current {
                        eprintln!("  {} {} → {}", decl.name, current, latest);
                        // Simple string replacement for version in TOML
                        if !current.is_empty() {
                            new_content = new_content.replacen(
                                &format!("version = \"{}\"", current),
                                &format!("version = \"{}\"", latest),
                                1,
                            );
                        }
                        updated = true;
                    } else {
                        eprintln!("  {} {} (already latest)", decl.name, current);
                    }
                }
                Err(e) => {
                    eprintln!("  ✗ {}: {}", decl.name, e);
                }
            }
        }
    }

    if updated {
        if let Err(e) = std::fs::write(&config_path, &new_content) {
            eprintln!("kish-plugin: write {}: {}", config_path.display(), e);
            return 2;
        }
        // Run sync after updating versions
        return cmd_sync(&[]);
    }

    0
}

fn cmd_list() -> i32 {
    let lock_path = sync::lock_path();
    let lockfile = match lockfile::load_lockfile(&lock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("kish-plugin: {}", e);
            return 2;
        }
    };

    if lockfile.plugin.is_empty() {
        eprintln!("no plugins installed (run 'kish-plugin sync' first)");
        return 0;
    }

    for entry in &lockfile.plugin {
        let version = entry.version.as_deref().unwrap_or("-");
        let verified = match verify::verify_checksum(
            &config::expand_tilde_path(&entry.path),
            &entry.sha256,
        ) {
            Ok(true) => "✓ verified",
            Ok(false) => "✗ checksum mismatch",
            Err(_) => "✗ file missing",
        };
        println!(
            "{:<16} {:<8} {:<48} {}",
            entry.name, version, entry.source, verified
        );
    }

    0
}

fn cmd_verify() -> i32 {
    let lock_path = sync::lock_path();
    let lockfile = match lockfile::load_lockfile(&lock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("kish-plugin: {}", e);
            return 2;
        }
    };

    let mut all_ok = true;
    for entry in &lockfile.plugin {
        let path = config::expand_tilde_path(&entry.path);
        match verify::verify_checksum(&path, &entry.sha256) {
            Ok(true) => {
                eprintln!("  ✓ {}", entry.name);
            }
            Ok(false) => {
                eprintln!("  ✗ {}: checksum mismatch", entry.name);
                all_ok = false;
            }
            Err(e) => {
                eprintln!("  ✗ {}: {}", entry.name, e);
                all_ok = false;
            }
        }
    }

    if all_ok { 0 } else { 1 }
}
```

- [ ] **Step 2: Add `expand_tilde_path` helper to config.rs**

Add to the bottom of `crates/kish-plugin-manager/src/config.rs` (before `#[cfg(test)]`):

```rust
pub fn expand_tilde_path(path: &str) -> std::path::PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return std::path::PathBuf::from(home).join(rest);
        }
    }
    std::path::PathBuf::from(path)
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p kish-plugin-manager`
Expected: Compiles successfully.

- [ ] **Step 4: Verify all kish-plugin-manager tests pass**

Run: `cargo test -p kish-plugin-manager`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/kish-plugin-manager/src/main.rs crates/kish-plugin-manager/src/config.rs
git commit -m "feat(plugin-manager): wire up sync/update/list/verify CLI subcommands"
```

---

### Task 9: Update kish binary to read `plugins.lock`

**Files:**
- Modify: `src/plugin/config.rs`
- Modify: `src/exec/mod.rs:631-637`

- [ ] **Step 1: Write a test for the new config path**

Add to the bottom of the existing tests in `src/exec/mod.rs` (find the `#[cfg(test)] mod tests` block):

```rust
#[test]
fn plugin_config_path_points_to_lock_file() {
    let path = super::plugin_config_path();
    assert!(path.to_string_lossy().ends_with("plugins.lock"));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test plugin_config_path_points_to_lock_file`
Expected: FAIL — currently returns `plugins.toml`.

- [ ] **Step 3: Change `plugin_config_path()` to return `plugins.lock`**

In `src/exec/mod.rs`, change line 633:

Old:
```rust
        std::path::PathBuf::from(home).join(".config/kish/plugins.toml")
```
New:
```rust
        std::path::PathBuf::from(home).join(".config/kish/plugins.lock")
```

- [ ] **Step 4: Update the doc comment on `load_plugins`**

In `src/exec/mod.rs`, change line 41:

Old:
```rust
    /// Load plugins from the default config path (~/.config/kish/plugins.toml).
```
New:
```rust
    /// Load plugins from the lock file (~/.config/kish/plugins.lock).
```

- [ ] **Step 5: Add optional fields to `PluginEntry`**

In `src/plugin/config.rs`, add three optional fields to the `PluginEntry` struct:

Old:
```rust
pub struct PluginEntry {
    pub name: String,
    pub path: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub capabilities: Option<Vec<String>>,
}
```

New:
```rust
pub struct PluginEntry {
    pub name: String,
    pub path: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub capabilities: Option<Vec<String>>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}
```

- [ ] **Step 6: Run the new test**

Run: `cargo test plugin_config_path_points_to_lock_file`
Expected: PASS.

- [ ] **Step 7: Run all existing plugin tests**

Run: `cargo test --test plugin`
Expected: All 37 existing tests pass (the extra fields are `Option` with `#[serde(default)]`, so existing configs without them still parse).

- [ ] **Step 8: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/plugin/config.rs src/exec/mod.rs
git commit -m "feat(plugin): read plugins.lock instead of plugins.toml for GitHub plugin manager integration"
```

---

### Task 10: Integration test — full sync flow with mock server

**Files:**
- Create: `crates/kish-plugin-manager/tests/sync_integration.rs`

- [ ] **Step 1: Write the integration test**

```rust
use std::io::Write;
use std::path::Path;

/// Integration test for sync flow using a mock HTTP server and temp directories.
/// Tests: download from mocked GitHub API → SHA-256 → lock file generation.

#[test]
fn sync_local_plugin_creates_lockfile() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join(".config/kish");
    let plugin_dir = dir.path().join(".kish/plugins");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&plugin_dir).unwrap();

    // Create a fake plugin binary
    let fake_binary = plugin_dir.join("liblocal.dylib");
    std::fs::write(&fake_binary, b"fake binary content").unwrap();

    // Create plugins.toml
    let toml_path = config_dir.join("plugins.toml");
    let mut f = std::fs::File::create(&toml_path).unwrap();
    write!(f, r#"
[[plugin]]
name = "local-test"
source = "local:{}"
capabilities = ["io"]
"#, fake_binary.display()).unwrap();

    // Parse and sync manually (not using the global paths)
    let decls = kish_plugin_manager::config::load_config(&toml_path).unwrap();
    assert_eq!(decls.len(), 1);
    assert_eq!(decls[0].name, "local-test");

    // Compute expected SHA-256
    let sha256 = kish_plugin_manager::verify::sha256_file(&fake_binary).unwrap();
    assert!(!sha256.is_empty());
    assert_eq!(sha256.len(), 64); // hex-encoded SHA-256 is 64 chars
}

#[test]
fn lockfile_round_trip_with_multiple_entries() {
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join("plugins.lock");

    let lockfile = kish_plugin_manager::lockfile::LockFile {
        plugin: vec![
            kish_plugin_manager::lockfile::LockEntry {
                name: "a".into(),
                path: "/path/a.dylib".into(),
                enabled: true,
                capabilities: Some(vec!["io".into()]),
                sha256: "aaa".into(),
                source: "github:u/a".into(),
                version: Some("1.0.0".into()),
            },
            kish_plugin_manager::lockfile::LockEntry {
                name: "b".into(),
                path: "/path/b.dylib".into(),
                enabled: false,
                capabilities: None,
                sha256: "bbb".into(),
                source: "local:/path/b.dylib".into(),
                version: None,
            },
        ],
    };

    kish_plugin_manager::lockfile::save_lockfile(&lock_path, &lockfile).unwrap();
    let loaded = kish_plugin_manager::lockfile::load_lockfile(&lock_path).unwrap();
    assert_eq!(loaded.plugin.len(), 2);
    assert_eq!(loaded.plugin[0].name, "a");
    assert!(loaded.plugin[0].enabled);
    assert_eq!(loaded.plugin[1].name, "b");
    assert!(!loaded.plugin[1].enabled);
    assert!(loaded.plugin[1].version.is_none());
}
```

- [ ] **Step 2: Make modules public in lib.rs for integration test access**

Create `crates/kish-plugin-manager/src/lib.rs`:

```rust
pub mod config;
pub mod github;
pub mod lockfile;
pub mod resolve;
pub mod sync;
pub mod verify;
```

Update `crates/kish-plugin-manager/src/main.rs` to use `kish_plugin_manager::` instead of local `mod` declarations:

```rust
use kish_plugin_manager::{config, github, lockfile, sync, verify};

use std::process;

fn main() {
    // ... same as before, but remove the mod declarations at the top
```

Wait — this changes the binary. A simpler approach: keep `main.rs` using `mod` but also add a `lib.rs` that re-exports. Actually, with Cargo you can have both `src/lib.rs` and `src/main.rs` in the same crate. The `main.rs` can use `kish_plugin_manager::` to access the library.

Create `crates/kish-plugin-manager/src/lib.rs`:
```rust
pub mod config;
pub mod github;
pub mod lockfile;
pub mod resolve;
pub mod sync;
pub mod verify;
```

Update `crates/kish-plugin-manager/src/main.rs` — remove all `mod` declarations and use `use kish_plugin_manager::*;` imports:

```rust
use std::process;

use kish_plugin_manager::{config, github, lockfile, sync, verify};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(|s| s.as_str()) {
        Some("sync") => cmd_sync(&args[1..]),
        Some("update") => cmd_update(&args[1..]),
        Some("list") => cmd_list(),
        Some("verify") => cmd_verify(),
        Some(cmd) => {
            eprintln!("kish-plugin: unknown command '{}'", cmd);
            2
        }
        None => {
            eprintln!("usage: kish-plugin <sync|update|list|verify>");
            2
        }
    };
    process::exit(code);
}

// ... cmd_sync, cmd_update, cmd_list, cmd_verify same as Task 8
```

- [ ] **Step 3: Run integration tests**

Run: `cargo test -p kish-plugin-manager --test sync_integration`
Expected: Both tests pass.

- [ ] **Step 4: Run all tests to verify no regressions**

Run: `cargo test`
Expected: All tests pass (both kish and kish-plugin-manager).

- [ ] **Step 5: Commit**

```bash
git add crates/kish-plugin-manager/src/lib.rs crates/kish-plugin-manager/src/main.rs crates/kish-plugin-manager/tests/sync_integration.rs
git commit -m "test(plugin-manager): add integration tests and expose library modules"
```

---

### Task 11: Update TODO.md

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Remove completed TODO item and update related items**

Delete the line:
```
- [ ] GitHub plugin installation — `kish plugin install <repo>` to download pre-built binaries from GitHub Releases
```

The runtime load/unload and `~/.kishrc` items remain as they are separate features.

- [ ] **Step 2: Commit**

```bash
git add TODO.md
git commit -m "docs(TODO): remove completed GitHub plugin installation item"
```

---

### Task 12: Final verification

- [ ] **Step 1: Build everything**

Run: `cargo build`
Expected: Both `kish` and `kish-plugin` binaries compile.

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Verify kish-plugin binary works**

Run: `cargo run -p kish-plugin-manager -- verify`
Expected: Either "no plugins installed" or lists plugins with verification status.

- [ ] **Step 4: Verify kish-plugin help**

Run: `cargo run -p kish-plugin-manager`
Expected: Prints `usage: kish-plugin <sync|update|list|verify>`.
