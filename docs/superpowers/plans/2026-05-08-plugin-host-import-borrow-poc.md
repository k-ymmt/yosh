# Plugin Host-Import Borrow PoC Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Determine whether changing `variables::get`'s wasmtime `func_wrap` closure from owned `String` to borrowed `&str` reduces the per-host-import canonical-ABI lift cost by ≥10% on `plugin_exec_noop_var` (without regressing `plugin_exec_noop_cmd`).

**Architecture:** Single-call-site PoC. Modify the closure at `src/plugin/linker.rs:77` (granted path) and its symmetric deny stub at `:81`, plus the matching `host_variables_get` / `deny_variables_get` parameter types in `src/plugin/host/variables.rs`. WIT stays unchanged. If the borrow signature does not compile under wasmtime 27 typed `func_wrap`, exit early with a documented negative result and revert; do not pivot to alternate fixes inside this plan.

**Tech Stack:** Rust 1.94.1 / Cargo workspace / wasmtime 27 (component model, sync) / Criterion benches / cargo-component for `perf_plugin.wasm` build (one-time prerequisite).

**Spec:** `docs/superpowers/specs/2026-05-08-plugin-host-import-borrow-design.md`

**Predecessor report:** `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` §4.1, §5.2

---

## File Structure

| Path | Action | Responsibility |
|---|---|---|
| `src/plugin/linker.rs:77-83` | Modify | The granted + deny `vars.func_wrap("get", ...)` closures. Sole call sites that determine the host-side argument type for `variables::get`. |
| `src/plugin/host/variables.rs:9-22` | Modify | `host_variables_get` and `deny_variables_get` function signatures (parameter `name: String` → `name: &str`). Bodies already only use `&name`, so the change is mechanical. |
| `src/plugin/host/variables.rs:78-82` | Modify | Single unit test (`variables_get_denied_when_env_null`) that calls `host_variables_get` directly; argument type follows the function signature change. |
| `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` | Append | New "Appendix A: §4.1 Phase 2 PoC Result" section with measurement table (success template) or signature-attempt log (failure template). |
| `TODO.md:48` | Modify | Update the `[plugin-perf §4.1]` line to reflect the PoC outcome and link to Appendix A. |

No new files. No new tests beyond the existing `tests/plugin.rs` regression coverage and the in-module `variables_get_denied_when_env_null`.

---

## Task 1: Re-run baseline benchmarks for fresh comparison

**Why first:** the report's 238.81 ns / 111.53 ns / 1,307 ns numbers are from commit `48bc83b`. We're now at `6dfeb86`. Re-confirm those are still the operative baseline before any change.

**Files:**
- Read-only: none modified

**Prerequisites (one-time, skip if already done):**

- [ ] **Step 1: Build `perf_plugin.wasm` if missing**

```bash
ls target/wasm32-wasip2/release/perf_plugin.wasm 2>/dev/null \
  || cargo component build -p perf_plugin --target wasm32-wasip2 --release
```

Expected: file exists (either pre-existing, or built fresh with no errors).

- [ ] **Step 2: Run the three target benches once**

```bash
cargo bench --bench plugin_bench --features test-helpers -- \
  plugin_exec_noop_cmd plugin_exec_noop_var plugin_exec_burst_var
```

Expected: each bench prints a `time: [low median high]` line. No errors.

- [ ] **Step 3: Record the baseline medians**

Read `target/criterion/plugin_exec_noop_cmd/new/estimates.json`,
`target/criterion/plugin_exec_noop_var/new/estimates.json`, and
`target/criterion/plugin_exec_burst_var/new/estimates.json`. The
median (in ns) is at `["median"]["point_estimate"]`.

Save the three numbers to a scratch note (file: `target/perf/poc-baseline.txt`):

