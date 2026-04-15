# GitHubApiError Type-Safe Error Design

## Problem

`GitHubClient::get_json` returns `Result<serde_json::Value, String>`. HTTP status codes are encoded as strings like `"HTTP 404"`, and callers discriminate errors via `e == "HTTP 404"` string comparison. This is fragile — a formatting change silently breaks error handling.

## Scope

Introduce a `GitHubApiError` enum used internally by `get_json` and its direct callers (`release_json`, `find_asset_url`, `latest_version`). Public API methods continue to return `Result<_, String>` so external callers (`sync.rs`, `install.rs`, `main.rs`) are unaffected.

## Design

### Error Enum

```rust
#[derive(Debug)]
enum GitHubApiError {
    /// HTTP response with non-2xx status code
    HttpStatus(u16),
    /// Network/transport error (DNS, connection, timeout)
    Network(String),
    /// Response body could not be read or parsed as JSON
    Parse(String),
}

impl std::fmt::Display for GitHubApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HttpStatus(code) => write!(f, "HTTP {}", code),
            Self::Network(msg) => write!(f, "request failed: {}", msg),
            Self::Parse(msg) => write!(f, "{}", msg),
        }
    }
}
```

### Changes

1. **`get_json`**: Return `Result<Value, GitHubApiError>` instead of `Result<Value, String>`.
   - `ureq::Error::StatusCode(code)` → `GitHubApiError::HttpStatus(*code)`
   - Other `ureq::Error` → `GitHubApiError::Network(e.to_string())`
   - Body read failure → `GitHubApiError::Parse("failed to read body: ...")`
   - JSON parse failure → `GitHubApiError::Parse("failed to parse JSON: ...")`

2. **`release_json`**: Return `Result<Value, GitHubApiError>` (passes through from `get_json`).

3. **`find_asset_url`**: Match on `GitHubApiError::HttpStatus(404)` instead of `e == "HTTP 404"`. Convert to `String` for the public return type.

4. **`latest_version`**: Same pattern — match on `GitHubApiError::HttpStatus(404)` instead of string comparison. Convert to `String` for the public return type.

5. **`download`**: Out of scope — uses its own `ureq::get` call and doesn't go through `get_json`.

6. **`GitHubClientWithBase` (test helper)**: No change needed — delegates to `inner` methods which already return `Result<_, String>`.

### Test Updates

Existing tests assert on string content of error messages. Since public API still returns `String`, most tests need no changes. The string format of error messages remains the same — only the internal representation changes.

## Non-Goals

- Changing public API signatures (`find_asset_url`, `latest_version`, `download`)
- Introducing a crate-wide error type
- Changing `download` method error handling
