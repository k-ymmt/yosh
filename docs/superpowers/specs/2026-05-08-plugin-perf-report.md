# Plugin Performance Report — Phase 1 Baseline

**Measurement date:** 2026-05-08
**Commit:** `48bc83b` (the implementation commit that produced these measurement artifacts)
**Environment:** Darwin 25.3.0 arm64 / Apple M3 / rustc 1.94.1
**Build profile:** `profiling` (`release` + `debug = true` + `strip = false`). Criterion runs use the default `bench` profile (inherits from `release`).
**Spec reference:** `docs/superpowers/specs/2026-05-08-plugin-perf-tuning-design.md` (commit `947efc8`)

## 1. Executive Summary

The plugin runtime is well-optimized in its hot paths; the major cost centres are one-time initialisation (wasmtime JIT compilation) and the exec-boundary crossing per call — not the per-hook dispatch loop itself.

**G2 carve-out (pre_prompt Δ):** zero-plugin baseline is 1.34 ns/call; single-plugin pre_prompt is 27.43 ns/call; three-plugin pre_prompt is 84.12 ns/call. **Single-plugin overhead = 26.1 ns/call (1,947× zero-plugin), roughly 27 ns added per prompt redisplay.** At typical interactive rates this is imperceptible, but the linear scaling to three plugins (84 ns = 3.07× the one-plugin cost, matching the expected 3×) confirms correct per-plugin dispatch with no unexpected fixed overhead.

**Top remaining hotspots (ranked by user-visible impact):**

1. **host-call argument copies (W-P3)** — `noop_var` (one `variables::get` boundary) costs 238 ns vs 112 ns for `noop_cmd` (zero host imports); `burst_var` (ten host imports) costs 1,307 ns (127 ns per additional host import). The WIT-generated binding converts `String` arguments by value on every crossing — a target for `&str`/borrowed-value host APIs.

2. **Linker rebuild per plugin load (W-P5)** — `LinkerInstance::insert` is the dominant repeating allocation pattern at startup, appearing across 12+ distinct call sites in the W-P5 dhat profile, collectively totalling ~200+ KB of indexmap/vec resizes. Each plugin load calls `build_linker` twice (scratch + real), creating two fully-populated `Linker<HostContext>` structs. A shared, cached `Linker` keyed on capability mask would eliminate the second build per plugin and amortise the first across sessions.

3. **JIT compilation dominates W-P1 heap (cold path)** — The 72 MB W-P1 dhat total is almost entirely Cranelift's register-allocator (`regalloc2::ion`, `cranelift_codegen`) during the one-time `Component::new()` call. At steady-state (cwasm cache warm) this cost is eliminated. The action item is to ensure the cwasm cache is always written on first load and to surface cache-miss events in the startup log at debug level, so users can diagnose unexpected cold reloads.

**Non-findings worth noting:**

- `call_pre_prompt` itself allocates zero bytes per iteration in steady state (confirmed by dhat W-P1: all blocks with call stacks near `run_pre_prompt_loop` resolve to the one-time plugin-load path, not the loop body).
- Startup overhead from one vs three plugins is statistically indistinguishable at the process level (65.90 ms baseline vs 66.72 ms one-plugin vs 66.32 ms three-plugins), confirming that cwasm warm loads are cheap enough to be hidden by OS process launch noise.
- The W-P3 dhat profile's top allocators (`yosh::expand::pattern::matches`, `yosh::expand::pathname::expand`) are shell-core artefacts of the script workload (`for i in $(seq 1 1000)` triggers pathname expansion), not plugin-specific.

**Recommended next-project order:** §5.2.

## 2. Methodology

### 2.1 Workloads

| | Definition | Driver commands |
|---|---|---|
| **W-P1 — pre_prompt heavy** | Models one plugin's `pre_prompt` hook called N times. Closest proxy for interactive prompt-redisplay cost. | `yosh-dhat --pre-prompt-loop 1000` (dhat); `yosh-dhat --pre-prompt-loop 5000` (samply). Criterion: `plugin_pre_prompt_*` bench group. |
| **W-P3 — command throughput** | `benches/data/plugin_w3.sh`: `for i in $(seq 1 1000); do noop_cmd; done`. Models a user script that repeatedly calls a plugin command. | `yosh-dhat benches/data/plugin_w3.sh` (dhat); `yosh benches/data/plugin_w3.sh` (samply). Criterion: `plugin_exec_*` + `plugin_hook_pre_exec*` bench group. |
| **W-P5 — startup with plugins** | Process launch of `yosh -c 'echo hi'` with 1 or 3 plugins configured. | `yosh-dhat target/perf/echo-hi.sh` with HOME pointing at a staged plugin config (dhat). Criterion: `startup_*` bench group. |

