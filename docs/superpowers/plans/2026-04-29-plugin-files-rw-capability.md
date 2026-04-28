# Plugin `files:read` / `files:write` Capability — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship two new plugin capabilities — `files:read` and `files:write` — that expose whole-file read/write, directory listing, metadata, and basic mkdir/rm operations to WASM plugins, gated by the existing capability allowlist mechanism.

**Architecture:** New WIT `interface files` with 8 functions, two new `CAP_*` bitflag constants, real and deny-stub host implementations following the established `variables:*` pattern. SDK helpers mirror `std::fs`. Integration tests via a `read-file`/`write-file` command added to the existing `test_plugin`.

**Tech Stack:** Rust 2024, wasmtime component model + wit-bindgen, cargo-component for guest builds, `tempfile` crate for fs fixtures, existing `tests/plugin.rs` `test-helpers` feature.

**Spec:** `docs/superpowers/specs/2026-04-29-plugin-files-rw-capability-design.md`

---

## File Structure

| File | Purpose | Action |
|---|---|---|
| `crates/yosh-plugin-api/wit/yosh-plugin.wit` | WIT contract: add `interface files` and `import files` to world | Modify |
| `crates/yosh-plugin-api/src/lib.rs` | Capability bitflags, enum variants, string parsing | Modify |
| `src/plugin/config.rs` | `capability_from_str` arms for new strings | Modify |
| `src/plugin/host.rs` | 8 real impls + 8 deny stubs + 8 metadata-contract tests | Modify |
| `src/plugin/linker.rs` | Wire `yosh:plugin/files@0.1.0` instance, real-vs-deny per cap bit | Modify |
| `crates/yosh-plugin-sdk/src/lib.rs` | Re-export `host_files`/`DirEntry`/`FileStat` + 14 typed wrappers | Modify |
| `tests/plugins/test_plugin/src/lib.rs` | Add `read-file`/`write-file` commands and the new required caps | Modify |
| `tests/plugin.rs` | `t15`–`t19` integration tests for the new caps | Modify |

Each task in this plan produces a self-contained, committable change. Tasks 3 and 4 are paired: Task 3 introduces the WIT (which forces every test plugin to declare these imports), so it must also wire all 8 host stubs at once or test plugin instantiation breaks. Task 4 then upgrades the deny stubs to real implementations behind the read/write capability bits.

---

## Task 1: Capability bits and string parsing in `yosh-plugin-api`

**Files:**
- Modify: `crates/yosh-plugin-api/src/lib.rs`

- [ ] **Step 1.1: Write the failing test for new capability strings**

Append to the `tests` module in `crates/yosh-plugin-api/src/lib.rs`:

```rust
    #[test]
    fn parse_files_capabilities() {
        assert_eq!(parse_capability("files:read"), Some(Capability::FilesRead));
        assert_eq!(parse_capability("files:write"), Some(Capability::FilesWrite));
    }

    #[test]
    fn files_capabilities_round_trip() {
        for cap in [Capability::FilesRead, Capability::FilesWrite] {
            assert_eq!(parse_capability(cap.as_str()), Some(cap));
        }
    }

    #[test]
    fn cap_all_includes_files_bits() {
        assert_eq!(CAP_ALL & CAP_FILES_READ, CAP_FILES_READ);
        assert_eq!(CAP_ALL & CAP_FILES_WRITE, CAP_FILES_WRITE);
    }
```

- [ ] **Step 1.2: Run the new tests to verify they fail**

Run: `cargo test -p yosh-plugin-api parse_files_capabilities files_capabilities_round_trip cap_all_includes_files_bits`
Expected: compile error — `Capability::FilesRead`, `Capability::FilesWrite`, `CAP_FILES_READ`, `CAP_FILES_WRITE` are undefined.

- [ ] **Step 1.3: Add the two new constants**

Edit `crates/yosh-plugin-api/src/lib.rs`. After the existing `CAP_HOOK_PRE_PROMPT` line, add:

```rust
pub const CAP_FILES_READ:      u32 = 0x100;
pub const CAP_FILES_WRITE:     u32 = 0x200;
```

- [ ] **Step 1.4: Extend `CAP_ALL` to include the new bits**

Replace the existing `CAP_ALL` definition with:

```rust
pub const CAP_ALL: u32 = CAP_VARIABLES_READ
    | CAP_VARIABLES_WRITE
    | CAP_FILESYSTEM
    | CAP_IO
    | CAP_HOOK_PRE_EXEC
    | CAP_HOOK_POST_EXEC
    | CAP_HOOK_ON_CD
    | CAP_HOOK_PRE_PROMPT
    | CAP_FILES_READ
    | CAP_FILES_WRITE;
```

- [ ] **Step 1.5: Add the two new enum variants**

