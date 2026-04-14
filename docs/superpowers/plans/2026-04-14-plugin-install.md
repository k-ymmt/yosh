# Plugin Install Command Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `kish plugin install <SOURCE>[@VERSION] [--force]` subcommand that registers plugins in `plugins.toml` without manual file editing.

**Architecture:** A new `install.rs` module in `kish-plugin-manager` handles argument parsing (GitHub URL vs local path), duplicate checking, optional latest-version resolution via GitHub API, and format-preserving TOML writing via `toml_edit`. The CLI entry point in `main.rs` gains an `Install` variant.

**Tech Stack:** Rust, clap (CLI), toml_edit (format-preserving TOML writes), ureq (HTTP for GitHub API)

---

### Task 1: Add `toml_edit` dependency

**Files:**
- Modify: `crates/kish-plugin-manager/Cargo.toml`

- [ ] **Step 1: Add `toml_edit` to `[dependencies]`**

In `crates/kish-plugin-manager/Cargo.toml`, add `toml_edit` under `[dependencies]`:

```toml
toml_edit = "0.22"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p kish-plugin-manager`
Expected: compiles without errors

- [ ] **Step 3: Commit**

```bash
git add crates/kish-plugin-manager/Cargo.toml Cargo.lock
git commit -m "build(plugin-manager): add toml_edit dependency for format-preserving TOML writes"
```

---

### Task 2: Implement install argument parsing

**Files:**
- Create: `crates/kish-plugin-manager/src/install.rs`
- Test: `crates/kish-plugin-manager/src/install.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write failing tests for `parse_install_arg`**

Create `crates/kish-plugin-manager/src/install.rs` with:

```rust
use std::path::{Path, PathBuf};

use crate::config::PluginSource;

/// Parsed result from the install argument.
pub struct InstallTarget {
    pub name: String,
    pub source: PluginSource,
    pub version: Option<String>,
}

/// Parse a raw CLI argument into an InstallTarget.
/// Accepts:
///   - `https://github.com/owner/repo` → GitHub source, version = None (resolve latest)
///   - `https://github.com/owner/repo@1.0.0` → GitHub source, version = Some("1.0.0")
///   - `/path/to/lib.dylib` or `./relative` → Local source (canonicalized)
pub fn parse_install_arg(arg: &str) -> Result<InstallTarget, String> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_github_url_no_version() {
        let t = parse_install_arg("https://github.com/example/kish-plugin-foo").unwrap();
        assert_eq!(t.name, "kish-plugin-foo");
        assert_eq!(
            t.source,
            PluginSource::GitHub {
                owner: "example".into(),
                repo: "kish-plugin-foo".into()
            }
        );
        assert_eq!(t.version, None);
    }

    #[test]
    fn parse_github_url_with_version() {
        let t = parse_install_arg("https://github.com/example/plugin@1.0.0").unwrap();
        assert_eq!(t.name, "plugin");
        assert_eq!(
            t.source,
            PluginSource::GitHub {
                owner: "example".into(),
                repo: "plugin".into()
            }
        );
        assert_eq!(t.version, Some("1.0.0".into()));
    }

    #[test]
    fn parse_github_url_trailing_slash_stripped() {
        let t = parse_install_arg("https://github.com/owner/repo/").unwrap();
        assert_eq!(t.name, "repo");
        assert_eq!(
            t.source,
            PluginSource::GitHub {
                owner: "owner".into(),
                repo: "repo".into()
            }
        );
    }

    #[test]
    fn parse_github_url_with_dot_git_suffix() {
        let t = parse_install_arg("https://github.com/owner/repo.git").unwrap();
        assert_eq!(t.name, "repo");
        assert_eq!(
            t.source,
            PluginSource::GitHub {
                owner: "owner".into(),
                repo: "repo".into()
            }
        );
    }

    #[test]
    fn parse_github_invalid_url_missing_repo() {
        let result = parse_install_arg("https://github.com/owneronly");
        assert!(result.is_err());
    }

    #[test]
    fn parse_github_invalid_url_empty_repo() {
        let result = parse_install_arg("https://github.com/owner/");
        assert!(result.is_err());
    }

    #[test]
    fn parse_local_absolute_path() {
        // Use a path that actually exists for canonicalization
        let t = parse_install_arg("/tmp").unwrap();
        assert_eq!(t.name, "tmp");
        assert!(matches!(t.source, PluginSource::Local { .. }));
        assert_eq!(t.version, None);
    }

    #[test]
    fn parse_local_nonexistent_path_error() {
        let result = parse_install_arg("/nonexistent/path/to/lib.dylib");
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kish-plugin-manager parse_install_arg`
Expected: all tests FAIL with "not yet implemented"

- [ ] **Step 3: Implement `parse_install_arg`**

Replace the `todo!()` in `parse_install_arg` with:

```rust
pub fn parse_install_arg(arg: &str) -> Result<InstallTarget, String> {
    if arg.starts_with("https://github.com/") {
        parse_github_url(arg)
    } else {
        parse_local_path(arg)
    }
}

fn parse_github_url(url: &str) -> Result<InstallTarget, String> {
    // Split off @version if present
    let (url_part, version) = match url.rfind('@') {
        Some(pos) if pos > "https://github.com/".len() => {
            let ver = &url[pos + 1..];
            if ver.is_empty() {
                return Err(format!("empty version after '@' in '{}'", url));
            }
            (&url[..pos], Some(ver.to_string()))
        }
        _ => (url, None),
    };

    // Strip "https://github.com/" prefix
    let path = url_part
        .strip_prefix("https://github.com/")
        .unwrap()
        .trim_end_matches('/')
        .trim_end_matches(".git");

    let parts: Vec<&str> = path.splitn(2, '/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(format!(
            "invalid GitHub URL '{}': expected https://github.com/owner/repo",
            url
        ));
    }

    let owner = parts[0].to_string();
    let repo = parts[1].to_string();
    let name = repo.clone();

    Ok(InstallTarget {
        name,
        source: PluginSource::GitHub { owner, repo },
        version,
    })
}