```bash
mkdir -p target/perf
{
  echo "noop_cmd: $(jq '.median.point_estimate' target/criterion/plugin_exec_noop_cmd/new/estimates.json) ns"
  echo "noop_var: $(jq '.median.point_estimate' target/criterion/plugin_exec_noop_var/new/estimates.json) ns"
  echo "burst_var: $(jq '.median.point_estimate' target/criterion/plugin_exec_burst_var/new/estimates.json) ns"
} > target/perf/poc-baseline.txt
cat target/perf/poc-baseline.txt
```

Expected: three lines, each with a numeric value. `noop_var` should be in the 200–270 ns range, `noop_cmd` in 100–130 ns. If wildly different (e.g. >2× the report values), stop and investigate environmental drift before proceeding.

`target/perf/` is gitignored — no commit needed for this scratch note.

---

## Task 2: Spike — change the granted `func_wrap` closure to a borrowed-string signature

**Files:**
- Modify: `src/plugin/linker.rs:77-79` (the granted-path `vars.func_wrap("get", ...)` only — leave the deny stub alone for this task to keep the diff small)

- [ ] **Step 1: Edit `src/plugin/linker.rs:77-79`**

Current code:

```rust
        vars.func_wrap("get", |mut store, (name,): (String,)| {
            Ok((host_variables_get(store.data_mut(), name),))
        })?;
```

Change to:

```rust
        vars.func_wrap("get", |mut store, (name,): (&str,)| {
            Ok((host_variables_get(store.data_mut(), name),))
        })?;
```

Note: `host_variables_get` still takes `String` at this point — the next step verifies whether the closure-level change compiles in isolation, before we touch the host function.

- [ ] **Step 2: Compile-check**

```bash
cargo check --features test-helpers 2>&1 | tee /tmp/borrow-spike.log
```

Three possible outcomes:

**(a) Compiles cleanly OR fails only because `host_variables_get` expects `String` but receives `&str`** → success-shaped: the typed `func_wrap` accepts `&str`. Proceed to Task 3.

