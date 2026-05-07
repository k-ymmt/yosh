# Design: Plugin Performance Tuning — Measurement and Targeted Fixes

**Date:** 2026-05-08
**Status:** Phase 1 design (measurement infrastructure). Phase 2..N per-fix specs derived from Phase 1 findings.
**Performance reference:** `performance.md` (existing pattern for shell-core perf work)
**Estimated effort:** 3–5 days total. Phase 1 ≈ 1.5 days; Phase 2 ≈ 1.5–3.5 days depending on findings.

## 1. Background and motivation

The plugin system (`src/plugin/`, `crates/yosh-plugin-{api,manager,sdk}`) is a featured capability of yosh. To make it credible as a "feature you reach for" rather than "feature you tolerate," its measured performance must be characterized and the worst offenders fixed.

`performance.md` already established a measurement methodology (Criterion + dhat-rs + samply, with `yosh-dhat` and Python extractors under `scripts/perf/`) for the shell core. That infrastructure does not yet cover the plugin path:

- `benches/plugin_bench.rs` exists but contains only three minimal Criterion benchmarks built on `test_plugin`, which is also used by capability tests and emits stdout side-effects per iteration (TODO.md item).
- No dhat workload exercises the plugin host-call boundary, hook dispatch, or multi-plugin startup.
- No samply scenario covers plugin code paths.
- There is no plugin equivalent of the W1/W2/W3 workload triple in `performance.md`.

Without these baselines, any plugin-side optimization is a guess.

## 2. Goal

Produce a measurement-backed performance report for the plugin subsystem, identify the worst hotspots across three workloads, and apply targeted fixes within an S2-scoped refactor budget (mid-scale: host call ABI, linker/store reuse, hook dispatch fast-paths). Defer items that exceed S2 to TODO.md with measurement-backed priority.

**Non-goals:**

- wasmtime engine config rewrites (pooling allocator swap, Cranelift opt-level changes). Out of scope per S2.
- cwasm precompile-strategy redesign. Out of scope per S2.
- New WIT exports / breaking SDK API changes. Out of scope per S2.
- Fuel metering / per-call memory caps. Already deferred in `2026-04-27-wasm-plugin-runtime-design.md` §10.
- Optimizing `test_plugin` itself. Capability tests are the source of truth; we add a separate perf fixture instead.

## 3. Decisions taken during brainstorming (2026-05-08)

| Decision | Choice | Rationale |
|---|---|---|
| Approach | F (measure all, then fix worst) | User explicitly requested measurement-driven tuning. |
| Workloads | W-P1 + W-P3 + W-P5 | Mirrors `performance.md`'s W1/W2/W3 (interactive / script / startup). W-P2 / W-P4 / W-P6 deferrable as derivative if needed. |
| Refactor scope | S2 (mid-scale) | S1 risks ending at "measure and TODO" with no visible win; S3 drags wasmtime version coupling. |
| Success criteria | G3 + partial G2 | G3 ("Top-N closed") matches existing `performance.md` operational pattern; G2 ("Δ %") added for `pre_prompt` because it is the most user-visible. |
| Document structure | Phase 1 report + Phase 2..N per-fix specs | Same pattern as `performance.md` §4.1 / §4.3 / §4.7 individual specs. |
| Plugin fixture | New `perf_plugin` (separate from `test_plugin`) | Avoids polluting capability tests; lets us drop stdout side-effects per existing TODO item. |

## 4. Workloads

### 4.1 W-P1: `pre_prompt`-heavy (interactive proxy)

Models the cost a user pays on every prompt redisplay when one or more plugins implement `pre_prompt`.

- Driver: `yosh-dhat --pre-prompt-loop N` (new flag) or Criterion bench function. Calls `PluginManager::call_pre_prompt` directly without going through `Repl::run`.
- Variants: 0 plugins, 1 plugin (`perf_plugin` noop pre_prompt), 3 plugins (3 copies of `perf_plugin` to surface dispatch-loop scaling).
- N = 1000 for dhat / samply runs; Criterion drives its own iteration budget.

### 4.2 W-P3: command throughput (scripted use)

Models a script that calls a plugin command in a loop (e.g., a custom utility that wraps `kubectl` or `git`).

- Driver: `benches/data/plugin_w3.sh` — a yosh script of the form `for i in $(seq 1 1000); do noop_cmd; done`. Run via `yosh-dhat` for heap and `samply record` for CPU.
- Sub-variants exposed at the Criterion level: `noop_cmd` (zero host calls), `noop_var` (1 host call), `burst_var` (10 host calls).

