//! `yosh-plugin update`: structural TOML rewrite of `[[plugin]].version`
//! by plugin `name`, replacing the legacy `String::replacen` flow.
//!
//! See `docs/superpowers/specs/2026-04-28-plugin-update-toml-edit-design.md`.

use std::path::Path;

use toml_edit::DocumentMut;

use crate::config;
use crate::github::GitHubClient;

/// Result of trying to update a single plugin.
#[derive(Debug)]
pub enum UpdateStatus {
    /// Latest differs from current; manifest was rewritten in-memory.
    Updated { from: String, to: String },
    /// Current already matches latest; no rewrite.
    AlreadyLatest { current: String },
    /// Per-plugin GitHub or TOML helper error; loop continues.
    Failed(String),
    /// Plugin was not considered for update for one of the SkipReason variants.
    Skipped(SkipReason),
}

#[derive(Debug)]
pub enum SkipReason {
    /// `name_filter` was Some(X) and this plugin's name was not X.
    NotMatched,
    /// Plugin source is `local:`, not GitHub.
    LocalSource,
    /// Defensive: GitHub plugin has empty/missing `version` field.
    /// `config::load_config` rejects this case, so it should be unreachable
    /// in practice; kept so the loop can surface it cleanly if it ever fires.
    NoCurrentVersion,
}

#[derive(Debug)]
pub struct PluginUpdateResult {
    pub name: String,
    pub status: UpdateStatus,
}

#[derive(Debug)]
pub struct UpdateOutcome {
    pub results: Vec<PluginUpdateResult>,
    /// True iff at least one `UpdateStatus::Updated` was produced.
    /// `cmd_update` reads this to decide whether to invoke `cmd_sync(false)`.
    pub any_updated: bool,
}

/// Orchestration entry point. Reads `config_path`, fetches the latest
/// version of each GitHub plugin (filtered by `name_filter` if set),
/// rewrites matching `[[plugin]].version` fields in a single
/// `DocumentMut`, and writes the result back exactly once if anything
/// changed.
pub fn update(
    config_path: &Path,
    name_filter: Option<&str>,
    client: &GitHubClient,
) -> Result<UpdateOutcome, String> {
    let content = std::fs::read_to_string(config_path)
        .map_err(|e| format!("{}: {}", config_path.display(), e))?;
    let mut doc: DocumentMut = content
        .parse()
        .map_err(|e| format!("{}: {}", config_path.display(), e))?;

    let decls = config::load_config(config_path)?;

    let mut results = Vec::with_capacity(decls.len());
    let mut any_updated = false;

    for decl in &decls {
        if name_filter.is_some_and(|f| decl.name != f) {
            results.push(PluginUpdateResult {
                name: decl.name.clone(),
                status: UpdateStatus::Skipped(SkipReason::NotMatched),
            });
            continue;
        }

        let (owner, repo) = match &decl.source {
            config::PluginSource::GitHub { owner, repo } => (owner, repo),
            config::PluginSource::Local { .. } => {
                results.push(PluginUpdateResult {
                    name: decl.name.clone(),
                    status: UpdateStatus::Skipped(SkipReason::LocalSource),
                });
                continue;
            }
        };

        let current = match decl.version.as_deref() {
            Some(v) if !v.is_empty() => v.to_string(),
            _ => {
                results.push(PluginUpdateResult {
                    name: decl.name.clone(),
                    status: UpdateStatus::Skipped(SkipReason::NoCurrentVersion),
                });
                continue;
            }
        };

        let status = match client.latest_version(owner, repo) {
            Ok(latest) if latest == current => UpdateStatus::AlreadyLatest { current },
            Ok(latest) => match set_plugin_version(&mut doc, &decl.name, &latest) {
                Ok(()) => {
                    any_updated = true;
                    UpdateStatus::Updated { from: current, to: latest }
                }
                Err(e) => UpdateStatus::Failed(e),
            },
            Err(e) => UpdateStatus::Failed(e),
        };

        results.push(PluginUpdateResult {
            name: decl.name.clone(),
            status,
        });
    }

    if any_updated {
        std::fs::write(config_path, doc.to_string())
            .map_err(|e| format!("write {}: {}", config_path.display(), e))?;
    }

    Ok(UpdateOutcome { results, any_updated })
}

