use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct PluginConfig {
    #[serde(default)]
    pub plugin: Vec<PluginEntry>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // schema fields parsed from plugins.toml; consumed by yosh-plugin sync
pub struct PluginEntry {
    pub name: String,
    pub path: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub capabilities: Option<Vec<String>>,
    /// SHA-256 of the on-disk `.wasm`, pinned by `yosh-plugin sync`.
    /// Required: an entry without it refuses to parse (and therefore
    /// to load) — integrity verification is unconditional and
    /// independent of the cwasm cache tuple below.
    pub sha256: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    /// Path to the precompiled cwasm cache file. `None` for entries that
    /// have not been through `yosh-plugin sync` yet — the host falls
    /// back to in-memory precompile in that case.
    #[serde(default)]
    pub cwasm_path: Option<std::path::PathBuf>,
    /// Wasmtime version that produced the cwasm at `cwasm_path`. Flat
    /// top-level field in the lockfile (see
    /// `yosh_plugin_manager::lockfile::LockEntry`). Combined with
    /// `target_triple`, `engine_config_hash`, and `sha256` to rebuild a
    /// `CacheKey` via [`PluginEntry::cache_key`].
    #[serde(default)]
    pub wasmtime_version: Option<String>,
    /// Target triple the cwasm was precompiled for. See `wasmtime_version`.
    #[serde(default)]
    pub target_triple: Option<String>,
    /// Hex-encoded wasmtime engine config hash captured at sync time.
    /// See `wasmtime_version`.
    #[serde(default)]
    pub engine_config_hash: Option<String>,
    /// Per-plugin allowlist of argv patterns that the `commands:exec`
    /// capability is restricted to. `None` or empty means no command is
    /// permitted; matching is OR across the list.
    #[serde(default)]
    pub allowed_commands: Option<Vec<String>>,
    /// Optional confinement root for the `files:read` / `files:write`
    /// capabilities. When set, every `files` host call is restricted to
    /// paths inside this directory (symlink-escape safe). When unset,
    /// `files` is a full-filesystem grant. Supports `~/` expansion.
    #[serde(default)]
    pub files_root: Option<String>,
    /// Per-plugin linear-memory cap in MiB (default 256, ceiling 4096).
    #[serde(default)]
    pub max_memory_mb: Option<u64>,
    /// Budget for pre_exec/post_exec/on_cd hooks in ms. 0 = unlimited.
    /// Default 5000.
    #[serde(default)]
    pub hook_timeout_ms: Option<u64>,
    /// Budget for plugin custom commands in ms. 0 = unlimited (default).
    #[serde(default)]
    pub command_timeout_ms: Option<u64>,
    /// Per-plugin pre_prompt budget in ms, range [1, 60000]. Overrides
    /// the `YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS` env var for this plugin.
    #[serde(default)]
    pub pre_prompt_timeout_ms: Option<u64>,
}

fn default_true() -> bool {
    true
}

impl PluginEntry {
    /// Reconstruct the four-tuple cwasm `CacheKey` from the flat lockfile
    /// fields written by `yosh-plugin sync`. Returns `None` when any of
    /// the three cwasm components is absent — in that case the host
    /// must fall back to an in-memory `Component::new`. This is solely
    /// the cwasm-trust gate; SHA-256 integrity is verified separately
    /// and unconditionally from the required `sha256` field.
    pub fn cache_key(&self) -> Option<crate::plugin::cache::CacheKey> {
        Some(crate::plugin::cache::CacheKey {
            wasm_sha256: self.sha256.clone(),
            wasmtime_version: self.wasmtime_version.clone()?,
            target_triple: self.target_triple.clone()?,
            engine_config_hash: self.engine_config_hash.clone()?,
        })
    }

    /// Bundle the four optional limit fields for `load_one`.
    pub fn limits_config(&self) -> crate::plugin::limits::LimitsConfig {
        crate::plugin::limits::LimitsConfig {
            max_memory_mb: self.max_memory_mb,
            hook_timeout_ms: self.hook_timeout_ms,
            command_timeout_ms: self.command_timeout_ms,
            pre_prompt_timeout_ms: self.pre_prompt_timeout_ms,
        }
    }
}

impl PluginConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        toml::from_str(&content).map_err(|e| format!("{}: {}", path.display(), e))
    }
}

