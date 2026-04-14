use std::path::Path;

use crate::config::PluginSource;

pub struct InstallTarget {
    pub name: String,
    pub source: PluginSource,
    pub version: Option<String>,
}

const GITHUB_PREFIX: &str = "https://github.com/";

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
        (&rest[..at_pos], Some(v))
    } else {
        (rest, None)
    };

    // Strip trailing `/` and `.git` suffix.
    let url_part = url_part.trim_end_matches('/');
    let url_part = url_part.strip_suffix(".git").unwrap_or(url_part);
    // Strip again in case there was a trailing slash before `.git`
    let url_part = url_part.trim_end_matches('/');

    // Split into owner/repo — exactly two non-empty segments.
    let parts: Vec<&str> = url_part.splitn(2, '/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(format!(
            "invalid GitHub URL 'https://github.com/{}': expected 'https://github.com/owner/repo'",
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
