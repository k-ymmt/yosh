# SP1 — `src/plugin/host.rs` Responsibility Redesign

Part of the [Large-File Responsibility Redesign Umbrella](2026-05-06-large-file-redesign-umbrella-design.md).

## Current State

`src/plugin/host.rs` is 1004 lines (~600 production + ~400 tests). All host import functions are grouped by `host_<group>_<op>` naming, but they share one file. The single facade `HostContext` carries WASI ctx, ShellEnv pointer, and allowed-commands patterns; every host function opens with `ctx.env_mut().ok_or(ErrorCode::Denied)?` to satisfy the §5 metadata-cannot-reach-host-APIs invariant.

External callers (`src/plugin/linker.rs`) import 19 host symbols by name.

## Proposed Structure

```
src/plugin/host/
  mod.rs        — HostContext, WasiView impl, with_env helper, public re-export
  variables.rs  — host_variables_{get,set,export_env}                                (3 fns)
  filesystem.rs — host_filesystem_{cwd,set_cwd}                                      (2 fns)
  io.rs         — host_io_write                                                      (1 fn)
  files.rs      — host_files_{read_file,read_dir,metadata,write_file,append_file,
                  create_dir,remove_file,remove_dir}                                 (8 fns)
  commands.rs   — host_commands_exec, deny_commands_exec, spawn_with_timeout         (3 fns)
```

| File | Production | Tests | Total |
|---|---|---|---|
| `mod.rs` | ~110 | ~30 | ~140 |
| `variables.rs` | ~65 | ~70 | ~135 |
| `filesystem.rs` | ~30 | ~50 | ~80 |
| `io.rs` | ~30 | ~50 | ~80 |
| `files.rs` | ~230 | ~210 | ~440 |
| `commands.rs` | ~140 | ~150 | ~290 |

`files.rs` exceeds the 400-line guideline because the eight file-I/O functions share an error-mapping table and tightly coupled tests for parent-dir behavior. Splitting into read/write/delete sub-files would scatter the table across files; we keep them together as a documented exception.

## Responsibility Redesign — `with_env` Helper

Today every host function opens with the same null-env guard:

```rust
pub(super) fn host_variables_get(ctx: &mut HostContext, name: String) -> Result<String, ErrorCode> {
    let env = ctx.env_mut().ok_or(ErrorCode::Denied)?;
    Ok(env.vars.get(&name).cloned().unwrap_or_default())
}
```

The metadata contract (§5: "metadata cannot reach host APIs") is enforced once per function — 19 copies of the same pattern, with 19 corresponding `metadata_contract_real_*_denied_when_env_null` tests.

Add a method to `HostContext` that captures the contract structurally:

```rust
impl HostContext {
    /// Run `f` with the bound `ShellEnv`. Returns `Err(Denied)` if env is null
    /// (metadata-contract enforcement: §5 metadata-cannot-reach-host-APIs).
    pub(super) fn with_env<F, T>(&mut self, f: F) -> Result<T, ErrorCode>
    where
        F: FnOnce(&mut ShellEnv) -> Result<T, ErrorCode>,
    {
        match self.env_mut() {
            Some(env) => f(env),
            None => Err(ErrorCode::Denied),
        }
    }
}
```

Each host function rewrites to:

```rust
pub(super) fn host_variables_get(ctx: &mut HostContext, name: String) -> Result<String, ErrorCode> {
    ctx.with_env(|env| Ok(env.vars.get(&name).cloned().unwrap_or_default()))
}
```

`host_commands_exec` does not read env directly but still requires the metadata guard. It uses the same helper with an unused closure parameter:

```rust
pub(super) fn host_commands_exec(ctx: &mut HostContext, program: String, args: Vec<String>)
    -> Result<ExecOutput, ErrorCode>
{
    ctx.with_env(|_env| {
        if program.is_empty() { return Err(ErrorCode::InvalidArgument); }
        let mut argv = Vec::with_capacity(1 + args.len());
        argv.push(program.clone());
        argv.extend(args.iter().cloned());
        if !ctx.allowed_commands.iter().any(|p| p.matches(&argv)) {
            return Err(ErrorCode::PatternNotAllowed);
        }
        spawn_with_timeout(&program, &args, std::time::Duration::from_millis(1000))
    })
}
```

Note: `ctx` is moved into the closure for `allowed_commands` access — adjust the helper signature or inline the lookup if borrow-checker rules require it. The exact form will be settled during implementation; the structural effect (every host function goes through one chokepoint) is the design intent.

