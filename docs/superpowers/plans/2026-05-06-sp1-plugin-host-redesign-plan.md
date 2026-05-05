# SP1 — `src/plugin/host.rs` Responsibility Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the 1004-line `src/plugin/host.rs` into a `host/` module with five capability submodules (`variables.rs`, `filesystem.rs`, `io.rs`, `files.rs`, `commands.rs`), and introduce two helpers (`HostContext::ensure_bound`, `HostContext::bound_env`) that enforce the §5 metadata-contract structurally instead of by per-function inlined guards.

**Architecture:** Two-phase refactor.
- **PR-A (scaffolding):** Add `ensure_bound` + `bound_env` helpers, then move `host.rs` into `host/{mod.rs, all.rs}`. `mod.rs` owns `HostContext` + helpers; `all.rs` is a temporary single-file home for every host/deny function and the existing tests, re-exported through `mod.rs` so `linker.rs` is untouched.
- **PR-B (capability split + helper rewrite):** Decompose `all.rs` into the five capability modules. Rewrite each `host_*` function to call `ensure_bound` (when env is unused) or `bound_env` (when env is read/written). Move test helpers to `mod.rs`'s `test_helpers` submodule. Collapse the 19 `metadata_contract_real_*_denied_when_env_null` tests into 7 tests (2 helper tests in `mod.rs` + 5 per-capability spot tests). Delete `all.rs`.

**Tech Stack:** Rust 2024, wasmtime + wasmtime-wasi 27, cargo test.

**Spec:** `docs/superpowers/specs/2026-05-06-sp1-plugin-host-redesign-design.md`
**Umbrella:** `docs/superpowers/specs/2026-05-06-large-file-redesign-umbrella-design.md`

**Naming-clash note (deviation from spec):** the spec proposes a single `HostContext::with_env` closure-based helper. `src/plugin/mod.rs` already contains a free function named `with_env` (the EnvGuard-based dispatch wrapper around guest calls), so a method of the same name on `HostContext` would shadow that name in this module's reading order. This plan splits the helper into two non-closure methods: `ensure_bound(&self) -> Result<(), ErrorCode>` and `bound_env(&mut self) -> Result<&mut ShellEnv, ErrorCode>`. The structural guarantee from the spec — every host function exits with `Err(Denied)` when env is null, and the guard is centralized — is preserved. The non-closure form also avoids the borrow-checker complication the spec flagged for `host_commands_exec` (no closure capture of `&mut HostContext`).

**Signature note (correction from spec):** `host_variables_get` returns `Result<Option<String>, ErrorCode>` (the spec stated `Result<String, ErrorCode>`). The plan reflects the actual signature.

---

## Pre-Flight Recon

### Task 0: Read context and confirm callers

- [ ] **Step 1: Read SP1 design document**

```bash
cat docs/superpowers/specs/2026-05-06-sp1-plugin-host-redesign-design.md
```

- [ ] **Step 2: Verify external callers of `host_*` and `deny_*` symbols**

The only external caller is `src/plugin/linker.rs`. Confirm:

```bash
grep -rn "use super::host::\|use crate::plugin::host::\|crate::plugin::host::" src/ tests/ --include='*.rs' | grep -v "src/plugin/host.rs:"
```

Expected: every match is in `src/plugin/linker.rs` referencing `host_*` and `deny_*` symbols by name. No other file imports from `crate::plugin::host`. Both PR-A and PR-B preserve these names via re-export from `host/mod.rs`, so `linker.rs` will require zero diff.

- [ ] **Step 3: Confirm baseline tests pass**

Run:
```bash
cargo test -p yosh --lib
```
Expected: PASS (no count requirement; record the count so post-PR comparison is possible).

Also run the integration test suite that depends on the WASM plugins (one-time WASM build required; safe to skip if you have already built them in this branch):
```bash
cargo component build -p test_plugin --target wasm32-wasip2 --release
cargo component build -p trap_plugin --target wasm32-wasip2 --release
cargo component build -p slow_plugin --target wasm32-wasip2 --release
cargo test --features test-helpers --test plugin
```
Expected: PASS.

- [ ] **Step 4: No commit (recon only)**

---

## PR-A: Scaffolding

PR-A is a behavior-preserving refactor. Bodies of every `host_*` and `deny_*` function are unchanged; only the file layout changes, plus two new helper methods on `HostContext`.

### Task A1: Add `ensure_bound` + `bound_env` helpers (TDD)

