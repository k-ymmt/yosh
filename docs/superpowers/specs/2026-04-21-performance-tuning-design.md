# Performance Tuning: Measurement Report Design

**Date:** 2026-04-21
**Status:** Approved (design), pending implementation plan
**Scope:** Measurement only — no code fixes in this project

## 1. Goal

Produce a single report file, `performance.md` at the repository root, that identifies the largest performance bottlenecks in yosh through broad, measurement-driven profiling. No fixes are made; findings feed future, separately-scoped projects.

The report must answer: *where is yosh actually spending time and allocating memory, across representative workloads?*

## 2. Scope

### In scope

- Three complementary measurement tools: samply (flame graphs), dhat-rs (allocation tracking), Criterion (micro-benchmarks).
- Three representative workloads: startup, script-heavy, interactive-smoke.
- A single consolidated report at `performance.md`.
- Supporting artifacts: new Criterion benches under `benches/`, workload scripts under `benches/data/`, and a dedicated `[profile.profiling]` build profile.

### Out of scope

- **Any performance fix.** Findings become the input to future projects.
- **CI integration.** The report is produced once, ad hoc.
- **Regression detection infrastructure.** No baseline tracking, no alerts.
- **Human-perceived interactive latency.** Only mechanical profiling of the interactive path; no keystroke-to-pixel timings.

## 3. Workloads

### W1: Startup

- `yosh -c 'echo hi'` invoked as an external process.
- Two variants:
  - **Single-shot:** N=1 with samply to capture the full init path.
  - **Loop:** N=1000 in a wrapper script, wall-clock median recorded, so that samply has enough samples on short-lived runs.
- Purpose: measure the cost of rc-file reading, `ShellEnv` initialization, plugin loading, argument parsing, and builtin dispatch to `echo`.

### W2: Script-heavy

- File: `benches/data/script_heavy.sh`.
- Contents cover the full execution pipeline:
  - A 1000-iteration `for` loop.
  - A function defined once and called 1000 times.
  - Parameter expansion mixing: `${var:-default}`, `${var#prefix}`, command substitution `$(...)`.
  - Redirection: stdout/stderr, `>&2`, at least one here-document.
- Purpose: exercise Lexer, Parser, Expander, and Executor hot paths simultaneously.
- Primary target of both samply and dhat.

### W3: Interactive-smoke

- Driven by `expectrl` against a PTY: startup → `echo hello<CR>` → Tab completion once → up-arrow (history) once → `exit<CR>`.
- Purpose: smoke-profile `LineEditor`, `redraw()`, syntax highlight, and completion paths.
- If PTY timing makes profiling unstable (3 consecutive runs with visibly inconsistent samples), W3 is skipped and the reason is recorded in `performance.md`.

## 4. Tooling

### samply (flame graphs)

- Install: `cargo install samply` (captured as a one-time step in the report, not automated).
- Invocation: `samply record -- ./target/profiling/yosh <workload>`.
- Artifact: `.perf.json` per workload; Top-N function tables and representative screenshots land in `performance.md`.
- macOS-friendly: no sudo required.

### dhat-rs (allocation tracking)

- `dev-dependencies`: `dhat = "0.3"`.
- Feature gate: `[features] dhat-heap = ["dhat"]` in the yosh crate's `Cargo.toml`.
- A dedicated binary at `src/bin/yosh-dhat.rs` sets `#[global_allocator] static ALLOC: dhat::Alloc = dhat::Alloc;` and runs W2.
- Output: `dhat-heap.json`; Top-N allocation sites (file:line, bytes, count) copied into `performance.md`.
- Scoped to a separate binary so that the main `yosh` binary keeps its default allocator untouched.

### Criterion (micro-benchmarks)

- Existing: `lexer_bench`, `parser_bench`, `expand_bench`. Re-run as baseline.
- New:
  - `benches/startup_bench.rs` — in-process startup measurement if feasible, otherwise external-process loop.
  - `benches/exec_bench.rs` — narrow subsets of W2 (loop alone, function-call alone, parameter-expansion alone).