Replace the existing `Capability` enum with:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    VariablesRead,
    VariablesWrite,
    Filesystem,
    Io,
    HookPreExec,
    HookPostExec,
    HookOnCd,
    HookPrePrompt,
    FilesRead,
    FilesWrite,
}
```

- [ ] **Step 1.6: Wire the new variants into `to_bitflag` and `as_str`**

Add new arms to both methods:

```rust
    pub fn to_bitflag(self) -> u32 {
        match self {
            // … existing arms …
            Capability::FilesRead      => CAP_FILES_READ,
            Capability::FilesWrite     => CAP_FILES_WRITE,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            // … existing arms …
            Capability::FilesRead      => "files:read",
            Capability::FilesWrite     => "files:write",
        }
    }
```

- [ ] **Step 1.7: Add the new arms to `parse_capability`**

```rust
pub fn parse_capability(s: &str) -> Option<Capability> {
    Some(match s {
        // … existing arms …
        "files:read"       => Capability::FilesRead,
        "files:write"      => Capability::FilesWrite,
        _ => return None,
    })
}
```

- [ ] **Step 1.8: Update the existing `cap_all_covers_every_variant` test**

Replace the slice contents to include the two new variants:

```rust
    #[test]
    fn cap_all_covers_every_variant() {
        let bits = capabilities_to_bitflags(&[
            Capability::VariablesRead,
            Capability::VariablesWrite,
            Capability::Filesystem,
            Capability::Io,
            Capability::HookPreExec,
            Capability::HookPostExec,
            Capability::HookOnCd,
            Capability::HookPrePrompt,
            Capability::FilesRead,
            Capability::FilesWrite,
        ]);
        assert_eq!(bits, CAP_ALL);
    }
```

- [ ] **Step 1.9: Run the full api crate test suite**

Run: `cargo test -p yosh-plugin-api`
Expected: all tests pass, including the four new/updated cases.

- [ ] **Step 1.10: Commit**

```bash
git add crates/yosh-plugin-api/src/lib.rs
git commit -m "feat(plugin-api): add CAP_FILES_READ / CAP_FILES_WRITE capability bits"
```

---

## Task 2: Config parser arms in `src/plugin/config.rs`

**Files:**
- Modify: `src/plugin/config.rs`

- [ ] **Step 2.1: Write the failing test**

Append to the `tests` module in `src/plugin/config.rs`:

```rust
    #[test]
    fn parse_files_capability_strings_to_bitflags() {
        use yosh_plugin_api::*;
        assert_eq!(capability_from_str("files:read"),  Some(CAP_FILES_READ));
        assert_eq!(capability_from_str("files:write"), Some(CAP_FILES_WRITE));
    }
```

- [ ] **Step 2.2: Run the test to verify it fails**

Run: `cargo test -p yosh parse_files_capability_strings_to_bitflags`
Expected: FAIL — `capability_from_str` returns `None` for the two new strings.

- [ ] **Step 2.3: Add the two new arms in `capability_from_str`**

In `src/plugin/config.rs`, in the `match s` body of `capability_from_str`, add (just before the `_ => None` arm):

```rust
        "files:read"       => Some(yosh_plugin_api::CAP_FILES_READ),
        "files:write"      => Some(yosh_plugin_api::CAP_FILES_WRITE),
```

- [ ] **Step 2.4: Run the test to verify it passes**

Run: `cargo test -p yosh parse_files_capability_strings_to_bitflags`
Expected: PASS.

- [ ] **Step 2.5: Run the full config test module**

Run: `cargo test -p yosh -- plugin::config`
Expected: all tests pass.

- [ ] **Step 2.6: Commit**

```bash
git add src/plugin/config.rs
git commit -m "feat(plugin/config): parse files:read / files:write capability strings"
```

---

## Task 3: WIT interface, host deny stubs, linker wiring (all-deny)

This task brings the WIT interface online with all 8 functions registered as **deny stubs**. Real implementations land in Task 4. The all-deny landing keeps the test plugin instantiable (every WIT import has a host binding) and isolates the WIT change from the I/O semantics work.

**Files:**
- Modify: `crates/yosh-plugin-api/wit/yosh-plugin.wit`
- Modify: `src/plugin/host.rs`
- Modify: `src/plugin/linker.rs`

- [ ] **Step 3.1: Add the `files` interface to the WIT**

Edit `crates/yosh-plugin-api/wit/yosh-plugin.wit`. After the existing `interface io { ... }` block (and before `interface plugin { ... }`), insert:

```wit
interface files {
    use types.{error-code};

    record file-stat {
        is-file: bool,
        is-dir: bool,
        is-symlink: bool,
        size: u64,
        mtime-secs: s64,
    }

    record dir-entry {
        name: string,
        is-file: bool,
        is-dir: bool,
        is-symlink: bool,
    }

    read-file: func(path: string) -> result<list<u8>, error-code>;
    read-dir:  func(path: string) -> result<list<dir-entry>, error-code>;
    metadata:  func(path: string) -> result<file-stat, error-code>;

