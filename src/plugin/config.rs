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
    pub capabilities: Option<Vec<String>>,
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

/// Parse a single capability string to its bitflag value.
pub fn capability_from_str(s: &str) -> Option<u32> {
    match s {
        "variables:read" => Some(kish_plugin_api::CAP_VARIABLES_READ),
        "variables:write" => Some(kish_plugin_api::CAP_VARIABLES_WRITE),
        "filesystem" => Some(kish_plugin_api::CAP_FILESYSTEM),
        "io" => Some(kish_plugin_api::CAP_IO),
        "hooks:pre_exec" => Some(kish_plugin_api::CAP_HOOK_PRE_EXEC),
        "hooks:post_exec" => Some(kish_plugin_api::CAP_HOOK_POST_EXEC),
        "hooks:on_cd" => Some(kish_plugin_api::CAP_HOOK_ON_CD),
        "hooks:pre_prompt" => Some(kish_plugin_api::CAP_HOOK_PRE_PROMPT),
        _ => None,
    }
}

/// Parse a list of capability strings into a combined bitflag.
/// Unknown strings are ignored.
pub fn capabilities_from_strs(strs: &[String]) -> u32 {
    strs.iter()
        .filter_map(|s| capability_from_str(s))
        .fold(0u32, |acc, f| acc | f)
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

    #[test]
    fn parse_capabilities_field() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "restricted"
path = "/usr/lib/librestricted.dylib"
capabilities = ["variables:read", "io", "hooks:pre_exec"]
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        let entry = &config.plugin[0];
        assert_eq!(
            entry.capabilities,
            Some(vec![
                "variables:read".to_string(),
                "io".to_string(),
                "hooks:pre_exec".to_string(),
            ])
        );
    }

    #[test]
    fn parse_missing_capabilities_is_none() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "trusted"
path = "/usr/lib/libtrusted.dylib"
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        assert!(config.plugin[0].capabilities.is_none());
    }

    #[test]
    fn parse_capability_string_to_bitflags() {
        use kish_plugin_api::*;
        assert_eq!(
            capability_from_str("variables:read"),
            Some(CAP_VARIABLES_READ)
        );
        assert_eq!(
            capability_from_str("variables:write"),
            Some(CAP_VARIABLES_WRITE)
        );
        assert_eq!(capability_from_str("filesystem"), Some(CAP_FILESYSTEM));
        assert_eq!(capability_from_str("io"), Some(CAP_IO));
        assert_eq!(
            capability_from_str("hooks:pre_exec"),
            Some(CAP_HOOK_PRE_EXEC)
        );
        assert_eq!(
            capability_from_str("hooks:post_exec"),
            Some(CAP_HOOK_POST_EXEC)
        );
        assert_eq!(capability_from_str("hooks:on_cd"), Some(CAP_HOOK_ON_CD));
        assert_eq!(
            capability_from_str("hooks:pre_prompt"),
            Some(CAP_HOOK_PRE_PROMPT)
        );
        assert_eq!(capability_from_str("unknown"), None);
    }

    #[test]
    fn parse_capabilities_to_bitflags() {
        use kish_plugin_api::*;
        let strs = vec![
            "variables:read".to_string(),
            "io".to_string(),
            "hooks:on_cd".to_string(),
        ];
        assert_eq!(
            capabilities_from_strs(&strs),
            CAP_VARIABLES_READ | CAP_IO | CAP_HOOK_ON_CD
        );
    }
}