### 2.2 Fixture

`perf_plugin` (at `tests/plugins/perf_plugin/`) is the measurement fixture:

- **Three commands:** `noop_cmd` (no host imports; pure exec-boundary cost), `noop_var` (one `variables::get` host import), `burst_var` (10 `variables::get` calls; linearity check).
- **Three hooks:** `pre_prompt` (empty body; dispatch cost), `pre_exec` (empty body; dispatch cost), `post_exec` (empty body; dispatch cost mirror of `pre_exec`).
- **Capabilities:** `CAP_VARIABLES_READ` + three hook capabilities. No I/O, no exec — keeps the boundary minimal and eliminates side-effects that would pollute timing.

This fixture replaces `test_plugin` in benchmarks, which printed to stdout on every iteration and invalidated Criterion's output-noise heuristics.

### 2.3 yosh-dhat extensions

`src/bin/yosh-dhat.rs` was extended with two non-interactive plugin-driver modes:

- `--pre-prompt-loop N` — loads plugins from config, then calls `PluginManager::call_pre_prompt` N times in a tight loop. Used for W-P1 heap and CPU profiling because `pre_prompt` is unreachable from a non-interactive script (the `Repl` is not started).
- `--exec-loop N CMD ARGS...` — loads plugins, calls `PluginManager::exec_command(CMD, ARGS)` N times. Available for W-P3 isolation; the script-based path was used in this report.

Both modes default to N=1 if the argument is absent, which is useful for samply attribution where a single pass is sufficient.

Note: `yosh-dhat` takes a script file path as argument, not `-c "..."` inline. The W-P5 dhat run used `target/perf/echo-hi.sh` containing `echo hi`.

### 2.4 Build profile

```toml
[profile.profiling]
inherits = "release"
debug = true
strip = false
```

All samply / dhat runs use `--profile profiling` artifacts. Criterion runs use the default `bench` profile (also inherits `release`; no debug symbols). Both profiles share identical codegen; timing results are directly comparable.

### 2.5 Limitations

**macOS samply limitations (inherits from `performance.md` §2.4):** samply's self-time column on macOS is dominated by Mach kernel sampling routines (`mach_get_times`, `host_get_special_port`, `macx_backing_store_recovery`). These account for ~49% of W-P3 self-time at this measurement. Total-time is the usable column for yosh function analysis on macOS.

**samply W-P1 sampling is dhat-dominated:** the W-P1 samply run uses `yosh-dhat` (dhat allocator active), so `dhat::Globals::finish` consumes ~80% of total-time samples during shutdown. The W-P1 samply table reflects dhat teardown, not plugin hot-path. Criterion medians and dhat per-block analysis are the authoritative W-P1 data sources.

**samply W-P5 sample count too low:** a single `yosh -c 'echo hi'` run generates only ~10 samply samples, which is insufficient for stable rankings. The Criterion `startup_*` benches and W-P5 dhat profile are the authoritative sources for startup cost.

**W-P3 script workload mixes shell-core and plugin costs:** `plugin_w3.sh` uses `$(seq 1 1000)` command substitution, which triggers pathname expansion and field splitting; these show up as the dhat W-P3 top allocators (`expand::pattern::matches`, `expand::pathname`). They are shell-core costs, not plugin costs. The plugin-specific cost is captured cleanly in Criterion (`plugin_exec_*`).

## 3. Results

### 3.1 W-P1: pre_prompt heavy

#### Criterion (commit `48bc83b`)

| Bench | Min | Median | Max |
|-------|-----|--------|-----|
| `plugin_pre_prompt_zero_plugins` | 1.34 ns | 1.35 ns | 1.35 ns |
| `plugin_pre_prompt_one_noop` | 27.40 ns | 27.43 ns | 27.49 ns |
| `plugin_pre_prompt_three_noop` | 83.52 ns | 84.12 ns | 84.42 ns |

**Δ analysis:** one-plugin vs zero-plugin: +26.1 ns (+1,947%). Three-plugin vs one-plugin: +56.7 ns per two additional plugins (+28.3 ns/plugin), consistent with O(N) linear dispatch with no hidden overhead.

#### dhat: total bytes / blocks (W-P1, 1000-iteration loop)

| Metric | Value |
|--------|-------|
| Total bytes allocated | 72,626,888 (69.3 MB) |
| Total blocks | 117,432 |
| At t-gmax (peak live) | 4,304,434 bytes in 3,323 blocks |
| At t-end | 959 bytes in 13 blocks |

