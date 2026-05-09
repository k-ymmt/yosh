# Plugin perf §4.1 follow-up: Borrow `Vec<u8>` data — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `Vec<u8>` data parameters in three host imports (`io.write`,
`files.write-file`, `files.append-file`) with `wasmtime::component::WasmList<u8>`,
reading bytes via zero-copy `as_le_slice(&store)` to eliminate one host-side
allocation per crossing.

**Architecture:** WasmStr-borrow pattern from §4.1 applied to a list-typed
parameter. Each `func_wrap` closure changes from `Vec<u8>` to `WasmList<u8>`;
the closure body extracts `&[u8]` via `data.as_le_slice(&store)` and passes it
to a host fn whose signature drops from `Vec<u8>` to `&[u8]`. `host_io_write`
also drops from `&mut HostContext` to `&HostContext` (its body only calls
`ensure_bound`, no `ShellEnv` access) so the immutable `&store` borrow is
compatible with the host call. The `perf_plugin` test fixture gains three new
no-op commands that each isolate one host-import crossing for dhat
measurement.

**Tech Stack:** Rust 2024, wasmtime 27 component model, `cargo component`
(wasm32-wasip2), dhat-rs heap profiler, Criterion.

**Spec:** `docs/superpowers/specs/2026-05-09-plugin-data-borrow-design.md`

---

## File Map

**Created:**
- (none — only edits)

**Modified:**
- `tests/plugins/perf_plugin/src/lib.rs` — add three commands to `commands()` and `exec()`.
- `src/plugin/linker.rs` — change six `func_wrap` closures (3 host + 3 deny) to take `WasmList<u8>` instead of `Vec<u8>`.
- `src/plugin/host/io.rs` — update `host_io_write` and `deny_io_write` signatures (`&mut HostContext` → `&HostContext`, `Vec<u8>` → `&[u8]`); update one unit test (line 40).
- `src/plugin/host/files.rs` — update `host_files_write_file`, `host_files_append_file`, and their deny pairs (`Vec<u8>` → `&[u8]`); update two unit-test lines (327, 328). Line 238 (`host_files_read_file_roundtrip`) is unrelated — its `Vec<u8>` is a return-value comparison and stays.
- `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` — append Appendix C with measured savings.
- `TODO.md` — remove the `Vec<u8>` data borrow entry.

**Untouched (verify-only):**
- `crates/yosh-plugin-api/wit/yosh-plugin.wit` — no WIT changes (the host signature change is purely Rust-side).
- `crates/yosh-plugin-sdk/src/lib.rs` — SDK helpers (`write_bytes`, `write_file`, `append_file`) already accept `&[u8]`; no change needed.
- On-disk plugin binaries (`*.cwasm`) — no change in the wasm-side ABI.

---

## Task 1: Add three measurement commands to `perf_plugin`

**Files:**
- Modify: `tests/plugins/perf_plugin/src/lib.rs`

**Rationale:** Each command isolates exactly one host-import crossing through
the target codepath, so `yosh-dhat --exec-loop 1000 <cmd>` measures the
allocation delta for that single crossing per iteration. Targets used:
- `IoStream::Stderr` for `noop_io_write` — bench redirects stderr to
  `/dev/null` so terminal output is suppressed; the host fn always writes
  regardless of data length.
- `/dev/null` for the file-write smokes — on Unix, `std::fs::write("/dev/null", …)`
  and `OpenOptions::new().create(true).append(true).open("/dev/null")` both
  succeed without producing on-disk artifacts.

The 1-byte payload `b"x"` keeps wasm-side allocation noise constant across
runs (an empty list might trip a wasmtime fast-path that skips the
canonical-ABI lift entirely, masking the savings we want to measure).

- [ ] **Step 1: Confirm SDK helper signatures match what we want to call**

Run:

```bash
grep -n "pub fn write_bytes\|pub fn write_file\|pub fn append_file" crates/yosh-plugin-sdk/src/lib.rs
```

Expected:

```
276:pub fn write_bytes(stream: IoStream, data: &[u8]) -> Result<(), ErrorCode> {
384:pub fn write_file(path: &str, data: &[u8]) -> Result<(), ErrorCode> {
413:pub fn append_file(path: &str, data: &[u8]) -> Result<(), ErrorCode> {
```

All three already accept `&[u8]`. No SDK change needed.

- [ ] **Step 2: Read the current `perf_plugin` source to know where to insert**

Run:

```bash
cat tests/plugins/perf_plugin/src/lib.rs
```

