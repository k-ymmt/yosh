# Plugin Host-Import Borrow PoC — Design

**Date:** 2026-05-08
**Phase:** 2, P0
**Predecessor:** `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` §4.1, §5.2
**Status:** Spec; awaiting plan + execution.

## 1. Goal & Success Criterion

Validate whether switching `variables::get`'s host-import closure from
an owned `String` parameter to a borrowed string (e.g. `&str`) reduces
the per-crossing canonical-ABI lift cost measured at ~127 ns/import in
the Phase 1 report.

**Baseline (commit `48bc83b`, Apple M3, profiling profile):**

| Bench | Median |
|---|---|
| `plugin_exec_noop_cmd` (0 host imports) | 111.53 ns |
| `plugin_exec_noop_var` (1 host import) | 238.81 ns |
| `plugin_exec_burst_var` (10 host imports) | 1,307 ns |

`plugin_exec_noop_cmd` is the theoretical floor (no host-import lift).
The gap between `noop_var` and `noop_cmd` (≈127 ns) is the
single-crossing target.

**Success criterion (PoC):** `plugin_exec_noop_var` median drops by
≥10% (≤215 ns) **and** `plugin_exec_noop_cmd` shows no statistically
meaningful regression. Both conditions must hold; either failing means
the PoC is judged a negative result and is not merged.

**Out-of-band cross-checks:**
- `plugin_exec_burst_var` should improve roughly proportionally
  (`(burst_after − cmd) / 9` per-import cost falls). Used as a
  consistency check, not a gating criterion.
- W-P3 dhat profile should show the wasmtime canonical-ABI
  String-lift allocation site shrink or vanish from the Top-10 by call
  count. Used as a corroboration signal.

## 2. Scope

### 2.1 In scope

- `src/plugin/linker.rs:77` — the
  `vars.func_wrap("get", |mut store, (name,): (String,)| { ... })`
  closure for the granted (`CAP_VARIABLES_READ`) path.
- `src/plugin/linker.rs:81` — the symmetric deny-stub closure. Its
  signature must match the granted path so the linker registers the
  same import shape regardless of capability state.
- `src/plugin/host/variables.rs:9` — `host_variables_get`. Internal
  use is `env.vars.get(&name)` only; the owned `String` is never
  consumed, so changing the parameter to `&str` is mechanical.
- `src/plugin/host/variables.rs:17` — `deny_variables_get`, same
  signature change for symmetry.
- `tests/plugin.rs` — verify the existing `variables::get`-touching
  tests still pass without source change.
- `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` — append a
  Phase 2 PoC result section (Appendix A) recording the outcome.

### 2.2 Out of scope

- Other `String`-typed host imports: `variables::set`,
  `variables::export-env`, `filesystem::set-cwd`, `files::read-file`,
  `files::read-dir`, `files::metadata`, `files::write-file`,
  `files::append-file`, `files::create-dir`, `files::remove-file`,
  `files::remove-dir`, `commands::exec`. If the PoC succeeds, a
  follow-up project applies the same transformation to all of these in
  one batch.
- `Vec<u8>` arguments (`io::write`, `files::write-file`,
  `files::append-file` data parameters). The wasmtime lift path for
  list-of-bytes is separate from string lift; out of scope for this
  PoC.
- WIT changes. The `yosh-plugin.wit` interface remains unchanged. This
  PoC is purely a host-side closure-signature change. Plugin authors
  see no API difference; existing `perf_plugin.wasm` does not need
  recompilation.
- The intern/LRU cache approach (report §4.1 fix candidate 2). Out of
  scope per the brainstorming session — pivoting to a different fix
  approach inside this spec would broaden scope and invalidate the
  PoC's "is borrow-based lift cheaper" question.
- Batch host imports (`variables::get_many`, report §4.1 fix candidate
  3). Phase 3 candidate; deferred regardless of PoC outcome.

## 3. Hypothesis

The 127 ns/import figure measured in Phase 1 is dominated by the
canonical-ABI string lift, which today allocates a host-side `String`
because `func_wrap`'s typed argument tuple specifies `String`. If
wasmtime 27's `func_wrap` accepts a borrowed string parameter where
the canonical-ABI lift can read directly from wasm linear memory
without copying onto the host heap, the per-crossing cost should drop
by a measurable amount. The exact amount is uncertain (the lift may
still copy into a temporary, may bounds-check, or may use a stack
buffer); the PoC measures it directly rather than predicting.