#### dhat Top-10 by bytes (W-P1)

| Rank | Site | Bytes | Blocks | Nearest yosh frame |
|------|------|-------|--------|-------------------|
| 1 | `alloc::raw_vec::RawVec::with_capacity_in` | 11.33 MB | 919 | `wasmtime_cranelift::compiler::compile_uncached` |
| 2 | `alloc::raw_vec::RawVec::with_capacity_in` | 5.21 MB | 216 | `wasmtime_cranelift::compiler::compile_uncached` |
| 3 | `alloc::raw_vec::RawVec::with_capacity_in` | 4.28 MB | 215 | `wasmtime_cranelift::compiler::compile_uncached` |
| 4 | `Vec::grow_amortized` | 2.78 MB | 356 | `wasmtime_cranelift::compiler::compile_uncached` |
| 5 | `alloc::raw_vec::RawVec::with_capacity_in` | 2.69 MB | 472 | `wasmtime_cranelift::compiler::compile_uncached` |
| 6 | `hashbrown::raw::new_uninitialized` | 1.58 MB | 216 | `wasmtime_cranelift::compiler::compile_uncached` |
| 7 | `alloc::raw_vec::RawVec::with_capacity_in` | 1.58 MB | 214 | `wasmtime_cranelift::compiler::compile_uncached` |
| 8 | `hashbrown::raw::fallible_with_capacity` | 1.57 MB | 216 | `wasmtime_cranelift::compiler::compile_uncached` |
| 9 | `BTreeMap::insert` (via `Box::new_uninit`) | 1.48 MB | 10,750 | `regalloc2::ion::liveranges::add_liverange_to_preg` |
| 10 | `alloc::raw_vec::RawVec::with_capacity_in` | 1.47 MB | 283 | `wasmtime_cranelift::compiler::compile_uncached` |

All top-10 sites resolve to the one-time wasmtime JIT compilation path (`Component::new` → `compile_cached` → `compile_uncached` → Cranelift + regalloc2). The per-iteration `call_pre_prompt` hot path contributes zero measured allocations.

#### samply Top-10 total time (W-P1)

Sampling dominated by `dhat::Globals::finish` (80% of 175 total samples) due to dhat teardown overhead. The `yosh_dhat::main` call tree accounts for 94.3% of total-time; within it, the actual pre_prompt loop is below sampler resolution at 5,000 iterations. **See `performance.md` §2.4 caveat; Criterion and dhat are the authoritative W-P1 data sources.**

### 3.2 W-P3: command throughput

#### Criterion (commit `48bc83b`)

| Bench | Min | Median | Max | Note |
|-------|-----|--------|-----|------|
| `plugin_pre_exec_zero_plugins` | 1.56 ns | 1.56 ns | 1.57 ns | zero-plugin baseline |
| `plugin_hook_pre_exec` | 67.05 ns | 67.15 ns | 67.27 ns | 1 plugin, empty hook |
| `plugin_exec_noop_cmd` | 111.32 ns | 111.53 ns | 112.38 ns | 0 host imports |
| `plugin_exec_noop_var` | 238.37 ns | 238.81 ns | 239.32 ns | 1 host import |
| `plugin_exec_burst_var` | 1,303 ns | 1,307 ns | 1,311 ns | 10 host imports |

**per-host-import cost:** `(burst_var − noop_cmd) / 9 imports = (1,307 − 112) / 9 ≈ 133 ns/import`. Each `variables::get` crossing costs ~127–133 ns.

#### dhat: total bytes / blocks (W-P3, plugin_w3.sh 1000-iteration script)

| Metric | Value |
|--------|-------|
| Total bytes allocated | 5,826,490 (5.56 MB) |
| Total blocks | 153,639 |
| At t-gmax (peak live) | 859,913 bytes in 277 blocks |
| At t-end | 977 bytes in 8 blocks |

#### dhat Top-10 by call count (W-P3)