fn parse_local_path(arg: &str) -> Result<InstallTarget, String> {
    let path = Path::new(arg);
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("'{}': {}", arg, e))?;

    let name = canonical
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("cannot determine plugin name from '{}'", arg))?
        .to_string();

    Ok(InstallTarget {
        name,
        source: PluginSource::Local {
            path: canonical.to_string_lossy().to_string(),
        },
        version: None,
    })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kish-plugin-manager parse_install_arg`
Expected: all tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/kish-plugin-manager/src/install.rs
git commit -m "feat(plugin-manager): add install argument parsing with GitHub URL and local path support"
```

---

### Task 3: Implement `plugins.toml` writing with `toml_edit`

**Files:**
- Modify: `crates/kish-plugin-manager/src/install.rs`

- [ ] **Step 1: Write failing tests for TOML writing**

Add to the `tests` module in `install.rs`:

```rust
    use std::io::Write as IoWrite;

    #[test]
    fn write_github_entry_to_empty_file() {
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(f.path(), "").unwrap();
        let target = InstallTarget {
            name: "foo".into(),
            source: PluginSource::GitHub {
                owner: "example".into(),
                repo: "foo".into(),
            },
            version: Some("1.0.0".into()),
        };
        write_plugin_entry(f.path(), &target, false).unwrap();
        let content = std::fs::read_to_string(f.path()).unwrap();
        assert!(content.contains("name = \"foo\""));
        assert!(content.contains("source = \"github:example/foo\""));
        assert!(content.contains("version = \"1.0.0\""));
        assert!(content.contains("enabled = true"));
    }

    #[test]
    fn write_local_entry_appends() {
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            f.path(),
            "[[plugin]]\nname = \"existing\"\nsource = \"local:/tmp/lib.dylib\"\nenabled = true\n",
        )
        .unwrap();
        let target = InstallTarget {
            name: "new-plugin".into(),
            source: PluginSource::Local {
                path: "/usr/lib/new.dylib".into(),
            },
            version: None,
        };
        write_plugin_entry(f.path(), &target, false).unwrap();
        let content = std::fs::read_to_string(f.path()).unwrap();
        assert!(content.contains("name = \"existing\""));
        assert!(content.contains("name = \"new-plugin\""));
        assert!(content.contains("source = \"local:/usr/lib/new.dylib\""));
        assert!(!content.contains("version"));
    }

    #[test]
    fn write_duplicate_without_force_errors() {
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            f.path(),
            "[[plugin]]\nname = \"foo\"\nsource = \"local:/tmp/lib.dylib\"\nenabled = true\n",
        )
        .unwrap();
        let target = InstallTarget {
            name: "foo".into(),
            source: PluginSource::Local {
                path: "/tmp/new.dylib".into(),
            },
            version: None,
        };
        let result = write_plugin_entry(f.path(), &target, false);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already installed"));
    }

    #[test]
    fn write_duplicate_with_force_replaces() {
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            f.path(),
            "[[plugin]]\nname = \"foo\"\nsource = \"local:/tmp/old.dylib\"\nenabled = true\n",
        )
        .unwrap();
        let target = InstallTarget {
            name: "foo".into(),
            source: PluginSource::GitHub {
                owner: "example".into(),
                repo: "foo".into(),
            },
            version: Some("2.0.0".into()),
        };
        write_plugin_entry(f.path(), &target, true).unwrap();
        let content = std::fs::read_to_string(f.path()).unwrap();
        // Old entry should be replaced
        assert!(!content.contains("local:/tmp/old.dylib"));
        assert!(content.contains("github:example/foo"));
        assert!(content.contains("version = \"2.0.0\""));
    }

    #[test]
    fn write_preserves_comments() {
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            f.path(),
            "# My plugins config\n\n[[plugin]]\nname = \"bar\"\nsource = \"local:/tmp/bar.dylib\"\nenabled = true\n",
        )
        .unwrap();
        let target = InstallTarget {
            name: "baz".into(),
            source: PluginSource::Local {
                path: "/tmp/baz.dylib".into(),
            },
            version: None,
        };
        write_plugin_entry(f.path(), &target, false).unwrap();
        let content = std::fs::read_to_string(f.path()).unwrap();
        assert!(content.contains("# My plugins config"));
        assert!(content.contains("name = \"bar\""));
        assert!(content.contains("name = \"baz\""));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kish-plugin-manager write_ -- --test-threads=1`
