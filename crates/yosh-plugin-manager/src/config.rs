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
            return Err(format!(
                "invalid github source '{}': expected 'github:owner/repo'",
                s
            ));
        }
        Ok(PluginSource::GitHub {
            owner: parts[0].to_string(),
            repo: parts[1].to_string(),
        })
    } else if let Some(rest) = s.strip_prefix("local:") {
        if rest.is_empty() {
            return Err(format!("invalid local source '{}': path is empty", s));
        }
        Ok(PluginSource::Local {
            path: rest.to_string(),
        })
    } else {
        Err(format!(
            "unknown source type '{}': expected 'github:' or 'local:' prefix",
            s
        ))
    }
}

fn validate_plugin_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("plugin name is empty".to_string());
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(format!(
            "plugin '{}': name must not contain '/', '\\', or '..'",
            name
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "plugin '{}': name must contain only alphanumeric characters, hyphens, or underscores",
            name
        ));
    }
    Ok(())
}

pub fn load_config(path: &Path) -> Result<Vec<PluginDecl>, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let raw: RawConfig =
        toml::from_str(&content).map_err(|e| format!("{}: {}", path.display(), e))?;
    raw.plugin
        .into_iter()
        .map(|entry| {
            validate_plugin_name(&entry.name)?;
            let source = parse_source(&entry.source)?;
            if matches!(source, PluginSource::GitHub { .. }) && entry.version.is_none() {
                return Err(format!(
                    "plugin '{}': github source requires 'version' field",
                    entry.name
                ));
            }
            // Reject pre-v0.2.0 asset templates with {os}/{arch}/{ext}
            // tokens; plugins now ship as single .wasm files.
            if let Some(t) = &entry.asset {
                crate::resolve::check_asset_template(t)
                    .map_err(|e| format!("plugin '{}': {}", entry.name, e))?;
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

pub fn expand_tilde_path(path: &str) -> std::path::PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return std::path::PathBuf::from(home).join(rest);
        }
    }
    std::path::PathBuf::from(path)
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
            PluginSource::GitHub {
                owner: "user".into(),
                repo: "repo".into()
            }
        );
    }

    #[test]
    fn parse_local_source() {
        let src = parse_source("local:~/.yosh/plugins/lib.dylib").unwrap();
        assert_eq!(
            src,
            PluginSource::Local {
                path: "~/.yosh/plugins/lib.dylib".into()
            }
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
        write!(
            f,
            r#"
[[plugin]]
name = "git-status"
source = "github:user/kish-plugin-git-status"
version = "1.2.3"
capabilities = ["variables:read", "io"]

[[plugin]]
name = "local-tool"
source = "local:~/.yosh/plugins/liblocal.dylib"
capabilities = ["io"]
"#
        )
        .unwrap();
        let decls = load_config(f.path()).unwrap();
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].name, "git-status");
        assert!(
            matches!(&decls[0].source, PluginSource::GitHub { owner, repo } if owner == "user" && repo == "kish-plugin-git-status")
        );
        assert_eq!(decls[0].version.as_deref(), Some("1.2.3"));
        assert_eq!(decls[1].name, "local-tool");
        assert!(matches!(&decls[1].source, PluginSource::Local { .. }));
        assert!(decls[1].version.is_none());
    }

    #[test]
    fn load_config_enabled_defaults_true() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "p"
source = "local:/tmp/lib.dylib"
"#
        )
        .unwrap();
        let decls = load_config(f.path()).unwrap();
        assert!(decls[0].enabled);
    }

    #[test]
    fn load_config_disabled_plugin() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "p"
source = "local:/tmp/lib.dylib"
enabled = false
"#
        )
        .unwrap();
        let decls = load_config(f.path()).unwrap();
        assert!(!decls[0].enabled);
    }

    #[test]
    fn load_config_with_asset_template() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "custom"
source = "github:user/repo"
version = "1.0.0"
asset = "myplugin-{{name}}.wasm"
"#
        )
        .unwrap();
        let decls = load_config(f.path()).unwrap();
        assert_eq!(decls[0].asset.as_deref(), Some("myplugin-{name}.wasm"));
    }

    #[test]
    fn load_config_rejects_legacy_asset_template() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "old"
source = "github:user/repo"
version = "1.0.0"
asset = "lib{{name}}-{{os}}-{{arch}}.{{ext}}"
"#
        )
        .unwrap();
        let err = load_config(f.path()).unwrap_err();
        assert!(err.contains("v0.2.0"), "expected migration message: {}", err);
    }

    #[test]
    fn github_source_without_version_is_error() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "bad"
source = "github:user/repo"
"#
        )
        .unwrap();
        assert!(load_config(f.path()).is_err());
    }

    #[test]
    fn reject_path_traversal_in_name() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "../../../etc"
source = "local:/tmp/lib.dylib"
"#
        )
        .unwrap();
        assert!(load_config(f.path()).is_err());
    }

    #[test]
    fn reject_slash_in_name() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "foo/bar"
source = "local:/tmp/lib.dylib"
"#
        )
        .unwrap();
        assert!(load_config(f.path()).is_err());
    }

    #[test]
    fn reject_empty_name() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = ""
source = "local:/tmp/lib.dylib"
"#
        )
        .unwrap();
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