**(b) Fails with a wasmtime trait-bound error like `the trait \`ComponentNamedList\` is not implemented for \`(&str,)\`\`** → wasmtime 27 typed `func_wrap` does not accept this borrow shape. Proceed to Step 3 (try alternates).

**(c) Some other failure** → record the diagnostic and treat as (b).

- [ ] **Step 3: If outcome (b), try alternate borrow types in order**

For each candidate below, edit `src/plugin/linker.rs:77` to use it, then re-run `cargo check --features test-helpers 2>&1 | tee /tmp/borrow-spike.log` and record the result. Stop at the first one that produces outcome (a).

Candidate signatures to try:

```rust
// Candidate 1
(name,): (wasmtime::component::__internal::String,)
// Candidate 2 (if the type WasmStr exists in wasmtime 27)
(name,): (wasmtime::component::WasmStr,)
// Candidate 3
(name,): (std::borrow::Cow<'_, str>,)
```

If wasmtime 27 does not export any of these, search wasmtime 27 docs.rs for the `Lift` trait implementations on borrow-shaped string types. The first compatible one is the answer.

- [ ] **Step 4: Decision point**

If any candidate produced outcome (a) → keep that signature in `linker.rs:77` and proceed to Task 3.

If every candidate failed → revert `linker.rs:77` to the original `(String,)` form, save `/tmp/borrow-spike.log` as `target/perf/poc-spike-failure.log`, and skip directly to Task 8 (failure write-up). Do not proceed to Task 3.

```bash
# Only run on revert path:
git checkout -- src/plugin/linker.rs
cp /tmp/borrow-spike.log target/perf/poc-spike-failure.log
```

- [ ] **Step 5: Note the chosen signature**

Append to `target/perf/poc-baseline.txt`:

```bash
echo "chosen signature: <TYPE>" >> target/perf/poc-baseline.txt
```

Where `<TYPE>` is the actual type that compiled (`&str`, `wasmtime::component::WasmStr`, etc.).

---

## Task 3: Propagate borrow to `host_variables_get` and `deny_variables_get`

**Files:**
- Modify: `src/plugin/host/variables.rs:9-22`
- Modify: `src/plugin/linker.rs:81-83` (deny stub, to keep symmetric)

- [ ] **Step 1: Edit `host_variables_get` in `src/plugin/host/variables.rs:9-15`**

Current:

```rust
pub fn host_variables_get(
    ctx: &mut HostContext,
    name: String,
) -> Result<Option<String>, ErrorCode> {
    let env = ctx.bound_env()?;
    Ok(env.vars.get(&name).map(|s| s.to_string()))
}
```

Change to:

```rust
pub fn host_variables_get(
    ctx: &mut HostContext,
    name: &str,
) -> Result<Option<String>, ErrorCode> {
    let env = ctx.bound_env()?;
    Ok(env.vars.get(name).map(|s| s.to_string()))
}
```

(Two changes: `name: String` → `name: &str`, and `&name` → `name` on the `vars.get` call.)

- [ ] **Step 2: Edit `deny_variables_get` in `src/plugin/host/variables.rs:17-22`**

Current:

```rust
pub fn deny_variables_get(
    _ctx: &mut HostContext,
    _name: String,
) -> Result<Option<String>, ErrorCode> {
    Err(ErrorCode::Denied)
}
```

Change to:

```rust
pub fn deny_variables_get(
    _ctx: &mut HostContext,
    _name: &str,
) -> Result<Option<String>, ErrorCode> {
    Err(ErrorCode::Denied)
}
```

- [ ] **Step 3: Edit the deny-stub `func_wrap` in `src/plugin/linker.rs:81-83`**

Current:

```rust
        vars.func_wrap("get", |mut store, (name,): (String,)| {
            Ok((deny_variables_get(store.data_mut(), name),))
        })?;
```

Change the closure-arg tuple type to match the granted path's chosen signature from Task 2 Step 5. If the chosen type is `&str`:

```rust
        vars.func_wrap("get", |mut store, (name,): (&str,)| {
            Ok((deny_variables_get(store.data_mut(), name),))
        })?;
```

- [ ] **Step 4: Update the in-module unit test**

In `src/plugin/host/variables.rs:78-82`, the test calls `host_variables_get(&mut ctx, "PATH".into())`. Drop the `.into()`:

```rust
    #[test]
    fn variables_get_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_variables_get(&mut ctx, "PATH");
        assert_eq!(result, Err(ErrorCode::Denied));
    }
```

- [ ] **Step 5: Compile-check both default and test profiles**

```bash
cargo check --features test-helpers
cargo check --tests --features test-helpers
cargo build --release
```

Expected: all three succeed with no errors. Warnings about unused imports are acceptable. The release build is included to catch lifetime issues that `debug_assertions` would mask.

If any of the three fails → revert all changes from Tasks 2 and 3, treat as a Task 2 outcome (b) failure, and proceed to Task 8 with the failure template.

```bash
# Revert path only:
git checkout -- src/plugin/linker.rs src/plugin/host/variables.rs
```

---

## Task 4: Run regression tests

**Files:** none modified

- [ ] **Step 1: Plugin variables tests**

```bash
cargo test --features test-helpers --test plugin -- variables
```

Expected: all matched tests pass.

- [ ] **Step 2: In-module host variables tests**

```bash
cargo test -p yosh plugin::host::variables
```

Expected: `variables_get_denied_when_env_null` passes.

- [ ] **Step 3: Full plugin-feature test suite**

```bash
cargo test --features test-helpers
```

Expected: all tests pass. This is the broadest gate — catches any unforeseen ripple from changing the host-call signatures.

If any of the three fails → revert all changes from Tasks 2 and 3, save the failing log to `target/perf/poc-regression-failure.log`, and proceed to Task 8 with the failure template.

```bash
# Revert path only:
git checkout -- src/plugin/linker.rs src/plugin/host/variables.rs
```

---

## Task 5: Criterion measurement (3 runs)

**Files:** none modified

- [ ] **Step 1: First Criterion run**

```bash
cargo bench --bench plugin_bench --features test-helpers -- \
  plugin_exec_noop_cmd plugin_exec_noop_var plugin_exec_burst_var