/// Load the plugin config for shell startup. A missing file is the
/// normal no-plugins-installed case and returns `Ok(None)` (silent);
/// any other failure — unreadable file, corrupted TOML — is `Err` so
/// the caller can report it instead of silently loading zero plugins.
pub fn read_config_for_load(path: &Path) -> Result<Option<PluginConfig>, String> {
    if !path.exists() {
        return Ok(None);
    }
    PluginConfig::load(path).map(Some)
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

/// Parse a single capability string to its bitflag value.
pub fn capability_from_str(s: &str) -> Option<u32> {
    match s {
        "variables:read" => Some(yosh_plugin_api::CAP_VARIABLES_READ),
        "variables:write" => Some(yosh_plugin_api::CAP_VARIABLES_WRITE),
        "filesystem" => Some(yosh_plugin_api::CAP_FILESYSTEM),
        "io" => Some(yosh_plugin_api::CAP_IO),
        "hooks:pre_exec" => Some(yosh_plugin_api::CAP_HOOK_PRE_EXEC),
        "hooks:post_exec" => Some(yosh_plugin_api::CAP_HOOK_POST_EXEC),
        "hooks:on_cd" => Some(yosh_plugin_api::CAP_HOOK_ON_CD),
        "hooks:pre_prompt" => Some(yosh_plugin_api::CAP_HOOK_PRE_PROMPT),
        "files:read" => Some(yosh_plugin_api::CAP_FILES_READ),
        "files:write" => Some(yosh_plugin_api::CAP_FILES_WRITE),
        "commands:exec" => Some(yosh_plugin_api::CAP_COMMANDS_EXEC),
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
sha256 = "testsha"
enabled = true

[[plugin]]
name = "disabled"
path = "/usr/lib/libdisabled.dylib"
sha256 = "testsha"
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
sha256 = "testsha"
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

    /// Contract for `load_from_config` diagnostics: a missing lockfile
    /// is the normal no-plugins case (silent), but a corrupted or
    /// unreadable one must surface an error — otherwise every plugin
    /// vanishes with zero diagnostics.
    #[test]
    fn read_config_for_load_missing_file_is_silent_none() {
        let result = read_config_for_load(Path::new("/nonexistent/plugins.lock"));
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn read_config_for_load_corrupted_file_is_error() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "this is [ not valid toml").unwrap();
        let err = read_config_for_load(f.path()).unwrap_err();
        assert!(
            err.contains(&f.path().display().to_string()),
            "error must name the file: {}",
            err
        );
    }

    #[test]
    fn expand_tilde_with_home() {
        let result = expand_tilde("~/.yosh/plugins/lib.dylib");
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
sha256 = "testsha"
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
sha256 = "testsha"
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        assert!(config.plugin[0].capabilities.is_none());
    }

    #[test]
    fn parse_capability_string_to_bitflags() {
        use yosh_plugin_api::*;
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
    fn parse_files_capability_strings_to_bitflags() {
        use yosh_plugin_api::*;
        assert_eq!(capability_from_str("files:read"), Some(CAP_FILES_READ));
        assert_eq!(capability_from_str("files:write"), Some(CAP_FILES_WRITE));
    }

    #[test]
    fn parse_capabilities_to_bitflags() {
        use yosh_plugin_api::*;
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

    #[test]
    fn parse_allowed_commands_field() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "git-prompt"
path = "/tmp/git-prompt.wasm"
sha256 = "testsha"
capabilities = ["commands:exec"]
allowed_commands = ["git status:*", "git rev-parse:*"]
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        let entry = &config.plugin[0];
        assert_eq!(
            entry.allowed_commands,
            Some(vec![
                "git status:*".to_string(),
                "git rev-parse:*".to_string(),
            ])
        );
    }

    #[test]
    fn parse_files_root_field() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "notes"
path = "/tmp/notes.wasm"
sha256 = "testsha"
capabilities = ["files:read", "files:write"]
files_root = "~/notes"
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        assert_eq!(config.plugin[0].files_root.as_deref(), Some("~/notes"));
    }

    #[test]
    fn parse_missing_files_root_is_none() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "full-fs"
path = "/tmp/x.wasm"
sha256 = "testsha"
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        assert!(config.plugin[0].files_root.is_none());
    }

    #[test]
    fn parse_missing_allowed_commands_is_none() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "no-exec"
path = "/tmp/x.wasm"
sha256 = "testsha"
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        assert!(config.plugin[0].allowed_commands.is_none());
    }

    #[test]
    fn parse_commands_exec_capability_string_to_bitflag() {
        use yosh_plugin_api::CAP_COMMANDS_EXEC;
        assert_eq!(
            capability_from_str("commands:exec"),
            Some(CAP_COMMANDS_EXEC)
        );
    }

    /// Regression: `yosh-plugin sync` writes `wasmtime_version`,
    /// `target_triple`, `engine_config_hash`, and `sha256` as flat
    /// top-level fields on each `[[plugin]]` entry. The host must
    /// reconstruct a `CacheKey` from those flat fields so the cwasm
    /// trust path is taken instead of the in-memory fallback warning.
    #[test]
    fn parse_lockfile_flat_cwasm_fields_populate_cache_key() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "rich-prompt-plugin"