| Rank | Site | Blocks | KB | Nearest yosh frame |
|------|------|--------|----|--------------------|
| 1 | `Vec::with_capacity_in` | 42,042 | 1,290 | `yosh::expand::pattern::matches` (pathname expansion) |
| 2 | `Vec::with_capacity_in` | 24,024 | 181 | `yosh::expand::pathname::glob_in_dir` |
| 3 | `Vec::with_capacity_in` | 19,019 | 297 | `yosh::expand::pattern::matches` |
| 4 | `exchange_malloc` | 8,008 | 407 | `yosh::expand::pipeline::expand_word_to_fields` |
| 5 | `Vec::with_capacity_in` | 4,004 | 12 | `yosh::expand::field_split::get_ifs` |
| 6 | `Vec::with_capacity_in` | 4,004 | 31 | `yosh::expand::field_split::split` |
| 7 | `Vec::grow_amortized` | 3,003 | 24 | `yosh::expand::ExpandedField::push_unquoted` |
| 8 | `Vec::grow_amortized` | 3,003 | 94 | `yosh::expand::ExpandedField::set_range` |
| 9 | `Vec::grow_amortized` | 2,002 | 430 | `yosh::expand::pathname::expand` |
| 10 | `Vec::grow_amortized` | 2,001 | 63 | `yosh::expand::ExpandedField::set_range` |

The W-P3 top allocators (ranked by call count rather than total bytes) are all shell-core expander functions. These are driven by `$(seq 1 1000)` command substitution in the script (8,008 `expand_word_to_fields` calls for 1000 `noop_cmd` invocations + loop variables). The plugin dispatch itself is not visible in the top-10, confirming that the `noop_cmd` exec path contributes negligible allocations.

#### samply Top-10 total time (W-P3)

48 total samples. Top by total time (macOS; use total-time per §2.5):

| Rank | Function | Total % |
|------|----------|---------|
| 1 | `std::sys::backtrace::__rust_begin_short_backtrace` | 100.0% |
| 2–8 | `yosh::run_string` → `yosh::main` → `main` | 81.2% |
| 9 | `yosh::exec::control::exec_command` | 70.8% |
| 10 | `yosh::exec::pipeline::exec_pipeline` | 70.8% |

Self-time dominated by `mach_get_times` (20.8%) and `host_get_special_port` (18.8%) — macOS sampling artefacts per §2.5. No plugin-specific frames visible at 48 samples, consistent with the Criterion result that per-call plugin overhead (112–1,307 ns) is dwarfed by the shell's script-execution overhead.

### 3.3 W-P5: startup with plugins

#### Criterion (commit `48bc83b`)

| Bench | Min | Median | Max |
|-------|-----|--------|-----|
| `startup_echo_hi` (0 plugins) | 65.05 ms | 65.90 ms | 66.45 ms |
| `startup_one_plugin` | 65.80 ms | 66.72 ms | 67.51 ms |
| `startup_three_plugins` | 65.64 ms | 66.32 ms | 66.93 ms |

**Δ analysis:** one-plugin vs zero-plugin: +0.82 ms (+1.2%). Three-plugin vs zero-plugin: +0.42 ms (+0.6%). The three-plugin median is slightly *lower* than one-plugin, indicating the differences are within measurement noise. Plugin loading overhead at cwasm-warm is **statistically indistinguishable** from the no-plugin baseline at this sample count (~100 iterations × ~66 ms = ~6.6 s per bench).

#### dhat: total bytes / blocks (W-P5, single `yosh target/perf/echo-hi.sh` with 1 plugin)

| Metric | Value |
|--------|-------|
| Total bytes allocated | 1,830,379 (1.75 MB) |
| Total blocks | 2,452 |
| At t-gmax (peak live) | 859,460 bytes in 277 blocks |
| At t-end | 2,062 bytes in 10 blocks |

#### dhat Top-10 by bytes (W-P5)

| Rank | Site | Bytes | Blocks | Nearest yosh/wasmtime frame |
|------|------|-------|--------|-----------------------------|
| 1 | `Vec::grow_amortized` | 1,016 KB | 7 | (likely cwasm mmap buffer) |
| 2 | `Vec::with_capacity_in` | 128 KB | 1 | `wasmtime::Component::new` via `yosh::plugin::PluginManager::load_one` |
| 3 | `Vec::try_with_capacity_in` | 119 KB | 1 | `wasmtime::Component::new` via `load_one` |
| 4 | `Vec::try_with_capacity_in` | 58 KB | 1 | `std::fs::read` via `load_one` (wasm binary read) |
| 5–10 | `indexmap::Core::reserve_entries` | 17 KB each | 5 each | `wasmtime::LinkerInstance::insert` |
| 11 | `hashbrown::new_uninitialized` | 14 KB | 6 | `yosh::env::vars::VarStore::from_environ` |

**Key finding:** the W-P5 startup heap cost is dominated by (a) loading the `.wasm` binary into memory (`std::fs::read` → 58 KB), (b) `wasmtime::Component::new` building the component metadata from the cwasm sidecar (~247 KB across two sites), and (c) `LinkerInstance::insert` populating the capability-gated linker namespaces (~17 KB × 10 sites = ~170 KB). The `sha2::sha256::compress256` frame visible in samply corresponds to the cwasm cache-key verification step.

