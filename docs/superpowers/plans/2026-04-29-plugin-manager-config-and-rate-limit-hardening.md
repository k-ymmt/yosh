# Plugin Manager: Config Duplicate Detection & GitHub Rate-Limit Hint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Combine TODO #17 (reject duplicate `[[plugin]].name` at `load_config` time) and TODO #9 (suggest `YOSH_GITHUB_TOKEN` when GitHub returns 403/429) into one branch, while removing the legacy `KISH_GITHUB_TOKEN` env var as part of the kish→yosh rename cleanup.

**Architecture:** Two independent improvements in `crates/yosh-plugin-manager/`. (A) Fail-fast duplicate detection in `config::load_config` so all consumers (`sync`, `list`, `install`, `update`) surface the same clear error. (B) New `RateLimited` variant on `GitHubApiError`, mapped from HTTP 403/429 in `get_json`, surfaced as a hint in `latest_version` / `find_asset_url` error messages when no token is set. Token reading drops `KISH_GITHUB_TOKEN`, keeping `YOSH_GITHUB_TOKEN` (preferred) and `GITHUB_TOKEN` (de-facto standard).

**Tech Stack:** Rust 2024, `toml`/`toml_edit` for config parsing, `ureq` for HTTP, `mockito` for HTTP test fixtures.

**No new spec doc:** TODO entries themselves carry the requirement; the existing spec at `docs/superpowers/specs/2026-04-13-github-plugin-fetching-design.md` only needs an env-var name update.

**Branch policy:** Work directly on `main` per project convention.

---

## File Structure

| File | Responsibility | Change kind |
|------|----------------|-------------|
| `crates/yosh-plugin-manager/src/config.rs` | TOML config parsing + name validation. Owns the new uniqueness check. | Modify |
| `crates/yosh-plugin-manager/src/github.rs` | GitHub API client + error type. Owns `RateLimited` variant, hint injection, env-var rename. | Modify |
| `docs/superpowers/specs/2026-04-13-github-plugin-fetching-design.md` | Spec for GitHub fetching. §Authentication mentions env var name. | Modify (one line) |
| `TODO.md` | Tracking list. Remove #17 and #9 once done (per CLAUDE.md "delete completed items"). | Modify |

No new files. No file splits.

---

## Task 1: Duplicate plugin name detection (TODO #17)

**Files:**
- Modify: `crates/yosh-plugin-manager/src/config.rs:92-124` (`load_config`)
- Modify: `crates/yosh-plugin-manager/src/config.rs:135-345` (tests module — append two tests)

- [ ] **Step 1.1: Write failing test for same-source duplicates**

Append to the `tests` module in `crates/yosh-plugin-manager/src/config.rs` (after `empty_config_returns_empty_vec` at line 339-344):

```rust
    #[test]
    fn reject_duplicate_plugin_names() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "dup"
source = "local:/tmp/a.wasm"

[[plugin]]
name = "dup"
source = "local:/tmp/b.wasm"
"#
        )
        .unwrap();
        let err = load_config(f.path()).unwrap_err();
        assert!(
            err.contains("duplicate"),
            "expected duplicate-name error, got: {}",
            err
        );
        assert!(
            err.contains("'dup'"),
            "expected duplicate name in error, got: {}",
            err
        );
    }

    #[test]
    fn reject_duplicate_plugin_names_different_sources() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "shared"
source = "github:owner/shared"
version = "1.0.0"

[[plugin]]
name = "shared"
source = "local:/tmp/shared.wasm"
"#
        )
        .unwrap();
        let err = load_config(f.path()).unwrap_err();
        assert!(
            err.contains("duplicate"),
            "uniqueness must be enforced regardless of source kind, got: {}",
            err
        );
    }
```

- [ ] **Step 1.2: Run new tests to verify they fail**

Run: `cargo test -p yosh-plugin-manager --lib config::tests::reject_duplicate -- --nocapture`

Expected: both `reject_duplicate_plugin_names` and `reject_duplicate_plugin_names_different_sources` FAIL because `load_config` accepts duplicates today (returns `Ok` with two entries).

- [ ] **Step 1.3: Implement duplicate-name check in `load_config`**

Replace the body of `load_config` (currently lines 92-124) with:

```rust
pub fn load_config(path: &Path) -> Result<Vec<PluginDecl>, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let raw: RawConfig =
        toml::from_str(&content).map_err(|e| format!("{}: {}", path.display(), e))?;
    let decls: Vec<PluginDecl> = raw
        .plugin
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
        .collect::<Result<Vec<_>, String>>()?;

    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for decl in &decls {
        if !seen.insert(decl.name.as_str()) {
            return Err(format!(
                "plugin '{}': duplicate name (already defined earlier in config)",
                decl.name
            ));
        }
    }

    Ok(decls)
}
```

The only structural change is collecting into `Vec` first (instead of returning the iterator chain directly), then walking once with a `HashSet` to detect duplicates.

- [ ] **Step 1.4: Run new tests to verify they pass**

Run: `cargo test -p yosh-plugin-manager --lib config::tests::reject_duplicate`

Expected: both tests PASS.

- [ ] **Step 1.5: Run full `config` test module to verify no regressions**

Run: `cargo test -p yosh-plugin-manager --lib config::tests`

Expected: all tests PASS (existing 12 tests + 2 new tests = 14 tests).

- [ ] **Step 1.6: Commit**

```bash
git add crates/yosh-plugin-manager/src/config.rs
git commit -m "$(cat <<'EOF'
fix(plugin-manager): reject duplicate plugin names at config load time

Original task: TODO.md #17 — `config::load_config` does not detect
duplicate plugin `name` entries. Previously the failure surfaced only
at `update::set_plugin_version` time as a defensive check; sync/list/
install silently used the first match. Now `load_config` walks the
parsed declarations once with a `HashSet` and returns a clear error on
the first duplicate, so all consumers fail fast with a consistent
message.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Drop KISH_GITHUB_TOKEN, prefer YOSH_GITHUB_TOKEN

**Files:**
- Modify: `crates/yosh-plugin-manager/src/github.rs:32-42` (`GitHubClient::new` + doc comment)
- Modify: `docs/superpowers/specs/2026-04-13-github-plugin-fetching-design.md:189` (Authentication line)

- [ ] **Step 2.1: Update `GitHubClient::new` and its doc comment**

In `crates/yosh-plugin-manager/src/github.rs`, replace lines 33-42 (the `/// Create a new client...` doc comment through the closing `}` of `new`):

```rust
    /// Create a new client, reading auth token from `YOSH_GITHUB_TOKEN`
    /// (preferred) or `GITHUB_TOKEN`. The legacy `KISH_GITHUB_TOKEN` env
    /// var was removed as part of the kish→yosh rename cleanup in
    /// v0.2.0.
    pub fn new() -> Self {
        let token = std::env::var("YOSH_GITHUB_TOKEN")
            .ok()
            .or_else(|| std::env::var("GITHUB_TOKEN").ok());
        Self {
            base_url: "https://api.github.com".to_string(),
            token,
        }
    }
```

- [ ] **Step 2.2: Update the spec doc**

In `docs/superpowers/specs/2026-04-13-github-plugin-fetching-design.md` line 189, replace:

```
- **With token**: Set `KISH_GITHUB_TOKEN` or `GITHUB_TOKEN` environment variable. Sent as `Authorization: Bearer {token}`. Supports private repositories. Rate limit: 5,000 requests/hour.
```

with:

```
- **With token**: Set `YOSH_GITHUB_TOKEN` or `GITHUB_TOKEN` environment variable. Sent as `Authorization: Bearer {token}`. Supports private repositories. Rate limit: 5,000 requests/hour.
```

- [ ] **Step 2.3: Run github tests to verify no regression**

Run: `cargo test -p yosh-plugin-manager --lib github::tests`

