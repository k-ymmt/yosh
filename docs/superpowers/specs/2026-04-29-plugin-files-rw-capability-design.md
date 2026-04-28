# Plugin `files:read` / `files:write` Capability

## Goal

Add two new plugin capabilities — `files:read` and `files:write` — that let
WASM plugins read and write files on the user's filesystem. The driving
use case is a git-aware prompt plugin that needs to read `.git/HEAD`,
walk up directories to locate `.git/`, and list `.git/refs/heads/`. Write
is shipped at the same time for symmetry, even though no concrete
write-side use case is specified yet.

## Non-Goals

- Per-path scoping or allowlisting. Plugins granted `files:read` /
  `files:write` can touch anywhere the shell process itself can reach.
- Streaming I/O via WIT `resource` handles. Whole-file `list<u8>` is
  enough for the foreseeable use cases (config / dotfiles / refs).
- Fuel metering or per-call file-size caps. Tracked separately under
  TODO.md "Plugin runtime limits".
- Re-architecting the existing `filesystem` capability (which covers
  `cwd` / `set-cwd`). It stays exactly as is for backward compatibility.

## Threat Model

Plugins are **trusted code** — the user explicitly opts in by editing
`plugins.toml` or running `yosh-plugin install`. The defence we are
building is against *unintended* permission grants, not against malicious
plugins. Mitigations:

1. **Default-deny in three layers** (described in §7).
2. **Explicit allowlist required**: the new capabilities are not granted
   unless the user writes them into `plugins.toml`'s `capabilities`
   field — same negotiation as every other capability.
3. **Documentation warning**: `files:write` lets the plugin destroy any
   file the user can write. Don't grant it without verifying the plugin.

A complete sandbox would need per-path scoping and a path-policy DSL.
That is deliberately out of scope; if it ever lands, it is additive on
top of this design (e.g., a future `files:read-paths = ["~/.git/**"]`
field can intersect with the bitflag).

## 1. Architecture Overview

The new capabilities follow the same pattern as the existing
`variables:read` / `variables:write` pair. Concretely:

- **WIT** (`crates/yosh-plugin-api/wit/yosh-plugin.wit`): new
  `interface files { ... }`; `world plugin-world` gains
  `import files`.
- **Capability bits** (`crates/yosh-plugin-api/src/lib.rs`):
  `CAP_FILES_READ = 0x100`, `CAP_FILES_WRITE = 0x200`. Added to
  `CAP_ALL`. Enum variants `Capability::FilesRead` /
  `Capability::FilesWrite`. String mappings `"files:read"` /
  `"files:write"` in both `parse_capability` and `Capability::as_str` /
  `Capability::to_bitflag`.
- **Config parser** (`src/plugin/config.rs::capability_from_str`): two
  new arms mapping the strings to the new bits.
- **Host implementations** (`src/plugin/host.rs`): one real impl + one
  deny stub per WIT function. Both gated by the `env_mut().is_none()`
  guard for metadata-contract enforcement.
- **Linker wiring** (`src/plugin/linker.rs`): a new
  `yosh:plugin/files@0.1.0` instance block, real-vs-deny chosen per
  function group based on bitflag.
- **SDK helpers** (`crates/yosh-plugin-sdk/src/lib.rs`):
  `read_file` / `read_to_string` / `read_dir` / `metadata` / `exists`
  on the read side; `write_file` / `write_string` / `append_file` /
  `create_dir` / `create_dir_all` / `remove_file` / `remove_dir` /
  `remove_dir_all` on the write side.

The existing `filesystem` capability (with `cwd` / `set-cwd`) is
untouched — it remains for backward compatibility with v0.1.5 plugins
already in the wild.

## 2. WIT Interface

Added to `crates/yosh-plugin-api/wit/yosh-plugin.wit`:

```wit
interface files {
    use types.{error-code};

    /// Lightweight stat. Extended in the future by adding new
    /// functions, never by changing this record's shape.
    record file-stat {
        is-file: bool,
        is-dir: bool,
        is-symlink: bool,
        size: u64,
        /// mtime as seconds since UNIX epoch. -1 if unavailable.
        mtime-secs: s64,
    }

    record dir-entry {
        name: string,        // basename only, not full path
        is-file: bool,
        is-dir: bool,
        is-symlink: bool,
    }

    // Read group — gated by CAP_FILES_READ
    read-file: func(path: string) -> result<list<u8>, error-code>;
    read-dir:  func(path: string) -> result<list<dir-entry>, error-code>;
    metadata:  func(path: string) -> result<file-stat, error-code>;

    // Write group — gated by CAP_FILES_WRITE
    write-file:  func(path: string, data: list<u8>) -> result<_, error-code>;
    append-file: func(path: string, data: list<u8>) -> result<_, error-code>;
    create-dir:  func(path: string, recursive: bool) -> result<_, error-code>;
    remove-file: func(path: string) -> result<_, error-code>;
    remove-dir:  func(path: string, recursive: bool) -> result<_, error-code>;
}

world plugin-world {
    import variables;
    import filesystem;
    import files;       // ← new
    import io;

    export plugin;
    export hooks;
}
```