Expected: a `PerfPlugin` struct with `commands()`, `required_capabilities()`,
`implemented_hooks()`, and `exec()` methods. The `commands()` array currently
lists six entries (`noop_cmd`, `noop_var`, `burst_var`, `noop_var_set`,
`noop_files_read`, `noop_files_remove`). Note the `use` line for
`yosh_plugin_sdk` — it imports specific helpers.

- [ ] **Step 3: Add the three new commands**

Modify `tests/plugins/perf_plugin/src/lib.rs`:

Update the `use` import line to add `IoStream`, `append_file`, `write_bytes`,
and `write_file`:

```rust
use yosh_plugin_sdk::{
    Capability, HookName, IoStream, Plugin, append_file, export, get_var,
    read_file, remove_file, set_var, write_bytes, write_file,
};
```

Add three entries to the `commands()` slice — the new array should read:

```rust
fn commands(&self) -> &[&'static str] {
    &[
        "noop_cmd",
        "noop_var",
        "burst_var",
        "noop_var_set",
        "noop_files_read",
        "noop_files_remove",
        "noop_io_write",
        "noop_files_write_file",
        "noop_files_append_file",
    ]
}
```

Add the matching arms inside `exec()`'s `match command { … }` block, after
the existing `"noop_files_remove"` arm:

```rust
"noop_io_write" => {
    let _ = write_bytes(IoStream::Stderr, b"x");
    0
}
"noop_files_write_file" => {
    let _ = write_file("/dev/null", b"x");
    0
}
"noop_files_append_file" => {
    let _ = append_file("/dev/null", b"x");
    0
}
```

`Capability::FilesWrite` is already in `required_capabilities()` (line 146 at
HEAD `2b02437`); `Capability::FilesRead`, `VariablesRead`, `VariablesWrite`,
and the hook caps are already declared. Confirm `Capability::Io` is needed:
the §4.1 plugin did not declare it because no `io` host import was used.

Add `Capability::Io` to the existing `required_capabilities()` slice. The
new array reads:

```rust
fn required_capabilities(&self) -> &[Capability] {
    &[
        Capability::VariablesRead,
        Capability::VariablesWrite,
        Capability::FilesRead,
        Capability::FilesWrite,
        Capability::Io,
        Capability::HookPrePrompt,
        Capability::HookPreExec,
        Capability::HookPostExec,
    ]
}
```

- [ ] **Step 4: Rebuild the wasm fixture**

Run:

```bash
cargo component build -p perf_plugin --target wasm32-wasip2 --release 2>&1 | tail -5
```

Expected: `Finished release [optimized] target(s) in <N>s` and an
updated `target/wasm32-wasip2/release/perf_plugin.wasm`. If the build fails
with `unresolved import yosh_plugin_sdk::IoStream` or similar, check the
`use` line — the missing item is the diagnostic.

- [ ] **Step 5: Verify the wasm rebuilt cleanly**

Run:

```bash
ls -la target/wasm32-wasip2/release/perf_plugin.wasm
```

Expected: file exists, mtime within the last minute.

- [ ] **Step 6: Smoke-test that the new commands dispatch through the linker**

The plugin loader must accept the additional capability declaration without
errors. The cheapest check is the capability-allowlist test:

```bash
cargo test --features test-helpers --test plugin -- t01 2>&1 | tail -10
```

Expected: `test t01_capability_allowlist_applied_to_linker ... ok`. This
confirms the rebuilt wasm still loads with the augmented capability set.
If it fails with "denied capability Io", the test fixture's allowlist
needs updating — see `tests/plugins/perf_plugin.toml` or
`tests/plugin.rs::load_plugin_with_caps` to grant `CAP_IO`. (Inspect the
failure output before changing anything; the existing perf_plugin manifest
may already grant it.)

- [ ] **Step 7: Commit**