- Invocation: `cargo bench`; result summaries extracted from `target/criterion/*/report/` into `performance.md`.

### Build profile

- New `[profile.profiling]` in `Cargo.toml` inheriting from `release` with `debug = true` and `strip = false`. All tools consume `cargo build --profile profiling` output, keeping release builds unaffected.

## 5. Report structure (`performance.md`)

```
# yosh Performance Report

## 1. Executive Summary
- Measurement date, commit SHA, environment (OS, CPU, Rust version)
- Top 5 hotspots, one line each
- Prioritized candidate fixes (Impact × Effort, 3-7 items)

## 2. Methodology
- Workload definitions (W1/W2/W3)
- Tooling (samply / dhat / Criterion) and reproduction commands
- Build profile

## 3. Results
### 3.1 W1: Startup — wall-clock stats, samply Top-N, startup_bench
### 3.2 W2: Script — samply Top-N, dhat Top-N, exec_bench + existing benches
### 3.3 W3: Interactive Smoke — samply Top-N, or skip reason

## 4. Findings
One section per hotspot:
- Location (file:line)
- Measurement (CPU %, allocated bytes)
- Suspected cause (from code reading)
- Fix candidates (1-3 options, trade-offs)
- De-duplication check against TODO.md

## 5. Recommendations
- Prioritized list (Impact × Effort matrix)
- "Pick up in a future project" queue — the only continuation action from this report

## 6. Reproducibility
- Exact commands to rerun all benches and workloads
- Steps to regenerate the report
```

## 6. Execution outline

Detailed step breakdown is deferred to the implementation plan (writing-plans skill). High-level order:

1. **Setup:** add `[profile.profiling]`, add `dhat-heap` feature + `dev-dependency`, document `cargo install samply`.
2. **Workload definitions:** `benches/data/script_heavy.sh`; W3 expectrl scenario colocated under `benches/` or `tests/`.
3. **New benches:** `benches/startup_bench.rs`, `benches/exec_bench.rs`.
4. **Measurement runs:** samply × 3 workloads, dhat × W2, `cargo bench`.
5. **Authoring:** consolidate into `performance.md` following §5 structure.
6. **Review:** user reviews `performance.md` for plausibility of findings and candidate fixes.

## 7. Risks and mitigations

- **Background load on macOS** (Spotlight, etc.) skews samples → for each wall-clock measurement, run 3 times and report the median with min/max as variance indicators; samply captures are single-pass but cross-checked against the Criterion runs for the same workload.
- **W1 too short-lived for samply** → N=1000 loop variant (shell wrapper script under `benches/data/`) is the primary samply target; single-shot `yosh -c 'echo hi'` is best-effort.
- **W3 PTY timing instability** → skip after 3 runs where the samply Top-5 function list differs between runs; record the skip reason and the divergent Top-5 lists in the report.
- **dhat replaces the global allocator** → isolated in `src/bin/yosh-dhat.rs` so the main binary is untouched.
- **Profiling vs. release divergence** → `performance.md` explicitly states measurements use the `profiling` profile and flags known differences from `release`.

## 8. Deliverables checklist

- [ ] `Cargo.toml` — `[profile.profiling]`, `dhat-heap` feature, `dhat` dev-dependency.
- [ ] `src/bin/yosh-dhat.rs` — dhat-instrumented entry point running W2.
- [ ] `benches/data/script_heavy.sh` — W2 workload script.
- [ ] `benches/data/startup_loop.sh` — W1 N=1000 loop wrapper for samply.
- [ ] `benches/startup_bench.rs` — W1 Criterion bench.
- [ ] `benches/exec_bench.rs` — W2-derived Criterion benches.
- [ ] `benches/interactive_smoke.rs` — W3 PTY scenario driven by `expectrl`.
- [ ] `performance.md` at repository root, following the §5 structure.

## 9. Continuation

After `performance.md` is approved, the "Recommendations" section becomes the input for separately-scoped fix projects. This design project terminates at report delivery.
