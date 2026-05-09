# Plugin perf §4.1 follow-up: Borrow `Vec<u8>` data in host imports — Design

**Status:** Draft
**Date:** 2026-05-09
**Author:** k-ymmt (with Claude)
**Related:**
- `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` Appendix B follow-up
- `docs/superpowers/specs/2026-05-08-plugin-host-import-borrow-rollout-design.md` (§4.1 `String` rollout, completed `2262c7c`)

## 1. Background

§4.1 (`2026-05-08`) borrowed all `String` host-import arguments via
`wasmtime::component::WasmStr`, eliminating one host-side `String` allocation
per crossing per parameter. The dhat verdict at HEAD: each `WasmStr` parameter
saves **−1,000 blocks per `--exec-loop 1000`**, exactly matching the per-arg
unit prediction.

The §4.1 report's Appendix B "Follow-up" section identifies two remaining
codepaths that share the same allocation problem but use a different
canonical-ABI lift: `Vec<u8>` (`list<u8>`) and `Vec<String>` (`list<string>`).
Each needs its own spike before authoring a rollout spec.

This spec covers `Vec<u8>` only. `Vec<String>` (`commands::exec` argv) is
out-of-scope and will be authored separately.

## 2. Goal & Scope

### Goal

Replace `Vec<u8> data` parameters in three host imports with
`wasmtime::component::WasmList<u8>`, then read the bytes via `as_le_slice(&store)`
to obtain a zero-copy `&[u8]` slice into the plugin's wasm linear memory.

### Scope (in)

| Host import | Capability | Current `data` arg | New `data` arg |
|---|---|---|---|
| `yosh:plugin/io.write` | `CAP_IO` | `Vec<u8>` | `WasmList<u8>` → `&[u8]` |
| `yosh:plugin/files.write-file` | `CAP_FILES_WRITE` | `Vec<u8>` | `WasmList<u8>` → `&[u8]` |
| `yosh:plugin/files.append-file` | `CAP_FILES_WRITE` | `Vec<u8>` | `WasmList<u8>` → `&[u8]` |

`deny_*` counterparts of the three functions also get the matching
`&[u8]` signature for type consistency.

### Scope (out)

- `commands::exec` argv (`Vec<String>` → `list<string>`) — separate codepath,
  separate spike, separate spec.
- `files::read-file` and similar `Vec<u8>` *return* paths — alloc direction is
  reversed (host → guest), unrelated to `WasmList<u8>` lift.
- `io::read` — currently absent in the WIT.

## 3. Architecture

The pattern is the WasmStr-borrow pattern applied to a list-typed parameter.
`WasmList<u8>::as_le_slice(&store) -> &[u8]` is wasmtime 27's borrow-shaped
accessor that returns a direct slice into the wasm module's linear memory; for
`u8` the alignment requirement is trivially satisfied
(`mem::size_of::<u8>() == 1`) and endianness is irrelevant.

### 3.1 `linker.rs` closures

Three `func_wrap` closures change. Example (`yosh:plugin/io.write`):

```rust
// Before
io.func_wrap("write", |mut store, (target, data): (IoStream, Vec<u8>)| {
    Ok((host_io_write(store.data_mut(), target, data),))
});

// After
io.func_wrap("write", |store, (target, data): (IoStream, WasmList<u8>)| {
    let bytes = data.as_le_slice(&store);
    Ok((host_io_write(store.data(), target, bytes),))
});
```

`store` becomes immutable because `as_le_slice(&store)` borrows `store`
immutably and `host_io_write` no longer requires `&mut HostContext` (see 3.2).
The deny variant follows the same shape.

For `files.write-file` and `files.append-file`, `path: WasmStr` was already
borrowed in §4.1; the new code reads both the path and the data slice from
`&store`, then calls the host fn:

```rust
files.func_wrap(
    "write-file",
    |store, (path, data): (WasmStr, WasmList<u8>)| {
        let path_str = path.to_str(&store)?;
        let bytes = data.as_le_slice(&store);
        Ok((host_files_write_file(store.data(), &path_str, bytes),))
    },
);
```

### 3.2 Host fn signatures

```rust
// src/plugin/host/io.rs
pub fn host_io_write(ctx: &HostContext, target: IoStream, data: &[u8]) -> Result<(), ErrorCode>;
pub fn deny_io_write(ctx: &HostContext, target: IoStream, data: &[u8]) -> Result<(), ErrorCode>;

// src/plugin/host/files.rs
pub fn host_files_write_file(ctx: &HostContext, path: &str, data: &[u8]) -> Result<(), ErrorCode>;
pub fn host_files_append_file(ctx: &HostContext, path: &str, data: &[u8]) -> Result<(), ErrorCode>;
pub fn deny_files_write_file(ctx: &HostContext, path: &str, data: &[u8]) -> Result<(), ErrorCode>;
pub fn deny_files_append_file(ctx: &HostContext, path: &str, data: &[u8]) -> Result<(), ErrorCode>;
```

`host_io_write` drops from `&mut HostContext` to `&HostContext` because its
body only calls `ctx.ensure_bound()` (`&self`) and writes to global
`std::io::stdout()` / `stderr()`. This brings it in line with the existing
`host_files_write_file` / `host_files_append_file` shape and removes the
`&store` (immutable) vs `store.data_mut()` (mutable) borrow conflict.

Bodies change mechanically: `result.write_all(&data)` →
`result.write_all(data)`, `std::fs::write(path, &data)` →
`std::fs::write(path, data)`. No `to_vec()` / `into_owned()` introduced.

