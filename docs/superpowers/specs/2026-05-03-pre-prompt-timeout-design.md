# Pre-prompt Hook Timeout (Design)

**Status**: Draft
**Date**: 2026-05-03
**Source**: `TODO.md` "Future: Interactive Mode Enhancements" — _Pre-prompt hook timeout — protect against slow `pre_prompt` plugins blocking prompt display; consider timeout or async approach (`src/plugin/mod.rs`)_

## 1. Problem

`PluginManager::call_pre_prompt` (`src/plugin/mod.rs:428`) is invoked from the
interactive REPL (`src/interactive/mod.rs:139`) before every PS1 prompt.
It iterates registered plugins and calls each plugin's `pre_prompt` hook
synchronously. A plugin whose `pre_prompt` is slow (network call, large
filesystem walk, infinite loop) blocks the prompt indefinitely, freezing the
shell from the user's perspective.

The wasmtime engine is currently configured with `async_support(false)` and
`consume_fuel(false)`, so the host has no built-in mechanism to cap wall-clock
time spent inside guest code.

## 2. Goals / Non-goals

### Goals

- Bound the wall-clock time spent in `pre_prompt` per plugin to a configurable
  budget (default 500 ms).
- On timeout, treat the plugin as if it had trapped: print one stderr line
  identifying the timeout, mark the plugin invalidated, and skip it for the
  rest of the session.
- Allow the user to override the budget without recompiling, via an
  environment variable.
- Reuse the existing trap-handling path in `with_env` so timeouts and panics
  share one disable mechanism.

### Non-goals

- Timeout for other hooks (`pre_exec`, `post_exec`, `on_cd`) or for `exec_command`.
  Tracked as a follow-up TODO if user demand surfaces.
- Asynchronous execution of `pre_prompt`. Stays synchronous.
- Per-plugin custom timeouts in `plugins.toml`. The single global budget covers
  the immediate need; the env-var override is the only configuration surface
  for now.
- Fuel metering or memory caps. Listed in `TODO.md` under
  "Plugin runtime limits" and intentionally out of scope here per
  `2026-04-27-wasm-plugin-runtime-design.md` §10.
- Re-enabling a timed-out plugin within the same session. Invalidation is
  permanent until the next `yosh` start, matching the existing trap behaviour.

## 3. Approach: wasmtime epoch interruption

wasmtime's epoch interruption mechanism is the standard way to bound guest
wall-clock time without converting to async. The engine maintains a
monotonically increasing epoch counter; each `Store` carries a deadline; when
the engine's epoch passes the store's deadline, every active and subsequent
guest call traps with `Trap::Interrupt`.

The host increments the epoch periodically from a tick thread. Per-call, the
host sets `store.set_epoch_deadline(N)` where `N` is the number of ticks
ahead the deadline should land. Wasm execution polls the epoch counter at
function call boundaries and loop back-edges, so a busy loop can be
interrupted in tick-granularity time.

### 3.1 Engine configuration

In `PluginManager::new`:

```rust
let mut config = wasmtime::Config::new();
config.wasm_component_model(true);
config.async_support(false);
config.consume_fuel(false);
config.epoch_interruption(true);  // new
```

The engine fingerprint becomes:

```text
v2;component_model=true;async=false;fuel=false;epoch=true;cranelift
```

(Bumped from `v1` so that any pre-existing `.cwasm` files compiled under the
old config are invalidated through the existing engine_config_hash check; users
will see the standard "cwasm cache stale" warning once and the cache rebuilds
on the next `yosh-plugin sync`.)

### 3.2 Tick thread

`PluginManager` gains two fields:

```rust
struct PluginManager {
    engine: Engine,
    engine_fingerprint: String,
    plugins: Vec<LoadedPlugin>,
    pre_prompt_timeout_ms: u64,        // resolved once at construction
    tick_thread: Option<TickThread>,   // RAII handle, dropped with manager
}

struct TickThread {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}
```

`PluginManager::new` spawns one tick thread that calls
`engine.increment_epoch()` every 50 ms until `stop` is set. The tick interval
is fixed; it is not user-configurable (internal detail).

`Drop for PluginManager` sets `stop = true` and joins the thread. We add a
`Drop` impl despite the existing comment in the file ("no `Drop` impl"); the
existing comment refers to plugin `on_unload` requiring `&mut ShellEnv`, which
is unrelated. The tick thread cleanup is a separate concern and the comment
will be amended.

### 3.3 Per-call deadline

`call_pre_prompt` sets the deadline for each plugin's store before the call:

```rust
pub fn call_pre_prompt(&mut self, env: &mut ShellEnv) {
    let ticks = self.pre_prompt_timeout_ms.div_ceil(TICK_MS);  // TICK_MS = 50
    for plugin in &mut self.plugins {
        if plugin.capabilities & CAP_HOOK_PRE_PROMPT == 0 {
            continue;
        }
        if !plugin.implements_hook(HookName::PrePrompt) {
            continue;
        }
        plugin.store.set_epoch_deadline(ticks);
        let _ = with_env(plugin, env, |bindings, store| {
            bindings.yosh_plugin_hooks().call_pre_prompt(store)
        });
    }
}
```

`set_epoch_deadline` takes ticks-from-now-relative semantics: each call
re-bases the deadline relative to the current epoch, so prior calls do not
affect later ones.

### 3.4 Trap classification in `with_env`

When `f` returns `Err(e)` and `e.downcast_ref::<wasmtime::Trap>()` yields a
trap, the existing code prints:

```text
yosh: plugin '<name>': trapped: <trap> — disabling for the rest of this session
```

We extend the branch to detect `Trap::Interrupt` (the variant epoch
interruption surfaces) and emit a more specific line:

```text
yosh: plugin '<name>': pre_prompt exceeded <N>ms timeout — disabling for the rest of this session
```

The `<N>ms` value is the resolved timeout. Because `with_env` is generic over
all hook calls, the message specifically says "pre_prompt" only when we know
the call was `pre_prompt`. We thread that context by:

1. Threading a `&'static str` "kind" parameter through `with_env`, OR
2. Letting the caller (`call_pre_prompt`) post-classify and emit the message.

Option 2 keeps `with_env` generic. Implementation: change `with_env` to
return either `Ok(R)`, `Err(WithEnvError::Trapped(trap))`, or
`Err(WithEnvError::Other(e))`, and let `call_pre_prompt` print the
hook-specific message. This is a small refactor; non-pre_prompt callers
fall back to the generic message via a helper.

Either option is acceptable. **Decision: option 2** — clearer separation of
concerns, and only `call_pre_prompt` needs the timeout-specific message today.

### 3.5 Configuration resolution

`PluginManager::new` reads `YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS` once.

| Input | Behaviour |
|---|---|
| unset | Use 500 ms default |
| valid integer in `[1, 60000]` | Use that value |
| 0 | Reject; warn `yosh: plugin: YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS=0 invalid (must be >= 1ms); using default 500ms`, use default |
| > 60000 | Reject with same warning shape, use default |
| non-numeric | Reject with same warning shape, use default |

The 60-second upper bound is a sanity rail; legitimate `pre_prompt` work
should be milliseconds. The 1 ms lower bound prevents nonsensical configs
that would always trip.

The warning prints to stderr at shell startup (once). Subsequent reads of the
env var have no effect.

## 4. Components

### 4.1 `src/plugin/mod.rs`

- New constants `TICK_MS: u64 = 50`, `DEFAULT_PRE_PROMPT_TIMEOUT_MS: u64 = 500`,
  `MAX_PRE_PROMPT_TIMEOUT_MS: u64 = 60_000`.
- New `TickThread` struct + `Drop` glue.
- New `PluginManager.pre_prompt_timeout_ms` and `PluginManager.tick_thread` fields.
- `PluginManager::new` updated: epoch_interruption flag, tick thread spawn,
  env-var resolution.
- Engine fingerprint string updated to v2.
- `call_pre_prompt` updated: deadline set, hook-specific timeout message
  via the refactored `with_env` return type.
- Comment near `Drop` policy updated to reflect the tick-thread cleanup.

### 4.2 `tests/plugins/slow_plugin/`

A new wasm-component test plugin (workspace member, excluded from
`default-members` like the existing test plugins). It exposes:

- `metadata()` returning `pre-prompt` in `implemented-hooks` and
  `hooks:pre_prompt` in `required-capabilities`.
- `pre_prompt()` body: a busy loop with `core::hint::black_box` to defeat
  optimisation. No host calls (we want to verify the interrupt mechanism
  itself, not host-call short-circuit behaviour).

Cargo.toml mirrors `test_plugin/Cargo.toml` with a different package name.

### 4.3 `tests/plugin.rs`

- A new test helper exposed under the existing `#[cfg(any(test,
  feature = "test-helpers"))] pub mod test_helpers`:

  ```rust
  pub fn set_pre_prompt_timeout_for_tests(manager: &mut PluginManager, ms: u64)
  ```

  Tests call this instead of setting `YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS` in
  the process environment. Rationale: Rust 2024 makes `std::env::set_var`
  `unsafe`, and process-level env mutation races across parallel tests.
  A direct setter on the manager sidesteps both issues and matches the
  existing test_helpers pattern (`load_plugin_with_caps`,
  `env_pointer_is_null_in_store`).