Design notes:

- **Reuse `error-code`** rather than introducing file-specific errors.
  Permission denied collapses into `io-failed`, matching POSIX where
  `EACCES` is just another I/O error from the caller's POV.
- **Path is a plain `string`**. Absolute or relative; relative paths
  resolve against the host process's `current_dir`.
- **Symlinks follow by default** (matches `std::fs::read` /
  `std::fs::metadata`). A `symlink_metadata` equivalent can be added
  later if needed.
- **mtime is `s64` seconds**. Avoids cross-platform `SystemTime`
  representation issues. `-1` signals "unavailable".
- **No size cap on `read-file`**. If a plugin asks for a 10 GiB file,
  it gets a 10 GiB allocation. Limits belong with the broader
  fuel/memory work tracked in TODO.md.
- **`read-dir` is non-recursive**. Plugins can recurse themselves.
- **`dir-entry.name` is the basename only**, not a full path. Plugins
  reconstruct full paths via `join`.

## 3. Capability Bits & String Parsing

`crates/yosh-plugin-api/src/lib.rs`:

```rust
pub const CAP_VARIABLES_READ:  u32 = 0x01;
pub const CAP_VARIABLES_WRITE: u32 = 0x02;
pub const CAP_FILESYSTEM:      u32 = 0x04;
pub const CAP_IO:              u32 = 0x08;
pub const CAP_HOOK_PRE_EXEC:   u32 = 0x10;
pub const CAP_HOOK_POST_EXEC:  u32 = 0x20;
pub const CAP_HOOK_ON_CD:      u32 = 0x40;
pub const CAP_HOOK_PRE_PROMPT: u32 = 0x80;
pub const CAP_FILES_READ:      u32 = 0x100;   // new
pub const CAP_FILES_WRITE:     u32 = 0x200;   // new

pub const CAP_ALL: u32 =
    /* existing seven bits */
    | CAP_FILES_READ
    | CAP_FILES_WRITE;

pub enum Capability {
    /* existing variants */
    FilesRead,
    FilesWrite,
}
```

String / bitflag mappings extended in `parse_capability`,
`Capability::as_str`, `Capability::to_bitflag`, and the host-side
`src/plugin/config.rs::capability_from_str`:

```text
"files:read"  ↔ Capability::FilesRead  ↔ CAP_FILES_READ
"files:write" ↔ Capability::FilesWrite ↔ CAP_FILES_WRITE
```

`u32` still has 22 spare bits — no need to widen.

## 4. Host Implementations

Added to `src/plugin/host.rs` alongside the existing
`host_filesystem_*` block. Every real impl starts with the
`env_mut().is_none()` guard so the metadata-contract holds.

```rust
// ── yosh:plugin/files host imports ───────────────────────────────────

pub(super) fn host_files_read_file(
    ctx: &mut HostContext,
    path: String,
) -> Result<Vec<u8>, ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    match std::fs::read(&path) {
        Ok(bytes) => Ok(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(ErrorCode::NotFound)
        }
        Err(_) => Err(ErrorCode::IoFailed),
    }
}

pub(super) fn host_files_read_dir(/* fs::read_dir → Vec<DirEntry>      */) {}
pub(super) fn host_files_metadata(/*  fs::metadata  → FileStat         */) {}
pub(super) fn host_files_write_file(/* fs::write                       */) {}
pub(super) fn host_files_append_file(/* OpenOptions { append, create } */) {}
pub(super) fn host_files_create_dir(/* fs::create_dir vs create_dir_all */) {}
pub(super) fn host_files_remove_file(/* fs::remove_file                */) {}
pub(super) fn host_files_remove_dir(/* remove_dir vs remove_dir_all    */) {}

// One deny stub per real impl, all returning Err(Denied):
pub(super) fn deny_files_read_file(/* … */) -> Result<Vec<u8>, ErrorCode> {
    Err(ErrorCode::Denied)
}
// (and seven more)
```