```

Expected: three benches complete successfully.

- [ ] **Step 2: Capture run-1 medians**

```bash
{
  echo "=== run 1 ==="
  echo "noop_cmd: $(jq '.median.point_estimate' target/criterion/plugin_exec_noop_cmd/new/estimates.json) ns"
  echo "noop_var: $(jq '.median.point_estimate' target/criterion/plugin_exec_noop_var/new/estimates.json) ns"
  echo "burst_var: $(jq '.median.point_estimate' target/criterion/plugin_exec_burst_var/new/estimates.json) ns"
} >> target/perf/poc-after.txt
```

- [ ] **Step 3: Second Criterion run**

```bash
cargo bench --bench plugin_bench --features test-helpers -- \
  plugin_exec_noop_cmd plugin_exec_noop_var plugin_exec_burst_var
```

- [ ] **Step 4: Capture run-2 medians**

```bash
{
  echo "=== run 2 ==="
  echo "noop_cmd: $(jq '.median.point_estimate' target/criterion/plugin_exec_noop_cmd/new/estimates.json) ns"
  echo "noop_var: $(jq '.median.point_estimate' target/criterion/plugin_exec_noop_var/new/estimates.json) ns"
  echo "burst_var: $(jq '.median.point_estimate' target/criterion/plugin_exec_burst_var/new/estimates.json) ns"
} >> target/perf/poc-after.txt
```

- [ ] **Step 5: Third Criterion run**

```bash
cargo bench --bench plugin_bench --features test-helpers -- \
  plugin_exec_noop_cmd plugin_exec_noop_var plugin_exec_burst_var
```

- [ ] **Step 6: Capture run-3 medians**

```bash
{
  echo "=== run 3 ==="
  echo "noop_cmd: $(jq '.median.point_estimate' target/criterion/plugin_exec_noop_cmd/new/estimates.json) ns"
  echo "noop_var: $(jq '.median.point_estimate' target/criterion/plugin_exec_noop_var/new/estimates.json) ns"
  echo "burst_var: $(jq '.median.point_estimate' target/criterion/plugin_exec_burst_var/new/estimates.json) ns"
} >> target/perf/poc-after.txt
cat target/perf/poc-after.txt
```

Expected: nine numbers total (3 benches × 3 runs).

---

## Task 6: Compute decision and aggregate the three-run median-of-medians

**Files:** none modified (analysis-only)

- [ ] **Step 1: Compute median-of-medians**

For each bench, take the three captured medians and pick the middle one (median-of-medians). Read `target/perf/poc-baseline.txt` and `target/perf/poc-after.txt`. Calculate Δ% as `(after − baseline) / baseline * 100`.

Worked example: if baseline `noop_var` = 238.81 ns, and three after-runs gave 210.0, 213.5, 211.2 → median-of-medians = 211.2 ns → Δ = (211.2 − 238.81) / 238.81 × 100 = −11.6%.

- [ ] **Step 2: Apply the decision matrix**

From spec §5:

| `noop_var` Δ | `noop_cmd` Δ | Decision |
|---|---|---|
| ≤ −10% | ≥ −5% (no regression beyond noise) | **Success** |
| > −10% (improvement smaller than 10%) | any | **Failure** |
| any | < −5% (regression worse than 5%) | **Failure** |

Note on sign convention: a negative Δ is an improvement (faster). `≥ −5%` for `noop_cmd` means "did not get more than 5% slower" — values from `−5%` upward (including positive Δ that are themselves regressions) are acceptable for `noop_cmd`'s side gate; the only failure trigger is `noop_cmd` regressing by more than 5%.

- [ ] **Step 3: Append the decision to scratch notes**

```bash
{
  echo "=== decision ==="
  echo "noop_var Δ%: <COMPUTED>"
  echo "noop_cmd Δ%: <COMPUTED>"
  echo "burst_var Δ%: <COMPUTED>"
  echo "verdict: <success|failure>"
} >> target/perf/poc-after.txt
```

- [ ] **Step 4: Branch**

If verdict is **success** → proceed to Task 7 (success write-up).

If verdict is **failure** → revert source changes:

```bash
git checkout -- src/plugin/linker.rs src/plugin/host/variables.rs
```

Then proceed to Task 8 (failure write-up).

---

## Task 7: Success write-up — Appendix A and TODO.md update

**Files:**
- Modify: `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` (append Appendix A)
- Modify: `TODO.md:48`

**Run only if Task 6's verdict is "success".** If failure, skip to Task 8.

- [ ] **Step 1: Append Appendix A to the report**

Append the following section to the END of `docs/superpowers/specs/2026-05-08-plugin-perf-report.md`. Replace placeholder values with the actual numbers from `target/perf/poc-baseline.txt` and `target/perf/poc-after.txt`.

```markdown