#### samply (W-P5)

Only 10 samples collected from a single `yosh -c 'echo hi'` invocation; insufficient for function-level ranking. Top self-time hit: `sha2::sha256::compress256` (10%, 1 sample) — consistent with the cwasm cache key computation in `load_one`. Dhat profile is the authoritative W-P5 source.

## 4. Findings

### 4.1 Host-Call Argument Allocation (W-P3, P0)

**Title:** WIT argument copies on every host-import boundary

**Location:** `src/plugin/linker.rs` — all `func_wrap` closures accept `String` / `Vec<u8>` by value (e.g., `|mut store, (name,): (String,)|` for `variables::get`).

**Measurement:**
- `plugin_exec_noop_cmd` (0 host imports): 112 ns median
- `plugin_exec_noop_var` (1 `variables::get`): 239 ns median → **+127 ns per import**
- `plugin_exec_burst_var` (10 host imports): 1,307 ns median → **+127–133 ns per import** (linear)
- Raw host-import overhead: ~127 ns / crossing at steady state on Apple M3.

**Suspected cause:** wasmtime's WIT bindgen lowering allocates a `String` on the host heap for each `string`-typed parameter lifted from wasm linear memory. The guest writes the string into its own heap, the canonical ABI lifts it via `memory.grow`/memcpy into a host `String`, and the closure then moves the owned `String` into the host function body. This is presumed to be two copies (wasm→linear, linear→host heap) per `string` argument, based on wasmtime's canonical ABI design; the 127 ns/import figure is directly measured.

In wasmtime's component model, `func_wrap` closures that accept `String` force the canonical ABI lift to allocate regardless of what the function body does with the value. Switching the closure signature to accept `&str` (where the WIT signature allows it) can let the runtime avoid the allocation if the backing memory is directly accessible.

**Fix candidates:**
1. **Borrow host-import signatures where possible** — change `(name,): (String,)` to `(name,): (wasmtime::component::Val,)` or use `func_wrap_concurrent` with borrowed lifetimes; evaluate whether wasmtime 27's `func_wrap` API supports `&str`-typed parameters (medium effort, S2 scope).
2. **Intern frequently-read variable names** — for `variables::get`, the guest typically reads the same names (e.g., `PERF_VAR`) on every call. A small LRU cache on the `HostContext` side mapping `name → Arc<str>` could avoid re-lookup in `VarStore` (low effort, possibly high impact for repeated identical calls).
3. **Batch host imports** — add a `variables::get_many(names: list<string>) -> list<option<string>>` WIT function; one crossing amortises N variable reads (medium effort; requires WIT and SDK change; Phase 2 candidate).

### 4.2 Linker Rebuilt Per Plugin Load (W-P5, P1)

**Title:** Two full `Linker<HostContext>` constructions per plugin at every load

**Location:** `src/plugin/mod.rs:411` (scratch linker) and `src/plugin/mod.rs:457` (real linker); `src/plugin/linker.rs:build_linker`.

**Measurement:**
- W-P5 dhat: `LinkerInstance::insert` appears across 12+ call sites totalling ~170 KB of indexmap/vec allocations per plugin load.
- Startup Criterion: one-plugin vs zero-plugin Δ is within measurement noise (~0.8 ms) at cwasm-warm, meaning the linker construction cost is hidden by process-launch overhead (~66 ms). At cwasm-cold (first run), the full JIT compilation cost of 72 MB (see §3.1 dhat) makes the linker cost immaterial. **Impact is therefore P1 rather than P0.**

**Suspected cause:** `build_linker` is called twice per `load_one` call: once for a permissive scratch linker (to probe `metadata()`), and once for the real capability-gated linker. Each call to `wasmtime_wasi::add_to_linker_sync` + all `yosh:plugin/*` `func_wrap` calls rebuilds the full `IndexMap`-backed internal `NameMap`. The scratch linker is used for a single `metadata()` call and then dropped; its construction work is entirely throw-away.

