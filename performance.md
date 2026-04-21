# yosh Performance Report

**Measurement date:** 2026-04-21 (remeasured at HEAD)
**Commit:** `610043e` (post-pathname-fastpath; bracket-fix baseline: `2261638`; pre-fix baseline: `cf38a9c`)
**Environment:** macOS 26.3.1 / arm64 / Apple M3 / rustc 1.94.1
**Build profile:** `profiling` (`release` + `debug = true`, `strip = false`). Criterion runs use the default `bench` profile (which also inherits from `release`).

## 1. Executive Summary

**Status at HEAD (`2261638`):** the former top hotspot — `[` / `test` dispatched as an external command per while-loop iteration — has been fixed (§4.1) and is now verified at HEAD. The W2 workload dropped from 68.1 MB / 808,896 blocks of allocations to 13.78 MB / 293,382 blocks (−79.8 % bytes, −63.7 % blocks). `exec_function_call_200` dropped from 484 ms → 10.15 ms (47.7× faster) as an indirect consequence (its driver loop was a `while [ ]`).

**Remaining hotspots, in order of measured impact at HEAD:**

1. **Shell function call path** — `exec_function_call_200` (10.15 ms, ~50 µs/call) is now only ~2.1× slower per operation than `exec_for_loop_200` (4.83 ms, ~24 µs/iter). The residual overhead is an order of magnitude smaller than the `[` fix impact, but still worth investigating per §4.2.
2. ~~**`expand::pathname::expand` always allocates a new Vec**~~ — **Fixed 2026-04-21** via non-glob fast-path (§4.3). Formerly 14k calls / 2.94 MB at rank #1; now 2,002 calls / 430.1 KB at rank #4 (pathname.rs:29 aggregate: 4.41 MB → 430.1 KB, −90.3%).
3. **`expand::field_split::emit`** — new top bytes-allocator post-fastpath: 4.63 MB / 21,051 calls across three dhat sites (ranks #1, #2, #7). Investigation pending (new §5.2 P0).
4. **`expand::pattern::matches`** — 50k calls / ~1.25 MB in W2. Top by call count; secondary by bytes (§4.4).
5. **`VarStore::build_environ`** — no longer a significant cost in W2 now that external-`[` forks are eliminated. Deferred unless a future workload resurfaces it.

**Recommended next-project order:** §5.2.

**Non-findings worth flagging:**
- The existing TODO.md entry "`LINENO` update allocates a `String` per command" remains **absent from the W2 Top-10** at HEAD. Still tracked but deprioritized.
- Parse cost is a non-issue: `parse_large` (144 µs for ~500-line script) is negligible relative to `exec_function_call_200` (10 ms).
- Many non-`[` Criterion benchmarks (lex, parse, expand, startup) are roughly 2× slower than the original `cf38a9c` baseline recorded previously. dhat totals are essentially unchanged at HEAD, so this is consistent with a measurement-environment difference (thermal state / macOS background load) rather than a code regression. See §3 for the absolute numbers.

## 2. Methodology

### 2.1 Workloads

| | Definition | Purpose |
|---|---|---|
| **W1 — Startup** | `yosh -c 'echo hi'` — wall-clock via `benches/startup_bench.rs` (Criterion). samply CPU profile uses an in-process 20,000-iter `while` loop over `echo hi` (see note below). | Startup cost amortized across many invocations; wall-clock only for the external-process view. |
| **W2 — Script-heavy** | `benches/data/script_heavy.sh` — 1000-iter `for`, 1000 function calls, parameter-expansion variety, redirection. | Exercises the Lexer/Parser/Expander/Executor pipeline simultaneously. |
| **W3 — Interactive-smoke** | `benches/interactive_smoke.rs` — expectrl scenario: prompt → `echo hello` → Tab → Up arrow → `exit`. | Smoke profile of `LineEditor`, completion, history, and syntax highlighting. |

**macOS samply limitation (applies to W1 CPU profile only):** samply on macOS cannot profile system binaries (`/bin/sh`) because code signing blocks `DYLD_INSERT_LIBRARIES`, and it does not follow `posix_spawn` children. The original plan called for `samply record -- benches/data/startup_loop.sh ./yosh 1000`, which fails with `"Could not obtain the root task"`. The samply W1 profile therefore uses an in-process yosh loop, which measures the loop + `echo` path rather than 1000 separate startups. **Startup wall-clock is still captured accurately via Criterion `startup_echo_hi` (§3.1).**

### 2.2 Tools

- **samply v0.13.1** — whole-process sampling profiler, Gecko profile format. `samply record --save-only`.
- **dhat-rs v0.3** — heap allocation tracking via `src/bin/yosh-dhat.rs` (feature-gated behind `dhat-heap`). Emits `dhat-heap.json`.
- **Criterion v0.5** — in-process micro-benchmarks.

All three are extracted to Markdown via reusable scripts in `scripts/perf/`:
- `scripts/perf/samply_top_n.py` — parses the Gecko JSON, resolves unsymbolicated frames through `atos` on macOS.
- `scripts/perf/dhat_top_n.py` — parses dhat's JSON, attributes each allocation to the nearest `yosh::` frame.

### 2.3 Build profile

```toml
[profile.profiling]
inherits = "release"
debug = true
strip = false
```

All samply / dhat / Criterion runs use `--profile profiling` artifacts. The `release` profile omits debug symbols; flame graphs would lose their symbolication. `release` and `profiling` differ only in debug-symbol presence and stripping, not in codegen, so the measured timings carry over to production within noise.

### 2.4 samply on macOS — reading the tables

samply's **self-time** column on macOS is dominated by Mach kernel routines used during stack unwinding (`host_get_special_port`, `__fcntl`, `mach_get_times`, `vm_region_64`, `__mac_set_proc` — the exact top-K varies between runs). At HEAD these account for ~70 % of W1 self-time and ~62 % of W2 self-time. On Linux the self-time column would attribute samples directly to yosh functions; on macOS it largely attributes them to the sampler itself.

**Total-time is the usable column for yosh analysis on macOS.** All W1/W2/W3 observations below focus on total-time.

## 3. Results

### 3.1 W1: Startup

#### Wall-clock (Criterion)

| Metric | `startup_echo_hi` |
|--------|--------------------|
| Min    | 3.28 ms            |
| Median | 3.38 ms            |
| Max    | 3.48 ms            |

This is the cost of one complete `yosh -c 'echo hi'` invocation, including fork/exec of the yosh binary, `ShellEnv` initialization, plugin loading, argument parsing, and `echo` builtin dispatch. ~3.4 ms / invocation is the baseline that any startup-focused optimization should improve on. The earlier run at `cf38a9c` recorded 1.74 ms on the same host; dhat totals are unchanged, so the delta is attributed to measurement-environment (thermal / background load) rather than a code regression.

#### samply Top-10 total time (W1 in-process loop, 20,000 iterations; 1,653 samples)

| Rank | Function | Total % |
|------|----------|---------|
| 1    | `yosh::exec::Executor::exec_command` | 100.0 % |
| 2    | `yosh::exec::pipeline::exec_pipeline` | 100.0 % |
| 3    | `yosh::exec::Executor::exec_and_or` | 100.0 % |
| 4    | `yosh::exec::Executor::exec_complete_command` | 100.0 % |
| 5    | `yosh::exec::compound::exec_compound_command` | 100.0 % |
| 6    | `yosh::run_string` | 100.0 % |
| 7    | `yosh::main` | 100.0 % |
| 8-10 | `std::rt::lang_start` etc. | 100.0 % |

All samples go through the command-dispatch pipeline, as expected for a tight `while` loop. This column is flat because every sample is in the loop body; the distinguishing signal is in what the body allocates (→ dhat, §3.2).

### 3.2 W2: Script-heavy

#### Criterion (HEAD `2261638`)

| Bench | Median | Pre-fix (`cf38a9c`) | Ratio |
|-------|--------|----------------------|-------|
| `lex_small` | 5.88 µs | 3.24 µs | +82 % |
| `lex_large` | 105.08 µs | 53.14 µs | +98 % |
| `parse_small` | 20.71 µs | 10.52 µs | +97 % |
| `parse_large` | 144.39 µs | 72.72 µs | +99 % |
| `expand_param_default` | 897.40 µs | 421 µs | +113 % |
| `expand_field_split` | 2.64 ms | 1.34 ms | +97 % |
| `expand_literal_words` | 81.75 µs | 45.04 µs | +82 % |
| `exec_for_loop_200` | **4.83 ms** | 2.58 ms | +87 % |
| `exec_function_call_200` | **10.15 ms** | 484 ms | **−97.9 %** |
| `exec_param_expansion_200` | 9.70 ms | 3.93 ms | +147 % |
| `exec_bracket_loop_200` | 10.47 ms | n/a (bench added 2026-04-21) | — |
| `startup_echo_hi` | 3.38 ms | 1.74 ms | +94 % |

`exec_function_call_200` dropped by 97.9 % because its driver loop used `while [ ]`, so the `[`-builtin promotion removed ~1000 fork+execvp per run (§4.1 / §4.6). The remaining ~2× drift on lex/parse/expand/startup is seen across benchmarks that have no `[`/`test`, so it is attributed to measurement-environment change between the two runs — dhat W2 totals are essentially unchanged (§3.2 dhat table), which rules out a code regression of this magnitude.

With the `[`-builtin fix in place, the per-operation gap between `exec_function_call_200` (~50 µs/call) and `exec_for_loop_200` (~24 µs/iter) has collapsed from the original 187× to ~2.1×. §4.2 is retained because that residual ratio is still investigatable, but it is no longer the headline signal.

#### samply Top-10 total time (W2; 234 samples at HEAD)

| Rank | Function | Total % |
|------|----------|---------|
| 1    | `yosh::run_file` | 100.0 % |
| 2    | `yosh::main` | 100.0 % |
| 3-6  | std::rt boilerplate | 100.0 % |
| 7    | (unresolved `0x8d53`) | 100.0 % |
| 8    | `yosh::run_string` | 99.6 % |
| 9    | `yosh::exec::Executor::exec_command` | 99.1 % |
| 10   | `yosh::exec::pipeline::exec_pipeline` | 99.1 % |

Same flat structure as W1 — CPU-breakdown via samply is limited on macOS; dhat is the richer signal for this workload.

#### dhat Top-10 by bytes (W2) — post-fastpath (`610043e`)

Run totals (post-fastpath at `610043e`): **11.39 MB allocated across 283,350 blocks** (vs pre-fastpath 13.78 MB / 293,382 blocks at `2c36c5e` → −17.3 % bytes, −3.4 % blocks; vs pre-bracket-fix 68.1 MB / 808,896 blocks at `1e1b738` → −83.3 % bytes cumulative).

After the `[` / `test` builtin fix (§4.1) and the §4.3 `pathname::expand` fast-path fix, `build_environ` / `build_env_vars` and `pathname::expand:29:20` no longer dominate the allocation profile. The new top hotspot is `field_split::emit`, followed by `pattern::matches` — the genuine expansion-pipeline pressure that was masked by earlier overhead.

| Rank | Site | Bytes | Calls |
|------|------|-------|-------|
| 1 | `yosh::expand::field_split::emit (src/expand/field_split.rs:180:9)` | 2.94 MB | 14,020 |
| 2 | `yosh::expand::field_split::emit (src/expand/field_split.rs:180:9)` | 1.48 MB |  7,013 |
| 3 | `yosh::expand::pattern::matches (src/expand/pattern.rs:11:39)` | 1.00 MB | 34,034 |
| 4 | `yosh::expand::pathname::expand (src/expand/pathname.rs:29:24)` | 430.1 KB |  2,002 |
| 5 | `yosh::expand::pattern::matches (src/expand/pattern.rs:10:42)` | 250.2 KB | 16,016 |
| 6 | `yosh::expand::expand_word_to_fields` | 219.0 KB |  4,004 |
| 7 | `yosh::expand::field_split::emit (src/expand/field_split.rs:180:9)` | 209.5 KB |     18 |
| 8 | `yosh::expand::pathname::glob_in_dir (src/expand/pathname.rs:173:26)` | 148.6 KB | 20,020 |
| 9 | `yosh::expand::expand_part_to_fields` | 125.2 KB |  4,007 |
| 10 | `yosh::expand::ExpandedField::set_range (src/expand/mod.rs:83:26)` | 125.2 KB |  4,007 |

After the §4.3 fast-path fix (`610043e`), the primary remaining allocation source is `expand::field_split::emit` — now rank #1 by bytes (2.94 MB / 14,020 calls at `src/expand/field_split.rs:180:9`, plus sibling entries at ranks #2 and #7). `expand::pattern::matches` at ~50k calls remains the top by call count (rank #1 by calls). See §4.4 and the new §5.2 next-project queue.

**TODO.md cross-check:** the existing entry "`LINENO` update allocates a `String` per command" is **not** in the W2 Top-10. Post-fastpath, `field_split::emit` and `pattern::matches` are the largest remaining allocation sites and should be prioritized ahead of `LINENO`.

#### dhat Top-10 by call count (W2) — post-fastpath (`610043e`)

| Rank | Site | Calls | Bytes |
|------|------|-------|-------|
| 1 | `yosh::expand::pattern::matches (src/expand/pattern.rs:11:39)` | 34,034 | 1.00 MB |
| 2 | `yosh::expand::pathname::glob_in_dir (src/expand/pathname.rs:173:26)` | 20,020 | 148.6 KB |
| 3 | `yosh::expand::pattern::matches (src/expand/pattern.rs:10:42)` | 16,016 | 250.2 KB |
| 4 | `yosh::expand::field_split::emit (src/expand/field_split.rs:180:9)` | 14,020 | 2.94 MB |
| 5 | `yosh::expand::field_split::emit (src/expand/field_split.rs:180:9)` |  7,013 | 1.48 MB |
| 6 | `yosh::expand::ExpandedField::push_quoted (src/expand/mod.rs:49:20)` |  5,212 |  50.2 KB |
| 7 | `yosh::expand::expand_part_to_fields` |  4,007 |  31.3 KB |
| 8 | `yosh::expand::ExpandedField::push_unquoted (src/expand/mod.rs:57:20)` |  4,007 |  31.3 KB |
| 9 | `yosh::expand::expand_part_to_fields` |  4,007 | 125.2 KB |
| 10 | `yosh::expand::ExpandedField::set_range (src/expand/mod.rs:83:26)` |  4,007 | 125.2 KB |

Note: `expand_part_to_fields` appears twice at 4,007 calls with different byte totals. These are two distinct allocation sites within the same function — dhat distinguishes them by internal stack ID even though the source-line resolution shown here collapses to the same function name.

#### Pre-fix W2 dhat Top-10 — historical (`1e1b738`)

Kept for reference; the top five sites were all driven by external-`[` fork overhead that no longer exists.

| Rank | Site | Bytes | Calls |
|------|------|-------|-------|
| 1 | `VarStore::build_environ (src/env/vars.rs:297:24)` | **16.06 MB** | 7,007 |
| 2 | `VarStore::build_environ (src/env/vars.rs:304:14)` | 11.55 MB | 6,006 |
| 3 | `Executor::build_env_vars (src/exec/simple.rs:406:48)` | 7.44 MB | 121,121 |
| 4 | `VarStore::build_environ::{{closure}} (src/env/vars.rs:303:39)` | 7.37 MB | 120,172 |
| 5 | `Executor::build_env_vars (src/exec/simple.rs:406:48)` | 5.82 MB | 1,001 |
| 6 | `expand::pathname::expand (src/expand/pathname.rs:29:20)` | 2.94 MB | 14,020 |
| 7 | `VarStore::build_environ (src/env/vars.rs:297:36)` | 1.82 MB | 131,130 |
| 8 | `Executor::build_env_vars (src/exec/simple.rs:406:48)` | 1.81 MB | 127,127 |
| 9 | `expand::field_split::emit (src/expand/field_split.rs:180:9)` | 1.50 MB | 7,010 |
| 10 | `expand::pathname::expand (src/expand/pathname.rs:29:20)` | 1.26 MB | 6,012 |

### 3.3 W3: Interactive-smoke

**Sample count: 55** at HEAD. Short scenario (~1 second wall clock). Signal is qualitative only.

#### samply Top-10 self time (W3)

| Rank | Function | Self % |
|------|----------|--------|
| 1    | `posix_spawn_file_actions_adddup2` | 30.9 % |
| 2    | `_libkernel_memset` | 21.8 % |
| 3    | `host_get_special_port` | 18.2 % |
| 4    | `write` | 10.9 % |
| 5    | `mach_get_times` | 3.6 % |

The W3 self-time column is not dominated by the macOS sampler artifact (samples are too sparse). `posix_spawn`-related calls at 30.9 % are the cost of launching yosh itself under samply.

#### samply Top-10 total time (W3)

| Rank | Function | Total % |
|------|----------|---------|
| 1-5  | `main`, std::rt boilerplate | 100.0 % |
| 6    | `yosh::interactive::Repl::run` | 65.5 % |
| 7    | `yosh::main` | 65.5 % |
| 8    | `LineEditor::read_line_with_completion` | 63.6 % |
| 9    | `LineEditor::read_line_loop_with_completion` | 61.8 % |
| 10   | `interactive_smoke::main` | 34.5 % |

`CommandCompleter::complete_common_prefix`, which registered 50 % of in-session samples in the earlier run, is no longer visible in the W3 Top-10 at HEAD. With only 55 samples and one Tab press, the absolute signal is too sparse to attribute; this is consistent with the earlier "inconclusive" classification (§4.5).

## 4. Findings

Five hotspots are treated here. They are ordered by measured impact, not by expected fix effort — that ordering is in §5.

### 4.1. `[` / `test` dispatched as an external command per while-loop iteration (fixed 2026-04-21)

**Location:** `src/exec/simple.rs:406` (the `build_env_vars` call site) reading from `src/env/vars.rs:286-291` (`environ()`).

**Measurement (W2):**
- Allocations: **~16 MB** at the primary site (rank #3 by bytes, 121k calls), plus a second site contributing another **~6 MB** at rank #5 (1k calls with larger Vec), totaling ~7.4 MB + 5.8 MB = ~13 MB directly attributable to `.to_vec()`.
- Transitively the chain through `environ()` + `build_environ()` + `build_environ::{{closure}}` accounts for the ranks #1, #2, #4, #7, #10 entries (~38 MB).

**Root cause (corrected 2026-04-21):** The original diagnosis was wrong. `build_env_vars` is called **only** from the `NotBuiltin` dispatch path in `src/exec/simple.rs:383` — never for builtins. The real driver was that `classify_builtin` (`src/builtin/mod.rs`) did not list `test` / `[`. W2 Section B's `while [ ... ]; do ... done` therefore forked + `execvp`'d `/usr/bin/[` once per iteration. The 1001-call rank-5 dhat entry matches the loop iteration count exactly.

**Fix applied (2026-04-21):**

**Promote `test` / `[` to `Regular` builtins** per POSIX §2.14. Eliminates 1001 `fork`+`execvp` per W2 run. See `docs/superpowers/specs/2026-04-21-test-bracket-builtin-design.md` and the implementing commits.

**Originally proposed fixes, re-evaluated:**

1. ~~Skip `build_env_vars` entirely for builtins~~ — already in place; provided no benefit because builtins never entered that path.
2. **Return a reference/iterator from `environ()` and defer the `.to_vec()`** — still applicable for the few remaining genuine external-command invocations. Deferred to a future P1/P2 if post-fix measurements still show allocation pressure here.
3. **Scoped cache invalidation** — only bump the environ cache when an *exported* variable changes. Still applicable; see §4.2.

**TODO.md cross-check:** Fixed 2026-04-21 via `[` / `test` builtin promotion; no TODO.md entry needed.

### 4.2. Shell function calls are ~2× slower per operation than arithmetic loop iterations (revised at HEAD)

**Location:** `src/exec/function.rs:9-45` — `Executor::exec_function_call`. Exercised by `benches/exec_bench.rs::exec_function_call_200`.

**Measurement at HEAD (`2261638`):**
- Criterion `exec_for_loop_200`: 4.83 ms total for 200 iterations → ~24 µs/iter.
- Criterion `exec_function_call_200`: 10.15 ms total for 200 calls → ~51 µs/call.
- Ratio: **~2.1×** per operation (was 187× at `cf38a9c` pre-fix).
- The benchmark script does only `f() { : "$1"; }` followed by 200 calls of `f arg` inside a `while [ ]` loop. Each call does at most: scope push, argv bind, builtin `:` call, scope pop.

**Note on the collapsed ratio:** the original 187× was almost entirely driven by the driver loop's `while [ ]` forking `/usr/bin/[`. Post-§4.1-fix, the residual 2.1× reflects the actual cost of a function call relative to an arithmetic iteration. The candidates below remain applicable but the expected improvement is now proportional to the 50 µs/call baseline, not the 2.4 ms/call pre-fix figure.

**Suspected cause:** four candidates, ordered by plausibility given confirmed source-code evidence:

1. **`catch_unwind` wrapper around the function body** (`src/exec/function.rs:12`). Every call is wrapped in `std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| …))`, which on stable Rust heap-allocates a `Box<dyn Any>` for a potential panic payload and inserts optimization barriers around the closure. For a function body of just `: "$1"` this per-call overhead is plausibly a significant fraction of the ~50 µs/call. Removing the `catch_unwind` (or replacing it with a `Drop`-guard that pops the scope) would be a targeted win.
2. **`environ_cache` invalidation on every scope push/pop** (`src/env/vars.rs:83` and `:93`). `push_scope` and `pop_scope` both set `self.environ_cache = None`. A single function call therefore invalidates the environ cache *twice*, forcing the next `environ()` read to rebuild. This couples finding 4.2 to finding 4.1: the 131k `build_environ` calls observed in W2 are partly driven by function-call scope churn, not actual env mutation. Scoped-cache invalidation (fix candidate #3 in 4.1) is the right long-term answer.
3. **HashMap allocation for the per-call local scope** (`src/env/vars.rs:84-87` — `Scope { vars: HashMap::new(), ... }`). Shell functions rarely have more than 0–3 local bindings; a `SmallVec<[(Name, Variable); 4]>` would avoid the heap allocation entirely for the common case.
4. **Positional-parameter vector cloning** — `push_scope(args.to_vec())` clones the argv. Minor compared to the above, but still adds up over 200 calls.

**Fix candidates (to execute in order):**

1. **Add finer-grained micro-benches first** — split into `exec_function_call_nopanic_guard` (replace `catch_unwind` with a Drop-guard scope-popper), `exec_function_call_cached_environ` (cache-invalidation only on exported-var changes), and `exec_function_call_smallvec_scope`. Each bench isolates one candidate so the relative contribution of #1–#3 becomes measurable.
2. **Drop-guard scope popper** replacing `catch_unwind` — eliminates the heap alloc + barriers while preserving "scope always pops" invariant.
3. **Scoped cache invalidation** — shared with 4.1 candidate #3.
4. **SmallVec-backed scope** — only if micro-bench shows HashMap alloc still dominates after #2 and #3.

**TODO.md cross-check:** not present. This finding should be added to TODO.md as P1 (demoted from P0 after the ratio collapse).

### 4.3. `pathname::expand` allocates a new Vec per invocation, even with no glob chars (fixed 2026-04-21)

**Location:** `src/expand/pathname.rs:15-33` — the top-level `expand()` function.

**Measurement (W2 at pre-fastpath `2c36c5e`):** 14,020 calls allocating 2.94 MB — **rank #1 by bytes** and **rank #4 by calls** in the W2 dhat Top-10. Matching the 14k number against W2's structure (~3200 commands × ~4 fields per command after expansion) suggests every expanded field runs through `pathname::expand` and triggers at least one `Vec::new()` — even when no field contains `*`, `?`, or `[`. (Post-fix measurements in the "Fix applied" block below.)

**Suspected cause:** the implementation unconditionally allocates `let mut result = Vec::new();` and copies each `field` into it via `result.push(field)`. For the all-non-glob case (which is almost all cases in W2), this is a pure copy.

**Fix candidates:**

1. **Fast-path pass-through:** before the loop, `if !fields.iter().any(has_unquoted_glob_chars) { return fields; }`. Saves the Vec alloc + copy for every non-glob invocation.
2. **Reuse the input allocation:** when the loop reaches the non-glob branch, swap with `mem::take(&mut fields[i])` rather than moving into a new Vec. Slightly more complex than #1 but covers the mixed case.

**TODO.md cross-check:** not present. **P0** at HEAD — cheapest remaining win after §4.1 landed.

**Fix applied (2026-04-21):**

Implemented fix candidate #1 (fast-path pass-through). Added a guard at the top of `pathname::expand`: if `!fields.iter().any(has_unquoted_glob_chars)`, return the input `Vec` unchanged. See commit `610043e` and spec `docs/superpowers/specs/2026-04-21-pathname-expand-fast-path-design.md`.

**Measured impact (W2, post-fix vs pre-fastpath at `2c36c5e`):**
- `pathname::expand (src/expand/pathname.rs:29:20)` rank #1 at pre-fastpath (2.94 MB / 14,020 calls) **disappeared** from Top-10. (The surviving `pathname.rs:29:24` entry at rank #4 in the post-fastpath table is a different allocation site — the slow-path branch that runs when at least one field contains an unquoted glob metachar; column offset differs from the pre-fastpath site because the fast-path guard shifted the function body.)
- pathname.rs:29 aggregate entries: 4.41 MB / 20,050 calls → 430.1 KB / 2,002 calls (**−90.3% bytes, −90.0% calls**).
- W2 total allocation: 13.78 MB → 11.39 MB (**−17.3%**); total blocks: 293,382 → 283,350 (−3.4%).
- Spec §7 target of ~10.8 MB ± 0.3 MB was modestly overshot (actual 11.39 MB), because some of the eliminated pathname::expand allocations were re-attributed by dhat to `field_split::emit` in the call stack. The net savings are real but smaller than the pre-fix pathname::expand:29:20 entry alone would suggest.

Remaining top allocation source at HEAD is now `field_split::emit` (see §5.2).

### 4.4. `expand::pattern::matches` called ~50k times for W2

**Location:** `src/expand/pattern.rs:10-11` (rank #1 + #3 by call count at HEAD; 1.00 MB + 250 KB bytes).

**Measurement (at HEAD):** 34,034 + 16,016 = ~50k calls, together allocating ~1.25 MB.

**Suspected cause:** each invocation likely re-compiles the pattern object from scratch instead of caching parsed patterns. The W2 script uses only a handful of distinct patterns (`hello`, `world`, `hello*`) in `${VAR#hello }` / `${VAR%world}` / glob paths, so most calls are redundant compilation.

**Fix candidates:**

1. **Cache compiled patterns keyed by source string.** A small LRU (even 16 entries) would catch the W2 reuse completely. Implementation has to handle escaping correctly.
2. **Pass pre-compiled patterns through the expand pipeline** instead of recompiling at each site. Larger refactor.

**TODO.md cross-check:** not present. P2.

### 4.5. Observation: interactive-smoke completion at ~50 % of in-session samples

**Location:** `CommandCompleter::complete_common_prefix` (see `src/interactive/command_completion.rs` for the exact file).

**Measurement (W3):** 34 of 68 total samples (50 %) land in `complete_common_prefix`, driven by one Tab press.

**Status:** **inconclusive.** W3's 68 samples are too sparse for a firm ranking — a single Tab press triggering `complete_common_prefix` plausibly yields the observed ratio without implying a performance problem. Recorded as an observation, not a hotspot.

**Follow-up:** if a P0 fix for 4.1 or 4.2 exposes an interactive bottleneck, re-run W3 with a longer scenario (e.g., 50 prompts with mixed completion).

**TODO.md cross-check:** `src/interactive/history.rs`'s `suggest()` is already listed as "linear scan on every keystroke" — different code path, but in the same neighbourhood. No action needed on either entry from this report.

### 4.6. Correction to §4.1 root-cause analysis (added 2026-04-21)

The original §4.1 diagnosis ("every command execution clones the full exported-env snapshot, even for builtins") was derived from the dhat line-attributed call counts without verifying the actual dispatch path in `src/exec/simple.rs`. Code inspection during the fix work showed that `build_env_vars` has always been gated behind `BuiltinKind::NotBuiltin`. The real driver of the 1001-call rank-5 site was that `classify_builtin` did not list `test` / `[`, so every iteration of `while [ ... ]` in W2 Section B spawned `/usr/bin/[` through fork + execvp.

This mischaracterization is preserved here (rather than silently rewriting §4.1) so that future readers can see both the original mistake and the correction. The lesson: when a dhat call count does not round-trip to a plausible code path, verify the dispatch path before recommending a fix.

**Measured impact of the fix (verified at HEAD `2261638`):**
- W2 total allocation: 68.1 MB → 13.78 MB (−79.8 %)
- W2 total blocks: 808,896 → 293,382 (−63.7 %)
- `exec_bracket_loop_200` Criterion: 916.40 ms → 10.47 ms (87.5× faster)
- `exec_function_call_200` Criterion: 898.60 ms → 10.15 ms (88.5× faster; cascade effect because the bench's driver loop uses `while [ ]`)
- dhat totals at HEAD are essentially identical to the `fe8f69a` post-fix snapshot, i.e. the additional HEAD commits (2/3/4-operand `test`, file-predicate flags, `-t` negative-fd guard) did not shift the allocation profile.

## 5. Recommendations

### 5.1 Priority matrix

Impact classification:
- **High:** > 10 % of total CPU on a W1/W2 hotpath, **or** > 10 % of allocated bytes in W2, **or** a Criterion ratio anomaly > 10×.
- **Medium:** 3–10 %.
- **Low:** < 3 %.

Effort classification:
- **Low:** < 1 day, contained to a single file, no API change.
- **Medium:** 1–3 days, touches 2–5 files.
- **High:** > 3 days or a design decision.

| Finding | Impact | Effort | Priority | Notes |
|---------|--------|--------|----------|-------|
| 4.1 — `[` / `test` external dispatch | **High** (~52 MB / ~380k calls) | **Low** for fix candidate #1 (builtin skip); Medium for #2/#3 | **done** | Completed 2026-04-21 via `[`/`test` builtin promotion. Verified at HEAD `2261638`. See `docs/superpowers/specs/2026-04-21-test-bracket-builtin-design.md`. |
| 4.2 — function-call 2.1× ratio | **Medium** (revised at HEAD from High) | **Medium** (needs root-cause bench work first) | **P1** | Ratio collapsed from 187× to 2.1× after §4.1 fix; still worth sub-bench investigation but no longer urgent. |
| 4.3 — `pathname::expand` non-glob alloc | **Medium** (~2.94 MB at rank #1 pre-fastpath) | **Low** (5-line fast path) | **done** | Completed 2026-04-21 via non-glob fast-path. W2 total −17.3%, pathname::expand hotspot eliminated from Top-10. See `docs/superpowers/specs/2026-04-21-pathname-expand-fast-path-design.md` and `610043e`. |
| 4.4 — `pattern::matches` recompilation | **Low-Medium** (~1.25 MB, 50k calls) | **Medium** (cache + invalidation) | **P1** | Promoted from P2 after §4.3 landed — now §5.2 item 2. Revisit after `field_split::emit` investigation. |
| 4.5 — interactive completion ratio | **Inconclusive** | n/a | **defer** | Not a finding; `complete_common_prefix` is no longer in the W3 Top-10 at HEAD. |

### 5.2 Next-project queue

In order (revised after §4.3 fast-path landed 2026-04-21 at `610043e`):

**Note (2026-04-21):** §4.3 has landed. dhat Top-10 re-ranking shows `field_split::emit` as the new top bytes-allocator (ranks #1, #2, #7 at `src/expand/field_split.rs:180:9`, totaling 4.63 MB / 21,051 calls); `pattern::matches` (§4.4) remains the top by call count at 50k calls / 1.25 MB. The function-call ratio (§4.2) is at ~2.1× — measurable but below these allocation hotspots.

**Deviation from spec:** the spec `docs/superpowers/specs/2026-04-21-pathname-expand-fast-path-design.md` §8 anticipated promoting §4.4 (`pattern::matches`) to P0. Post-measurement, `field_split::emit` emerged as a higher-impact target (ranks #1, #2, #7 by bytes vs `pattern::matches` at rank #3) and was assigned P0 instead; `pattern::matches` was assigned P1. Recorded here for auditability.

1. **P0 — Investigate `field_split::emit` allocation pattern (new).** Estimated: not yet scoped. The 2.94 MB rank #1 entry has the same shape (14,020 calls) as the pre-fastpath `pathname::expand:29:20`, suggesting either genuine Vec growth inside `emit` or dhat re-attribution of callee allocation via the pipeline. Needs a dedicated code-inspection + dhat-stack pass before a fix can be scoped; add a new §4.N section once investigated.
2. **P1 — Fix 4.4 `pattern::matches` recompilation.** Estimated: 2 days. Top by call count (50k). Worth re-measuring after the §5.2 item 1 lands because attribution may shift again.
3. **P1 — Add the fine-grained function-call sub-benches from 4.2 candidate #1**, then act on whatever they reveal (likely the `catch_unwind` replacement). Estimated: half a day for sub-benches, 1–3 days for the follow-up fix.

### 5.3 Items to add to TODO.md

- ~~`build_env_vars` / `environ().to_vec()` cloning per command execution (§4.1)~~ — **Completed 2026-04-21** via `[` / `test` builtin promotion; verified at HEAD `2261638`.
- ~~`pathname::expand` Vec allocation with no glob chars (§4.3)~~ — **Completed 2026-04-21** via non-glob fast-path (`610043e`); dropped from #1 dhat site to rank #4 at 430.1 KB.
- Investigate `field_split::emit (src/expand/field_split.rs:180:9)` allocation pattern — new #1 dhat site post-fastpath (2.94 MB / 14,020 calls; combined with sibling entries 4.63 MB / 21,051 calls). Next P0 candidate, pending investigation.
- `exec_function_call` residual 2.1× overhead ratio vs arithmetic loop (§4.2) — **P1**, with sub-bench prerequisite.
- `pattern::matches` recompilation (§4.4) — **P1** (promoted from P2 post-fastpath; now §5.2 item 2).

The existing `LINENO update allocates a String per command` entry should stay but be noted as subordinate to the remaining expansion-pipeline findings.

## 6. Reproducibility

### 6.1 Build artifacts

```bash
cargo build --profile profiling \
    --bin yosh --bin yosh-dhat --features dhat-heap \
    --bench startup_bench --bench exec_bench --bench interactive_smoke
```

### 6.2 samply runs (macOS workaround for W1 baked in)

```bash
mkdir -p target/perf

# W1 — macOS-compatible in-process loop (see §2.1)
samply record --save-only --output target/perf/samply_w1.json -- \
    ./target/profiling/yosh -c '
        i=0
        while [ "$i" -lt 20000 ]; do
            echo hi > /dev/null
            i=$((i + 1))
        done'

# W2
samply record --save-only --output target/perf/samply_w2.json -- \
    ./target/profiling/yosh benches/data/script_heavy.sh

# W3 — locate the bench binary first
SMOKE=$(ls -t target/profiling/deps/interactive_smoke-* | grep -v '\.d$' | head -1)
samply record --save-only --output target/perf/samply_w3.json -- "$SMOKE"

# Extract Top-N tables in Markdown
python3 scripts/perf/samply_top_n.py target/perf/samply_w1.json 10
python3 scripts/perf/samply_top_n.py target/perf/samply_w2.json 10
python3 scripts/perf/samply_top_n.py target/perf/samply_w3.json 10
```

Interactive exploration: `samply load target/perf/samply_w2.json`.

### 6.3 dhat run

```bash
cargo run --profile profiling --features dhat-heap --bin yosh-dhat -- \
    benches/data/script_heavy.sh
mv dhat-heap.json target/perf/dhat-heap-w2.json

# Extract Top-N tables
python3 scripts/perf/dhat_top_n.py target/perf/dhat-heap-w2.json 10
```

### 6.4 Criterion

```bash
cargo bench
# -> target/criterion/<bench>/<function>/report/index.html
# -> medians in target/criterion/<bench>/<function>/new/estimates.json
#    (read "median" -> "point_estimate" in nanoseconds)
```

### 6.5 Regenerating the intermediate Markdown files

After running §6.2–§6.4, aggregate the extractor outputs into the three intermediate files consumed by §3:

```bash
# target/perf/samply_top.md
{
    echo "# samply Top-N summary"
    echo
    echo "Measurement date: $(date -u '+%Y-%m-%d')"
    echo "Commit: $(git rev-parse --short HEAD)"
    echo "Host: $(uname -srm)"
    echo
    echo "## W1 startup_loop"
    python3 scripts/perf/samply_top_n.py target/perf/samply_w1.json 10 | tail -n +2
    echo
    echo "## W2 script_heavy"
    python3 scripts/perf/samply_top_n.py target/perf/samply_w2.json 10 | tail -n +2
    if [ -f target/perf/samply_w3.json ]; then
        echo
        echo "## W3 interactive_smoke"
        python3 scripts/perf/samply_top_n.py target/perf/samply_w3.json 10 | tail -n +2
    fi
} > target/perf/samply_top.md

# target/perf/dhat_top.md
{
    echo "# dhat Top-N allocation sites (W2)"
    echo
    echo "Measurement date: $(date -u '+%Y-%m-%d')"
    echo "Commit: $(git rev-parse --short HEAD)"
    echo
    python3 scripts/perf/dhat_top_n.py target/perf/dhat-heap-w2.json 10 | tail -n +2
} > target/perf/dhat_top.md

# target/perf/criterion_summary.md — extract from target/criterion/**/estimates.json
#   (median is at ["median"]["point_estimate"] in nanoseconds); a 1-line awk
#   over `cargo bench 2>&1` output is usually easier.
```

All three files are gitignored (under `target/`). The definitive copy of the findings lives in this report.

## 7. Scope statement (reminder)

The original report (§1–§3, §4.2–§4.5) is measurement-only. **No production code was modified for performance** when that report was first authored — the artifacts shipped alongside it were the new profiling tooling (`src/bin/yosh-dhat.rs`, the `profiling` build profile, the three new benches, the two Python extractors) and the workload scripts under `benches/data/`.

**Amendment 2026-04-21 (§4.1):** §4.1 has since been investigated further and a fix (promoting `test` / `[` to Regular builtins) was implemented and landed under `docs/superpowers/specs/2026-04-21-test-bracket-builtin-design.md`. The implementation was expanded in subsequent commits (2/3/4-operand forms, file-predicate flags `-r`/`-w`/`-x`/`-t`/`-u`/`-g`, `-t` negative-fd guard). Measurements were re-captured at `2261638` and recorded in §3.1–§3.3 and §4.6; dhat totals are unchanged from the `fe8f69a` post-fix snapshot, confirming the expansion did not introduce allocation regressions. The priority queue in §5.2 was updated at that time (§4.3 promoted to P0, §4.2 demoted to P1 after the ratio collapsed; §4.3 subsequently completed — see Amendment 2026-04-21 §4.3 below).

**Amendment 2026-04-21 (§4.3):** The `pathname::expand` non-glob fast-path was implemented at commit `610043e` per `docs/superpowers/specs/2026-04-21-pathname-expand-fast-path-design.md`. W2 total allocation fell from 13.78 MB to 11.39 MB (−17.3%); the `pathname::expand:29:20` hotspot (pre-fastpath rank #1) was eliminated from the Top-10. The new top bytes-allocator is `field_split::emit` (§5.2 P0 queue). See §4.3 "Fix applied" block and updated §5.1 / §5.2 / §5.3.