**Files:**
- Modify: `src/plugin/host.rs:91` (extend `impl HostContext`, append after `env_mut`)
- Modify: `src/plugin/host.rs:600+` (add tests inside the existing `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write failing tests for `ensure_bound`**

Append to the existing `tests` module in `src/plugin/host.rs` (right after the existing helper functions like `null_env_ctx` / `bound_env_ctx`):

```rust
#[test]
fn ensure_bound_returns_denied_when_env_null() {
    let ctx = null_env_ctx();
    assert_eq!(ctx.ensure_bound(), Err(ErrorCode::Denied));
}

#[test]
fn ensure_bound_returns_ok_when_env_bound() {
    let mut env = ShellEnv::new("yosh", vec![]);
    let ctx = bound_env_ctx(&mut env);
    assert_eq!(ctx.ensure_bound(), Ok(()));
}
```

- [ ] **Step 2: Run tests — verify FAIL**

```bash
cargo test -p yosh --lib plugin::host::tests::ensure_bound
```
Expected: FAIL with `error[E0599]: no method named ensure_bound found for struct HostContext`.

- [ ] **Step 3: Implement `ensure_bound`**

Add this method to `impl HostContext` in `src/plugin/host.rs`, immediately after the existing `env_mut` method:

```rust
    /// Metadata-contract guard: returns `Err(Denied)` if env is null
    /// (during `metadata()` or between `with_env` invocations), `Ok(())`
    /// otherwise. Used by host functions that do not need to read or
    /// write `ShellEnv` but still must reject calls during the
    /// metadata phase.
    pub(super) fn ensure_bound(&self) -> Result<(), ErrorCode> {
        if self.env.is_null() {
            Err(ErrorCode::Denied)
        } else {
            Ok(())
        }
    }
```

- [ ] **Step 4: Run tests — verify PASS**

```bash
cargo test -p yosh --lib plugin::host::tests::ensure_bound
```
Expected: 2 tests pass.

- [ ] **Step 5: Write failing tests for `bound_env`**

Append to the same `tests` module:

```rust
#[test]
fn bound_env_returns_denied_when_env_null() {
    let mut ctx = null_env_ctx();
    let result = ctx.bound_env();
    assert!(matches!(result, Err(ErrorCode::Denied)));
}

#[test]
fn bound_env_returns_env_when_bound() {
    let mut env = ShellEnv::new("yosh", vec![]);
    let mut ctx = bound_env_ctx(&mut env);
    let result = ctx.bound_env();
    assert!(result.is_ok());
}
```

- [ ] **Step 6: Run tests — verify FAIL**

```bash
cargo test -p yosh --lib plugin::host::tests::bound_env
```
Expected: FAIL with `no method named bound_env`.

- [ ] **Step 7: Implement `bound_env`**

Add to `impl HostContext` immediately after `ensure_bound`:

```rust
    /// Metadata-contract guard that also returns the bound `&mut ShellEnv`.
    /// Returns `Err(Denied)` if env is null. Used by host functions that
    /// need to read or write shell state.
    pub(super) fn bound_env(&mut self) -> Result<&mut ShellEnv, ErrorCode> {
        self.env_mut().ok_or(ErrorCode::Denied)
    }
```

- [ ] **Step 8: Run tests — verify PASS**

```bash
cargo test -p yosh --lib plugin::host::tests::bound_env
cargo test -p yosh --lib plugin::host::tests::ensure_bound
```
Expected: 4 tests pass total (2 ensure_bound + 2 bound_env).

- [ ] **Step 9: Run full host tests to confirm no regression**

```bash
cargo test -p yosh --lib plugin::host
```
Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add src/plugin/host.rs
git commit -m "$(cat <<'EOF'
refactor(plugin): add ensure_bound + bound_env helpers on HostContext

Two non-closure helpers on HostContext that centralize the §5
metadata-contract guard (env-null → Err(Denied)). PR-B will rewrite
every host_* function to delegate the guard to these helpers,
collapsing the 19 inlined guard sites and the 19 corresponding
metadata-contract tests.

Naming: ensure_bound (no env access) and bound_env (returns &mut env)
avoid the name collision with the existing free function with_env in
src/plugin/mod.rs (the EnvGuard-based dispatch wrapper).

Part of SP1 PR-A scaffolding. See
docs/superpowers/specs/2026-05-06-sp1-plugin-host-redesign-design.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task A2: Move `host.rs` into `host/{mod.rs, all.rs}`

**Files:**
- Move (via `git mv`): `src/plugin/host.rs` → `src/plugin/host/all.rs`
- Create: `src/plugin/host/mod.rs`
- Modify: `src/plugin/host/all.rs` (strip pieces that move to mod.rs, keep everything else)

This task is a pure file move + thin facade. No function bodies change. Validation is "existing tests still pass."

- [ ] **Step 1: Create the new directory and move the file**

```bash
mkdir -p src/plugin/host
git mv src/plugin/host.rs src/plugin/host/all.rs
```

- [ ] **Step 2: Create `src/plugin/host/mod.rs` with HostContext, helpers, and re-exports**

Create the file with this exact content:

```rust
//! HostContext + WasiView impl + the real / deny implementations of the
//! `yosh:plugin/*` host imports.
//!
//! See `docs/superpowers/specs/2026-04-27-wasm-plugin-runtime-design.md`
//! §5 "Execution Model" — `HostContext`, the metadata contract, and the
//! relationship to `EnvGuard` / the free `with_env` in `src/plugin/mod.rs`.
//!
//! Layout (per SP1 redesign,
//! docs/superpowers/specs/2026-05-06-sp1-plugin-host-redesign-design.md):
//! - this `mod.rs` owns `HostContext`, its `WasiView` impl, and the
//!   `ensure_bound` / `bound_env` helpers.
//! - During PR-A, `all.rs` is a temporary single-file home for every
//!   `host_*` and `deny_*` function with bodies preserved bit-for-bit.
//! - PR-B will split `all.rs` into per-capability submodules
//!   (`variables.rs`, `filesystem.rs`, `io.rs`, `files.rs`, `commands.rs`).

use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

use crate::env::ShellEnv;

use super::generated::yosh::plugin::types::ErrorCode;
use super::pattern::CommandPattern;

mod all;

pub(super) use all::{
    deny_commands_exec, deny_files_append_file, deny_files_create_dir, deny_files_metadata,
    deny_files_read_dir, deny_files_read_file, deny_files_remove_dir, deny_files_remove_file,
    deny_files_write_file, deny_filesystem_cwd, deny_filesystem_set_cwd, deny_io_write,
    deny_variables_export_env, deny_variables_get, deny_variables_set,
};
pub(super) use all::{
    host_commands_exec, host_files_append_file, host_files_create_dir, host_files_metadata,
    host_files_read_dir, host_files_read_file, host_files_remove_dir, host_files_remove_file,
    host_files_write_file, host_filesystem_cwd, host_filesystem_set_cwd, host_io_write,
    host_variables_export_env, host_variables_get, host_variables_set,
};

/// Per-plugin store data. See module docstring for invariants.
pub struct HostContext {
    /// Raw pointer to the live `ShellEnv`. Confined to a single `unsafe`
    /// helper (`env_mut`); this is the only `unsafe` site in the new host
    /// binding layer.
    pub(super) env: *mut ShellEnv,
    #[allow(dead_code)]
    pub(super) plugin_name: String,
    #[allow(dead_code)]
    pub(super) capabilities: u32,

    pub(super) wasi: WasiCtx,
    pub(super) resource_table: ResourceTable,
    pub(super) allowed_commands: Vec<CommandPattern>,
}

// SAFETY: see equivalent comment on the Send/Sync impls below.
unsafe impl Send for HostContext {}
unsafe impl Sync for HostContext {}

impl HostContext {
    pub fn new_for_plugin(plugin_name: impl Into<String>, capabilities: u32) -> Self {
        let wasi = WasiCtxBuilder::new().build();
        HostContext {
            env: std::ptr::null_mut(),
            plugin_name: plugin_name.into(),
            capabilities,
            wasi,
            resource_table: ResourceTable::new(),
            allowed_commands: Vec::new(),
        }
    }

    /// Borrow the live `ShellEnv` if currently bound. Returns `None` when
    /// `env` is null (during `metadata()` calls or between `with_env`
    /// invocations).
    ///
    /// SAFETY: callers must hold exclusive access to the `Store<HostContext>`,
    /// which is implied by `&mut self` here. The pointer's lifetime is
    /// managed by `EnvGuard` in `mod.rs` and is guaranteed to be valid
    /// for the duration of any `with_env` callback.
    pub(super) fn env_mut(&mut self) -> Option<&mut ShellEnv> {
        if self.env.is_null() {
            None
        } else {
            // SAFETY: `EnvGuard::bind` set this pointer from a live
            // `&mut ShellEnv`; it is reset to null on guard drop. The
            // shell is single-threaded for plugin dispatch.
            Some(unsafe { &mut *self.env })
        }
    }

    /// Metadata-contract guard: returns `Err(Denied)` if env is null
    /// (during `metadata()` or between `with_env` invocations), `Ok(())`
    /// otherwise. Used by host functions that do not need to read or
    /// write `ShellEnv` but still must reject calls during the
    /// metadata phase.
    pub(super) fn ensure_bound(&self) -> Result<(), ErrorCode> {
        if self.env.is_null() {
            Err(ErrorCode::Denied)
        } else {
            Ok(())
        }
    }

    /// Metadata-contract guard that also returns the bound `&mut ShellEnv`.
    /// Returns `Err(Denied)` if env is null. Used by host functions that
    /// need to read or write shell state.
    pub(super) fn bound_env(&mut self) -> Result<&mut ShellEnv, ErrorCode> {
        self.env_mut().ok_or(ErrorCode::Denied)
    }
}

impl WasiView for HostContext {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.resource_table
    }
}
```

- [ ] **Step 3: Strip duplicated symbols from `src/plugin/host/all.rs`**

`mod.rs` now owns `HostContext`, the `Send`/`Sync` impls, the `WasiView` impl, the inherent impl block (`new_for_plugin`, `env_mut`, `ensure_bound`, `bound_env`), and the module-level `use` statements those need. Remove these from `all.rs`. Concretely:

- Delete lines 1–101 of `all.rs` (the doc comment header, all `use` statements at the top, the `HostContext` struct, both `unsafe impl` blocks, the `impl HostContext` block, and the `impl WasiView for HostContext` block).
- Replace the deleted region with a minimal header that re-uses the imports the remaining code needs:

```rust
//! Temporary single-file home for host_* and deny_* functions.
//!
//! PR-A scaffolding step (see SP1 design doc). PR-B splits this file
//! into per-capability submodules and deletes it.

