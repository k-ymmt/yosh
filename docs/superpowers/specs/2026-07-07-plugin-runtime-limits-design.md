# Plugin Runtime Resource Limits — Design

Date: 2026-07-07
Status: Approved (phase 1 of 2; phase 2 = plugin-author DX sweep, separate spec)

## 1. Context

The plugin host (`src/plugin/`) already has epoch-based interruption:
a `TickThread` bumps the engine epoch every `TICK_MS = 50` ms, and
`call_pre_prompt` sets a tight per-call deadline (default 500 ms,
`YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS` override). Everything else runs at
an effectively-never baseline deadline, and no memory limits exist
anywhere. TODO.md tracks the gaps:

- Runtime limits (fuel / memory caps) deferred from v0.2.0 (TODO ~L463).
- `yosh-plugin-manager` runner watchdog is a one-shot thread; a
  CPU-bound guest takes 3–8 s to trap instead of the spec §10 ~2 s
  budget, and `tests/runner.rs::case_5_timeout_on_slow_plugin_pre_prompt`
  was relaxed to a 15 s ceiling (TODO ~L446).

## 2. Goals

1. A runaway plugin cannot exhaust shell memory: per-plugin memory cap.
2. A hung plugin cannot wedge the shell: per-call timeouts on every
   guest entry point, not just `pre_prompt`.
3. The `yosh plugin run/test` harness traps CPU-bound guests on the
   same schedule as the production host (continuous tick thread).

## 3. Non-goals — fuel metering is rejected

`consume_fuel` stays off. Epoch interruption already bounds runaway CPU
at near-zero overhead; fuel adds a per-instruction decrement cost to
every guest call and a second limiting mechanism to maintain. No
current use case needs deterministic instruction budgets. This decision
supersedes the "add wasmtime fuel metering" wording of the v0.2.0 spec
§10 deferral. Revisit only if a reproducible-execution use case appears.

Also out of scope: async wasmtime, per-plugin OS-level sandboxing,
limits on host-import work (e.g. `commands:exec` already has its own
1000 ms timeout).

## 4. Design

### 4.1 Memory cap

- `HostContext` gains a `limits: wasmtime::StoreLimits` field, built
  with `StoreLimitsBuilder::new().memory_size(max_memory_mb * MiB)`.
- `load_plugin` calls `store.limiter(|ctx| &mut ctx.limits)` right
  after `Store::new`. Same wiring on the metadata scratch store,
  which always uses the 256 MiB default (per-plugin config is not yet
  resolved at metadata-extract time, and metadata needs no headroom).
- Default: **256 MiB** per plugin. Config: `max_memory_mb` (u64,
  validated ≥ 1; values > 4096 clamp to 4096 with a stderr warning,
  mirroring the pre-prompt clamp style).
- Behaviour on breach: `memory.grow` fails, the guest allocator
  aborts, the trap propagates through the existing `with_env` error
  path → plugin invalidated for the session. The failure log gains a
  hint: when a trap message contains the wasmtime grow-failure marker,
  append `(memory limit N MiB exceeded?)` — best-effort attribution,
  since wasm traps do not carry a structured "limiter denied" code.

### 4.2 Timeouts on every guest entry point

Extend the `call_pre_prompt` pattern (set deadline → call → restore
`STORE_BASELINE_DEADLINE_TICKS`) to all dispatches:

| Entry point | Config key | Default |
|---|---|---|
| `pre_prompt` | `pre_prompt_timeout_ms` | 500 ms (unchanged) |
| `pre_exec`, `post_exec`, `on_cd` | `hook_timeout_ms` | 5 000 ms |
| `exec` (custom commands) | `command_timeout_ms` | 0 = unlimited |

- Custom commands default to unlimited because users invoke them
  interactively and long-running work is legitimate; hooks sit on the
  prompt/exec hot path and must stay bounded.
- `pre_prompt` precedence: per-plugin `pre_prompt_timeout_ms` >
  `YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS` env var > 500 ms default. The
  env var keeps its existing global semantics and [1, 60 000] range.
