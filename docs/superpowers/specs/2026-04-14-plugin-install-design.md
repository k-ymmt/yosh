# Plugin Install Command Design

**Date:** 2026-04-14
**Scope:** Add `install` subcommand to `kish-plugin-manager`

## Summary

Add `kish plugin install` to allow users to register plugins in `plugins.toml` without manual file editing. The command handles two source types: GitHub repositories (via URL) and local file paths. It writes entries to `plugins.toml` only — actual downloading is delegated to the existing `kish plugin sync`.

## Command Interface

```
kish plugin install <SOURCE>[@<VERSION>] [--force]
```

### Argument Parsing

| Input | Source | Version |
|-------|--------|---------|
| `https://github.com/owner/repo` | `github:owner/repo` | Latest (via GitHub API) |
| `https://github.com/owner/repo@1.0.0` | `github:owner/repo` | `1.0.0` |
| `/path/to/lib.dylib` | `local:/path/to/lib.dylib` | None |
| `./relative/lib.dylib` | `local:<absolute path>` | None |

### Flags

- `--force`: Overwrite an existing plugin entry with the same name.

### Plugin Name Resolution

- GitHub: repository name (e.g., `owner/kish-plugin-foo` → `kish-plugin-foo`)
- Local: file stem (e.g., `/path/to/libfoo.dylib` → `libfoo`)

### Error Cases

- Same-name plugin already exists and `--force` not specified → error
- GitHub URL parse failure → error
- Local path does not exist → error
- Version unspecified and GitHub API returns no releases → error

## Internal Processing Flow

1. **Parse argument** — Determine if input is a GitHub URL or local path. Extract `owner/repo` and optional `@version` from URLs. Canonicalize local paths to absolute.
2. **Load `plugins.toml`** — Read existing config from `~/.config/kish/plugins.toml`.
3. **Duplicate check** — If a plugin with the same name exists: error without `--force`, overwrite with `--force`.
4. **Resolve version** (GitHub, no `@version`) — Call GitHub API to get the latest release tag. Reuse existing `github.rs` infrastructure.
5. **Write to `plugins.toml`** — Use `toml_edit` to add/replace the `[[plugin]]` entry while preserving existing formatting and comments.
   - Fields written: `name`, `source`, `version` (GitHub only), `enabled = true`
6. **Print result** — e.g., `Installed plugin 'foo' (github:example/foo@1.0.0)`. For GitHub plugins, print `Run 'kish plugin sync' to download.`

## TOML Writing

The current codebase uses `toml` (serde) for reading `plugins.toml`. Writing with `toml_edit` is necessary to preserve user comments and formatting. This requires adding `toml_edit` as a dependency to `kish-plugin-manager`.

### Example Output

Before:
```toml
[[plugin]]
name = "bar"
source = "local:/usr/lib/bar.dylib"
enabled = true
```

After `kish plugin install https://github.com/example/foo@1.0.0`:
```toml
[[plugin]]
name = "bar"
source = "local:/usr/lib/bar.dylib"
enabled = true

[[plugin]]
name = "foo"
source = "github:example/foo"
version = "1.0.0"
enabled = true
```

## Integration with Existing Commands

- `install` adds entries to `plugins.toml` only.
- `sync` downloads binaries and updates `plugins.lock` — no changes needed.
- `update` updates GitHub plugin versions — no changes needed.
- `list` / `verify` read `plugins.lock` — no changes needed.

## Testing Strategy

### Unit Tests

- **URL parsing:** `https://github.com/owner/repo` → `("owner", "repo", None)`, with `@version` → `Some(version)`, invalid URLs → error.
- **Source type detection:** Distinguish GitHub URLs from local file paths.
- **Duplicate detection:** With/without `--force`.

### Integration Tests

- Create temporary `plugins.toml`, run install, verify entry is correctly appended.
- Verify existing entries and comments are preserved after install.
- Test `--force` overwrites existing entry.
- Test local path canonicalization.

### Not Tested in Install

- GitHub API calls for latest version — covered by existing `github.rs` tests.
- Actual binary download — that is `sync`'s responsibility.

## Changes Required

| File | Change |
|------|--------|
| `crates/kish-plugin-manager/Cargo.toml` | Add `toml_edit` dependency |
| `crates/kish-plugin-manager/src/main.rs` | Add `Install` variant to clap subcommands |
| `crates/kish-plugin-manager/src/install.rs` | New file: install logic (parse, check, write) |
| `crates/kish-plugin-manager/src/lib.rs` | Export `install` module |