use std::io::Write;

use super::HostContext;
use super::super::generated::yosh::plugin::commands::ExecOutput;
use super::super::generated::yosh::plugin::files::{DirEntry, FileStat};
use super::super::generated::yosh::plugin::types::{ErrorCode, IoStream};
use std::time::UNIX_EPOCH;
```

(The previous in-file `use super::generated::yosh::plugin::files::{DirEntry, FileStat};` and `use std::time::UNIX_EPOCH;` further down the file should be deleted now that they are at the top.)

The `ensure_bound` / `bound_env` tests added in Task A1 stay in `all.rs::tests` for now — they exercise methods that live on `HostContext` (in `mod.rs`); the `super::*` import in the existing `tests` module will still resolve them via `super::HostContext`, but the test bodies reference `null_env_ctx()` / `bound_env_ctx()` directly, which are still in `all.rs::tests`. No change required for those tests beyond what already lands.

- [ ] **Step 4: Run cargo build to surface any leftover symbol issues**

```bash
cargo build -p yosh
```
Expected: PASS. If you see `unresolved import` errors in `all.rs`, they indicate a path that needs `super::super::` instead of `super::` (since `all.rs` is now one level deeper). Fix in place.

- [ ] **Step 5: Run lib tests**

```bash
cargo test -p yosh --lib plugin
```
Expected: PASS. The 19 metadata-contract tests, the 9 host_files_* behavior tests, the 9 host_commands_* tests, and the 4 helpers tests from Task A1 all run from `all.rs::tests`.

- [ ] **Step 6: Run integration tests with WASM plugins**

```bash
cargo test --features test-helpers --test plugin
```
Expected: PASS. (If WASM artifacts are stale, the `ensure_built` fixture rebuilds them on demand.)

- [ ] **Step 7: Commit**

```bash
git add src/plugin/host
git commit -m "$(cat <<'EOF'
refactor(plugin): split host.rs into host/{mod,all}.rs

Pure file-layout change. mod.rs owns HostContext, its WasiView impl,
the Send/Sync impls, and the inherent impl block (new_for_plugin,
env_mut, ensure_bound, bound_env). all.rs is a temporary single-file
home for every host_* / deny_* function and the existing tests, with
bodies preserved bit-for-bit. mod.rs re-exports each function name
under pub(super) use so src/plugin/linker.rs requires zero diff.

PR-B will split all.rs into per-capability submodules (variables,
filesystem, io, files, commands) and delete it.

Part of SP1 PR-A scaffolding. See
docs/superpowers/specs/2026-05-06-sp1-plugin-host-redesign-design.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

PR-A is complete after Tasks A1 + A2.

---

## PR-B: Capability Split + Helper Rewrite

PR-B decomposes `all.rs` into five capability modules, rewrites every host function to use the new helpers, consolidates test helpers, and prunes redundant metadata-contract tests.

### Task B1: Move test helpers to `host/mod.rs::test_helpers`

The helpers (`null_env_ctx`, `bound_env_ctx`, `ctx_with_allowed`) currently live inside `all.rs::tests`. They will be needed by every capability module's tests after the split, so they move to a `#[cfg(test)] mod test_helpers` in `mod.rs`.

**Files:**
- Modify: `src/plugin/host/mod.rs` (append a `test_helpers` module)
- Modify: `src/plugin/host/all.rs` (remove the helper definitions, import them via `super::test_helpers::*`)

- [ ] **Step 1: Append `test_helpers` to `src/plugin/host/mod.rs`**

Append at the end of `mod.rs`:

```rust
#[cfg(test)]
pub(super) mod test_helpers {
    //! Test fixtures shared by every capability submodule.
    //!
    //! `null_env_ctx` produces a `HostContext` with `env = null` to
    //! exercise the metadata-contract guard. `bound_env_ctx` binds a
    //! real `ShellEnv` so happy-path tests can proceed past the guard.
    //! `ctx_with_allowed` adds command-pattern allowlists for
    //! `commands_exec` tests.

    use super::super::pattern::CommandPattern;
    use super::HostContext;
    use crate::env::ShellEnv;
    use yosh_plugin_api::CAP_ALL;

    pub fn null_env_ctx() -> HostContext {
        // CAP_ALL is intentional — the deny short-circuit we test
        // fires regardless of granted capabilities, because it lives
        // inside the *real* implementations.
        HostContext::new_for_plugin("<test>", CAP_ALL)
    }

    pub fn bound_env_ctx(env: &mut ShellEnv) -> HostContext {
        let mut ctx = HostContext::new_for_plugin("<test>", CAP_ALL);
        ctx.env = env as *mut ShellEnv;
        ctx
    }

    pub fn ctx_with_allowed(env: &mut ShellEnv, patterns: &[&str]) -> HostContext {
        let mut ctx = bound_env_ctx(env);
        ctx.allowed_commands = patterns
            .iter()
            .map(|s| CommandPattern::parse(s).expect("valid pattern"))
            .collect();
        ctx
    }
}
```

- [ ] **Step 2: Update `src/plugin/host/all.rs::tests` to import from `super::test_helpers`**

In `all.rs`, locate the `#[cfg(test)] mod tests` block. Replace the in-module definitions of `null_env_ctx`, `bound_env_ctx`, `ctx_with_allowed` with a `use` statement at the top of the `tests` module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_helpers::{bound_env_ctx, ctx_with_allowed, null_env_ctx};
    use tempfile::tempdir;

    // (delete the inline `fn null_env_ctx`, `fn bound_env_ctx`, `fn ctx_with_allowed` definitions)
    // (the rest of the test bodies stay unchanged)
    // ...
}
```

The existing `use yosh_plugin_api::CAP_ALL;` at the top of the `tests` module can be removed (it was only used by the inline `null_env_ctx`).

- [ ] **Step 3: Run all tests**

```bash
cargo test -p yosh --lib plugin
```
Expected: PASS. Test count unchanged.

- [ ] **Step 4: Commit**

```bash
git add src/plugin/host
git commit -m "$(cat <<'EOF'
refactor(plugin): hoist test helpers to host/mod.rs::test_helpers