### 3.3 Unit tests

Three call sites in unit tests pass owned `Vec<u8>` and become slice
literals (line numbers as of HEAD `2b02437`):

| File | Line | Current | New |
|---|---|---|---|
| `src/plugin/host/io.rs` | 40 | `b"hi".to_vec()` | `b"hi"` |
| `src/plugin/host/files.rs` | 327 | `b"hello".to_vec()` | `b"hello"` |
| `src/plugin/host/files.rs` | 328 | `b" world".to_vec()` | `b" world"` |

`src/plugin/host/files.rs:238` (`let payload = b"hello world".to_vec();`)
is in `host_files_read_file_roundtrip` and remains a `Vec<u8>` because
`host_files_read_file` *returns* `Result<Vec<u8>, ErrorCode>` (return-side
allocation, out of scope here).

Example:

```rust
// Before
let result = host_io_write(&mut ctx, IoStream::Stdout, b"hi".to_vec());

// After
let result = host_io_write(&ctx, IoStream::Stdout, b"hi");
```

## 4. Verification

### 4.1 dhat acceptance gate

§4.1's rollout established the per-borrow unit savings:
**−1,000 blocks per `--exec-loop 1000`** for each canonical-ABI argument
converted from owned to `WasmStr` borrow. The same delta is expected for
`WasmList<u8>::as_le_slice` because the lift currently allocates one `Vec<u8>`
per crossing for the host's typed-func `Vec<u8>` parameter, and the borrow
codepath skips that allocation entirely.

Three new dhat smokes will be added to `src/bin/yosh-dhat.rs`, each running
1,000 host-import crossings of the corresponding write fn with a small (e.g.,
8-byte) payload to keep allocation noise dominated by the per-crossing cost
rather than payload size:

| Smoke name | Crossing measured | Gate (vs HEAD baseline before this rollout) |
|---|---|---|
| `noop_io_write_borrow` | `host_io_write` × 1000 | ≤ −1,000 blocks |
| `noop_files_write_file_borrow` | `host_files_write_file` × 1000 | ≤ −1,000 blocks |
| `noop_files_append_file_borrow` | `host_files_append_file` × 1000 | ≤ −1,000 blocks |

If a smoke misses by more than 10% (e.g., observed savings &lt; 900 blocks), pause
the rollout and investigate before proceeding.

### 4.2 Regression check

`cargo test --features test-helpers` must pass with no count regression
(currently 2,177 / 2,177 at HEAD `2b02437`).

### 4.3 Bench noise band

`plugin_exec_*` Criterion benches (3-run median-of-medians) must stay within
the §4.1-rollout noise band of ±5% vs the HEAD baseline. The `burst_var` bench
is unaffected by this change (it exercises `variables::get`, not write paths)
but is included as a stability sentinel.

## 5. Risks

- **`as_le_slice` alignment:** trivially satisfied for `u8`; the
  `raw_wasm_list_accessors!` macro in wasmtime 27 specializes for
  `i8 i16 i32 i64 u8 u16 u32 u64`, and the body's `assert!(head.is_empty() && tail.is_empty())`
  cannot fire for `u8` (size 1, alignment 1).
- **Borrow lifetime:** `as_le_slice(&store)` returns a slice tied to the
  `&store` borrow. The slice is consumed inside the closure (passed to the
  host fn which finishes before the closure returns), so no lifetime escape.
- **Wasm linear memory mutation during host call:** wasmtime guarantees the
  host call holds the wasm instance's lock; the guest cannot observe or
  mutate linear memory between `as_le_slice` and the host fn's return on the
  same store. (Same guarantee §4.1 relies on for `WasmStr::to_str`.)
- **`host_io_write` signature change ripple:** any out-of-tree caller passing
  `&mut HostContext` would need to update to `&HostContext`. No such callers
  exist — `host_io_write` is `pub` only within the `src/plugin/host/io.rs`
  module path and is invoked exclusively from `linker.rs` closures.

## 6. Out-of-scope follow-ups

- `commands::exec` argv borrow (`Vec<String>` → `WasmList<…>`?). This needs
  its own spike: `WasmList<WasmStr>` is not a wasmtime 27 type; the canonical
  way to borrow `list<string>` may require iterating the list and calling
  `to_str` on each element, which retains a per-element conversion but skips
  the outer `Vec<String>` allocation. Defer until a spike confirms the API
  shape and unit savings.
- `Vec<u8>` *return* paths (e.g., `files::read-file`) — the host *produces*
  the bytes, so the alloc is unavoidable on the lower side; deferred unless a
  future wasmtime API exposes a write-into-linear-memory shape.

## 7. Implementation references

- `src/plugin/linker.rs` lines ~147–235 (io and files write closures)
- `src/plugin/host/io.rs` (`host_io_write`, `deny_io_write`)
- `src/plugin/host/files.rs` (`host_files_write_file`, `host_files_append_file`,
  and their deny pairs)
- `src/bin/yosh-dhat.rs` (add three smoke entries)
- §4.1 commits for pattern reference: `7844e15`, `bd5139b`, `1461d1f`, `2262c7c`

## 8. Success criteria

1. All three new dhat smokes meet `≤ −1,000 blocks vs baseline` per
   `--exec-loop 1000`.
2. `cargo test --features test-helpers` passes with no count change.
3. `plugin_exec_*` Criterion benches within ±5% of HEAD baseline.
4. No new `unsafe` introduced; no `to_vec()` / `into_owned()` introduced in
   any of the three converted closures or host fns.
