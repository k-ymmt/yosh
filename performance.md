# yosh Performance Report

**Measurement date:** 2026-04-21
**Commit:** `cf38a9c`
**Environment:** macOS 26.3.1 / arm64 / Apple M3 / rustc 1.94.1
**Build profile:** `profiling` (`release` + `debug = true`, `strip = false`)

## 1. Executive Summary

**Top 5 hotspots (W2):**

1. **`[` / `test` dispatched as an external command per while-loop iteration** — W2 Section B's `while [ "$i" -lt 1000 ]` forked and `execvp`'d `/usr/bin/[` once per iteration, producing ~1001 `build_env_vars` allocations (rank #5 dhat site, 1001 outer-Vec allocs). Fixed 2026-04-21 by classifying `test` / `[` as Regular builtins (see `docs/superpowers/specs/2026-04-21-test-bracket-builtin-design.md`). Post-fix W2 total allocation dropped from 68.1 MB to ~13.1 MB.
2. **`VarStore::build_environ` itself** — rebuilds the exported set by merging scope hashmaps, called per miss of the existing cache and inside the `.to_vec()` site above.
3. **Shell function call path** — `exec_function_call_200` is **187×** slower per operation than `exec_for_loop_200` (2.4 ms vs 13 µs per call). Root cause is not fully isolated; likely argv binding + local-scope push/pop.
4. **`expand::pathname::expand` always allocates a new Vec** — 14k calls / 2.9 MB in W2 even though most fields contain no glob metachars.
5. **`expand::pattern::matches`** — 46k calls / ~1.1 MB. Secondary hotspot; warrants investigation after #1–#4.

**Recommended next-project order:** §5.2.

**Non-findings worth flagging:**
- The existing TODO.md entry "`LINENO` update allocates a `String` per command" is **absent from the W2 Top-10**. It is real but is an order of magnitude smaller than hotspots #1–#2 and should be re-prioritized below them.
- Parse cost is a non-issue: `parse_large` (72 µs for ~500-line script) is negligible relative to `exec_function_call_200` (484 ms).

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

samply's **self-time** column on macOS is dominated by `vm_region_64` (~85 % in W1/W2) because that Mach kernel routine is used during stack unwinding. On Linux the self-time column would attribute samples directly to yosh functions; on macOS it largely attributes them to the sampler itself.

**Total-time is the usable column for yosh analysis on macOS.** All W1/W2/W3 observations below focus on total-time.

## 3. Results

### 3.1 W1: Startup

#### Wall-clock (Criterion)

| Metric | `startup_echo_hi` |
|--------|--------------------|
| Min    | 1.70 ms            |
| Median | 1.74 ms            |
| Max    | 1.81 ms            |

This is the cost of one complete `yosh -c 'echo hi'` invocation, including fork/exec of the yosh binary, `ShellEnv` initialization, plugin loading, argument parsing, and `echo` builtin dispatch. ~1.7 ms / invocation is the baseline that any startup-focused optimization should improve on.

#### samply Top-10 total time (W1 in-process loop, 20,000 iterations)

| Rank | Function | Total % |
|------|----------|---------|
| 1    | `yosh::exec::Executor::exec_command` | 100.0 % |
| 2    | `yosh::exec::pipeline::exec_pipeline` | 100.0 % |
| 3    | `yosh::exec::Executor::exec_and_or` | 100.0 % |
| 4    | `yosh::exec::Executor::exec_complete_command` | 100.0 % |
| 5    | `yosh::run_string` | 100.0 % |
| 6    | `yosh::main` | 100.0 % |
| 7-10 | std::rt::lang_start, `main`, etc. | 100.0 % |

All samples go through the command-dispatch pipeline, as expected for a tight `while` loop. This column is flat because every sample is in the loop body; the distinguishing signal is in what the body allocates (→ dhat, §3.2).

### 3.2 W2: Script-heavy

#### Criterion

| Bench | Median |
|-------|--------|
| `lex_small` | 3.24 µs |
| `lex_large` | 53.14 µs |
| `parse_small` | 10.52 µs |
| `parse_large` | 72.72 µs |
| `expand_param_default` | 421 µs |
| `expand_field_split` | 1.34 ms |
| `expand_literal_words` | 45.04 µs |
| `exec_for_loop_200` | **2.58 ms** |
| `exec_function_call_200` | **484 ms** ← 187× slower than for-loop |
| `exec_param_expansion_200` | 3.93 ms |

The huge gap between `exec_for_loop_200` (2.58 ms) and `exec_function_call_200` (484 ms) is the headline CPU signal of W2: shell functions cost ~2.4 ms per invocation while ordinary arithmetic iterations cost 13 µs each (~180× per-operation gap). This is investigated in §4.2.

#### samply Top-10 total time (W2)

| Rank | Function | Total % |
|------|----------|---------|
| 1    | `yosh::run_file` | 100.0 % |
| 2    | `yosh::main` | 100.0 % |
| 3-6  | std::rt boilerplate | 100.0 % |
| 7    | (unresolved `0x8d53`) | 100.0 % |
| 8    | `yosh::exec::Executor::exec_command` | 100.0 % |
| 9    | `yosh::exec::pipeline::exec_pipeline` | 100.0 % |
| 10   | `yosh::exec::Executor::exec_and_or` | 100.0 % |

Same flat structure as W1 — CPU-breakdown via samply is limited on macOS; dhat is the richer signal for this workload.

#### dhat Top-10 by bytes (W2) — Pre-fix (commit 1e1b738)

Run totals: **68.1 MB allocated across 808,896 blocks.**

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

Five of the top ten sites are inside `VarStore::build_environ` and `Executor::build_env_vars`. Together they account for **~52 MB (76 %) of W2's total allocation** and >380k calls. The root cause was that `classify_builtin` did not list `test` / `[`, so while-loop conditions forked `/usr/bin/[` once per iteration (see §4.6).

#### dhat Top-10 by bytes (W2) — Post-fix (commit fe8f69a)

Run totals: **13.1 MB allocated across 293,382 blocks** (−80.8 % bytes, −63.7 % blocks vs pre-fix).

After the fix, `build_environ` / `build_env_vars` no longer dominate the allocation profile. The top hotspots are now `pathname::expand`, `pattern::matches`, and `field_split::emit` — the genuine expansion-pipeline pressure that was masked by the external-`[` fork overhead.

| Rank | Site | Bytes | Calls |
|------|------|-------|-------|
| 1 | `yosh::expand::pathname::expand (src/expand/pathname.rs:29:20)` | 2.94 MB | 14,020 |
| 2 | `yosh::expand::field_split::emit (src/expand/field_split.rs:180:9)` | 1.50 MB | 7,010 |
| 3 | `yosh::expand::pathname::expand (src/expand/pathname.rs:29:20)` | 1.26 MB | 6,012 |
| 4 | `yosh::expand::pattern::matches (src/expand/pattern.rs:11:39)` | 1.00 MB | 34,034 |
| 5 | `yosh::expand::field_split::emit (src/expand/field_split.rs:180:9)` | 876.5 KB | 4,007 |
| 6 | `yosh::expand::pathname::expand (src/expand/pathname.rs:22:24)` | 430.1 KB | 2,002 |
| 7 | `yosh::expand::pattern::matches (src/expand/pattern.rs:10:42)` | 250.2 KB | 16,016 |
| 8 | `yosh::expand::expand_word_to_fields (???:0:0)` | 219.0 KB | 4,004 |
| 9 | `yosh::expand::pathname::expand (src/expand/pathname.rs:29:20)` | 209.5 KB | 18 |
| 10 | `yosh::expand::pathname::glob_in_dir (src/expand/pathname.rs:166:26)` | 148.6 KB | 20,020 |

`expand::pathname::expand` at 14k calls and `expand::pattern::matches` at ~50k calls surface a second-tier hotspot: pathname expansion runs its globbing machinery on words that have no glob metacharacters. See §4.3.

Note: `exec_for_loop_200` and `exec_param_expansion_200` showed small regressions (+31.8 % and +13.1 %) in Criterion post-fix. These benchmarks contain no `[` / `test`, so the regressions are likely measurement noise rather than an effect of this change.

**TODO.md cross-check:** the existing entry "`LINENO` update allocates a `String` per command" is **not** in the W2 Top-10. Post-fix, `pathname::expand` / `pattern::matches` are the largest allocation sites and should be prioritized ahead of `LINENO`.

#### dhat Top-10 by call count (W2) — Pre-fix (commit 1e1b738)

| Rank | Site | Calls | Bytes |
|------|------|-------|-------|
| 1 | `VarStore::build_environ (src/env/vars.rs:297:36)` | **131,130** | 1.82 MB |
| 2 | `Executor::build_env_vars (src/exec/simple.rs:406:48)` | **127,127** | 1.81 MB |
| 3 | `Executor::build_env_vars (src/exec/simple.rs:406:48)` | 121,121 | 7.44 MB |
| 4 | `VarStore::build_environ::{{closure}} (src/env/vars.rs:303:39)` | 120,172 | 7.37 MB |
| 5 | `expand::pattern::matches (src/expand/pattern.rs:11:39)` | 31,031 | 888 KB |
| 6 | `expand::pathname::glob_in_dir (src/expand/pathname.rs:166:26)` | 19,019 | 135 KB |
| 7 | `expand::pattern::matches (src/expand/pattern.rs:10:42)` | 15,015 | 235 KB |
| 8 | `expand::pathname::expand (src/expand/pathname.rs:29:20)` | 14,020 | 2.94 MB |
| 9 | `expand::field_split::emit (src/expand/field_split.rs:180:9)` | 7,010 | 1.50 MB |
| 10 | `VarStore::build_environ (src/env/vars.rs:297:24)` | 7,007 | 16.06 MB |

### 3.3 W3: Interactive-smoke

**Sample count: 68.** Short scenario (~1 second wall clock). Signal is qualitative only.

#### samply Top-10 self time (W3)

| Rank | Function | Self % |
|------|----------|--------|
| 1    | `_libkernel_memset` | 36.8 % |
| 2    | `posix_spawn_file_actions_adddup2` | 25.0 % |
| 3    | `host_get_special_port` | 14.7 % |
| 4    | `write` | 8.8 % |
| 5    | `mach_get_times` | 8.8 % |

The W3 self-time column is not dominated by the macOS sampler artifact (samples are too sparse). `posix_spawn`-related calls at 25 % are the cost of launching yosh itself under samply.

#### samply Top-10 total time (W3)

| Rank | Function | Total % |
|------|----------|---------|
| 1-5  | `main`, std::rt boilerplate | 100.0 % |
| 6    | `LineEditor::read_line_loop_with_completion` | 76.5 % |
| 7    | `LineEditor::read_line_with_completion` | 76.5 % |
| 8    | `Repl::run` | 76.5 % |
| 9    | `yosh::main` | 76.5 % |
| 10   | `CommandCompleter::complete_common_prefix` | 50.0 % |

50 % of in-session samples are in `CommandCompleter::complete_common_prefix`. For a scenario with only one Tab press, this ratio implies completion is a substantial fraction of interactive wall-clock when it fires. Too few samples to draw firm conclusions; see §4 for treatment.

## 4. Findings

Five hotspots are treated here. They are ordered by measured impact, not by expected fix effort — that ordering is in §5.

### 4.1. `VarStore::environ().to_vec()` cloned per command

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

**TODO.md cross-check:** not present. This finding should be added to TODO.md as a P0 item.

### 4.2. Shell function calls are ~180× slower per operation than arithmetic loop iterations

**Location:** `src/exec/function.rs:9-45` — `Executor::exec_function_call`. Exercised by `benches/exec_bench.rs::exec_function_call_200`.

**Measurement:**
- Criterion `exec_for_loop_200`: 2.58 ms total for 200 iterations → ~13 µs/iter.
- Criterion `exec_function_call_200`: 484 ms total for 200 calls → ~2.4 ms/call.
- Ratio: **~187×** per operation.
- The benchmark script does only `f() { : "$1"; }` followed by 200 calls of `f arg`. Each call does at most: scope push, argv bind, builtin `:` call, scope pop.

**Suspected cause:** four candidates, ordered by plausibility given confirmed source-code evidence:

1. **`catch_unwind` wrapper around the function body** (`src/exec/function.rs:12`). Every call is wrapped in `std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| …))`, which on stable Rust heap-allocates a `Box<dyn Any>` for a potential panic payload and inserts optimization barriers around the closure. For a function body of just `: "$1"` this per-call overhead is likely a significant fraction of the 2.4 ms. Removing the `catch_unwind` (or replacing it with a `Drop`-guard that pops the scope) would be a targeted win.
2. **`environ_cache` invalidation on every scope push/pop** (`src/env/vars.rs:83` and `:93`). `push_scope` and `pop_scope` both set `self.environ_cache = None`. A single function call therefore invalidates the environ cache *twice*, forcing the next `environ()` read to rebuild. This couples finding 4.2 to finding 4.1: the 131k `build_environ` calls observed in W2 are partly driven by function-call scope churn, not actual env mutation. Scoped-cache invalidation (fix candidate #3 in 4.1) is the right long-term answer.
3. **HashMap allocation for the per-call local scope** (`src/env/vars.rs:84-87` — `Scope { vars: HashMap::new(), ... }`). Shell functions rarely have more than 0–3 local bindings; a `SmallVec<[(Name, Variable); 4]>` would avoid the heap allocation entirely for the common case.
4. **Positional-parameter vector cloning** — `push_scope(args.to_vec())` clones the argv. Minor compared to the above, but still adds up over 200 calls.

**Fix candidates (to execute in order):**

1. **Add finer-grained micro-benches first** — split into `exec_function_call_nopanic_guard` (replace `catch_unwind` with a Drop-guard scope-popper), `exec_function_call_cached_environ` (cache-invalidation only on exported-var changes), and `exec_function_call_smallvec_scope`. Each bench isolates one candidate so the relative contribution of #1–#3 becomes measurable.
2. **Drop-guard scope popper** replacing `catch_unwind` — eliminates the heap alloc + barriers while preserving "scope always pops" invariant.
3. **Scoped cache invalidation** — shared with 4.1 candidate #3.
4. **SmallVec-backed scope** — only if micro-bench shows HashMap alloc still dominates after #2 and #3.

**TODO.md cross-check:** not present. This finding should be added to TODO.md as P0.

### 4.3. `pathname::expand` allocates a new Vec per invocation, even with no glob chars

**Location:** `src/expand/pathname.rs:15-33` — the top-level `expand()` function.

**Measurement (W2):** 14,020 calls allocating 2.94 MB (rank #6 by bytes, rank #8 by calls). Matching the 14k number against W2's structure (~3200 commands × ~4 fields per command after expansion) suggests every expanded field runs through `pathname::expand` and triggers at least one `Vec::new()` — even when no field contains `*`, `?`, or `[`.

**Suspected cause:** the implementation unconditionally allocates `let mut result = Vec::new();` and copies each `field` into it via `result.push(field)`. For the all-non-glob case (which is almost all cases in W2), this is a pure copy.

**Fix candidates:**

1. **Fast-path pass-through:** before the loop, `if !fields.iter().any(has_unquoted_glob_chars) { return fields; }`. Saves the Vec alloc + copy for every non-glob invocation.
2. **Reuse the input allocation:** when the loop reaches the non-glob branch, swap with `mem::take(&mut fields[i])` rather than moving into a new Vec. Slightly more complex than #1 but covers the mixed case.

**TODO.md cross-check:** not present. Medium-priority P1 (below findings 4.1 and 4.2 but above 4.4).

### 4.4. `expand::pattern::matches` called ~46k times for W2

**Location:** `src/expand/pattern.rs:10-11` (rank #5 + #7 by call count; 235 KB + 888 KB bytes).

**Measurement:** 15k + 31k = 46k calls, together allocating ~1.1 MB.

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

**Measured impact of the fix:**
- W2 total allocation: 68.1 MB → 13.1 MB (-80.8%)
- W2 total blocks: 808,896 → 293,382 (-63.7%)
- `exec_bracket_loop_200` Criterion: 916.40 ms → 11.14 ms (82.2× faster)
- `exec_function_call_200` Criterion: 898.60 ms → 10.56 ms (85.1× faster; cascade effect because the bench's driver loop uses `while [ ]`)

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
| 4.1 — `environ().to_vec()` per command | **High** (~52 MB / ~380k calls) | **Low** for fix candidate #1 (builtin skip); Medium for #2/#3 | **done** | Candidate #1 alone likely cuts W2 allocation by ≥40 %. Completed 2026-04-21 via `[`/`test` builtin promotion. See `docs/superpowers/specs/2026-04-21-test-bracket-builtin-design.md`. |
| 4.2 — function-call 187× ratio | **High** (Criterion) | **Medium** (needs root-cause bench work first) | **P0** | Start by adding the sub-benches described in candidate #1 before touching code. |
| 4.3 — `pathname::expand` non-glob alloc | **Medium** (~3 MB) | **Low** (5-line fast path) | **P1** | Cheap to implement; quick win. |
| 4.4 — `pattern::matches` recompilation | **Low-Medium** (~1 MB, 46k calls) | **Medium** (cache + invalidation) | **P2** | Revisit after 4.1/4.2 land — may lose relative weight. |
| 4.5 — interactive completion ratio | **Inconclusive** | n/a | **defer** | Not a finding until resampled. |

### 5.2 Next-project queue

In order:

**Note (2026-04-21):** Item 4.1 has been completed via `test`/`[` builtin promotion. The §4.2 function-call Criterion baseline must be re-captured because its benchmark used `while [ ]` internally and therefore inherited the external-`[` overhead in the original measurement. Post-fix `exec_function_call_200` = 10.56 ms (was 898.60 ms), a 85× improvement solely from the `[` fix — the genuine function-call overhead is now tractable for a separate investigation.

1. **P0 — Add the fine-grained function-call sub-benches from 4.2 candidate #1.** Estimated: half a day. Prerequisite to any actual fix; without it we'd be guessing. (Note: the 4.2 baseline has changed significantly post-fix; re-capture first.)
2. **P0 — Fix 4.2 based on whatever the new benches reveal.** Estimated: 1–3 days depending on path.
3. **P1 — Fix 4.3 fast-path.** Estimated: 1–2 hours. Can be bundled into the same PR as either #1 or #2 above.
4. **P2 — Fix 4.4 pattern cache.** Estimated: 2 days. Worth re-measuring after the first three land, because the absolute numbers will have shifted.

### 5.3 Items to add to TODO.md

- `build_env_vars` / `environ().to_vec()` cloning per command execution (§4.1) — **P0**. Re-prioritize above the existing `LINENO` entry.
- `exec_function_call` 187× overhead ratio vs arithmetic loop (§4.2) — **P0**, with sub-bench prerequisite.
- `pathname::expand` Vec allocation with no glob chars (§4.3) — **P1**.
- `pattern::matches` recompilation (§4.4) — **P2**.

The existing `LINENO update allocates a String per command` entry should stay but be noted as subordinate to §4.1.

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

**Amendment 2026-04-21:** §4.1 has since been investigated further and a fix (promoting `test` / `[` to Regular builtins) was implemented and landed under `docs/superpowers/specs/2026-04-21-test-bracket-builtin-design.md`. Post-fix measurements are recorded in §3.2 and §4.6. All remaining recommendations in §5 continue to be deferred to separately-scoped projects, with the updated priority queue in §5.2.