null_env_ctx, bound_env_ctx, and ctx_with_allowed move to a shared
#[cfg(test)] pub(super) mod test_helpers in host/mod.rs so that the
per-capability submodules introduced in subsequent commits can import
them via super::test_helpers::*.

Part of SP1 PR-B prep. See
docs/superpowers/specs/2026-05-06-sp1-plugin-host-redesign-design.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task B2: Extract `host/variables.rs` (3 fns + spot test)

This is the canonical extraction — every subsequent capability follows the same pattern.

**Files:**
- Create: `src/plugin/host/variables.rs`
- Modify: `src/plugin/host/all.rs` (remove the variables host/deny functions and their metadata-contract tests)
- Modify: `src/plugin/host/mod.rs` (re-export from `variables` instead of `all`)

- [ ] **Step 1: Create `src/plugin/host/variables.rs`**

Move the three host functions (`host_variables_get`, `host_variables_set`, `host_variables_export_env`) and three deny stubs out of `all.rs` and into a new `variables.rs`. **Rewrite each host function to use `bound_env`.** Leave the deny stubs unchanged.

Create the file with this exact content:

```rust
//! `yosh:plugin/variables` host imports — read/write/export shell
//! variables. Granted via CAP_VARIABLES. The deny stubs short-circuit
//! to `Err(Denied)` regardless of env state and are wired in by the
//! linker when the capability bit is clear.

use super::super::generated::yosh::plugin::types::ErrorCode;
use super::HostContext;

pub(super) fn host_variables_get(
    ctx: &mut HostContext,
    name: String,
) -> Result<Option<String>, ErrorCode> {
    let env = ctx.bound_env()?;
    Ok(env.vars.get(&name).map(|s| s.to_string()))
}

pub(super) fn deny_variables_get(
    _ctx: &mut HostContext,
    _name: String,
) -> Result<Option<String>, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn host_variables_set(
    ctx: &mut HostContext,
    name: String,
    value: String,
) -> Result<(), ErrorCode> {
    let env = ctx.bound_env()?;
    env.vars.set(&name, &value).map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn deny_variables_set(
    _ctx: &mut HostContext,
    _name: String,
    _value: String,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

/// `variables.export-env` — name in WIT is `export-env` (because
/// `export` is a reserved WIT keyword); the wit-bindgen-generated
/// Rust function is `export_env`.
pub(super) fn host_variables_export_env(
    ctx: &mut HostContext,
    name: String,
    value: String,
) -> Result<(), ErrorCode> {
    let env = ctx.bound_env()?;
    env.vars
        .set(&name, &value)
        .map_err(|_| ErrorCode::IoFailed)?;
    env.vars.export(&name);
    Ok(())
}

pub(super) fn deny_variables_export_env(
    _ctx: &mut HostContext,
    _name: String,
    _value: String,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

#[cfg(test)]
mod tests {
    //! One spot test confirms the metadata-contract is reachable through
    //! the variables capability. The full structural guarantee is
    //! verified by the `ensure_bound` / `bound_env` tests in
    //! `host/mod.rs`. Behavioral tests for set/get/export-env round-trip
    //! belong in this module too — add them as the need arises (the
    //! existing legacy tests covered only the deny path).

    use super::*;
    use super::super::test_helpers::null_env_ctx;

    #[test]
    fn variables_get_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_variables_get(&mut ctx, "PATH".into());
        assert_eq!(result, Err(ErrorCode::Denied));
    }
}
```

- [ ] **Step 2: Remove the variables fns and their tests from `src/plugin/host/all.rs`**

In `all.rs`:

1. Delete the production code for `host_variables_get`, `deny_variables_get`, `host_variables_set`, `deny_variables_set`, `host_variables_export_env`, `deny_variables_export_env` (lines roughly 107–168 of the post-A2 file). The `// ── yosh:plugin/variables host imports ──` section comment goes too.
2. Delete these tests from `all.rs::tests`:
   - `metadata_contract_real_variables_get_denied_when_env_null`
   - `metadata_contract_real_variables_set_denied_when_env_null`
   - `metadata_contract_real_variables_export_env_denied_when_env_null`

(All three are made redundant by the `mod.rs` `bound_env` tests plus the variables.rs spot test.)

- [ ] **Step 3: Update `src/plugin/host/mod.rs` re-exports**

Add `mod variables;` after the existing `mod all;` line. Move the three variables-related re-exports out of the `pub(super) use all::{...}` block and into a new block sourced from `variables`:

```rust
mod all;
mod variables;

pub(super) use all::{
    deny_commands_exec, deny_files_append_file, deny_files_create_dir, deny_files_metadata,
    deny_files_read_dir, deny_files_read_file, deny_files_remove_dir, deny_files_remove_file,
    deny_files_write_file, deny_filesystem_cwd, deny_filesystem_set_cwd, deny_io_write,
};
pub(super) use all::{
    host_commands_exec, host_files_append_file, host_files_create_dir, host_files_metadata,
    host_files_read_dir, host_files_read_file, host_files_remove_dir, host_files_remove_file,
    host_files_write_file, host_filesystem_cwd, host_filesystem_set_cwd, host_io_write,
};
pub(super) use variables::{
    deny_variables_export_env, deny_variables_get, deny_variables_set,
    host_variables_export_env, host_variables_get, host_variables_set,
};
```

- [ ] **Step 4: Build and run all tests**

```bash
cargo build -p yosh
cargo test -p yosh --lib plugin
```
Expected: PASS. Three tests fewer than baseline (variables metadata-contract tests deleted) plus one new spot test in `variables.rs::tests` — net of −2.

- [ ] **Step 5: Commit**

```bash
git add src/plugin/host
git commit -m "$(cat <<'EOF'
refactor(plugin): extract variables capability to host/variables.rs

Move host_variables_{get,set,export_env} and their deny stubs to a
new submodule. Rewrite the three host functions to use bound_env
instead of inlined ctx.env_mut().ok_or(Denied)?.

Test consolidation: drop the three metadata_contract_real_variables_*
tests (redundant once bound_env is the structural choke-point in
host/mod.rs); keep one variables_get_denied_when_env_null spot test
to confirm the deny path is reachable through this capability.

linker.rs unchanged — re-exports from host/mod.rs preserve the public
symbol names.

Part of SP1 PR-B.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task B3: Extract `host/filesystem.rs` (2 fns + spot test)

Apply the same pattern. The two filesystem host functions do **not** read or write the bound `ShellEnv` (they just guard on metadata, then call `std::env::current_dir` / `set_current_dir`). They use `ensure_bound`, not `bound_env`.

**Files:**
- Create: `src/plugin/host/filesystem.rs`
- Modify: `src/plugin/host/all.rs` (delete the filesystem fns and metadata-contract tests for them)
- Modify: `src/plugin/host/mod.rs` (`mod filesystem;`, move the two filesystem re-exports to a `pub(super) use filesystem::{...}` line)

- [ ] **Step 1: Create `src/plugin/host/filesystem.rs`**

```rust
//! `yosh:plugin/filesystem` host imports — cwd / set-cwd. Granted
//! via CAP_FILESYSTEM. These do not read or write `ShellEnv` directly;
//! they call `std::env::current_dir` / `set_current_dir` after the
//! metadata-contract guard.