### 4.3 W-P5: startup with plugins (cold path)

Models the steady-state cost a user pays for every shell launch when their config lists N plugins.

- Driver: Criterion bench functions `startup_one_plugin` / `startup_three_plugins`. Each iteration spawns one `yosh -c 'echo hi'` process (matching the existing `startup_echo_hi` pattern); Criterion picks the iteration count. dhat counterpart via `yosh-dhat -c 'echo hi'` with a config that names `perf_plugin`.
- Variants: 0 plugins (existing `startup_echo_hi` baseline), 1 plugin, 3 plugins. cwasm cold vs warm controlled by clearing the cache directory between runs.

## 5. Phase 1 deliverables

### 5.1 `perf_plugin` fixture

New workspace member at `tests/plugins/perf_plugin/` (excluded from `default-members`, same convention as `test_plugin` and `trap_plugin`). WIT bindings via cargo-component, target `wasm32-wasip2`.

Exported commands:

| Command | Body | Purpose |
|---|---|---|
| `noop_cmd` | `return 0` | Pure exec-boundary cost; no host imports, no stdout. Replaces the noisy `test_cmd` in benches. |
| `noop_var` | `let _ = variables::get("PERF_VAR"); return 0` | One host-import boundary per call. |
| `burst_var` | 10× `variables::get("PERF_VAR")` | Linearity check on host-import cost. |

Exported hooks:

| Hook | Body | Purpose |
|---|---|---|
| `pre_prompt` | empty | W-P1 dispatch cost with one implementer. |
| `pre_exec` | empty | dispatch cost; pairs with existing `plugin_hook_pre_exec` bench. |
| `post_exec` | empty | dispatch cost mirror of pre_exec. |

Capabilities required: `CAP_VARIABLES_READ` only. No I/O, no exec capability — keeps the boundary minimal.

The existing TODO.md item _"`benches/plugin_bench.rs` output noise — add a silent `noop_cmd` to test_plugin"_ is addressed here by routing through `perf_plugin` rather than extending `test_plugin`. Test-plugin contracts stay untouched.

The existing TODO.md item _"Cargo workspace profile warning — `tests/plugins/{test_plugin,trap_plugin}/Cargo.toml` declare `[profile.release]` blocks"_ is folded in: `perf_plugin/Cargo.toml` will not declare its own profile. If feasible without touching `test_plugin`/`trap_plugin`, the workspace-root profile hoist mentioned in that TODO is also done as part of Phase 1.

### 5.2 Criterion bench expansion (`benches/plugin_bench.rs` and `benches/startup_bench.rs`)

W-P1 (in `plugin_bench.rs`):

- `plugin_pre_prompt_zero_plugins` — empty `PluginManager`. Measures dispatch-loop empty cost.
- `plugin_pre_prompt_one_noop` — 1× `perf_plugin`.
- `plugin_pre_prompt_three_noop` — 3 plugins. Implementation detail: `PluginManager` keys plugins by manifest `name`, so loading the same wasm under three names requires either three thin alias manifests or a small loader-side change to allow alias entries. The Phase 2-fix-or-Phase-1-precondition decision is taken at implementation time; the bench design itself is independent of which path is chosen.

W-P3 (in `plugin_bench.rs`):

- `plugin_exec_noop_cmd` — `perf_plugin::noop_cmd`. Replaces the existing `plugin_exec_test_cmd` (which stays for backward comparability during the transition, then removed).
- `plugin_exec_noop_var` — `perf_plugin::noop_var`. Replaces `plugin_exec_echo_var`.
- `plugin_exec_burst_var` — `perf_plugin::burst_var` (10 host imports).

Existing hook benches:

- `plugin_hook_pre_exec` (existing) — keep, but back it with `perf_plugin` not `test_plugin`.
- New `plugin_hook_pre_exec_zero_plugins` — baseline.

W-P5 (in `startup_bench.rs`):

- `startup_zero_plugins` — alias / restructure of existing `startup_echo_hi` to make the comparison explicit.
- `startup_one_plugin` — config file pre-staged with `perf_plugin`, run `yosh -c 'echo hi'`.
- `startup_three_plugins` — config with three plugins.

cwasm cold vs warm split: covered via two helper functions that wipe / pre-populate the cwasm cache directory before measurement. Same Criterion bench function name with a `_cold` / `_warm` suffix.

