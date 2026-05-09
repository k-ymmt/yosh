# Plugin Real Linker Cache by Capability Mask — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cache `Linker<HostContext>` by negotiated `effective_capabilities` so multi-plugin sessions sharing a cap mask only build the linker once. Closes the gap from fix#1's verified −33.3% drop on `LinkerInstance<T>::insert` blocks toward the perf-report §5.1 ≥50% target.

**Architecture:** Replace `PluginManager::scratch_linker: Option<Linker<HostContext>>` (the fix#1 field at `src/plugin/mod.rs:171`) with a unified `linker_cache: HashMap<u32, Linker<HostContext>>` keyed on the cap bitfield. Both the metadata-probe path (always `CAP_ALL`) and per-plugin real linkers share this single cache. A `get_or_build_linker(&mut self, caps, path) -> Result<&Linker, String>` helper handles lookup-or-build with no cache insert on builder failure.

**Tech Stack:** Rust 2024, wasmtime 27 (`Linker<T>::instantiate_pre(&self, &Component)`), `std::collections::HashMap`, existing dhat-heap profiling pipeline (`yosh-dhat` + `scripts/perf/dhat_filter_frame.py`).

**Spec:** `docs/superpowers/specs/2026-05-09-plugin-real-linker-cache-design.md`

**Spec deviation noted upfront:** spec §9.2 places new tests in `src/plugin/mod.rs::tests`, but the wasm-artefact loader (`ensure_built`) lives in `tests/plugin.rs` and lib unit tests cannot reach it. This plan places the two new tests in `tests/plugin.rs` instead, exposing `linker_cache.len()` via a `test_helpers::linker_cache_len` accessor (already the established pattern — see `test_helpers::env_pointer_is_null_in_store` at `src/plugin/mod.rs:902`).

---

## Task 1: Failing integration tests for cache reuse / separation

**Files:**
- Modify: `tests/plugin.rs` (append two new tests near other multi-plugin tests)
- Modify: `src/plugin/mod.rs` (add a stub `test_helpers::linker_cache_len` that returns 0 so the tests compile)

This task captures the desired behavior in tests before any production change. The tests will compile (after the stub is added) and **fail** because no cache exists yet.

- [ ] **Step 1.1: Add a stub accessor `test_helpers::linker_cache_len`**

In `src/plugin/mod.rs`, inside the existing `pub mod test_helpers` block (right after `env_pointer_is_null_in_store`, around line 905), add:

```rust
    /// Number of `Linker<HostContext>` entries currently cached on the
    /// manager. Used by §4.2 fix#2 cache-reuse / cache-separation tests.
    pub fn linker_cache_len(_manager: &PluginManager) -> usize {
        0 // STUB — replaced in Task 2
    }
```

The leading underscore avoids an `unused_variables` warning while the stub returns 0.

- [ ] **Step 1.2: Add the failing integration tests**

In `tests/plugin.rs`, append (after the last existing test):

```rust
#[test]
fn linker_cache_reuses_entry_for_same_mask() {
    // Two loads with identical caps must share one real-linker cache
    // entry. With the metadata-probe scratch entry (CAP_ALL) plus one
    // shared real-mask entry, the total is 2.
    let wasm = test_plugin_wasm();
    let mut env = ShellEnv::new();
    let mut mgr = PluginManager::new();
    let caps = yosh_plugin_api::CAP_ALL;

    test_helpers::load_plugin_with_caps(&mut mgr, &wasm, &mut env, caps, &[])
        .expect("first load");
    test_helpers::load_plugin_with_caps(&mut mgr, &wasm, &mut env, caps, &[])
        .expect("second load");

    assert_eq!(
        test_helpers::linker_cache_len(&mgr),
        2,
        "expected 2 entries (CAP_ALL scratch + shared real mask), got {}",
        test_helpers::linker_cache_len(&mgr)
    );
}

#[test]
fn linker_cache_separates_entries_for_distinct_masks() {
    // Two loads with different cap subsets must produce two real-linker
    // cache entries (plus one CAP_ALL scratch entry) for a total of 3.
    let wasm = test_plugin_wasm();
    let mut env = ShellEnv::new();
    let mut mgr = PluginManager::new();
    let caps_a = yosh_plugin_api::CAP_VARIABLES_READ;
    let caps_b = yosh_plugin_api::CAP_VARIABLES_READ | yosh_plugin_api::CAP_FILESYSTEM;

    test_helpers::load_plugin_with_caps(&mut mgr, &wasm, &mut env, caps_a, &[])
        .expect("load a");
    test_helpers::load_plugin_with_caps(&mut mgr, &wasm, &mut env, caps_b, &[])
        .expect("load b");

    assert_eq!(
        test_helpers::linker_cache_len(&mgr),
        3,
        "expected 3 entries (CAP_ALL scratch + 2 distinct real masks), got {}",
        test_helpers::linker_cache_len(&mgr)
    );
}
```

- [ ] **Step 1.3: Verify the new tests compile and FAIL as expected**

Run:

```sh
cargo component build -p test_plugin --target wasm32-wasip2 --release
cargo test --features test-helpers --test plugin linker_cache_ -- --nocapture 2>&1 | tail -40
```

Expected: both tests run; both **FAIL** with `expected 2 ... got 0` and `expected 3 ... got 0`. (The stub returns 0; this confirms the tests are wired correctly and there is something to fix.)

- [ ] **Step 1.4: Commit the failing tests**

```sh
git add src/plugin/mod.rs tests/plugin.rs
git commit -m "$(cat <<'EOF'
test(plugin): add §4.2 fix#2 linker-cache reuse/separation tests (red)

Capture the desired cache-reuse property before the implementation.
Both tests compile against a stub `test_helpers::linker_cache_len`
returning 0 and currently fail (expected 2 / 3, got 0). Stub will
be replaced by the real `linker_cache.len()` in the next task.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Implement `linker_cache` and switch both call sites

**Files:**
- Modify: `src/plugin/mod.rs` — replace `scratch_linker` field, add helper, update both `build_linker` call sites, fix the `linker_cache_len` accessor.

This task introduces the production change atomically (one compile-clean state from the field replacement to the call-site rewrite to the test-helper update).

- [ ] **Step 2.1: Replace the `scratch_linker` field with `linker_cache`**

In `src/plugin/mod.rs`, find the existing field (around line 163-171):

```rust
    /// Permissive (`CAP_ALL`) linker reused for the metadata probe step
    /// of every `load_one`. Built lazily on the first plugin load and
    /// then shared across subsequent loads — the metadata-contract
    /// (host imports return `Err(Denied)` on null env) makes the
    /// permissive linker safe to reuse regardless of the negotiated
    /// capability mask. Eliminates one full `Linker<HostContext>`
    /// rebuild per plugin after the first. See report §4.2 in
    /// `docs/superpowers/specs/2026-05-08-plugin-perf-report.md`.
    scratch_linker: Option<Linker<HostContext>>,
```

Replace with:

```rust
    /// Linkers keyed by negotiated `effective_capabilities`. Both the
    /// metadata-probe scratch linker (always `CAP_ALL`) and per-plugin
    /// real linkers share this cache; an entry is built lazily on first
    /// load that needs it. The Linker is plugin-independent: it depends
    /// only on the engine and the cap bitfield, so two plugins granted
    /// the same caps share one cached linker. See report §4.2 fix#2 and
    /// `docs/superpowers/specs/2026-05-09-plugin-real-linker-cache-design.md`.
    pub(super) linker_cache: std::collections::HashMap<u32, Linker<HostContext>>,
```

`pub(super)` lets the `test_helpers` accessor read `.len()` without `pub` leakage outside the `plugin` module.

- [ ] **Step 2.2: Update `PluginManager::new()` to initialize the new field**

Find the `PluginManager::new` constructor (around line 263-275, look for `plugins: Vec::new(),`). Replace the line:

```rust
            scratch_linker: None,
```

with:

```rust
            linker_cache: std::collections::HashMap::new(),
```

- [ ] **Step 2.3: Add the `get_or_build_linker` helper method on `PluginManager`**

Find a spot inside `impl PluginManager` near `load_one` (right before the `pub(super) fn load_one(...)` definition at line 328 is a good spot). Add:

```rust
    /// Look up a cached `Linker<HostContext>` for the given capability
    /// bitfield, or build and cache one. Returns a borrowed reference
    /// suitable for `Linker::instantiate_pre(&self, &Component)`. On
    /// `build_linker` failure the cache is not modified, so a
    /// subsequent load for the same caps retries from scratch.
    fn get_or_build_linker(
        &mut self,
        caps: u32,
        path: &Path,
    ) -> Result<&Linker<HostContext>, String> {
        if !self.linker_cache.contains_key(&caps) {
            let l = linker::build_linker(&self.engine, caps)
                .map_err(|e| format!("{}: linker build failed: {}", path.display(), e))?;
            self.linker_cache.insert(caps, l);
        }
        Ok(self
            .linker_cache
            .get(&caps)
            .expect("inserted on the line above if missing"))
    }
```

- [ ] **Step 2.4: Replace the scratch-linker call site (Step 5 in `load_one`)**

Find the current scratch path in `load_one` (around line 425-440):

```rust
        // 5. Build a permissive linker first so we can call `metadata` to
        //    learn the plugin's requested capabilities. The metadata
        //    contract (host imports return `Err(Denied)` on null env) makes
        //    this safe — even a permissive linker rejects host calls during
        //    `metadata`. The scratch linker is cached on `self` because it
        //    is plugin-independent (always `CAP_ALL`); subsequent loads
        //    reuse it, eliminating one full `Linker` rebuild per plugin.
        if self.scratch_linker.is_none() {
            let l = linker::build_linker(&self.engine, CAP_ALL)
                .map_err(|e| format!("{}: linker init failed: {}", path.display(), e))?;
            self.scratch_linker = Some(l);
        }
        let scratch_linker = self
            .scratch_linker
            .as_ref()
            .expect("scratch_linker initialized just above");
```

Replace with:

```rust
        // 5. Build a permissive linker first so we can call `metadata` to
        //    learn the plugin's requested capabilities. The metadata
        //    contract (host imports return `Err(Denied)` on null env) makes
        //    this safe — even a permissive linker rejects host calls during
        //    `metadata`. The linker is fetched from `linker_cache`
        //    (per-cap-mask cache); the `CAP_ALL` entry is shared across
        //    all metadata probes, and any plugin granted `CAP_ALL` shares
        //    this same cached linker for its real-instantiation step.
        let scratch_linker = self.get_or_build_linker(CAP_ALL, path)?;
```

- [ ] **Step 2.5: Replace the real-linker call site (Step 7 in `load_one`)**

Find (around line 482-486):

```rust
        // 7. Build the real linker with the negotiated capability mask,
        //    create a fresh store, instantiate, and call on_load under
        //    with_env so the plugin can use its granted host imports.
        let real_linker = linker::build_linker(&self.engine, effective_capabilities)
            .map_err(|e| format!("{}: linker build failed: {}", path.display(), e))?;
```

Replace with:

```rust
        // 7. Fetch the real linker from `linker_cache` (built lazily on
        //    first use of this cap mask), create a fresh store,
        //    instantiate, and call on_load under with_env so the plugin
        //    can use its granted host imports. Plugins sharing a cap
        //    mask reuse the same cached linker.
        let real_linker = self.get_or_build_linker(effective_capabilities, path)?;
```

- [ ] **Step 2.6: Update `test_helpers::linker_cache_len` to return the real length**

Find the stub added in Task 1.1 (in the `pub mod test_helpers` block):

```rust
    /// Number of `Linker<HostContext>` entries currently cached on the
    /// manager. Used by §4.2 fix#2 cache-reuse / cache-separation tests.
    pub fn linker_cache_len(_manager: &PluginManager) -> usize {
        0 // STUB — replaced in Task 2
    }
```

Replace with:

```rust
    /// Number of `Linker<HostContext>` entries currently cached on the
    /// manager. Used by §4.2 fix#2 cache-reuse / cache-separation tests.
    pub fn linker_cache_len(manager: &PluginManager) -> usize {
        manager.linker_cache.len()
    }
```

- [ ] **Step 2.7: Verify the workspace compiles**

```sh
cargo build 2>&1 | tail -10
```

Expected: `Finished \`dev\` profile [unoptimized + debuginfo] target(s)` with no errors. (One pre-existing clippy warning on `src/plugin/mod.rs:98` is unrelated — see TODO.md `cargo clippy --all-targets` entry.)

- [ ] **Step 2.8: Run the two new tests — they must PASS**

```sh
cargo test --features test-helpers --test plugin linker_cache_ -- --nocapture 2>&1 | tail -20
```

Expected: `test linker_cache_reuses_entry_for_same_mask ... ok` and `test linker_cache_separates_entries_for_distinct_masks ... ok`. `2 passed; 0 failed`.

- [ ] **Step 2.9: Run the full plugin integration suite — no regression**

```sh
cargo test --features test-helpers --test plugin 2>&1 | tail -10
```

Expected: all tests pass (count should match the pre-change count + 2 new tests).

- [ ] **Step 2.10: Run lib unit tests — no regression**

```sh
cargo test --lib 2>&1 | tail -10
```

Expected: all lib tests pass.

- [ ] **Step 2.11: Commit the implementation**

```sh
git add src/plugin/mod.rs
git commit -m "$(cat <<'EOF'
perf(plugin): cache real linker by capability mask (§4.2 fix#2)

Replace the scratch-linker `Option<Linker<HostContext>>` (added in
`70d78ec` for fix#1) with a unified `HashMap<u32, Linker<HostContext>>`
keyed on the negotiated `effective_capabilities`. Both the
metadata-probe path (always `CAP_ALL`) and per-plugin real linkers
share this single cache: same-mask plugins reuse one linker entry
instead of each rebuilding the full `IndexMap`-backed `NameMap`.

`get_or_build_linker(&mut self, caps, path) -> Result<&Linker, String>`
encapsulates the lookup-or-build pattern. On `build_linker` failure
the cache is not modified, so a subsequent load for the same caps
retries from scratch. `Linker::instantiate_pre(&self, &Component)`
takes the linker by reference, so cached entries are reused without
cloning.

Visibility: the field is `pub(super)` to let the
`test_helpers::linker_cache_len` accessor read `.len()` from the
adjacent integration tests; no production API changes.

Original task: docs/superpowers/specs/2026-05-08-plugin-perf-report.md
§4.2 fix candidate #2.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Performance verification (≥50% drop) and documentation

**Files:**
- Modify: `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` — add Appendix D
- Modify: `TODO.md` — delete the §4.2 follow-up entry

This task gates "done" on the §12 acceptance criterion ≥50% drop on `LinkerInstance<T>::insert` blocks. The numbers from the same-mask 3-plugin run go into Appendix D; the distinct-mask sanity run is recorded inline in the same appendix.

- [ ] **Step 3.1: Build profiling binaries**

```sh
cargo component build -p perf_plugin --target wasm32-wasip2 --release
cargo build --profile profiling --features dhat-heap --bin yosh --bin yosh-dhat
```

Expected: both finish without error. Verify the binary timestamp is newer than the previous build:

```sh
ls -la target/profiling/yosh-dhat
```

- [ ] **Step 3.2: Verify the same-mask staging from Appendix C is still in place**

The 3-plugin same-mask `plugins.lock` from the fix#1 verification (Appendix C reproducer) should already exist at `/tmp/yosh-perf-home/.config/yosh/plugins.lock` with three entries (`perf_a`, `perf_b`, `perf_c`) all pointing at the same wasm with identical caps. If absent, recreate:

```sh
mkdir -p /tmp/yosh-perf-home/.config/yosh target/perf
WASM_PATH="$(pwd)/target/wasm32-wasip2/release/perf_plugin.wasm"
cat > /tmp/yosh-perf-home/.config/yosh/plugins.lock <<EOF
[[plugin]]
name = "perf_a"
path = "${WASM_PATH}"
enabled = true
capabilities = ["variables:read", "hooks:pre_prompt", "hooks:pre_exec", "hooks:post_exec"]

[[plugin]]
name = "perf_b"
path = "${WASM_PATH}"
enabled = true
capabilities = ["variables:read", "hooks:pre_prompt", "hooks:pre_exec", "hooks:post_exec"]

[[plugin]]
name = "perf_c"
path = "${WASM_PATH}"
enabled = true
capabilities = ["variables:read", "hooks:pre_prompt", "hooks:pre_exec", "hooks:post_exec"]
EOF
echo "echo hi" > target/perf/echo-hi.sh
```

- [ ] **Step 3.3: Run W-P5 dhat 3-plugin same-mask, take steady state (run 3×, keep run 3)**

The first run after a fresh binary includes one-shot dyld / engine-init overhead that inflates totals; runs 2/3 are steady state.

```sh
for i in 1 2 3; do
  HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat target/perf/echo-hi.sh 2>&1 | grep "Total:"
done
mv dhat-heap.json target/perf/dhat-plugin-w5-3p-same-mask-postfix2.json
python3 scripts/perf/dhat_filter_frame.py \
  target/perf/dhat-plugin-w5-3p-same-mask-postfix2.json \
  "LinkerInstance<T>::insert" \
  | tee target/perf/dhat-plugin-w5-3p-same-mask-postfix2-linker.md
```

Expected: 3 runs print consistent `Total:` numbers (within a few bytes of each other). The filter output reports `Matched blocks ≤ 700` (target ≥50% drop vs fix#1's 1,396).

If `Matched blocks > 700`: stop, investigate (likely a cache-hit gap somewhere in `load_one`). Do not proceed to Step 3.4.

- [ ] **Step 3.4: Run W-P5 dhat 3-plugin distinct-mask sanity**

Stage a distinct-mask plugins.lock (overwriting the same-mask one is fine — the same-mask numbers are already saved):

```sh
WASM_PATH="$(pwd)/target/wasm32-wasip2/release/perf_plugin.wasm"
cat > /tmp/yosh-perf-home/.config/yosh/plugins.lock <<EOF
[[plugin]]
name = "perf_a"
path = "${WASM_PATH}"
enabled = true
capabilities = ["variables:read"]

[[plugin]]
name = "perf_b"
path = "${WASM_PATH}"
enabled = true
capabilities = ["variables:read", "hooks:pre_prompt"]

[[plugin]]
name = "perf_c"
path = "${WASM_PATH}"
enabled = true
capabilities = ["variables:read", "hooks:pre_prompt", "hooks:pre_exec"]
EOF

for i in 1 2 3; do
  HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat target/perf/echo-hi.sh 2>&1 | grep "Total:"
done
mv dhat-heap.json target/perf/dhat-plugin-w5-3p-distinct-mask-postfix2.json
python3 scripts/perf/dhat_filter_frame.py \
  target/perf/dhat-plugin-w5-3p-distinct-mask-postfix2.json \
  "LinkerInstance<T>::insert" \
  | tee target/perf/dhat-plugin-w5-3p-distinct-mask-postfix2-linker.md
```

Expected: `Matched blocks` ≈ 1,396 (same as fix#1 alone) ± 50 — confirms that distinct masks correctly produce distinct cache entries (no over-sharing).

- [ ] **Step 3.5: Restore the same-mask plugins.lock**

```sh
WASM_PATH="$(pwd)/target/wasm32-wasip2/release/perf_plugin.wasm"
cat > /tmp/yosh-perf-home/.config/yosh/plugins.lock <<EOF
[[plugin]]
name = "perf_a"
path = "${WASM_PATH}"
enabled = true
capabilities = ["variables:read", "hooks:pre_prompt", "hooks:pre_exec", "hooks:post_exec"]

[[plugin]]
name = "perf_b"
path = "${WASM_PATH}"
enabled = true
capabilities = ["variables:read", "hooks:pre_prompt", "hooks:pre_exec", "hooks:post_exec"]

[[plugin]]
name = "perf_c"
path = "${WASM_PATH}"
enabled = true
capabilities = ["variables:read", "hooks:pre_prompt", "hooks:pre_exec", "hooks:post_exec"]
EOF
```

- [ ] **Step 3.6: Add Appendix D to the perf report**

Open `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` and append (after Appendix C):

```markdown

## Appendix D: §4.2 Real Linker Cache Verification — Target Met

**Date:** 2026-05-09
**Implementation commit:** <fill in commit hash from Task 2.11>
**Verdict:** Real-linker caching by capability mask delivers **−<X>%** drop on `LinkerInstance<T>::insert` blocks at N=3 same-mask, meeting the §5.1 ≥50% target. The distinct-mask sanity reproduces the fix#1 33% ceiling, confirming the cache key correctly differentiates cap masks.

### Method

Same `/tmp/yosh-perf-home` 3-plugin staging as Appendix C. Two scenarios:
- **Same-mask:** all three plugins.lock entries grant `["variables:read", "hooks:pre_prompt", "hooks:pre_exec", "hooks:post_exec"]` (identical mask).
- **Distinct-mask:** three different cap subsets to verify the key correctly separates entries.

For each scenario: 3 warm-up runs, keep the steady-state dhat output, extract `LinkerInstance<T>::insert` matches via `scripts/perf/dhat_filter_frame.py`.

### Numbers (same-mask, 3 plugins)

| Metric | fix#1 only (Appendix C) | fix#1 + fix#2 | Δ vs fix#1 | % drop |
|--------|-------------------------|---------------|------------|--------|
| `LinkerInstance<T>::insert` bytes | 534,288 | <fill> | <fill> | <fill>% |
| `LinkerInstance<T>::insert` blocks | 1,396 | <fill> | <fill> | <fill>% |
| `LinkerInstance<T>::insert` matched sites | 632 | <fill> | <fill> | <fill>% |

### Numbers (distinct-mask, 3 plugins)

| Metric | fix#1 only (extrapolated) | fix#1 + fix#2 | Verdict |
|--------|---------------------------|---------------|---------|
| `LinkerInstance<T>::insert` blocks | ≈ 1,396 | <fill> | <within ±50 → key separates correctly> |

### Cumulative drop vs original baseline

Combining Appendix C and Appendix D:

| Stage | `LinkerInstance<T>::insert` blocks (3-plugin same-mask) | Δ vs original |
|-------|---------------------------------------------------------|---------------|
| Pre-fix baseline (Appendix C pre-fix) | 2,094 | — |
| Post fix#1 (Appendix C post-fix) | 1,396 | −33.3% |
| Post fix#2 (this appendix) | <fill> | <fill>% |

≥50% target met at <fill>% drop vs original baseline.

### Recommendation

§4.2 closed. Future improvements (fix#3 — eliminate the two-stage probe entirely) remain available if a workload emerges that benefits, but the ≥50% threshold from §5.1 is now satisfied.
```

Replace each `<fill>` with the actual numbers from Step 3.3 / 3.4 outputs. Compute percentages with two decimal places.

- [ ] **Step 3.7: Remove the §4.2 follow-up entry from TODO.md**

Find this line in `TODO.md` (one line, multi-clause; keep an eye on the closing sentence):

```markdown
- [ ] Plugin perf §4.2 follow-up: real linker caching by capability mask (`HashMap<u32, Linker<HostContext>>` keyed on negotiated `effective_capabilities`). Listed as fix candidate #2 in report §4.2; **indicated by measurement** (Appendix C, 2026-05-08) — scratch-linker caching alone delivers exactly the algorithmic 33.3% drop on `LinkerInstance::insert` at N=3, missing the ≥50% target. Real linker is built per-plugin (different caps), so caching needs interior mutability (`RwLock`/`OnceCell` per-mask). Most plugins share the same caps mask in practice, so the win could be substantial for multi-plugin sessions.
```

Delete the entire line. Per CLAUDE.md "Delete completed items" — do not mark with `[x]`.

- [ ] **Step 3.8: Verify the documentation diff looks correct**

```sh
git diff TODO.md docs/superpowers/specs/2026-05-08-plugin-perf-report.md
```

Expected: TODO.md shows one line removed; perf-report.md shows Appendix D added with all `<fill>` placeholders replaced by real numbers.

- [ ] **Step 3.9: Commit**

```sh
git add TODO.md docs/superpowers/specs/2026-05-08-plugin-perf-report.md
git commit -m "$(cat <<'EOF'
docs(plugin-perf): record §4.2 fix#2 verification — target met

Re-took the W-P5 dhat measurement on a 3-plugin same-mask startup
after the real-linker-cache landing, and recorded the verdict in
the perf report's new Appendix D: `LinkerInstance<T>::insert`
blocks dropped from 1,396 (fix#1 only) to <fill>, a -<fill>% drop
vs fix#1 and -<fill>% cumulative vs the original pre-fix baseline.
The §5.1 ≥50% target is now met.

The distinct-mask sanity run (three plugins with different cap
subsets) reproduced the 33% fix#1 ceiling within noise, confirming
the cache key (`effective_capabilities`) correctly separates entries
when masks differ.

TODO.md follow-up entry deleted per "delete completed items" policy.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

(Replace `<fill>` placeholders in the commit message body with the actual numbers before committing.)

---

## Self-Review Checklist (run before handing off)

- [ ] **Spec coverage:** spec §3 (architecture) ↔ Task 2.1–2.2; §5 (helper) ↔ Task 2.3; §6 (load_one integration) ↔ Tasks 2.4, 2.5; §7 (concurrency/lifetime) ↔ no code (asserted by `&mut self` signature on `load_one`); §8 (failure mode) ↔ Task 2.3 helper body; §9.1 functional regression ↔ Tasks 2.9, 2.10; §9.2 unit tests ↔ Tasks 1.1, 1.2, 2.6 (relocated to `tests/plugin.rs` per the deviation noted in the header); §9.3 perf verification ↔ Tasks 3.3–3.4; §10 documentation ↔ Tasks 3.6, 3.7; §11 risks ↔ no code; §12 acceptance criteria 1–5 ↔ Tasks 2.9, 2.8, 3.3, 3.4, 3.6 / 3.7.
- [ ] **No placeholders in tasks:** all code blocks complete; all `<fill>` markers are deliberately confined to Appendix D / commit body and are explicitly called out as "replace with measured numbers".
- [ ] **Type consistency:** field name is `linker_cache` everywhere (not `linker_map` / `linkers` / etc.); helper is `get_or_build_linker` everywhere; accessor is `linker_cache_len` everywhere.
