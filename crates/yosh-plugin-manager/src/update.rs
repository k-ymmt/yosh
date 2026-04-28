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
    _config_path: &Path,
    _name_filter: Option<&str>,
    _client: &GitHubClient,
) -> Result<UpdateOutcome, String> {
    unimplemented!("Task 5")
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
        .ok_or_else(|| format!("plugin '{}' not found in config", name))?;
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
}
