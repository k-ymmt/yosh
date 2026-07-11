# Plugin Settings Read API — Design

Date: 2026-07-11
Status: Approved

## Goal

Let every plugin read its own configuration file without declaring any
capability. The file lives at a fixed, per-plugin location inside the
yosh config directory:

```
~/.config/yosh/plugins/{plugin-name}/settings.toml
```

The API takes no path argument, so a plugin can structurally reach only
its own settings file — other plugins' settings and arbitrary host
paths remain out of reach. The existing capability model
(`files:read`, `files_root`, …) is unchanged.

## WIT contract

A new interface is added to `yosh:plugin` (all four bundled WIT copies:
`wit/`, `crates/yosh-plugin-api/wit/`, `crates/yosh-plugin-sdk/wit/`,
`crates/yosh-plugin-manager/wit/` — `build.rs` already enforces they
stay in sync):

```wit
interface settings {
    use types.{error-code};
    /// Contents of this plugin's own settings.toml
    /// (`~/.config/yosh/plugins/<name>/settings.toml`).
    /// Returns `none` when the file does not exist (the normal
    /// "no settings" case). TOML parsing is the plugin's concern —
    /// the host returns the raw text.
    read: func() -> result<option<string>, error-code>;
}
```

`world plugin-world` gains `import settings;`.

**The package version stays `@0.2.1`.** Adding an interface does not
change any import string of already-built plugins, so installed
plugins keep loading byte-for-byte identically. Bumping the version
would require touching 5 linker strings in `src/plugin/linker.rs` plus
3 in `crates/yosh-plugin-manager/src/metadata_extract.rs` and would
make old-plugin loading depend on wasmtime's semver matching — a class
of breakage this repo has hit before (see dfb3167). Plugins built
against the new WIT and run on an old host fail to instantiate with a
clear "imports instance yosh:plugin/settings@0.2.1" diagnostic, which
a version bump would not improve.

## Host implementation

### HostContext

`src/plugin/host/mod.rs` gains:

```rust
/// Absolute path of this plugin's settings.toml, resolved at load
/// time from `$HOME/.config/yosh/plugins/<name>/settings.toml`.
/// `None` when HOME is unset or the plugin name is unsafe as a
/// path component — `settings.read` then reports "no settings".
pub(super) settings_path: Option<PathBuf>,
```

Resolution happens in `load_one` (`src/plugin/mod.rs`) via a helper in
`src/plugin/config.rs`:

```rust
pub fn settings_path_for(plugin_name: &str) -> Option<PathBuf>
```

- Returns `None` if `HOME` is unset.
- Returns `None` if the name is empty, is `.` or `..`, or contains
  `/` (defense in depth; lockfile names are trusted but cheap to
  harden).
- Otherwise `HOME/.config/yosh/plugins/<name>/settings.toml`.

### Host function

New `src/plugin/host/settings.rs`:

```rust
pub fn host_settings_read(ctx: &HostContext)
    -> Result<Option<String>, ErrorCode>
```

- `ctx.ensure_bound()?` — the metadata contract holds: `metadata()`
  runs with no env binding and gets `Denied`; `on-load` and later
  callbacks can read settings.
- `settings_path == None` → `Ok(None)`.
- `std::fs::read_to_string`:
  - `Ok(text)` → `Ok(Some(text))`
  - `ErrorKind::NotFound` → `Ok(None)`
  - anything else (permissions, invalid UTF-8 / `InvalidData`) →
    `Err(IoFailed)`

No deny variant exists — the interface is granted unconditionally.

### Linker registration

`src/plugin/linker.rs` registers `yosh:plugin/settings@0.2.1` with the
real implementation, outside any capability check.

`crates/yosh-plugin-manager/src/metadata_extract.rs` registers the
same instance (a stub returning `Ok(None)` is sufficient there — it
only needs the import satisfied for instantiation during metadata
extraction).

## SDK

`crates/yosh-plugin-sdk` re-exports the generated binding and adds:

```rust
/// Read this plugin's own settings.toml. Needs no capability.
/// Returns Ok(None) when no settings file exists.
pub fn read_settings() -> Result<Option<String>, ErrorCode>
```

Crate docs note that `settings` is the one capability-free host
interface and that TOML parsing is the plugin author's choice (e.g.
the `toml` crate compiles to wasm32-wasip2).

## Testing

- Unit tests in `src/plugin/host/settings.rs`:
  - unbound ctx → `Denied` (metadata contract)
  - `settings_path == None` → `Ok(None)`
  - missing file → `Ok(None)`
  - existing file → `Ok(Some(content))`
- Unit tests for `settings_path_for`: HOME-relative happy path,
  unsafe names (`..`, `a/b`, empty) → `None`. HOME manipulation uses
  the existing test-env conventions in this repo.
- Integration: `tests/plugins/test_plugin` gains a command
  (e.g. `settings-echo`) that prints the result of `read_settings()`;
  a `--features test-helpers` integration test writes
  `settings.toml` under a temp HOME, loads the plugin, and asserts
  the content round-trips, plus the file-absent case.

## Out of scope

- Writing settings (read-only by decision; can be added later as a
  separate function or capability).
- Key/value access API — the host returns raw text.
- Watching/reloading settings on change.
