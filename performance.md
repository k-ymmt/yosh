# yosh Performance Report

**Measurement date:** 2026-04-21
**Commit:** `cf38a9c`
**Environment:** macOS 26.3.1 / arm64 / Apple M3 / rustc 1.94.1
**Build profile:** `profiling` (`release` + `debug = true`, `strip = false`)

## 1. Executive Summary

_(Populated in §4/§5; see those sections for the full ranking.)_

yosh's W2 (script-heavy) execution is dominated by per-command environ reconstruction: `VarStore::build_environ` and `Executor::build_env_vars` together account for over 60 % of the ~68 MB allocated during a single run of `script_heavy.sh`, called 100k+ times each. The function-call path is a second major hotspot: `exec_function_call_200` runs ~190× slower than the comparable for-loop bench.

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

#### dhat Top-10 by bytes (W2)

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

Five of the top ten sites are inside `VarStore::build_environ` and `Executor::build_env_vars`. Together they account for **~52 MB (76 %) of W2's total allocation** and >380k calls.

#### dhat Top-10 by call count (W2)

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

`expand::pathname::expand` at 14k calls and `expand::pattern::matches` at 46k calls (ranks 5 + 7) surface a second-tier hotspot: pathname expansion runs its globbing machinery on words that have no glob metacharacters. See §4.3.

**TODO.md cross-check:** the existing entry "`LINENO` update allocates a `String` per command" is **not** in the W2 Top-10. `build_environ` / `build_env_vars` are much larger and should be prioritized ahead of `LINENO`.

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
