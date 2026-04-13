use std::fs;
use std::io::{Read, Write};
use std::path::Path;

/// GitHub API client for fetching release information and downloading assets.
pub struct GitHubClient {
    base_url: String,
    token: Option<String>,
}

impl GitHubClient {
    /// Create a new client, reading auth token from `KISH_GITHUB_TOKEN` or `GITHUB_TOKEN`.
    pub fn new() -> Self {
        let token = std::env::var("KISH_GITHUB_TOKEN")
            .ok()
            .or_else(|| std::env::var("GITHUB_TOKEN").ok());
        Self {
            base_url: "https://api.github.com".to_string(),
            token,
        }
    }

    fn get_json(&self, url: &str) -> Result<serde_json::Value, String> {
        let mut req = ureq::get(url)
            .header("User-Agent", "kish-plugin-manager")
            .header("Accept", "application/vnd.github.v3+json");
        if let Some(token) = &self.token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }
        let body = req
            .call()
            .map_err(|e| format!("request failed: {}", e))?
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("failed to read body: {}", e))?;
        serde_json::from_str(&body).map_err(|e| format!("failed to parse JSON: {}", e))
    }

    fn release_json(&self, owner: &str, repo: &str, tag: &str) -> Result<serde_json::Value, String> {
        let url = format!("{}/repos/{}/{}/releases/tags/{}", self.base_url, owner, repo, tag);
        self.get_json(&url)
    }

    /// Look up a GitHub release by tag, trying `v{version}` first then `{version}`.
    /// Returns the download URL of the named asset.
    pub fn find_asset_url(
        &self,
        owner: &str,
        repo: &str,
        version: &str,
        asset_name: &str,
    ) -> Result<String, String> {
        let v_tag = format!("v{}", version);
        let release = match self.release_json(owner, repo, &v_tag) {
            Ok(r) => r,
            Err(_) => {
                // Fallback to bare version tag
                self.release_json(owner, repo, version)
                    .map_err(|e| format!("release not found for {} or {}: {}", v_tag, version, e))?
            }
        };

        let assets = release["assets"]
            .as_array()
            .ok_or_else(|| "release has no assets array".to_string())?;

        for asset in assets {
            if asset["name"].as_str() == Some(asset_name) {
                let url = asset["browser_download_url"]
                    .as_str()
                    .ok_or_else(|| "asset has no browser_download_url".to_string())?;
                return Ok(url.to_string());
            }
        }

        Err(format!("asset '{}' not found in release", asset_name))
    }

    /// Download a file from an HTTPS URL to a local path. Rejects non-HTTPS URLs.
    pub fn download(&self, url: &str, dest: &Path) -> Result<(), String> {
        if !url.starts_with("https://") {
            return Err(format!("refusing non-HTTPS URL: {}", url));
        }

        let mut req = ureq::get(url)
            .header("User-Agent", "kish-plugin-manager")
            .header("Accept", "application/vnd.github.v3+json");
        if let Some(token) = &self.token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }
        let mut response = req
            .call()
            .map_err(|e| format!("download request failed: {}", e))?;

        let mut file = fs::File::create(dest)
            .map_err(|e| format!("failed to create {}: {}", dest.display(), e))?;

        let mut reader = response.body_mut().as_reader();
        let mut buf = [0u8; 8192];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| format!("failed to read response body: {}", e))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])
                .map_err(|e| format!("failed to write to {}: {}", dest.display(), e))?;
        }

        Ok(())
    }

    /// Get the latest release tag for a repo, stripping a leading `v` prefix.
    pub fn latest_version(&self, owner: &str, repo: &str) -> Result<String, String> {
        let url = format!("{}/repos/{}/{}/releases/latest", self.base_url, owner, repo);
        let json = self.get_json(&url)?;

        let tag = json["tag_name"]
            .as_str()
            .ok_or_else(|| "release has no tag_name".to_string())?;

        Ok(tag.trim_start_matches('v').to_string())
    }
}

impl Default for GitHubClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Test-only client that uses a custom base URL (for mockito tests).
#[cfg(test)]
pub struct GitHubClientWithBase {
    inner: GitHubClient,
}

#[cfg(test)]
impl GitHubClientWithBase {
    pub fn new(base_url: &str) -> Self {
        Self {
            inner: GitHubClient {
                base_url: base_url.to_string(),
                token: None,
            },
        }
    }

    pub fn find_asset_url(
        &self,
        owner: &str,
        repo: &str,
        version: &str,
        asset_name: &str,
    ) -> Result<String, String> {
        self.inner.find_asset_url(owner, repo, version, asset_name)
    }

    pub fn latest_version(&self, owner: &str, repo: &str) -> Result<String, String> {
        self.inner.latest_version(owner, repo)
    }