use super::super::generated::yosh::plugin::types::ErrorCode;
use super::HostContext;

pub(super) fn host_filesystem_cwd(ctx: &mut HostContext) -> Result<String, ErrorCode> {
    ctx.ensure_bound()?;
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn deny_filesystem_cwd(_ctx: &mut HostContext) -> Result<String, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn host_filesystem_set_cwd(
    ctx: &mut HostContext,
    path: String,
) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    std::env::set_current_dir(&path).map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn deny_filesystem_set_cwd(
    _ctx: &mut HostContext,
    _path: String,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_helpers::null_env_ctx;

    #[test]
    fn filesystem_cwd_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        assert_eq!(host_filesystem_cwd(&mut ctx), Err(ErrorCode::Denied));
    }
}
```

- [ ] **Step 2: Remove the filesystem section from `all.rs`**

Delete the production code for `host_filesystem_cwd`, `deny_filesystem_cwd`, `host_filesystem_set_cwd`, `deny_filesystem_set_cwd` (and the `// ── yosh:plugin/filesystem host imports ──` section comment).

Delete these tests from `all.rs::tests`:
- `metadata_contract_real_cwd_denied_when_env_null`
- `metadata_contract_real_set_cwd_denied_when_env_null`

- [ ] **Step 3: Update `mod.rs`**

Add `mod filesystem;` after `mod variables;`. Move the two filesystem re-exports out of the `all::{...}` blocks into a new `pub(super) use filesystem::{...}` block.

- [ ] **Step 4: Build and run tests**

```bash
cargo test -p yosh --lib plugin
```
Expected: PASS. Net test delta: −2 metadata + +1 spot = −1.

- [ ] **Step 5: Commit**

```bash
git add src/plugin/host
git commit -m "$(cat <<'EOF'
refactor(plugin): extract filesystem capability to host/filesystem.rs

Move host_filesystem_{cwd,set_cwd} and their deny stubs to a new
submodule. These functions do not touch ShellEnv beyond the metadata
guard, so they delegate to ensure_bound (no env borrow needed).

Test consolidation: drop the two metadata_contract_real_{cwd,set_cwd}
tests; keep one filesystem_cwd_denied_when_env_null spot test.

Part of SP1 PR-B.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task B4: Extract `host/io.rs` (1 fn + spot test)

Same pattern. `host_io_write` does not read env; it uses `ensure_bound`.

**Files:**
- Create: `src/plugin/host/io.rs`
- Modify: `src/plugin/host/all.rs`
- Modify: `src/plugin/host/mod.rs`

- [ ] **Step 1: Create `src/plugin/host/io.rs`**

```rust
//! `yosh:plugin/io` host import — write to host stdout/stderr.
//! Granted via CAP_IO.

use std::io::Write;

use super::super::generated::yosh::plugin::types::{ErrorCode, IoStream};
use super::HostContext;

pub(super) fn host_io_write(
    ctx: &mut HostContext,
    target: IoStream,
    data: Vec<u8>,
) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    let result = match target {
        IoStream::Stdout => std::io::stdout().write_all(&data),
        IoStream::Stderr => std::io::stderr().write_all(&data),
    };
    result.map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn deny_io_write(
    _ctx: &mut HostContext,
    _target: IoStream,
    _data: Vec<u8>,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_helpers::null_env_ctx;

    #[test]
    fn io_write_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_io_write(&mut ctx, IoStream::Stdout, b"hi".to_vec());
        assert_eq!(result, Err(ErrorCode::Denied));
    }
}
```

- [ ] **Step 2: Remove from `all.rs`**

Delete `host_io_write`, `deny_io_write`, the `// ── yosh:plugin/io host imports ──` section header, and the `metadata_contract_real_io_write_denied_when_env_null` test.

Also delete `use std::io::Write;` from the top of `all.rs` (no longer needed there).

- [ ] **Step 3: Update `mod.rs`**

Add `mod io;`. Move `host_io_write` and `deny_io_write` out of `all::{...}` re-exports into a new `pub(super) use io::{deny_io_write, host_io_write};` line.

- [ ] **Step 4: Build and run tests**

```bash
cargo test -p yosh --lib plugin
```
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/plugin/host
git commit -m "$(cat <<'EOF'
refactor(plugin): extract io capability to host/io.rs

Move host_io_write and deny_io_write to a new submodule. host_io_write
delegates the metadata guard to ensure_bound.

Test consolidation: drop metadata_contract_real_io_write_denied_when_env_null;
keep one io_write_denied_when_env_null spot test.

Part of SP1 PR-B.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task B5: Extract `host/files.rs` (8 fns + spot test + behavioral tests)