    write-file:  func(path: string, data: list<u8>) -> result<_, error-code>;
    append-file: func(path: string, data: list<u8>) -> result<_, error-code>;
    create-dir:  func(path: string, recursive: bool) -> result<_, error-code>;
    remove-file: func(path: string) -> result<_, error-code>;
    remove-dir:  func(path: string, recursive: bool) -> result<_, error-code>;
}
```

- [ ] **Step 3.2: Add `import files` to the world**

In the same file, replace the `world plugin-world { ... }` block with:

```wit
world plugin-world {
    import variables;
    import filesystem;
    import files;
    import io;

    export plugin;
    export hooks;
}
```

- [ ] **Step 3.3: Run cargo build to regenerate bindings**

Run: `cargo build -p yosh`
Expected: build succeeds. Both wasmtime bindgen (in `src/plugin/mod.rs`) and wit-bindgen (in `crates/yosh-plugin-sdk/src/lib.rs`) regenerate to include the new interface; the new types exist as `super::generated::yosh::plugin::files::{DirEntry, FileStat}` on the host side.

- [ ] **Step 3.4: Add the 8 deny stubs to `src/plugin/host.rs`**

Append to `src/plugin/host.rs` (after the existing `// ── yosh:plugin/io host imports ──` block):

```rust
// ── yosh:plugin/files host imports (deny stubs only — real impls in Task 4) ──

use super::generated::yosh::plugin::files::{DirEntry, FileStat};

pub(super) fn deny_files_read_file(
    _ctx: &mut HostContext,
    _path: String,
) -> Result<Vec<u8>, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_read_dir(
    _ctx: &mut HostContext,
    _path: String,
) -> Result<Vec<DirEntry>, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_metadata(
    _ctx: &mut HostContext,
    _path: String,
) -> Result<FileStat, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_write_file(
    _ctx: &mut HostContext,
    _path: String,
    _data: Vec<u8>,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_append_file(
    _ctx: &mut HostContext,
    _path: String,
    _data: Vec<u8>,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_create_dir(
    _ctx: &mut HostContext,
    _path: String,
    _recursive: bool,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_remove_file(
    _ctx: &mut HostContext,
    _path: String,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_remove_dir(
    _ctx: &mut HostContext,
    _path: String,
    _recursive: bool,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}
```

- [ ] **Step 3.5: Wire the `yosh:plugin/files` instance in `src/plugin/linker.rs` (all deny)**

Add the new constants to the `use yosh_plugin_api::{...}` import line:

```rust
use yosh_plugin_api::{
    CAP_FILES_READ, CAP_FILES_WRITE, CAP_FILESYSTEM, CAP_IO,
    CAP_VARIABLES_READ, CAP_VARIABLES_WRITE,
};
```

Add the new helpers to the `use super::host::{...}` import line:

```rust
use super::host::{
    HostContext,
    deny_files_append_file, deny_files_create_dir, deny_files_metadata,
    deny_files_read_dir, deny_files_read_file, deny_files_remove_dir,
    deny_files_remove_file, deny_files_write_file,
    deny_filesystem_cwd, deny_filesystem_set_cwd, deny_io_write,
    deny_variables_export_env, deny_variables_get, deny_variables_set,
    host_filesystem_cwd, host_filesystem_set_cwd, host_io_write,
    host_variables_export_env, host_variables_get, host_variables_set,
};
```

(Real `host_files_*` helpers are added in Task 4; for now only the deny variants are imported.)

After the `// ── yosh:plugin/io ──` block in `build_linker`, add:

```rust
    // ── yosh:plugin/files ───────────────────────────────────────────────
    let mut files = linker.instance("yosh:plugin/files@0.1.0")?;

    // Read group — gated by CAP_FILES_READ
    if has(allowed, CAP_FILES_READ) {
        // Real impls land in Task 4; until then, granted reads also deny.
        files.func_wrap("read-file", |mut store, (path,): (String,)| {
            Ok((deny_files_read_file(store.data_mut(), path),))
        })?;
        files.func_wrap("read-dir", |mut store, (path,): (String,)| {
            Ok((deny_files_read_dir(store.data_mut(), path),))
        })?;
        files.func_wrap("metadata", |mut store, (path,): (String,)| {
            Ok((deny_files_metadata(store.data_mut(), path),))
        })?;
    } else {
        files.func_wrap("read-file", |mut store, (path,): (String,)| {
            Ok((deny_files_read_file(store.data_mut(), path),))
        })?;
        files.func_wrap("read-dir", |mut store, (path,): (String,)| {
            Ok((deny_files_read_dir(store.data_mut(), path),))
        })?;
        files.func_wrap("metadata", |mut store, (path,): (String,)| {
            Ok((deny_files_metadata(store.data_mut(), path),))
        })?;
    }

    // Write group — gated by CAP_FILES_WRITE
    if has(allowed, CAP_FILES_WRITE) {
        files.func_wrap("write-file", |mut store, (path, data): (String, Vec<u8>)| {
            Ok((deny_files_write_file(store.data_mut(), path, data),))
        })?;
        files.func_wrap("append-file", |mut store, (path, data): (String, Vec<u8>)| {
            Ok((deny_files_append_file(store.data_mut(), path, data),))
        })?;
        files.func_wrap("create-dir", |mut store, (path, recursive): (String, bool)| {
            Ok((deny_files_create_dir(store.data_mut(), path, recursive),))
        })?;
        files.func_wrap("remove-file", |mut store, (path,): (String,)| {
            Ok((deny_files_remove_file(store.data_mut(), path),))
        })?;
        files.func_wrap("remove-dir", |mut store, (path, recursive): (String, bool)| {
            Ok((deny_files_remove_dir(store.data_mut(), path, recursive),))
        })?;
    } else {
        files.func_wrap("write-file", |mut store, (path, data): (String, Vec<u8>)| {
            Ok((deny_files_write_file(store.data_mut(), path, data),))
        })?;
        files.func_wrap("append-file", |mut store, (path, data): (String, Vec<u8>)| {
            Ok((deny_files_append_file(store.data_mut(), path, data),))
        })?;
        files.func_wrap("create-dir", |mut store, (path, recursive): (String, bool)| {
            Ok((deny_files_create_dir(store.data_mut(), path, recursive),))
        })?;
        files.func_wrap("remove-file", |mut store, (path,): (String,)| {
            Ok((deny_files_remove_file(store.data_mut(), path),))
        })?;
        files.func_wrap("remove-dir", |mut store, (path, recursive): (String, bool)| {
            Ok((deny_files_remove_dir(store.data_mut(), path, recursive),))
        })?;
    }
```