Error mapping (uniform across all 8 functions):

| Condition                                | `ErrorCode`           |
|------------------------------------------|-----------------------|
| `env_mut()` is `None` (metadata context) | `Denied`              |
| Empty path string                        | `InvalidArgument`     |
| `io::ErrorKind::NotFound`                | `NotFound`            |
| Any other `io::Error`                    | `IoFailed`            |

Generated types `DirEntry` and `FileStat` come from
`super::generated::yosh::plugin::files::*` (wit-bindgen output) and are
imported into `host.rs` the same way `IoStream` already is.

## 5. Linker Wiring

Added to `src/plugin/linker.rs`, immediately after the existing
`yosh:plugin/filesystem` block:

```rust
let mut files = linker.instance("yosh:plugin/files@0.1.0")?;

// Read group (CAP_FILES_READ)
if has(allowed, CAP_FILES_READ) {
    files.func_wrap("read-file", |mut store, (path,): (String,)| {
        Ok((host_files_read_file(store.data_mut(), path),))
    })?;
    files.func_wrap("read-dir", /* host_files_read_dir */)?;
    files.func_wrap("metadata", /* host_files_metadata */)?;
} else {
    files.func_wrap("read-file", /* deny_files_read_file */)?;
    files.func_wrap("read-dir",  /* deny_files_read_dir  */)?;
    files.func_wrap("metadata",  /* deny_files_metadata  */)?;
}

// Write group (CAP_FILES_WRITE)
if has(allowed, CAP_FILES_WRITE) {
    files.func_wrap("write-file",  /* host_files_write_file  */)?;
    files.func_wrap("append-file", /* host_files_append_file */)?;
    files.func_wrap("create-dir",  /* host_files_create_dir  */)?;
    files.func_wrap("remove-file", /* host_files_remove_file */)?;
    files.func_wrap("remove-dir",  /* host_files_remove_dir  */)?;
} else {
    /* five deny stubs */
}
```

Read and write are independent bits — granting only `files:read` leaves
all five write functions on deny stubs, and vice versa. Symmetric to
the `variables:read` / `variables:write` split.

`use` additions: `super::host::{host_files_*, deny_files_*}` and
`yosh_plugin_api::{CAP_FILES_READ, CAP_FILES_WRITE}`.

The existing `linker_construction_smoke` test exercises both `0` and
`CAP_ALL`. Updating `CAP_ALL` to include the two new bits (§3) makes
the new wiring covered automatically.

## 6. SDK Helpers

`crates/yosh-plugin-sdk/src/lib.rs` gains:

```rust
pub use self::yosh::plugin::files as host_files;
pub use self::yosh::plugin::files::{DirEntry, FileStat};

// Read API
pub fn read_file(path: &str) -> Result<Vec<u8>, ErrorCode> {
    host_files::read_file(path)
}

pub fn read_to_string(path: &str) -> Result<String, ErrorCode> {
    let bytes = host_files::read_file(path)?;
    String::from_utf8(bytes).map_err(|_| ErrorCode::InvalidArgument)
}

pub fn read_dir(path: &str) -> Result<Vec<DirEntry>, ErrorCode> {
    host_files::read_dir(path)
}

pub fn metadata(path: &str) -> Result<FileStat, ErrorCode> {
    host_files::metadata(path)
}

pub fn exists(path: &str) -> bool {
    host_files::metadata(path).is_ok()
}

// Write API
pub fn write_file(path: &str, data: &[u8])     -> Result<(), ErrorCode> { … }
pub fn write_string(path: &str, s: &str)       -> Result<(), ErrorCode> { … }
pub fn append_file(path: &str, data: &[u8])    -> Result<(), ErrorCode> { … }
pub fn create_dir(path: &str)                  -> Result<(), ErrorCode> { … }
pub fn create_dir_all(path: &str)              -> Result<(), ErrorCode> { … }
pub fn remove_file(path: &str)                 -> Result<(), ErrorCode> { … }
pub fn remove_dir(path: &str)                  -> Result<(), ErrorCode> { … }
pub fn remove_dir_all(path: &str)              -> Result<(), ErrorCode> { … }
```

Names mirror `std::fs` — `read_to_string`, `create_dir_all`,
`remove_dir_all` — so plugin authors get zero-learning-cost ergonomics.
The raw `host_files` is also re-exported for cases the helpers don't
cover (e.g., manipulating raw `DirEntry`).

## 7. Default-Deny Layers