Expected: FAIL — `write_plugin_entry` not found

- [ ] **Step 3: Implement `write_plugin_entry`**

Add to `install.rs`, above the `#[cfg(test)]` block:

```rust
use toml_edit::{DocumentMut, Item, Table, value};

/// Source string for plugins.toml (e.g., "github:owner/repo" or "local:/path")
fn source_string(source: &PluginSource) -> String {
    match source {
        PluginSource::GitHub { owner, repo } => format!("github:{}/{}", owner, repo),
        PluginSource::Local { path } => format!("local:{}", path),
    }
}

/// Write a plugin entry to plugins.toml, preserving existing formatting.
/// If `force` is true, an existing entry with the same name is replaced.
pub fn write_plugin_entry(
    config_path: &Path,
    target: &InstallTarget,
    force: bool,
) -> Result<(), String> {
    let content = std::fs::read_to_string(config_path)
        .unwrap_or_default();

    let mut doc: DocumentMut = content
        .parse()
        .map_err(|e| format!("failed to parse {}: {}", config_path.display(), e))?;

    // Ensure [[plugin]] array of tables exists
    if !doc.contains_key("plugin") {
        doc["plugin"] = Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }

    let plugins = doc["plugin"]
        .as_array_of_tables_mut()
        .ok_or_else(|| "'plugin' key is not an array of tables".to_string())?;

    // Check for duplicates
    let existing_idx = plugins
        .iter()
        .position(|t| t.get("name").and_then(|v| v.as_str()) == Some(&target.name));

    if let Some(idx) = existing_idx {
        if !force {
            return Err(format!(
                "plugin '{}' is already installed. Use --force to overwrite.",
                target.name
            ));
        }
        plugins.remove(idx);
    }

    // Build new entry
    let mut entry = Table::new();
    entry.insert("name", value(&target.name));
    entry.insert("source", value(source_string(&target.source)));
    if let Some(ver) = &target.version {
        entry.insert("version", value(ver.as_str()));
    }
    entry.insert("enabled", value(true));

    plugins.push(entry);

    std::fs::write(config_path, doc.to_string())
        .map_err(|e| format!("failed to write {}: {}", config_path.display(), e))?;

    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kish-plugin-manager write_ -- --test-threads=1`