- `hook_timeout_ms` / `command_timeout_ms` validation: 0 means
  unlimited (baseline deadline); positive values clamp to
  MAX_PRE_PROMPT_TIMEOUT_MS-style ceiling of 600 000 ms (10 min).
- Deadline restore must be unconditional (current pre_prompt code
  already restores after both Ok and Err) — keep that invariant.
- Timeout behaviour: identical to pre_prompt today — `Trap::Interrupt`
  → invalidated for the session, warning names the plugin, the entry
  point, and the budget, e.g.
  `yosh: plugin 'foo': on_cd exceeded 5000ms timeout — disabling for the rest of this session`.

Implementation shape: `LoadedPlugin` gains a small `Limits` struct
(`pre_prompt_ticks`, `hook_ticks`, `command_ticks: Option<u64>`)
resolved once at load from the config entry; `with_env` callers pass
the applicable deadline instead of each call site hand-rolling the
set/restore dance — a `with_deadline(plugin, ticks, f)` helper wraps
`with_env`.

### 4.3 Manager harness parity

Replace the one-shot watchdog in
`crates/yosh-plugin-manager/src/runner.rs` (and the matching pattern
in `metadata_extract.rs` if present) with a continuous tick thread:
same structure as the production `TickThread` (50 ms interval, stop
flag, joined on drop). Deadlines become tick counts like production.
Then restore `tests/runner.rs::case_5_timeout_on_slow_plugin_pre_prompt`
to the spec §10 ~2 s budget (allow generous CI margin: assert < 5 s
wall clock, not 15 s).

### 4.4 Config & docs

- `src/plugin/config.rs::PluginEntry` gains four optional fields:
  `max_memory_mb`, `hook_timeout_ms`, `command_timeout_ms`,
  `pre_prompt_timeout_ms`. All `Option<u64>`, absent = default.
  They flow through `load_one` → `LoadedPlugin.limits`.
- `yosh plugin sync` must pass the fields through to `plugins.lock`
  unchanged (same flat-copy treatment as `allowed_commands` /
  `files_root`).
- `docs/yosh/plugin.md`: add the four fields to the config-fields
  table with defaults and semantics; add a short "Resource Limits"
  subsection after "Confining `files` Access".
- TODO.md: delete the runtime-limits item (~L463) and the watchdog
  item (~L446); record the fuel rejection nowhere else — this spec is
  the record.

## 5. Error handling summary

| Failure | Mechanism | User-visible outcome |
|---|---|---|
| Hook over budget | epoch `Trap::Interrupt` | warning with hook name + budget; plugin disabled for session |
| Command over budget (if configured) | same | same, names the command |
| Memory over cap | `memory.grow` fails → guest trap | trap warning with `(memory limit N MiB exceeded?)` hint; plugin disabled |
| Invalid config value | clamp + stderr warning at load | plugin still loads with clamped value |

## 6. Testing

- Unit: config parsing/validation/clamping for the four new fields;
  `Limits` resolution precedence (per-plugin vs env var vs default).
- Integration (`tests/plugin.rs`, `--features test-helpers`):
  - new `tests/plugins/hog_plugin` variant that `Vec`-allocates
    unboundedly in a hook → expect invalidation warning with memory
    hint, shell survives.
  - reuse `slow_plugin` with a busy-loop `on_cd`/`pre_exec` to verify
    the new hook timeouts trap and invalidate.
  - `command_timeout_ms = 0` (default) lets a slow custom command
    finish; a positive value traps it.
- Manager: tick-thread unit test mirroring production; `case_5`
  budget restored to < 5 s assert.
- Full suite + e2e must stay green (memory limiter must not disturb
  well-behaved plugins; 256 MiB default is far above the test
  plugins' footprint).

## 7. Phase 2 pointer

The DX sweep (denial hints, `--format json` error routing, scenario
JSON fields, `--watch`, `log` wiring, `--cap` double-compile,
`files_write` content capture, sandbox E2E scenario) is a separate
spec. It should surface the two new error kinds introduced here
(`timeout`, memory-cap trap hint) in its JSON/hint work.
