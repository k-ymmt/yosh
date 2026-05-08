# Plugin Host-Import Borrow Rollout — Design

**Date:** 2026-05-08
**Phase:** 2, P0 (rollout follow-up to §4.1 PoC)
**Predecessor PoC:** `docs/superpowers/specs/2026-05-08-plugin-host-import-borrow-design.md` (commit `c22e63a`)
**Predecessor result:** `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` Appendix A
**Status:** Spec; awaiting plan + execution.

## 1. Goal & Success Criterion

Apply the `WasmStr` borrow pattern proven in the §4.1 PoC to the
remaining `String`-typed host-import parameters across all 11
candidate functions. The PoC eliminated exactly one host-side
canonical-ABI `String` allocation per `variables::get` crossing
(measured by dhat: −1,000 allocation blocks per 1,000 iterations).
Rollout extends the same elimination to `variables::set`,
`variables::export-env`, `filesystem::set-cwd`, and the seven
`files::*` functions.

### 1.1 Target functions (11)

| # | Host import | Args | Helper |
|---|---|---|---|
| 1 | `filesystem::set-cwd` | `WasmStr` | `bound_env_with` |
| 2 | `files::read-file` | `WasmStr` | `bound_env_ref` |
| 3 | `files::read-dir` | `WasmStr` | `bound_env_ref` |
| 4 | `files::metadata` | `WasmStr` | `bound_env_ref` |
| 5 | `files::write-file` | `WasmStr, Vec<u8>` | `bound_env_with` |
| 6 | `files::append-file` | `WasmStr, Vec<u8>` | `bound_env_with` |
| 7 | `files::create-dir` | `WasmStr, bool` | `bound_env_with` |
| 8 | `files::remove-file` | `WasmStr` | `bound_env_with` |
| 9 | `files::remove-dir` | `WasmStr, bool` | `bound_env_with` |
| 10 | `variables::set` | `WasmStr, WasmStr` | `bound_env_with` |
| 11 | `variables::export-env` | `WasmStr, WasmStr` | `bound_env_with` |

### 1.2 Success criterion

Decisive measurement is dhat alloc-diff (deterministic, noise-free —
the lesson from the §4.1 PoC's contaminated `noop_var` Criterion
metric). Three representative functions are measured against their
own pre-rollout baseline:

| Bench (dhat `--exec-loop 1000 <cmd>`) | Expected Δ blocks | Proves |
|---|---|---|
| `noop_var_set` (calls `variables.set("PERF_VAR", "v")`) | **−2,000** | Mutation dual-`String` `bound_env_with` pattern |
| `noop_files_read` (calls `files.read-file("/dev/null")`) | **−1,000** | Read-only pattern extends to `files::*` |
| `noop_files_remove` (calls `files.remove-file("/tmp/...")`) | **−1,000** | Mutation single-`String` `bound_env_with` pattern |

Plus regression gates:

- `cargo test --features test-helpers` — 2,177/2,177 pass
- `plugin_exec_burst_var` Criterion median ≤ 1,170 ns (do not regress
  the §4.1 PoC's improvement; this bench exercises 10
  `variables::get` crossings and is unaffected by the rollout, so any
  regression here would indicate ripple damage)

### 1.3 Out of scope

- `Vec<u8>` data parameters in `files::write-file`,
  `files::append-file`, `io::write`. The `list<u8>` lift is a separate
  wasmtime codepath (`WasmList<u8>` or similar). Worth measuring in a
  follow-up spec but mixing it here would dilute focus.
- `commands::exec` argv (`Vec<String>` → `list<string>`). Different
  lift codepath; needs its own spike.
- `filesystem::cwd` — no parameters, nothing to borrow.
- `variables::get` — already done in §4.1 PoC.
- WIT changes. The `yosh-plugin.wit` interface is unchanged. Existing
  `perf_plugin.wasm` is unaffected; a separate `perf_plugin` source
  change is needed only to add new measurement commands (§4 below).

## 2. Approach

### 2.1 New helper

Add to `src/plugin/host/mod.rs`:

```rust
impl HostContext {
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
}
```

`bound_env_ref` (added in §4.1 PoC) handles read-only paths.
`bound_env_with` is the new helper for mutation paths. Both take
`&self` and are reachable through `store.data()` without a mutable
store borrow.

### 2.2 Host-fn signature transformation

| Pattern | Before | After |
|---|---|---|
| Read-only single-`String` | `(&mut HostContext, String) -> R` | `(&HostContext, &str) -> R` |
| Mutation single-`String` | `(&mut HostContext, String) -> R` | `(&HostContext, &str) -> R` |
| Mutation dual-`String` | `(&mut HostContext, String, String) -> R` | `(&HostContext, &str, &str) -> R` |
| Mutation `String + Vec<u8>` | `(&mut HostContext, String, Vec<u8>) -> R` | `(&HostContext, &str, Vec<u8>) -> R` |
| Mutation `String + bool` | `(&mut HostContext, String, bool) -> R` | `(&HostContext, &str, bool) -> R` |