```bash
git add tests/plugins/perf_plugin/src/lib.rs target/wasm32-wasip2/release/perf_plugin.wasm
git commit -m "$(cat <<'EOF'
test(perf_plugin): add §4.1 follow-up Vec<u8> measurement commands

Three new commands isolate one host-import crossing each, for use
with yosh-dhat --exec-loop:
  - noop_io_write:           io.write(IoStream::Stderr, b"x")
  - noop_files_write_file:   files.write-file("/dev/null", b"x")
  - noop_files_append_file:  files.append-file("/dev/null", b"x")

These will measure the per-arg savings of the WasmList<u8>::as_le_slice
borrow rollout (next commits).

Spec: docs/superpowers/specs/2026-05-09-plugin-data-borrow-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

If `target/wasm32-wasip2/release/perf_plugin.wasm` is `.gitignore`d (it may
be — many wasm artifacts are), drop that path from the `git add` line and
let the build step regenerate it on demand. Verify after the commit:

```bash
git log -1 --stat
```

Expected: at least `tests/plugins/perf_plugin/src/lib.rs` listed in the
diffstat.

---

## Task 2: Capture baseline dhat block counts

**Files:** none modified (`target/perf/` and `/tmp/yosh-perf-home/` are gitignored / out-of-tree)

**Rationale:** §4.1 established `−1,000 blocks per --exec-loop 1000` as the
per-borrow-arg unit savings. Before any host-side change, capture the
current "blocks" total for each of the three new smokes so the rollout
delta is unambiguous.

**Methodology (matches §4.1 task 3 exactly):**
- Use `HOME=/tmp/yosh-perf-home` to isolate the dhat run from the user's
  real `~/.config/yosh/plugins.lock`. **Never modify the user's actual
  config.** The isolated home is set up once in Step 2 below.
- Extract the metric from the **stderr `dhat: Total: <N> bytes in <M> blocks`**
  summary line that `dhat::Profiler` prints on Drop. Do NOT parse
  `dhat-heap.json`'s `"tb"` field — that field is per-allocation-site and
  summing it gives a different number than the canonical "blocks" total.
- Save the per-run `dhat-heap.json` artifacts to `target/perf/` for later
  cross-checking; baseline summary goes to `target/perf/rollout-baseline.txt`.

- [ ] **Step 1: Build the dhat-instrumented binary**

```bash
cargo build --profile profiling --features dhat-heap --bin yosh-dhat 2>&1 | tail -3
```

Expected: `Finished profiling profile [optimized + debuginfo]` and
`./target/profiling/yosh-dhat` exists. May take 1–2 minutes — use
`timeout: 300000` (5 minutes) on the bash call.

If the build fails because the `dhat-heap` feature is missing from
`Cargo.toml` `[features]`, BLOCKED — report back. (§4.1 introduced this
feature; it should be present at HEAD `f1c9b52`.)

- [ ] **Step 2: Set up isolated `HOME` with a `plugins.lock` for `perf_plugin`**

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
    "io",
    "hooks:pre_prompt",
    "hooks:pre_exec",
    "hooks:post_exec",
]
EOF
cat /tmp/yosh-perf-home/.config/yosh/plugins.lock
```