### 5.3 `yosh-dhat` extension

Current `src/bin/yosh-dhat.rs` runs a script through the normal interpreter under dhat. Add two non-interactive plugin-driver modes:

- `--pre-prompt-loop N` — load configured plugins, then call `PluginManager::call_pre_prompt` N times in a tight loop. Used for W-P1 dhat / samply measurement (pre_prompt is otherwise unreachable without an interactive Repl).
- `--exec-loop N CMD ARGS...` — load configured plugins, then call `PluginManager::exec_command(CMD, ARGS)` N times. Used as a non-PTY alternative to the W-P3 script path when isolating allocations is needed.

Both modes default to N=1 if omitted, which is useful for samply attribution.

### 5.4 Workload scripts (`benches/data/`)

- `plugin_w3.sh` — `for i in $(seq 1 1000); do noop_cmd; done`. Used by both `yosh-dhat` (heap) and `samply record` (CPU).
- W-P1 has no script — it is driven entirely via `yosh-dhat --pre-prompt-loop N` and `samply record -- yosh-dhat --pre-prompt-loop N` because `pre_prompt` is not reachable from a script.
- W-P5 has no script — it is driven by Criterion bench functions (process launch per iter) and `yosh-dhat -c 'echo hi'` (single launch under heap).

### 5.5 Report document

Output: `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` (separate file from this design spec; this design specifies what the report will contain, not its findings).

Report structure (mirroring `performance.md`):

- §1 Executive Summary — top remaining hotspots, ranked.
- §2 Methodology — workload definitions, perf_plugin fixture, yosh-dhat extension, build profile (`profiling`).
- §3 Results — per-workload tables: Criterion medians, dhat Top-10 by bytes, dhat Top-10 by calls, samply Top-10 by total time.
- §4 Findings — one subsection per identified hotspot: location, measurement, suspected cause, fix candidates (ordered by plausibility).
- §5 Recommendations — priority matrix (impact × effort), next-project queue, items to add to TODO.md.
- §6 Reproducibility — commands to regenerate every table.

Any finding meeting the Phase 2 entry criteria (§7 below) gets a sibling per-fix design spec.

## 6. Phase 1 acceptance criteria

- `perf_plugin` builds via `cargo component build -p perf_plugin --target wasm32-wasip2 --release` without warnings.
- All new Criterion benches run cleanly under `cargo bench` (no stdout pollution; medians stable across two consecutive runs within ±15%).
- `yosh-dhat --pre-prompt-loop` and `--exec-loop` modes produce dhat-heap.json files that `scripts/perf/dhat_top_n.py` can parse.
- `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` is committed with all six sections populated and at least three findings ranked.
- TODO.md gains entries for any finding deferred to Phase 2 or beyond, each with a `performance.md`-style measurement attribution.

## 7. Phase 2..N: per-fix process

Phase 1 produces a ranked list of hotspots in `2026-05-08-plugin-perf-report.md` §4. Phase 2 onward picks them up under this rule:

**Entry criteria for a Phase 2 spec:**

- Hotspot is in the report's §4 with a measurement.
- Estimated effort fits within S2 (mid-scale; no engine config rewrites; no breaking SDK API changes).
- Either (a) Criterion bench shows ≥10% improvement potential on at least one bench, or (b) dhat shows ≥10% bytes drop on the targeted workload, or (c) the hotspot is `pre_prompt`-related (the G2 carve-out).

**Spec naming:** `docs/superpowers/specs/YYYY-MM-DD-plugin-<short-name>-design.md`, mirroring `2026-04-21-pathname-expand-fast-path-design.md` etc.

**Anticipated candidates** (priors; will be confirmed or refuted by Phase 1):

- `catch_unwind` overhead in hook dispatch — `performance.md` §4.2 found this pattern in shell-core function calls (~2.1× ratio); plausibly recurs in `PluginManager::call_*` paths if every hook wraps the wasm call in `catch_unwind`.
- Hook dispatch with no implementers — if `call_pre_prompt` iterates all plugins regardless of whether any implements `pre_prompt`, a "implementer set" cached at load time could short-circuit the dispatch.
- Linker rebuild per Store — wasmtime `Linker` construction is non-trivial; if it happens per-instance rather than per-engine, a one-shot `Linker` cache would amortize it.
- Host-call argument copies — host imports that take `Vec<u8>` / `String` may force allocations; some can be `&[u8]` / `&str` if the WIT signature allows.