**Fix candidates:**
1. **Cache the scratch linker as a `once_cell::Lazy`** — build one permissive `Linker<HostContext>` at `PluginManager::new()` time and reuse it for all `metadata()` probes. The linker is `Send + Sync` so it can sit in `PluginManager` fields (low effort).
2. **Cache the real linker keyed by capability mask** — there are at most 2^7 = 128 distinct capability combinations; in practice almost all plugins use the same mask. A `HashMap<u32, Arc<Linker<HostContext>>>` in `PluginManager` would amortise linker construction across multiple plugins with the same grants (medium effort).
3. **Eliminate the two-stage probe** — restructure `load_one` so `metadata()` is called from a `PluginWorldPre` built against the real linker (using `CAP_ALL` at pre-instantiation time, but with a null env pointer to enforce the metadata contract). This removes the scratch linker entirely and halves linker construction cost (medium effort, needs careful contract review).

### 4.3 Startup Cost Dominated by cwasm Cache Miss (W-P5, P1)

**Title:** Plugin JIT compilation on first run allocates ~72 MB / 117k blocks

**Location:** `src/plugin/mod.rs:380–387` (`Component::new`), `src/plugin/cache.rs` (cwasm validation).

**Measurement:**
- W-P1 dhat (1000-iteration loop): 72,626,888 bytes / 117,432 blocks, all in `wasmtime_cranelift::compile_uncached` → `regalloc2::ion` → `cranelift_codegen` paths.
- W-P5 dhat (single startup, cwasm warm): 1,830,379 bytes / 2,452 blocks — 40× less than a cold load.
- Startup Criterion (cwasm warm): no measurable overhead over zero-plugin baseline.

**Suspected cause:** On the first invocation after `perf_plugin.wasm` changes, `validate_cwasm` returns `false` (hash mismatch or missing sidecar) and `Component::new` re-runs the full Cranelift pipeline. The 72 MB / 117k block cost is a one-time tax per wasm binary version. The cwasm cache correctly eliminates this on subsequent runs (W-P5 dhat confirms warm cost is 1.8 MB, not 72 MB).

The actionable gap is observability: there is currently no log message when a cache miss occurs, so users who install a new plugin version see a silent ~100–500 ms startup delay without knowing why. A `log::debug!` or stderr note in `load_one` when the cwasm path is bypassed would close this.

**Fix candidates:**
1. **Add cache-miss log message** — emit `yosh: debug: plugin {name}: cwasm cache miss, recompiling (this is one-time)` at debug level when `Component::new` is called instead of the cached path (low effort, high discoverability value).
2. **Parallelize multi-plugin compilation** — if multiple plugins miss the cache simultaneously (e.g., first run after `yosh plugin install`), they compile serially; rayon-based parallelisation of the `load_one` loop is feasible (high effort, S3 scope — defer).
3. **Emit the cwasm sidecar immediately after install** — during `yosh plugin install`, pre-compile and cache immediately rather than deferring to first run. This makes the cold-compile cost fall at install time, not interactive time (medium effort, Phase 2 candidate).

## 5. Recommendations

### 5.1 Priority matrix

| Finding | Impact | Effort | Priority |
|---------|--------|--------|----------|
| §4.1 Host-call argument copies | High (127 ns/import, linear scaling) | Medium | P0 |
| §4.2 Linker rebuilt per plugin load | Low at cwasm-warm; Medium at cwasm-cold | Medium | P1 |
| §4.3 Startup cwasm cache miss observability | Medium (user-visible silent delay) | Low | P1 |

### 5.2 Next-project queue

1. **`plugin-host-import-borrow` (Phase 2, P0)** — Investigate whether wasmtime 27 `func_wrap` supports `&str` typed parameters and switch `variables::get` / `filesystem::cwd` / `commands::exec` closures to borrowed signatures. Spec: `docs/superpowers/specs/YYYY-MM-DD-plugin-host-import-borrow-design.md`. Success criterion: `plugin_exec_noop_var` Criterion median drops by ≥10% (from ~239 ns baseline).

2. **`plugin-cached-linker` (Phase 2, P1)** — Cache the scratch linker as a `Lazy` in `PluginManager` and optionally cache real linkers by capability mask. Spec: `docs/superpowers/specs/YYYY-MM-DD-plugin-cached-linker-design.md`. Success criterion: W-P5 dhat `LinkerInstance::insert` blocks drop by ≥50%.

3. **`plugin-cwasm-miss-log` (Phase 2, P1)** — Add a visible log/stderr message when the cwasm cache is bypassed during plugin load. Low-effort; can be done as a small patch without a full spec.

4. **`plugin-batch-variables` (Phase 3, P2)** — Add `variables::get_many` WIT function to amortise N variable reads into one host crossing. Requires WIT + SDK + Rust crate version bump. Deferred until Phase 2 benchmarks confirm host-import borrow is insufficient for `burst_var` workloads.

### 5.3 Items to add to TODO.md

