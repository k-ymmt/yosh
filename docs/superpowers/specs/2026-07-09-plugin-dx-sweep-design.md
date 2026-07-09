# Plugin Author DX Sweep — Design (Runtime Limits Phase 2)

Date: 2026-07-09
Status: Approved

## 1. Context

Phase 2 of the plugin runtime limits work, per
`docs/superpowers/specs/2026-07-07-plugin-runtime-limits-design.md` §7.
The `yosh plugin run` / `yosh plugin test` harness landed 2026-05-12
(`2026-05-12-plugin-dev-test-runner-design.md`) with several DX
promises deferred to TODO.md. This sweep closes them, plus the
closely-related harness items tracked in the same TODO section.

Scope (12 items, TODO.md "Future: Plugin System Enhancements"):

1. Harness-level errors bypass `--format json` (~L446)
2. `--cap` empty fallback double-reads/compiles the wasm (~L447)
3. `test --format json` failure lines lack `step`/`expected`/`got` (~L448)
4. `log` wiring promised by old spec §6 never landed (~L449)
5. Troubleshooting hint strings not implemented (~L450)
6. `files_write` expectations compare length only, not content (~L451)
7. No end-to-end sandbox-mode (real FS) scenario test (~L452)
8. `yosh plugin run --watch` (~L444)
9. `RunnerError::{Trap, Timeout}` variants are dead code (~L453)
10. CLI-only types in `lib.rs` are `pub` (~L454)
11. `set_cwd` empty-path error-code drift vs production (~L456)
12. `Expect::denied` scenario key deferred from spec §5 (~L457)

Plus the §7 mandate: surface phase 1's two new error kinds (`timeout`,
memory-cap trap hint) in the harness's JSON/hint output.

Constraints settled during brainstorming:

- **Zero new dependencies.** `--watch` polls mtime; tracing is a
  hand-rolled stderr tracer gated by an env var (supersedes the old
  spec's `RUST_LOG`/`log`-crate promise — recorded here, item 4).
- **Unified error type** rather than per-item patches: one
  `HarnessError` carries kind + message + hint through every
  harness-level failure path.
- All changes live in `crates/yosh-plugin-manager` (+ docs, TODO.md).
  `src/plugin/` (yosh runtime) is untouched.

## 2. Non-goals

- `yosh plugin test --watch`. Only `run --watch` is tracked; add the
  (cheap) test variant when someone asks.
- Multi-plugin scenarios (TODO ~L445) — still deferred, no use case.
- `HostBackend` trait consolidation (TODO ~L443) — separate work.
- Changing the exit-code policy (0..=255 pass-through, 99 harness, 2
  clap) — unchanged.

## 3. Design

### 3.1 `HarnessError` — unified harness-level error (items 1, 5, 9, §7 mandate)

`runner.rs` replaces `RunnerError` (whose `Trap`/`Timeout` variants
are dead) with:

```rust
pub struct HarnessError {
    pub kind: ErrorKind,      // Load | Metadata | Trap | Timeout | Memory
    pub message: String,
    pub hint: Option<String>, // one-line remediation, when we have one
}

pub enum ErrorKind { Load, Metadata, Trap, Timeout, Memory }
```

- `ErrorKind` serializes to the JSON `kind` strings `"load"`,
  `"metadata"`, `"trap"`, `"timeout"`, `"memory"`. `"load"` covers
  read/engine/compile/instantiate failures (same bucketing as today);
  `"metadata"` is split out so its hint can differ.
- `load_plugin` returns `Result<LoadedPlugin, HarnessError>`.
- `cmd_run` is restructured so every early-exit path (`read`,
  `engine`, `metadata`, `load`) returns `HarnessError` to a single
  exit point. On error:
  - `--format human`: stderr line as today
    (`yosh-plugin: <kind>: <message>`, plus `hint: <hint>` line).
  - `--format json`: stdout gets
    `{"error":{"kind":"...","message":"...","hint":...}}` (hint null
    when absent), stderr keeps the human line. Exit 99 either way.
- Hints attached at construction:
  - metadata failure → "the `metadata` function must be
    side-effect-free (no host imports); see docs/yosh/plugin.md
    §Plugin Development Guide"
  - timeout → "the invocation exceeded --timeout (<N> ms); raise it
    if the plugin legitimately needs longer"
  - memory (see §3.3) → "the plugin exceeded --max-memory-mb
    (<N> MiB); raise it or fix the allocation"
- In-invocation errors keep flowing through `RunOutcome.error` /
  `error_kind` (already JSON-routed); `classify_trap` gains the
  `memory` classification (§3.3) and its result feeds both
  `RunOutcome.error_kind` and the human/JSON hint lines. To avoid
  two error vocabularies, `RunOutcome.error_kind` uses
  `ErrorKind`'s serialized strings.

### 3.2 Denied tracking (items 5, 12)

`TestState` gains:

```rust
pub denied_log: Vec<String>,  // e.g. "commands:exec: echo hello", "files:read: /etc/passwd"
```

Every host import that returns `Err(ErrorCode::Denied)` (or
`PatternNotAllowed` for `commands:exec`) first pushes a record naming
the interface and the operand. This is recorded state, not string
sniffing, so attribution is deterministic.

- `RunOutcome` gains `denied: Vec<String>`.
- Human output gains a `[denied]` section; each entry is followed by
  the applicable hint:
  - `commands:exec` → `re-run with --allow-exec '<program> *' or add
    it to the scenario's env.allow_exec`
  - `files:*` → `add files:read / files:write to --cap (or env.caps);
    seed [files] for the virtual FS or pass --sandbox-root`
  - variables/io/filesystem → `add <capability> to --cap (or
    env.caps)`
- JSON output gains `"denied": ["..."]`.
- Scenario `Expect` gains `denied: Option<bool>`: `true` passes iff
  `denied` is non-empty, `false` passes iff empty. The spec-§5
  deferral comment in `scenario.rs` is deleted.

### 3.3 Memory cap in the harness (§7 mandate)

Parity with phase 1's production memory cap:

- `yosh plugin run` gains `--max-memory-mb <N>` (default 256, same as
  production's `DEFAULT_MAX_MEMORY_MB`); scenarios gain
  `env.max_memory_mb` (same default).
- `test_host` gets a `TestLimiter` — the same ~30-line shape as
  production's `limits::MemoryLimiter` (deny growth beyond the cap,
  set a `denied` flag). Duplicated rather than shared because the
  manager crate cannot depend on the yosh binary crate; the struct is
  small and stable.
- `TestCtx` holds the limiter; `load_plugin` installs it via
  `store.limiter(...)`.
- `classify_trap` becomes limiter-aware: when the store's limiter
  `denied` flag is set, the failure classifies as `Memory` with the
  `(memory limit N MiB exceeded)` message and the §3.1 hint,
  regardless of the trap text. (The flag is read from the store data
  after the call fails, mirroring production's `with_env`.)

### 3.4 Structured step failures (item 3)

`StepResult::Fail(String)` becomes:

```rust
pub enum StepResult {
    Pass,
    Fail {
        step: usize,                        // 1-based
        check: &'static str,                // "exit", "stdout", "vars_set", "load", ...
        expected: Option<serde_json::Value>,
        got: Option<serde_json::Value>,
        reason: String,                     // human sentence, as today
    },
}
```

The `fail!` macro in `evaluate` is extended to capture
`check`/`expected`/`got` where they exist (comparison checks);
schema/load errors leave them `None`. `format_summary_json` failure
lines gain `"step"`, `"check"`, `"expected"`, `"got"` alongside the
existing `"reason"` (spec §4.2 shape). `format_summary_human` output
is unchanged.

### 3.5 Compile once (item 2)

`cmd_run` currently reads the wasm and builds an engine to run
`metadata_extract` (when `--cap` is empty), then `load_plugin`
re-reads and re-compiles. Fix:

- `cmd_run` reads the bytes once, builds the engine once, compiles
  `Component::new` once, and passes them down.
- `runner.rs` gains
  `load_plugin_precompiled(engine: &Engine, component: &Component, state, timeout)`;
  the existing path-based `load_plugin` becomes a thin wrapper
  (read + engine + compile + delegate) so external callers keep
  working.
- `metadata_extract` gains an `extract_component(&Engine, &Component)`
  entry point next to the byte-based `extract` (which becomes a
  wrapper), so the `--cap` fallback shares the compiled component.
- `scenario.rs::run_scenario` compiles the component once per
  scenario and calls `load_plugin_precompiled` per step (today every
  step recompiles). Fresh `Store`/`TestState` per step is preserved —
  only the immutable compile artifacts are shared.

### 3.6 `--watch` (item 8)

`yosh plugin run --watch`:

- After each invocation completes (and its output is flushed), poll
  the wasm path's mtime every 300 ms; when it changes (and the file
  exists — editors may briefly unlink during rebuild), re-run the
  same invocation. Loop until Ctrl-C (SIGINT default: process dies —
  no custom handler needed).
- Change detection lives in a testable helper:
  `fn wait_for_change(path: &Path, last: SystemTime) -> SystemTime`
  (sleeps in 300 ms steps; returns the new mtime). A short settle
  delay (one extra poll interval after first change) avoids reading
  a half-written wasm.
- Human mode prints a separator (`--- watching <path>; change
  detected, re-running ---`) between runs; JSON mode emits one JSON
  object per run as usual (JSONL over time).
- Watch-mode exit code: the loop never exits normally; on startup
  errors that make watching pointless (file missing at first read)
  it errors out as usual via §3.1.
- Implementation: the single-run body of `cmd_run` moves into a
  helper returning the exit code; `--watch` wraps it in the loop and
  re-reads/recompiles the component each iteration (that is the
  point of watching).

### 3.7 Trace channel, dependency-free (item 4)

New `trace.rs` in the manager crate:

- `pub fn enabled() -> bool` — reads `YOSH_PLUGIN_TRACE` once via
  `OnceLock<bool>`; truthy when set to anything but empty or `0`.
- `trace!(...)` macro — `eprintln!("yosh-plugin[trace]: ...")` when
  enabled, no-op otherwise.
- Call sites: every `test_host` host-import entry (interface, args
  summary, Ok/Err result), `runner.rs` phases (read, compile,
  instantiate, invoke, outcome), scenario step boundaries.
- This supersedes the 2026-05-12 spec §6 promise of
  `RUST_LOG=yosh_plugin_manager::runner=debug` via the `log` crate;
  the env-var tracer costs no dependency and covers the same debug
  story. Documented in `docs/yosh/plugin.md`.

### 3.8 `files_write` content capture (item 6)

- `TestState.write_log` and `RunOutcome.write_log` widen from
  `Vec<(PathBuf, usize)>` to `Vec<(PathBuf, Vec<u8>)>` (both write
  sites in `test_host/files.rs` — virtual FS and sandbox mode — clone
  the written bytes; test-plugin-scale payloads make the copy cost
  irrelevant).
- Evaluator: `FileExpect::Bytes(s)` and `bytes_eq` now compare
  content equality; `len` keeps comparing length. Mismatch failures
  show expected vs got content (lossy UTF-8), truncated to 200 chars
  per side to keep messages readable.
- Human output keeps `(N bytes)`. JSON `files_write` entries keep
  `"bytes": N` and gain `"content": "<lossy UTF-8>"`.

### 3.9 Sandbox E2E scenario (item 7)

`test_plugin` already ships a `write-file` command (writes
`YOSH_TEST_CONTENT\n` via `files:write`) — no fixture changes needed.
New integration test in `crates/yosh-plugin-manager/tests/runner.rs`:

- Create a tempdir; generate a scenario TOML into it with
  `sandbox_root` pointing at the tempdir, `caps = ["files:write"]`,
  and a step `exec write-file /out.txt` expecting `exit = 0` and
  `files_write` content.
- Run via `scenario::run_scenario`, assert the step passed AND the
  file exists on the real filesystem with the expected content.
- Skips silently when `test_plugin.wasm` is not built (same pattern
  as the existing artifact-gated tests).

### 3.10 Small alignments (items 10, 11)

- `RunAction`, `HookKind`, `OutputFormat`, `parse_kv` in `lib.rs`
  tighten to `pub(crate)`.
- `test_host/filesystem.rs::host_set_cwd` returns `IoFailed` for the
  empty path, matching the production host's error mapping (was
  `InvalidArgument`).