    pub fn download(&self, url: &str, dest: &Path) -> Result<(), String> {
        self.inner.download(url, dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_release_json(assets: &[(&str, &str)]) -> String {
        let assets_json: Vec<String> = assets
            .iter()
            .map(|(name, url)| {
                format!(
                    r#"{{"name": "{}", "browser_download_url": "{}"}}"#,
                    name, url
                )
            })
            .collect();
        format!(r#"{{"tag_name": "v1.2.3", "assets": [{}]}}"#, assets_json.join(", "))
    }

    #[test]
    fn parse_release_json_finds_asset() {
        let json: serde_json::Value =
            serde_json::from_str(&make_release_json(&[("libfoo-linux-x86_64.so", "https://example.com/libfoo-linux-x86_64.so")])).unwrap();
        let url = json["assets"][0]["browser_download_url"].as_str().unwrap();
        assert_eq!(url, "https://example.com/libfoo-linux-x86_64.so");
    }

    #[test]
    fn parse_release_json_asset_not_found() {
        let json: serde_json::Value =
            serde_json::from_str(&make_release_json(&[("other-asset.so", "https://example.com/other.so")])).unwrap();
        let assets = json["assets"].as_array().unwrap();
        let found = assets.iter().any(|a| a["name"].as_str() == Some("libfoo-linux-x86_64.so"));
        assert!(!found);
    }

    #[test]
    fn download_rejects_non_https() {
        let client = GitHubClient::new();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let err = client.download("http://example.com/file", tmp.path()).unwrap_err();
        assert!(err.contains("non-HTTPS"), "expected non-HTTPS error, got: {}", err);
    }

    #[test]
    fn download_rejects_ftp_url() {
        let client = GitHubClient::new();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let err = client.download("ftp://example.com/file", tmp.path()).unwrap_err();
        assert!(err.contains("non-HTTPS"), "expected non-HTTPS error, got: {}", err);
    }

    #[test]
    fn find_asset_url_v_prefix_fallback() {
        let mut server = mockito::Server::new();
        let base = server.url();

        // v-prefixed tag returns 404
        let _m1 = server
            .mock("GET", "/repos/owner/repo/releases/tags/v1.0.0")
            .with_status(404)
            .with_body(r#"{"message": "Not Found"}"#)
            .create();

        // bare version tag succeeds
        let body = make_release_json(&[("myasset.so", "https://dl.example.com/myasset.so")]);
        let _m2 = server
            .mock("GET", "/repos/owner/repo/releases/tags/1.0.0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&body)
            .create();

        let client = GitHubClientWithBase::new(&base);
        let url = client.find_asset_url("owner", "repo", "1.0.0", "myasset.so").unwrap();
        assert_eq!(url, "https://dl.example.com/myasset.so");
    }

    #[test]
    fn find_asset_url_v_prefix_succeeds() {
        let mut server = mockito::Server::new();
        let base = server.url();

        let body = make_release_json(&[("myasset.so", "https://dl.example.com/myasset.so")]);
        let _m = server
            .mock("GET", "/repos/owner/repo/releases/tags/v2.0.0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&body)
            .create();

        let client = GitHubClientWithBase::new(&base);
        let url = client.find_asset_url("owner", "repo", "2.0.0", "myasset.so").unwrap();
        assert_eq!(url, "https://dl.example.com/myasset.so");
    }

    #[test]
    fn find_asset_url_asset_not_found() {
        let mut server = mockito::Server::new();
        let base = server.url();

        let body = make_release_json(&[("other.so", "https://dl.example.com/other.so")]);
        let _m = server
            .mock("GET", "/repos/owner/repo/releases/tags/v3.0.0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&body)
            .create();

        let client = GitHubClientWithBase::new(&base);
        let err = client.find_asset_url("owner", "repo", "3.0.0", "nonexistent.so").unwrap_err();
        assert!(err.contains("not found"), "expected not found error, got: {}", err);
    }

    #[test]
    fn latest_version_strips_v_prefix() {
        let mut server = mockito::Server::new();
        let base = server.url();

        let _m = server
            .mock("GET", "/repos/owner/repo/releases/latest")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"tag_name": "v4.5.6"}"#)
            .create();

        let client = GitHubClientWithBase::new(&base);
        let version = client.latest_version("owner", "repo").unwrap();
        assert_eq!(version, "4.5.6");
    }

    #[test]
    fn latest_version_no_v_prefix() {
        let mut server = mockito::Server::new();
        let base = server.url();

        let _m = server
            .mock("GET", "/repos/owner/repo/releases/latest")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"tag_name": "1.0.0"}"#)
            .create();

        let client = GitHubClientWithBase::new(&base);
        let version = client.latest_version("owner", "repo").unwrap();
        assert_eq!(version, "1.0.0");
    }
}