The PoC explicitly does *not* assume `&str` is supported. Task 1 is a
spike whose first job is to determine support; if every borrow-shaped
signature fails to compile, the PoC ends with a negative result.

## 4. Approach

A single approach: change the closure-signature directly via
`func_wrap`. Alternatives considered and rejected:

- `Func::wrap_async` / `func_wrap_concurrent` — introduces async
  semantics into a synchronous host-call path. Disproportionate scope
  and side-effects for a PoC.
- `Linker::root().func_new` with manual `Val::String` lifting —
  bypasses the typed wrapper and lets us write our own lift. Doable
  but expands the PoC into lifting-strategy research; conflicts with
  the brainstorming-decided "negative result if direct route fails"
  posture.

### 4.1 Task outline

The detailed task plan is produced by the writing-plans skill. This
section sketches the high-level steps so the plan author has a frame.

1. **Spike — wasmtime 27 borrowed-string support.** Try compiling
   `linker.rs:77` with `(name,): (&str,)`. If that fails, try
   alternative borrow-shaped types from
   `wasmtime::component` (`WasmStr`, `Cow<str>`,
   `wasmtime::component::__internal::String`, etc.) in order. Stop at
   the first signature that compiles, OR conclude unsupported if none
   compile. Record every signature tried and its compiler diagnostic.

2. **Apply borrow to `host_variables_get`.** Change parameter to
   `name: &str`. Body becomes `env.vars.get(name)` (drop the
   `&`). Apply the same change to `deny_variables_get`. Update the
   `variables_get_denied_when_env_null` unit test if its call site
   needs adjustment (today: `host_variables_get(&mut ctx, "PATH".into())`
   → `host_variables_get(&mut ctx, "PATH")`).

3. **Criterion measurement.** Run
   `cargo bench --bench plugin_bench --features test-helpers -- plugin_exec_noop_var plugin_exec_noop_cmd plugin_exec_burst_var`
   three times. Record the median-of-medians for each bench. Compare
   `plugin_exec_noop_var` against the baseline 238.81 ns. Compute
   `(burst_after − noop_cmd_after) / 9` as the per-import cost.

4. **Regression tests.** Run, in this order:
   - `cargo test --features test-helpers --test plugin -- variables`
   - `cargo test -p yosh plugin::host::variables`
   - `cargo test --features test-helpers`
   - `cargo build --release`

   All four must pass. The release build is included to catch
   lifetime issues that `debug_assertions` would mask.

5. **Result write-up.** Append "Appendix A: §4.1 Phase 2 PoC Result"
   to `docs/superpowers/specs/2026-05-08-plugin-perf-report.md`. Two
   templates depending on outcome (see §6 below). Update the matching
   `[plugin-perf §4.1]` entry in `TODO.md` (currently L48).

### 4.2 Negative-result protocol

The PoC may exit early at any of these points:

- **End of Task 1, no signature compiles.** Skip Tasks 2–4. Go
  directly to Task 5 with the failure template. Code reverts to
  baseline.
- **End of Task 3, improvement < 10%.** Skip none, but Task 5 uses
  the failure template and the source change is reverted before commit
  so `main` retains the baseline.
- **End of Task 3, `plugin_exec_noop_cmd` regresses ≥5%.** Same as
  above: failure, revert.
- **Task 4 surfaces lifetime errors elsewhere in the codebase.**
  Failure, revert. Record the call sites that broke as a follow-up
  hazard note in TODO.md.

In every failure mode the spec exits cleanly: no pivot to §4.2 (cached
linker), no pivot to §4.3 (cwasm cache-miss log), no pivot to intern
cache or batch API. The next project is selected fresh from
report §5.2.

## 5. Testing & Verification

**Primary metric (gating):**
- `plugin_exec_noop_var` Criterion median, three-run aggregate.

**Secondary metrics (corroborating):**
- `plugin_exec_noop_cmd` median — must not regress ≥5%.
- `plugin_exec_burst_var` median — expected to improve in proportion
  if the PoC succeeds.
- W-P3 dhat allocation Top-10 — wasmtime String-lift site expected to
  shrink or drop out.

**Regression gates:**
- `cargo test --features test-helpers --test plugin -- variables`
- `cargo test -p yosh plugin::host::variables`
- `cargo test --features test-helpers` (full plugin-feature test
  suite — catches any host-call type ripple)
- `cargo build --release`

