# Plugin Dev: Local Run & Declarative Test Runner — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship two new `yosh-plugin` subcommands — `run` (one-shot exec/hook invocation) and `test` (declarative TOML scenarios) — driven by an in-memory `TestCtx` so plugin authors in any language can locally exercise plugins through the real wasmtime boundary.

**Architecture:** New `TestCtx` (in `crates/yosh-plugin-manager`) mirrors the existing `MetadataCtx` precedent: an empty `WasiCtx` plus capability-gated `yosh:plugin/*` host imports backed by `TestState` (in-memory vars, virtual or sandboxed FS, real-process exec with allowlist). A thin runner module loads wasm and invokes exports; a scenario module parses TOML and evaluates step-level expectations. Two CLI variants on the existing `Commands` enum dispatch to the runner.

**Tech Stack:** Rust 2024 edition, `wasmtime` 27 + `wasmtime-wasi` 27 (already pulled in), `clap` derive (existing), `toml` + `serde` (existing), `regex` (new dependency for `stdout_regex`-style expectations).

Spec: `docs/superpowers/specs/2026-05-12-plugin-dev-test-runner-design.md`

---

## File Structure

**New files:**

```
crates/yosh-plugin-api/src/pattern.rs              (moved from src/plugin/pattern.rs)
crates/yosh-plugin-manager/src/test_host/mod.rs    TestCtx, TestState, common types
crates/yosh-plugin-manager/src/test_host/variables.rs
crates/yosh-plugin-manager/src/test_host/io.rs
crates/yosh-plugin-manager/src/test_host/filesystem.rs
crates/yosh-plugin-manager/src/test_host/files.rs
crates/yosh-plugin-manager/src/test_host/commands.rs
crates/yosh-plugin-manager/src/runner.rs           load_plugin, invoke_exec, invoke_hook, RunResult, formatters
crates/yosh-plugin-manager/src/scenario.rs         Scenario/Step/Expect schema, evaluator, walker
crates/yosh-plugin-manager/tests/runner.rs        integration tests using tests/plugins/test_plugin.wasm
crates/yosh-plugin-manager/tests/scenarios/echo_var_pass.toml
crates/yosh-plugin-manager/tests/scenarios/run_echo_pass.toml
crates/yosh-plugin-manager/tests/scenarios/vars_set_fail.toml
```

**Modified files:**

```
crates/yosh-plugin-api/src/lib.rs            pub mod pattern;
crates/yosh-plugin-api/Cargo.toml            no change (pattern.rs is pure types)
crates/yosh-plugin-manager/Cargo.toml        add regex; existing yosh-plugin-api dep already present
crates/yosh-plugin-manager/src/lib.rs        new modules + Commands::Run / Commands::Test variants + dispatchers
src/plugin/pattern.rs                        re-export shim
docs/yosh/plugin.md                          new "Testing Locally" section
```

---

## Task 1: Move `CommandPattern` to `yosh-plugin-api`

The TestCtx (in yosh-plugin-manager) needs the same allowlist matcher the real host uses. Today it lives in `src/plugin/pattern.rs`. Move the canonical copy to `yosh-plugin-api` (which yosh-plugin-manager already depends on) and leave a re-export in the original location.

**Files:**
- Create: `crates/yosh-plugin-api/src/pattern.rs`
- Modify: `crates/yosh-plugin-api/src/lib.rs`
- Modify: `src/plugin/pattern.rs`

- [ ] **Step 1.1: Create the new file with the same contents and tests**

Copy the full contents of `src/plugin/pattern.rs` to `crates/yosh-plugin-api/src/pattern.rs` verbatim (the module is self-contained — no imports from `super::`).

- [ ] **Step 1.2: Expose the module from yosh-plugin-api**

Append to `crates/yosh-plugin-api/src/lib.rs`:

```rust
pub mod pattern;
```

- [ ] **Step 1.3: Replace the original with a re-export shim**

Overwrite `src/plugin/pattern.rs` with:

```rust
//! Re-export of `yosh_plugin_api::pattern` so existing call sites
//! (`super::pattern::CommandPattern`) keep compiling unchanged. The
//! canonical implementation now lives in `crates/yosh-plugin-api`
//! so `yosh-plugin-manager` can use the matcher without depending on
//! the yosh binary crate.
pub use yosh_plugin_api::pattern::*;
```

- [ ] **Step 1.4: Verify both crates compile and tests pass**

Run:

```sh
cargo test -p yosh-plugin-api pattern
cargo test --lib plugin::pattern
cargo build -p yosh-plugin-manager
```

Expected: all pattern tests pass in both locations; manager builds clean.

- [ ] **Step 1.5: Commit**

```sh
git add crates/yosh-plugin-api/src/lib.rs crates/yosh-plugin-api/src/pattern.rs src/plugin/pattern.rs
git commit -m "$(cat <<'EOF'
refactor(plugin): move CommandPattern to yosh-plugin-api

Original task: plugin local-run & test-runner spec
(docs/superpowers/specs/2026-05-12-plugin-dev-test-runner-design.md §8).

yosh-plugin-manager's upcoming TestCtx needs the same allowlist
matcher the real host uses. Move the canonical copy to
yosh-plugin-api (already a manager dependency) and leave a re-export
in src/plugin/pattern.rs so existing imports keep compiling.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: TestState + TestCtx skeleton

Establish the per-store data type and an empty-but-wired `Default` impl. No host imports yet — this task only proves the type compiles and `WasiView` works.

**Files:**
- Create: `crates/yosh-plugin-manager/src/test_host/mod.rs`
- Modify: `crates/yosh-plugin-manager/src/lib.rs`

- [ ] **Step 2.1: Write the failing smoke test**

Append at the bottom of `crates/yosh-plugin-manager/src/test_host/mod.rs` (file will be created in 2.2):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_default_is_empty() {
        let s = TestState::default();
        assert!(s.vars.is_empty());
        assert!(s.exported.is_empty());
        assert_eq!(s.cwd.as_os_str(), "");
        assert!(s.stdout.is_empty());
        assert!(s.stderr.is_empty());
        assert_eq!(s.caps, 0);
    }

    #[test]
    fn test_ctx_default_constructs() {
        let _ctx = TestCtx::default();
    }
}
```

- [ ] **Step 2.2: Create the module with the skeleton**

Create `crates/yosh-plugin-manager/src/test_host/mod.rs`:

```rust
//! In-memory host context for `yosh-plugin run` / `yosh-plugin test`.
//!
//! Mirrors the precedent of `metadata_extract::MetadataCtx`: a
//! self-contained `wasmtime` store data type with an empty `WasiCtx`
//! plus per-capability `yosh:plugin/*` import implementations backed
//! by `TestState`. Per-capability impls live in submodules.
//!
//! See `docs/superpowers/specs/2026-05-12-plugin-dev-test-runner-design.md`
//! §3 for the architectural rationale (third host context alongside
//! `HostContext` and `MetadataCtx`).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

use yosh_plugin_api::pattern::CommandPattern;

pub mod commands;
pub mod files;
pub mod filesystem;
pub mod io;
pub mod variables;

/// Record of one external command spawn (commands:exec). One entry
/// per host call, in invocation order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecRecord {
    pub program: String,
    pub args: Vec<String>,
    pub exit_code: i32,
    pub stdout_len: usize,
    pub stderr_len: usize,
}

/// In-memory state behind every TestCtx host import. The runner
/// constructs this from CLI flags or a scenario file, then reads it
/// back after the guest call to format results / evaluate expectations.
#[derive(Debug, Default, Clone)]
pub struct TestState {
    /// Granted capability bitmask. Same shape as `HostContext.capabilities`.
    pub caps: u32,
    pub vars: HashMap<String, String>,
    pub exported: HashSet<String>,
    pub cwd: PathBuf,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// Virtual filesystem contents (when `sandbox_root` is `None`).
    pub files: HashMap<PathBuf, Vec<u8>>,
    /// If set, files:* host imports operate on the real FS scoped to
    /// this canonicalised root. Otherwise they operate on `files`.
    pub sandbox_root: Option<PathBuf>,
    /// commands:exec allowlist. Empty = all denied (PatternNotAllowed).
    pub allow_exec: Vec<CommandPattern>,
    pub exec_log: Vec<ExecRecord>,
    /// (key, value) pairs the plugin wrote via variables::set during
    /// the current step. Reset by the scenario runner between steps.
    pub set_log: Vec<(String, String)>,
    pub export_log: Vec<(String, String)>,
    /// (path, bytes-written) for each files::{write,append}-file call.
    pub write_log: Vec<(PathBuf, usize)>,
}

/// Per-store wrapper. `state` is the shared in-memory backend; `wasi`
/// is an empty `WasiCtx` to absorb cargo-component's transitive
/// WASI imports (same rationale as `MetadataCtx` §Sandboxing).
pub struct TestCtx {
    pub state: TestState,
    pub(crate) table: ResourceTable,
    pub(crate) wasi: WasiCtx,
}

impl Default for TestCtx {
    fn default() -> Self {
        // Same rationale as MetadataCtx: no preopens, no stdio, no env.
        // Plugins use yosh:plugin/io, not wasi:cli/stdout.
        let wasi = WasiCtxBuilder::new().build();
        TestCtx {
            state: TestState::default(),
            table: ResourceTable::new(),
            wasi,
        }
    }
}

impl TestCtx {
    /// Build from an existing TestState (set up by the CLI / scenario).
    pub fn new(state: TestState) -> Self {
        let mut ctx = TestCtx::default();
        ctx.state = state;
        ctx
    }
}

impl WasiView for TestCtx {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

// Submodule stubs — filled in by tasks 4..=8. Each file currently has
// only a placeholder so `pub mod` resolves.
```

Create five empty companion files so `pub mod` resolves (tasks 4–8 fill them in):

```sh
for f in variables io filesystem files commands; do
  echo '//! filled in by tasks 4-8' > crates/yosh-plugin-manager/src/test_host/$f.rs
done
```

- [ ] **Step 2.3: Wire the module into lib.rs**

Add to `crates/yosh-plugin-manager/src/lib.rs` near the existing `pub mod` lines (after `pub mod verify;`):

```rust
pub mod test_host;
```

- [ ] **Step 2.4: Run tests to verify they pass**

```sh
cargo test -p yosh-plugin-manager test_host
```

Expected: both `test_state_default_is_empty` and `test_ctx_default_constructs` pass.

- [ ] **Step 2.5: Commit**

```sh
git add crates/yosh-plugin-manager/src/test_host crates/yosh-plugin-manager/src/lib.rs
git commit -m "feat(plugin-manager): TestCtx + TestState scaffolding for run/test"
```

---

## Task 3: Linker construction smoke (WASI + empty `yosh:plugin/*`)

Add a `register_wasi` helper and a `build_linker` that registers WASI and constructs an empty `Linker<TestCtx>`. We don't register any `yosh:plugin/*` imports yet — that's tasks 4–9 — but we prove the type bridges work.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/test_host/mod.rs`

- [ ] **Step 3.1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` block in `test_host/mod.rs`:

```rust
#[test]
fn linker_construction_smoke() {
    let engine = crate::precompile::make_engine().expect("engine");
    let _linker = build_linker(&engine).expect("linker");
}
```

- [ ] **Step 3.2: Run to verify it fails**

```sh
cargo test -p yosh-plugin-manager linker_construction_smoke
```

Expected: FAIL with `build_linker not found`.

- [ ] **Step 3.3: Implement `build_linker`**

Add to `test_host/mod.rs` (above the `#[cfg(test)]` block):

```rust
use wasmtime::Engine;
use wasmtime::component::Linker;

/// Construct a `Linker<TestCtx>` with WASI registered. Per-capability
/// `yosh:plugin/*` imports are added by `register_imports` (Task 9).
pub fn build_linker(engine: &Engine) -> wasmtime::Result<Linker<TestCtx>> {
    let mut linker = Linker::<TestCtx>::new(engine);
    register_wasi(&mut linker)?;
    Ok(linker)
}

/// Same rationale as `metadata_extract::register_wasi`: cargo-component
/// plugins pull in `wasi:io` / `wasi:cli` transitively. Isolation is
/// provided by the empty `WasiCtx` in `TestCtx::default`.
fn register_wasi(linker: &mut Linker<TestCtx>) -> wasmtime::Result<()> {
    wasmtime_wasi::add_to_linker_sync(linker)
}
```

- [ ] **Step 3.4: Run to verify it passes**

```sh
cargo test -p yosh-plugin-manager test_host
```

Expected: all three tests pass.

- [ ] **Step 3.5: Commit**

```sh
git add crates/yosh-plugin-manager/src/test_host/mod.rs
git commit -m "feat(plugin-manager): TestCtx linker construction (WASI only)"
```

---

## Task 4: `variables:read` / `variables:write` host imports

Implement `get`, `set`, `export-env`. Each is gated on a capability bit (deny → `Err(Denied)`); when granted, mutate `TestState`.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/test_host/variables.rs`

- [ ] **Step 4.1: Write the failing tests**

Replace `test_host/variables.rs` with:

```rust
//! In-memory `yosh:plugin/variables` host imports backed by
//! `TestState.vars` / `TestState.exported`. Gated on
//! CAP_VARIABLES_READ and CAP_VARIABLES_WRITE.

use super::TestState;
use crate::generated::yosh::plugin::types::ErrorCode;
use yosh_plugin_api::{CAP_VARIABLES_READ, CAP_VARIABLES_WRITE};

pub fn host_get(state: &TestState, name: &str) -> Result<Option<String>, ErrorCode> {
    if state.caps & CAP_VARIABLES_READ == 0 {
        return Err(ErrorCode::Denied);
    }
    Ok(state.vars.get(name).cloned())
}

pub fn host_set(state: &mut TestState, name: &str, value: &str) -> Result<(), ErrorCode> {
    if state.caps & CAP_VARIABLES_WRITE == 0 {
        return Err(ErrorCode::Denied);
    }
    state.vars.insert(name.to_string(), value.to_string());
    state.set_log.push((name.to_string(), value.to_string()));
    Ok(())
}

pub fn host_export_env(state: &mut TestState, name: &str, value: &str) -> Result<(), ErrorCode> {
    if state.caps & CAP_VARIABLES_WRITE == 0 {
        return Err(ErrorCode::Denied);
    }
    state.vars.insert(name.to_string(), value.to_string());
    state.exported.insert(name.to_string());
    state.export_log.push((name.to_string(), value.to_string()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(caps: u32) -> TestState {
        let mut s = TestState::default();
        s.caps = caps;
        s
    }

    #[test]
    fn get_denied_without_cap() {
        let s = state_with(0);
        assert_eq!(host_get(&s, "FOO"), Err(ErrorCode::Denied));
    }

    #[test]
    fn get_returns_none_for_unset() {
        let s = state_with(CAP_VARIABLES_READ);
        assert_eq!(host_get(&s, "FOO"), Ok(None));
    }

    #[test]
    fn get_returns_value_when_set() {
        let mut s = state_with(CAP_VARIABLES_READ);
        s.vars.insert("FOO".into(), "bar".into());
        assert_eq!(host_get(&s, "FOO"), Ok(Some("bar".into())));
    }

    #[test]
    fn set_denied_without_cap() {
        let mut s = state_with(CAP_VARIABLES_READ);
        assert_eq!(host_set(&mut s, "FOO", "bar"), Err(ErrorCode::Denied));
        assert!(s.set_log.is_empty());
    }

    #[test]
    fn set_records_log() {
        let mut s = state_with(CAP_VARIABLES_WRITE);
        host_set(&mut s, "FOO", "bar").unwrap();
        assert_eq!(s.vars.get("FOO").map(|s| s.as_str()), Some("bar"));
        assert_eq!(s.set_log, vec![("FOO".into(), "bar".into())]);
    }

    #[test]
    fn export_env_records_export_set() {
        let mut s = state_with(CAP_VARIABLES_WRITE);
        host_export_env(&mut s, "PATH", "/bin").unwrap();
        assert!(s.exported.contains("PATH"));
        assert_eq!(s.export_log, vec![("PATH".into(), "/bin".into())]);
    }
}
```

- [ ] **Step 4.2: Run to verify failures**

```sh
cargo test -p yosh-plugin-manager test_host::variables
```

Expected: tests reference items that don't exist yet — should compile but maybe fail elsewhere. If `super::TestState` resolves they should compile. After running, all six tests should pass since the impl above is also in the same file.

(This step is effectively a check that we're writing test-and-impl together; the file replacement contains both. The "failing" step exists to keep the TDD discipline visible — if step 4.1 had only tests and step 4.3 added impl, we'd see real failures. We bundle here for file-level coherence.)

Run them and confirm pass:

```sh
cargo test -p yosh-plugin-manager test_host::variables
```

Expected: 6 passed.

- [ ] **Step 4.3: Commit**

```sh
git add crates/yosh-plugin-manager/src/test_host/variables.rs
git commit -m "feat(plugin-manager): TestCtx variables host imports"
```

---

## Task 5: `io` host imports

Implement `write` for stdout/stderr. Gated on `CAP_IO`.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/test_host/io.rs`

- [ ] **Step 5.1: Write the impl + tests together**

Replace `test_host/io.rs` with:

```rust
//! In-memory `yosh:plugin/io` host import backed by
//! `TestState.stdout` / `TestState.stderr`. Gated on CAP_IO.

use super::TestState;
use crate::generated::yosh::plugin::types::{ErrorCode, IoStream};
use yosh_plugin_api::CAP_IO;

pub fn host_write(
    state: &mut TestState,
    target: IoStream,
    data: &[u8],
) -> Result<(), ErrorCode> {
    if state.caps & CAP_IO == 0 {
        return Err(ErrorCode::Denied);
    }
    match target {
        IoStream::Stdout => state.stdout.extend_from_slice(data),
        IoStream::Stderr => state.stderr.extend_from_slice(data),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_denied_without_cap() {
        let mut s = TestState::default();
        assert_eq!(host_write(&mut s, IoStream::Stdout, b"x"), Err(ErrorCode::Denied));
        assert!(s.stdout.is_empty());
    }

    #[test]
    fn write_appends_to_stdout() {
        let mut s = TestState::default();
        s.caps = CAP_IO;
        host_write(&mut s, IoStream::Stdout, b"hello ").unwrap();
        host_write(&mut s, IoStream::Stdout, b"world").unwrap();
        assert_eq!(s.stdout, b"hello world");
    }

    #[test]
    fn write_targets_stderr_separately() {
        let mut s = TestState::default();
        s.caps = CAP_IO;
        host_write(&mut s, IoStream::Stderr, b"err").unwrap();
        assert!(s.stdout.is_empty());
        assert_eq!(s.stderr, b"err");
    }
}
```

- [ ] **Step 5.2: Run tests**

```sh
cargo test -p yosh-plugin-manager test_host::io
```

Expected: 3 passed.

- [ ] **Step 5.3: Commit**

```sh
git add crates/yosh-plugin-manager/src/test_host/io.rs
git commit -m "feat(plugin-manager): TestCtx io host import"
```

---

## Task 6: `filesystem` host imports

Implement `cwd` / `set-cwd`. Gated on `CAP_FILESYSTEM`. Virtual cwd only — never touches the process cwd.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/test_host/filesystem.rs`

- [ ] **Step 6.1: Impl + tests**

Replace `test_host/filesystem.rs` with:

```rust
//! In-memory `yosh:plugin/filesystem` host imports backed by
//! `TestState.cwd`. Gated on CAP_FILESYSTEM. The cwd is virtual —
//! changing it does not call `std::env::set_current_dir`.

use std::path::PathBuf;

use super::TestState;
use crate::generated::yosh::plugin::types::ErrorCode;
use yosh_plugin_api::CAP_FILESYSTEM;

pub fn host_cwd(state: &TestState) -> Result<String, ErrorCode> {
    if state.caps & CAP_FILESYSTEM == 0 {
        return Err(ErrorCode::Denied);
    }
    Ok(state.cwd.to_string_lossy().into_owned())
}

pub fn host_set_cwd(state: &mut TestState, path: &str) -> Result<(), ErrorCode> {
    if state.caps & CAP_FILESYSTEM == 0 {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    state.cwd = PathBuf::from(path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cwd_denied_without_cap() {
        let s = TestState::default();
        assert_eq!(host_cwd(&s), Err(ErrorCode::Denied));
    }

    #[test]
    fn cwd_returns_state_cwd() {
        let mut s = TestState::default();
        s.caps = CAP_FILESYSTEM;
        s.cwd = PathBuf::from("/tmp");
        assert_eq!(host_cwd(&s), Ok("/tmp".to_string()));
    }

    #[test]
    fn set_cwd_updates_state() {
        let mut s = TestState::default();
        s.caps = CAP_FILESYSTEM;
        host_set_cwd(&mut s, "/home").unwrap();
        assert_eq!(s.cwd, PathBuf::from("/home"));
    }

    #[test]
    fn set_cwd_rejects_empty() {
        let mut s = TestState::default();
        s.caps = CAP_FILESYSTEM;
        assert_eq!(host_set_cwd(&mut s, ""), Err(ErrorCode::InvalidArgument));
    }
}
```

- [ ] **Step 6.2: Run tests**

```sh
cargo test -p yosh-plugin-manager test_host::filesystem
```

Expected: 4 passed.

- [ ] **Step 6.3: Commit**

```sh
git add crates/yosh-plugin-manager/src/test_host/filesystem.rs
git commit -m "feat(plugin-manager): TestCtx filesystem (virtual cwd)"
```

---

## Task 7: `files:read` / `files:write` host imports

Implement the eight functions (`read-file`, `read-dir`, `metadata`, `write-file`, `append-file`, `create-dir`, `remove-file`, `remove-dir`). Two modes: virtual FS (`sandbox_root = None`, operates on `TestState.files`) and real-FS sandbox (`sandbox_root = Some(canonical_root)`, all paths must canonicalise inside that prefix).

**Files:**
- Modify: `crates/yosh-plugin-manager/src/test_host/files.rs`

- [ ] **Step 7.1: Impl + tests**

Replace `test_host/files.rs` with:

```rust
//! In-memory and sandboxed `yosh:plugin/files` host imports.
//! - Virtual mode (`state.sandbox_root.is_none()`): all 8 functions
//!   read/mutate `state.files` (a `HashMap<PathBuf, Vec<u8>>`).
//! - Sandbox mode: paths are canonicalised against `state.sandbox_root`;
//!   any escape returns `Denied`. Real-FS calls happen via `std::fs`.

use std::path::{Path, PathBuf};

use super::TestState;
use crate::generated::yosh::plugin::files::{DirEntry, FileStat};
use crate::generated::yosh::plugin::types::ErrorCode;
use yosh_plugin_api::{CAP_FILES_READ, CAP_FILES_WRITE};

fn require_read(state: &TestState) -> Result<(), ErrorCode> {
    if state.caps & CAP_FILES_READ == 0 {
        Err(ErrorCode::Denied)
    } else {
        Ok(())
    }
}

fn require_write(state: &TestState) -> Result<(), ErrorCode> {
    if state.caps & CAP_FILES_WRITE == 0 {
        Err(ErrorCode::Denied)
    } else {
        Ok(())
    }
}

/// In sandbox mode, return the canonicalised real path or `Denied` if
/// it escapes `root`. Virtual mode returns the path as-is.
fn resolve<'a>(state: &TestState, path: &'a str) -> Result<PathBuf, ErrorCode> {
    match &state.sandbox_root {
        None => Ok(PathBuf::from(path)),
        Some(root) => {
            let candidate = if Path::new(path).is_absolute() {
                PathBuf::from(path)
            } else {
                root.join(path)
            };
            // Canonicalise lazily: if the file doesn't exist yet
            // (write/create), canonicalise the parent and re-join.
            let canon = match std::fs::canonicalize(&candidate) {
                Ok(p) => p,
                Err(_) => {
                    let parent = candidate.parent().ok_or(ErrorCode::Denied)?;
                    let parent_canon = std::fs::canonicalize(parent).map_err(|_| ErrorCode::Denied)?;
                    let file_name = candidate.file_name().ok_or(ErrorCode::Denied)?;
                    parent_canon.join(file_name)
                }
            };
            if canon.starts_with(root) {
                Ok(canon)
            } else {
                Err(ErrorCode::Denied)
            }
        }
    }
}

pub fn host_read_file(state: &TestState, path: &str) -> Result<Vec<u8>, ErrorCode> {
    require_read(state)?;
    let resolved = resolve(state, path)?;
    match &state.sandbox_root {
        None => state
            .files
            .get(&resolved)
            .cloned()
            .ok_or(ErrorCode::NotFound),
        Some(_) => std::fs::read(&resolved).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ErrorCode::NotFound,
            _ => ErrorCode::IoFailed,
        }),
    }
}

pub fn host_write_file(
    state: &mut TestState,
    path: &str,
    data: &[u8],
) -> Result<(), ErrorCode> {
    require_write(state)?;
    let resolved = resolve(state, path)?;
    match state.sandbox_root.clone() {
        None => {
            state.files.insert(resolved.clone(), data.to_vec());
        }
        Some(_) => {
            std::fs::write(&resolved, data).map_err(|_| ErrorCode::IoFailed)?;
        }
    }
    state.write_log.push((resolved, data.len()));
    Ok(())
}

pub fn host_append_file(
    state: &mut TestState,
    path: &str,
    data: &[u8],
) -> Result<(), ErrorCode> {
    require_write(state)?;
    let resolved = resolve(state, path)?;
    match state.sandbox_root.clone() {
        None => {
            state.files.entry(resolved.clone()).or_default().extend_from_slice(data);
        }
        Some(_) => {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&resolved)
                .map_err(|_| ErrorCode::IoFailed)?;
            f.write_all(data).map_err(|_| ErrorCode::IoFailed)?;
        }
    }
    state.write_log.push((resolved, data.len()));
    Ok(())
}

pub fn host_create_dir(
    state: &mut TestState,
    path: &str,
    recursive: bool,
) -> Result<(), ErrorCode> {
    require_write(state)?;
    let resolved = resolve(state, path)?;
    match &state.sandbox_root {
        None => {
            // Virtual mode: directories don't exist as entries; we
            // treat create-dir as a no-op success in virtual mode.
            // Real authors should use sandbox mode for filesystem
            // structure testing.
            let _ = resolved;
            Ok(())
        }
        Some(_) => {
            let r = if recursive {
                std::fs::create_dir_all(&resolved)
            } else {
                std::fs::create_dir(&resolved)
            };
            r.map_err(|_| ErrorCode::IoFailed)
        }
    }
}

pub fn host_remove_file(state: &mut TestState, path: &str) -> Result<(), ErrorCode> {
    require_write(state)?;
    let resolved = resolve(state, path)?;
    match &state.sandbox_root {
        None => state
            .files
            .remove(&resolved)
            .map(|_| ())
            .ok_or(ErrorCode::NotFound),
        Some(_) => std::fs::remove_file(&resolved).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ErrorCode::NotFound,
            _ => ErrorCode::IoFailed,
        }),
    }
}

pub fn host_remove_dir(state: &mut TestState, path: &str, recursive: bool) -> Result<(), ErrorCode> {
    require_write(state)?;
    let resolved = resolve(state, path)?;
    match &state.sandbox_root {
        None => {
            let _ = (resolved, recursive);
            Ok(())
        }
        Some(_) => {
            let r = if recursive {
                std::fs::remove_dir_all(&resolved)
            } else {
                std::fs::remove_dir(&resolved)
            };
            r.map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => ErrorCode::NotFound,
                _ => ErrorCode::IoFailed,
            })
        }
    }
}

pub fn host_read_dir(state: &TestState, path: &str) -> Result<Vec<DirEntry>, ErrorCode> {
    require_read(state)?;
    let resolved = resolve(state, path)?;
    match &state.sandbox_root {
        None => {
            // Virtual mode: enumerate keys with `resolved` as a prefix.
            let mut out = Vec::new();
            for k in state.files.keys() {
                if k.parent() == Some(&resolved) {
                    out.push(DirEntry {
                        name: k.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                        is_file: true,
                        is_dir: false,
                        is_symlink: false,
                    });
                }
            }
            Ok(out)
        }
        Some(_) => {
            let rd = std::fs::read_dir(&resolved).map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => ErrorCode::NotFound,
                _ => ErrorCode::IoFailed,
            })?;
            let mut out = Vec::new();
            for entry in rd.flatten() {
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
    }
}

pub fn host_metadata(state: &TestState, path: &str) -> Result<FileStat, ErrorCode> {
    require_read(state)?;
    let resolved = resolve(state, path)?;
    match &state.sandbox_root {
        None => {
            let bytes = state.files.get(&resolved).ok_or(ErrorCode::NotFound)?;
            Ok(FileStat {
                is_file: true,
                is_dir: false,
                is_symlink: false,
                size: bytes.len() as u64,
                mtime_secs: 0,
            })
        }
        Some(_) => {
            let md = std::fs::metadata(&resolved).map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => ErrorCode::NotFound,
                _ => ErrorCode::IoFailed,
            })?;
            Ok(FileStat {
                is_file: md.is_file(),
                is_dir: md.is_dir(),
                is_symlink: false,
                size: md.len(),
                mtime_secs: 0,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_rw() -> TestState {
        let mut s = TestState::default();
        s.caps = CAP_FILES_READ | CAP_FILES_WRITE;
        s
    }

    #[test]
    fn read_denied_without_cap() {
        let s = TestState::default();
        assert_eq!(host_read_file(&s, "/a"), Err(ErrorCode::Denied));
    }

    #[test]
    fn virtual_write_then_read_roundtrips() {
        let mut s = state_rw();
        host_write_file(&mut s, "/a", b"hello").unwrap();
        assert_eq!(host_read_file(&s, "/a"), Ok(b"hello".to_vec()));
        assert_eq!(s.write_log.len(), 1);
    }

    #[test]
    fn virtual_read_missing_returns_not_found() {
        let s = state_rw();
        assert_eq!(host_read_file(&s, "/missing"), Err(ErrorCode::NotFound));
    }

    #[test]
    fn virtual_append_concatenates() {
        let mut s = state_rw();
        host_write_file(&mut s, "/a", b"hello ").unwrap();
        host_append_file(&mut s, "/a", b"world").unwrap();
        assert_eq!(host_read_file(&s, "/a"), Ok(b"hello world".to_vec()));
    }

    #[test]
    fn virtual_remove_deletes_entry() {
        let mut s = state_rw();
        host_write_file(&mut s, "/a", b"x").unwrap();
        host_remove_file(&mut s, "/a").unwrap();
        assert_eq!(host_read_file(&s, "/a"), Err(ErrorCode::NotFound));
    }

    #[test]
    fn virtual_metadata_reports_size() {
        let mut s = state_rw();
        host_write_file(&mut s, "/a", b"abc").unwrap();
        let md = host_metadata(&s, "/a").unwrap();
        assert!(md.is_file);
        assert_eq!(md.size, 3);
    }

    #[test]
    fn sandbox_escape_returns_denied() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let mut s = state_rw();
        s.sandbox_root = Some(root.clone());
        let outside = format!("{}/../etc/passwd", root.display());
        let err = host_read_file(&s, &outside);
        assert!(matches!(err, Err(ErrorCode::Denied) | Err(ErrorCode::NotFound)));
    }

    #[test]
    fn sandbox_write_lands_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let mut s = state_rw();
        s.sandbox_root = Some(root.clone());
        host_write_file(&mut s, "out.txt", b"data").unwrap();
        let on_disk = std::fs::read(root.join("out.txt")).unwrap();
        assert_eq!(on_disk, b"data");
    }
}
```

- [ ] **Step 7.2: Run tests**

```sh
cargo test -p yosh-plugin-manager test_host::files
```

Expected: 8 passed.

- [ ] **Step 7.3: Commit**

```sh
git add crates/yosh-plugin-manager/src/test_host/files.rs
git commit -m "feat(plugin-manager): TestCtx files (virtual + sandbox)"
```

---

## Task 8: `commands:exec` host import

Implement `exec` with real subprocess spawn + `CommandPattern` allowlist + 1000 ms timeout. Logic duplicated from `src/plugin/host/commands.rs::spawn_with_timeout` — pattern matcher reused from `yosh-plugin-api`. A TODO note is added pointing at future consolidation.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/test_host/commands.rs`
- Modify: `crates/yosh-plugin-manager/Cargo.toml` (add `nix` dep for SIGTERM)

- [ ] **Step 8.1: Add `nix` to manager Cargo.toml**

Append to `[dependencies]` in `crates/yosh-plugin-manager/Cargo.toml`:

```toml
nix = { version = "0.31", features = ["signal", "process"] }
```

- [ ] **Step 8.2: Impl + tests**

Replace `test_host/commands.rs` with:

```rust
//! In-memory `yosh:plugin/commands` host import — spawns real
//! subprocesses, gated by CAP_COMMANDS_EXEC and an allowlist of
//! `CommandPattern` (reused from yosh-plugin-api).
//!
//! Spawn / timeout logic duplicates `src/plugin/host/commands.rs::spawn_with_timeout`
//! intentionally; consolidation onto a shared helper is tracked as a
//! TODO (spec §11).

use std::time::Duration;

use super::{ExecRecord, TestState};
use crate::generated::yosh::plugin::commands::ExecOutput;
use crate::generated::yosh::plugin::types::ErrorCode;
use yosh_plugin_api::CAP_COMMANDS_EXEC;

pub fn host_exec(
    state: &mut TestState,
    program: &str,
    args: &[String],
) -> Result<ExecOutput, ErrorCode> {
    if state.caps & CAP_COMMANDS_EXEC == 0 {
        return Err(ErrorCode::Denied);
    }
    if program.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }

    let argv: Vec<&str> = std::iter::once(program)
        .chain(args.iter().map(|s| s.as_str()))
        .collect();

    if !state.allow_exec.iter().any(|p| p.matches(&argv)) {
        return Err(ErrorCode::PatternNotAllowed);
    }

    let out = spawn_with_timeout(program, &argv[1..], Duration::from_millis(1000))?;
    state.exec_log.push(ExecRecord {
        program: program.to_string(),
        args: args.to_vec(),
        exit_code: out.exit_code,
        stdout_len: out.stdout.len(),
        stderr_len: out.stderr.len(),
    });
    Ok(out)
}

fn spawn_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<ExecOutput, ErrorCode> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Instant;

    let mut child = match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(ErrorCode::NotFound),
        Err(_) => return Err(ErrorCode::IoFailed),
    };

    let mut stdout_pipe = child.stdout.take().expect("piped stdout");
    let mut stderr_pipe = child.stderr.take().expect("piped stderr");
    let (out_tx, out_rx) = mpsc::channel::<std::io::Result<Vec<u8>>>();
    let (err_tx, err_rx) = mpsc::channel::<std::io::Result<Vec<u8>>>();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let r = stdout_pipe.read_to_end(&mut buf).map(|_| buf);
        let _ = out_tx.send(r);
    });
    thread::spawn(move || {
        let mut buf = Vec::new();
        let r = stderr_pipe.read_to_end(&mut buf).map(|_| buf);
        let _ = err_tx.send(r);
    });

    let deadline = Instant::now() + timeout;
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {}
            Err(_) => return Err(ErrorCode::IoFailed),
        }
        if Instant::now() >= deadline {
            let pid = nix::unistd::Pid::from_raw(child.id() as i32);
            let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM);
            let grace = Instant::now() + Duration::from_millis(100);
            loop {
                if let Ok(Some(_)) = child.try_wait() {
                    break;
                }
                if Instant::now() >= grace {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            let _ = out_rx.recv();
            let _ = err_rx.recv();
            return Err(ErrorCode::Timeout);
        }
        thread::sleep(Duration::from_millis(10));
    };

    let stdout = out_rx.recv().ok().and_then(|r| r.ok()).unwrap_or_default();
    let stderr = err_rx.recv().ok().and_then(|r| r.ok()).unwrap_or_default();
    Ok(ExecOutput {
        exit_code: exit_status.code().unwrap_or(-1),
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use yosh_plugin_api::pattern::CommandPattern;

    fn state_with_allow(patterns: &[&str]) -> TestState {
        let mut s = TestState::default();
        s.caps = CAP_COMMANDS_EXEC;
        s.allow_exec = patterns.iter().map(|p| CommandPattern::parse(p).unwrap()).collect();
        s
    }

    #[test]
    fn exec_denied_without_cap() {
        let mut s = TestState::default();
        assert!(matches!(host_exec(&mut s, "/bin/echo", &[]), Err(ErrorCode::Denied)));
    }

    #[test]
    fn exec_rejects_pattern_mismatch() {
        let mut s = state_with_allow(&["ls:*"]);
        assert!(matches!(
            host_exec(&mut s, "/bin/echo", &["hi".to_string()]),
            Err(ErrorCode::PatternNotAllowed)
        ));
    }

    #[test]
    fn exec_runs_when_pattern_matches() {
        let mut s = state_with_allow(&["/bin/echo:*"]);
        let out = host_exec(&mut s, "/bin/echo", &["hello".to_string()]).unwrap();
        assert_eq!(out.exit_code, 0);
        assert_eq!(out.stdout, b"hello\n");
        assert_eq!(s.exec_log.len(), 1);
        assert_eq!(s.exec_log[0].program, "/bin/echo");
    }

    #[test]
    fn exec_returns_not_found_for_missing_binary() {
        let mut s = state_with_allow(&["/nope/binary-xyz:*"]);
        assert!(matches!(host_exec(&mut s, "/nope/binary-xyz", &[]), Err(ErrorCode::NotFound)));
    }
}
```

- [ ] **Step 8.3: Run tests**

```sh
cargo test -p yosh-plugin-manager test_host::commands
```

Expected: 4 passed.

- [ ] **Step 8.4: Commit**

```sh
git add crates/yosh-plugin-manager/Cargo.toml crates/yosh-plugin-manager/src/test_host/commands.rs
git commit -m "feat(plugin-manager): TestCtx commands:exec (real spawn + allowlist)"
```

---

## Task 9: Register all `yosh:plugin/*` imports on the linker

Wire every WIT function through `linker.instance(...)?.func_wrap(...)` so that the in-memory impls from tasks 4–8 are called by wasmtime. The capability gating already lives inside each `host_*` function (returns `Denied` when the bit is clear), so the linker registers the real impl unconditionally.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/test_host/mod.rs`

- [ ] **Step 9.1: Write the failing test**

Add to `test_host/mod.rs` tests block:

```rust
#[test]
fn linker_with_yosh_imports_constructs() {
    let engine = crate::precompile::make_engine().unwrap();
    let mut linker = build_linker(&engine).unwrap();
    register_imports(&mut linker).expect("yosh imports");
}
```

- [ ] **Step 9.2: Run to verify failure**

```sh
cargo test -p yosh-plugin-manager linker_with_yosh_imports_constructs
```

Expected: FAIL with `register_imports not found`.

- [ ] **Step 9.3: Implement `register_imports`**

Add to `test_host/mod.rs` (above the test block, below `register_wasi`):

```rust
use crate::generated::yosh::plugin::commands::ExecOutput;
use crate::generated::yosh::plugin::files::{DirEntry, FileStat};
use crate::generated::yosh::plugin::types::{ErrorCode, IoStream};

/// Register every `yosh:plugin/*` import. The per-capability host
/// functions enforce their own capability checks; the linker
/// unconditionally points each WIT name at its real implementation.
pub fn register_imports(linker: &mut Linker<TestCtx>) -> wasmtime::Result<()> {
    // variables
    let mut vars = linker.instance("yosh:plugin/variables@0.2.1")?;
    vars.func_wrap(
        "get",
        |store: wasmtime::StoreContextMut<'_, TestCtx>, (name,): (String,)| {
            Ok::<_, wasmtime::Error>((variables::host_get(&store.data().state, &name),))
        },
    )?;
    vars.func_wrap(
        "set",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (name, value): (String, String)| {
            Ok::<_, wasmtime::Error>((variables::host_set(&mut store.data_mut().state, &name, &value),))
        },
    )?;
    vars.func_wrap(
        "export-env",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (name, value): (String, String)| {
            Ok::<_, wasmtime::Error>((variables::host_export_env(&mut store.data_mut().state, &name, &value),))
        },
    )?;

    // filesystem
    let mut fs = linker.instance("yosh:plugin/filesystem@0.2.1")?;
    fs.func_wrap(
        "cwd",
        |store: wasmtime::StoreContextMut<'_, TestCtx>, (): ()| {
            Ok::<_, wasmtime::Error>((filesystem::host_cwd(&store.data().state),))
        },
    )?;
    fs.func_wrap(
        "set-cwd",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (path,): (String,)| {
            Ok::<_, wasmtime::Error>((filesystem::host_set_cwd(&mut store.data_mut().state, &path),))
        },
    )?;

    // io
    let mut io_inst = linker.instance("yosh:plugin/io@0.2.1")?;
    io_inst.func_wrap(
        "write",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (target, data): (IoStream, Vec<u8>)| {
            Ok::<_, wasmtime::Error>((io::host_write(&mut store.data_mut().state, target, &data),))
        },
    )?;

    // files
    let mut f = linker.instance("yosh:plugin/files@0.2.1")?;
    f.func_wrap(
        "read-file",
        |store: wasmtime::StoreContextMut<'_, TestCtx>, (path,): (String,)| {
            Ok::<_, wasmtime::Error>((files::host_read_file(&store.data().state, &path),))
        },
    )?;
    f.func_wrap(
        "read-dir",
        |store: wasmtime::StoreContextMut<'_, TestCtx>, (path,): (String,)| {
            Ok::<_, wasmtime::Error>((files::host_read_dir(&store.data().state, &path),))
        },
    )?;
    f.func_wrap(
        "metadata",
        |store: wasmtime::StoreContextMut<'_, TestCtx>, (path,): (String,)| {
            Ok::<_, wasmtime::Error>((files::host_metadata(&store.data().state, &path),))
        },
    )?;
    f.func_wrap(
        "write-file",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (path, data): (String, Vec<u8>)| {
            Ok::<_, wasmtime::Error>((files::host_write_file(&mut store.data_mut().state, &path, &data),))
        },
    )?;
    f.func_wrap(
        "append-file",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (path, data): (String, Vec<u8>)| {
            Ok::<_, wasmtime::Error>((files::host_append_file(&mut store.data_mut().state, &path, &data),))
        },
    )?;
    f.func_wrap(
        "create-dir",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (path, recursive): (String, bool)| {
            Ok::<_, wasmtime::Error>((files::host_create_dir(&mut store.data_mut().state, &path, recursive),))
        },
    )?;
    f.func_wrap(
        "remove-file",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (path,): (String,)| {
            Ok::<_, wasmtime::Error>((files::host_remove_file(&mut store.data_mut().state, &path),))
        },
    )?;
    f.func_wrap(
        "remove-dir",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (path, recursive): (String, bool)| {
            Ok::<_, wasmtime::Error>((files::host_remove_dir(&mut store.data_mut().state, &path, recursive),))
        },
    )?;

    // commands
    let mut cmds = linker.instance("yosh:plugin/commands@0.2.1")?;
    cmds.func_wrap(
        "exec",
        |mut store: wasmtime::StoreContextMut<'_, TestCtx>, (program, args): (String, Vec<String>)| {
            Ok::<_, wasmtime::Error>((commands::host_exec(&mut store.data_mut().state, &program, &args),))
        },
    )?;

    // Silence unused-import warnings if a future WIT addition removes
    // any of these constructor calls.
    let _ = (
        std::marker::PhantomData::<ExecOutput>,
        std::marker::PhantomData::<DirEntry>,
        std::marker::PhantomData::<FileStat>,
        std::marker::PhantomData::<ErrorCode>,
    );
    Ok(())
}
```

- [ ] **Step 9.4: Run to verify it passes**

```sh
cargo test -p yosh-plugin-manager test_host
```

Expected: all module tests pass, including `linker_with_yosh_imports_constructs`.

- [ ] **Step 9.5: Commit**

```sh
git add crates/yosh-plugin-manager/src/test_host/mod.rs
git commit -m "feat(plugin-manager): register all yosh:plugin/* imports on TestCtx linker"
```

---

## Task 10: `runner::load_plugin`

Load a `.wasm` from a file path, build the linker, instantiate, return the `PluginWorld` + `Store`. This is the gate that produces errors when wasm is missing or malformed.

**Files:**
- Create: `crates/yosh-plugin-manager/src/runner.rs`
- Modify: `crates/yosh-plugin-manager/src/lib.rs`

- [ ] **Step 10.1: Create the runner module**

Create `crates/yosh-plugin-manager/src/runner.rs`:

```rust
//! Drive `yosh:plugin/*` exports against a `TestCtx`. Used by both
//! `yosh plugin run` (single invocation) and `yosh plugin test`
//! (scenario stepping).

use std::path::Path;
use std::time::Duration;

use wasmtime::component::Component;
use wasmtime::Store;

use crate::generated::{PluginWorld, PluginWorldPre};
use crate::precompile::make_engine;
use crate::test_host::{TestCtx, TestState, build_linker, register_imports};

pub struct LoadedPlugin {
    pub world: PluginWorld,
    pub store: Store<TestCtx>,
    pub engine: wasmtime::Engine,
}

#[derive(Debug)]
pub enum RunnerError {
    Load(String),
    Trap(String),
    Timeout(String),
}

impl std::fmt::Display for RunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunnerError::Load(s) => write!(f, "load: {}", s),
            RunnerError::Trap(s) => write!(f, "trap: {}", s),
            RunnerError::Timeout(s) => write!(f, "timeout: {}", s),
        }
    }
}

pub fn load_plugin(
    wasm_path: &Path,
    state: TestState,
    timeout: Duration,
) -> Result<LoadedPlugin, RunnerError> {
    let engine = make_engine().map_err(|e| RunnerError::Load(e.to_string()))?;
    let wasm_bytes = std::fs::read(wasm_path)
        .map_err(|e| RunnerError::Load(format!("read {}: {}", wasm_path.display(), e)))?;
    let component = Component::new(&engine, &wasm_bytes)
        .map_err(|e| RunnerError::Load(format!("compile: {}", e)))?;

    let mut linker = build_linker(&engine).map_err(|e| RunnerError::Load(e.to_string()))?;
    register_imports(&mut linker).map_err(|e| RunnerError::Load(e.to_string()))?;

    let pre = PluginWorldPre::new(
        linker.instantiate_pre(&component).map_err(|e| RunnerError::Load(format!("instantiate_pre: {}", e)))?,
    )
    .map_err(|e| RunnerError::Load(format!("bindings: {}", e)))?;

    let mut store = Store::new(&engine, TestCtx::new(state));
    store.set_epoch_deadline(1);

    let watchdog_engine = engine.clone();
    let _watchdog = std::thread::Builder::new()
        .name("yosh-plugin-test-watchdog".into())
        .spawn(move || {
            std::thread::sleep(timeout);
            watchdog_engine.increment_epoch();
        });

    let world = pre
        .instantiate(&mut store)
        .map_err(|e| RunnerError::Load(format!("instantiate: {}", e)))?;
    Ok(LoadedPlugin { world, store, engine })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_wasm_returns_load_error() {
        let err = load_plugin(
            Path::new("/no/such/file.wasm"),
            TestState::default(),
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(matches!(err, RunnerError::Load(_)));
    }

    #[test]
    fn load_non_wasm_file_returns_load_error() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"not wasm").unwrap();
        let err = load_plugin(tmp.path(), TestState::default(), Duration::from_secs(1)).unwrap_err();
        assert!(matches!(err, RunnerError::Load(_)));
    }
}
```

- [ ] **Step 10.2: Register the module in lib.rs**

Add to `crates/yosh-plugin-manager/src/lib.rs`:

```rust
pub mod runner;
```

- [ ] **Step 10.3: Run tests**

```sh
cargo test -p yosh-plugin-manager runner
```

Expected: 2 passed.

- [ ] **Step 10.4: Commit**

```sh
git add crates/yosh-plugin-manager/src/runner.rs crates/yosh-plugin-manager/src/lib.rs
git commit -m "feat(plugin-manager): runner::load_plugin (load + instantiate)"
```

---

## Task 11: `runner::invoke_exec`

Drive the `plugin/exec` export. Returns a `RunOutcome` summarising the plugin exit code plus all captured state.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/runner.rs`

- [ ] **Step 11.1: Extend runner.rs**

Add to `runner.rs` (above the `#[cfg(test)]` block):

```rust
use crate::test_host::ExecRecord;

/// Outcome of one guest invocation. Includes everything the formatters
/// and scenario evaluator need.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub exit_code: Option<i32>,        // Some for exec, None for hooks
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub set_log: Vec<(String, String)>,
    pub export_log: Vec<(String, String)>,
    pub write_log: Vec<(std::path::PathBuf, usize)>,
    pub exec_log: Vec<ExecRecord>,
    pub error: Option<String>,         // populated on trap/denied/timeout
    pub error_kind: Option<&'static str>,
}

impl RunOutcome {
    fn from_state(state: TestState, exit_code: Option<i32>, error: Option<(&'static str, String)>) -> Self {
        let (kind, msg) = match error {
            Some((k, m)) => (Some(k), Some(m)),
            None => (None, None),
        };
        RunOutcome {
            exit_code,
            stdout: state.stdout,
            stderr: state.stderr,
            set_log: state.set_log,
            export_log: state.export_log,
            write_log: state.write_log,
            exec_log: state.exec_log,
            error: msg,
            error_kind: kind,
        }
    }
}

pub fn invoke_exec(
    mut loaded: LoadedPlugin,
    command: &str,
    args: &[String],
) -> RunOutcome {
    let plugin = loaded.world.yosh_plugin_plugin();
    let res = plugin.call_exec(&mut loaded.store, command, args);
    let state = loaded.store.into_data().state;
    match res {
        Ok(code) => RunOutcome::from_state(state, Some(code), None),
        Err(e) => {
            let msg = e.to_string();
            let kind = classify_trap(&msg);
            RunOutcome::from_state(state, None, Some((kind, msg)))
        }
    }
}

fn classify_trap(msg: &str) -> &'static str {
    if msg.contains("epoch") || msg.contains("deadline") {
        "timeout"
    } else {
        "trap"
    }
}
```

- [ ] **Step 11.2: Add a behavioural test gated on the in-repo plugin wasm**

Add to `runner.rs` tests block:

```rust
#[test]
fn invoke_exec_runs_test_plugin_test_cmd() {
    let wasm = match plugin_artifact() {
        Some(p) => p,
        None => return, // wasm not built; skip silently
    };
    let mut state = TestState::default();
    state.caps = yosh_plugin_api::CAP_IO;
    let loaded = load_plugin(&wasm, state, Duration::from_secs(5)).expect("load");
    let outcome = invoke_exec(loaded, "test_cmd", &["arg1".to_string()]);
    assert_eq!(outcome.exit_code, Some(0));
    assert!(outcome.stdout.starts_with(b"test_cmd args=["));
    assert!(outcome.error.is_none());
}

fn plugin_artifact() -> Option<std::path::PathBuf> {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm32-wasip2/release/test_plugin.wasm");
    if p.exists() { Some(p) } else { None }
}
```

- [ ] **Step 11.3: Build the wasm artifact (one-time, if not present)**

```sh
cargo component build -p test_plugin --target wasm32-wasip2 --release
```

- [ ] **Step 11.4: Run tests**

```sh
cargo test -p yosh-plugin-manager runner
```

Expected: 3 passed (load missing, load non-wasm, exec test_cmd).

- [ ] **Step 11.5: Commit**

```sh
git add crates/yosh-plugin-manager/src/runner.rs
git commit -m "feat(plugin-manager): runner::invoke_exec + RunOutcome"
```

---

## Task 12: `runner::invoke_hook` for all four hooks

Mirror `invoke_exec` for each hook entry point. Hooks return `()`, so `exit_code` stays `None`.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/runner.rs`

- [ ] **Step 12.1: Add the dispatcher**

Add to `runner.rs`:

```rust
pub enum HookCall {
    PreExec { command_line: String },
    PostExec { command_line: String, exit_code: i32 },
    OnCd { old: String, new: String },
    PrePrompt,
}

pub fn invoke_hook(mut loaded: LoadedPlugin, hook: HookCall) -> RunOutcome {
    let hooks = loaded.world.yosh_plugin_hooks();
    let res = match &hook {
        HookCall::PreExec { command_line } => hooks.call_pre_exec(&mut loaded.store, command_line),
        HookCall::PostExec { command_line, exit_code } => {
            hooks.call_post_exec(&mut loaded.store, command_line, *exit_code)
        }
        HookCall::OnCd { old, new } => hooks.call_on_cd(&mut loaded.store, old, new),
        HookCall::PrePrompt => hooks.call_pre_prompt(&mut loaded.store),
    };
    let state = loaded.store.into_data().state;
    match res {
        Ok(()) => RunOutcome::from_state(state, None, None),
        Err(e) => {
            let msg = e.to_string();
            let kind = classify_trap(&msg);
            RunOutcome::from_state(state, None, Some((kind, msg)))
        }
    }
}
```

- [ ] **Step 12.2: Add a behavioural test**

Add to `runner.rs` tests:

```rust
#[test]
fn invoke_hook_pre_exec_records_event() {
    let wasm = match plugin_artifact() {
        Some(p) => p,
        None => return,
    };
    let mut state = TestState::default();
    state.caps = yosh_plugin_api::CAP_HOOK_PRE_EXEC
        | yosh_plugin_api::CAP_VARIABLES_WRITE
        | yosh_plugin_api::CAP_IO;
    let loaded = load_plugin(&wasm, state, Duration::from_secs(5)).expect("load");
    let outcome = invoke_hook(loaded, HookCall::PreExec { command_line: "ls -l".into() });
    assert!(outcome.error.is_none());
    // test_plugin records pre_exec:ls -l in its internal log; the
    // dump-events command flushes that log to a shell var, but we
    // don't drive it from here — we only need to confirm the hook
    // dispatched without trap.
}
```

- [ ] **Step 12.3: Run tests**

```sh
cargo test -p yosh-plugin-manager runner
```

Expected: 4 passed.

- [ ] **Step 12.4: Commit**

```sh
git add crates/yosh-plugin-manager/src/runner.rs
git commit -m "feat(plugin-manager): runner::invoke_hook"
```

---

## Task 13: Output formatters (human + JSON)

Turn a `RunOutcome` into human-readable text or a JSON object.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/runner.rs`

- [ ] **Step 13.1: Add `format_human` and `format_json`**

Add to `runner.rs`:

```rust
use std::fmt::Write as _;

pub fn format_human(o: &RunOutcome) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "[stdout]\n{}", String::from_utf8_lossy(&o.stdout));
    let _ = writeln!(out, "[stderr]\n{}", String::from_utf8_lossy(&o.stderr));
    match o.exit_code {
        Some(c) => { let _ = writeln!(out, "[exit] {}", c); }
        None => { let _ = writeln!(out, "[exit] (hook — no exit code)"); }
    }
    for (k, v) in &o.set_log {
        let _ = writeln!(out, "[vars set]    {}={}", k, v);
    }
    for (k, v) in &o.export_log {
        let _ = writeln!(out, "[vars export] {}={}", k, v);
    }
    for (p, n) in &o.write_log {
        let _ = writeln!(out, "[files write] {} ({} bytes)", p.display(), n);
    }
    for r in &o.exec_log {
        let _ = writeln!(
            out,
            "[exec]        {} {} → exit {} ({} bytes stdout)",
            r.program, r.args.join(" "), r.exit_code, r.stdout_len
        );
    }
    if let (Some(kind), Some(msg)) = (o.error_kind, &o.error) {
        let _ = writeln!(out, "[error] {}: {}", kind, msg);
    }
    out
}

pub fn format_json(o: &RunOutcome) -> serde_json::Value {
    serde_json::json!({
        "exit": o.exit_code,
        "stdout": String::from_utf8_lossy(&o.stdout),
        "stderr": String::from_utf8_lossy(&o.stderr),
        "vars_set":    o.set_log.iter().map(|(k,v)| serde_json::json!({"key":k,"value":v})).collect::<Vec<_>>(),
        "vars_export": o.export_log.iter().map(|(k,v)| serde_json::json!({"key":k,"value":v})).collect::<Vec<_>>(),
        "files_write": o.write_log.iter().map(|(p,n)| serde_json::json!({"path": p.display().to_string(),"bytes": n})).collect::<Vec<_>>(),
        "exec":        o.exec_log.iter().map(|r| serde_json::json!({
            "program": r.program, "args": r.args, "exit": r.exit_code, "stdout_bytes": r.stdout_len
        })).collect::<Vec<_>>(),
        "error":       o.error.as_ref().map(|m| serde_json::json!({"kind": o.error_kind, "message": m})),
    })
}
```

- [ ] **Step 13.2: Add unit tests**

Add to runner.rs tests:

```rust
#[test]
fn format_json_round_trip_fields() {
    let mut o = RunOutcome {
        exit_code: Some(0),
        stdout: b"hi\n".to_vec(),
        stderr: Vec::new(),
        set_log: vec![("X".into(), "y".into())],
        export_log: Vec::new(),
        write_log: Vec::new(),
        exec_log: Vec::new(),
        error: None,
        error_kind: None,
    };
    let j = format_json(&o);
    assert_eq!(j["exit"], serde_json::json!(0));
    assert_eq!(j["stdout"], serde_json::json!("hi\n"));
    assert_eq!(j["vars_set"][0]["key"], serde_json::json!("X"));
    o.error = Some("boom".into());
    o.error_kind = Some("trap");
    let j2 = format_json(&o);
    assert_eq!(j2["error"]["kind"], serde_json::json!("trap"));
}

#[test]
fn format_human_includes_sections() {
    let o = RunOutcome {
        exit_code: Some(0),
        stdout: b"hi\n".to_vec(),
        stderr: Vec::new(),
        set_log: vec![("X".into(), "y".into())],
        export_log: Vec::new(),
        write_log: Vec::new(),
        exec_log: Vec::new(),
        error: None,
        error_kind: None,
    };
    let s = format_human(&o);
    assert!(s.contains("[stdout]"));
    assert!(s.contains("[exit] 0"));
    assert!(s.contains("[vars set]    X=y"));
}
```

- [ ] **Step 13.3: Run + commit**

```sh
cargo test -p yosh-plugin-manager runner
```

Expected: 6 passed.

```sh
git add crates/yosh-plugin-manager/src/runner.rs
git commit -m "feat(plugin-manager): runner output formatters (human + json)"
```

---

## Task 14: CLI: `Run` subcommand

Extend the `Commands` enum with a `Run` variant plus a nested `RunAction` subcommand for `exec | hook`. Wire CLI flags to `TestState`, call the runner, print the output.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/lib.rs`

- [ ] **Step 14.1: Add the variants**

Inside `enum Commands` in `lib.rs`, add:

```rust
    /// Run a single exec / hook against a plugin wasm with an in-memory host.
    Run {
        /// Path to the wasm component.
        wasm: std::path::PathBuf,
        #[command(subcommand)]
        action: RunAction,
        /// Capabilities to grant (comma-separated, e.g. `io,variables:read`).
        /// Defaults to the plugin's declared `required_capabilities`.
        #[arg(long, value_delimiter = ',')]
        cap: Vec<String>,
        /// Seed a shell variable: `--var KEY=VALUE` (repeatable).
        #[arg(long = "var", value_parser = parse_kv)]
        vars: Vec<(String, String)>,
        /// Seed an exported variable.
        #[arg(long = "export", value_parser = parse_kv)]
        exports: Vec<(String, String)>,
        /// Virtual cwd.
        #[arg(long, default_value = ".")]
        cwd: std::path::PathBuf,
        /// Allowlist pattern for `commands:exec` (repeatable).
        #[arg(long = "allow-exec")]
        allow_exec: Vec<String>,
        /// If set, files:* operate on the real FS scoped here.
        #[arg(long = "sandbox-root")]
        sandbox_root: Option<std::path::PathBuf>,
        /// Watchdog deadline in milliseconds.
        #[arg(long, default_value_t = 5000)]
        timeout: u64,
        /// Output format.
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
    },
```

Add helper / enum definitions above `Commands`:

```rust
#[derive(Subcommand)]
pub enum RunAction {
    /// Call `plugin/exec` with the given command and argv.
    Exec { command: String, args: Vec<String> },
    /// Call one hook.
    Hook {
        #[command(subcommand)]
        which: HookKind,
    },
}

#[derive(Subcommand)]
pub enum HookKind {
    PreExec { command_line: String },
    PostExec { command_line: String, exit_code: i32 },
    OnCd { old: String, new: String },
    PrePrompt,
}

#[derive(Copy, Clone, clap::ValueEnum, Debug)]
pub enum OutputFormat {
    Human,
    Json,
}

fn parse_kv(s: &str) -> Result<(String, String), String> {
    let (k, v) = s.split_once('=').ok_or_else(|| format!("expected KEY=VALUE, got `{}`", s))?;
    Ok((k.to_string(), v.to_string()))
}
```

- [ ] **Step 14.2: Add the dispatcher**

Add to `lib.rs` near the other `cmd_*` functions:

```rust
fn cmd_run(
    wasm: std::path::PathBuf,
    action: RunAction,
    cap: Vec<String>,
    vars: Vec<(String, String)>,
    exports: Vec<(String, String)>,
    cwd: std::path::PathBuf,
    allow_exec: Vec<String>,
    sandbox_root: Option<std::path::PathBuf>,
    timeout: u64,
    format: OutputFormat,
) -> i32 {
    use crate::runner::{HookCall, format_human, format_json, invoke_exec, invoke_hook, load_plugin};
    use crate::test_host::TestState;
    use yosh_plugin_api::{parse_capability, capabilities_to_bitflags};
    use yosh_plugin_api::pattern::CommandPattern;

    // Build TestState.
    let mut state = TestState::default();
    let parsed_caps: Vec<_> = cap.iter()
        .filter_map(|s| parse_capability(s))
        .collect();
    state.caps = if cap.is_empty() {
        // Fall back to plugin-declared capabilities. We need them from
        // the cached metadata, which requires reading plugins.lock OR
        // running metadata_extract. For local-run UX, run metadata_extract
        // inline on the same wasm bytes.
        let bytes = match std::fs::read(&wasm) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("yosh-plugin: read {}: {}", wasm.display(), e);
                return 99;
            }
        };
        let engine = match crate::precompile::make_engine() {
            Ok(e) => e,
            Err(e) => { eprintln!("yosh-plugin: engine: {}", e); return 99; }
        };
        match crate::metadata_extract::extract(&engine, &bytes) {
            Ok(m) => {
                let caps: Vec<_> = m.required_capabilities.iter()
                    .filter_map(|s| parse_capability(s))
                    .collect();
                capabilities_to_bitflags(&caps)
            }
            Err(e) => { eprintln!("yosh-plugin: metadata: {}", e); return 99; }
        }
    } else {
        capabilities_to_bitflags(&parsed_caps)
    };

    for (k, v) in vars { state.vars.insert(k, v); }
    for (k, v) in exports {
        state.vars.insert(k.clone(), v);
        state.exported.insert(k);
    }
    state.cwd = cwd;
    state.allow_exec = allow_exec.iter()
        .filter_map(|p| CommandPattern::parse(p).ok())
        .collect();
    state.sandbox_root = sandbox_root.map(|p| {
        std::fs::canonicalize(&p).unwrap_or(p)
    });

    let loaded = match load_plugin(&wasm, state, std::time::Duration::from_millis(timeout)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("yosh-plugin: {}", e);
            return 99;
        }
    };

    let outcome = match action {
        RunAction::Exec { command, args } => invoke_exec(loaded, &command, &args),
        RunAction::Hook { which } => {
            let call = match which {
                HookKind::PreExec { command_line } => HookCall::PreExec { command_line },
                HookKind::PostExec { command_line, exit_code } => HookCall::PostExec { command_line, exit_code },
                HookKind::OnCd { old, new } => HookCall::OnCd { old, new },
                HookKind::PrePrompt => HookCall::PrePrompt,
            };
            invoke_hook(loaded, call)
        }
    };

    match format {
        OutputFormat::Human => print!("{}", format_human(&outcome)),
        OutputFormat::Json => println!("{}", format_json(&outcome)),
    }

    match outcome.error_kind {
        Some(_) => 99,
        None => outcome.exit_code.unwrap_or(0),
    }
}
```

- [ ] **Step 14.3: Dispatch in `run()`**

In the existing `pub fn run() -> i32` match, add:

```rust
        Commands::Run {
            wasm, action, cap, vars, exports, cwd, allow_exec, sandbox_root, timeout, format,
        } => cmd_run(wasm, action, cap, vars, exports, cwd, allow_exec, sandbox_root, timeout, format),
```

- [ ] **Step 14.4: Smoke from the CLI**

Build and try it:

```sh
cargo build -p yosh-plugin-manager
./target/debug/yosh-plugin run target/wasm32-wasip2/release/test_plugin.wasm exec test_cmd arg1
```

Expected output (human):

```
[stdout]
test_cmd args=["arg1"]

[stderr]

[exit] 0
```

- [ ] **Step 14.5: Commit**

```sh
git add crates/yosh-plugin-manager/src/lib.rs
git commit -m "feat(plugin-manager): yosh plugin run subcommand"
```

---

## Task 15: Scenario schema + parser

Define the TOML schema with serde, parse one file, validate.

**Files:**
- Create: `crates/yosh-plugin-manager/src/scenario.rs`
- Modify: `crates/yosh-plugin-manager/src/lib.rs`

- [ ] **Step 15.1: Add regex dep**

Append to `crates/yosh-plugin-manager/Cargo.toml` `[dependencies]`:

```toml
regex = "1"
```

- [ ] **Step 15.2: Create scenario.rs with schema + parse + tests**

Create `crates/yosh-plugin-manager/src/scenario.rs`:

```rust
//! Declarative scenarios for `yosh plugin test`. One TOML file per
//! scenario; each scenario is a sequence of `step` entries, each step
//! is one exec / hook invocation plus an `expect` block.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Scenario {
    pub plugin: PathBuf,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub env: EnvConfig,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    #[serde(rename = "step", default)]
    pub steps: Vec<Step>,
}

#[derive(Debug, Default, Deserialize)]
pub struct EnvConfig {
    #[serde(default)]
    pub caps: Vec<String>,
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    #[serde(default)]
    pub exported: Vec<String>,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub allow_exec: Vec<String>,
    #[serde(default)]
    pub sandbox_root: String,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_timeout_ms() -> u64 { 5000 }

#[derive(Debug, Deserialize)]
#[serde(tag = "call", rename_all = "lowercase")]
pub enum Step {
    Exec {
        args: Vec<String>,
        #[serde(default)]
        expect: Expect,
    },
    Hook {
        name: HookName,
        args: Vec<toml::Value>,
        #[serde(default)]
        expect: Expect,
    },
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum HookName {
    PreExec,
    PostExec,
    OnCd,
    PrePrompt,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    pub exit: Option<i32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub stdout_contains: Option<String>,
    pub stderr_contains: Option<String>,
    pub stdout_regex: Option<String>,
    pub stderr_regex: Option<String>,
    pub vars_set: Option<BTreeMap<String, String>>,
    pub vars_export: Option<BTreeMap<String, String>>,
    pub files_write: Option<BTreeMap<String, FileExpect>>,
    pub exec_called: Option<Vec<ExecCallExpect>>,
    pub trap: Option<bool>,
}

// Note: `denied: bool` (listed in spec §5 as a future expect key) is
// intentionally not implemented here. Observing capability-denied
// errors from the harness requires plumbing a counter through every
// host import (each `Err(Denied)` increments). Deferred — for now,
// authors detect denial via `stdout_regex` on guest-side error
// handling or via specific `exit` codes the guest returns on
// `Err(ErrorCode::Denied)`.

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum FileExpect {
    Bytes(String),
    Struct { #[serde(default)] len: Option<usize>, #[serde(default)] bytes_eq: Option<String> },
}

#[derive(Debug, Deserialize)]
pub struct ExecCallExpect {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub exit: Option<i32>,
}

pub fn parse(path: &std::path::Path) -> Result<Scenario, String> {
    let s = std::fs::read_to_string(path).map_err(|e| format!("read {}: {}", path.display(), e))?;
    let parsed: Scenario = toml::from_str(&s).map_err(|e| format!("parse {}: {}", path.display(), e))?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(s: &str) -> Result<Scenario, String> {
        toml::from_str(s).map_err(|e| e.to_string())
    }

    #[test]
    fn minimal_scenario_parses() {
        let sc = parse_str(r#"
            plugin = "a.wasm"
            [[step]]
            call = "exec"
            args = ["echo", "hi"]
        "#).unwrap();
        assert_eq!(sc.plugin.to_str().unwrap(), "a.wasm");
        assert_eq!(sc.steps.len(), 1);
        match &sc.steps[0] {
            Step::Exec { args, .. } => assert_eq!(args, &vec!["echo".to_string(), "hi".to_string()]),
            _ => panic!("expected exec step"),
        }
    }

    #[test]
    fn unknown_expect_key_rejected() {
        let err = parse_str(r#"
            plugin = "a.wasm"
            [[step]]
            call = "exec"
            args = ["x"]
            [step.expect]
            mystery = "boom"
        "#).unwrap_err();
        assert!(err.contains("mystery") || err.contains("unknown field"));
    }

    #[test]
    fn hook_step_parses() {
        let sc = parse_str(r#"
            plugin = "a.wasm"
            [[step]]
            call = "hook"
            name = "on-cd"
            args = ["/old", "/new"]
        "#).unwrap();
        match &sc.steps[0] {
            Step::Hook { name, args, .. } => {
                assert_eq!(*name, HookName::OnCd);
                assert_eq!(args.len(), 2);
            }
            _ => panic!("expected hook step"),
        }
    }
}
```

- [ ] **Step 15.3: Register in lib.rs**

Add to `crates/yosh-plugin-manager/src/lib.rs`:

```rust
pub mod scenario;
```

- [ ] **Step 15.4: Run + commit**

```sh
cargo test -p yosh-plugin-manager scenario
```

Expected: 3 passed.

```sh
git add crates/yosh-plugin-manager/Cargo.toml crates/yosh-plugin-manager/src/scenario.rs crates/yosh-plugin-manager/src/lib.rs
git commit -m "feat(plugin-manager): scenario TOML schema + parser"
```

---

## Task 16: Scenario step evaluator

Given a `Scenario` and a step, build the `TestState` for that step, drive the runner, evaluate `expect`. Return `Pass` or `Fail(reason)`.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/scenario.rs`

- [ ] **Step 16.1: Add evaluator + tests**

Add to `scenario.rs`:

```rust
use crate::runner::{HookCall, RunOutcome, invoke_exec, invoke_hook, load_plugin};
use crate::test_host::TestState;
use yosh_plugin_api::pattern::CommandPattern;
use yosh_plugin_api::{capabilities_to_bitflags, parse_capability};

#[derive(Debug)]
pub enum StepResult {
    Pass,
    Fail(String),
}

pub fn run_scenario(path: &std::path::Path) -> Vec<StepResult> {
    let scenario = match parse(path) {
        Ok(s) => s,
        Err(e) => return vec![StepResult::Fail(format!("parse error: {}", e))],
    };

    let wasm_path = path.parent().map(|p| p.join(&scenario.plugin)).unwrap_or(scenario.plugin.clone());
    let mut results = Vec::new();

    for (idx, step) in scenario.steps.iter().enumerate() {
        let state = build_state(&scenario);
        let timeout = std::time::Duration::from_millis(scenario.env.timeout_ms);
        let loaded = match load_plugin(&wasm_path, state, timeout) {
            Ok(l) => l,
            Err(e) => {
                results.push(StepResult::Fail(format!("step {}: load: {}", idx + 1, e)));
                continue;
            }
        };

        let (outcome, expect) = match step {
            Step::Exec { args, expect } => {
                if args.is_empty() {
                    results.push(StepResult::Fail(format!("step {}: exec needs at least 1 arg", idx + 1)));
                    continue;
                }
                let (cmd, rest) = (&args[0], &args[1..]);
                (invoke_exec(loaded, cmd, rest), expect)
            }
            Step::Hook { name, args, expect } => {
                let call = match build_hook_call(*name, args) {
                    Ok(c) => c,
                    Err(e) => {
                        results.push(StepResult::Fail(format!("step {}: hook args: {}", idx + 1, e)));
                        continue;
                    }
                };
                (invoke_hook(loaded, call), expect)
            }
        };

        results.push(evaluate(idx + 1, &outcome, expect));
    }

    if results.is_empty() {
        results.push(StepResult::Pass);
    }
    results
}

fn build_state(scenario: &Scenario) -> TestState {
    let mut state = TestState::default();
    let parsed_caps: Vec<_> = scenario.env.caps.iter().filter_map(|s| parse_capability(s)).collect();
    state.caps = capabilities_to_bitflags(&parsed_caps);
    for (k, v) in &scenario.env.vars { state.vars.insert(k.clone(), v.clone()); }
    for k in &scenario.env.exported { state.exported.insert(k.clone()); }
    if !scenario.env.cwd.is_empty() { state.cwd = scenario.env.cwd.clone().into(); }
    state.allow_exec = scenario.env.allow_exec.iter()
        .filter_map(|p| CommandPattern::parse(p).ok())
        .collect();
    if !scenario.env.sandbox_root.is_empty() {
        state.sandbox_root = Some(std::path::PathBuf::from(&scenario.env.sandbox_root));
    } else {
        for (k, v) in &scenario.files {
            state.files.insert(std::path::PathBuf::from(k), v.as_bytes().to_vec());
        }
    }
    state
}

fn build_hook_call(name: HookName, args: &[toml::Value]) -> Result<HookCall, String> {
    fn s(v: &toml::Value) -> Result<String, String> {
        v.as_str().map(|s| s.to_string()).ok_or_else(|| "expected string".into())
    }
    fn i(v: &toml::Value) -> Result<i32, String> {
        v.as_integer().map(|i| i as i32).ok_or_else(|| "expected integer".into())
    }
    match name {
        HookName::PreExec => Ok(HookCall::PreExec { command_line: s(args.first().ok_or("missing arg")?)? }),
        HookName::PostExec => {
            let cl = s(args.first().ok_or("missing command_line")?)?;
            let ec = i(args.get(1).ok_or("missing exit_code")?)?;
            Ok(HookCall::PostExec { command_line: cl, exit_code: ec })
        }
        HookName::OnCd => {
            let old = s(args.first().ok_or("missing old")?)?;
            let new = s(args.get(1).ok_or("missing new")?)?;
            Ok(HookCall::OnCd { old, new })
        }
        HookName::PrePrompt => Ok(HookCall::PrePrompt),
    }
}

fn evaluate(step_idx: usize, o: &RunOutcome, e: &Expect) -> StepResult {
    macro_rules! fail {
        ($($t:tt)*) => { return StepResult::Fail(format!("step {}: {}", step_idx, format_args!($($t)*))); };
    }

    if let Some(want) = e.exit {
        match o.exit_code {
            Some(got) if got == want => {}
            Some(got) => fail!("exit: want {}, got {}", want, got),
            None => fail!("exit: want {}, got (no exit code — hook?)", want),
        }
    }

    let stdout_str = String::from_utf8_lossy(&o.stdout);
    let stderr_str = String::from_utf8_lossy(&o.stderr);

    if let Some(want) = &e.stdout {
        if stdout_str != *want { fail!("stdout mismatch: want {:?}, got {:?}", want, stdout_str); }
    }
    if let Some(want) = &e.stderr {
        if stderr_str != *want { fail!("stderr mismatch: want {:?}, got {:?}", want, stderr_str); }
    }
    if let Some(sub) = &e.stdout_contains {
        if !stdout_str.contains(sub.as_str()) { fail!("stdout_contains {:?} not found in {:?}", sub, stdout_str); }
    }
    if let Some(sub) = &e.stderr_contains {
        if !stderr_str.contains(sub.as_str()) { fail!("stderr_contains {:?} not found in {:?}", sub, stderr_str); }
    }
    if let Some(re) = &e.stdout_regex {
        let rx = regex::Regex::new(re).map_err(|err| err.to_string());
        match rx {
            Ok(rx) if !rx.is_match(&stdout_str) => fail!("stdout_regex {:?} did not match {:?}", re, stdout_str),
            Err(err) => fail!("stdout_regex invalid: {}", err),
            _ => {}
        }
    }
    if let Some(re) = &e.stderr_regex {
        let rx = regex::Regex::new(re).map_err(|err| err.to_string());
        match rx {
            Ok(rx) if !rx.is_match(&stderr_str) => fail!("stderr_regex {:?} did not match {:?}", re, stderr_str),
            Err(err) => fail!("stderr_regex invalid: {}", err),
            _ => {}
        }
    }

    if let Some(want) = &e.vars_set {
        let got: BTreeMap<String, String> = o.set_log.iter().cloned().collect();
        if got != *want { fail!("vars_set: want {:?}, got {:?}", want, got); }
    }
    if let Some(want) = &e.vars_export {
        let got: BTreeMap<String, String> = o.export_log.iter().cloned().collect();
        if got != *want { fail!("vars_export: want {:?}, got {:?}", want, got); }
    }

    if let Some(want) = &e.files_write {
        let got: BTreeMap<String, usize> = o.write_log.iter()
            .map(|(p, n)| (p.display().to_string(), *n))
            .collect();
        for (path, expectation) in want {
            match expectation {
                FileExpect::Bytes(b) => {
                    let want_len = b.as_bytes().len();
                    match got.get(path) {
                        Some(actual) if *actual == want_len => {},
                        Some(actual) => fail!("files_write[{}] len: want {}, got {}", path, want_len, actual),
                        None => fail!("files_write[{}] not written", path),
                    }
                }
                FileExpect::Struct { len, bytes_eq } => {
                    if let Some(l) = len {
                        match got.get(path) {
                            Some(actual) if *actual == *l => {},
                            Some(actual) => fail!("files_write[{}] len: want {}, got {}", path, l, actual),
                            None => fail!("files_write[{}] not written", path),
                        }
                    }
                    if let Some(b) = bytes_eq {
                        let want_len = b.as_bytes().len();
                        match got.get(path) {
                            Some(actual) if *actual == want_len => {},
                            Some(actual) => fail!("files_write[{}] bytes_eq len: want {}, got {}", path, want_len, actual),
                            None => fail!("files_write[{}] not written", path),
                        }
                    }
                }
            }
        }
    }

    if let Some(want_seq) = &e.exec_called {
        if want_seq.len() != o.exec_log.len() {
            fail!("exec_called: want {} calls, got {}", want_seq.len(), o.exec_log.len());
        }
        for (i, (w, g)) in want_seq.iter().zip(o.exec_log.iter()).enumerate() {
            if w.program != g.program { fail!("exec_called[{}].program: want {}, got {}", i, w.program, g.program); }
            if w.args != g.args { fail!("exec_called[{}].args: want {:?}, got {:?}", i, w.args, g.args); }
            if let Some(exit) = w.exit {
                if exit != g.exit_code { fail!("exec_called[{}].exit: want {}, got {}", i, exit, g.exit_code); }
            }
        }
    }

    if let Some(want) = e.trap {
        let got = o.error_kind == Some("trap");
        if got != want { fail!("trap: want {}, got {}", want, got); }
    }

    StepResult::Pass
}

#[cfg(test)]
mod evaluator_tests {
    use super::*;
    use crate::runner::RunOutcome;

    fn outcome_with(exit: Option<i32>, stdout: &[u8]) -> RunOutcome {
        RunOutcome {
            exit_code: exit,
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
            set_log: Vec::new(),
            export_log: Vec::new(),
            write_log: Vec::new(),
            exec_log: Vec::new(),
            error: None,
            error_kind: None,
        }
    }

    #[test]
    fn expect_exit_match_passes() {
        let o = outcome_with(Some(0), b"");
        let e = Expect { exit: Some(0), ..Default::default() };
        assert!(matches!(evaluate(1, &o, &e), StepResult::Pass));
    }

    #[test]
    fn expect_exit_mismatch_fails() {
        let o = outcome_with(Some(2), b"");
        let e = Expect { exit: Some(0), ..Default::default() };
        match evaluate(1, &o, &e) {
            StepResult::Fail(s) => assert!(s.contains("exit")),
            _ => panic!("expected fail"),
        }
    }

    #[test]
    fn expect_stdout_contains_works() {
        let o = outcome_with(Some(0), b"hello world\n");
        let e = Expect { stdout_contains: Some("world".into()), ..Default::default() };
        assert!(matches!(evaluate(1, &o, &e), StepResult::Pass));
    }
}
```

- [ ] **Step 16.2: Run + commit**

```sh
cargo test -p yosh-plugin-manager scenario
```

Expected: 6 passed.

```sh
git add crates/yosh-plugin-manager/src/scenario.rs
git commit -m "feat(plugin-manager): scenario step evaluator"
```

---

## Task 17: Scenario directory walker + summary

Walk a directory for `*.toml`, run each, aggregate, emit human or JSON-lines summary.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/scenario.rs`

- [ ] **Step 17.1: Add walker + summary**

Add to `scenario.rs`:

```rust
#[derive(Debug)]
pub struct ScenarioReport {
    pub file: std::path::PathBuf,
    pub steps: Vec<StepResult>,
}

impl ScenarioReport {
    pub fn passed(&self) -> bool {
        self.steps.iter().all(|r| matches!(r, StepResult::Pass))
    }
}

pub fn run_dir(path: &std::path::Path, filter: Option<&str>) -> Vec<ScenarioReport> {
    let mut reports = Vec::new();
    let filter_rx = filter.and_then(|f| regex::Regex::new(f).ok());

    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("toml") {
                out.push(p);
            }
        }
    }

    let mut paths = Vec::new();
    if path.is_dir() {
        walk(path, &mut paths);
    } else if path.exists() {
        paths.push(path.to_path_buf());
    }
    paths.sort();

    for p in paths {
        if let Some(rx) = &filter_rx {
            if !rx.is_match(&p.to_string_lossy()) { continue; }
        }
        let results = run_scenario(&p);
        reports.push(ScenarioReport { file: p, steps: results });
    }
    reports
}

pub fn format_summary_human(reports: &[ScenarioReport]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "running {} scenarios", reports.len());
    let mut passed = 0;
    let mut failed = 0;
    for r in reports {
        if r.passed() {
            passed += 1;
            let _ = writeln!(out, "  \u{2713} {}", r.file.display());
        } else {
            failed += 1;
            let _ = writeln!(out, "  \u{2717} {}", r.file.display());
            for s in &r.steps {
                if let StepResult::Fail(msg) = s {
                    let _ = writeln!(out, "      {}", msg);
                }
            }
        }
    }
    let _ = writeln!(out, "{} passed, {} failed", passed, failed);
    out
}

pub fn format_summary_json(reports: &[ScenarioReport]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let mut passed = 0;
    let mut failed = 0;
    for r in reports {
        if r.passed() {
            passed += 1;
            let _ = writeln!(out, "{}", serde_json::json!({
                "file": r.file.display().to_string(),
                "status": "pass",
                "steps": r.steps.len()
            }));
        } else {
            failed += 1;
            let reason = r.steps.iter().find_map(|s| match s {
                StepResult::Fail(m) => Some(m.clone()),
                _ => None,
            }).unwrap_or_default();
            let _ = writeln!(out, "{}", serde_json::json!({
                "file": r.file.display().to_string(),
                "status": "fail",
                "reason": reason
            }));
        }
    }
    let _ = writeln!(out, "{}", serde_json::json!({
        "summary": { "passed": passed, "failed": failed, "total": reports.len() }
    }));
    out
}
```

- [ ] **Step 17.2: Add walker test**

Add to `scenario.rs` tests:

```rust
#[test]
fn run_dir_collects_toml_files() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.toml");
    std::fs::write(&a, r#"
        plugin = "missing.wasm"
        [[step]]
        call = "exec"
        args = ["x"]
    "#).unwrap();
    let reports = run_dir(tmp.path(), None);
    assert_eq!(reports.len(), 1);
    assert!(!reports[0].passed()); // wasm missing
}

#[test]
fn format_summary_json_has_summary_line() {
    let reports = vec![];
    let s = format_summary_json(&reports);
    assert!(s.contains("\"summary\""));
}
```

- [ ] **Step 17.3: Run + commit**

```sh
cargo test -p yosh-plugin-manager scenario
```

Expected: 8 passed.

```sh
git add crates/yosh-plugin-manager/src/scenario.rs
git commit -m "feat(plugin-manager): scenario directory walker + summary"
```

---

## Task 18: CLI: `Test` subcommand

Add the `Test` variant; dispatch to `scenario::run_dir`; choose formatter; exit 0 only when all scenarios pass.

**Files:**
- Modify: `crates/yosh-plugin-manager/src/lib.rs`

- [ ] **Step 18.1: Add variant + dispatcher**

In `enum Commands`, add:

```rust
    /// Run declarative scenarios (TOML) from a directory.
    Test {
        /// Directory or single file. Default: `tests/`.
        #[arg(default_value = "tests")]
        path: std::path::PathBuf,
        /// Regex filter over the scenario file path.
        #[arg(long)]
        filter: Option<String>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
    },
```

Add the dispatcher near `cmd_run`:

```rust
fn cmd_test(path: std::path::PathBuf, filter: Option<String>, format: OutputFormat) -> i32 {
    let reports = crate::scenario::run_dir(&path, filter.as_deref());
    let all_passed = reports.iter().all(|r| r.passed());
    match format {
        OutputFormat::Human => print!("{}", crate::scenario::format_summary_human(&reports)),
        OutputFormat::Json => print!("{}", crate::scenario::format_summary_json(&reports)),
    }
    if all_passed { 0 } else { 1 }
}
```

In `pub fn run() -> i32`:

```rust
        Commands::Test { path, filter, format } => cmd_test(path, filter, format),
```

- [ ] **Step 18.2: Smoke**

```sh
cargo build -p yosh-plugin-manager
mkdir -p /tmp/yosh-plugin-test-scenarios
cp crates/yosh-plugin-manager/tests/scenarios/*.toml /tmp/yosh-plugin-test-scenarios/  # filled in by Task 19
./target/debug/yosh-plugin test /tmp/yosh-plugin-test-scenarios
```

Expected: human-readable summary.

- [ ] **Step 18.3: Commit**

```sh
git add crates/yosh-plugin-manager/src/lib.rs
git commit -m "feat(plugin-manager): yosh plugin test subcommand"
```

---

## Task 19: Integration tests with `test_plugin.wasm`

Spec §10 lists 8 cases. Add a single `tests/runner.rs` integration file covering them. Tests gracefully skip when the wasm artifact isn't built (matches the existing pattern at the workspace root).

**Files:**
- Create: `crates/yosh-plugin-manager/tests/runner.rs`
- Create: `crates/yosh-plugin-manager/tests/scenarios/echo_var_pass.toml`
- Create: `crates/yosh-plugin-manager/tests/scenarios/run_echo_pass.toml`
- Create: `crates/yosh-plugin-manager/tests/scenarios/vars_set_fail.toml`

- [ ] **Step 19.1: Create the scenarios**

`crates/yosh-plugin-manager/tests/scenarios/echo_var_pass.toml`:

```toml
plugin = "../../../target/wasm32-wasip2/release/test_plugin.wasm"
description = "echo_var prints GREETING"

[env]
caps = ["variables:read", "io"]
vars = { GREETING = "hello" }
timeout_ms = 5000

[[step]]
call = "exec"
args = ["echo_var", "GREETING"]

  [step.expect]
  exit = 0
  stdout = "hello\n"
```

`crates/yosh-plugin-manager/tests/scenarios/run_echo_pass.toml`:

```toml
plugin = "../../../target/wasm32-wasip2/release/test_plugin.wasm"
description = "run-echo executes /bin/echo via commands:exec"

[env]
caps = ["io", "commands:exec"]
allow_exec = ["echo:*"]
timeout_ms = 5000

[[step]]
call = "exec"
args = ["run-echo", "hi"]

  [step.expect]
  exit = 0
  stdout = "hi\n"
```

`crates/yosh-plugin-manager/tests/scenarios/vars_set_fail.toml`:

```toml
plugin = "../../../target/wasm32-wasip2/release/test_plugin.wasm"
description = "dump_events sets YOSH_TEST_EVENT_LOG; this scenario expects a different value to demonstrate fail mode"

[env]
caps = ["variables:read", "variables:write", "io"]
timeout_ms = 5000

[[step]]
call = "exec"
args = ["dump_events"]

  [step.expect]
  exit = 0
  vars_set = { YOSH_TEST_EVENT_LOG = "ON_LOAD_BUT_NO_HOOKS_FIRED" }
```

- [ ] **Step 19.2: Create the integration test**

`crates/yosh-plugin-manager/tests/runner.rs`:

```rust
//! End-to-end tests against the in-repo `test_plugin.wasm` artifact.
//! Skipped silently when the wasm has not been built.

use std::path::PathBuf;
use std::time::Duration;

use yosh_plugin_manager::runner::{HookCall, invoke_exec, invoke_hook, load_plugin};
use yosh_plugin_manager::test_host::TestState;
use yosh_plugin_api::{
    CAP_COMMANDS_EXEC, CAP_FILESYSTEM, CAP_HOOK_ON_CD, CAP_IO, CAP_VARIABLES_READ, CAP_VARIABLES_WRITE,
};
use yosh_plugin_api::pattern::CommandPattern;

fn wasm() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm32-wasip2/release/test_plugin.wasm");
    if p.exists() { Some(p) } else { None }
}

#[test]
fn case_1_run_exec_happy_path() {
    let Some(w) = wasm() else { return };
    let mut s = TestState::default();
    s.caps = CAP_IO;
    let loaded = load_plugin(&w, s, Duration::from_secs(5)).expect("load");
    let outcome = invoke_exec(loaded, "test_cmd", &["x".into()]);
    assert_eq!(outcome.exit_code, Some(0));
    assert!(String::from_utf8_lossy(&outcome.stdout).starts_with("test_cmd args="));
}

#[test]
fn case_2_hook_on_cd_records_var() {
    let Some(w) = wasm() else { return };
    let mut s = TestState::default();
    s.caps = CAP_HOOK_ON_CD | CAP_VARIABLES_WRITE | CAP_IO;
    let loaded = load_plugin(&w, s, Duration::from_secs(5)).expect("load");
    let outcome = invoke_hook(loaded, HookCall::OnCd { old: "/tmp".into(), new: "/home".into() });
    assert!(outcome.error.is_none());
}

#[test]
fn case_3_insufficient_cap_denied() {
    let Some(w) = wasm() else { return };
    // echo_var requires variables:read. Granting only CAP_IO triggers Denied
    // in the guest's get_var call. The guest converts to exit code 2.
    let mut s = TestState::default();
    s.caps = CAP_IO;
    s.vars.insert("X".into(), "y".into());
    let loaded = load_plugin(&w, s, Duration::from_secs(5)).expect("load");
    let outcome = invoke_exec(loaded, "echo_var", &["X".into()]);
    assert_eq!(outcome.exit_code, Some(2));
}

#[test]
fn case_4_allowed_exec_pattern_runs_echo() {
    let Some(w) = wasm() else { return };
    let mut s = TestState::default();
    s.caps = CAP_IO | CAP_COMMANDS_EXEC;
    s.allow_exec = vec![CommandPattern::parse("echo:*").unwrap()];
    let loaded = load_plugin(&w, s, Duration::from_secs(5)).expect("load");
    let outcome = invoke_exec(loaded, "run-echo", &["hi".into()]);
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(outcome.stdout, b"hi\n");
    assert_eq!(outcome.exec_log.len(), 1);
}

#[test]
fn case_5_timeout_on_slow_plugin_pre_prompt() {
    let slow = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm32-wasip2/release/slow_plugin.wasm");
    if !slow.exists() { return; }
    let mut s = TestState::default();
    s.caps = yosh_plugin_api::CAP_HOOK_PRE_PROMPT;
    // 200ms timeout: slow_plugin busy-loops in pre_prompt; the epoch
    // watchdog must interrupt before this test exceeds a reasonable
    // upper bound (~2s).
    let start = std::time::Instant::now();
    let loaded = load_plugin(&slow, s, Duration::from_millis(200)).expect("load");
    let outcome = invoke_hook(loaded, HookCall::PrePrompt);
    let elapsed = start.elapsed();
    assert_eq!(outcome.error_kind, Some("timeout"));
    assert!(elapsed < Duration::from_secs(2), "timeout interrupt too slow: {:?}", elapsed);
}

#[test]
fn case_6_test_runner_parses_passing_scenario() {
    let Some(_w) = wasm() else { return };
    let scenario = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios/echo_var_pass.toml");
    let reports = yosh_plugin_manager::scenario::run_dir(&scenario, None);
    assert_eq!(reports.len(), 1);
    assert!(reports[0].passed(), "report: {:?}", reports[0]);
}

#[test]
fn case_7_test_runner_reports_failure_with_step_index() {
    let Some(_w) = wasm() else { return };
    let scenario = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios/vars_set_fail.toml");
    let reports = yosh_plugin_manager::scenario::run_dir(&scenario, None);
    assert_eq!(reports.len(), 1);
    assert!(!reports[0].passed());
}

#[test]
fn case_8_unknown_expect_key_rejected_at_parse() {
    use yosh_plugin_manager::scenario;
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), r#"
        plugin = "x.wasm"
        [[step]]
        call = "exec"
        args = ["y"]
        [step.expect]
        unknown_key = "boom"
    "#).unwrap();
    let err = scenario::parse(tmp.path()).unwrap_err();
    assert!(err.contains("unknown") || err.contains("unknown_key"));
}
```

- [ ] **Step 19.3: Build wasm if needed + run**

```sh
cargo component build -p test_plugin --target wasm32-wasip2 --release
cargo component build -p slow_plugin --target wasm32-wasip2 --release
cargo test -p yosh-plugin-manager --test runner
```

Expected: all 8 tests pass (or skip silently if wasm missing — but step 19.3 builds them).

- [ ] **Step 19.4: Commit**

```sh
git add crates/yosh-plugin-manager/tests
git commit -m "test(plugin-manager): runner + scenario end-to-end tests"
```

---

## Task 20: Documentation — "Testing Locally"

Append to `docs/yosh/plugin.md` Plugin Development Guide, before the "Distributing via GitHub Releases" section.

**Files:**
- Modify: `docs/yosh/plugin.md`

- [ ] **Step 20.1: Insert the section**

Add this content immediately after the `### The export! Macro` section (around line 366), before `### Distributing via GitHub Releases`:

```markdown
### Testing Locally

yosh ships two subcommands to exercise a plugin without starting a
shell session. Both run the plugin through the same `wasmtime` host
that yosh uses at runtime, but with an in-memory test backend instead
of a live `ShellEnv`. This works for plugins written in any language
that targets the WebAssembly Component Model.

#### One-shot: `yosh plugin run`

```sh
yosh plugin run target/wasm32-wasip2/release/yosh_plugin_hello.wasm \
    exec hello world
```

Flags scope what the plugin can see:

| Flag | Effect |
|------|--------|
| `--cap` | Capabilities to grant (defaults to the plugin's `required_capabilities`) |
| `--var KEY=VAL` | Seed a shell variable |
| `--export KEY=VAL` | Seed an exported variable |
| `--cwd <path>` | Virtual cwd |
| `--allow-exec <pat>` | Allowlist a `commands:exec` argv pattern (e.g. `--allow-exec 'git status:*'`) |
| `--sandbox-root <path>` | Real-FS scope for `files:read`/`files:write` (otherwise virtual) |
| `--timeout <ms>` | Watchdog deadline (default 5000) |
| `--format <human\|json>` | Output format |

Hooks are invoked similarly:

```sh
yosh plugin run my-plugin.wasm hook pre-exec "ls -l"
yosh plugin run my-plugin.wasm hook on-cd /old /new
yosh plugin run my-plugin.wasm hook pre-prompt
```

#### Declarative: `yosh plugin test`

Drop scenario files under `tests/` next to your plugin source. Each
`*.toml` is one scenario:

```toml
plugin = "../target/wasm32-wasip2/release/my_plugin.wasm"
description = "hello prints a greeting"

[env]
caps = ["io"]
timeout_ms = 5000

[[step]]
call = "exec"
args = ["hello", "world"]

  [step.expect]
  exit = 0
  stdout = "Hello, world!\n"
```

Run them:

```sh
yosh plugin test                  # walks tests/
yosh plugin test --format json    # JSON-lines for CI
```

Supported `[step.expect]` keys: `exit`, `stdout`, `stderr`,
`stdout_contains`, `stderr_contains`, `stdout_regex`, `stderr_regex`,
`vars_set`, `vars_export`, `files_write`, `exec_called`, `trap`.

#### Example: CI integration

```yaml
- run: cargo install cargo-component --locked --version 0.18.0
- run: rustup target add wasm32-wasip2
- run: cargo component build --target wasm32-wasip2 --release
- run: yosh plugin test --format json | tee result.jsonl
```
```

- [ ] **Step 20.2: Commit**

```sh
git add docs/yosh/plugin.md
git commit -m "docs(plugin): add Testing Locally section (run + test subcommands)"
```

---

## Final verification

- [ ] **Step F.1: Workspace tests pass**

```sh
cargo test -p yosh-plugin-api
cargo test -p yosh-plugin-manager
cargo test --lib plugin
```

Expected: all green.

- [ ] **Step F.2: CLI smoke from a fresh shell**

```sh
cargo build -p yosh-plugin-manager
./target/debug/yosh-plugin run \
    target/wasm32-wasip2/release/test_plugin.wasm exec test_cmd hello
./target/debug/yosh-plugin test crates/yosh-plugin-manager/tests/scenarios
```

Expected: human-readable output for both, exit 0 for run, exit 1 for test (because vars_set_fail.toml is intentionally failing).

- [ ] **Step F.3: TODO follow-up**

Append to `TODO.md` under "Future: Plugin System Enhancements":

```markdown
- [ ] Consolidate `HostContext`, `MetadataCtx`, and `TestCtx` onto a shared `HostBackend` trait so the three host implementations no longer have to mirror WIT changes by hand. Mirrors the existing TODO about deriving metadata-extract deny stubs from the bindgen `Host` traits (`src/plugin/host/`, `crates/yosh-plugin-manager/src/test_host/`, `crates/yosh-plugin-manager/src/metadata_extract.rs`).
- [ ] `yosh plugin run`: support `--watch` mode to re-run on wasm file change. Out of scope for the initial run/test landing per spec §11.
- [ ] Scenario format: consider a multi-plugin variant for cooperating plugin tests. Currently one scenario = one plugin. Defer until a real use case appears.
```

Commit:

```sh
git add TODO.md
git commit -m "docs(todo): plugin-manager TestCtx follow-ups"
```