/// Pure TOML helper: locate the `[[plugin]]` table whose `name` equals
/// `name`, then set its `version` field to `new_version`. Returns `Err`
/// on missing/duplicate match or on structural anomalies in the
/// `plugin` key.
pub fn set_plugin_version(
    doc: &mut DocumentMut,
    name: &str,
    new_version: &str,
) -> Result<(), String> {
    let plugin_item = doc
        .get_mut("plugin")
        .ok_or_else(|| "config has no [[plugin]] array".to_string())?;
    let plugins = plugin_item
        .as_array_of_tables_mut()
        .ok_or_else(|| "config 'plugin' key is not an array of tables".to_string())?;

    let matches: Vec<usize> = plugins
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            if t.get("name").and_then(|v| v.as_str()) == Some(name) {
                Some(i)
            } else {
                None
            }
        })
        .collect();

    match matches.as_slice() {
        [] => Err(format!("plugin '{}' not found in config", name)),
        [idx] => {
            plugins
                .get_mut(*idx)
                .expect("index from filter_map is in-bounds")
                .insert("version", toml_edit::value(new_version));
            Ok(())
        }
        _ => Err(format!(
            "plugin '{}' appears multiple times in config",
            name
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::GitHubClientWithBase;

    #[test]
    fn set_version_basic_replaces_existing() {
        let toml = r#"[[plugin]]
name = "foo"
source = "github:owner/foo"
version = "1.0.0"
enabled = true
"#;
        let mut doc = toml.parse::<DocumentMut>().unwrap();
        set_plugin_version(&mut doc, "foo", "2.0.0").unwrap();
        let out = doc.to_string();
        assert!(out.contains(r#"version = "2.0.0""#), "out:\n{}", out);
        assert!(!out.contains(r#"version = "1.0.0""#), "out:\n{}", out);
    }

    #[test]
    fn set_version_same_version_siblings_no_collision() {
        let toml = r#"[[plugin]]
name = "alpha"
source = "github:owner/alpha"
version = "1.0.0"
enabled = true

[[plugin]]
name = "beta"
source = "github:owner/beta"
version = "1.0.0"
enabled = true
"#;
        let mut doc = toml.parse::<DocumentMut>().unwrap();
        set_plugin_version(&mut doc, "beta", "1.1.0").unwrap();
        let out = doc.to_string();

        let reparsed = out.parse::<DocumentMut>().unwrap();
        let plugins = reparsed["plugin"].as_array_of_tables().unwrap();
        assert_eq!(plugins.len(), 2);

        let alpha = plugins
            .iter()
            .find(|t| t.get("name").and_then(|v| v.as_str()) == Some("alpha"))
            .expect("alpha entry survives");
        let beta = plugins
            .iter()
            .find(|t| t.get("name").and_then(|v| v.as_str()) == Some("beta"))
            .expect("beta entry survives");

        assert_eq!(
            alpha.get("version").and_then(|v| v.as_str()),
            Some("1.0.0"),
            "sibling alpha was modified"
        );
        assert_eq!(
            beta.get("version").and_then(|v| v.as_str()),
            Some("1.1.0"),
            "target beta was not updated"
        );
    }

    #[test]
    fn set_version_preserves_comments_and_layout() {
        let toml = r#"# yosh plugin manifest
# managed by yosh-plugin

[[plugin]]
name = "foo"
source = "github:owner/foo"
version = "1.0.0"
enabled = true
"#;
        let mut doc = toml.parse::<DocumentMut>().unwrap();
        set_plugin_version(&mut doc, "foo", "1.1.0").unwrap();
        let out = doc.to_string();
        assert!(out.contains("# yosh plugin manifest"), "out:\n{}", out);
        assert!(out.contains("# managed by yosh-plugin"), "out:\n{}", out);
        assert!(out.contains(r#"version = "1.1.0""#), "out:\n{}", out);
    }

    #[test]
    fn set_version_inserts_when_missing() {
        let toml = r#"[[plugin]]
name = "foo"
source = "github:owner/foo"
enabled = true
"#;
        let mut doc = toml.parse::<DocumentMut>().unwrap();
        set_plugin_version(&mut doc, "foo", "1.0.0").unwrap();
        let out = doc.to_string();
        assert!(out.contains(r#"version = "1.0.0""#), "out:\n{}", out);
    }

    #[test]
    fn set_version_unknown_name_errors() {
        let toml = r#"[[plugin]]
name = "foo"
source = "github:owner/foo"
version = "1.0.0"
"#;
        let mut doc = toml.parse::<DocumentMut>().unwrap();
        let err = set_plugin_version(&mut doc, "nonexistent", "2.0.0").unwrap_err();
        assert!(err.contains("nonexistent"), "err: {}", err);
        assert!(err.contains("not found"), "err: {}", err);
    }

    #[test]
    fn set_version_no_plugin_array_errors() {
        let toml = "# empty config\n";
        let mut doc = toml.parse::<DocumentMut>().unwrap();
        let err = set_plugin_version(&mut doc, "foo", "1.0.0").unwrap_err();
        assert!(err.contains("no [[plugin]] array"), "err: {}", err);
    }

    #[test]
    fn set_version_plugin_key_wrong_type_errors() {
        let toml = "plugin = \"not-an-array\"\n";
        let mut doc = toml.parse::<DocumentMut>().unwrap();
        let err = set_plugin_version(&mut doc, "foo", "1.0.0").unwrap_err();
        assert!(err.contains("array of tables"), "err: {}", err);
    }

    #[test]
    fn set_version_duplicate_name_errors() {
        let toml = r#"[[plugin]]
name = "foo"
source = "github:owner/foo"
version = "1.0.0"

[[plugin]]
name = "foo"
source = "github:other/foo"
version = "2.0.0"
"#;
        let mut doc = toml.parse::<DocumentMut>().unwrap();
        let err = set_plugin_version(&mut doc, "foo", "3.0.0").unwrap_err();
        assert!(err.contains("multiple"), "err: {}", err);
    }

    #[test]
    fn update_skips_local_sources() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("plugins.toml");
        // Stage a local plugin file so config::load_config doesn't trip on the path.
        let plugin_file = dir.path().join("local.wasm");
        std::fs::write(&plugin_file, b"\0asm\x01\0\0\0").unwrap();
        std::fs::write(
            &config_path,
            format!(
                r#"[[plugin]]
name = "local-only"
source = "local:{}"
"#,
                plugin_file.display()
            ),
        )
        .unwrap();

        // Point at an unreachable base; if update tries to call out, the
        // test would either hang or fail. LocalSource skip should bypass.
        let client = GitHubClientWithBase::new("http://127.0.0.1:1").into_client();
        let outcome = update(&config_path, None, &client).unwrap();

        assert_eq!(outcome.results.len(), 1);
        assert!(matches!(
            outcome.results[0].status,
            UpdateStatus::Skipped(SkipReason::LocalSource)
        ));
        assert!(!outcome.any_updated);
    }
}