The following items should be added to `TODO.md` by Task 14:

- `[plugin-perf §4.1]` Add Phase 2 spec for host-import argument borrow: investigate `&str`-typed `func_wrap` closures in wasmtime 27 to reduce per-crossing String allocation (~127 ns/import measured in `plugin_exec_noop_var`).
- `[plugin-perf §4.2]` Cache scratch linker (`CAP_ALL`) as `once_cell` field in `PluginManager::new()` to eliminate the throw-away scratch linker construction in `load_one`.
- `[plugin-perf §4.3]` Emit a debug/stderr message in `plugin::load_one` when cwasm cache is bypassed (cold compile path), so users can diagnose unexpected startup latency.

## 6. Reproducibility

All commands are relative to the repo root. Run them in order.

### Step 0: Prerequisites

```sh
# Build perf_plugin wasm (already cached if done for Task 12)
cargo component build -p perf_plugin --target wasm32-wasip2 --release

# Build profiling binaries (yosh + yosh-dhat)
cargo build --profile profiling --features dhat-heap --bin yosh --bin yosh-dhat
```

### Step 1: Criterion benchmarks

```sh
cargo bench --bench plugin_bench --features test-helpers
cargo bench --bench startup_bench --features test-helpers
```

Medians are in `target/criterion/<bench-name>/new/estimates.json`, field `["median"]["point_estimate"]` (nanoseconds).

### Step 2: Stage HOME for plugin-aware runs

```sh
mkdir -p /tmp/yosh-perf-home/.config/yosh target/perf
cat > /tmp/yosh-perf-home/.config/yosh/plugins.lock <<EOF
[[plugin]]
name = "perf"
path = "$(pwd)/target/wasm32-wasip2/release/perf_plugin.wasm"
enabled = true
capabilities = ["variables:read", "hooks:pre_prompt", "hooks:pre_exec", "hooks:post_exec"]
EOF
```

### Step 3: dhat runs

```sh
# W-P1 (pre_prompt loop, 1000 iters)
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat --pre-prompt-loop 1000
mv dhat-heap.json target/perf/dhat-plugin-w1.json
python3 scripts/perf/dhat_top_n.py target/perf/dhat-plugin-w1.json 10 \
    > target/perf/dhat-plugin-w1.md

# W-P3 (script throughput)
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat benches/data/plugin_w3.sh
mv dhat-heap.json target/perf/dhat-plugin-w3.json
python3 scripts/perf/dhat_top_n.py target/perf/dhat-plugin-w3.json 10 \
    > target/perf/dhat-plugin-w3.md

# W-P5 (startup with 1 plugin)
echo "echo hi" > target/perf/echo-hi.sh
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat target/perf/echo-hi.sh
mv dhat-heap.json target/perf/dhat-plugin-w5.json
python3 scripts/perf/dhat_top_n.py target/perf/dhat-plugin-w5.json 10 \
    > target/perf/dhat-plugin-w5.md
```

### Step 4: samply runs

```sh
# W-P1 — use yosh-dhat (pre_prompt not reachable from regular yosh without repl)
HOME=/tmp/yosh-perf-home samply record --save-only \
    --output target/perf/samply-plugin-w1.json -- \
    ./target/profiling/yosh-dhat --pre-prompt-loop 5000
python3 scripts/perf/samply_top_n.py target/perf/samply-plugin-w1.json 10 \
    > target/perf/samply-plugin-w1.md

# W-P3
HOME=/tmp/yosh-perf-home samply record --save-only \
    --output target/perf/samply-plugin-w3.json -- \
    ./target/profiling/yosh benches/data/plugin_w3.sh
python3 scripts/perf/samply_top_n.py target/perf/samply-plugin-w3.json 10 \
    > target/perf/samply-plugin-w3.md

# W-P5 — profile a single startup (few samples; Criterion is authoritative)
HOME=/tmp/yosh-perf-home samply record --save-only \
    --output target/perf/samply-plugin-w5.json -- \
    ./target/profiling/yosh target/perf/echo-hi.sh
python3 scripts/perf/samply_top_n.py target/perf/samply-plugin-w5.json 10 \
    > target/perf/samply-plugin-w5.md
```

**Notes vs spec §10:**
- `yosh-dhat` does not accept `-c "..."` inline scripts; use a script file path instead (per §2.3 note).
- samply W-P5 with `sh -c '...'` fails on macOS ("Could not obtain the root task") because `sh` is a system binary. Profile `./target/profiling/yosh` directly.
- samply W-P5 generates ~10 samples per single run; results are low-confidence. Repeat with longer scripts or iterate within the binary for better coverage.
- The intermediate JSON files in `target/perf/` are gitignored; only this report is committed.