## Appendix A: §4.1 Phase 2 PoC Result — Success

**Date:** YYYY-MM-DD
**Commit:** `<sha-after-task-7>`
**Signature used:** `(name,): (<TYPE>,)` (from Task 2 Step 5 record)

### Measurement

| Bench | Baseline (commit `48bc83b`) | After (median-of-3 runs) | Δ |
|---|---|---|---|
| `plugin_exec_noop_cmd` | 111.53 ns | <X> ns | <Δ%> |
| `plugin_exec_noop_var` | 238.81 ns | <Y> ns | <Δ%> |
| `plugin_exec_burst_var` | 1,307 ns | <Z> ns | <Δ%> |

Per-import cost (post): `(burst_var − noop_cmd) / 9` = <V> ns/import (baseline: ~127 ns/import).

dhat W-P3 cross-check: canonical-ABI String-lift allocation site <removed from / still present in> the Top-10 by call count.

### Follow-up

Apply the same borrow conversion to all remaining `String`-typed host imports in one project: `variables::set`, `variables::export-env`, `filesystem::set-cwd`, `files::{read-file, read-dir, metadata, write-file, append-file, create-dir, remove-file, remove-dir}`, `commands::exec`. New spec: `docs/superpowers/specs/YYYY-MM-DD-plugin-host-import-borrow-rollout-design.md`.
```

- [ ] **Step 2: Manual W-P3 dhat cross-check (corroborating, not gating)**

The Criterion result already determines verdict. This step confirms the alloc-level cause of the improvement and produces a one-line note for the appendix. If the verdict is borderline (Δ between −10% and −12%), this step's result can also be cited in the appendix to bolster the success claim.

```bash
cargo component build -p perf_plugin --target wasm32-wasip2 --release
cargo build --profile profiling --features dhat-heap --bin yosh-dhat

mkdir -p /tmp/yosh-perf-home/.config/yosh
cat > /tmp/yosh-perf-home/.config/yosh/plugins.lock <<EOF
[[plugin]]
name = "perf"
path = "$(pwd)/target/wasm32-wasip2/release/perf_plugin.wasm"
enabled = true
capabilities = ["variables:read", "hooks:pre_prompt", "hooks:pre_exec", "hooks:post_exec"]
EOF

mkdir -p target/perf
HOME=/tmp/yosh-perf-home ./target/profiling/yosh-dhat benches/data/plugin_w3.sh
mv dhat-heap.json target/perf/dhat-plugin-w3-after.json
python3 scripts/perf/dhat_top_n.py target/perf/dhat-plugin-w3-after.json 10 \
    > target/perf/dhat-plugin-w3-after.md