Expected: file written with all 8 capabilities (notice `io` was added vs.
§4.1's set, because Task 1 added `Capability::Io` for `noop_io_write`).
The `name = "perf"` matches §4.1's convention; `path` resolves to the wasm
that Task 1 just rebuilt.

**DO NOT modify the user's `~/.config/yosh/plugins.lock`.** That file is
shared state; any change is a bug. The isolated `HOME=/tmp/yosh-perf-home`
is the only place we write plugin config.

- [ ] **Step 3: Run baseline for `noop_io_write`**

```bash
mkdir -p target/perf
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat --exec-loop 1000 noop_io_write 2>&1 | tail -5
mv dhat-heap.json target/perf/dhat-rollout-noop_io_write-baseline.json
```

Expected: the last 5 lines include
`dhat: Total: <bytes> bytes in <blocks> blocks` and
`dhat: The data has been saved to dhat-heap.json`. Record `<bytes>` and
`<blocks>` — the `<blocks>` integer is `B_io`. Plausible range: low to
mid thousands (§4.1's similar smokes were 3,451–6,451). If it's millions,
something is wrong (likely a non-isolated HOME picking up the user's
plugins.lock).

If the dhat run prints a plugin-loading error (`failed to load plugin
"perf"` etc.) instead of the dhat summary, the wasm path or capability
list is wrong — investigate before proceeding.

- [ ] **Step 4: Run baseline for `noop_files_write_file`**

```bash
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat --exec-loop 1000 noop_files_write_file 2>&1 | tail -5
mv dhat-heap.json target/perf/dhat-rollout-noop_files_write_file-baseline.json
```

Record the `<blocks>` integer as `B_wf`.

- [ ] **Step 5: Run baseline for `noop_files_append_file`**

```bash
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat --exec-loop 1000 noop_files_append_file 2>&1 | tail -5
mv dhat-heap.json target/perf/dhat-rollout-noop_files_append_file-baseline.json
```

Record as `B_af`.

- [ ] **Step 6: Save the baseline summary**

```bash
cat > target/perf/rollout-baseline.txt <<EOF
=== §4.1 follow-up Vec<u8> rollout dhat baselines (commit f1c9b52, before Tasks 3–5) ===
noop_io_write           bytes=<bytes_io>  blocks=<B_io>
noop_files_write_file   bytes=<bytes_wf>  blocks=<B_wf>
noop_files_append_file  bytes=<bytes_af>  blocks=<B_af>

Expected post-rollout deltas (per --exec-loop 1000):
  noop_io_write:           −1,000 blocks (single Vec<u8> arg → WasmList<u8>::as_le_slice borrow)
  noop_files_write_file:   −1,000 blocks (data Vec<u8>; path is already WasmStr from §4.1)
  noop_files_append_file:  −1,000 blocks (same as write_file)

Methodology: HOME=/tmp/yosh-perf-home isolation; metric = "dhat: Total: ... blocks" summary line.
EOF
cat target/perf/rollout-baseline.txt
```

Replace the `<…>` placeholders with the integers from Steps 3–5. (No
commit — `target/perf/` is gitignored scratch.)

---

## Task 3: Convert `io.write` to `WasmList<u8>` borrow

**Files:**
- Modify: `src/plugin/linker.rs:147-155`
- Modify: `src/plugin/host/io.rs` (function bodies and the test at line 40)

- [ ] **Step 1: Read the current closures and host fn for context**

Run:

```bash
sed -n '143,160p' src/plugin/linker.rs
cat src/plugin/host/io.rs
```

Expected: closure uses `|mut store, (target, data): (IoStream, Vec<u8>)|` and
calls `host_io_write(store.data_mut(), target, data)`. Host fn signature is
`pub fn host_io_write(ctx: &mut HostContext, target: IoStream, data: Vec<u8>) -> Result<(), ErrorCode>`.

- [ ] **Step 2: Update the host fn signatures and bodies in `src/plugin/host/io.rs`**

Replace the entire file content with:

```rust
//! `yosh:plugin/io` host import — write to host stdout/stderr.
//! Granted via CAP_IO.

use std::io::Write;

use super::super::generated::yosh::plugin::types::{ErrorCode, IoStream};
use super::HostContext;

pub fn host_io_write(
    ctx: &HostContext,
    target: IoStream,
    data: &[u8],
) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    let result = match target {
        IoStream::Stdout => std::io::stdout().write_all(data),
        IoStream::Stderr => std::io::stderr().write_all(data),
    };
    result.map_err(|_| ErrorCode::IoFailed)
}

pub fn deny_io_write(
    _ctx: &HostContext,
    _target: IoStream,
    _data: &[u8],
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

#[cfg(test)]
mod tests {
    //! Spot test for the metadata-contract via io_write.

    use super::super::test_helpers::null_env_ctx;
    use super::*;

    #[test]
    fn io_write_denied_when_env_null() {
        let ctx = null_env_ctx();
        let result = host_io_write(&ctx, IoStream::Stdout, b"hi");
        assert_eq!(result, Err(ErrorCode::Denied));
    }
}
```

Diff vs. before: `&mut HostContext` → `&HostContext`, `Vec<u8>` → `&[u8]`,
`&data` → `data`, `let mut ctx` → `let ctx`, `b"hi".to_vec()` → `b"hi"`.

- [ ] **Step 3: Update the linker.rs closures for `io.write`**

In `src/plugin/linker.rs`, locate the `io.write` block (currently lines
147–155). The block is wrapped in `if has(allowed, CAP_IO) { … } else { … }`.
Replace it with:

```rust
io.func_wrap("write", |store, (target, data): (IoStream, wasmtime::component::WasmList<u8>)| {
    let bytes = data.as_le_slice(&store);
    Ok((host_io_write(store.data(), target, bytes),))
})?;
```

…inside the `if has(allowed, CAP_IO)` branch, and:

```rust
io.func_wrap("write", |store, (target, data): (IoStream, wasmtime::component::WasmList<u8>)| {
    let bytes = data.as_le_slice(&store);
    Ok((deny_io_write(store.data(), target, bytes),))
})?;
```

…inside the `else` branch. Note: `mut store` is dropped (no longer needed
because we use `store.data()`, not `store.data_mut()`).

- [ ] **Step 4: Build and run the host io tests**

Run:

```bash
cargo build --features test-helpers 2>&1 | tail -10
```

Expected: clean build. If `wasmtime::component::WasmList` is unresolved,
add `use wasmtime::component::WasmList;` near the top of `linker.rs` (or
keep the fully-qualified path — the spec uses fully-qualified for clarity).

```bash
cargo test --features test-helpers --lib plugin::host::io 2>&1 | tail -10
```

Expected: `test plugin::host::io::tests::io_write_denied_when_env_null ... ok`.

- [ ] **Step 5: Build the dhat binary and re-measure**

Run (use `timeout: 300000` for the cargo build):

```bash
cargo build --profile profiling --features dhat-heap --bin yosh-dhat 2>&1 | tail -3
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat --exec-loop 1000 noop_io_write 2>&1 | tail -5
mv dhat-heap.json target/perf/dhat-rollout-noop_io_write-after.json
```

Read the `dhat: Total: <bytes> bytes in <blocks> blocks` line; the
`<blocks>` integer is `A_io`. The isolated `HOME=/tmp/yosh-perf-home` and
its `plugins.lock` from Task 2 Step 2 are still in place; do not modify
the user's real `~/.config/yosh/plugins.lock`.

The acceptance gate is `B_io − A_io ≥ 1000` (≥ −1,000 blocks vs the
baseline you recorded in `target/perf/rollout-baseline.txt`). If the
delta is less than 900 (more than 10% miss), pause and investigate before
continuing — common causes:
- Forgot to drop `mut` from `store.data_mut()` in one branch (the lift still
  allocates a `Vec<u8>` for the unconverted branch).
- `data.as_le_slice(&store)` not actually called (e.g., if you accidentally
  kept `Vec<u8>` and added a `let bytes = data.as_slice();` adapter).
- The wasm fixture wasn't rebuilt — re-run Task 1 Step 4.

- [ ] **Step 6: Run the full plugin integration test to catch regressions**

Run:

```bash
cargo test --features test-helpers --test plugin 2>&1 | tail -15
```

Expected: all tests pass (~30 tests in this binary). If a test that
exercises `io.write` fails (e.g., an `io_*` test in `tests/plugin.rs`),
investigate before continuing.

- [ ] **Step 7: Commit**

```bash
git add src/plugin/host/io.rs src/plugin/linker.rs
git commit -m "$(cat <<'EOF'
perf(plugin): borrow Vec<u8> in io.write via WasmList<u8>

§4.1 follow-up: replace Vec<u8> data parameter with
WasmList<u8>::as_le_slice for zero-copy access to wasm linear memory.
host_io_write drops to &HostContext (immutable) so the &store borrow
held by as_le_slice is compatible with the host call.

dhat --exec-loop 1000 noop_io_write: <B_io> -> <A_io> blocks (Δ=<B_io − A_io>)

Spec: docs/superpowers/specs/2026-05-09-plugin-data-borrow-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Substitute the actual numbers for `<B_io>`, `<A_io>`, and `<B_io − A_io>`.

---

## Task 4: Convert `files.write-file` to `WasmList<u8>` borrow

**Files:**
- Modify: `src/plugin/linker.rs` (the `write-file` closures, lines ~190–200 and the deny variant ~223–230)
- Modify: `src/plugin/host/files.rs:83-93` (host fn) and the related deny pair (around line 178–184)
- Modify: `src/plugin/host/files.rs` test fixture at line 327

- [ ] **Step 1: Read current state for the write-file path**

Run:

```bash
grep -n "write-file\|write_file" src/plugin/linker.rs
sed -n '83,93p' src/plugin/host/files.rs
sed -n '178,184p' src/plugin/host/files.rs
```

Expected: closures use `(WasmStr, Vec<u8>)`; host fn signatures use
`data: Vec<u8>`.

- [ ] **Step 2: Update `host_files_write_file` and `deny_files_write_file` signatures**

In `src/plugin/host/files.rs`, change `host_files_write_file` to:

```rust
pub fn host_files_write_file(
    ctx: &HostContext,
    path: &str,
    data: &[u8],
) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    std::fs::write(path, data).map_err(|_| ErrorCode::IoFailed)
}
```

And `deny_files_write_file`:

```rust
pub fn deny_files_write_file(
    _ctx: &HostContext,
    _path: &str,
    _data: &[u8],
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}
```

Diff: `Vec<u8>` → `&[u8]`, `&data` → `data` (in the `std::fs::write` call).

- [ ] **Step 3: Update the linker.rs closures for `files.write-file`**

In `src/plugin/linker.rs`, the `write-file` block has two variants (cap
allowed vs deny). Replace each with:

Allowed branch:

```rust
files.func_wrap(
    "write-file",
    |store, (path, data): (wasmtime::component::WasmStr, wasmtime::component::WasmList<u8>)| {
        let path_str = path.to_str(&store)?;
        let bytes = data.as_le_slice(&store);
        Ok((host_files_write_file(store.data(), &path_str, bytes),))
    },
)?;
```

Deny branch:

```rust
files.func_wrap(
    "write-file",
    |store, (path, data): (wasmtime::component::WasmStr, wasmtime::component::WasmList<u8>)| {
        let path_str = path.to_str(&store)?;
        let bytes = data.as_le_slice(&store);
        Ok((deny_files_write_file(store.data(), &path_str, bytes),))
    },
)?;
```

The only diff vs. before is the second tuple element type
(`Vec<u8>` → `wasmtime::component::WasmList<u8>`) and the new
`let bytes = data.as_le_slice(&store);` line, plus passing `bytes` instead
of `data` to the host fn.

- [ ] **Step 4: Update the test fixture line at `src/plugin/host/files.rs:327`**

Locate the test `host_files_append_file_appends` (around lines 318–331). The
body contains two consecutive calls:

```rust
host_files_write_file(&ctx, &p, b"hello".to_vec()).unwrap();    // line 327
host_files_append_file(&ctx, &p, b" world".to_vec()).unwrap();  // line 328
```

Change ONLY line 327 in this step:

```rust
// Before
host_files_write_file(&ctx, &p, b"hello".to_vec()).unwrap();
// After
host_files_write_file(&ctx, &p, b"hello").unwrap();
```

Leave line 328 (`host_files_append_file`) as `b" world".to_vec()` until
Task 5 — `host_files_append_file` still requires `Vec<u8>` until then, and
changing it now would break the build for Tasks 4's intermediate state.

Note: line 238 (`let payload = b"hello world".to_vec();` in
`host_files_read_file_roundtrip`) is unrelated to this rollout —
`host_files_read_file` *returns* `Result<Vec<u8>, ErrorCode>`, so the
comparison `assert_eq!(result, Ok(payload))` requires `payload: Vec<u8>`.
Do not touch it.

- [ ] **Step 5: Build and run the host files tests**

Run:

```bash
cargo build --features test-helpers 2>&1 | tail -10
cargo test --features test-helpers --lib plugin::host::files 2>&1 | tail -15
```

Expected: clean build, all tests pass. The `host_files_append_file_appends`
test at lines 318–331 still compiles after this task because Task 4 only
changed line 327's argument literal (now `&[u8]`, matching the new
`write_file` signature) and left line 328 alone (still `.to_vec()`,
matching `append_file`'s unchanged `Vec<u8>` signature). Both lines are
consistent with their respective host fn signatures at this intermediate
state.

- [ ] **Step 6: Re-measure dhat for write-file**

Run (use `timeout: 300000` for the cargo build):

```bash
cargo build --profile profiling --features dhat-heap --bin yosh-dhat 2>&1 | tail -3
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat --exec-loop 1000 noop_files_write_file 2>&1 | tail -5
mv dhat-heap.json target/perf/dhat-rollout-noop_files_write_file-after.json
```

Read the `dhat: Total: <bytes> bytes in <blocks> blocks` line; the
`<blocks>` integer is `A_wf`. Acceptance gate: `B_wf − A_wf ≥ 1000` (vs
the baseline in `target/perf/rollout-baseline.txt`).

- [ ] **Step 7: Run the full plugin integration test**

```bash
cargo test --features test-helpers --test plugin 2>&1 | tail -15
```

Expected: pass. The `tests/plugin.rs` binary uses fixtures that exercise
`files.write-file` via the SDK's `write_file` helper, so a regression here
is the fastest signal that the closure didn't survive the rewrite.

- [ ] **Step 8: Commit**

```bash
git add src/plugin/host/files.rs src/plugin/linker.rs
git commit -m "$(cat <<'EOF'
perf(plugin): borrow Vec<u8> in files.write-file via WasmList<u8>

§4.1 follow-up: same WasmList<u8>::as_le_slice pattern as the io.write
conversion. Closure now reads both path and data slices from &store
without allocation.

dhat --exec-loop 1000 noop_files_write_file: <B_wf> -> <A_wf>
blocks (Δ=<B_wf − A_wf>)

Spec: docs/superpowers/specs/2026-05-09-plugin-data-borrow-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Convert `files.append-file` to `WasmList<u8>` borrow

**Files:**
- Modify: `src/plugin/linker.rs` (the `append-file` closures)
- Modify: `src/plugin/host/files.rs:95-111` (host fn) and the deny pair (around line 186–192)
- Modify: `src/plugin/host/files.rs:328` (the second line of the `host_files_append_file_appends` test)

- [ ] **Step 1: Update `host_files_append_file` and `deny_files_append_file` signatures**

In `src/plugin/host/files.rs`:

```rust
pub fn host_files_append_file(
    ctx: &HostContext,
    path: &str,
    data: &[u8],
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
    f.write_all(data).map_err(|_| ErrorCode::IoFailed)
}
```

And the deny pair:

```rust
pub fn deny_files_append_file(
    _ctx: &HostContext,
    _path: &str,
    _data: &[u8],
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}
```

Diff: `Vec<u8>` → `&[u8]`, `&data` → `data` in `f.write_all`.

- [ ] **Step 2: Update the linker.rs closures for `files.append-file`**

Allowed branch:

```rust
files.func_wrap(
    "append-file",
    |store, (path, data): (wasmtime::component::WasmStr, wasmtime::component::WasmList<u8>)| {
        let path_str = path.to_str(&store)?;
        let bytes = data.as_le_slice(&store);
        Ok((host_files_append_file(store.data(), &path_str, bytes),))
    },
)?;
```

Deny branch:

```rust
files.func_wrap(
    "append-file",
    |store, (path, data): (wasmtime::component::WasmStr, wasmtime::component::WasmList<u8>)| {
        let path_str = path.to_str(&store)?;
        let bytes = data.as_le_slice(&store);
        Ok((deny_files_append_file(store.data(), &path_str, bytes),))
    },
)?;
```

- [ ] **Step 3: Update the test fixture line that was deferred in Task 4**

In `src/plugin/host/files.rs` around line 328:

```rust
// Before
host_files_append_file(&ctx, &p, b" world".to_vec()).unwrap();
// After
host_files_append_file(&ctx, &p, b" world").unwrap();
```

- [ ] **Step 4: Build and test**

```bash
cargo build --features test-helpers 2>&1 | tail -10
cargo test --features test-helpers --lib plugin::host::files 2>&1 | tail -15
```

Expected: clean build, all `plugin::host::files` tests pass.

- [ ] **Step 5: Re-measure dhat for append-file**

Run (use `timeout: 300000` for the cargo build):

```bash
cargo build --profile profiling --features dhat-heap --bin yosh-dhat 2>&1 | tail -3
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat --exec-loop 1000 noop_files_append_file 2>&1 | tail -5
mv dhat-heap.json target/perf/dhat-rollout-noop_files_append_file-after.json
```

Read the `dhat: Total: <bytes> bytes in <blocks> blocks` line; the
`<blocks>` integer is `A_af`. Acceptance gate: `B_af − A_af ≥ 1000`.

- [ ] **Step 6: Full plugin integration test**

```bash
cargo test --features test-helpers --test plugin 2>&1 | tail -15
```

Expected: pass.

- [ ] **Step 7: Commit**

```bash
git add src/plugin/host/files.rs src/plugin/linker.rs
git commit -m "$(cat <<'EOF'
perf(plugin): borrow Vec<u8> in files.append-file via WasmList<u8>

§4.1 follow-up: completes the three-function Vec<u8> borrow rollout.
Same WasmList<u8>::as_le_slice pattern as io.write and write-file.

dhat --exec-loop 1000 noop_files_append_file: <B_af> -> <A_af>
blocks (Δ=<B_af − A_af>)

Spec: docs/superpowers/specs/2026-05-09-plugin-data-borrow-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Final regression sweep, perf-report Appendix C, TODO.md cleanup

**Files:**
- Modify: `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` (append Appendix C)
- Modify: `TODO.md` (remove the `Vec<u8>` data borrow item)

- [ ] **Step 1: Run the full test suite**

```bash
cargo test --features test-helpers 2>&1 | tail -20
```

Expected: count matches the §4.1 baseline of **2,177 / 2,177 pass** (or
higher if anything was added since). If the count drops, identify the
regression before declaring success.

- [ ] **Step 2: Run the relevant Criterion benches as a noise sentinel**

```bash
cargo bench --bench plugin_exec_bench -- burst_var 2>&1 | tail -20
```

Expected: median within ±5% of the §4.1 baseline of ~1,205 ns. The
`burst_var` bench exercises `variables::get`, which this rollout does not
touch; any movement is codegen noise. If the bench is named differently
in this codebase (`grep -l 'burst_var\|plugin_exec' benches/`), substitute
the actual name. If the bench harness is missing entirely, skip this step
and note it in the perf-report append.

- [ ] **Step 3: Append Appendix C to the perf report**

In `docs/superpowers/specs/2026-05-08-plugin-perf-report.md`, append at the
end of the file:

```markdown
## Appendix C: §4.1 Follow-up Vec<u8> Borrow Rollout — Result

**Date:** 2026-05-09
**Spec:** `docs/superpowers/specs/2026-05-09-plugin-data-borrow-design.md`
**Plan:** `docs/superpowers/plans/2026-05-09-plugin-data-borrow.md`
**Commits:** see `git log` between this entry and Appendix B's HEAD

### Coverage

Three host imports converted from `Vec<u8>` to
`wasmtime::component::WasmList<u8>::as_le_slice` borrow:

- `io.write` (closure + `host_io_write` signature dropped to `&HostContext`)
- `files.write-file`
- `files.append-file`

The deny counterparts of each were updated to the same `&[u8]` shape for
type consistency.

### Decisive cross-check (dhat `--exec-loop 1000`)

| Smoke | Baseline blocks | After blocks | Δ | Target | Verdict |
|---|---|---|---|---|---|
| `noop_io_write` | <B_io> | <A_io> | **<B_io − A_io>** | −1,000 | <verdict> |
| `noop_files_write_file` | <B_wf> | <A_wf> | **<B_wf − A_wf>** | −1,000 | <verdict> |
| `noop_files_append_file` | <B_af> | <A_af> | **<B_af − A_af>** | −1,000 | <verdict> |

`<verdict>` is ✅ exact (Δ ≥ −1,000), ✅ over (Δ much larger), or ❌ if the
gate was missed. Substitute the actual numbers from Tasks 2–5.

### Regression check

- `cargo test --features test-helpers`: **<X> / <Y> pass** (no count change vs HEAD before this rollout).
- `plugin_exec_burst_var` Criterion median: <N> ns (baseline ~1,205 ns from §4.1; within ±5%).

### Remaining Vec<…> follow-up

`commands::exec` argv (`Vec<String>` → `list<string>`) is a separate
codepath. The wasmtime 27 borrow shape for `list<string>` is not symmetric
with `WasmList<u8>` (no `as_le_slice` for variable-width elements), so a
distinct spike is required before authoring a rollout spec. Tracked in
TODO.md "Plugin perf: borrow `commands::exec` argv".
```

Substitute the placeholders (`<B_io>`, `<A_io>`, `<verdict>`, `<X>`, `<Y>`,
`<N>`) with the actual values you recorded.

- [ ] **Step 4: Remove the `Vec<u8>` data borrow entry from TODO.md**

In `TODO.md`, locate the line that starts with:

```
- [ ] Plugin perf: borrow `Vec<u8>` data parameters in host imports
```

(currently the first sub-bullet under "Future: Plugin System Enhancements"
that mentions `Vec<u8>`). Delete the entire bullet (it spans one wrapped
paragraph). Per `CLAUDE.md`, completed items are deleted, not crossed off.

The `commands::exec` argv entry stays — it remains out-of-scope.

- [ ] **Step 5: Verify the docs and TODO edits compile cleanly**

```bash
git diff docs/superpowers/specs/2026-05-08-plugin-perf-report.md TODO.md
```

Expected: Appendix C addition + one bullet deletion in TODO.md.

- [ ] **Step 6: Final commit**

```bash
git add docs/superpowers/specs/2026-05-08-plugin-perf-report.md TODO.md
git commit -m "$(cat <<'EOF'
docs(plugin-perf): record §4.1 follow-up Vec<u8> rollout result

Three host imports (io.write, files.write-file, files.append-file)
converted to WasmList<u8>::as_le_slice borrow. dhat verdict:
- noop_io_write:           Δ=<B_io − A_io> blocks (target −1,000)
- noop_files_write_file:   Δ=<B_wf − A_wf> blocks (target −1,000)
- noop_files_append_file:  Δ=<B_af − A_af> blocks (target −1,000)

Test suite: <X> / <Y> pass (no regression vs HEAD before rollout).

Removes the Vec<u8> data follow-up from TODO.md. The commands::exec
argv (Vec<String>) item stays — it needs its own spike (no
WasmList::as_le_slice equivalent for list<string> in wasmtime 27).

Spec: docs/superpowers/specs/2026-05-09-plugin-data-borrow-design.md
Plan: docs/superpowers/plans/2026-05-09-plugin-data-borrow.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 7: Verify clean state**

```bash
git status
git log --oneline -7
```

Expected: working tree clean; the last six commits are the spec, the
perf_plugin commands, and the four implementation/regression commits in
order.

---

## Done

Success criteria from the spec:

1. ✅ All three new dhat smokes meet `≤ −1,000 blocks vs baseline` per `--exec-loop 1000`.
2. ✅ `cargo test --features test-helpers` passes with no count change.
3. ✅ `plugin_exec_*` Criterion benches within ±5% of HEAD baseline.
4. ✅ No new `unsafe`; no `to_vec()` / `into_owned()` introduced.

If any criterion failed, `git revert` the implementation commits and
investigate before declaring done.
