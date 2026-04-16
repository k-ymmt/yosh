# Design: Publish to crates.io via `cargo install`

**Date:** 2026-04-16
**Status:** Approved

## Goal

Enable `cargo install yosh` to install the yosh shell from crates.io. This involves:
1. Renaming the project from `kish` to `yosh` across the entire codebase
2. Adding crates.io-required metadata to all crates
3. Publishing all workspace crates to crates.io
4. Renaming the GitHub repository

## Decisions

- **Crate name:** `yosh` (verified available on crates.io)
- **Binary name:** `yosh`
- **License:** MIT
- **Scope:** Full rename — all code, tests, configs, and documentation references
- **Publish strategy:** All 4 workspace crates published to crates.io
- **Repository:** `https://github.com/k-ymmt/yosh` (renamed via `gh repo rename`)

## Rename Scope

### Changed

| Category | File count | Changes |
|---|---|---|
| Cargo.toml files | 5 | Crate names, dependency names, crates.io metadata |
| Source code (.rs) | ~47 | Error messages, `use` statements, string literals |
| Tests (.rs) | ~14 | Binary paths, output assertions |
| E2E tests / scripts (.sh) | ~4 | Binary references |
| Directory names | 3 | `crates/kish-plugin-*` → `crates/yosh-plugin-*` |
| CLAUDE.md | 1 | Project description update |
| New files | 1 | `LICENSE` (MIT) |

### Unchanged

- `docs/superpowers/` design documents and plans — historical records, left as-is
- `docs/kish/plugin.md` — documentation, left as-is

## Cargo.toml Metadata

### Common fields added to all crates

```toml
license = "MIT"
repository = "https://github.com/k-ymmt/yosh"
```

### Per-crate configuration

| Crate | name | description |
|---|---|---|
| Root | `yosh` | `"A POSIX-compliant shell implemented in Rust"` |
| Plugin API | `yosh-plugin-api` | `"Plugin API for the yosh shell"` |
| Plugin SDK | `yosh-plugin-sdk` | `"Plugin SDK for the yosh shell"` |
| Plugin Manager | `yosh-plugin-manager` | `"Plugin manager for the yosh shell"` |

### Dependency declarations

Path dependencies use `version` + `path` for crates.io compatibility:

```toml
# In root Cargo.toml
yosh-plugin-api = { version = "0.1.0", path = "crates/yosh-plugin-api" }

# In yosh-plugin-sdk/Cargo.toml
yosh-plugin-api = { version = "0.1.0", path = "../yosh-plugin-api" }
```

- Local development uses `path` (Cargo prioritizes it)
- crates.io resolution uses `version`

### Workspace members

```toml
[workspace]
members = [
    ".",
    "crates/yosh-plugin-api",
    "crates/yosh-plugin-sdk",
    "crates/yosh-plugin-manager",
    "tests/plugins/test_plugin",
]
```

## Source Code Changes

### Error message prefix

`"kish: "` → `"yosh: "` across all source files.

### Plugin symbols

- `KISH_PLUGIN_API_VERSION` → `YOSH_PLUGIN_API_VERSION`
- `kish_plugin_api::*` → `yosh_plugin_api::*`
- `kish_plugin_manager` → `yosh_plugin_manager`
- `kish_plugin_sdk` → `yosh_plugin_sdk`

### Subcommand delegation (main.rs)

`yosh plugin` delegates to `yosh-plugin` binary in PATH (was `kish-plugin`).

### Shell name

- Default shell name: `"yosh"`
- Help text, usage display: all updated

### Tests

- `cargo_bin("kish")` → `cargo_bin("yosh")`
- Output assertions: `"kish: "` → `"yosh: "`

### E2E tests (e2e/run_tests.sh)

- Built binary reference: `kish` → `yosh`

## New Files

### LICENSE

MIT license, copyright holder: Kazuki Yamamoto.

## Publish Order

crates.io requires dependencies to be published before dependents:

1. `yosh-plugin-api` (no internal dependencies)
2. `yosh-plugin-sdk` (depends on `yosh-plugin-api`)
3. `yosh-plugin-manager` (external dependencies only)
4. `yosh` (depends on `yosh-plugin-api`)

## GitHub Repository Rename

Final step after all code changes are committed and pushed:

```bash
gh repo rename yosh
```

GitHub automatically redirects the old URL (`k-ymmt/kish`) to the new one (`k-ymmt/yosh`).

## CLAUDE.md Update

- Project name: `kish` → `yosh`
- Error message prefix convention: `kish: ` → `yosh: `