cat target/perf/dhat-plugin-w3-after.md
```

Expected: the Top-10 by call count should NOT contain a wasmtime canonical-ABI String-lift allocator (anything in `wasmtime_environ::component::translate` or `wasmtime::component::func::typed` lowering paths). Compare against the report §3.2 Top-10 table where the alloc site (if it was visible) appears.

Record one line in `target/perf/poc-after.txt`:

```bash
echo "dhat W-P3 string-lift site present in Top-10: <yes|no>" >> target/perf/poc-after.txt
```

This finding will be added to Appendix A's Measurement section as a footnote.

- [ ] **Step 3: Update `TODO.md:48`**

Find the line currently reading:

```markdown
- [ ] Plugin perf P0: Host-call argument copies — `variables::get` host-import costs ~127 ns/call due to wasmtime canonical-ABI String allocation; `burst_var` (10 imports) is linear at 1,307 ns. Investigate `&str`-typed `func_wrap` closures to remove the wasm→host string copy. See `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` §4.1 (`src/plugin/linker.rs`).
```

Replace it with:

```markdown
- [ ] Plugin perf P0 (rollout): Apply `<TYPE>`-typed `func_wrap` to remaining `String`-arg host imports (`variables::set`, `variables::export-env`, `filesystem::set-cwd`, `files::*`, `commands::exec`). PoC on `variables::get` succeeded with <Δ%> on `plugin_exec_noop_var`; see `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` Appendix A. New rollout spec to be authored separately (`src/plugin/linker.rs`, `src/plugin/host/`).
```

Replace `<TYPE>` and `<Δ%>` with the actual values.

- [ ] **Step 4: Commit success-path changes together**

```bash
git add src/plugin/linker.rs src/plugin/host/variables.rs \
        docs/superpowers/specs/2026-05-08-plugin-perf-report.md TODO.md
git commit -m "$(cat <<'EOF'
feat(plugin): borrow string in variables::get host import (Phase 2 PoC)

Switch the wasmtime func_wrap closure for variables::get from owned
String to <TYPE>, eliminating the canonical-ABI host-side allocation
on every host-import crossing. Measured improvement on
plugin_exec_noop_var: <Δ%> (baseline 238.81 ns → <Y> ns, median of
three Criterion runs). plugin_exec_noop_cmd unchanged within noise.
host_variables_get and deny_variables_get adopt the matching &str
parameter; the WIT interface and on-disk plugin binaries are
unaffected.

PoC scope: variables::get only. Rollout to remaining String-typed
host imports tracked in TODO.md and a separate follow-up spec.

Spec: docs/superpowers/specs/2026-05-08-plugin-host-import-borrow-design.md
Plan: docs/superpowers/plans/2026-05-08-plugin-host-import-borrow-poc.md
Result appendix: report.md Appendix A.

Original prompt: "docs/superpowers/specs/2026-05-08-plugin-perf-report.md
から優先的に対応をした方が良いものからして下さい"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Skip Task 8.

---

## Task 8: Failure write-up — Appendix A and TODO.md update

**Files:**
- Modify: `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` (append Appendix A — failure template)
- Modify: `TODO.md:48`

**Run only if any of the failure exits triggered:** Task 2 Step 4 (no signature compiles), Task 3 Step 5 (compile breaks elsewhere), Task 4 (regression tests fail), Task 6 Step 4 (improvement < 10% or `noop_cmd` regression).

Source changes must already be reverted by this point. Confirm:

- [ ] **Step 1: Verify clean working tree on source files**

```bash
git status -s src/plugin/linker.rs src/plugin/host/variables.rs
```

Expected: empty output (no modifications). If output is non-empty, run:

```bash
git checkout -- src/plugin/linker.rs src/plugin/host/variables.rs
```

- [ ] **Step 2: Append failure-template Appendix A to the report**

Append to the END of `docs/superpowers/specs/2026-05-08-plugin-perf-report.md`. Fill placeholders with values gathered during the failed run.