### Effect

1. The metadata contract is **structurally enforced** — host functions cannot reach `&mut ShellEnv` except through `with_env`.
2. Per-function boilerplate disappears; each function becomes one closure expression.
3. The 19 `metadata_contract_real_*_denied_when_env_null` tests collapse into:
   - 1 test for `with_env`'s null-env behavior (in `mod.rs`).
   - 1 representative test per capability module (5 spot tests) verifying the deny path is reachable.
   - 12 tests deleted (~150 lines of test code).

## Test Reorganization

| Test Group | Current Count | New Count | Location |
|---|---|---|---|
| `with_env` null/bound behavior | 0 | 2 | `mod.rs` |
| Per-capability metadata-contract spot tests | 19 | 5 | One per capability module (variables, filesystem, io, files, commands) |
| Behavioral tests (file roundtrip, exec captures, etc.) | unchanged | unchanged | Each capability module |

Test helpers (`null_env_ctx`, `bound_env_ctx`, `ctx_with_allowed`) live in `host/mod.rs`'s `#[cfg(test)] mod test_helpers` and are imported as `super::test_helpers::*` from each capability module.

## Public API (linker.rs Compatibility)

`linker.rs` currently imports 19 symbols:

```rust
use super::host::{
    host_commands_exec, host_files_append_file, host_files_create_dir, host_files_metadata,
    host_files_read_dir, host_files_read_file, host_files_remove_dir, host_files_remove_file,
    host_files_write_file, host_filesystem_cwd, host_filesystem_set_cwd, host_io_write,
    host_variables_export_env, host_variables_get, host_variables_set,
};
```

These imports are preserved by re-exporting from `host/mod.rs`:

```rust
mod variables;
mod filesystem;
mod io;
mod files;
mod commands;

pub(super) use variables::{host_variables_export_env, host_variables_get, host_variables_set};
pub(super) use filesystem::{host_filesystem_cwd, host_filesystem_set_cwd};
pub(super) use io::host_io_write;
pub(super) use files::{
    host_files_append_file, host_files_create_dir, host_files_metadata,
    host_files_read_dir, host_files_read_file, host_files_remove_dir,
    host_files_remove_file, host_files_write_file,
};
pub(super) use commands::{deny_commands_exec, host_commands_exec};
```

`linker.rs` requires zero changes.

## PR Breakdown

1. **PR-A — Scaffolding.** Create `host/mod.rs`, add the `with_env` helper, rename `host.rs` to `host/all.rs` keeping all bodies intact, and re-export every symbol so callers don't change. `cargo test` PASS confirms the move alone is safe.
2. **PR-B — Capability split + redesign.** Decompose `all.rs` into the five capability modules. Rewrite each host function to use `with_env`. Move tests into the capability modules. Collapse the 19 metadata-contract tests as described. Delete `all.rs`.

PR-A is mechanical; PR-B is the responsibility-redesign body. Reviewers can verify each independently.

## Risks

- The `with_env` closure borrows `&mut self` across the call. If `host_commands_exec` needs both `&mut ShellEnv` and `&allowed_commands`, the closure form may require restructuring (e.g., split into `ensure_metadata_bound() -> Result<(), Denied>` plus inline allowed-commands check). Either form is acceptable; the goal is structural enforcement of the metadata contract.
- `spawn_with_timeout` lives in `commands.rs` as a private helper. Tests for it remain in `commands.rs` (`host_commands_exec_timeout_after_1000ms`, `host_commands_exec_kills_child_on_timeout`).
- `null_env_ctx` is currently defined inside `host.rs::tests`; moving to a `test_helpers` module changes its path. All callers update accordingly.

## Definition of Done

- `cargo test -p yosh` PASS (unit + integration).
- `cargo test --features test-helpers` PASS — the `tests/plugin.rs` integration suite passes after rebuilding the WASM test plugins (`cargo component build -p test_plugin --target wasm32-wasip2 --release` and similarly for `trap_plugin`/`slow_plugin`).
- `cargo clippy --all-targets -- -D warnings` — only pre-existing violations remain.
- Each capability file ≤ 290 lines (`files.rs` ~440 lines is the documented exception).
- TODO.md entry "`src/plugin/host.rs` is now ~970 lines after the `commands:exec` addition. Consider splitting into `src/plugin/host/{mod,variables,filesystem,io,files}.rs`…" is removed.
- `linker.rs` source is byte-for-byte identical (only host/ contents change).
