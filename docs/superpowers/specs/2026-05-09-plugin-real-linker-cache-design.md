# Plugin Real Linker Cache by Capability Mask — Design

**Date:** 2026-05-09
**Status:** Approved for implementation
**Tracking:** Plugin perf report §4.2 fix candidate #2; TODO.md "Plugin perf §4.2 follow-up"
**Predecessor:** `70d78ec` (fix #1 — scratch linker cache, verified at −33.3% on `LinkerInstance<T>::insert` blocks at N=3 — see report Appendix C)

## 1. Goal

Eliminate redundant `Linker<HostContext>` constructions across plugin loads that share the same negotiated capability mask, so a multi-plugin shell session pays the per-mask construction cost only once. Closes the gap from the verified −33.3% (scratch caching alone at N=3) toward the report §5.1 ≥50% target for sessions where multiple plugins share a cap mask.

## 2. Non-goals

- **Not** restructuring the metadata two-stage probe (report §4.2 fix candidate #3) — kept as a separate future option.
- **Not** caching `Component`s, `InstancePre`s, or `Store`s — only the cap-keyed `Linker`.
- **Not** introducing concurrency primitives — `PluginManager::load_one` already runs under `&mut self`.
- **Not** runtime evict/refresh — the cache lives for the `PluginManager`'s lifetime (one shell session).

## 3. Architecture

Replace the existing `scratch_linker: Option<Linker<HostContext>>` field on `PluginManager` (added in `70d78ec`) with a unified per-capability-mask cache:

```rust
pub struct PluginManager {
    // ...
    /// Linkers keyed by negotiated `effective_capabilities`. Both the
    /// metadata-probe scratch linker (always `CAP_ALL`) and per-plugin
    /// real linkers share this cache; an entry is built lazily on first
    /// load that needs it. The Linker is plugin-independent: it depends
    /// only on the engine and the cap bitfield. `instantiate_pre(&self,
    /// &Component)` takes the linker by reference, so cached entries
    /// can be reused across components without cloning.
    linker_cache: std::collections::HashMap<u32, Linker<HostContext>>,
    // ...
}
```

**Why unify with the existing `scratch_linker` field:**
- Conceptually one cache, one lookup pattern, one field — easier to reason about.
- A real plugin granted `CAP_ALL` (rare but legal) would automatically share the same cache entry as the scratch path, instead of building two identical linkers.
- Removes a dedicated field that becomes redundant once the keyed cache exists.

## 4. Cache Key

**Key:** `effective_capabilities: u32` (the post-negotiation bitfield from `mod.rs:470-480`).

**Why this is sufficient:**
- `linker::build_linker(&self.engine, allowed: u32)` is a pure function of `engine` (constant per `PluginManager`) and `allowed` (the cap mask).
- `allowed_commands` (per-plugin argv allowlist) does **not** affect linker construction — it lives on `HostContext`, set on the `Store` after the linker is built (`mod.rs:494-497`). Two plugins with the same caps but different `allowed_commands` correctly share a linker.
- All other per-plugin state (component bytes, plugin name, env pointer) is bound at `instantiate_pre` / `Store::new` time, not at linker time.

**Cardinality:** at most 2^7 = 128 distinct masks (one bit per declared capability). In practice plugins cluster around a small number (often 1–3) of masks per session.

## 5. Helper Function

```rust
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

Notes:
- The double lookup (`contains_key` then `get`) is required because `HashMap::entry().or_insert_with` does not compose with fallible builders without an external crate. The two operations are O(1) and the linker build itself dominates.
- `build_linker` failures do **not** insert into the cache — `?` returns before `insert`. Subsequent loads for that mask will retry.
- Returns `&Linker` (not a clone) — `Linker::instantiate_pre(&self, &Component)` accepts `&self`, so a borrowed reference is what callers need.

## 6. `load_one` Integration

Two replacements in `src/plugin/mod.rs`:

**Step 5 (scratch / metadata probe)** — current (fix#1 form):
```rust
if self.scratch_linker.is_none() {
    let l = linker::build_linker(&self.engine, CAP_ALL)
        .map_err(|e| format!("{}: linker init failed: {}", path.display(), e))?;
    self.scratch_linker = Some(l);
}
let scratch_linker = self.scratch_linker.as_ref().expect("...");
```
→ becomes:
```rust
let scratch_linker = self.get_or_build_linker(CAP_ALL, path)?;
```

**Step 7 (real linker)** — current:
```rust
let real_linker = linker::build_linker(&self.engine, effective_capabilities)
    .map_err(|e| format!("{}: linker build failed: {}", path.display(), e))?;
let real_pre = PluginWorldPre::new(
    real_linker.instantiate_pre(&component)
        .map_err(|e| format!("{}: real instantiate_pre: {}", path.display(), e))?,
)
.map_err(|e| format!("{}: real bindings pre-init: {}", path.display(), e))?;
```
→ becomes:
```rust
let real_linker = self.get_or_build_linker(effective_capabilities, path)?;
let real_pre = PluginWorldPre::new(
    real_linker.instantiate_pre(&component)
        .map_err(|e| format!("{}: real instantiate_pre: {}", path.display(), e))?,
)
.map_err(|e| format!("{}: real bindings pre-init: {}", path.display(), e))?;
```

The `&mut self` borrow from `get_or_build_linker` returns immediately (the `&Linker<HostContext>` reborrows `&self`), so the rest of `load_one` can use `&self.engine`, `&self.plugins`, etc., without conflict — same pattern as the current `self.scratch_linker.as_ref()` use.

## 7. Concurrency and Lifetime

- **Concurrency:** Plain `HashMap` — `load_one` already holds `&mut self`. No `RwLock` / `Arc` / `OnceCell` is needed. If concurrent loads are added later, this field will need `RwLock<HashMap<u32, Arc<Linker<...>>>>` or equivalent — but YAGNI for now.
- **Lifetime:** Cache lives for the `PluginManager`'s lifetime (one shell session). No invalidation: the linker depends only on the engine, which is itself constant per manager. Memory ceiling ≈ N_distinct_masks × ~170 KB; with the typical 1–3 masks per session, < 1 MB.

## 8. Failure Mode

If `build_linker(engine, caps)` returns `Err`, the cache is not modified. The error is propagated up via the existing `format!("{}: linker build failed: {}", path.display(), e)` wrapper. A subsequent load for the same `caps` will retry. This is the same observable behavior as today (where every load attempts a fresh build).

## 9. Tests

### 9.1 Functional regression
- `cargo test` (lib + integration) — must pass green.
- `cargo test --features test-helpers` — exercises the 23-plugin integration suite under `tests/plugin.rs`.

### 9.2 New unit tests in `src/plugin/mod.rs::tests`

Two tests, both using existing `test_helpers::load_plugin_with_caps` (or equivalent) to load multiple plugins through one `PluginManager`. Requires bumping `linker_cache` visibility to `pub(super)` (one-line `pub(super)` on the field is enough — keep `PluginManager` itself unchanged).

1. **`linker_cache_reuses_entry_for_same_mask`** — Load two plugins with identical capabilities. Assert `manager.linker_cache.len() == 2` (one for `CAP_ALL` from the metadata probe path, one for the shared real mask) — proves the second plugin's real-linker request hit the cache.

2. **`linker_cache_separates_entries_for_distinct_masks`** — Load two plugins with different capabilities. Assert `manager.linker_cache.len() == 3` (`CAP_ALL` + two distinct real masks).

The "no insert on `build_linker` failure" branch is not directly testable without injecting a broken `Engine`; it is left to code review. The current `build_linker` only fails on a wasmtime-internal misconfiguration, which the existing test corpus would also miss.

### 9.3 Performance verification (gate for "done")

Re-run the W-P5 dhat 3-plugin reproducer documented in report §6 / Appendix C:

```sh
cargo build --profile profiling --features dhat-heap --bin yosh-dhat
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat target/perf/echo-hi.sh
mv dhat-heap.json target/perf/dhat-plugin-w5-3p-postfix2.json
python3 scripts/perf/dhat_filter_frame.py target/perf/dhat-plugin-w5-3p-postfix2.json "LinkerInstance<T>::insert"
```

**Expected (same-mask, N=3):**
- Pre-fix#2 (current HEAD = post-fix#1): 1,396 blocks / 522 KB on `LinkerInstance::insert`
- Post-fix#2: builds drop from `N+1=4` to `1+1=2` (one `CAP_ALL` cached scratch + one real cached for the shared mask) → ≈ 467 blocks / ≈ 174 KB
- **Δ vs fix#1: −67%; Δ vs original baseline: −78%.** Meets ≥50% target.

**Sanity (distinct-mask, N=3):** Stage three plugins.lock entries with three distinct cap subsets. Expected post-fix#2: `1+3=4` builds = same as fix#1 alone (33% over original baseline). Confirms the cache key correctly differentiates masks.

The numbers from the same-mask run are recorded in the report as Appendix D; the distinct-mask sanity is recorded inline in the same appendix.

## 10. Documentation Touchpoints

- **`docs/superpowers/specs/2026-05-08-plugin-perf-report.md`** — add **Appendix D: §4.2 Real Linker Cache Verification** with the same shape as Appendix C (method / numbers / verdict).
- **`TODO.md`** — delete the "Plugin perf §4.2 follow-up: real linker caching by capability mask" entry per CLAUDE.md ("delete completed items").
- **No spec edit required** for `2026-04-27-wasm-plugin-runtime-design.md` — the public PluginManager surface is unchanged; this is a private internal optimization.

## 11. Risks and Open Questions

- **Cache visibility leak via `pub(super)` field:** Lifting visibility for tests is a trivial code change but adds a `pub(super)` field to `PluginManager`. Acceptable because the rest of `PluginManager` is intra-module already.
- **Future async/parallel loads:** If `load_one` is ever called concurrently, the plain `HashMap` becomes a data race. The migration path is documented in §7. No action today.

## 12. Acceptance Criteria

1. `cargo test --features test-helpers` passes (no regression in 23-plugin suite + lib).
2. New unit tests `linker_cache_reuses_entry_for_same_mask` and `linker_cache_separates_entries_for_distinct_masks` pass.
3. W-P5 3-plugin same-mask dhat shows `LinkerInstance<T>::insert` blocks ≤ 700 (≥50% drop vs fix#1's 1,396).
4. W-P5 3-plugin distinct-mask dhat reproduces the existing 33%-drop ceiling, confirming key correctness.
5. Report Appendix D added; TODO.md follow-up entry removed.