Expected: all tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/kish-plugin-manager/src/install.rs
git commit -m "feat(plugin-manager): implement format-preserving TOML writing for plugin install"
```

---

### Task 4: Implement the `install` function and wire up CLI

**Files:**
- Modify: `crates/kish-plugin-manager/src/install.rs`
- Modify: `crates/kish-plugin-manager/src/main.rs`
- Modify: `crates/kish-plugin-manager/src/lib.rs`

- [ ] **Step 1: Write failing test for `install` orchestration**

Add to the `tests` module in `install.rs`:

```rust
    #[test]
    fn install_github_with_explicit_version() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("plugins.toml");
        std::fs::write(&config_path, "").unwrap();

        install(
            "https://github.com/example/my-plugin@1.0.0",
            false,
            &config_path,
            None, // skip GitHub API when version is explicit
        )
        .unwrap();

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("name = \"my-plugin\""));
        assert!(content.contains("source = \"github:example/my-plugin\""));
        assert!(content.contains("version = \"1.0.0\""));
        assert!(content.contains("enabled = true"));
    }

    #[test]
    fn install_local_path() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("plugins.toml");
        std::fs::write(&config_path, "").unwrap();

        // Create a temp file to act as the local plugin binary
        let lib_file = dir.path().join("libtest.dylib");
        std::fs::write(&lib_file, b"fake").unwrap();
        let lib_path = lib_file.to_string_lossy().to_string();

        install(&lib_path, false, &config_path, None).unwrap();

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("name = \"libtest\""));
        assert!(content.contains(&format!("source = \"local:{}\"", lib_path)));
        assert!(!content.contains("version"));
    }

    #[test]
    fn install_duplicate_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("plugins.toml");
        std::fs::write(
            &config_path,
            "[[plugin]]\nname = \"my-plugin\"\nsource = \"local:/tmp/x.dylib\"\nenabled = true\n",
        )
        .unwrap();

        let result = install(
            "https://github.com/example/my-plugin@1.0.0",
            false,
            &config_path,
            None,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already installed"));
    }

    #[test]
    fn install_duplicate_with_force() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("plugins.toml");
        std::fs::write(
            &config_path,
            "[[plugin]]\nname = \"my-plugin\"\nsource = \"local:/tmp/old.dylib\"\nenabled = true\n",
        )
        .unwrap();

        install(
            "https://github.com/example/my-plugin@2.0.0",
            true,
            &config_path,
            None,
        )
        .unwrap();

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(!content.contains("local:/tmp/old.dylib"));
        assert!(content.contains("github:example/my-plugin"));
        assert!(content.contains("version = \"2.0.0\""));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kish-plugin-manager install_ -- --test-threads=1`
Expected: FAIL — `install` function not found

- [ ] **Step 3: Implement `install` function**

Add to `install.rs`, above `#[cfg(test)]`:

```rust
use crate::github::GitHubClient;
use crate::sync;

/// Main install entry point.
/// `github_client` is optional — if None and a GitHub latest version is needed, a default client is created.
pub fn install(
    arg: &str,
    force: bool,
    config_path: &Path,
    github_client: Option<&GitHubClient>,
) -> Result<String, String> {
    let mut target = parse_install_arg(arg)?;

    // Resolve latest version for GitHub sources when not specified
    if matches!(&target.source, PluginSource::GitHub { .. }) && target.version.is_none() {
        let default_client;
        let client = match github_client {
            Some(c) => c,
            None => {
                default_client = GitHubClient::new();
                &default_client
            }
        };
        if let PluginSource::GitHub { owner, repo } = &target.source {
            let version = client.latest_version(owner, repo)?;
            target.version = Some(version);
        }
    }

    // Ensure config file exists
    if !config_path.exists() {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {}", parent.display(), e))?;
        }
        std::fs::write(config_path, "")
            .map_err(|e| format!("failed to create {}: {}", config_path.display(), e))?;
    }

    write_plugin_entry(config_path, &target, force)?;

    // Build result message
    let source_str = source_string(&target.source);
    let msg = match &target.version {
        Some(v) => format!("Installed plugin '{}' ({}@{})", target.name, source_str, v),
        None => format!("Installed plugin '{}' ({})", target.name, source_str),
    };

    Ok(msg)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kish-plugin-manager install_ -- --test-threads=1`
