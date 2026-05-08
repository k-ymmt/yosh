# Plugin Host-Import Borrow Rollout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Apply the `WasmStr` borrow pattern proven in the §4.1 PoC (commit `c22e63a`) to the remaining 11 `String`-arg host imports across `variables`, `filesystem`, and `files` modules, eliminating one host-side canonical-ABI `String` allocation per crossing for each.

**Architecture:** Add a single new `HostContext::bound_env_with` closure-style mutable env helper. All 11 closures in `src/plugin/linker.rs` are rewritten to use `WasmStr` and `store.data()` (no `mut store`). Host functions get `(&HostContext, &str, ...)` signatures; the 9 functions that use `ensure_bound` only (no `ShellEnv` access) need only signature changes, and the 2 functions that mutate `ShellEnv` (`variables::set`, `variables::export-env`) move to `bound_env_with`. WIT and on-disk plugin binaries are unaffected. The `perf_plugin` test fixture gains three new no-op commands (`noop_var_set`, `noop_files_read`, `noop_files_remove`) so dhat `--exec-loop` can measure each pattern variant independently.

**Tech Stack:** Rust 1.94.1 / wasmtime 27 component model / `wasmtime::component::WasmStr` / Criterion / dhat / cargo-component for `perf_plugin.wasm` rebuild.

**Spec:** `docs/superpowers/specs/2026-05-08-plugin-host-import-borrow-rollout-design.md`

**Predecessor PoC:** `docs/superpowers/specs/2026-05-08-plugin-host-import-borrow-design.md` (commit `c22e63a`)

**Important reality check vs spec §1.1:** The spec table assigned `bound_env_ref` to `files::read-*` and `bound_env_with` to `filesystem::set-cwd` and `files::*` mutation paths. The current implementations of `host_filesystem_set_cwd` and all `host_files_*` functions only call `ctx.ensure_bound()?` — they do not access `ShellEnv`. They only need the parameter-type swap (`&mut HostContext` → `&HostContext`, `String` → `&str`). Only `host_variables_set` and `host_variables_export_env` actually mutate `ShellEnv` and need `bound_env_with`. The plan reflects this.

---

## File Structure

| Path | Action | Responsibility |
|---|---|---|
| `src/plugin/host/mod.rs` | Modify (add helper) | New `bound_env_with` closure helper |
| `src/plugin/host/variables.rs` | Modify | `host_variables_set`, `deny_variables_set`, `host_variables_export_env`, `deny_variables_export_env` signature + body refactor |
| `src/plugin/host/filesystem.rs` | Modify | `host_filesystem_set_cwd`, `deny_filesystem_set_cwd` signature swap |
| `src/plugin/host/files.rs` | Modify | All 8 `host_files_*` and 8 `deny_files_*` signature swaps + 7+ unit-test call-site updates |
| `src/plugin/linker.rs` | Modify | 22 `func_wrap` closures (11 host imports × granted+deny) |
| `tests/plugins/perf_plugin/src/lib.rs` | Modify | Add 3 commands, expand `required_capabilities` |
| `target/wasm32-wasip2/release/perf_plugin.wasm` | Rebuild | Re-emit fixture with new commands |
| `/tmp/yosh-perf-home/.config/yosh/plugins.lock` | Re-stage | Update capabilities to include `variables:write`, `files:read`, `files:write` |
| `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` | Append | Add Appendix B (rollout result, success or partial-failure or failure template) |
| `TODO.md` | Modify | Update line 48 (rollout entry) to reflect outcome |

No new files. No new Criterion benches (the §4.1 lesson is that `noop_var`-style metrics dilute per-crossing improvements; dhat alloc-diff is the gating signal).

---

## Task 1: Add `bound_env_with` helper

**Files:**
- Modify: `src/plugin/host/mod.rs:124` (insert after `bound_env_ref`)

- [ ] **Step 1: Edit `src/plugin/host/mod.rs`, after the existing `bound_env_ref` body**

After the closing `}` of `bound_env_ref` (currently around line 142, just before the final `}` of `impl HostContext`), insert:

```rust

    /// Closure-style mutable env access. Used by host functions that
    /// must mutate `ShellEnv` while a wasmtime store borrow (e.g. from
    /// a `WasmStr::to_str` `Cow`) is held immutably. The mutation goes
    /// through the raw `*mut ShellEnv` so the wasmtime store's borrow
    /// state is unaffected.
    ///
    /// SAFETY: same invariants as `env_mut` — pointer is non-null only
    /// while `EnvGuard` keeps the bound `&mut ShellEnv` alive, and
    /// plugin dispatch is single-threaded.
    pub(super) fn bound_env_with<R, F>(&self, f: F) -> Result<R, ErrorCode>
    where
        F: FnOnce(&mut ShellEnv) -> R,
    {
        if self.env.is_null() {
            Err(ErrorCode::Denied)
        } else {
            // SAFETY: `EnvGuard::bind` set this pointer from a live
            // `&mut ShellEnv`; it is reset to null on guard drop.
            // Plugin dispatch is single-threaded.
            Ok(f(unsafe { &mut *self.env }))
        }
    }
```

- [ ] **Step 2: Compile-check**

```bash
cargo check --features test-helpers 2>&1 | tail -10
```

Expected: clean compile, only the pre-existing `unused imports: JobSpec and parse_job_spec` warning. The helper is added but not yet used; no errors expected.

- [ ] **Step 3: Commit**

```bash
git add src/plugin/host/mod.rs
git commit -m "$(cat <<'EOF'
feat(plugin): add bound_env_with closure helper to HostContext

Mirror of bound_env_ref but for mutation paths: takes a closure that
receives &mut ShellEnv. Mutation goes through the raw *mut ShellEnv
so the wasmtime store's borrow state is unaffected, allowing a host
import closure to hold a WasmStr-derived &str (immutable store borrow)
while still mutating shell state. Used by the upcoming
variables::set / variables::export-env rollout (the only mutation
paths in the rollout); files::* and filesystem::set-cwd already use
ensure_bound only and need just the signature swap.

Spec: docs/superpowers/specs/2026-05-08-plugin-host-import-borrow-rollout-design.md
Plan: docs/superpowers/plans/2026-05-08-plugin-host-import-borrow-rollout.md
EOF
)"
```

---

## Task 2: Extend `perf_plugin` with rollout-measurement commands

**Files:**
- Modify: `tests/plugins/perf_plugin/src/lib.rs` (add commands and expand capabilities)
- Rebuild: `target/wasm32-wasip2/release/perf_plugin.wasm`

- [ ] **Step 1: Edit `tests/plugins/perf_plugin/src/lib.rs`**

Replace the entire file contents with:

```rust
//! perf_plugin — minimal-overhead fixture for plugin performance benches.
//!
//! Used by `benches/plugin_bench.rs` and `benches/startup_bench.rs`. Has no
//! stdout side-effects (unlike `test_plugin`'s `print()` calls), so Criterion
//! measurements are not polluted.

use yosh_plugin_sdk::{
    Capability, HookName, Plugin, export, get_var, read_file, remove_file, set_var,
};

#[derive(Default)]
struct PerfPlugin;

impl Plugin for PerfPlugin {
    fn commands(&self) -> &[&'static str] {
        &[
            "noop_cmd",
            "noop_var",
            "burst_var",
            "noop_var_set",
            "noop_files_read",
            "noop_files_remove",
        ]
    }

    fn required_capabilities(&self) -> &[Capability] {
        &[
            Capability::VariablesRead,
            Capability::VariablesWrite,
            Capability::FilesRead,
            Capability::FilesWrite,
            Capability::HookPrePrompt,
            Capability::HookPreExec,
            Capability::HookPostExec,
        ]
    }

    fn implemented_hooks(&self) -> &[HookName] {
        &[HookName::PrePrompt, HookName::PreExec, HookName::PostExec]
    }

    fn exec(&mut self, command: &str, _args: &[String]) -> i32 {
        match command {
            "noop_cmd" => 0,
            "noop_var" => {
                let _ = get_var("PERF_VAR");
                0
            }
            "burst_var" => {
                for _ in 0..10 {
                    let _ = get_var("PERF_VAR");
                }
                0
            }
            "noop_var_set" => {
                let _ = set_var("PERF_VAR", "v");
                0
            }
            "noop_files_read" => {
                let _ = read_file("/dev/null");
                0
            }
            "noop_files_remove" => {
                let _ = remove_file("/tmp/yosh-perf-rollout-nonexistent");
                0
            }
            _ => 127,
        }
    }

    fn hook_pre_prompt(&mut self) {
        // Empty body — measures dispatch overhead, not user work.
    }

    fn hook_pre_exec(&mut self, _command: &str) {
        // Empty body — measures dispatch overhead.
    }

    fn hook_post_exec(&mut self, _command: &str, _exit_code: i32) {
        // Empty body — measures dispatch overhead.
    }
}

export!(PerfPlugin);
```

- [ ] **Step 2: Rebuild the wasm fixture**

```bash
cargo component build -p perf_plugin --target wasm32-wasip2 --release 2>&1 | tail -5
```

Expected: `Finished release [optimized] target(s) in <N>s`. The `target/wasm32-wasip2/release/perf_plugin.wasm` file is updated.

- [ ] **Step 3: Verify the wasm rebuilt**

```bash
ls -la target/wasm32-wasip2/release/perf_plugin.wasm
```

Expected: file exists, mtime within the last minute.

- [ ] **Step 4: Run perf_plugin's bench-helpers tests**

The new commands need to dispatch correctly through the linker before we can measure. The plugin-feature smoke is:

```bash
cargo test --features test-helpers --test plugin -- t01 2>&1 | tail -10
```

Expected: `test t01_capability_allowlist_applied_to_linker ... ok`. This confirms the rebuilt wasm still loads. We are NOT running the new commands through tests — they will be exercised via dhat in Tasks 3 and 8.

- [ ] **Step 5: Commit**

```bash
git add tests/plugins/perf_plugin/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(perf_plugin): add rollout-measurement commands

Three new commands isolate one host-import crossing each, for use
with yosh-dhat --exec-loop:
  - noop_var_set:      variables.set (dual-WasmStr mutation path)
  - noop_files_read:   files.read-file (read-only path)
  - noop_files_remove: files.remove-file (single-WasmStr mutation path)

Each calls the SDK helper once and returns 0; the read/remove
targets ("/dev/null", "/tmp/yosh-perf-rollout-nonexistent") are
chosen so the call returns quickly (or with a NotFound error) without
filesystem side effects. The alloc-count delta from dhat is the
metric, not the call's success.

required_capabilities expanded to include VariablesWrite, FilesRead,
FilesWrite. WIT unchanged. existing benches (noop_cmd / noop_var /
burst_var) still work unchanged.

Plan: docs/superpowers/plans/2026-05-08-plugin-host-import-borrow-rollout.md
EOF
)"
```

---

## Task 3: Capture pre-rollout dhat baselines

**Files:** none modified (target/perf/ scratch files are gitignored)

- [ ] **Step 1: Build profiling yosh-dhat (if outdated)**

```bash
cargo build --profile profiling --features dhat-heap --bin yosh-dhat 2>&1 | tail -3
```

Expected: `Finished profiling profile [optimized + debuginfo]`.

- [ ] **Step 2: Update `/tmp/yosh-perf-home/.config/yosh/plugins.lock`**

Re-stage the plugins.lock with all the new capabilities the rollout fixture needs:

```bash
mkdir -p /tmp/yosh-perf-home/.config/yosh
cat > /tmp/yosh-perf-home/.config/yosh/plugins.lock <<EOF
[[plugin]]
name = "perf"
path = "$(pwd)/target/wasm32-wasip2/release/perf_plugin.wasm"
enabled = true
capabilities = [
    "variables:read",
    "variables:write",
    "files:read",
    "files:write",
    "hooks:pre_prompt",
    "hooks:pre_exec",
    "hooks:post_exec",
]
EOF
cat /tmp/yosh-perf-home/.config/yosh/plugins.lock
```

Expected: file rewritten with the new capability set printed back.

- [ ] **Step 3: Run dhat for `noop_var_set` (baseline)**

```bash
mkdir -p target/perf
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat --exec-loop 1000 noop_var_set 2>&1 | tail -5
mv dhat-heap.json target/perf/dhat-rollout-noop_var_set-baseline.json
```

Expected output ends with `dhat: Total: <N> bytes in <M> blocks` and `dhat: The data has been saved to dhat-heap.json`. Record `<N>` and `<M>` (the "Total" line from the dhat summary) — these are the baseline alloc counts for `variables::set`.

- [ ] **Step 4: Run dhat for `noop_files_read` (baseline)**

```bash
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat --exec-loop 1000 noop_files_read 2>&1 | tail -5
mv dhat-heap.json target/perf/dhat-rollout-noop_files_read-baseline.json
```

Record the Total bytes/blocks numbers.

- [ ] **Step 5: Run dhat for `noop_files_remove` (baseline)**

```bash
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat --exec-loop 1000 noop_files_remove 2>&1 | tail -5
mv dhat-heap.json target/perf/dhat-rollout-noop_files_remove-baseline.json
```

Record the Total bytes/blocks numbers.

- [ ] **Step 6: Save baseline summary to scratch**

```bash
cat > target/perf/rollout-baseline.txt <<EOF
=== rollout dhat baselines (commit 5c8ced5, before rollout) ===
noop_var_set:       <bytes_var_set>     bytes / <blocks_var_set>     blocks
noop_files_read:    <bytes_files_read>  bytes / <blocks_files_read>  blocks
noop_files_remove:  <bytes_files_rm>    bytes / <blocks_files_rm>    blocks

Expected post-rollout deltas:
  noop_var_set:       -2,000 blocks (dual-WasmStr → 2 fewer String allocs/call × 1000 iters)
  noop_files_read:    -1,000 blocks (single WasmStr → 1 fewer String alloc/call × 1000 iters)
  noop_files_remove:  -1,000 blocks (single WasmStr → 1 fewer String alloc/call × 1000 iters)
EOF
cat target/perf/rollout-baseline.txt
```