Three independent layers must all line up before a file operation
runs:

1. **WIT layer**: if the plugin's source omits `import files`, the
   compiled wasm has no symbols for the file functions at all.
2. **Linker layer**: when the granted bitfield lacks
   `CAP_FILES_READ` / `CAP_FILES_WRITE`, the corresponding functions
   are bound to deny stubs that return `Err(Denied)` immediately.
3. **Host layer**: even the real implementations short-circuit to
   `Err(Denied)` while `env` is null — this enforces the
   metadata-contract (no host calls during the single `metadata()`
   invocation at startup).

Capability negotiation is unchanged: the plugin declares
`required-capabilities = ["files:read"]` in `plugin-info`, the user
allows `capabilities = ["files:read"]` in `plugins.toml`, and the
effective grant is the bitwise AND. Omitting `capabilities` in
`plugins.toml` still means "trust everything the plugin asked for"
— the same shorthand as today.

## 8. Tests

**Host unit tests** (`src/plugin/host.rs::tests`):

- Metadata-contract: one `*_denied_when_env_null` per new function
  (8 total), modeled on `metadata_contract_real_filesystem_cwd_denied_…`.
- Read happy paths against a `tempfile::tempdir`:
  - `host_files_read_file_roundtrip` (write + read returns same bytes)
  - `host_files_read_dir_returns_entries`
  - `host_files_metadata_distinguishes_file_and_dir`
- Error mapping:
  - `host_files_read_file_returns_not_found_for_missing_path`
  - `host_files_read_file_invalid_argument_on_empty_path`
  - `host_files_remove_dir_io_failed_on_nonempty_without_recursive`
- Write semantics:
  - `host_files_append_file_appends`
  - `host_files_create_dir_all_creates_intermediate_dirs`
  - `host_files_remove_dir_recursive_removes_subtree`

**Linker tests** (`src/plugin/linker.rs::tests`): `linker_construction_smoke`
auto-covers the new wiring once `CAP_ALL` includes the two new bits.

**Integration tests** (`tests/plugin.rs` + `tests/plugins/test_plugin/`):

Add two commands to `test_plugin`:
- `read-file <path>` → calls `sdk::read_to_string(path)`, prints result
  or maps the error to a non-zero exit code
- `write-file <path> <content>` → calls `sdk::write_string`

Add new test cases:
- `t11_files_read_granted_works`: with `files:read` allowed, `read-file`
  returns the expected bytes
- `t12_files_read_denied_returns_error`: without the cap, exit code
  reflects `Denied`
- `t13_files_write_granted_works`: write-then-read roundtrip
- `t14_files_write_denied`: write returns `Denied`
- `t15_files_read_only_blocks_write`: granting only `files:read` leaves
  the write functions on deny stubs (independence check)

`tests/plugins/test_plugin/Cargo.toml` (the wit metadata) gets
`required-capabilities` extended with `files:read`, `files:write`. Per
the workspace caveat in CLAUDE.md, the test plugin is rebuilt with
`cargo component build -p test_plugin --target wasm32-wasip2 --release`,
and integration tests run with `--features test-helpers`.

**Out of scope**: e2e tests (plugins are excluded from `e2e/`),
benchmarks (file I/O cost dwarfs wasmtime call cost — meaningless to
bench here).

## 9. Versioning & Compat

- WIT package version stays at `yosh:plugin@0.1.0` — adding new
  interfaces/imports is a *minor* surface change. Existing plugins that
  do not `import files` continue to instantiate cleanly.
- `CAP_ALL` widens; this only matters to call sites that build a
  capability mask from `CAP_ALL`, all of which live inside this repo.
- `plugins.toml` parser silently ignores unknown capability strings
  (existing behavior in `capabilities_from_strs`), so older yosh
  binaries reading a v0.2.x `plugins.toml` with `"files:read"` will
  drop the unknown bit — degraded but not broken. Document this in the
  release notes.

## 10. Open Questions / Future Work

- Path scoping (per-plugin allowlist) is deliberately deferred. If a
  user complains about over-broad permissions, revisit by adding a
  `paths = [...]` allowlist that intersects with the bitflag at the
  host layer.
- `symlink_metadata` (don't-follow variant) is not exposed. Add as a
  separate WIT function if a real use case appears.
- File I/O quotas / fuel metering / per-call size caps belong with the
  broader runtime-limits work in TODO.md "Plugin runtime limits".
- A future `files:exec` / `files:chmod` capability could exist; not
  included here because no driving use case exists.