**Manual cross-check (success-path only):**
- Rebuild `perf_plugin.wasm`:
  `cargo component build -p perf_plugin --target wasm32-wasip2 --release`
- Rebuild profiling binaries:
  `cargo build --profile profiling --features dhat-heap --bin yosh-dhat`
- Re-run W-P3 dhat:
  `HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat benches/data/plugin_w3.sh`
- Confirm canonical-ABI String-lift allocation site shrinks or leaves
  Top-10.

**Decision matrix:**

| `plugin_exec_noop_var` Δ | `noop_cmd` Δ | Decision |
|---|---|---|
| ≤ −10% | ≥ −5% (no regression) | Success — merge, write success appendix |
| > −10% | any | Failure — revert, write failure appendix |
| any | < −5% (regression) | Failure — revert, write failure appendix |

## 6. Result Templates

### 6.1 Success appendix (template)

> ## Appendix A: §4.1 Phase 2 PoC Result — Success
>
> **Date:** YYYY-MM-DD
> **Commit:** `<sha>`
> **Signature used:** `(name,): (<TYPE>,)` (e.g. `(&str,)`)
>
> ### Measurement
>
> | Bench | Baseline (`48bc83b`) | After | Δ |
> |---|---|---|---|
> | `plugin_exec_noop_cmd` | 111.53 ns | … ns | … |
> | `plugin_exec_noop_var` | 238.81 ns | … ns | … |
> | `plugin_exec_burst_var` | 1,307 ns | … ns | … |
>
> Per-import cost (post): `(burst − cmd) / 9` = … ns/import.
>
> ### Follow-up
>
> Apply the same borrow conversion to all remaining `String`-typed
> host imports in one project: `variables::set`,
> `variables::export-env`, `filesystem::set-cwd`,
> `files::{read-file, read-dir, metadata, write-file, append-file,
> create-dir, remove-file, remove-dir}`, `commands::exec`. New spec
> file: `docs/superpowers/specs/YYYY-MM-DD-plugin-host-import-borrow-rollout-design.md`.

### 6.2 Failure appendix (template)

> ## Appendix A: §4.1 Phase 2 PoC Result — Negative
>
> **Date:** YYYY-MM-DD
> **Commit attempted:** `<sha>`
> **Outcome:** … (e.g. "No `func_wrap` borrow signature compiles" /
> "Compiles but `noop_var` did not improve" / "Compiles but
> `noop_cmd` regressed").
>
> ### Signatures attempted
>
> | Form | Result |
> |---|---|
> | `(&str,)` | … |
> | `WasmStr` | … |
> | … | … |
>
> ### Measurement (if applicable)
>
> | Bench | Baseline | After | Δ |
> |---|---|---|---|
> | … | … | … | … |
>
> ### Hypothesis for null result
>
> … (e.g. "wasmtime 27's typed `func_wrap` always lowers `string`
> WIT type to host `String`; borrowed lifts require dropping to
> `Func::new` with raw canonical-ABI access, which is out of scope
> for this PoC.")
>
> ### Next action
>
> §4.1 closed. Proceed to §4.3 (cwasm cache-miss observability) per
> report §5.2 ordering.

## 7. Risk Register

| Risk | Impact | Mitigation |
|---|---|---|
| wasmtime 27's `func_wrap` does not accept any borrow-shaped string parameter | PoC null result | Task 1 enumerates candidate types and exits cleanly with the failure template |
| Borrow compiles but the lift implementation still allocates internally | Improvement < 10% | Decision matrix gates on measured improvement, not on the source-level change |
| Criterion noise around the ±10% boundary | Indeterminate result | Three runs; aggregate median; if still on the edge, run W-P3 dhat to inspect alloc-site call counts directly |
| Lifetime errors propagate through `HostContext` borrow tree to other paths | Scope creep | Task 1 includes `cargo check --tests --features test-helpers` as the spike's compile gate; surface ripple → revert and treat as failure |
| `noop_cmd` regression (5%+) caused by a thunk shape change | Wrong-direction result | Decision matrix treats this as failure |

## 8. References

- Phase 1 report: `docs/superpowers/specs/2026-05-08-plugin-perf-report.md`
  (especially §3.2 Criterion table, §4.1 finding, §5.2 next-project queue)
- Source under change: `src/plugin/linker.rs`,
  `src/plugin/host/variables.rs`
- Bench harness: `benches/plugin_bench.rs` (the `plugin_exec_*`
  group)
- TODO.md anchor: line 48 — `[plugin-perf §4.1]`