This is the largest capability. The eight `host_files_*` functions plus their deny stubs and the nine `host_files_*` behavioral tests all move together. The spot test stays for the metadata-contract; the eight per-function metadata-contract tests are dropped (the structural guarantee is now in `mod.rs`'s `bound_env` test plus the spot test).

**Files:**
- Create: `src/plugin/host/files.rs`
- Modify: `src/plugin/host/all.rs`
- Modify: `src/plugin/host/mod.rs`

- [ ] **Step 1: Create `src/plugin/host/files.rs`**

Build the file by lifting the exact bodies from `all.rs`, replacing each `if ctx.env_mut().is_none() { return Err(ErrorCode::Denied); }` guard with `ctx.ensure_bound()?;`. Bodies are otherwise identical. Layout:

```rust
//! `yosh:plugin/files` host imports — read/write/inspect filesystem
//! within the plugin sandbox. Granted via CAP_FILES_READ /
//! CAP_FILES_WRITE. None of these functions read or write `ShellEnv`,
//! so the metadata guard delegates to `ensure_bound`.
//!
//! Error mapping table (see spec
//! docs/superpowers/specs/2026-04-29-plugin-files-rw-capability-design.md
//! §4):
//! - empty path                 → InvalidArgument
//! - std::io::ErrorKind::NotFound (read side) → NotFound
//! - other I/O errors           → IoFailed

use std::time::UNIX_EPOCH;

use super::super::generated::yosh::plugin::files::{DirEntry, FileStat};
use super::super::generated::yosh::plugin::types::ErrorCode;
use super::HostContext;

pub(super) fn host_files_read_file(
    ctx: &mut HostContext,
    path: String,
) -> Result<Vec<u8>, ErrorCode> {
    ctx.ensure_bound()?;
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
    ctx.ensure_bound()?;
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
    ctx.ensure_bound()?;
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
    ctx.ensure_bound()?;
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
    ctx.ensure_bound()?;
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
    ctx.ensure_bound()?;
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

pub(super) fn host_files_remove_file(ctx: &mut HostContext, path: String) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
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
    ctx.ensure_bound()?;
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

#[cfg(test)]
mod tests {
    //! Spot test for the metadata-contract through this capability,
    //! plus the nine §8 host happy-path / error-mapping tests
    //! prescribed by the 2026-04-29 plugin-files-rw-capability spec.

    use super::*;
    use super::super::test_helpers::{bound_env_ctx, null_env_ctx};
    use crate::env::ShellEnv;
    use tempfile::tempdir;

    #[test]
    fn files_read_file_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_files_read_file(&mut ctx, "/tmp/anything".into());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    // ── Spec §8 happy-path / error-mapping tests ───────────────────────

    #[test]
    fn host_files_read_file_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("data.txt");
        let payload = b"hello world".to_vec();
        std::fs::write(&path, &payload).unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        let result = host_files_read_file(&mut ctx, path.to_string_lossy().into_owned());
        assert_eq!(result, Ok(payload));
    }

    #[test]
    fn host_files_read_dir_returns_entries() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        let entries =
            host_files_read_dir(&mut ctx, dir.path().to_string_lossy().into_owned()).unwrap();

        assert_eq!(entries.len(), 2);
        let a = entries.iter().find(|e| e.name == "a.txt").expect("a.txt");
        assert!(a.is_file);
        assert!(!a.is_dir);
        let sub = entries.iter().find(|e| e.name == "sub").expect("sub");
        assert!(!sub.is_file);
        assert!(sub.is_dir);
    }

    #[test]
    fn host_files_metadata_distinguishes_file_and_dir() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("f");
        std::fs::write(&file_path, b"abc").unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);

        let f = host_files_metadata(&mut ctx, file_path.to_string_lossy().into_owned()).unwrap();
        assert!(f.is_file);
        assert!(!f.is_dir);
        assert_eq!(f.size, 3);

        let d = host_files_metadata(&mut ctx, dir.path().to_string_lossy().into_owned()).unwrap();
        assert!(!d.is_file);
        assert!(d.is_dir);
    }

    #[test]
    fn host_files_read_file_returns_not_found_for_missing_path() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("does-not-exist.txt");

        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        let result = host_files_read_file(&mut ctx, missing.to_string_lossy().into_owned());
        assert_eq!(result, Err(ErrorCode::NotFound));
    }

    #[test]
    fn host_files_read_file_invalid_argument_on_empty_path() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        let result = host_files_read_file(&mut ctx, String::new());
        assert_eq!(result, Err(ErrorCode::InvalidArgument));
    }

    #[test]
    fn host_files_remove_dir_io_failed_on_nonempty_without_recursive() {
        let dir = tempdir().unwrap();
        let inner = dir.path().join("d");
        std::fs::create_dir(&inner).unwrap();
        std::fs::write(inner.join("f"), b"x").unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        let result = host_files_remove_dir(&mut ctx, inner.to_string_lossy().into_owned(), false);
        assert_eq!(result, Err(ErrorCode::IoFailed));
        assert!(inner.exists());
    }

    #[test]
    fn host_files_append_file_appends() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("log");

        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        let p = path.to_string_lossy().into_owned();

        host_files_write_file(&mut ctx, p.clone(), b"hello".to_vec()).unwrap();
        host_files_append_file(&mut ctx, p, b" world".to_vec()).unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes, b"hello world");
    }

    #[test]
    fn host_files_create_dir_all_creates_intermediate_dirs() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("a/b/c");

        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        host_files_create_dir(&mut ctx, nested.to_string_lossy().into_owned(), true).unwrap();

        assert!(nested.is_dir());
        assert!(dir.path().join("a").is_dir());
        assert!(dir.path().join("a/b").is_dir());
    }

    #[test]
    fn host_files_remove_dir_recursive_removes_subtree() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("tree");
        std::fs::create_dir_all(root.join("inner")).unwrap();
        std::fs::write(root.join("f"), b"x").unwrap();
        std::fs::write(root.join("inner/g"), b"y").unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        host_files_remove_dir(&mut ctx, root.to_string_lossy().into_owned(), true).unwrap();

        assert!(!root.exists());
    }
}
```

- [ ] **Step 2: Remove from `all.rs`**

Delete:

- All eight `host_files_*` functions and all eight `deny_files_*` stubs.
- The `// ── yosh:plugin/files host imports ──` section header.
- The `use super::generated::yosh::plugin::files::{DirEntry, FileStat};` and `use std::time::UNIX_EPOCH;` if they were originally inside `all.rs`'s body region (they are now in `files.rs`). The header-region `use` block at the top of `all.rs` only needs them if other remaining functions use them — they don't, so delete those imports too.
- The eight metadata-contract tests in `all.rs::tests`:
  - `metadata_contract_real_files_read_file_denied_when_env_null`
  - `metadata_contract_real_files_read_dir_denied_when_env_null`
  - `metadata_contract_real_files_metadata_denied_when_env_null`
  - `metadata_contract_real_files_write_file_denied_when_env_null`
  - `metadata_contract_real_files_append_file_denied_when_env_null`
  - `metadata_contract_real_files_create_dir_denied_when_env_null`
  - `metadata_contract_real_files_remove_file_denied_when_env_null`
  - `metadata_contract_real_files_remove_dir_denied_when_env_null`
- The nine `host_files_*` behavioral tests (they have moved to `files.rs::tests`).

Also remove `use tempfile::tempdir;` from `all.rs::tests` if no remaining test uses it (the commands tests do — keep it if so).

- [ ] **Step 3: Update `mod.rs`**

Add `mod files;`. Move all eight `host_files_*` and eight `deny_files_*` re-exports out of the `all::{...}` blocks and into a new `pub(super) use files::{...}` block.

- [ ] **Step 4: Build and run tests**

```bash
cargo test -p yosh --lib plugin
```
Expected: PASS. Net test delta for this task: −8 metadata tests + 1 spot test + 9 behavioral tests preserved (just moved) = −7.

- [ ] **Step 5: Commit**

```bash
git add src/plugin/host
git commit -m "$(cat <<'EOF'
refactor(plugin): extract files capability to host/files.rs

Move all eight host_files_* functions, eight deny stubs, and the nine
spec-§8 behavioral tests to a new submodule. Each host function
delegates the metadata guard to ensure_bound (none of them read or
write ShellEnv).

Test consolidation: drop the eight metadata_contract_real_files_*
tests; keep one files_read_file_denied_when_env_null spot test.
The nine behavioral tests (read_file_roundtrip, read_dir_returns_entries,
metadata_distinguishes_file_and_dir, ...) are preserved verbatim.

files.rs is ~440 lines (the documented exception in the umbrella's
≤400-line guideline) — eight read/write/delete operations sharing
one error-mapping table belong in one file.

Part of SP1 PR-B.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task B6: Extract `host/commands.rs` (1 fn + 1 deny + spawn helper + spot test + behavioral tests)

The largest behavioral test set (nine `host_commands_exec_*` tests) lives here, plus the private `spawn_with_timeout` helper.

**Files:**
- Create: `src/plugin/host/commands.rs`
- Modify: `src/plugin/host/all.rs`
- Modify: `src/plugin/host/mod.rs`

- [ ] **Step 1: Create `src/plugin/host/commands.rs`**

Lift `host_commands_exec`, `deny_commands_exec`, and `spawn_with_timeout` from `all.rs`. Rewrite `host_commands_exec` to use `ensure_bound` instead of inlined `if ctx.env_mut().is_none()`. Move all nine `host_commands_exec_*` tests verbatim from `all.rs::tests`.

The body of `spawn_with_timeout` is preserved bit-for-bit. Place it as a `fn spawn_with_timeout(...)` (no `pub`) inside `commands.rs`.

`host_commands_exec` becomes:

```rust
pub(super) fn host_commands_exec(
    ctx: &mut HostContext,
    program: String,
    args: Vec<String>,
) -> Result<ExecOutput, ErrorCode> {
    // The metadata-contract guard runs first. CWD and environment
    // inheritance happen implicitly via std::process::Command::new
    // defaults (spec §5: "CWD is the shell's current directory;
    // environment is the shell's full environment") — `ctx` is read
    // here only for `allowed_commands`, not for ShellEnv state.
    ctx.ensure_bound()?;
    if program.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }

    // argv = [program, args...]; pattern matcher consumes the literal
    // strings (no PATH resolution, no basename normalization — see
    // spec §5).
    let mut argv = Vec::with_capacity(1 + args.len());
    argv.push(program.clone());
    argv.extend(args.iter().cloned());

    if !ctx.allowed_commands.iter().any(|p| p.matches(&argv)) {
        return Err(ErrorCode::PatternNotAllowed);
    }

    spawn_with_timeout(&program, &args, std::time::Duration::from_millis(1000))
}
```

The full file structure:

```rust
//! `yosh:plugin/commands` host import — execute external commands
//! against a per-plugin allowlist of CommandPattern. Granted via
//! CAP_COMMANDS_EXEC.