## Appendix A: §4.1 Phase 2 PoC Result — Marginal Success

**Date:** 2026-05-08
**Spec:** `docs/superpowers/specs/2026-05-08-plugin-host-import-borrow-design.md`
**Plan:** `docs/superpowers/plans/2026-05-08-plugin-host-import-borrow-poc.md`
**Signature used:** `(name,): (wasmtime::component::WasmStr,)` — `&str` and `Cow<'_, str>` were rejected by wasmtime 27's `IntoComponentFunc` trait bounds; `WasmStr` is the `Lift`-only type that the typed `func_wrap` accepts for borrowed string parameters. The closure body extracts the `&str` via `name.to_str(&store)?` (returns `Cow::Borrowed` for valid UTF-8, no allocation).

The host-side change is broader than the spec anticipated. Because `WasmStr::to_str` borrows the store, and the original `host_variables_get` required `store.data_mut()`, the lookup path was refactored to a read-only variant: a new `bound_env_ref(&self) -> Result<&ShellEnv, ErrorCode>` on `HostContext` plus `host_variables_get(ctx: &HostContext, name: &str)`. No `into_owned()` / `to_string()` was introduced — the only `String` allocation that survives is the return-value clone of the lookup result, which is unrelated to the canonical-ABI lift this PoC targets.

### Measurement (Criterion, median of 3 runs)

| Bench | Baseline (commit `b2b46ce`) | After (this PoC) | Δ |
|---|---|---|---|
| `plugin_exec_noop_cmd` (0 imports) | 113.41 ns | 112.47 ns | −0.83% |
| `plugin_exec_noop_var` (1 import) | 243.49 ns | 224.21 ns | **−7.92%** |
| `plugin_exec_burst_var` (10 imports) | 1,323 ns | 1,170.43 ns | **−11.53%** |

Per-import cost: baseline `(1323 − 113.41)/9 = 134.4 ns`, after `(1170.43 − 112.47)/9 = 117.55 ns`. **Per-crossing improvement: −12.54%.**

### Decisive cross-check: dhat `--exec-loop 1000 noop_var`

| Metric | Baseline (`String`) | After (`WasmStr`) | Δ |
|---|---|---|---|
| Total bytes | 1,834,749 | 1,826,749 | **−8,000 B** |
| Total blocks (allocations) | 3,410 | **2,410** | **−1,000** |

`-1,000` blocks for 1,000 iterations confirms exactly one host-side `String` allocation per host-call crossing was eliminated. This is incontrovertible alloc-level evidence the canonical-ABI lift cost is now zero-copy in the happy path.

### Spec-gate analysis

The spec defined the success criterion as `plugin_exec_noop_var ≤ −10%`. The observed result (`−7.92%`) is below that threshold by ~2 percentage points despite the alloc being eliminated and the per-crossing cost dropping 12.54%. The reason is metric contamination: `plugin_exec_noop_var` measures `exec_boundary + 1 host crossing`, where the exec_boundary (~112 ns) dominates and does not change under this PoC. The fractional improvement on a contaminated metric is necessarily smaller than the fractional improvement on the underlying mechanism. `plugin_exec_burst_var` (10 host imports) absorbs more of the per-crossing improvement and crossed the 10% bar (`−11.53%`).

**Verdict:** the PoC's underlying hypothesis ("borrowing a string parameter eliminates the per-crossing canonical-ABI host-side allocation") is confirmed. The chosen gate metric was poorly suited because `noop_var` retains a fixed boundary cost in its denominator. Future PoCs targeting per-crossing optimizations should gate on per-crossing cost (`(burst_var − noop_cmd) / 9`), not on `noop_var` directly.

### Follow-up

Apply the same `WasmStr` borrow conversion to the remaining `String`-typed host imports: `variables::set` (× 2 args), `variables::export-env` (× 2 args), `filesystem::set-cwd`, `files::read-file`, `files::read-dir`, `files::metadata`, `files::write-file`, `files::append-file`, `files::create-dir`, `files::remove-file`, `files::remove-dir`, `commands::exec`. The mutation paths (`set`, `set-cwd`, `write-file`, `append-file`, `create-dir`, `remove-file`, `remove-dir`) need similar `bound_env_ref` / read-then-mutate restructuring; `commands::exec` accepts a `Vec<String>` for argv which is a separate `list<string>` lift codepath worth measuring before pattern-matching to the same approach. New rollout spec to be authored separately.