These are not commitments to fix; they are starting hypotheses for §4 of the report.

## 8. Risks and unknowns

- **`pre_prompt` driving via `yosh-dhat` may not exactly match the interactive path.** The interactive `Repl::run` calls `call_pre_prompt` once per line read; our loop calls it 1000 times back-to-back without intervening I/O. Allocation-wise this should be representative; latency-wise it skips terminal-I/O cost which is desirable (we are measuring the plugin path, not the terminal). Documented in the report's §2 alongside the existing macOS samply caveat.
- **Three-plugin scaling may be dominated by `Vec<Plugin>` iteration cost rather than per-plugin work.** If so, the relevant fix moves from "per-plugin overhead" to "dispatch loop structure," which is still in S2 scope.
- **cwasm warm/cold split sensitivity.** macOS file-cache state varies; first run may be cold even if cwasm exists. Phase 1 includes one warm-up iteration before the timed window for cold/warm benches.
- **Criterion measurement noise.** `performance.md` §3.2 already documents ±20–22% wall-time variance for `release.sh test`. Plugin benches inherit the same envelope; report medians plus min/max, do not over-interpret single-percent deltas.
- **`perf_plugin` cargo-component build adds a ~10–20 s step to first-time bench setup.** Documented; not a regression on subsequent runs because the wasm artifact is cached by cargo.

## 9. Out of scope (revisit later)

- W-P2 (pre_exec + post_exec heavy): derivative of W-P1; add only if Phase 1 shows pre_prompt is unrepresentative.
- W-P4 (host-import multi-mix): subsumed by `burst_var` for now.
- W-P6 (resident-memory snapshot): dhat already records peak bytes; explicit RSS measurement adds platform-specific tooling cost without clear payoff at S2 scope.
- Async wasmtime APIs. Current host imports are sync; switching is an S3-class decision.
- Cross-plugin shared-state benchmarks. No shared-state path exists in the current SDK.

## 10. Reproducibility (Phase 1)

```bash
# Build perf_plugin
cargo component build -p perf_plugin --target wasm32-wasip2 --release

# Build the runtime artifacts under the profiling profile
cargo build --profile profiling \
    --bin yosh --bin yosh-dhat --features dhat-heap \
    --bench plugin_bench --bench startup_bench

# Criterion
cargo bench --bench plugin_bench
cargo bench --bench startup_bench

# Plugin config for the dhat runs is staged via YOSH_CONFIG_DIR (or equivalent)
# pointing at a directory that lists perf_plugin. Phase 1 records the exact
# env-var name and config layout in the report's §6.

# dhat: W-P1
YOSH_CONFIG_DIR=target/perf/cfg-1plugin \
    cargo run --profile profiling --features dhat-heap --bin yosh-dhat -- \
    --pre-prompt-loop 1000
mv dhat-heap.json target/perf/dhat-plugin-w1.json

# dhat: W-P3
YOSH_CONFIG_DIR=target/perf/cfg-1plugin \
    cargo run --profile profiling --features dhat-heap --bin yosh-dhat -- \
    benches/data/plugin_w3.sh
mv dhat-heap.json target/perf/dhat-plugin-w3.json

# dhat: W-P5
YOSH_CONFIG_DIR=target/perf/cfg-1plugin \
    cargo run --profile profiling --features dhat-heap --bin yosh-dhat -- \
    -c 'echo hi'
mv dhat-heap.json target/perf/dhat-plugin-w5.json

# samply (same triplet via samply record --save-only)
# Top-N extraction
python3 scripts/perf/dhat_top_n.py target/perf/dhat-plugin-w1.json 10
python3 scripts/perf/dhat_top_n.py target/perf/dhat-plugin-w3.json 10
python3 scripts/perf/dhat_top_n.py target/perf/dhat-plugin-w5.json 10
```

## 11. Definition of done (whole project)

- Phase 1 report committed with §1–§6 populated.
- Every Phase 1 §4 finding is either (a) addressed by a landed Phase 2 spec, or (b) recorded in TODO.md with measurement attribution.
- `pre_prompt` G2 measurement: report § Executive Summary states an explicit Δ (e.g., "single-plugin pre_prompt overhead reduced from X µs to Y µs, −Z%").
- `cargo test` and `./e2e/run_tests.sh` are green at the final commit.