## 4. Error handling summary

| Failure | kind | Where surfaced | Hint |
|---|---|---|---|
| wasm read/compile/instantiate | `load` | stderr + JSON `error` | — |
| metadata extraction | `metadata` | stderr + JSON `error` | metadata must be side-effect-free |
| guest trap | `trap` | `RunOutcome.error` | — |
| epoch deadline | `timeout` | `RunOutcome.error` | raise `--timeout` / `env.timeout_ms` |
| memory cap | `memory` | `RunOutcome.error` | raise `--max-memory-mb` / `env.max_memory_mb` |
| capability denied | not an error (guest decides) | `[denied]` section / JSON `denied` array | per-capability remediation |

Exit codes unchanged: plugin exit pass-through, 99 harness, 2 clap;
`test` exits 0 iff all scenarios pass.

## 5. Testing

- Unit (`cargo test -p yosh-plugin-manager`):
  - `HarnessError` JSON shape; hint attachment per kind.
  - `denied_log` recording in each `test_host` module's denied path;
    `Expect::denied` true/false evaluation.
  - `TestLimiter` denies over cap and sets the flag (mirror of the
    production limiter test).
  - Structured `Fail` fields for exit/stdout/vars_set mismatches;
    `format_summary_json` includes `step`/`check`/`expected`/`got`.
  - `files_write` content match pass + mismatch (message truncation).
  - `wait_for_change` detects an mtime bump on a temp file.
  - `trace::enabled` env-var gating.
  - `host_set_cwd("")` returns `IoFailed`.
- Integration (`tests/runner.rs`, artifact-gated):
  - JSON error routing: the error-object formatting is a pure
    function (`HarnessError` → `serde_json::Value`) unit-tested
    directly; `cmd_run`'s single exit point prints its result, so
    the wiring needs no stdout-capture test.
  - Denied scenario: `test_plugin` `read-file` without `files:read`
    → `denied = true` passes, JSON carries the denied entry.
  - Memory cap: `hog_plugin` under `env.max_memory_mb = 8` traps with
    kind `memory` (reuses the phase 1 fixture).
  - Compile-once: `run` with empty `--cap` still works end-to-end
    (behavioral; the perf win is not timed in tests).
  - Sandbox E2E per §3.9.
- Full suite + e2e stay green.

## 6. Documentation & cleanup

- `docs/yosh/plugin.md` §Testing Locally: `--max-memory-mb`,
  `--watch`, `YOSH_PLUGIN_TRACE`, the `denied` expect key,
  `files_write` content semantics, and the JSON error object shape.
- TODO.md: delete the 12 swept items.
- `2026-05-12-plugin-dev-test-runner-design.md` is NOT edited; this
  spec records the `RUST_LOG` → `YOSH_PLUGIN_TRACE` supersession.