Replace `<bytes_*>` and `<blocks_*>` with the actual numbers from Steps 3-5. (Use a text editor or `sed` — these are scratch numbers for the controller's later comparison.)

No commit. `target/perf/` is gitignored.

---

## Task 4: Rollout `variables::set` and `variables::export-env`

**Files:**
- Modify: `src/plugin/host/variables.rs:24-63` (4 fns)
- Modify: `src/plugin/linker.rs:86-104` (4 closures: granted+deny × 2 host imports)

- [ ] **Step 1: Refactor `host_variables_set` body in `src/plugin/host/variables.rs:24-31`**

Current:

```rust
pub fn host_variables_set(
    ctx: &mut HostContext,
    name: String,
    value: String,
) -> Result<(), ErrorCode> {
    let env = ctx.bound_env()?;
    env.vars.set(&name, &value).map_err(|_| ErrorCode::IoFailed)
}
```

Replace with:

```rust
pub fn host_variables_set(
    ctx: &HostContext,
    name: &str,
    value: &str,
) -> Result<(), ErrorCode> {
    ctx.bound_env_with(|env| env.vars.set(name, value).map_err(|_| ErrorCode::IoFailed))?
}
```

- [ ] **Step 2: Refactor `deny_variables_set` in `src/plugin/host/variables.rs:33-39`**

Current:

```rust
pub fn deny_variables_set(
    _ctx: &mut HostContext,
    _name: String,
    _value: String,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}
```

Replace with:

```rust
pub fn deny_variables_set(
    _ctx: &HostContext,
    _name: &str,
    _value: &str,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}
```

- [ ] **Step 3: Refactor `host_variables_export_env` body in `src/plugin/host/variables.rs:44-55`**

Current:

```rust
pub fn host_variables_export_env(
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
```

Replace with:

```rust
pub fn host_variables_export_env(
    ctx: &HostContext,
    name: &str,
    value: &str,
) -> Result<(), ErrorCode> {
    ctx.bound_env_with(|env| {
        env.vars
            .set(name, value)
            .map_err(|_| ErrorCode::IoFailed)?;
        env.vars.export(name);
        Ok(())
    })?
}
```

- [ ] **Step 4: Refactor `deny_variables_export_env` in `src/plugin/host/variables.rs:57-63`**

Current:

```rust
pub fn deny_variables_export_env(
    _ctx: &mut HostContext,
    _name: String,
    _value: String,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}
```

Replace with:

```rust
pub fn deny_variables_export_env(
    _ctx: &HostContext,
    _name: &str,
    _value: &str,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}
```

- [ ] **Step 5: Refactor the four `func_wrap` closures in `src/plugin/linker.rs:86-104`**

Current (lines 86-104, the `if has(allowed, CAP_VARIABLES_WRITE)` block):

```rust
    if has(allowed, CAP_VARIABLES_WRITE) {
        vars.func_wrap("set", |mut store, (name, value): (String, String)| {
            Ok((host_variables_set(store.data_mut(), name, value),))
        })?;
        vars.func_wrap(
            "export-env",
            |mut store, (name, value): (String, String)| {
                Ok((host_variables_export_env(store.data_mut(), name, value),))
            },
        )?;
    } else {
        vars.func_wrap("set", |mut store, (name, value): (String, String)| {
            Ok((deny_variables_set(store.data_mut(), name, value),))
        })?;
        vars.func_wrap(
            "export-env",
            |mut store, (name, value): (String, String)| {
                Ok((deny_variables_export_env(store.data_mut(), name, value),))
            },
        )?;
    }
```

Replace with:

```rust
    if has(allowed, CAP_VARIABLES_WRITE) {
        vars.func_wrap(
            "set",
            |store, (name, value): (wasmtime::component::WasmStr, wasmtime::component::WasmStr)| {
                let name_str = name.to_str(&store)?;
                let value_str = value.to_str(&store)?;
                Ok((host_variables_set(store.data(), &name_str, &value_str),))
            },
        )?;
        vars.func_wrap(
            "export-env",
            |store, (name, value): (wasmtime::component::WasmStr, wasmtime::component::WasmStr)| {
                let name_str = name.to_str(&store)?;
                let value_str = value.to_str(&store)?;
                Ok((host_variables_export_env(store.data(), &name_str, &value_str),))
            },
        )?;
    } else {
        vars.func_wrap(
            "set",
            |store, (name, value): (wasmtime::component::WasmStr, wasmtime::component::WasmStr)| {
                let name_str = name.to_str(&store)?;
                let value_str = value.to_str(&store)?;
                Ok((deny_variables_set(store.data(), &name_str, &value_str),))
            },
        )?;
        vars.func_wrap(
            "export-env",
            |store, (name, value): (wasmtime::component::WasmStr, wasmtime::component::WasmStr)| {
                let name_str = name.to_str(&store)?;
                let value_str = value.to_str(&store)?;
                Ok((deny_variables_export_env(store.data(), &name_str, &value_str),))
            },
        )?;
    }
```

- [ ] **Step 6: Compile-check**

```bash
cargo check --features test-helpers 2>&1 | tail -10
cargo build --release 2>&1 | tail -3
```

Expected: clean compile (only pre-existing warnings). If a compile error mentions a caller of `host_variables_set` / `deny_variables_set` / `host_variables_export_env` / `deny_variables_export_env` outside `linker.rs`, search and fix:

```bash
grep -rn "host_variables_set\|deny_variables_set\|host_variables_export_env\|deny_variables_export_env" src/ tests/
```

(All callers should be in `linker.rs`. There is no in-module unit test for set/export-env in `variables.rs`.)

- [ ] **Step 7: Run regression tests**

```bash
cargo test --features test-helpers --test plugin 2>&1 | tail -20
cargo test -p yosh plugin::host::variables 2>&1 | tail -10
```

Expected: all pass. The existing `t13_hook_dispatch_suppression` exercises `set_var` denied-path; `t01` and others exercise variables broadly.

- [ ] **Step 8: Commit**

```bash
git add src/plugin/host/variables.rs src/plugin/linker.rs
git commit -m "$(cat <<'EOF'
feat(plugin): borrow strings in variables::set / export-env (rollout)

Apply the §4.1 PoC pattern (WasmStr + bound_env_*) to the two
mutation paths in variables. Both host fns retyped to
(&HostContext, &str, &str); bodies refactored to use bound_env_with
so the closure-style mutable env access coexists with the WasmStr's
immutable store borrow. Linker closures rewritten to take dual
WasmStr args, drop `mut` on store, and pass store.data() (immutable).

Plan: docs/superpowers/plans/2026-05-08-plugin-host-import-borrow-rollout.md (Task 4)
EOF
)"
```

---

## Task 5: Rollout `filesystem::set-cwd`

**Files:**
- Modify: `src/plugin/host/filesystem.rs:20-27` (2 fns)
- Modify: `src/plugin/linker.rs:113-122` (2 closures)

- [ ] **Step 1: Refactor `host_filesystem_set_cwd` in `src/plugin/host/filesystem.rs:20-23`**

Current:

```rust
pub fn host_filesystem_set_cwd(ctx: &mut HostContext, path: String) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    std::env::set_current_dir(&path).map_err(|_| ErrorCode::IoFailed)
}
```

Replace with:

```rust
pub fn host_filesystem_set_cwd(ctx: &HostContext, path: &str) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    std::env::set_current_dir(path).map_err(|_| ErrorCode::IoFailed)
}
```

(Two changes: `&mut HostContext` → `&HostContext`, `path: String` + `&path` → `path: &str` + `path`.)

- [ ] **Step 2: Refactor `deny_filesystem_set_cwd` in `src/plugin/host/filesystem.rs:25-27`**

Current:

```rust
pub fn deny_filesystem_set_cwd(_ctx: &mut HostContext, _path: String) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}
```

Replace with:

```rust
pub fn deny_filesystem_set_cwd(_ctx: &HostContext, _path: &str) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}
```

- [ ] **Step 3: Refactor the two `func_wrap` closures in `src/plugin/linker.rs:113-122`**

Current:

```rust
    if has(allowed, CAP_FILESYSTEM) {
        fs.func_wrap("cwd", |mut store, (): ()| {
            Ok((host_filesystem_cwd(store.data_mut()),))
        })?;
        fs.func_wrap("set-cwd", |mut store, (path,): (String,)| {
            Ok((host_filesystem_set_cwd(store.data_mut(), path),))
        })?;
    } else {
        fs.func_wrap("cwd", |mut store, (): ()| {
            Ok((deny_filesystem_cwd(store.data_mut()),))
        })?;
        fs.func_wrap("set-cwd", |mut store, (path,): (String,)| {
            Ok((deny_filesystem_set_cwd(store.data_mut(), path),))
        })?;
    }
```

Replace ONLY the two `set-cwd` closures (the `cwd` closures stay unchanged because `cwd` has no string parameter):

```rust
    if has(allowed, CAP_FILESYSTEM) {
        fs.func_wrap("cwd", |mut store, (): ()| {
            Ok((host_filesystem_cwd(store.data_mut()),))
        })?;
        fs.func_wrap("set-cwd", |store, (path,): (wasmtime::component::WasmStr,)| {
            let path_str = path.to_str(&store)?;
            Ok((host_filesystem_set_cwd(store.data(), &path_str),))
        })?;
    } else {
        fs.func_wrap("cwd", |mut store, (): ()| {
            Ok((deny_filesystem_cwd(store.data_mut()),))
        })?;
        fs.func_wrap("set-cwd", |store, (path,): (wasmtime::component::WasmStr,)| {
            let path_str = path.to_str(&store)?;
            Ok((deny_filesystem_set_cwd(store.data(), &path_str),))
        })?;
    }
```

(Note: `host_filesystem_cwd` / `deny_filesystem_cwd` still take `&mut HostContext` — they have no string args and are out of scope for this rollout.)

- [ ] **Step 4: Compile-check**

```bash
cargo check --features test-helpers 2>&1 | tail -10
cargo build --release 2>&1 | tail -3
```

Expected: clean compile.

- [ ] **Step 5: Run regression tests**

```bash
cargo test --features test-helpers --test plugin 2>&1 | tail -20
cargo test -p yosh plugin::host::filesystem 2>&1 | tail -10
```

Expected: all pass. The `filesystem_cwd_denied_when_env_null` test in `host/filesystem.rs` exercises only `host_filesystem_cwd` (not set_cwd), so its call site is untouched.

- [ ] **Step 6: Commit**

```bash
git add src/plugin/host/filesystem.rs src/plugin/linker.rs
git commit -m "$(cat <<'EOF'
feat(plugin): borrow path in filesystem::set-cwd (rollout)

Apply WasmStr borrow pattern to filesystem::set-cwd. Host fn retyped
to (&HostContext, &str); body unchanged otherwise (this fn never
accessed ShellEnv, only ensure_bound). Linker closure rewritten to
WasmStr + store.data().

filesystem::cwd takes no string arg and stays unchanged.

Plan: docs/superpowers/plans/2026-05-08-plugin-host-import-borrow-rollout.md (Task 5)
EOF
)"
```

---

## Task 6: Rollout `files::*` read functions (`read-file`, `read-dir`, `metadata`)

**Files:**
- Modify: `src/plugin/host/files.rs:19-81` (3 host fns)
- Modify: `src/plugin/host/files.rs:163-176` (3 deny stubs)
- Modify: `src/plugin/host/files.rs:220+` (unit tests touching these 3 fns)
- Modify: `src/plugin/linker.rs:142-161` (6 closures)

- [ ] **Step 1: Refactor `host_files_read_file` in `src/plugin/host/files.rs:19-29`**

Current:

```rust
pub fn host_files_read_file(ctx: &mut HostContext, path: String) -> Result<Vec<u8>, ErrorCode> {
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
```

Replace with:

```rust
pub fn host_files_read_file(ctx: &HostContext, path: &str) -> Result<Vec<u8>, ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    match std::fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ErrorCode::NotFound),
        Err(_) => Err(ErrorCode::IoFailed),
    }
}
```

- [ ] **Step 2: Refactor `host_files_read_dir` in `src/plugin/host/files.rs:31-56`**

Current:

```rust
pub fn host_files_read_dir(
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
    // ... rest unchanged ...
}
```

Replace the signature and the `read_dir` call:

```rust
pub fn host_files_read_dir(
    ctx: &HostContext,
    path: &str,
) -> Result<Vec<DirEntry>, ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let iter = match std::fs::read_dir(path) {
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
```

- [ ] **Step 3: Refactor `host_files_metadata` in `src/plugin/host/files.rs:58-81`**

Current:

```rust
pub fn host_files_metadata(ctx: &mut HostContext, path: String) -> Result<FileStat, ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let md = match std::fs::metadata(&path) {
        // ... rest ...
    };
    // ... rest ...
}
```

Replace signature and `metadata` call:

```rust
pub fn host_files_metadata(ctx: &HostContext, path: &str) -> Result<FileStat, ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let md = match std::fs::metadata(path) {
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
```

- [ ] **Step 4: Refactor the 3 deny stubs in `src/plugin/host/files.rs:163-176`**

Current:

```rust
pub fn deny_files_read_file(_ctx: &mut HostContext, _path: String) -> Result<Vec<u8>, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_read_dir(
    _ctx: &mut HostContext,
    _path: String,
) -> Result<Vec<DirEntry>, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_metadata(_ctx: &mut HostContext, _path: String) -> Result<FileStat, ErrorCode> {
    Err(ErrorCode::Denied)
}
```

Replace with:

```rust
pub fn deny_files_read_file(_ctx: &HostContext, _path: &str) -> Result<Vec<u8>, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_read_dir(
    _ctx: &HostContext,
    _path: &str,
) -> Result<Vec<DirEntry>, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_metadata(_ctx: &HostContext, _path: &str) -> Result<FileStat, ErrorCode> {
    Err(ErrorCode::Denied)
}
```

- [ ] **Step 5: Update unit-test call sites in `src/plugin/host/files.rs`**

Run:

```bash
grep -n "host_files_read_file\|host_files_read_dir\|host_files_metadata" /Users/kazukiyamamoto/Projects/rust/kish/src/plugin/host/files.rs
```

For each test that calls one of these three host fns, change:
- `&mut ctx` → `&ctx` on the ctx argument
- `"<literal>".into()` → `"<literal>"`
- `<pathbuf>.to_string_lossy().into_owned()` → `&<pathbuf>.to_string_lossy()` (Cow auto-derefs to &str)
- `String::new()` → `""`

Example (test at line 228):

```rust
// Before:
let result = host_files_read_file(&mut ctx, "/tmp/anything".into());

// After:
let result = host_files_read_file(&ctx, "/tmp/anything");
```

Example (test at line 243):

```rust
// Before:
let result = host_files_read_file(&mut ctx, path.to_string_lossy().into_owned());

// After:
let result = host_files_read_file(&ctx, &path.to_string_lossy());
```

Example (test at line 256):

```rust
// Before:
host_files_read_dir(&mut ctx, dir.path().to_string_lossy().into_owned()).unwrap();

// After:
host_files_read_dir(&ctx, &dir.path().to_string_lossy()).unwrap();
```

Apply the same transformation to every call site that touches `host_files_read_file`, `host_files_read_dir`, or `host_files_metadata`. (Other host fn tests stay unchanged for now — they're done in Task 7.)

If a test still calls `let mut ctx = ...` but no longer uses any `&mut ctx`, drop the `mut`:

```rust
// Before:
let mut ctx = bound_env_ctx(&mut env);
let result = host_files_read_file(&mut ctx, "/tmp/anything".into());

// After:
let ctx = bound_env_ctx(&mut env);
let result = host_files_read_file(&ctx, "/tmp/anything");
```

But if the same `ctx` is used by a yet-unrefactored test fn (e.g. `host_files_write_file` in the same scope), keep `mut` until Task 7 completes.

- [ ] **Step 6: Refactor the 6 `func_wrap` closures in `src/plugin/linker.rs:142-161`**

The current `files` instance block (lines 141-225) has six closures for the read functions: 3 in the granted block and 3 in the deny block. Replace ONLY the read-* closures (write/append/create/remove are in Task 7).

Current granted-side block (around lines 142-152):

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
```

Replace with:

```rust
    if has(allowed, CAP_FILES_READ) {
        files.func_wrap("read-file", |store, (path,): (wasmtime::component::WasmStr,)| {
            let path_str = path.to_str(&store)?;
            Ok((host_files_read_file(store.data(), &path_str),))
        })?;
        files.func_wrap("read-dir", |store, (path,): (wasmtime::component::WasmStr,)| {
            let path_str = path.to_str(&store)?;
            Ok((host_files_read_dir(store.data(), &path_str),))
        })?;
        files.func_wrap("metadata", |store, (path,): (wasmtime::component::WasmStr,)| {
            let path_str = path.to_str(&store)?;
            Ok((host_files_metadata(store.data(), &path_str),))
        })?;
    } else {
        files.func_wrap("read-file", |store, (path,): (wasmtime::component::WasmStr,)| {
            let path_str = path.to_str(&store)?;
            Ok((deny_files_read_file(store.data(), &path_str),))
        })?;
        files.func_wrap("read-dir", |store, (path,): (wasmtime::component::WasmStr,)| {
            let path_str = path.to_str(&store)?;
            Ok((deny_files_read_dir(store.data(), &path_str),))
        })?;
        files.func_wrap("metadata", |store, (path,): (wasmtime::component::WasmStr,)| {
            let path_str = path.to_str(&store)?;
            Ok((deny_files_metadata(store.data(), &path_str),))
        })?;
    }
```

- [ ] **Step 7: Compile-check**

```bash
cargo check --features test-helpers 2>&1 | tail -15
cargo build --release 2>&1 | tail -3
```

Expected: clean compile. If a test in `host/files.rs::tests` fails to compile, finish Step 5 (any missed call site).

- [ ] **Step 8: Run regression tests**

```bash
cargo test --features test-helpers --test plugin 2>&1 | tail -20
cargo test -p yosh plugin::host::files 2>&1 | tail -20
```

Expected: all pass. The `host_files_read_file_*` / `host_files_read_dir_*` / `host_files_metadata_*` unit tests are now exercising the borrow-shaped fns directly.

- [ ] **Step 9: Commit**

```bash
git add src/plugin/host/files.rs src/plugin/linker.rs
git commit -m "$(cat <<'EOF'
feat(plugin): borrow paths in files::read-file/read-dir/metadata (rollout)

Apply WasmStr borrow pattern to the three read-only files host imports.
Host fns retyped to (&HostContext, &str). Bodies unchanged otherwise
(these fns never accessed ShellEnv, only ensure_bound). Linker
closures rewritten to WasmStr + store.data(). Unit-test call sites
mechanically updated (drop mut, drop .into() / .into_owned()).

Plan: docs/superpowers/plans/2026-05-08-plugin-host-import-borrow-rollout.md (Task 6)
EOF
)"
```

---

## Task 7: Rollout `files::*` mutation functions (`write-file`, `append-file`, `create-dir`, `remove-file`, `remove-dir`)

**Files:**
- Modify: `src/plugin/host/files.rs:83-161` (5 host fns)
- Modify: `src/plugin/host/files.rs:178-212` (5 deny stubs)
- Modify: `src/plugin/host/files.rs:220+` (remaining unit tests)
- Modify: `src/plugin/linker.rs:166-219` (10 closures)

- [ ] **Step 1: Refactor `host_files_write_file` in `src/plugin/host/files.rs:83-93`**

Current:

```rust
pub fn host_files_write_file(
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
```

Replace with:

```rust
pub fn host_files_write_file(
    ctx: &HostContext,
    path: &str,
    data: Vec<u8>,
) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    std::fs::write(path, &data).map_err(|_| ErrorCode::IoFailed)
}
```

(`data` stays `Vec<u8>` — Vec<u8> borrow is out of scope for this rollout.)

- [ ] **Step 2: Refactor `host_files_append_file` in `src/plugin/host/files.rs:95-111`**

Current:

```rust
pub fn host_files_append_file(
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
```

Replace with:

```rust
pub fn host_files_append_file(
    ctx: &HostContext,
    path: &str,
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
        .open(path)
        .map_err(|_| ErrorCode::IoFailed)?;
    f.write_all(&data).map_err(|_| ErrorCode::IoFailed)
}
```

- [ ] **Step 3: Refactor `host_files_create_dir` in `src/plugin/host/files.rs:113-128`**

Current:

```rust
pub fn host_files_create_dir(
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
```

Replace with:

```rust
pub fn host_files_create_dir(
    ctx: &HostContext,
    path: &str,
    recursive: bool,
) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let result = if recursive {
        std::fs::create_dir_all(path)
    } else {
        std::fs::create_dir(path)
    };
    result.map_err(|_| ErrorCode::IoFailed)
}
```

- [ ] **Step 4: Refactor `host_files_remove_file` in `src/plugin/host/files.rs:130-140`**

Current:

```rust
pub fn host_files_remove_file(ctx: &mut HostContext, path: String) -> Result<(), ErrorCode> {
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
```

Replace with:

```rust
pub fn host_files_remove_file(ctx: &HostContext, path: &str) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ErrorCode::NotFound),
        Err(_) => Err(ErrorCode::IoFailed),
    }
}
```

- [ ] **Step 5: Refactor `host_files_remove_dir` in `src/plugin/host/files.rs:142-161`**

Current:

```rust
pub fn host_files_remove_dir(
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
```

Replace with:

```rust
pub fn host_files_remove_dir(
    ctx: &HostContext,
    path: &str,
    recursive: bool,
) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let result = if recursive {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_dir(path)
    };
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ErrorCode::NotFound),
        Err(_) => Err(ErrorCode::IoFailed),
    }
}
```

- [ ] **Step 6: Refactor the 5 deny stubs in `src/plugin/host/files.rs:178-212`**

Current:

```rust
pub fn deny_files_write_file(
    _ctx: &mut HostContext,
    _path: String,
    _data: Vec<u8>,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_append_file(
    _ctx: &mut HostContext,
    _path: String,
    _data: Vec<u8>,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_create_dir(
    _ctx: &mut HostContext,
    _path: String,
    _recursive: bool,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_remove_file(_ctx: &mut HostContext, _path: String) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_remove_dir(
    _ctx: &mut HostContext,
    _path: String,
    _recursive: bool,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}
```

Replace with:

```rust
pub fn deny_files_write_file(
    _ctx: &HostContext,
    _path: &str,
    _data: Vec<u8>,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_append_file(
    _ctx: &HostContext,
    _path: &str,
    _data: Vec<u8>,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_create_dir(
    _ctx: &HostContext,
    _path: &str,
    _recursive: bool,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_remove_file(_ctx: &HostContext, _path: &str) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_remove_dir(
    _ctx: &HostContext,
    _path: &str,
    _recursive: bool,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}
```

- [ ] **Step 7: Update remaining unit-test call sites in `src/plugin/host/files.rs`**

Run:

```bash
grep -n "host_files_write_file\|host_files_append_file\|host_files_create_dir\|host_files_remove_file\|host_files_remove_dir" /Users/kazukiyamamoto/Projects/rust/kish/src/plugin/host/files.rs
```

For each test call, apply the same transformations as Task 6 Step 5:
- `&mut ctx` → `&ctx`
- Drop `.into()` / `.to_string_lossy().into_owned()` / `String::new()` → use `&str` directly
- Use `&path.to_string_lossy()` for `PathBuf` → `&str` (Cow auto-derefs)

Examples (lines 320-360 area):

```rust
// Before (line ~328-329):
host_files_write_file(&mut ctx, p.clone(), b"hello".to_vec()).unwrap();
host_files_append_file(&mut ctx, p, b" world".to_vec()).unwrap();

// After (assuming p is String):
host_files_write_file(&ctx, &p, b"hello".to_vec()).unwrap();
host_files_append_file(&ctx, &p, b" world".to_vec()).unwrap();
```

```rust
// Before (line ~342):
host_files_create_dir(&mut ctx, nested.to_string_lossy().into_owned(), true).unwrap();

// After:
host_files_create_dir(&ctx, &nested.to_string_lossy(), true).unwrap();
```

After updating all call sites, drop `mut` from any `let mut ctx = ...` lines that no longer need it (every test in this file should now work with immutable `ctx`).

- [ ] **Step 8: Refactor the 10 `func_wrap` closures in `src/plugin/linker.rs:166-219`**

The current `files` instance has, after the read closures, write/append/create/remove closures. Both granted and deny sides have 5 of each (10 total). Replace ALL of them.

Current (lines ~166-225, the granted+deny blocks for write/append/create/remove):

```rust
    if has(allowed, CAP_FILES_WRITE) {
        files.func_wrap(
            "write-file",
            |mut store, (path, data): (String, Vec<u8>)| {
                Ok((host_files_write_file(store.data_mut(), path, data),))
            },
        )?;
        files.func_wrap(
            "append-file",
            |mut store, (path, data): (String, Vec<u8>)| {
                Ok((host_files_append_file(store.data_mut(), path, data),))
            },
        )?;
        files.func_wrap(
            "create-dir",
            |mut store, (path, recursive): (String, bool)| {
                Ok((host_files_create_dir(store.data_mut(), path, recursive),))
            },
        )?;
        files.func_wrap("remove-file", |mut store, (path,): (String,)| {
            Ok((host_files_remove_file(store.data_mut(), path),))
        })?;
        files.func_wrap(
            "remove-dir",
            |mut store, (path, recursive): (String, bool)| {
                Ok((host_files_remove_dir(store.data_mut(), path, recursive),))
            },
        )?;
    } else {
        files.func_wrap(
            "write-file",
            |mut store, (path, data): (String, Vec<u8>)| {
                Ok((deny_files_write_file(store.data_mut(), path, data),))
            },
        )?;
        files.func_wrap(
            "append-file",
            |mut store, (path, data): (String, Vec<u8>)| {
                Ok((deny_files_append_file(store.data_mut(), path, data),))
            },
        )?;
        files.func_wrap(
            "create-dir",
            |mut store, (path, recursive): (String, bool)| {
                Ok((deny_files_create_dir(store.data_mut(), path, recursive),))
            },
        )?;
        files.func_wrap("remove-file", |mut store, (path,): (String,)| {
            Ok((deny_files_remove_file(store.data_mut(), path),))
        })?;
        files.func_wrap(
            "remove-dir",
            |mut store, (path, recursive): (String, bool)| {
                Ok((deny_files_remove_dir(store.data_mut(), path, recursive),))
            },
        )?;
    }
```

Replace with:

```rust
    if has(allowed, CAP_FILES_WRITE) {
        files.func_wrap(
            "write-file",
            |store, (path, data): (wasmtime::component::WasmStr, Vec<u8>)| {
                let path_str = path.to_str(&store)?;
                Ok((host_files_write_file(store.data(), &path_str, data),))
            },
        )?;
        files.func_wrap(
            "append-file",
            |store, (path, data): (wasmtime::component::WasmStr, Vec<u8>)| {
                let path_str = path.to_str(&store)?;
                Ok((host_files_append_file(store.data(), &path_str, data),))
            },
        )?;
        files.func_wrap(
            "create-dir",
            |store, (path, recursive): (wasmtime::component::WasmStr, bool)| {
                let path_str = path.to_str(&store)?;
                Ok((host_files_create_dir(store.data(), &path_str, recursive),))
            },
        )?;
        files.func_wrap("remove-file", |store, (path,): (wasmtime::component::WasmStr,)| {
            let path_str = path.to_str(&store)?;
            Ok((host_files_remove_file(store.data(), &path_str),))
        })?;
        files.func_wrap(
            "remove-dir",
            |store, (path, recursive): (wasmtime::component::WasmStr, bool)| {
                let path_str = path.to_str(&store)?;
                Ok((host_files_remove_dir(store.data(), &path_str, recursive),))
            },
        )?;
    } else {
        files.func_wrap(
            "write-file",
            |store, (path, data): (wasmtime::component::WasmStr, Vec<u8>)| {
                let path_str = path.to_str(&store)?;
                Ok((deny_files_write_file(store.data(), &path_str, data),))
            },
        )?;
        files.func_wrap(
            "append-file",
            |store, (path, data): (wasmtime::component::WasmStr, Vec<u8>)| {
                let path_str = path.to_str(&store)?;
                Ok((deny_files_append_file(store.data(), &path_str, data),))
            },
        )?;
        files.func_wrap(
            "create-dir",
            |store, (path, recursive): (wasmtime::component::WasmStr, bool)| {
                let path_str = path.to_str(&store)?;
                Ok((deny_files_create_dir(store.data(), &path_str, recursive),))
            },
        )?;
        files.func_wrap("remove-file", |store, (path,): (wasmtime::component::WasmStr,)| {
            let path_str = path.to_str(&store)?;
            Ok((deny_files_remove_file(store.data(), &path_str),))
        })?;
        files.func_wrap(
            "remove-dir",
            |store, (path, recursive): (wasmtime::component::WasmStr, bool)| {
                let path_str = path.to_str(&store)?;
                Ok((deny_files_remove_dir(store.data(), &path_str, recursive),))
            },
        )?;
    }
```

- [ ] **Step 9: Compile-check**

```bash
cargo check --features test-helpers 2>&1 | tail -15
cargo build --release 2>&1 | tail -3
```

Expected: clean compile.

- [ ] **Step 10: Run regression tests (full suite)**

```bash
cargo test --features test-helpers 2>&1 | tail -30
```

Expected: 2,177/2,177 pass. This is the full broad gate — catches any test that calls one of the 8 host fns we renamed AND any ripple from the linker closure changes.

If any test fails, READ the error carefully — likely a missed call-site update. Fix and re-run.

- [ ] **Step 11: Commit**

```bash
git add src/plugin/host/files.rs src/plugin/linker.rs
git commit -m "$(cat <<'EOF'
feat(plugin): borrow paths in files write/append/create/remove (rollout)

Apply WasmStr borrow pattern to the five mutation files host imports
(write-file, append-file, create-dir, remove-file, remove-dir). Host
fns retyped to (&HostContext, &str[, ...]). Bodies unchanged otherwise
(these fns never accessed ShellEnv, only ensure_bound). Linker
closures rewritten to WasmStr + store.data(). Vec<u8> data params
remain owned — list<u8> lift is a separate codepath, out of scope.

This completes the 11-function String-arg rollout from the spec.
Unit-test call sites in host/files.rs updated mechanically.

Plan: docs/superpowers/plans/2026-05-08-plugin-host-import-borrow-rollout.md (Task 7)
EOF
)"
```

---

## Task 8: Run final dhat measurements

**Files:** none modified (target/perf/ scratch only)

- [ ] **Step 1: Rebuild profiling yosh-dhat**

```bash
cargo build --profile profiling --features dhat-heap --bin yosh-dhat 2>&1 | tail -3
```

Expected: `Finished profiling profile`. The binary now contains the rollout source.

- [ ] **Step 2: Run dhat for `noop_var_set` (after-rollout)**

```bash
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat --exec-loop 1000 noop_var_set 2>&1 | tail -5
mv dhat-heap.json target/perf/dhat-rollout-noop_var_set-after.json
```

- [ ] **Step 3: Run dhat for `noop_files_read` (after-rollout)**

```bash
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat --exec-loop 1000 noop_files_read 2>&1 | tail -5
mv dhat-heap.json target/perf/dhat-rollout-noop_files_read-after.json
```

- [ ] **Step 4: Run dhat for `noop_files_remove` (after-rollout)**

```bash
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat --exec-loop 1000 noop_files_remove 2>&1 | tail -5
mv dhat-heap.json target/perf/dhat-rollout-noop_files_remove-after.json
```

- [ ] **Step 5: Compute deltas**

For each command, the dhat summary line `dhat: Total: <X> bytes in <Y> blocks` (printed during the run) is the metric. Compare baseline (Task 3) vs. after (Steps 2-4):

```bash
{
  echo "=== rollout dhat deltas ==="
  echo "noop_var_set:       baseline <BLOCKS_baseline_var_set>     →  after <BLOCKS_after_var_set>     (Δ <DELTA_var_set>)     [target: -2,000]"
  echo "noop_files_read:    baseline <BLOCKS_baseline_files_read>  →  after <BLOCKS_after_files_read>  (Δ <DELTA_files_read>)  [target: -1,000]"
  echo "noop_files_remove:  baseline <BLOCKS_baseline_files_rm>    →  after <BLOCKS_after_files_rm>    (Δ <DELTA_files_rm>)    [target: -1,000]"
} >> target/perf/rollout-baseline.txt
cat target/perf/rollout-baseline.txt
```

Replace placeholders with the actual numbers. Each Δ should be negative (after < baseline).

- [ ] **Step 6: Decision**

Apply the decision matrix from spec §5.3:

- **All three Δ hit target (≤ −1,000 / ≤ −2,000)** → Success. Proceed to Task 10 success path.
- **Some Δ miss target** → Partial success. Identify which path(s) fell short. Proceed to Task 10 partial path (revert just the failing rollout commits).
- **All three Δ ≥ 0 (no improvement at all)** → Total failure. Proceed to Task 10 failure path (revert all rollout commits, keep `bound_env_with` helper and perf_plugin extensions which are independently useful).

---

## Task 9: Run regression bench

**Files:** none modified

- [ ] **Step 1: Run `plugin_exec_burst_var` (regression gate)**

```bash
cargo bench --bench plugin_bench --features test-helpers -- plugin_exec_burst_var 2>&1 | tail -10
```

Expected: median time ≤ 1,170 ns (matches the §4.1 PoC's improved baseline; do not regress that prior win).

- [ ] **Step 2: Capture median**

```bash
echo "burst_var post-rollout: $(jq '.median.point_estimate' target/criterion/plugin_exec_burst_var/new/estimates.json) ns" \
  >> target/perf/rollout-baseline.txt
```

If median > 1,170 ns by more than 3% (~1,205 ns), the rollout has caused a ripple regression on the `variables::get` hot path (which `burst_var` exercises 10× per call). Treat as failure for Task 10; full revert of rollout commits.

If median ≤ 1,170 ns, proceed to Task 10.

---

## Task 10: Write Appendix B and update TODO.md

**Files:**
- Modify: `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` (append Appendix B)
- Modify: `TODO.md:48` (rollout entry)

This task has three branches based on Task 8 verdict.

### Task 10A: Success path (all three Δ hit target)

- [ ] **Step 1: Append success template to report**

Append to the END of `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` (after Appendix A):

```markdown

## Appendix B: §4.1 Phase 2 Rollout Result — Success

**Date:** YYYY-MM-DD
**Spec:** `docs/superpowers/specs/2026-05-08-plugin-host-import-borrow-rollout-design.md`
**Plan:** `docs/superpowers/plans/2026-05-08-plugin-host-import-borrow-rollout.md`
**Commit (rollout HEAD):** `<sha>`

### Coverage

11 host imports converted to `WasmStr` parameters across:

- `variables::set`, `variables::export-env` (uses new `bound_env_with` helper)
- `filesystem::set-cwd`
- `files::read-file`, `files::read-dir`, `files::metadata`, `files::write-file`, `files::append-file`, `files::create-dir`, `files::remove-file`, `files::remove-dir`

The 9 `filesystem`/`files` host fns only needed signature changes
(they call `ensure_bound` and never accessed `ShellEnv` directly);
only the two `variables` mutation paths needed `bound_env_with`.

### Decisive cross-check (dhat `--exec-loop 1000`)

| Command | Baseline blocks | After blocks | Δ | Target |
|---|---|---|---|---|
| `noop_var_set` | <X> | <Y> | <ΔY> | −2,000 |
| `noop_files_read` | <X> | <Y> | <ΔY> | −1,000 |
| `noop_files_remove` | <X> | <Y> | <ΔY> | −1,000 |

All three meet target. The dual-`WasmStr` mutation pattern via
`bound_env_with` and the single-`WasmStr` patterns (read-only and
mutation-via-`ensure_bound`) all eliminate one host-side `String`
allocation per crossing per parameter.

### Regression check

- `plugin_exec_burst_var` Criterion median: <X> ns (baseline 1,170 ns from §4.1 PoC). Within tolerance — no ripple regression on the read path.
- `cargo test --features test-helpers`: 2,177/2,177 pass.

### Follow-up

- `Vec<u8>` data parameters (`io::write`, `files::write-file`/`append-file` data) — separate `list<u8>` lift codepath, worth a dedicated spike.
- `commands::exec` argv (`Vec<String>` → `list<string>`) — separate codepath, separate spec.
```

Replace placeholders with actual numbers.

- [ ] **Step 2: Update `TODO.md:48`**

Find the line containing `Plugin perf P0 (rollout)` and replace it with:

```markdown
- [ ] Plugin perf §4.1 rollout COMPLETE (YYYY-MM-DD): 11 String-arg host imports converted to WasmStr borrow pattern. dhat verified all three measurement paths hit target alloc reduction (−1,000 / −2,000 blocks per 1,000 iterations). See `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` Appendix B. Follow-up candidates: Vec<u8> data params and commands::exec argv (separate list-lift codepaths).
```

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/specs/2026-05-08-plugin-perf-report.md TODO.md
git commit -m "$(cat <<'EOF'
docs(plugin-perf): record §4.1 rollout success result

11 String-arg host imports converted to WasmStr borrow pattern. dhat
verification on three measurement paths confirmed alloc-elimination
hits target (-1,000 / -2,000 blocks per 1,000 iterations across the
read-only, single-arg mutation, and dual-arg mutation patterns).
plugin_exec_burst_var regression gate held within tolerance. Follow-up
items (Vec<u8>, commands::exec) tracked in TODO.md.

Plan: docs/superpowers/plans/2026-05-08-plugin-host-import-borrow-rollout.md
EOF
)"
```

Skip Tasks 10B and 10C.

### Task 10B: Partial-success path (some Δ hit target, some don't)

- [ ] **Step 1: Identify failing function(s) and revert their commit(s)**

Determine from Task 8 which function(s) missed the alloc-target. Map back to the commit(s):

| Failing measurement | Likely failed commits |
|---|---|
| `noop_var_set` short of −2,000 | Task 4 commit (variables) |
| `noop_files_read` short of −1,000 | Task 6 commit (files read) |
| `noop_files_remove` short of −1,000 | Task 7 commit (files mutation) |

For each failing area, revert just that commit:

```bash
git log --oneline -8
# Identify the SHA of the failing-area commit(s)
git revert --no-edit <sha-of-failing-commit>
```

- [ ] **Step 2: Re-run regression test after revert**

```bash
cargo test --features test-helpers 2>&1 | tail -10
```

Expected: 2,177/2,177 pass.

- [ ] **Step 3: Append partial-success template to report**

Append to the END of `docs/superpowers/specs/2026-05-08-plugin-perf-report.md`:

```markdown

## Appendix B: §4.1 Phase 2 Rollout Result — Partial Success

**Date:** YYYY-MM-DD
**Spec:** `docs/superpowers/specs/2026-05-08-plugin-host-import-borrow-rollout-design.md`
**Plan:** `docs/superpowers/plans/2026-05-08-plugin-host-import-borrow-rollout.md`
**Commit (rollout HEAD after revert):** `<sha>`

### Outcome

The rollout converted some but not all of the 11 target host imports.
The following paths are now using `WasmStr`:

- <list the kept paths>

The following paths were reverted because their dhat-measurement
target was not met:

- <list the reverted paths>

### Measurements (dhat `--exec-loop 1000`)

| Command | Baseline blocks | After blocks | Δ | Target | Outcome |
|---|---|---|---|---|---|
| `noop_var_set` | <X> | <Y> | <ΔY> | −2,000 | <kept/reverted> |
| `noop_files_read` | <X> | <Y> | <ΔY> | −1,000 | <kept/reverted> |
| `noop_files_remove` | <X> | <Y> | <ΔY> | −1,000 | <kept/reverted> |

### Hypothesis for the missed target

<one paragraph explaining why the failing path didn't hit target —
e.g. "the closure introduced an extra allocation through ...">

### Next action

The kept paths reduce per-crossing alloc cost as designed. The
reverted paths remain on the `String` baseline. A follow-up spec
should investigate the specific failure mode before retrying those
paths.
```

- [ ] **Step 4: Update `TODO.md:48`**

Replace the rollout line with:

```markdown
- [ ] Plugin perf §4.1 rollout PARTIAL (YYYY-MM-DD): kept paths <list>; reverted paths <list> due to alloc-target miss. See `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` Appendix B for measurements. Reverted paths need follow-up investigation before retry.
```

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/specs/2026-05-08-plugin-perf-report.md TODO.md
git commit -m "$(cat <<'EOF'
docs(plugin-perf): record §4.1 rollout partial-success result

The rollout converted <N> of 11 String-arg host imports to WasmStr;
<M> paths were reverted after dhat measurement showed the
alloc-elimination target was not met. The kept paths' wins are
documented in Appendix B; the reverted paths return to baseline and
need follow-up investigation of the specific failure mode.

Plan: docs/superpowers/plans/2026-05-08-plugin-host-import-borrow-rollout.md
EOF
)"
```

Skip Task 10C.

### Task 10C: Total-failure path (all three Δ ≥ 0, or `plugin_exec_burst_var` regressed)

- [ ] **Step 1: Revert the four rollout source commits**

Identify the four `feat(plugin): borrow ...` commits from Tasks 4, 5, 6, 7 (do NOT revert Tasks 1-2 — `bound_env_with` helper and `perf_plugin` extensions are independently useful):

```bash
git log --oneline -10
# Identify the SHAs of the four rollout commits
git revert --no-edit <sha-task-7> <sha-task-6> <sha-task-5> <sha-task-4>
```

(Revert in reverse order so each revert applies cleanly.)

- [ ] **Step 2: Verify clean state**

```bash
cargo test --features test-helpers 2>&1 | tail -10
git diff src/plugin/linker.rs src/plugin/host/files.rs src/plugin/host/filesystem.rs src/plugin/host/variables.rs
```

Expected: all 2,177 tests pass; the source files diff against pre-rollout state should be empty (`bound_env_with` in `mod.rs` stays since Task 1 isn't reverted).

- [ ] **Step 3: Append failure template to report**

Append to the END of `docs/superpowers/specs/2026-05-08-plugin-perf-report.md`:

```markdown

## Appendix B: §4.1 Phase 2 Rollout Result — Negative

**Date:** YYYY-MM-DD
**Spec:** `docs/superpowers/specs/2026-05-08-plugin-host-import-borrow-rollout-design.md`
**Plan:** `docs/superpowers/plans/2026-05-08-plugin-host-import-borrow-rollout.md`
**Commit (rollout reverted at):** `<sha>`

### Outcome

Rollout source changes (Tasks 4-7) reverted. The `bound_env_with`
helper (Task 1) and `perf_plugin` measurement-command extensions
(Task 2) are kept — they are independently useful and not part of
the failed change. The 11 String-arg host imports remain on the
`String` baseline.

### Measurements (dhat `--exec-loop 1000`)

| Command | Baseline blocks | After (pre-revert) blocks | Δ | Target |
|---|---|---|---|---|
| `noop_var_set` | <X> | <Y> | <ΔY> | −2,000 |
| `noop_files_read` | <X> | <Y> | <ΔY> | −1,000 |
| `noop_files_remove` | <X> | <Y> | <ΔY> | −1,000 |

### Hypothesis for the null result

<one paragraph — e.g. "all three measurements showed near-zero alloc
delta, suggesting the WasmStr lift internally allocates regardless of
closure-arg type ascription. The §4.1 PoC's positive result on
`variables::get` may have been an outlier or specific to that one
host fn's call shape.">

### Next action

§4.1 closed by this rollout failure. Per report §5.2 ordering, next
plugin-perf project is §4.3 (cwasm cache-miss observability) or
§4.2 (cached linker).
```

- [ ] **Step 4: Update `TODO.md:48`**

Replace the rollout line with:

```markdown
- [ ] Plugin perf §4.1 rollout FAILED (YYYY-MM-DD): all three rollout-measurement paths missed alloc-elimination target after WasmStr conversion. Source changes reverted; bound_env_with helper and perf_plugin commands kept (independently useful). See `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` Appendix B. Next plugin-perf project: §4.3 (cwasm cache-miss observability) or §4.2 (cached linker).
```

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/specs/2026-05-08-plugin-perf-report.md TODO.md
git commit -m "$(cat <<'EOF'
docs(plugin-perf): record §4.1 rollout negative result

All three rollout-measurement paths (noop_var_set, noop_files_read,
noop_files_remove) failed to meet the dhat alloc-elimination target
after the WasmStr conversion was applied. Source changes (Tasks 4-7)
have been reverted; the bound_env_with helper and perf_plugin command
extensions are kept as independently useful infrastructure. §4.1
rollout is closed by this attempt; next plugin-perf project is §4.3
(cwasm cache-miss observability) or §4.2 (cached linker) per
report §5.2.

Plan: docs/superpowers/plans/2026-05-08-plugin-host-import-borrow-rollout.md
EOF
)"
```

---

## Done — Final state checklist

After Task 10 (one of 10A/10B/10C), verify:

- [ ] `cargo build --release` succeeds
- [ ] `cargo test --features test-helpers` passes (2,177/2,177)
- [ ] `git status -s` is clean except for gitignored `target/perf/*` scratch
- [ ] HEAD commit message references the plan and either Appendix B (success/partial) or the revert chain (failure)
- [ ] `TODO.md:48` reflects the outcome (rollout complete / partial / failed-and-closed)
- [ ] Report Appendix B exists and is filled in (no template placeholders left)