use super::super::generated::yosh::plugin::commands::ExecOutput;
use super::super::generated::yosh::plugin::types::ErrorCode;
use super::HostContext;

pub(super) fn host_commands_exec(
    ctx: &mut HostContext,
    program: String,
    args: Vec<String>,
) -> Result<ExecOutput, ErrorCode> {
    // ... (body shown above)
}

pub(super) fn deny_commands_exec(
    _ctx: &mut HostContext,
    _program: String,
    _args: Vec<String>,
) -> Result<ExecOutput, ErrorCode> {
    Err(ErrorCode::Denied)
}

fn spawn_with_timeout(
    program: &str,
    args: &[String],
    timeout: std::time::Duration,
) -> Result<ExecOutput, ErrorCode> {
    // ... (preserve the existing body verbatim — including the read_to_end
    // background threads, the deadline loop, and the SIGTERM/100ms grace/
    // SIGKILL escalation)
}

#[cfg(test)]
mod tests {
    //! Metadata-contract spot test plus the nine spec-§10 behavioral
    //! tests for commands:exec.

    use super::*;
    use super::super::test_helpers::{ctx_with_allowed, null_env_ctx, bound_env_ctx};
    use crate::env::ShellEnv;

    #[test]
    fn commands_exec_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_commands_exec(&mut ctx, "/bin/echo".into(), vec!["hi".into()]);
        assert!(matches!(result, Err(ErrorCode::Denied)));
    }

    // ── Move the nine existing host_commands_exec_* tests here verbatim:
    //   - host_commands_exec_invalid_argument_on_empty_program
    //   - host_commands_exec_pattern_not_allowed_when_no_match
    //   - host_commands_exec_runs_when_pattern_matches
    //   - host_commands_exec_captures_stderr_separately
    //   - host_commands_exec_propagates_nonzero_exit
    //   - host_commands_exec_returns_not_found_for_missing_binary
    //   - host_commands_exec_timeout_after_1000ms
    //   - host_commands_exec_kills_child_on_timeout
    // (The 9th, host_commands_exec_metadata_contract_denied_when_env_null,
    // is replaced by `commands_exec_denied_when_env_null` above.)
}
```

When migrating each test, change the existing `super::super::pattern::CommandPattern::parse(...)` reference inside `ctx_with_allowed` calls — that helper is already imported from `super::test_helpers::*`, so test bodies need no other path adjustments.

- [ ] **Step 2: Remove from `all.rs`**

Delete:
- `host_commands_exec`, `deny_commands_exec`, `spawn_with_timeout`.
- The `// ── yosh:plugin/commands host imports ──` section header.
- All ten existing `host_commands_exec_*` tests.

At this point `all.rs` should contain **no production code** — only an empty body (or a comment to that effect). Tests likewise should be empty.

- [ ] **Step 3: Update `mod.rs`**

Add `mod commands;`. Move `host_commands_exec` and `deny_commands_exec` out of `all::{...}` and into a new `pub(super) use commands::{deny_commands_exec, host_commands_exec};`.

After this step `all::{...}` re-exports should be empty. Don't delete the `mod all;` and the empty re-export blocks yet — Task B7 handles that to keep the diff focused.

- [ ] **Step 4: Build and run tests**

```bash
cargo test -p yosh --lib plugin
```
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/plugin/host
git commit -m "$(cat <<'EOF'
refactor(plugin): extract commands capability to host/commands.rs

Move host_commands_exec, deny_commands_exec, and the private
spawn_with_timeout helper to a new submodule. host_commands_exec
delegates the metadata guard to ensure_bound, which removes the
borrow-checker concern from a closure-based with_env design (the
inline call to ctx.allowed_commands no longer fights an outer
mutable borrow).

Test consolidation: rename
host_commands_exec_metadata_contract_denied_when_env_null to
commands_exec_denied_when_env_null (the spot test for this capability).
The nine §10 behavioral tests (invalid_argument, pattern_not_allowed,
runs_when_pattern_matches, captures_stderr_separately,
propagates_nonzero_exit, returns_not_found, timeout_after_1000ms,
kills_child_on_timeout) move verbatim.

After this commit all.rs holds no production code; Task B7 deletes it.

Part of SP1 PR-B.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task B7: Delete `all.rs`

After B2–B6 every host/deny function and its tests have moved out. `all.rs` is now empty. Delete it and remove the `mod all;` plus the empty `pub(super) use all::{};` block from `mod.rs`.

**Files:**
- Delete: `src/plugin/host/all.rs`
- Modify: `src/plugin/host/mod.rs`