path = "~/.yosh/plugins/rich-prompt-plugin/rich_prompt_plugin.wasm"
enabled = true
sha256 = "96c55424ea8c0f87a7b33702022eb070d798f54b2daf7cf526448f6203eea550"
source = "github:k-ymmt/rich-prompt-plugin"
version = "0.1.2"
cwasm_path = "~/.yosh/plugins/rich-prompt-plugin/rich_prompt_plugin.cwasm"
wasmtime_version = "27"
target_triple = "aarch64-apple-darwin"
engine_config_hash = "deadbeefcafebabe0123456789abcdef0123456789abcdef0123456789abcdef"
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        let entry = &config.plugin[0];
        assert!(entry.cwasm_path.is_some(), "cwasm_path must deserialize");
        let key = entry
            .cache_key()
            .expect("flat cwasm fields must compose into a CacheKey");
        assert_eq!(
            key.wasm_sha256,
            "96c55424ea8c0f87a7b33702022eb070d798f54b2daf7cf526448f6203eea550"
        );
        assert_eq!(key.wasmtime_version, "27");
        assert_eq!(key.target_triple, "aarch64-apple-darwin");
        assert_eq!(
            key.engine_config_hash,
            "deadbeefcafebabe0123456789abcdef0123456789abcdef0123456789abcdef"
        );
    }

    /// Entries without the cwasm tuple (e.g. sync's local-plugin
    /// tolerance path when precompile failed) must still load without a
    /// `cache_key` — the host falls back to an in-memory compile — but
    /// the SHA-256 is still required and still verified: integrity is
    /// decoupled from the cwasm-trust gate.
    #[test]
    fn parse_lockfile_without_cwasm_fields_yields_no_cache_key() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "legacy"
path = "~/.yosh/plugins/legacy/legacy.wasm"
enabled = true
sha256 = "deadbeef"
source = "github:owner/legacy"
version = "0.1.0"
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        let entry = &config.plugin[0];
        assert!(entry.cwasm_path.is_none());
        assert!(
            entry.cache_key().is_none(),
            "no cwasm tuple → no cwasm-trust cache key"
        );
        assert_eq!(
            entry.sha256, "deadbeef",
            "sha256 is independent of the cache key and always available for verification"
        );
    }

    /// No backward compat (pre-release decision 2026-07-11): an entry
    /// without `sha256` is a parse error and refuses to load, instead
    /// of silently loading unverified.
    #[test]
    fn parse_lockfile_without_sha256_is_error() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "unverified"
path = "~/.yosh/plugins/unverified/unverified.wasm"
enabled = true
source = "github:owner/unverified"
version = "0.1.0"
"#
        )
        .unwrap();
        let result = PluginConfig::load(f.path());
        assert!(
            result.is_err(),
            "entry without sha256 must be a parse error"
        );
    }

    #[test]
    fn parse_limit_fields() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "limited"
path = "/tmp/x.wasm"
sha256 = "testsha"
max_memory_mb = 64
hook_timeout_ms = 1000
command_timeout_ms = 30000
pre_prompt_timeout_ms = 250
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        let lc = config.plugin[0].limits_config();
        assert_eq!(lc.max_memory_mb, Some(64));
        assert_eq!(lc.hook_timeout_ms, Some(1000));
        assert_eq!(lc.command_timeout_ms, Some(30000));
        assert_eq!(lc.pre_prompt_timeout_ms, Some(250));
    }

    #[test]
    fn parse_missing_limit_fields_default_to_none() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "plain"
path = "/tmp/x.wasm"
sha256 = "testsha"
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        assert_eq!(
            config.plugin[0].limits_config(),
            crate::plugin::limits::LimitsConfig::default()
        );
    }
}