The `Vec<u8>` parameter stays owned in this rollout (separate spec
will tackle it). The `bool` parameter is `Copy` and unaffected.

### 2.3 Linker closure pattern

Three shapes, illustrated:

```rust
// Single-arg (read-only) — same as §4.1 PoC
files.func_wrap("read-file", |store, (path,): (WasmStr,)| {
    let path_str = path.to_str(&store)?;
    Ok((host_files_read_file(store.data(), &path_str),))
})?;

// Single-arg (mutation)
fs.func_wrap("set-cwd", |store, (path,): (WasmStr,)| {
    let path_str = path.to_str(&store)?;
    Ok((host_filesystem_set_cwd(store.data(), &path_str),))
})?;

// Dual-arg (mutation)
vars.func_wrap("set", |store, (name, value): (WasmStr, WasmStr)| {
    let name_str = name.to_str(&store)?;
    let value_str = value.to_str(&store)?;
    Ok((host_variables_set(store.data(), &name_str, &value_str),))
})?;

// Mixed (path + Vec<u8>)
files.func_wrap("write-file", |store, (path, data): (WasmStr, Vec<u8>)| {
    let path_str = path.to_str(&store)?;
    Ok((host_files_write_file(store.data(), &path_str, data),))
})?;
```

`mut` is dropped from `store` everywhere (no closure needs
`store.data_mut()` after this rollout).

### 2.4 Host-fn body pattern (mutation case)

The mutation host functions wrap the closure body in `bound_env_with`.
Schematic illustration (the real env-mutation calls — e.g.
`env.vars.set`, `env.set_cwd` — already exist in the current
`String`-arg implementations and are kept verbatim, only the
parameter type and the env-borrow path changes):

```rust
pub fn host_variables_set(
    ctx: &HostContext,
    name: &str,
    value: &str,
) -> Result<(), ErrorCode> {
    ctx.bound_env_with(|env| {
        env.vars.set(name, value).map_err(|_| ErrorCode::IoFailed)
    })?
}
```

The double-`Result` is intentional: `bound_env_with` returns
`Result<R, ErrorCode>` for the null-env guard, and the closure's `R`
itself is `Result<(), ErrorCode>`. The trailing `?` flattens
`Result<Result<(), ErrorCode>, ErrorCode>` to `Result<(), ErrorCode>`.

If this nested-`Result` shape proves too verbose across the eight
mutation functions, factor a thin wrapper:

```rust
fn host_with_env<R>(ctx: &HostContext, f: impl FnOnce(&mut ShellEnv) -> Result<R, ErrorCode>) -> Result<R, ErrorCode> {
    ctx.bound_env_with(f)?
}
```

Add only if 3+ functions visibly benefit (YAGNI).

## 3. Scope detail per function

| # | linker.rs lines (granted / deny) | host fn file | Sig change |
|---|---|---|---|
| 1 | `:113-115` / `:120-122` | `host/filesystem.rs` (`host_filesystem_set_cwd`, `deny_filesystem_set_cwd`) | `(&mut HostContext, String)` → `(&HostContext, &str)` |
| 2 | `:143-145` / `:153-155` | `host/files.rs` (`host_files_read_file`, `deny_files_read_file`) | `(&mut HostContext, String)` → `(&HostContext, &str)` |
| 3 | `:146-148` / `:156-158` | `host/files.rs` (`host_files_read_dir`, `deny_files_read_dir`) | same |
| 4 | `:149-151` / `:159-161` | `host/files.rs` (`host_files_metadata`, `deny_files_metadata`) | same |
| 5 | `:166-170` / `:194-198` | `host/files.rs` (`host_files_write_file`, `deny_files_write_file`) | `(&mut HostContext, String, Vec<u8>)` → `(&HostContext, &str, Vec<u8>)` |
| 6 | `:172-176` / `:200-204` | `host/files.rs` (`host_files_append_file`, `deny_files_append_file`) | same |
| 7 | `:178-182` / `:206-210` | `host/files.rs` (`host_files_create_dir`, `deny_files_create_dir`) | `(&mut HostContext, String, bool)` → `(&HostContext, &str, bool)` |
| 8 | `:184-186` / `:212-214` | `host/files.rs` (`host_files_remove_file`, `deny_files_remove_file`) | `(&mut HostContext, String)` → `(&HostContext, &str)` |
| 9 | `:187-191` / `:215-219` | `host/files.rs` (`host_files_remove_dir`, `deny_files_remove_dir`) | `(&mut HostContext, String, bool)` → `(&HostContext, &str, bool)` |
| 10 | `:86-88` / `:96-98` | `host/variables.rs` (`host_variables_set`, `deny_variables_set`) | `(&mut HostContext, String, String)` → `(&HostContext, &str, &str)` |
| 11 | `:89-94` / `:99-104` | `host/variables.rs` (`host_variables_export_env`, `deny_variables_export_env`) | same |

## 4. perf_plugin extensions

Three new measurement commands added to
`tests/plugins/perf_plugin/src/lib.rs`. Each takes no args, performs
exactly one host call, returns 0:

| Command | Body |
|---|---|
| `noop_var_set` | `variables::set("PERF_VAR", "v")` |
| `noop_files_read` | `files::read_file("/dev/null")` (return value discarded) |
| `noop_files_remove` | `files::remove_file("/tmp/yosh-perf-rollout-nonexistent")` (NotFound is fine; alloc count is the metric) |

After adding, rebuild via:

```sh
cargo component build -p perf_plugin --target wasm32-wasip2 --release
```

Then re-stage the plugins.lock with `variables:read+write` and
`files:read+write` capabilities to permit the new commands.

## 5. Verification

### 5.1 dhat measurement protocol

For each of the three new commands, measure baseline (pre-rollout)
and after (post-rollout) alloc counts:

```sh
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat \
    --exec-loop 1000 <command>
mv dhat-heap.json target/perf/dhat-rollout-<command>-{baseline|after}.json
```

Compare `Total: ... blocks` between baseline and after JSON files.
Expected deltas listed in §1.2.

### 5.2 Regression gates

```sh
cargo test --features test-helpers
cargo bench --bench plugin_bench --features test-helpers -- 'plugin_exec_burst_var'
```

All 2,177 tests must pass. `plugin_exec_burst_var` median ≤ 1,170 ns.

### 5.3 Decision matrix

| Outcome | Action |
|---|---|
| All three dhat deltas hit target AND tests pass AND bench OK | **Success** — commit, write rollout result appendix, update TODO.md |
| Any dhat delta misses target | Investigate the specific function; revert that function's path only if cause unclear; treat as **Partial success** if other paths held |
| Any test fails | Full revert, write failure appendix, exit |
| `plugin_exec_burst_var` regresses | Full revert, write failure appendix, exit |

## 6. Negative-result protocol

Partial-revert is permitted (unlike §4.1 PoC which was all-or-nothing
because it had only one function). If e.g. `files::*` rollout works
but `variables::set` doesn't hit −2,000 alloc blocks:

- Revert `variables::set` and `variables::export-env` only
- Keep `files::*` and `filesystem::set-cwd` changes
- Document the partial outcome in the report appendix
- Do not pivot to alternative approaches (intern cache, etc.) inside
  this spec

If the new helper `bound_env_with` itself proves problematic (e.g.,
its closure-based API generates unexpected codegen overhead): full
revert, document the helper's failure mode, and either (a) try a
non-closure variant in a follow-up spec, or (b) close §4.1 rollout
as not-feasible-for-mutation and proceed with read-only-only rollout
in a smaller follow-up.

## 7. Risk Register

| Risk | Impact | Mitigation |
|---|---|---|
| `bound_env_with` closure body has nested `Result` (helper's `Result<R>` + closure's `Result<R, ErrorCode>`) → verbose / hard to read | Maintenance pain | Use trailing `?` to flatten in host-fn bodies; if 3+ host fns repeat, factor into a `host_with_env` helper |
| `WasmStr::to_str` returns `Cow::Owned` for non-UTF-8 input | Alloc not eliminated for that call | Canonical ABI rejects non-UTF-8 at guest boundary; host should never see `Cow::Owned`. The `?` propagates any error path to wasm trap |
| `perf_plugin` rebuild required → CI / dev-env friction | Setup overhead | Plan task includes the `cargo component build` step; the `wasm32-wasip2` target is already a one-time setup per CLAUDE.md |
| 11 functions × granted+deny = 22 closure rewrites; one slip → mismatched signature compile errors | Compile failure mid-rollout | Group changes per host import (granted+deny pair) and run `cargo check` after each pair; commit at natural boundaries |
| Existing unit tests in `host/{variables,files,filesystem}.rs::tests` use old signatures | Test failure | Plan task: grep `mod tests` in each file before refactoring; update test call sites mechanically |
| Partial rollout (e.g. only files::* succeeds) leaves an asymmetric API | Surprising mix of `String` and `&str` host fns | Document the partial state explicitly in TODO.md and report appendix; the rollout should aim for all-or-nothing but tolerate partial as graceful degradation |
| `bound_env_with` exposes mutation through `&self` (raw pointer) — reviewers may flag as unsafe-API leak | Code-review pushback | The pattern is the same as `env_mut` already used by `bound_env`; the SAFETY comment ties to `EnvGuard` invariants. No new `unsafe` invariants are introduced |

## 8. References

- §4.1 PoC spec: `docs/superpowers/specs/2026-05-08-plugin-host-import-borrow-design.md`
- §4.1 PoC plan: `docs/superpowers/plans/2026-05-08-plugin-host-import-borrow-poc.md`
- §4.1 PoC result: `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` Appendix A (commit `c22e63a`)
- Source under change: `src/plugin/linker.rs`, `src/plugin/host/{mod,variables,files,filesystem}.rs`
- Bench harness: `benches/plugin_bench.rs`
- Plugin fixture: `tests/plugins/perf_plugin/`
- TODO.md anchor: line 48 (rollout entry, written by §4.1 PoC commit)