- [ ] **Step 1: Confirm `all.rs` is empty**

```bash
wc -l src/plugin/host/all.rs
```
Expected: a single-digit count (just the doc comment header at most).

- [ ] **Step 2: Delete `all.rs`**

```bash
git rm src/plugin/host/all.rs
```

- [ ] **Step 3: Remove `mod all;` and empty re-export blocks from `mod.rs`**

In `src/plugin/host/mod.rs`, delete the line `mod all;` and any remaining `pub(super) use all::{...};` block (it should now be empty).

After this edit, the module declarations and re-exports in `mod.rs` should look like:

```rust
mod commands;
mod files;
mod filesystem;
mod io;
mod variables;

pub(super) use commands::{deny_commands_exec, host_commands_exec};
pub(super) use files::{
    deny_files_append_file, deny_files_create_dir, deny_files_metadata,
    deny_files_read_dir, deny_files_read_file, deny_files_remove_dir,
    deny_files_remove_file, deny_files_write_file,
    host_files_append_file, host_files_create_dir, host_files_metadata,
    host_files_read_dir, host_files_read_file, host_files_remove_dir,
    host_files_remove_file, host_files_write_file,
};
pub(super) use filesystem::{
    deny_filesystem_cwd, deny_filesystem_set_cwd, host_filesystem_cwd,
    host_filesystem_set_cwd,
};
pub(super) use io::{deny_io_write, host_io_write};
pub(super) use variables::{
    deny_variables_export_env, deny_variables_get, deny_variables_set,
    host_variables_export_env, host_variables_get, host_variables_set,
};
```

(Order alphabetically by submodule name; within each `use` block, alphabetize symbol names. This matches the rustfmt-default ordering applied across the project.)

- [ ] **Step 4: Build, test, and lint**

```bash
cargo build -p yosh
cargo test -p yosh --lib plugin
cargo fmt --check
```
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/plugin/host
git commit -m "$(cat <<'EOF'
refactor(plugin): delete temporary all.rs after capability split

After the variables/filesystem/io/files/commands submodules absorbed
every host_* / deny_* function, all.rs is empty. Remove the file and
the corresponding mod all; declaration and empty re-export block
from host/mod.rs.

The post-split layout is:
  src/plugin/host/
    mod.rs        — HostContext, helpers, WasiView, test_helpers
    variables.rs  — 3 host fns + 3 deny stubs + 1 spot test
    filesystem.rs — 2 host fns + 2 deny stubs + 1 spot test
    io.rs         — 1 host fn  + 1 deny stub + 1 spot test
    files.rs      — 8 host fns + 8 deny stubs + 1 spot test + 9 behavioral tests
    commands.rs   — 1 host fn  + 1 deny stub + 1 spot test + 8 behavioral tests
                    + private spawn_with_timeout helper

Closes SP1 PR-B.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task B8: Final verification, lint, and TODO.md cleanup

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Run the full lib test suite**

```bash
cargo test -p yosh --lib
```
Expected: PASS.

- [ ] **Step 2: Run integration tests with the WASM plugins**

```bash
cargo component build -p test_plugin --target wasm32-wasip2 --release
cargo component build -p trap_plugin --target wasm32-wasip2 --release
cargo component build -p slow_plugin --target wasm32-wasip2 --release
cargo test --features test-helpers --test plugin
```
Expected: PASS.

- [ ] **Step 3: Run the E2E suite**

```bash
./e2e/run_tests.sh
```
Expected: PASS.

- [ ] **Step 4: Run benches in `--no-run` mode to confirm bench API is intact**

```bash
cargo bench --no-run
```
Expected: PASS (no execution; just confirming benches still compile).

- [ ] **Step 5: Run clippy**

```bash
cargo clippy --all-targets -- -D warnings
```
Expected: only the pre-existing `doc_lazy_continuation` violation at `src/plugin/mod.rs:98-99` remains. Anything new is a regression to fix before committing.

- [ ] **Step 6: Run rustfmt check**

```bash
cargo fmt --check
```
Expected: PASS.

- [ ] **Step 7: Verify caller diff is zero**

```bash
git diff main -- src/plugin/linker.rs
```
Expected: empty diff. `linker.rs` should be byte-for-byte identical to `main`.

- [ ] **Step 8: Verify `wc -l` per file is within the SP1 design budget**

```bash
wc -l src/plugin/host/*.rs
```
Expected (approximate, ±5%):
- `mod.rs` ≤ 200
- `variables.rs` ≤ 150
- `filesystem.rs` ≤ 100
- `io.rs` ≤ 100
- `files.rs` ≤ 460
- `commands.rs` ≤ 320

If any file is significantly over budget, review the design assumptions before declaring DoD.

- [ ] **Step 9: Remove the resolved TODO.md entry**

Open `TODO.md`. Locate the entry beginning:

> `src/plugin/host.rs` is now ~970 lines after the `commands:exec` addition. Consider splitting into `src/plugin/host/{mod,variables,filesystem,io,files}.rs` so each capability owns a focused file (`HostContext` + `WasiView` impl + `null_env_ctx` helper stay in `mod.rs`). Code-review follow-up from 2026-04-29 plugin files-rw branch.

Delete the entire bullet (per project convention: "delete completed items rather than marking them with `[x]`" — see CLAUDE.md).

- [ ] **Step 10: Commit the TODO.md cleanup**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
chore(todo): remove host.rs split entry resolved by SP1

The 2026-04-29 follow-up "consider splitting src/plugin/host.rs into
host/{mod,variables,filesystem,io,files}.rs" is resolved by the SP1
capability-split landed in this branch's earlier commits. Adds a
sixth submodule (commands.rs) and a structural ensure_bound /
bound_env helper pair beyond what the original suggestion described,
both motivated by the 2026-05-06 SP1 design.

Closes SP1.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-Review Checklist (run before declaring SP1 done)

- [ ] Each spec section is implemented by at least one task above (Task 0 cross-references; Tasks A1–A2 deliver the helpers + scaffolding; Tasks B1–B7 deliver the capability split + helper rewrite + test consolidation; Task B8 is verification + DoD).
- [ ] No placeholders ("TBD", "implement later") in any task.
- [ ] Helper method names are consistent throughout: `ensure_bound` (no env), `bound_env` (returns env). No drift to `with_bound_env`, `bound_env_or_denied`, etc.
- [ ] Submodule paths are consistent: `src/plugin/host/{mod,variables,filesystem,io,files,commands}.rs`. No stray `host/all.rs` reference after Task B7.
- [ ] PR-A and PR-B are mergeable independently if the team chooses (PR-A leaves `all.rs` as a single-file-but-moved layout; PR-B is the consequential split).
- [ ] Caller `linker.rs` is unchanged at the byte level after PR-A and after PR-B (verified by Step 7 of Task B8).
- [ ] DoD per umbrella §1.7 is verifiable (Steps 1–8 of Task B8 cover unit, integration, E2E, bench compile, clippy, fmt, caller-diff, file size).
