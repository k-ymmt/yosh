use std::path::Path;

use toml_edit::{DocumentMut, Item, Table, value};

use crate::config::PluginSource;
use crate::github::GitHubClient;

#[derive(Debug)]
pub struct InstallTarget {
    pub name: String,
    pub source: PluginSource,
    pub version: Option<String>,
}

const GITHUB_PREFIX: &str = "https://github.com/";

fn source_string(source: &PluginSource) -> String {
    match source {
        PluginSource::GitHub { owner, repo } => format!("github:{}/{}", owner, repo),
        PluginSource::Local { path } => format!("local:{}", path),
    }
}

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

pub fn parse_install_arg(arg: &str) -> Result<InstallTarget, String> {
    if let Some(rest) = arg.strip_prefix(GITHUB_PREFIX) {
        parse_github(rest)
    } else if arg.starts_with('/') || arg.starts_with("./") || arg.starts_with("../") {
        parse_local(arg)
    } else {
        Err(format!(
            "unrecognized install argument '{}': expected a GitHub URL (https://github.com/owner/repo) or a local path",
            arg
        ))
    }
}

/// Parse the portion of a GitHub URL after `https://github.com/`.
fn parse_github(rest: &str) -> Result<InstallTarget, String> {
    // Split off version at `@` — but only after the github.com/ prefix has been stripped.
    let (url_part, version) = if let Some(at_pos) = rest.find('@') {
        let v = rest[at_pos + 1..].to_string();
        if v.is_empty() {
            return Err(format!(
                "empty version after '@' in 'https://github.com/{}'",
                rest
            ));
        }
        (&rest[..at_pos], Some(v))
    } else {
        (rest, None)
    };

    // Strip trailing `/` and `.git` suffix.
    let url_part = url_part.trim_end_matches('/');
    let url_part = url_part.strip_suffix(".git").unwrap_or(url_part);
    // Strip again in case there was a trailing slash before `.git`
    let url_part = url_part.trim_end_matches('/');

    // Split into owner/repo — exactly two non-empty segments, no extra path components.
    let parts: Vec<&str> = url_part.splitn(3, '/').collect();
    if parts.len() < 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(format!(
            "invalid GitHub URL 'https://github.com/{}': expected 'https://github.com/owner/repo'",
            url_part
        ));
    }
    if parts.len() > 2 {
        return Err(format!(
            "invalid GitHub URL 'https://github.com/{}': unexpected path after repo name",
            url_part
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

/// Parse a local filesystem path (absolute or relative).
fn parse_local(arg: &str) -> Result<InstallTarget, String> {
    let path = Path::new(arg);
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("cannot resolve local path '{}': {}", arg, e))?;

    let name = canonical
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("cannot determine plugin name from path '{}'", canonical.display()))?
        .to_string();

    let path_str = canonical
        .to_str()
        .ok_or_else(|| format!("path '{}' contains non-UTF-8 characters", canonical.display()))?
        .to_string();

    Ok(InstallTarget {
        name,
        source: PluginSource::Local { path: path_str },
        version: None,
    })
}

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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn parse_github_empty_version_error() {
        let result = parse_install_arg("https://github.com/owner/repo@");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty version"));
    }

    #[test]
    fn parse_github_extra_path_segments_error() {
        let result = parse_install_arg("https://github.com/owner/repo/tree/main");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unexpected path"));
    }

    #[test]
    fn parse_local_absolute_path() {
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
        // canonicalize resolves symlinks (e.g. /var -> /private/var on macOS)
        let canonical_lib_path = lib_file.canonicalize().unwrap().to_string_lossy().to_string();

        install(&lib_path, false, &config_path, None).unwrap();

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("name = \"libtest\""));
        assert!(content.contains(&format!("source = \"local:{}\"", canonical_lib_path)));
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
}