(The redundant deny-on-grant branches are intentional placeholders for Task 4.)

- [ ] **Step 3.6: Run the linker smoke test**

Run: `cargo test -p yosh -- plugin::linker`
Expected: `linker_construction_smoke` passes. Building with both `0` and `CAP_ALL` exercises the new wiring with both branches.

- [ ] **Step 3.7: Rebuild test_plugin and trap_plugin to validate WIT compatibility**

Run:
```bash
cargo component build -p test_plugin --target wasm32-wasip2 --release
cargo component build -p trap_plugin --target wasm32-wasip2 --release
```
Expected: both succeed. The new `import files` is picked up by wit-bindgen but neither plugin actually calls those functions yet, so generated guest code is just unused stubs.

- [ ] **Step 3.8: Run plugin integration tests to verify nothing broke**

Run: `cargo test --features test-helpers --test plugin`
Expected: all existing `t01`–`t14` tests pass (the WIT addition is transparent to plugins that don't use it).

- [ ] **Step 3.9: Commit**

```bash
git add crates/yosh-plugin-api/wit/yosh-plugin.wit src/plugin/host.rs src/plugin/linker.rs
git commit -m "feat(plugin): add files WIT interface with all-deny host wiring"
```

---

## Task 4: Real host implementations + metadata-contract tests

This task replaces the deny-on-grant branches from Task 3 with real implementations and adds the metadata-contract unit tests for all 8 functions.

**Files:**
- Modify: `src/plugin/host.rs`
- Modify: `src/plugin/linker.rs`

- [ ] **Step 4.1: Write the 8 failing metadata-contract tests**

Append to `src/plugin/host.rs`, inside the existing `mod tests { ... }` block:

```rust
    #[test]
    fn metadata_contract_real_files_read_file_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_files_read_file(&mut ctx, "/tmp/anything".into());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_files_read_dir_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_files_read_dir(&mut ctx, "/tmp".into());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_files_metadata_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_files_metadata(&mut ctx, "/tmp".into());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_files_write_file_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_files_write_file(&mut ctx, "/tmp/x".into(), b"hi".to_vec());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_files_append_file_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_files_append_file(&mut ctx, "/tmp/x".into(), b"hi".to_vec());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_files_create_dir_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_files_create_dir(&mut ctx, "/tmp/newdir".into(), true);
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_files_remove_file_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_files_remove_file(&mut ctx, "/tmp/x".into());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_files_remove_dir_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_files_remove_dir(&mut ctx, "/tmp/newdir".into(), true);
        assert_eq!(result, Err(ErrorCode::Denied));
    }
```

- [ ] **Step 4.2: Run the new tests to verify they fail**

Run: `cargo test -p yosh -- plugin::host::tests::metadata_contract_real_files`
Expected: compile error — none of `host_files_*` functions exist yet.

- [ ] **Step 4.3: Add the 8 real host implementations to `src/plugin/host.rs`**

Add these above the existing `// ── yosh:plugin/files host imports (deny stubs only…) ──` block, replacing its title so the section reads `// ── yosh:plugin/files host imports ──`:

```rust
// ── yosh:plugin/files host imports ───────────────────────────────────

use super::generated::yosh::plugin::files::{DirEntry, FileStat};
use std::time::UNIX_EPOCH;

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
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ErrorCode::NotFound),
        Err(_) => Err(ErrorCode::IoFailed),
    }
}

pub(super) fn host_files_read_dir(
    ctx: &mut HostContext,
    path: String,
) -> Result<Vec<DirEntry>, ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let iter = match std::fs::read_dir(&path) {
        Ok(i) => i,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(ErrorCode::NotFound),
        Err(_) => return Err(ErrorCode::IoFailed),
    };
    let mut out = Vec::new();
    for entry in iter {
        let entry = entry.map_err(|_| ErrorCode::IoFailed)?;
        let ft = entry.file_type().map_err(|_| ErrorCode::IoFailed)?;
        out.push(DirEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_file: ft.is_file(),
            is_dir: ft.is_dir(),
            is_symlink: ft.is_symlink(),
        });
    }
    Ok(out)
}

pub(super) fn host_files_metadata(
    ctx: &mut HostContext,
    path: String,
) -> Result<FileStat, ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let md = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(ErrorCode::NotFound),
        Err(_) => return Err(ErrorCode::IoFailed),
    };
    let mtime_secs = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(-1);
    Ok(FileStat {
        is_file: md.is_file(),
        is_dir: md.is_dir(),
        is_symlink: md.file_type().is_symlink(),
        size: md.len(),
        mtime_secs,
    })
}

pub(super) fn host_files_write_file(
    ctx: &mut HostContext,
    path: String,
    data: Vec<u8>,
) -> Result<(), ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    std::fs::write(&path, &data).map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn host_files_append_file(
    ctx: &mut HostContext,
    path: String,
    data: Vec<u8>,
) -> Result<(), ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|_| ErrorCode::IoFailed)?;
    f.write_all(&data).map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn host_files_create_dir(
    ctx: &mut HostContext,
    path: String,
    recursive: bool,
) -> Result<(), ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let result = if recursive {
        std::fs::create_dir_all(&path)
    } else {
        std::fs::create_dir(&path)
    };
    result.map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn host_files_remove_file(
    ctx: &mut HostContext,
    path: String,
) -> Result<(), ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ErrorCode::NotFound),
        Err(_) => Err(ErrorCode::IoFailed),
    }
}

pub(super) fn host_files_remove_dir(
    ctx: &mut HostContext,
    path: String,
    recursive: bool,
) -> Result<(), ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let result = if recursive {
        std::fs::remove_dir_all(&path)
    } else {
        std::fs::remove_dir(&path)
    };
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ErrorCode::NotFound),
        Err(_) => Err(ErrorCode::IoFailed),
    }
}
```

Then **delete** the old standalone deny-stubs section header line (`// ── yosh:plugin/files host imports (deny stubs only — real impls in Task 4) ──`) and the duplicate `use super::generated::yosh::plugin::files::{DirEntry, FileStat};` line that Task 3 introduced — the deny stubs themselves stay, just under the unified section header. After this step the file has one section banner and both real impls + deny stubs underneath.

- [ ] **Step 4.4: Update `src/plugin/linker.rs` to call real impls when capabilities are granted**

In `src/plugin/linker.rs`, add the real helpers to the `use super::host::{...}` import line:

```rust
use super::host::{
    HostContext,
    deny_files_append_file, deny_files_create_dir, deny_files_metadata,
    deny_files_read_dir, deny_files_read_file, deny_files_remove_dir,
    deny_files_remove_file, deny_files_write_file,
    deny_filesystem_cwd, deny_filesystem_set_cwd, deny_io_write,
    deny_variables_export_env, deny_variables_get, deny_variables_set,
    host_files_append_file, host_files_create_dir, host_files_metadata,
    host_files_read_dir, host_files_read_file, host_files_remove_dir,
    host_files_remove_file, host_files_write_file,
    host_filesystem_cwd, host_filesystem_set_cwd, host_io_write,
    host_variables_export_env, host_variables_get, host_variables_set,
};
```

Then in the `// ── yosh:plugin/files ──` block, replace the read-group `if has(allowed, CAP_FILES_READ)` branch's body with the real-impl forms:

```rust
    if has(allowed, CAP_FILES_READ) {
        files.func_wrap("read-file", |mut store, (path,): (String,)| {
            Ok((host_files_read_file(store.data_mut(), path),))
        })?;
        files.func_wrap("read-dir", |mut store, (path,): (String,)| {
            Ok((host_files_read_dir(store.data_mut(), path),))
        })?;
        files.func_wrap("metadata", |mut store, (path,): (String,)| {
            Ok((host_files_metadata(store.data_mut(), path),))
        })?;
    } else {
        // … unchanged deny branch …
    }
```

And the write-group `if has(allowed, CAP_FILES_WRITE)` branch:

```rust
    if has(allowed, CAP_FILES_WRITE) {
        files.func_wrap("write-file", |mut store, (path, data): (String, Vec<u8>)| {
            Ok((host_files_write_file(store.data_mut(), path, data),))
        })?;
        files.func_wrap("append-file", |mut store, (path, data): (String, Vec<u8>)| {
            Ok((host_files_append_file(store.data_mut(), path, data),))
        })?;
        files.func_wrap("create-dir", |mut store, (path, recursive): (String, bool)| {
            Ok((host_files_create_dir(store.data_mut(), path, recursive),))
        })?;
        files.func_wrap("remove-file", |mut store, (path,): (String,)| {
            Ok((host_files_remove_file(store.data_mut(), path),))
        })?;
        files.func_wrap("remove-dir", |mut store, (path, recursive): (String, bool)| {
            Ok((host_files_remove_dir(store.data_mut(), path, recursive),))
        })?;
    } else {
        // … unchanged deny branch …
    }
```

- [ ] **Step 4.5: Run all the new metadata-contract tests**

Run: `cargo test -p yosh -- plugin::host::tests::metadata_contract_real_files`
Expected: all 8 PASS.

- [ ] **Step 4.6: Run the full plugin module test suite**

Run: `cargo test -p yosh -- plugin::`
Expected: all unit tests pass — `linker_construction_smoke` covers the real-impl branches via `CAP_ALL`.

- [ ] **Step 4.7: Commit**

```bash
git add src/plugin/host.rs src/plugin/linker.rs
git commit -m "feat(plugin/host): real impls for files:read / files:write hosts"
```

---

## Task 5: SDK helpers in `yosh-plugin-sdk`

**Files:**
- Modify: `crates/yosh-plugin-sdk/src/lib.rs`

- [ ] **Step 5.1: Re-export the generated bindings**

In `crates/yosh-plugin-sdk/src/lib.rs`, after the existing `pub use self::yosh::plugin::variables as host_variables;` line, add:

```rust
pub use self::yosh::plugin::files as host_files;
pub use self::yosh::plugin::files::{DirEntry, FileStat};
```

- [ ] **Step 5.2: Add the read-side typed wrappers**

After the existing `pub fn write_bytes(...)` function, add:

```rust
// ── files:read helpers ───────────────────────────────────────────────

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
```

- [ ] **Step 5.3: Add the write-side typed wrappers**

Immediately after the read-side block, add:

```rust
// ── files:write helpers ──────────────────────────────────────────────

pub fn write_file(path: &str, data: &[u8]) -> Result<(), ErrorCode> {
    host_files::write_file(path, data)
}

pub fn write_string(path: &str, s: &str) -> Result<(), ErrorCode> {
    host_files::write_file(path, s.as_bytes())
}

pub fn append_file(path: &str, data: &[u8]) -> Result<(), ErrorCode> {
    host_files::append_file(path, data)
}

pub fn create_dir(path: &str) -> Result<(), ErrorCode> {
    host_files::create_dir(path, false)
}

pub fn create_dir_all(path: &str) -> Result<(), ErrorCode> {
    host_files::create_dir(path, true)
}

pub fn remove_file(path: &str) -> Result<(), ErrorCode> {
    host_files::remove_file(path)
}

pub fn remove_dir(path: &str) -> Result<(), ErrorCode> {
    host_files::remove_dir(path, false)
}

pub fn remove_dir_all(path: &str) -> Result<(), ErrorCode> {
    host_files::remove_dir(path, true)
}
```

- [ ] **Step 5.4: Build the SDK to verify it compiles**

Run: `cargo build -p yosh-plugin-sdk --target wasm32-wasip2`
Expected: build succeeds. The SDK is a wasm-target crate; host-target builds may complain about wit-bindgen runtime, so use the wasip2 target.

- [ ] **Step 5.5: Commit**

```bash
git add crates/yosh-plugin-sdk/src/lib.rs
git commit -m "feat(plugin-sdk): add typed wrappers for files:read / files:write"
```

---

## Task 6: Add `read-file` / `write-file` commands to `test_plugin`

**Files:**
- Modify: `tests/plugins/test_plugin/src/lib.rs`

- [ ] **Step 6.1: Add the new capabilities to `required_capabilities`**

In `tests/plugins/test_plugin/src/lib.rs`, replace the `required_capabilities` body with:

```rust
    fn required_capabilities(&self) -> &[Capability] {
        &[
            Capability::VariablesRead,
            Capability::VariablesWrite,
            Capability::Io,
            Capability::HookPreExec,
            Capability::HookOnCd,
            Capability::FilesRead,
            Capability::FilesWrite,
        ]
    }
```

- [ ] **Step 6.2: Add the new command names to `commands()`**

Replace the `commands` body with:

```rust
    fn commands(&self) -> &[&'static str] {
        &[
            "test_cmd",
            "echo_var",
            "trap_now",
            "dump_events",
            "set_post_exec_marker",
            "read-file",
            "write-file",
        ]
    }
```

- [ ] **Step 6.3: Add SDK imports for the new helpers**

Replace the existing import line:

```rust
use yosh_plugin_sdk::{Capability, HookName, Plugin, export, get_var, print, set_var};
```

with:

```rust
use yosh_plugin_sdk::{
    Capability, ErrorCode, HookName, Plugin, export, get_var, print, read_file, set_var,
    write_string,
};
```

- [ ] **Step 6.4: Add the two new match arms in `exec()`**

In the `match command` block in `exec`, before the `_ => 127,` fallback arm, insert:

```rust
            "read-file" => {
                let Some(path) = args.first() else { return 1 };
                match read_file(path) {
                    Ok(bytes) => {
                        if bytes == b"YOSH_TEST_CONTENT\n" {
                            0
                        } else {
                            5 // contents mismatch
                        }
                    }
                    Err(ErrorCode::Denied)   => 13,
                    Err(ErrorCode::NotFound) => 4,
                    Err(_)                   => 1,
                }
            }
            "write-file" => {
                let Some(path) = args.first() else { return 1 };
                match write_string(path, "YOSH_TEST_CONTENT\n") {
                    Ok(()) => 0,
                    Err(ErrorCode::Denied) => 13,
                    Err(_) => 1,
                }
            }
```

- [ ] **Step 6.5: Rebuild test_plugin**

Run:
```bash
cargo component build -p test_plugin --target wasm32-wasip2 --release
```
Expected: success. The plugin's `metadata()` now reports the two new required capabilities and the two new command names.

- [ ] **Step 6.6: Verify existing plugin integration tests still pass**

Run: `cargo test --features test-helpers --test plugin -- t01 t02 t03`
Expected: PASS. The plugin now requests two extra capabilities, but `t01` only grants `CAP_VARIABLES_READ | CAP_IO`, which intersects against the request to grant exactly `read + io` (Files caps requested but not granted → effective bits unchanged from before).

- [ ] **Step 6.7: Commit**

```bash
git add tests/plugins/test_plugin/src/lib.rs
git commit -m "test(plugin): add read-file / write-file commands to test_plugin"
```

---

## Task 7: Integration tests `t15`–`t19`

**Files:**
- Modify: `tests/plugin.rs`

The exit-code contract for the new commands (set in Task 6) is:

| Outcome           | Exit code |
|-------------------|-----------|
| Success / contents match | 0 |
| Missing argument  | 1 (also generic IO error) |
| `NotFound`        | 4 |
| Contents mismatch | 5 |
| `Denied`          | 13 |

- [ ] **Step 7.1: Confirm `tempfile` is available**

The root `Cargo.toml` already declares `tempfile = "3"` (used by host unit tests). No dependency addition is needed; the integration test can import `use tempfile;` directly. Skip to Step 7.2.

- [ ] **Step 7.2: Add `t15_files_read_granted_works`**

Add at the bottom of `tests/plugin.rs` (after `t14_linker_construction_smoke_covered_by_unit_test`):

```rust
/// §8.5 — `files:read` granted: real read returns file contents.
///
/// Creates a tempfile with the canonical YOSH_TEST_CONTENT marker, loads
/// the plugin with `files:read` granted, and exercises `read-file`. The
/// plugin returns 0 only when the bytes match exactly, so a passing test
/// verifies both that the host import is wired and that bytes survive
/// the host→guest round trip.
#[test]
fn t15_files_read_granted_works() {
    let _g = lock_test();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("hello.txt");
    std::fs::write(&path, b"YOSH_TEST_CONTENT\n").expect("write fixture");

    let wasm = test_plugin_wasm();
    let mut env = fresh_env();
    let mut mgr = PluginManager::new();

    let allowed = yosh_plugin_api::CAP_FILES_READ;
    test_helpers::load_plugin_with_caps(&mut mgr, &wasm, &mut env, allowed)
        .expect("load test_plugin with files:read");

    let exec = mgr.exec_command(
        &mut env,
        "read-file",
        &[path.to_string_lossy().into_owned()],
    );
    assert!(
        matches!(exec, PluginExec::Handled(0)),
        "read-file with files:read grant must Handled(0), got {:?}",
        exec
    );
}
```

- [ ] **Step 7.3: Add `t16_files_read_denied_returns_error`**

```rust
/// §8.5 — `files:read` not granted: deny stub returns Denied (exit 13).
#[test]
fn t16_files_read_denied_returns_error() {
    let _g = lock_test();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("hello.txt");
    std::fs::write(&path, b"YOSH_TEST_CONTENT\n").expect("write fixture");

    let wasm = test_plugin_wasm();
    let mut env = fresh_env();
    let mut mgr = PluginManager::new();

    // Grant something else so the plugin loads, but NOT files:read.
    let allowed = yosh_plugin_api::CAP_VARIABLES_READ;
    test_helpers::load_plugin_with_caps(&mut mgr, &wasm, &mut env, allowed)
        .expect("load test_plugin without files:read");

    let exec = mgr.exec_command(
        &mut env,
        "read-file",
        &[path.to_string_lossy().into_owned()],
    );
    assert!(
        matches!(exec, PluginExec::Handled(13)),
        "read-file without files:read grant must Handled(13) (Denied), got {:?}",
        exec
    );
}
```

- [ ] **Step 7.4: Add `t17_files_write_granted_works`**

```rust
/// §8.5 — `files:write` granted: real write produces the expected file.
#[test]
fn t17_files_write_granted_works() {
    let _g = lock_test();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("out.txt");

    let wasm = test_plugin_wasm();
    let mut env = fresh_env();
    let mut mgr = PluginManager::new();

    let allowed = yosh_plugin_api::CAP_FILES_WRITE;
    test_helpers::load_plugin_with_caps(&mut mgr, &wasm, &mut env, allowed)
        .expect("load test_plugin with files:write");

    let exec = mgr.exec_command(
        &mut env,
        "write-file",
        &[path.to_string_lossy().into_owned()],
    );
    assert!(
        matches!(exec, PluginExec::Handled(0)),
        "write-file with files:write grant must Handled(0), got {:?}",
        exec
    );

    let written = std::fs::read(&path).expect("read written file");
    assert_eq!(
        written, b"YOSH_TEST_CONTENT\n",
        "host-side read of plugin-written file must match canonical marker",
    );
}
```

- [ ] **Step 7.5: Add `t18_files_write_denied_returns_error`**

```rust
/// §8.5 — `files:write` not granted: deny stub returns Denied (exit 13).
#[test]
fn t18_files_write_denied_returns_error() {
    let _g = lock_test();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("out.txt");

    let wasm = test_plugin_wasm();
    let mut env = fresh_env();
    let mut mgr = PluginManager::new();

    let allowed = yosh_plugin_api::CAP_VARIABLES_READ;
    test_helpers::load_plugin_with_caps(&mut mgr, &wasm, &mut env, allowed)
        .expect("load test_plugin without files:write");

    let exec = mgr.exec_command(
        &mut env,
        "write-file",
        &[path.to_string_lossy().into_owned()],
    );
    assert!(
        matches!(exec, PluginExec::Handled(13)),
        "write-file without files:write grant must Handled(13) (Denied), got {:?}",
        exec
    );

    assert!(
        !path.exists(),
        "deny stub must not create the file"
    );
}
```

- [ ] **Step 7.6: Add `t19_files_read_only_blocks_write`**

```rust
/// §8.5 — Read and write capabilities are independent: granting only
/// `files:read` leaves `files:write` functions on deny stubs.
#[test]
fn t19_files_read_only_blocks_write() {
    let _g = lock_test();
    let dir = tempfile::tempdir().expect("tempdir");
    let read_path = dir.path().join("in.txt");
    let write_path = dir.path().join("out.txt");
    std::fs::write(&read_path, b"YOSH_TEST_CONTENT\n").expect("write fixture");

    let wasm = test_plugin_wasm();
    let mut env = fresh_env();
    let mut mgr = PluginManager::new();

    let allowed = yosh_plugin_api::CAP_FILES_READ; // read only
    test_helpers::load_plugin_with_caps(&mut mgr, &wasm, &mut env, allowed)
        .expect("load test_plugin with files:read only");

    // Read should succeed.
    let r = mgr.exec_command(
        &mut env,
        "read-file",
        &[read_path.to_string_lossy().into_owned()],
    );
    assert!(
        matches!(r, PluginExec::Handled(0)),
        "read-file with files:read grant must Handled(0), got {:?}",
        r
    );

    // Write should be denied.
    let w = mgr.exec_command(
        &mut env,
        "write-file",
        &[write_path.to_string_lossy().into_owned()],
    );
    assert!(
        matches!(w, PluginExec::Handled(13)),
        "write-file without files:write grant must Handled(13), got {:?}",
        w
    );
    assert!(!write_path.exists(), "deny stub must not create the file");
}
```

- [ ] **Step 7.7: Run the new tests**

Run: `cargo test --features test-helpers --test plugin -- t15 t16 t17 t18 t19`
Expected: all 5 PASS.

- [ ] **Step 7.8: Run the full plugin test suite to catch regressions**

Run: `cargo test --features test-helpers --test plugin`
Expected: every test passes — no regressions in `t01`–`t14`.

- [ ] **Step 7.9: Run the workspace tests as a final sanity check**

Run: `cargo test -p yosh`
Expected: all unit tests across `src/plugin/{config,host,linker}.rs` and the api crate pass.

- [ ] **Step 7.10: Commit**

```bash
git add tests/plugin.rs
git commit -m "test(plugin): add t15-t19 integration tests for files:read / files:write"
```

---

## Task 8: Wrap-up — release notes nudge

**Files:**
- (no code changes)

- [ ] **Step 8.1: Confirm CHANGELOG-style notes belong with the release commit, not this branch**

Per spec §9, the user-visible note "older yosh binaries silently drop unknown `files:read`/`files:write` strings from `plugins.toml`" needs to land in release notes when v0.2.x ships. There is no top-level `CHANGELOG.md` in this repo (verify with `ls CHANGELOG.md` — should fail). Therefore: **no changes for this task**, but flag for the release process.

- [ ] **Step 8.2: Verify the spec is fully covered**

Cross-check against `docs/superpowers/specs/2026-04-29-plugin-files-rw-capability-design.md`:
- §1 Architecture Overview → Tasks 1–6
- §2 WIT Interface → Task 3
- §3 Capability Bits & String Parsing → Task 1 + Task 2
- §4 Host Implementations → Tasks 3 (deny stubs) + 4 (real impls + metadata-contract tests)
- §5 Linker Wiring → Tasks 3 + 4
- §6 SDK Helpers → Task 5
- §7 Default-Deny Layers → covered by all of the above (no separate task; the layers exist by construction)
- §8 Tests → Task 4 (host unit) + Task 7 (integration)
- §9 Versioning & Compat → Task 8 (release-time documentation pointer)
- §10 Open Questions → no implementation work

- [ ] **Step 8.3: No commit needed for this task**

Documentation deferred to release time.