```markdown

## Appendix A: §4.1 Phase 2 PoC Result — Negative

**Date:** YYYY-MM-DD
**Commit attempted:** `<sha-of-spec-commit-or-current-HEAD>`
**Outcome:** <one of: "No `func_wrap` borrow signature compiles" | "Compiled but `plugin_exec_noop_var` improvement <10%" | "Compiled but `plugin_exec_noop_cmd` regressed >5%" | "Regression test failure">

### Signatures attempted (Task 2)

| Form | Result |
|---|---|
| `(&str,)` | <pass/compile-error: brief excerpt> |
| `(wasmtime::component::__internal::String,)` | <if tried> |
| `(wasmtime::component::WasmStr,)` | <if tried> |
| `(std::borrow::Cow<'_, str>,)` | <if tried> |

(Omit rows for signatures not attempted.)

### Measurement (if a signature compiled)

| Bench | Baseline | After (median of 3) | Δ |
|---|---|---|---|
| `plugin_exec_noop_cmd` | 111.53 ns | <X> ns | <Δ%> |
| `plugin_exec_noop_var` | 238.81 ns | <Y> ns | <Δ%> |
| `plugin_exec_burst_var` | 1,307 ns | <Z> ns | <Δ%> |

(Omit table if no signature compiled.)

### Hypothesis for null result

<One paragraph. Examples: "wasmtime 27's typed `func_wrap` lowers WIT `string` to host `String` regardless of closure-arg-type ascription; borrowed lifts require dropping to `Func::new` with raw canonical-ABI access, which was out of scope per the PoC spec." OR "Borrow signature compiled but lift implementation still allocates internally; per-import cost unchanged.">

### Next action

§4.1 closed by this PoC. Per report §5.2, next project is §4.3 (cwasm cache-miss observability) followed by §4.2 (cached linker).
```

- [ ] **Step 3: Update `TODO.md:48`**

Replace the existing `[plugin-perf §4.1]` line with:

```markdown
- [ ] Plugin perf §4.1 closed (Phase 2 PoC negative result, YYYY-MM-DD): borrowed-string `func_wrap` route did not yield the ≥10% target on `plugin_exec_noop_var`. See `docs/superpowers/specs/2026-05-08-plugin-perf-report.md` Appendix A for signature attempts and measured numbers. Next plugin-perf project is §4.3 (cwasm cache-miss observability).
```

- [ ] **Step 4: Commit failure-path changes**

```bash
git add docs/superpowers/specs/2026-05-08-plugin-perf-report.md TODO.md
git commit -m "$(cat <<'EOF'
docs(plugin-perf): record §4.1 Phase 2 PoC negative result

The borrowed-string `func_wrap` PoC for variables::get did not meet
the ≥10% target on plugin_exec_noop_var (or did not compile). Source
changes reverted; only the report appendix and TODO.md are updated.
§4.1 is closed by this attempt; next plugin-perf project is §4.3
(cwasm cache-miss observability) per report §5.2.

Spec: docs/superpowers/specs/2026-05-08-plugin-host-import-borrow-design.md
Plan: docs/superpowers/plans/2026-05-08-plugin-host-import-borrow-poc.md
Outcome details: report.md Appendix A.

Original prompt: "docs/superpowers/specs/2026-05-08-plugin-perf-report.md
から優先的に対応をした方が良いものからして下さい"

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Done — Final state checklist

After Task 7 (success) OR Task 8 (failure), the repository should satisfy:

- [ ] `cargo build --release` succeeds
- [ ] `cargo test --features test-helpers` passes
- [ ] `git status -s` is clean (or only untracked `target/perf/*` scratch files, which are gitignored)
- [ ] HEAD commit message references both this plan and the predecessor spec
- [ ] `TODO.md:48` reflects the outcome (rollout in progress on success; §4.1 closed on failure)
- [ ] Report Appendix A exists and is filled in (success or failure template — never left blank or with placeholders)