Expected: all existing tests PASS. (No test depends on `KISH_GITHUB_TOKEN`; the env-var change does not alter behavior for tests that don't set tokens.)

- [ ] **Step 2.4: Commit**

```bash
git add crates/yosh-plugin-manager/src/github.rs docs/superpowers/specs/2026-04-13-github-plugin-fetching-design.md
git commit -m "$(cat <<'EOF'
refactor(plugin-manager): drop KISH_GITHUB_TOKEN, prefer YOSH_GITHUB_TOKEN

Original task: TODO.md #9 (env-var renaming portion). The legacy env
var name from before the kish→yosh rename is no longer accepted; users
should set YOSH_GITHUB_TOKEN (or the de-facto-standard GITHUB_TOKEN).
Aligned the GitHub-fetching spec doc to match.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Rate-limit hint in API errors (TODO #9 main change)

**Files:**
- Modify: `crates/yosh-plugin-manager/src/github.rs:5-24` (`GitHubApiError` enum + Display impl)
- Modify: `crates/yosh-plugin-manager/src/github.rs:44-62` (`get_json` error mapping)
- Modify: `crates/yosh-plugin-manager/src/github.rs:79-119` (`find_asset_url` error mapping)
- Modify: `crates/yosh-plugin-manager/src/github.rs:157-175` (`latest_version` error mapping)
- Modify: `crates/yosh-plugin-manager/src/github.rs:185-225` (`GitHubClientWithBase` test helper — add `with_token`)
- Modify: `crates/yosh-plugin-manager/src/github.rs` tests module (append 3 tests)

- [ ] **Step 3.1: Add `with_token` test helper**

In `crates/yosh-plugin-manager/src/github.rs`, after the existing `pub fn new(base_url: &str) -> Self { ... }` of `GitHubClientWithBase` (around line 192-199), add:

```rust
    pub fn with_token(base_url: &str, token: &str) -> Self {
        Self {
            inner: GitHubClient {
                base_url: base_url.to_string(),
                token: Some(token.to_string()),
            },
        }
    }
```

- [ ] **Step 3.2: Write 3 failing tests**

Append to the `tests` module at the end of `crates/yosh-plugin-manager/src/github.rs` (after the existing `find_asset_url_both_tags_404_gives_helpful_error` test):

```rust
    #[test]
    fn latest_version_403_without_token_includes_hint() {
        let mut server = mockito::Server::new();
        let base = server.url();

        let _m = server
            .mock("GET", "/repos/owner/repo/releases/latest")
            .with_status(403)
            .with_body(r#"{"message": "API rate limit exceeded"}"#)
            .create();

        let client = GitHubClientWithBase::new(&base);
        let err = client.latest_version("owner", "repo").unwrap_err();
        assert!(
            err.contains("YOSH_GITHUB_TOKEN"),
            "expected hint mentioning YOSH_GITHUB_TOKEN, got: {}",
            err
        );
    }

    #[test]
    fn latest_version_403_with_token_no_hint() {
        let mut server = mockito::Server::new();
        let base = server.url();

        let _m = server
            .mock("GET", "/repos/owner/repo/releases/latest")
            .with_status(403)
            .with_body(r#"{"message": "Bad credentials"}"#)
            .create();

        let client = GitHubClientWithBase::with_token(&base, "fake-token");
        let err = client.latest_version("owner", "repo").unwrap_err();
        assert!(
            !err.contains("YOSH_GITHUB_TOKEN"),
            "should not suggest setting a token when one is already set, got: {}",
            err
        );
        assert!(
            err.contains("403"),
            "should still surface the HTTP status, got: {}",
            err
        );
    }

    #[test]
    fn find_asset_url_429_without_token_includes_hint() {
        let mut server = mockito::Server::new();
        let base = server.url();

        // Both v-prefix and bare-version tag attempts return 429 (rate-limited).
        let _m1 = server
            .mock("GET", "/repos/owner/repo/releases/tags/v1.0.0")
            .with_status(429)
            .with_body(r#"{"message": "Too many requests"}"#)
            .create();
        let _m2 = server
            .mock("GET", "/repos/owner/repo/releases/tags/1.0.0")
            .with_status(429)
            .with_body(r#"{"message": "Too many requests"}"#)
            .create();

        let client = GitHubClientWithBase::new(&base);
        let err = client
            .find_asset_url("owner", "repo", "1.0.0", "asset.wasm")
            .unwrap_err();
        assert!(
            err.contains("YOSH_GITHUB_TOKEN"),
            "expected rate-limit hint, got: {}",
            err
        );
    }
```

- [ ] **Step 3.3: Run new tests to verify they fail**

Run: `cargo test -p yosh-plugin-manager --lib github::tests::latest_version_403 github::tests::find_asset_url_429`

Expected: all 3 new tests FAIL. The 403/429 tests fail because the current code surfaces a generic `HTTP 403`/`HTTP 429` string with no hint.

- [ ] **Step 3.4: Add `RateLimited` variant to `GitHubApiError`**

In `crates/yosh-plugin-manager/src/github.rs`, replace lines 5-24 (the enum and its Display impl) with:

```rust
/// Error type for GitHub API requests made through `get_json`.
#[derive(Debug)]
enum GitHubApiError {
    /// HTTP response with non-2xx status code (excluding 403/429).
    HttpStatus(u16),
    /// HTTP 403 or 429 — likely rate-limited, or auth-rejected when a
    /// token is set. Surfaced separately so callers can attach a
    /// "set YOSH_GITHUB_TOKEN" hint when no token is configured.
    RateLimited(u16),
    /// Network/transport error (DNS, connection, timeout).
    Network(String),
    /// Response body could not be read or parsed as JSON.
    Parse(String),
}

impl std::fmt::Display for GitHubApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HttpStatus(code) => write!(f, "HTTP {}", code),
            Self::RateLimited(code) => write!(f, "HTTP {} (rate-limited or unauthorized)", code),
            Self::Network(msg) => write!(f, "request failed: {}", msg),
            Self::Parse(msg) => write!(f, "{}", msg),
        }
    }
}

const RATE_LIMIT_HINT: &str =
    "hint: set YOSH_GITHUB_TOKEN or GITHUB_TOKEN to raise the rate limit (60 -> 5000 req/hour)";
```

The `RATE_LIMIT_HINT` constant lives at module scope so both `latest_version` and `find_asset_url` can reuse it.

- [ ] **Step 3.5: Update `get_json` to map 403/429 to `RateLimited`**

In `crates/yosh-plugin-manager/src/github.rs`, replace the `.map_err(|e| match &e { ... })` block in `get_json` (currently around lines 53-56) with:

```rust
        let body = req
            .call()
            .map_err(|e| match &e {
                ureq::Error::StatusCode(code) if *code == 403 || *code == 429 => {
                    GitHubApiError::RateLimited(*code)
                }
                ureq::Error::StatusCode(code) => GitHubApiError::HttpStatus(*code),
                _ => GitHubApiError::Network(e.to_string()),
            })?
            .body_mut()
            .read_to_string()
            .map_err(|e| GitHubApiError::Parse(format!("failed to read body: {}", e)))?;
```

(Only the `match &e` arms change — the chained `.body_mut().read_to_string()...` stays as-is.)

- [ ] **Step 3.6: Inject hint in `latest_version` error mapping**

In `crates/yosh-plugin-manager/src/github.rs`, replace the `.map_err(|e| match e { ... })` block in `latest_version` (currently around lines 159-168) with:

```rust
        let json = self.get_json(&url).map_err(|e| match e {
            GitHubApiError::HttpStatus(404) => format!(
                "no releases found for {}/{}: publish a GitHub Release first",
                owner, repo
            ),
            other @ GitHubApiError::RateLimited(_) if self.token.is_none() => format!(
                "failed to fetch latest release for {}/{}: {}\n  {}",
                owner, repo, other, RATE_LIMIT_HINT
            ),
            other => format!(
                "failed to fetch latest release for {}/{}: {}",
                owner, repo, other
            ),
        })?;
```

The `@`-binding `other @ GitHubApiError::RateLimited(_)` lets the guarded arm reuse the value's `Display` impl while still matching the variant.

- [ ] **Step 3.7: Inject hint in `find_asset_url` error mapping**

In `crates/yosh-plugin-manager/src/github.rs`, replace the inner `.map_err(|e| match e { ... })` block in `find_asset_url`'s fallback branch (currently around lines 91-101) with:

```rust
            Err(_) => {
                // Fallback to bare version tag
                self.release_json(owner, repo, version)
                    .map_err(|e| match e {
                        GitHubApiError::HttpStatus(404) => format!(
                            "release not found for {}/{} (tried tags '{}' and '{}')",
                            owner, repo, v_tag, version
                        ),
                        other @ GitHubApiError::RateLimited(_) if self.token.is_none() => format!(
                            "failed to fetch release for {}/{} (tried tags '{}' and '{}'): {}\n  {}",
                            owner, repo, v_tag, version, other, RATE_LIMIT_HINT
                        ),
                        other => format!(
                            "failed to fetch release for {}/{} (tried tags '{}' and '{}'): {}",
                            owner, repo, v_tag, version, other
                        ),
                    })?
            }
```

- [ ] **Step 3.8: Run the new tests to verify they pass**

Run: `cargo test -p yosh-plugin-manager --lib github::tests::latest_version_403 github::tests::find_asset_url_429`

Expected: all 3 new tests PASS.

- [ ] **Step 3.9: Run all `github` tests to verify no regression**

Run: `cargo test -p yosh-plugin-manager --lib github::tests`

Expected: all tests PASS (existing 11 tests + 3 new = 14 tests). In particular, `find_asset_url_both_tags_404_gives_helpful_error`, `latest_version_no_releases_gives_helpful_error`, and the v-prefix-fallback tests must still pass — they use 404 which now goes through `HttpStatus(404)` (unchanged path).

- [ ] **Step 3.10: Run the full plugin-manager test suite**

Run: `cargo test -p yosh-plugin-manager`

Expected: all unit tests + integration tests (`tests/sync_integration.rs`) PASS. The integration tests do not exercise rate-limited paths, so behavior is unchanged for them.

- [ ] **Step 3.11: Commit**

```bash
git add crates/yosh-plugin-manager/src/github.rs
git commit -m "$(cat <<'EOF'
feat(plugin-manager): suggest YOSH_GITHUB_TOKEN on GitHub rate-limit

Original task: TODO.md #9 — when `yosh-plugin sync` / `install` /
`update` hits the unauthenticated GitHub API rate limit (60 req/hour),
the previous error was a bare "HTTP 403" with no hint. Added a
`RateLimited` variant to `GitHubApiError` mapped from HTTP 403/429,
and surface a one-line hint suggesting YOSH_GITHUB_TOKEN /
GITHUB_TOKEN whenever no token is configured. When a token is set,
403 likely indicates a real auth issue, so the hint is suppressed.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Remove completed TODO entries

**Files:**
- Modify: `TODO.md:57` (delete #9 entry)
- Modify: `TODO.md:65` (delete #17 entry)

Per CLAUDE.md / project memory: "Delete completed items rather than marking them with `[x]`."

- [ ] **Step 4.1: Remove the TODO entries**

In `TODO.md`, delete two specific lines from the "Future: Plugin System Enhancements" section:

Line 57:
```
- [ ] `yosh-plugin sync`/`install`: suggest `YOSH_GITHUB_TOKEN` when GitHub API rate limit (60 req/hour) is hit without auth (`crates/yosh-plugin-manager/src/github.rs`, `crates/yosh-plugin-manager/src/install.rs`)
```

Line 65:
```
- [ ] `config::load_config` does not detect duplicate plugin `name` entries — `validate_plugin_name` only checks character set, not uniqueness within the config. `update::set_plugin_version` errors defensively when it sees a duplicate, but the failure surfaces only at update time. Reject duplicates at load time so `sync`/`list`/`install` also fail fast and the user gets a clearer error (`crates/yosh-plugin-manager/src/config.rs`). Spec follow-up from 2026-04-28 plugin-update toml_edit migration.
```

- [ ] **Step 4.2: Final sanity check — run plugin-manager tests once more**

Run: `cargo test -p yosh-plugin-manager`

Expected: all tests PASS.

- [ ] **Step 4.3: Commit**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
docs(todo): remove completed plugin-manager items #17 and #9

Original task: track completion of TODO #17 (duplicate plugin name
detection at load time) and TODO #9 (YOSH_GITHUB_TOKEN rate-limit hint
+ KISH_GITHUB_TOKEN cleanup). Per CLAUDE.md convention, completed
items are deleted rather than checked off.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-Review Notes

**Spec coverage:**
- TODO #17 → Task 1 (duplicate detection at load_config + 2 tests)
- TODO #9 (hint portion) → Task 3 (RateLimited variant + hint injection + 3 tests)
- TODO #9 (env-var rename portion, agreed during brainstorming) → Task 2 (drop KISH_GITHUB_TOKEN)
- Spec doc env-var line → Task 2 (one-line update)
- TODO entry deletion → Task 4

**Type/method consistency:**
- `GitHubApiError::RateLimited(u16)` — single tuple variant, used identically in Tasks 3.4 / 3.5 / 3.6 / 3.7
- `GitHubClientWithBase::with_token(base_url: &str, token: &str)` — defined in Task 3.1, used in Task 3.2's middle test
- `RATE_LIMIT_HINT` constant — defined in Task 3.4, used in Tasks 3.6 and 3.7
- `load_config` signature unchanged — only body restructured

**Placeholder scan:** None. Every code block is complete; every test has full assertions; every commit message is concrete.

**Edge cases covered:**
- Token-set 403 → no hint (3.2 second test)
- 429 status as well as 403 (3.2 third test)
- Same name across `github:` and `local:` source kinds (1.1 second test)
- Existing 404-path tests still pass (3.9)
