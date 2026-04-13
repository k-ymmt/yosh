use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct PluginConfig {
    #[serde(default)]
    pub plugin: Vec<PluginEntry>,
}

#[derive(Debug, Deserialize)]
pub struct PluginEntry {
    pub name: String,
    pub path: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl PluginConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("{}: {}", path.display(), e))?;
        toml::from_str(&content)
            .map_err(|e| format!("{}: {}", path.display(), e))
    }
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_valid_config() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "hello"
path = "/usr/lib/libhello.dylib"
enabled = true

[[plugin]]
name = "disabled"
path = "/usr/lib/libdisabled.dylib"
enabled = false
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        assert_eq!(config.plugin.len(), 2);
        assert_eq!(config.plugin[0].name, "hello");
        assert!(config.plugin[0].enabled);
        assert!(!config.plugin[1].enabled);
    }

    #[test]
    fn parse_empty_config() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "").unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        assert!(config.plugin.is_empty());
    }

    #[test]
    fn parse_missing_enabled_defaults_true() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "hello"
path = "/usr/lib/libhello.dylib"
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        assert!(config.plugin[0].enabled);
    }

    #[test]
    fn missing_config_file_returns_error() {
        let result = PluginConfig::load(Path::new("/nonexistent/plugins.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn expand_tilde_with_home() {
        let result = expand_tilde("~/.kish/plugins/lib.dylib");
        // Just check it doesn't start with ~ anymore (HOME varies by environment)
        assert!(!result.to_string_lossy().starts_with("~"));
    }

    #[test]
    fn expand_tilde_no_tilde() {
        let result = expand_tilde("/absolute/path/lib.dylib");
        assert_eq!(result, PathBuf::from("/absolute/path/lib.dylib"));
    }
}