- New integration test `pre_prompt_timeout_invalidates_slow_plugin`:
  - Build `slow_plugin` cwasm following the existing `test_plugin` /
    `trap_plugin` pattern: precondition is `cargo component build -p
    slow_plugin --target wasm32-wasip2 --release`, documented in the test
    file's module-level comment (mirroring the existing test plugins).
  - Call `set_pre_prompt_timeout_for_tests(&mut manager, 100)`.
  - `load_plugin_with_caps(...)` with `hooks:pre_prompt`.
  - Capture stderr (the existing tests use a stderr capture pattern; reuse
    it). Call `call_pre_prompt` once; assert:
    - elapsed wall time < 1 s (i.e. clearly bounded, not unbounded).
    - stderr contains `pre_prompt exceeded 100ms timeout`.
    - second `call_pre_prompt` is a no-op for that plugin (logged as
      `skipped (instance invalidated by earlier trap)`).

### 4.4 `src/plugin/mod.rs::tests`

- The env-var resolver is factored as a pure function:

  ```rust
  fn parse_pre_prompt_timeout(input: Option<&str>) -> Result<u64, String>
  ```

  Taking the env value as an `Option<&str>` (so tests pass literals;
  production calls `std::env::var("YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS").ok()`
  at the call site). `Ok(n)` carries a usable timeout; `Err(raw)` carries
  the original invalid input so the caller can quote it in a warning. The
  caller (`PluginManager::new`) is responsible for the stderr warning and
  the default fallback. This factoring keeps the parser pure and the
  warning testable indirectly through behaviour rather than through env
  mutation.

- `parse_pre_prompt_timeout_*` unit tests cover: `None`, valid integer in
  range, `Some("0")`, `Some("60001")`, `Some("999999")`, `Some("abc")`,
  `Some("")`, `Some("-1")`. Pure parsing, no engine.
- `tick_thread_stops_on_drop` covers: spawn the tick thread, drop, verify
  the thread joined within a generous timeout. Catches future regressions
  where `Drop` fails to signal stop.

## 5. Data flow

```text
PluginManager::new
  └─ env var → resolved_timeout_ms (clamped, with warning)
  └─ Engine::new(epoch_interruption=true)
  └─ spawn tick thread → increment_epoch every 50ms

call_pre_prompt(env):
  for each plugin with CAP_HOOK_PRE_PROMPT and PrePrompt impl:
    plugin.store.set_epoch_deadline(ceil(timeout_ms/50))
    with_env(plugin, env, |b,s| b.yosh_plugin_hooks().call_pre_prompt(s))
      → on Ok: continue
      → on Trap::Interrupt: emit timeout-specific line, invalidate
      → on other Trap: emit existing trap line, invalidate
      → on non-trap Err: emit existing call-failed line, do NOT invalidate

Drop for PluginManager:
  tick_thread.stop.store(true)
  tick_thread.handle.join()
  (existing field drops happen after this)
```

## 6. Error handling

- Tick thread spawn failure: panic at startup. The thread is part of the
  plugin runtime contract; without it, all `pre_prompt` calls would block
  forever. A panic is the right signal — there is no graceful degradation.
- `set_epoch_deadline` is infallible.
- Inside `with_env`, `Trap::Interrupt` is just one more `Trap` variant; we
  pattern-match on it via `wasmtime::Trap` (the public enum exposes `Interrupt`
  in wasmtime 27).

## 7. Testing strategy

Unit tests for env parsing and tick-thread lifecycle. Integration tests for
end-to-end timeout behaviour using `slow_plugin`. No PTY tests needed — the
feature is observable purely through `PluginManager` API.

The integration test sets a low timeout (100 ms) so the test runs fast. The
`slow_plugin` busy-loops, so the wall-clock bound is dominated by the
deadline, not by tick granularity beating against a slow plugin's natural
duration.

## 8. Migration / compatibility

- Existing cwasm caches invalidate once because the engine fingerprint
  changes from `v1` to `v2`. Users see the existing "cwasm cache stale"
  warning and `yosh-plugin sync` regenerates them. No manual migration step.
- Existing plugins that complete `pre_prompt` quickly are unaffected.
- A plugin whose `pre_prompt` already exceeds 500 ms will start being
  invalidated once per session at the first prompt. Users can raise the
  timeout via `YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS=2000` if a specific plugin
  legitimately needs more.

## 9. Open questions

None at design time. The two design judgement calls are recorded:

- **Tick interval: 50 ms.** Worst-case overshoot is 50 ms past the deadline
  (one tick window). For a 500 ms budget that is a 10 % overshoot, well
  within "fast enough that the user sees a brief stall, not a freeze".
  Smaller intervals (10 ms) would tighten this at the cost of background
  CPU; larger (100 ms) would loosen it. 50 ms is the standard wasmtime
  example value and is appropriate here.
- **`with_env` return-type refactor.** Section 3.4 picked option 2 (typed
  error variants) over a `kind` string parameter. Both work; the typed
  variant keeps callers in charge of message phrasing.