Expected: all tests PASS

- [ ] **Step 5: Export `install` module in `lib.rs`**

Add to `crates/kish-plugin-manager/src/lib.rs`:

```rust
pub mod install;
```

- [ ] **Step 6: Add `Install` variant to CLI in `main.rs`**

In `crates/kish-plugin-manager/src/main.rs`, add to the `Commands` enum:

```rust
    /// Add a plugin from a GitHub URL or local path to plugins.toml
    Install {
        /// GitHub URL (https://github.com/owner/repo[@version]) or local file path
        source: String,
        /// Overwrite existing plugin with the same name
        #[arg(long)]
        force: bool,
    },
```

Add to the `use` statement at the top:

```rust
use kish_plugin_manager::{config, github, install, lockfile, sync, verify};
```

Add to the `match` in `main()`:

```rust
        Commands::Install { source, force } => cmd_install(&source, force),
```

Add the `cmd_install` function:

```rust
fn cmd_install(source: &str, force: bool) -> i32 {
    let config_path = sync::config_path();
    match install::install(source, force, &config_path, None) {
        Ok(msg) => {
            eprintln!("{}", msg);
            if source.starts_with("https://github.com/") {
                eprintln!("Run 'kish plugin sync' to download.");
            }
            0
        }
        Err(e) => {
            eprintln!("kish-plugin: {}", e);
            1
        }
    }
}
```

- [ ] **Step 7: Verify full build**

Run: `cargo build -p kish-plugin-manager`
Expected: compiles without errors

- [ ] **Step 8: Run all plugin-manager tests**

Run: `cargo test -p kish-plugin-manager`
Expected: all tests PASS

- [ ] **Step 9: Commit**

```bash
git add crates/kish-plugin-manager/src/install.rs crates/kish-plugin-manager/src/main.rs crates/kish-plugin-manager/src/lib.rs
git commit -m "feat(plugin-manager): add install subcommand for registering plugins in plugins.toml"
```

---

### Task 5: Manual smoke test

- [ ] **Step 1: Test help output**

Run: `cargo run -p kish-plugin-manager -- install --help`
Expected: shows usage with `<SOURCE>` argument and `--force` flag

- [ ] **Step 2: Test local path install**

```bash
touch /tmp/test-plugin.dylib
cargo run -p kish-plugin-manager -- install /tmp/test-plugin.dylib
```

Expected output: `Installed plugin 'test-plugin' (local:/tmp/test-plugin.dylib)`
Verify: `cat ~/.config/kish/plugins.toml` shows the new entry

- [ ] **Step 3: Test duplicate error**

```bash
cargo run -p kish-plugin-manager -- install /tmp/test-plugin.dylib
```

Expected: error message containing "already installed"

- [ ] **Step 4: Test --force overwrite**

```bash
cargo run -p kish-plugin-manager -- install /tmp/test-plugin.dylib --force
```

Expected: `Installed plugin 'test-plugin' (local:/tmp/test-plugin.dylib)` — succeeds

- [ ] **Step 5: Clean up test entries**

Remove the test entries from `~/.config/kish/plugins.toml` and delete `/tmp/test-plugin.dylib`.

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: all tests pass, no regressions

- [ ] **Step 7: Final commit if any fixes were needed**

If any fixes were made during smoke testing, commit them:
```bash
git add -A
git commit -m "fix(plugin-manager): address issues found during install smoke testing"
```
