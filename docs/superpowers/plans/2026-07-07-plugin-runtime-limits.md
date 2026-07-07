# Plugin Runtime Resource Limits Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Per-plugin memory caps, epoch timeouts on every guest entry point, and a continuous tick thread in the `yosh plugin run/test` harness.

**Architecture:** The production host (`src/plugin/`) already runs an epoch tick thread and applies a per-call deadline to `pre_prompt`. This plan generalizes that pattern to all guest entry points via a `with_deadline` helper, adds a custom `ResourceLimiter` (with a `denied` flag for trap attribution) to each plugin `Store`, and resolves per-plugin limits from four new optional `plugins.lock` fields. The manager harness swaps its one-shot watchdog threads for the same continuous-tick design.

**Tech Stack:** Rust, wasmtime 27 (`epoch_interruption`, `ResourceLimiter`), cargo-component for wasm fixtures.

**Spec:** `docs/superpowers/specs/2026-07-07-plugin-runtime-limits-design.md`

## Global Constraints

- Defaults: `max_memory_mb` 256 (clamp to [1, 4096]); `hook_timeout_ms` 5000; `command_timeout_ms` 0 = unlimited; `pre_prompt_timeout_ms` 500 (clamp to [1, 60000]). Hook/command timeouts clamp to ≤ 600000 ms; 0 means unlimited for both.
- `pre_prompt` precedence: per-plugin field > `YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS` env var > 500 ms default.
- Fuel metering stays OFF (`consume_fuel(false)`). Do not touch the engine fingerprint string in `PluginManager::new` or `crates/yosh-plugin-manager/src/precompile.rs` — changing it invalidates every cached cwasm.
- Timeout trap behavior must match existing `pre_prompt`: `Trap::Interrupt` → plugin invalidated for session, warning names plugin + entry point + budget.
- NEVER run `cargo build --workspace` / `cargo test --workspace` (wasm crates fail host builds). Build fixtures with `cargo component build -p <name> --target wasm32-wasip2 --release`.
- Integration tests live in `tests/plugin.rs` behind `--features test-helpers`; they serialize via `lock_test()`.
- `cargo test` full runs take minutes — run in background per repo convention; single-test runs are fine in foreground.
- Error messages start with `yosh: ` on stderr.
- Commit after every task with the task context in the message.

---

### Task 1: Limits resolution module (`src/plugin/limits.rs`)

**Files:**
- Create: `src/plugin/limits.rs`
- Modify: `src/plugin/mod.rs` (add `mod limits;` near the other module decls, ~line 40)

**Interfaces:**
- Produces: `limits::LimitsConfig` (public raw-options struct), `limits::PluginLimits` (resolved, `pub(super)`), `limits::resolve_limits(cfg: &LimitsConfig, global_pre_prompt_ms: u64, plugin_name: &str) -> (PluginLimits, Vec<String>)`, `limits::MemoryLimiter` (used in Task 3), constants `DEFAULT_MAX_MEMORY_MB`, `MIB`.
- Consumes: `super::TICK_MS` (existing, `= 50`).

- [ ] **Step 1: Write the module with failing-to-compile tests first is impractical for a new file — create module + tests together, TDD at function level**

Create `src/plugin/limits.rs`:

```rust
//! Per-plugin runtime resource limits: resolution of the four optional
//! `plugins.lock` fields (with clamping + warnings) and the wasmtime
//! memory limiter. See
//! `docs/superpowers/specs/2026-07-07-plugin-runtime-limits-design.md`.

pub(super) const MIB: u64 = 1024 * 1024;

/// Default per-plugin linear-memory cap in MiB.
pub(super) const DEFAULT_MAX_MEMORY_MB: u64 = 256;
/// Hard ceiling for `max_memory_mb`; higher configured values clamp here.
pub(super) const MAX_MAX_MEMORY_MB: u64 = 4096;
/// Default budget for `pre_exec` / `post_exec` / `on_cd` hooks.
pub(super) const DEFAULT_HOOK_TIMEOUT_MS: u64 = 5_000;
/// Hard ceiling for hook/command timeouts (10 minutes).
pub(super) const MAX_TIMEOUT_MS: u64 = 600_000;
/// Ceiling for the per-plugin pre_prompt override — matches the env
/// var's `MAX_PRE_PROMPT_TIMEOUT_MS` range in `super`.
const MAX_PRE_PROMPT_MS: u64 = 60_000;

/// Raw optional limit values as parsed from a `plugins.lock` entry.
/// `None` = use the default. Carried separately from `PluginLimits` so
/// resolution (clamping, warnings) happens exactly once at load.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct LimitsConfig {
    pub max_memory_mb: Option<u64>,
    pub hook_timeout_ms: Option<u64>,
    pub command_timeout_ms: Option<u64>,
    pub pre_prompt_timeout_ms: Option<u64>,
}

/// Resolved per-plugin limits, stored on `LoadedPlugin`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PluginLimits {
    pub max_memory_bytes: usize,
    /// 0 = unlimited (hooks run at the baseline deadline).
    pub hook_timeout_ms: u64,
    /// 0 = unlimited (the default).
    pub command_timeout_ms: u64,
    /// Always in [1, 60000].
    pub pre_prompt_timeout_ms: u64,
}

impl PluginLimits {
    pub(super) fn pre_prompt_ticks(&self) -> u64 {
        self.pre_prompt_timeout_ms.div_ceil(super::TICK_MS)
    }
    pub(super) fn hook_deadline_ticks(&self) -> Option<u64> {
        (self.hook_timeout_ms > 0).then(|| self.hook_timeout_ms.div_ceil(super::TICK_MS))
    }
    pub(super) fn command_deadline_ticks(&self) -> Option<u64> {
        (self.command_timeout_ms > 0).then(|| self.command_timeout_ms.div_ceil(super::TICK_MS))
    }
    pub(super) fn max_memory_mb(&self) -> u64 {
        (self.max_memory_bytes as u64) / MIB
    }
}

/// Resolve raw config values into `PluginLimits`, clamping out-of-range
/// values. Returns the warnings to print (caller prefixes `yosh: `) so
/// this stays pure and unit-testable.
pub(super) fn resolve_limits(
    cfg: &LimitsConfig,
    global_pre_prompt_ms: u64,
    plugin_name: &str,
) -> (PluginLimits, Vec<String>) {
    let mut warnings = Vec::new();

    let max_memory_mb = match cfg.max_memory_mb {
        None => DEFAULT_MAX_MEMORY_MB,
        Some(0) => {
            warnings.push(format!(
                "plugin '{}': max_memory_mb 0 is invalid; using 1",
                plugin_name
            ));
            1
        }
        Some(mb) if mb > MAX_MAX_MEMORY_MB => {
            warnings.push(format!(
                "plugin '{}': max_memory_mb {} exceeds ceiling; clamped to {}",
                plugin_name, mb, MAX_MAX_MEMORY_MB
            ));
            MAX_MAX_MEMORY_MB
        }
        Some(mb) => mb,
    };

    // 0 = unlimited is a valid setting for hook/command budgets.
    let mut clamp_timeout = |field: &str, v: Option<u64>, default: u64| match v {
        None => default,
        Some(ms) if ms > MAX_TIMEOUT_MS => {
            warnings.push(format!(
                "plugin '{}': {} {} exceeds ceiling; clamped to {}",
                plugin_name, field, ms, MAX_TIMEOUT_MS
            ));
            MAX_TIMEOUT_MS
        }
        Some(ms) => ms,
    };
    let hook_timeout_ms = clamp_timeout("hook_timeout_ms", cfg.hook_timeout_ms, DEFAULT_HOOK_TIMEOUT_MS);
    let command_timeout_ms = clamp_timeout("command_timeout_ms", cfg.command_timeout_ms, 0);

    let pre_prompt_timeout_ms = match cfg.pre_prompt_timeout_ms {
        None => global_pre_prompt_ms,
        Some(0) => {
            warnings.push(format!(
                "plugin '{}': pre_prompt_timeout_ms 0 is invalid; using {}",
                plugin_name, global_pre_prompt_ms
            ));
            global_pre_prompt_ms
        }
        Some(ms) if ms > MAX_PRE_PROMPT_MS => {
            warnings.push(format!(
                "plugin '{}': pre_prompt_timeout_ms {} exceeds ceiling; clamped to {}",
                plugin_name, ms, MAX_PRE_PROMPT_MS
            ));
            MAX_PRE_PROMPT_MS
        }
        Some(ms) => ms,
    };

    (
        PluginLimits {
            max_memory_bytes: (max_memory_mb * MIB) as usize,
            hook_timeout_ms,
            command_timeout_ms,
            pre_prompt_timeout_ms,
        },
        warnings,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> PluginLimits {
        resolve_limits(&LimitsConfig::default(), 500, "p").0
    }

    #[test]
    fn defaults_match_spec() {
        let l = defaults();
        assert_eq!(l.max_memory_bytes, (256 * MIB) as usize);
        assert_eq!(l.hook_timeout_ms, 5_000);
        assert_eq!(l.command_timeout_ms, 0);
        assert_eq!(l.pre_prompt_timeout_ms, 500);
    }

    #[test]
    fn no_warnings_for_defaults_or_in_range_values() {
        let (_, w) = resolve_limits(&LimitsConfig::default(), 500, "p");
        assert!(w.is_empty());
        let cfg = LimitsConfig {
            max_memory_mb: Some(64),
            hook_timeout_ms: Some(100),
            command_timeout_ms: Some(30_000),
            pre_prompt_timeout_ms: Some(250),
        };
        let (l, w) = resolve_limits(&cfg, 500, "p");
        assert!(w.is_empty());
        assert_eq!(l.max_memory_bytes, (64 * MIB) as usize);
        assert_eq!(l.pre_prompt_timeout_ms, 250);
    }

    #[test]
    fn memory_clamps_zero_to_one_and_huge_to_ceiling() {
        let (l, w) = resolve_limits(
            &LimitsConfig { max_memory_mb: Some(0), ..Default::default() },
            500,
            "p",
        );
        assert_eq!(l.max_memory_bytes, MIB as usize);
        assert_eq!(w.len(), 1);
        let (l, w) = resolve_limits(
            &LimitsConfig { max_memory_mb: Some(100_000), ..Default::default() },
            500,
            "p",
        );
        assert_eq!(l.max_memory_bytes, (4096 * MIB) as usize);
        assert!(w[0].contains("max_memory_mb"));
    }

    #[test]
    fn zero_timeout_means_unlimited_for_hooks_and_commands() {
        let cfg = LimitsConfig {
            hook_timeout_ms: Some(0),
            command_timeout_ms: Some(0),
            ..Default::default()
        };
        let (l, w) = resolve_limits(&cfg, 500, "p");
        assert!(w.is_empty());
        assert_eq!(l.hook_deadline_ticks(), None);
        assert_eq!(l.command_deadline_ticks(), None);
    }

    #[test]
    fn timeouts_clamp_to_ten_minute_ceiling() {
        let cfg = LimitsConfig {
            hook_timeout_ms: Some(1_000_000),
            command_timeout_ms: Some(2_000_000),
            ..Default::default()
        };
        let (l, w) = resolve_limits(&cfg, 500, "p");
        assert_eq!(l.hook_timeout_ms, 600_000);
        assert_eq!(l.command_timeout_ms, 600_000);
        assert_eq!(w.len(), 2);
    }

    #[test]
    fn pre_prompt_falls_back_to_global_and_clamps() {
        let (l, _) = resolve_limits(&LimitsConfig::default(), 123, "p");
        assert_eq!(l.pre_prompt_timeout_ms, 123);
        let (l, w) = resolve_limits(
            &LimitsConfig { pre_prompt_timeout_ms: Some(0), ..Default::default() },
            123,
            "p",
        );
        assert_eq!(l.pre_prompt_timeout_ms, 123);
        assert_eq!(w.len(), 1);
        let (l, w) = resolve_limits(
            &LimitsConfig { pre_prompt_timeout_ms: Some(90_000), ..Default::default() },
            123,
            "p",
        );
        assert_eq!(l.pre_prompt_timeout_ms, 60_000);
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn tick_helpers_round_up() {
        // TICK_MS = 50: 100ms → 2 ticks, 101ms → 3 ticks, 1ms → 1 tick.
        let l = PluginLimits {
            max_memory_bytes: 0,
            hook_timeout_ms: 101,
            command_timeout_ms: 100,
            pre_prompt_timeout_ms: 1,
        };
        assert_eq!(l.hook_deadline_ticks(), Some(3));
        assert_eq!(l.command_deadline_ticks(), Some(2));
        assert_eq!(l.pre_prompt_ticks(), 1);
    }
}
```

- [ ] **Step 2: Register the module**

In `src/plugin/mod.rs`, next to the existing `mod cache;` / `pub mod config;` declarations add:

```rust
pub mod limits;
```

(`pub` because `LimitsConfig` appears in `pub(super)` signatures and the `test_helpers` API in Task 5.)

- [ ] **Step 3: Run the tests**

Run: `cargo test --lib plugin::limits -- --nocapture`
Expected: all 7 tests PASS.

- [ ] **Step 4: Commit**

```bash
git add src/plugin/limits.rs src/plugin/mod.rs
git commit -m "feat(plugin): add per-plugin limits resolution module

Task 1 of docs/superpowers/plans/2026-07-07-plugin-runtime-limits.md
(plugin runtime resource limits)."
```

---

### Task 2: Config fields + limits carried on `LoadedPlugin`

**Files:**
- Modify: `src/plugin/config.rs` (add 4 fields to `PluginEntry` + `limits_config()` + tests)
- Modify: `src/plugin/mod.rs` (`load_one` signature, `load_from_config`, `load_plugin`, `LoadedPlugin`, `test_helpers`)
- Modify: `tests/plugin.rs` only if compilation requires (helpers keep their signatures)

**Interfaces:**
- Consumes: `limits::LimitsConfig`, `limits::resolve_limits` (Task 1).
- Produces: `PluginEntry::limits_config() -> limits::LimitsConfig`; `load_one(..., limits_cfg: limits::LimitsConfig)`; `LoadedPlugin.limits: PluginLimits` (Tasks 3/5 read it); `test_helpers::load_plugin_with_limits(manager, path, env, caps, limits_cfg) -> Result<(), String>`.

- [ ] **Step 1: Write failing config parse test**

Append to `src/plugin/config.rs` tests:

```rust
    #[test]
    fn parse_limit_fields() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "limited"
path = "/tmp/x.wasm"
max_memory_mb = 64
hook_timeout_ms = 1000
command_timeout_ms = 30000
pre_prompt_timeout_ms = 250
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        let lc = config.plugin[0].limits_config();
        assert_eq!(lc.max_memory_mb, Some(64));
        assert_eq!(lc.hook_timeout_ms, Some(1000));
        assert_eq!(lc.command_timeout_ms, Some(30000));
        assert_eq!(lc.pre_prompt_timeout_ms, Some(250));
    }

    #[test]
    fn parse_missing_limit_fields_default_to_none() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "plain"
path = "/tmp/x.wasm"
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        assert_eq!(
            config.plugin[0].limits_config(),
            crate::plugin::limits::LimitsConfig::default()
        );
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib plugin::config::tests::parse_limit_fields`
Expected: FAIL to compile — `limits_config` not found.

- [ ] **Step 3: Add the fields and accessor**

In `src/plugin/config.rs`, append to `PluginEntry` after `files_root`:

```rust
    /// Per-plugin linear-memory cap in MiB (default 256, ceiling 4096).
    #[serde(default)]
    pub max_memory_mb: Option<u64>,
    /// Budget for pre_exec/post_exec/on_cd hooks in ms. 0 = unlimited.
    /// Default 5000.
    #[serde(default)]
    pub hook_timeout_ms: Option<u64>,
    /// Budget for plugin custom commands in ms. 0 = unlimited (default).
    #[serde(default)]
    pub command_timeout_ms: Option<u64>,
    /// Per-plugin pre_prompt budget in ms, range [1, 60000]. Overrides
    /// the `YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS` env var for this plugin.
    #[serde(default)]
    pub pre_prompt_timeout_ms: Option<u64>,
```

And in `impl PluginEntry`:

```rust
    /// Bundle the four optional limit fields for `load_one`.
    pub fn limits_config(&self) -> crate::plugin::limits::LimitsConfig {
        crate::plugin::limits::LimitsConfig {
            max_memory_mb: self.max_memory_mb,
            hook_timeout_ms: self.hook_timeout_ms,
            command_timeout_ms: self.command_timeout_ms,
            pre_prompt_timeout_ms: self.pre_prompt_timeout_ms,
        }
    }
```

- [ ] **Step 4: Thread `LimitsConfig` through `load_one` and store `PluginLimits`**

In `src/plugin/mod.rs`:

1. Add to `LoadedPlugin` (after `capabilities: u32`):

```rust
    /// Resolved runtime resource limits (memory cap + per-entry-point
    /// timeouts). Fixed at load time.
    limits: limits::PluginLimits,
```

2. `load_one` gains a trailing parameter `limits_cfg: limits::LimitsConfig`. Inside, after step 6 (capability negotiation — `plugin_info.name` is now known), resolve:

```rust
        let (plugin_limits, limit_warnings) =
            limits::resolve_limits(&limits_cfg, self.pre_prompt_timeout_ms, &plugin_info.name);
        for w in &limit_warnings {
            eprintln!("yosh: {}", w);
        }
```

and set `limits: plugin_limits` in the `LoadedPlugin` literal at step 8.

3. `load_from_config` passes `entry.limits_config()` as the new argument; `load_plugin` passes `limits::LimitsConfig::default()`.

4. In `test_helpers`, update both existing helpers to pass `limits::LimitsConfig::default()`, and add:

```rust
    /// Load a plugin with explicit runtime limits, for the timeout /
    /// memory-cap integration tests.
    pub fn load_plugin_with_limits(
        manager: &mut PluginManager,
        path: &Path,
        env: &mut ShellEnv,
        caps: u32,
        limits_cfg: limits::LimitsConfig,
    ) -> Result<(), String> {
        manager.load_one(path, env, Some(caps), None, None, &[], None, limits_cfg)
    }
```

5. Update the doc comment on `set_pre_prompt_timeout_for_tests` to note it must be called **before** loading plugins (per-plugin limits are resolved at load time). The manager field stays as the resolution fallback.

- [ ] **Step 5: Run lib tests + the two new config tests**

Run: `cargo test --lib plugin`
Expected: PASS (including `parse_limit_fields`, `parse_missing_limit_fields_default_to_none`).

- [ ] **Step 6: Verify integration tests still compile and pass (background, ~minutes)**

Run in background: `cargo test --features test-helpers --test plugin`
Expected: PASS — existing t01–t25 unaffected.

- [ ] **Step 7: Commit**

```bash
git add src/plugin/config.rs src/plugin/mod.rs
git commit -m "feat(plugin): parse per-plugin limit fields and resolve at load

Task 2 of docs/superpowers/plans/2026-07-07-plugin-runtime-limits.md."
```

---

### Task 3: Memory limiter wired into every plugin store

**Files:**
- Modify: `src/plugin/limits.rs` (add `MemoryLimiter`)
- Modify: `src/plugin/host/mod.rs` (`HostContext` field + `new_for_plugin` signature)
- Modify: `src/plugin/mod.rs` (limiter wiring, `WithEnvError::Trapped.memory_denied`, message hint)

**Interfaces:**
- Produces: `limits::MemoryLimiter { pub(in crate::plugin) denied: bool }` with `new(max_bytes: usize)`; `HostContext::new_for_plugin(name, caps, max_memory_bytes: usize)`; `WithEnvError::Trapped { is_interrupt: bool, memory_denied: bool, trap: wasmtime::Trap }`; `log_with_env_failure(plugin_name: &str, err: &WithEnvError, max_memory_mb: u64)`.
- Consumes: `LoadedPlugin.limits` (Task 2).

- [ ] **Step 1: Write failing unit test for the limiter**

Append to `src/plugin/limits.rs` tests:

```rust
    #[test]
    fn memory_limiter_denies_over_cap_and_sets_flag() {
        use wasmtime::ResourceLimiter;
        let mut l = MemoryLimiter::new((8 * MIB) as usize);
        assert!(l.memory_growing(0, (4 * MIB) as usize, None).unwrap());
        assert!(!l.denied);
        assert!(!l.memory_growing((4 * MIB) as usize, (16 * MIB) as usize, None).unwrap());
        assert!(l.denied);
        // Table growth is never memory-capped.
        assert!(l.table_growing(0, 10_000, None).unwrap());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib plugin::limits::tests::memory_limiter_denies_over_cap_and_sets_flag`
Expected: FAIL to compile — `MemoryLimiter` not found.

- [ ] **Step 3: Implement `MemoryLimiter`**

Add to `src/plugin/limits.rs`:

```rust
/// Per-store memory limiter. Denies any linear-memory growth beyond
/// `max_memory_bytes` and records the denial so `with_env` can
/// attribute the guest's subsequent trap (a failed `memory.grow`
/// surfaces as an allocator abort, which carries no structured cause).
pub(super) struct MemoryLimiter {
    max_memory_bytes: usize,
    pub(super) denied: bool,
}

impl MemoryLimiter {
    pub(super) fn new(max_memory_bytes: usize) -> Self {
        MemoryLimiter { max_memory_bytes, denied: false }
    }
}

impl wasmtime::ResourceLimiter for MemoryLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.max_memory_bytes {
            self.denied = true;
            Ok(false)
        } else {
            Ok(true)
        }
    }

    fn table_growing(
        &mut self,
        _current: usize,
        _desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(true)
    }
}
```

(If the wasmtime 27 trait signature differs — e.g. `table_growing` taking `u32` — match the trait exactly; the compiler error will show the expected signature. `wasmtime::Result` is `anyhow::Result`; if unavailable under the crate's feature set use `anyhow::Result<bool>`.)

- [ ] **Step 4: Run the unit test**

Run: `cargo test --lib plugin::limits`
Expected: PASS.

- [ ] **Step 5: Wire into `HostContext` and both stores**

In `src/plugin/host/mod.rs`:

1. Add field to `HostContext` (after `files_root`):

```rust
    /// Linear-memory limiter installed on the owning `Store` via
    /// `Store::limiter`. Its `denied` flag is read (and reset) by
    /// `with_env` after a trap to attribute memory-cap kills.
    pub(super) mem_limiter: crate::plugin::limits::MemoryLimiter,
```

2. Change `new_for_plugin` to `pub fn new_for_plugin(plugin_name: impl Into<String>, capabilities: u32, max_memory_bytes: usize)` and initialize `mem_limiter: crate::plugin::limits::MemoryLimiter::new(max_memory_bytes)`. Fix any other constructor call sites the compiler reports (host unit tests construct contexts — pass `256 * 1024 * 1024`).

Note: `MemoryLimiter` needs visibility from `host/mod.rs`; make the struct, `new`, and `denied` `pub(in crate::plugin)` instead of `pub(super)` if the compiler complains (`limits` and `host` are sibling modules).

In `src/plugin/mod.rs` `load_one`:

3. Scratch store (~line 470): pass the default cap and install the limiter:

```rust
        let mut scratch_store = Store::new(
            &self.engine,
            HostContext::new_for_plugin(
                "<probing>",
                CAP_ALL,
                (limits::DEFAULT_MAX_MEMORY_MB * limits::MIB) as usize,
            ),
        );
        scratch_store.limiter(|ctx| &mut ctx.mem_limiter);
```

4. Real store (~line 517): `HostContext::new_for_plugin(plugin_info.name.clone(), effective_capabilities, plugin_limits.max_memory_bytes)` and after `Store::new`:

```rust
        store.limiter(|ctx| &mut ctx.mem_limiter);
```

(This requires the Task 2 change that resolves `plugin_limits` before step 7 — it already is, step 6.5.)

- [ ] **Step 6: Extend `WithEnvError::Trapped` with `memory_denied`**

In `src/plugin/mod.rs`:

1. Variant becomes:

```rust
    Trapped {
        is_interrupt: bool,
        /// The store's memory limiter denied a grow during this call —
        /// the trap is (almost certainly) the guest allocator aborting
        /// on the failed allocation.
        memory_denied: bool,
        trap: wasmtime::Trap,
    },
```

2. In `with_env`, on the trap path read-and-reset the flag:

```rust
            if let Some(trap) = e.downcast_ref::<wasmtime::Trap>() {
                let trap = *trap;
                let is_interrupt = matches!(trap, wasmtime::Trap::Interrupt);
                let memory_denied =
                    std::mem::replace(&mut plugin.store.data_mut().mem_limiter.denied, false);
                plugin.invalidated = true;
                Err(WithEnvError::Trapped { is_interrupt, memory_denied, trap })
            } else {
```

3. `log_with_env_failure` gains a `max_memory_mb: u64` parameter and prints the hint:

```rust
fn log_with_env_failure(plugin_name: &str, err: &WithEnvError, max_memory_mb: u64) {
    match err {
        WithEnvError::Skipped => {}
        WithEnvError::Trapped { memory_denied: true, trap, .. } => {
            eprintln!(
                "yosh: plugin '{}': trapped: {} (memory limit {} MiB exceeded) — disabling for the rest of this session",
                plugin_name, trap, max_memory_mb
            );
        }
        WithEnvError::Trapped { trap, .. } => {
            eprintln!(
                "yosh: plugin '{}': trapped: {} — disabling for the rest of this session",
                plugin_name, trap
            );
        }
        WithEnvError::Other(e) => {
            eprintln!("yosh: plugin '{}': call failed: {}", plugin_name, e);
        }
    }
}
```

4. Update every `log_with_env_failure(&plugin.name, &e)` call site to `log_with_env_failure(&plugin.name, &e, plugin.limits.max_memory_mb())` (call sites: `exec_command`, `call_pre_exec`, `call_post_exec`, `call_on_cd`, `call_pre_prompt`'s fallthrough arm, `unload_all`). Update the `Trapped { is_interrupt: true, .. }` match in `call_pre_prompt` to the new variant shape (add `..` — it already uses `..`, the compiler will confirm).

- [ ] **Step 7: Run lib + integration tests**

Run: `cargo test --lib plugin` then in background `cargo test --features test-helpers --test plugin`
Expected: all PASS (limiter defaults are far above test-plugin footprints).

- [ ] **Step 8: Commit**

```bash
git add src/plugin/limits.rs src/plugin/host/mod.rs src/plugin/mod.rs
git commit -m "feat(plugin): enforce per-plugin memory cap via ResourceLimiter

Task 3 of docs/superpowers/plans/2026-07-07-plugin-runtime-limits.md.
Denied grows are attributed in the trap warning via the limiter's
denied flag instead of trap-message string sniffing."
```

---

### Task 4: Wasm fixtures — extend `slow_plugin`, add `hog_plugin`

**Files:**
- Modify: `tests/plugins/slow_plugin/src/lib.rs`
- Create: `tests/plugins/hog_plugin/Cargo.toml`, `tests/plugins/hog_plugin/src/lib.rs`
- Modify: `Cargo.toml` (workspace `members` + release profile override)

**Interfaces:**
- Produces: `slow_plugin.wasm` gains a busy-loop `on_cd` hook and a busy-loop `spin` command; `hog_plugin.wasm` allocates unboundedly in `pre_exec`. Tasks 5–6 consume both.

- [ ] **Step 1: Extend `slow_plugin`**

Replace `tests/plugins/slow_plugin/src/lib.rs` body (keep the module doc comment, and extend it) with:

```rust
//! `slow_plugin` — minimal plugin whose `pre_prompt` and `on_cd` hooks
//! busy-loop and whose `spin` command busy-loops, while `pre_exec`
//! returns immediately. Used by `tests/plugin.rs` and the manager's
//! `tests/runner.rs` to verify the epoch-deadline timeout paths
//! (per-entry-point budgets) and the post-call deadline restore.
//!
//! The plugin makes **zero host calls** by design. The test goal is to
//! verify that wasmtime's epoch-interrupt path itself terminates the
//! busy loop — *not* that the host-call deny short-circuit terminates
//! it. Keep this plugin host-call free.

use yosh_plugin_sdk::{Capability, HookName, Plugin, export};

#[derive(Default)]
struct SlowPlugin;

fn busy_loop() -> ! {
    // core::hint::black_box defeats trivial dead-code elimination; the
    // actual termination comes from increment_epoch -> Trap::Interrupt.
    loop {
        core::hint::black_box(0u64);
    }
}

impl Plugin for SlowPlugin {
    fn commands(&self) -> &[&'static str] {
        &["spin"]
    }

    fn required_capabilities(&self) -> &[Capability] {
        &[
            Capability::HookPrePrompt,
            Capability::HookPreExec,
            Capability::HookOnCd,
        ]
    }

    fn implemented_hooks(&self) -> &[HookName] {
        &[HookName::PrePrompt, HookName::PreExec, HookName::OnCd]
    }

    fn exec(&mut self, command: &str, _args: &[String]) -> i32 {
        if command == "spin" {
            busy_loop();
        }
        0
    }

    fn hook_pre_prompt(&mut self) {
        busy_loop();
    }

    fn hook_pre_exec(&mut self, _command: &str) {
        // No-op. Proves the per-call deadline is restored to baseline
        // so a later hook on the same store still runs.
    }

    fn hook_on_cd(&mut self, _old: &str, _new: &str) {
        busy_loop();
    }
}

export!(SlowPlugin);
```

(If the SDK `Plugin` trait's `hook_on_cd` signature differs — check `crates/yosh-plugin-sdk/src/lib.rs` — match it exactly.)

- [ ] **Step 2: Create `hog_plugin`**

`tests/plugins/hog_plugin/Cargo.toml`:

```toml
[package]
name = "hog_plugin"
version = "0.2.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
yosh-plugin-sdk = { path = "../../../crates/yosh-plugin-sdk" }

[package.metadata.component]
package = "yosh:hog-plugin"

[package.metadata.component.target]
path  = "../../../crates/yosh-plugin-api/wit"
world = "plugin-world"
```

`tests/plugins/hog_plugin/src/lib.rs`:

```rust
//! `hog_plugin` — allocates linear memory without bound in `pre_exec`.
//! Used by `tests/plugin.rs` to verify the per-plugin memory cap: the
//! store's `MemoryLimiter` denies the grow, the guest allocator aborts,
//! and the host invalidates the plugin with a memory-cap hint. Makes no
//! host calls (same rationale as `slow_plugin`).

use yosh_plugin_sdk::{Capability, HookName, Plugin, export};

#[derive(Default)]
struct HogPlugin;

impl Plugin for HogPlugin {
    fn commands(&self) -> &[&'static str] {
        &[]
    }

    fn required_capabilities(&self) -> &[Capability] {
        &[Capability::HookPreExec]
    }

    fn implemented_hooks(&self) -> &[HookName] {
        &[HookName::PreExec]
    }

    fn exec(&mut self, _command: &str, _args: &[String]) -> i32 {
        0
    }

    fn hook_pre_exec(&mut self, _command: &str) {
        // 1 MiB chunks force repeated memory.grow until the limiter
        // denies one; the failed allocation aborts the guest.
        let mut sink: Vec<Vec<u8>> = Vec::new();
        loop {
            let mut chunk = vec![0u8; 1 << 20];
            core::hint::black_box(chunk.as_mut_ptr());
            sink.push(chunk);
        }
    }
}

export!(HogPlugin);
```

- [ ] **Step 3: Register the workspace member + profile override**

In root `Cargo.toml`: add `"tests/plugins/hog_plugin",` to `members` (NOT to `default-members`), and next to the other test-plugin profile overrides add:

```toml
[profile.release.package.hog_plugin]
opt-level = "s"
strip = true
```

- [ ] **Step 4: Build both fixtures**

Run:
```bash
cargo component build -p slow_plugin --target wasm32-wasip2 --release
cargo component build -p hog_plugin --target wasm32-wasip2 --release
```
Expected: both produce `target/wasm32-wasip2/release/{slow_plugin,hog_plugin}.wasm`.

- [ ] **Step 5: Verify no regression in the existing slow_plugin consumers (background)**

Run in background: `cargo test --features test-helpers --test plugin t25` and `cargo test -p yosh-plugin-manager --test runner case_5`
Expected: PASS — `pre_prompt` busy-loop behavior is unchanged.

- [ ] **Step 6: Commit**

```bash
git add tests/plugins/slow_plugin tests/plugins/hog_plugin Cargo.toml Cargo.lock
git commit -m "test(plugin): extend slow_plugin (on_cd/spin busy loops), add hog_plugin

Task 4 of docs/superpowers/plans/2026-07-07-plugin-runtime-limits.md.
Fixtures for hook/command timeout and memory-cap integration tests."
```

---

### Task 5: `with_deadline` — timeouts on every guest entry point

**Files:**
- Modify: `src/plugin/mod.rs` (`with_deadline` helper, `log_hook_interrupt` wording, all `call_*` + `exec_command`)
- Test: `tests/plugin.rs` (t26–t28)

**Interfaces:**
- Consumes: `LoadedPlugin.limits` (Task 2), `WithEnvError::Trapped { is_interrupt, memory_denied, trap }` (Task 3), fixtures (Task 4), `test_helpers::load_plugin_with_limits` (Task 2).
- Produces: `fn with_deadline<R>(plugin, env, deadline_ticks: Option<u64>, f) -> Result<R, WithEnvError>`; `fn log_entry_failure(plugin_name: &str, entry: &str, timeout_ms: u64, max_memory_mb: u64, err: &WithEnvError)`.

- [ ] **Step 1: Write failing integration tests**

Append to `tests/plugin.rs` (reuse the existing `lock_test`, `fresh_env`, `slow_plugin_wasm` helpers; add `use std::time::{Duration, Instant};` if not present — t25 already uses them):

```rust
/// on_cd hook timeout — busy loop is interrupted at hook_timeout_ms and
/// the plugin is invalidated for the session.
#[test]
fn t26_on_cd_timeout_invalidates_plugin() {
    let _guard = lock_test();
    let wasm = slow_plugin_wasm();
    let mut env = fresh_env();
    let mut mgr = PluginManager::new();
    let caps = yosh_plugin_api::CAP_HOOK_PRE_PROMPT
        | yosh_plugin_api::CAP_HOOK_PRE_EXEC
        | yosh_plugin_api::CAP_HOOK_ON_CD;
    test_helpers::load_plugin_with_limits(
        &mut mgr,
        &wasm,
        &mut env,
        caps,
        yosh::plugin::limits::LimitsConfig {
            hook_timeout_ms: Some(100),
            ..Default::default()
        },
    )
    .expect("load slow_plugin");

    let start = Instant::now();
    mgr.call_on_cd(&mut env, "/a", "/b");
    let first = start.elapsed();
    assert!(
        first < Duration::from_secs(5),
        "on_cd busy loop was not interrupted: {:?}",
        first
    );

    // Invalidated: second dispatch is a fast skip.
    let start = Instant::now();
    mgr.call_on_cd(&mut env, "/b", "/c");
    assert!(start.elapsed() < Duration::from_millis(100));
}

/// Custom command timeout — with command_timeout_ms set, the busy-loop
/// `spin` command is interrupted and reported as Failed (not Handled).
#[test]
fn t27_command_timeout_interrupts_spin() {
    let _guard = lock_test();
    let wasm = slow_plugin_wasm();
    let mut env = fresh_env();
    let mut mgr = PluginManager::new();
    let caps = yosh_plugin_api::CAP_HOOK_PRE_PROMPT
        | yosh_plugin_api::CAP_HOOK_PRE_EXEC
        | yosh_plugin_api::CAP_HOOK_ON_CD;
    test_helpers::load_plugin_with_limits(
        &mut mgr,
        &wasm,
        &mut env,
        caps,
        yosh::plugin::limits::LimitsConfig {
            command_timeout_ms: Some(100),
            ..Default::default()
        },
    )
    .expect("load slow_plugin");

    let start = Instant::now();
    let result = mgr.exec_command(&mut env, "spin", &[]);
    let elapsed = start.elapsed();
    assert!(matches!(result, PluginExec::Failed), "got {:?}", result);
    assert!(
        elapsed < Duration::from_secs(5),
        "spin was not interrupted: {:?}",
        elapsed
    );
}

/// Baseline restore — after a deadline-bounded hook call returns in
/// time, a later call without its own deadline (default-unlimited
/// command) must not trip a stale epoch deadline.
#[test]
fn t28_deadline_restored_after_bounded_hook() {
    let _guard = lock_test();
    let wasm = test_plugin_wasm();
    let mut env = fresh_env();
    let mut mgr = PluginManager::new();
    test_helpers::load_plugin_with_limits(
        &mut mgr,
        &wasm,
        &mut env,
        yosh_plugin_api::CAP_ALL,
        yosh::plugin::limits::LimitsConfig {
            hook_timeout_ms: Some(100),
            ..Default::default()
        },
    )
    .expect("load test_plugin");

    // Fast hook under a 100ms budget: returns fine.
    mgr.call_pre_exec(&mut env, "ls");
    // Let more than 100ms of epoch ticks pass; if the deadline were not
    // restored to baseline, the next guest call would trap immediately.
    std::thread::sleep(Duration::from_millis(300));
    let result = mgr.exec_command(&mut env, "test_cmd", &["x".to_string()]);
    assert!(
        matches!(result, PluginExec::Handled(0)),
        "stale deadline tripped the default-unlimited command: {:?}",
        result
    );
}
```

Note: `t28` uses `test_plugin` — confirm its command name/exit with the existing `t01`/`t02` tests in this file and match them (the manager crate's runner test invokes `test_cmd` with `CAP_IO`; if `exec_command` requires other caps, mirror what existing tests in `tests/plugin.rs` pass).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --features test-helpers --test plugin t26 t27 t28` (background if slow)
Expected: t26/t27 FAIL (hang is prevented only by… actually the busy loops never trap at baseline deadline — the test will hang; use `--test-threads=1` and a manual timeout: `timeout 60 cargo test ...`). Expected outcome: timeout/failure demonstrating the feature is missing. If the hang makes the red step impractical, verify failure by compilation of the missing `limits::LimitsConfig` import path only, then proceed.

- [ ] **Step 3: Implement `with_deadline` and rewire all entry points**

In `src/plugin/mod.rs`:

1. Add below `with_env`:

```rust
/// Run a guest call under an optional epoch deadline. `None` leaves the
/// store at the baseline (effectively unlimited). The baseline is
/// restored after the call unless the plugin trapped (it is then
/// invalidated and its deadline is moot).
fn with_deadline<R>(
    plugin: &mut LoadedPlugin,
    env: &mut ShellEnv,
    deadline_ticks: Option<u64>,
    f: impl FnOnce(&PluginWorld, &mut Store<HostContext>) -> Result<R, wasmtime::Error>,
) -> Result<R, WithEnvError> {
    if let Some(ticks) = deadline_ticks {
        plugin.store.set_epoch_deadline(ticks);
    }
    let result = with_env(plugin, env, f);
    if deadline_ticks.is_some() && !matches!(&result, Err(WithEnvError::Trapped { .. })) {
        plugin
            .store
            .set_epoch_deadline(STORE_BASELINE_DEADLINE_TICKS);
    }
    result
}

/// Failure logger for deadline-bounded entry points: names the entry
/// point and its budget on an epoch interrupt; defers to
/// `log_with_env_failure` for everything else.
fn log_entry_failure(
    plugin_name: &str,
    entry: &str,
    timeout_ms: u64,
    max_memory_mb: u64,
    err: &WithEnvError,
) {
    match err {
        WithEnvError::Trapped {
            is_interrupt: true, ..
        } => {
            eprintln!(
                "yosh: plugin '{}': {} exceeded {}ms timeout — disabling for the rest of this session",
                plugin_name, entry, timeout_ms
            );
        }
        other => log_with_env_failure(plugin_name, other, max_memory_mb),
    }
}
```

2. `call_pre_exec`, `call_post_exec`, `call_on_cd`: replace the `with_env(...)` call + `log_with_env_failure` with (shown for `pre_exec`; the other two are identical apart from the closure and entry name):

```rust
            let ticks = plugin.limits.hook_deadline_ticks();
            let timeout_ms = plugin.limits.hook_timeout_ms;
            let max_mb = plugin.limits.max_memory_mb();
            if let Err(e) = with_deadline(plugin, env, ticks, |bindings, store| {
                bindings.yosh_plugin_hooks().call_pre_exec(store, cmd)
            }) {
                log_entry_failure(&plugin.name, "pre_exec", timeout_ms, max_mb, &e);
            }
```

3. `exec_command`: same pattern with `plugin.limits.command_deadline_ticks()` / `plugin.limits.command_timeout_ms` and entry label `&format!("command '{}'", name)`.

4. `call_pre_prompt`: rewrite onto the shared helpers — per-plugin budget, same message wording as today:

```rust
    pub fn call_pre_prompt(&mut self, env: &mut ShellEnv) {
        for plugin in &mut self.plugins {
            if plugin.capabilities & CAP_HOOK_PRE_PROMPT == 0 {
                continue;
            }
            if !plugin.implements_hook(HookName::PrePrompt) {
                continue;
            }
            let ticks = Some(plugin.limits.pre_prompt_ticks());
            let timeout_ms = plugin.limits.pre_prompt_timeout_ms;
            let max_mb = plugin.limits.max_memory_mb();
            if let Err(e) = with_deadline(plugin, env, ticks, |bindings, store| {
                bindings.yosh_plugin_hooks().call_pre_prompt(store)
            }) {
                log_entry_failure(&plugin.name, "pre_prompt", timeout_ms, max_mb, &e);
            }
        }
    }
```

(`log_entry_failure` on Skipped falls through to `log_with_env_failure`, which prints nothing — same as today.)

- [ ] **Step 4: Run the new tests + t25 regression**

Run in background: `cargo test --features test-helpers --test plugin t25 t26 t27 t28`
Expected: all PASS. (t25 exercises the pre_prompt path now flowing through `with_deadline` — its wording assertion is behavioral, not string-based.)

- [ ] **Step 5: Run the full lib suite**

Run: `cargo test --lib plugin`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/plugin/mod.rs tests/plugin.rs
git commit -m "feat(plugin): epoch timeouts on all hooks and plugin commands

Task 5 of docs/superpowers/plans/2026-07-07-plugin-runtime-limits.md.
pre_exec/post_exec/on_cd default to a 5s budget, commands default to
unlimited; all entry points share the with_deadline set/restore helper."
```

---

### Task 6: Memory-cap integration test

**Files:**
- Modify: `tests/plugin.rs` (t29 + `hog_plugin_wasm` helper)

**Interfaces:**
- Consumes: `hog_plugin` fixture (Task 4), limiter + hint (Task 3), `load_plugin_with_limits` (Task 2).

- [ ] **Step 1: Add the fixture helper and failing test**

In `tests/plugin.rs`, add next to the other fixture statics/helpers:

```rust
static HOG_PLUGIN_WASM: OnceLock<PathBuf> = OnceLock::new();

fn hog_plugin_wasm() -> PathBuf {
    ensure_built("hog_plugin", &HOG_PLUGIN_WASM)
}
```

And the test:

```rust
/// Memory cap — a plugin that allocates without bound is killed when
/// the limiter denies growth beyond max_memory_mb, is invalidated for
/// the session, and the shell survives.
#[test]
fn t29_memory_cap_kills_hog_plugin() {
    let _guard = lock_test();
    let wasm = hog_plugin_wasm();
    let mut env = fresh_env();
    let mut mgr = PluginManager::new();
    test_helpers::load_plugin_with_limits(
        &mut mgr,
        &wasm,
        &mut env,
        yosh_plugin_api::CAP_HOOK_PRE_EXEC,
        yosh::plugin::limits::LimitsConfig {
            max_memory_mb: Some(8),
            ..Default::default()
        },
    )
    .expect("load hog_plugin");

    // The unbounded allocator hits the 8 MiB cap quickly; the trap
    // invalidates the plugin. Bounded wall clock guards against the cap
    // silently not being installed (which would OOM-crawl for a long
    // time before the default 256 MiB, or hang the epoch baseline).
    let start = std::time::Instant::now();
    mgr.call_pre_exec(&mut env, "ls");
    assert!(
        start.elapsed() < std::time::Duration::from_secs(10),
        "hog was not killed by the memory cap: {:?}",
        start.elapsed()
    );

    // Invalidated: subsequent dispatch is a fast skip, shell still fine.
    let start = std::time::Instant::now();
    mgr.call_pre_exec(&mut env, "ls");
    assert!(start.elapsed() < std::time::Duration::from_millis(100));
}
```

- [ ] **Step 2: Run it**

Run: `cargo test --features test-helpers --test plugin t29`
Expected: PASS (feature already implemented in Task 3 — this is the integration-level proof; if it fails, debug Task 3's wiring, e.g. `store.limiter` not installed on the real store).

- [ ] **Step 3: Commit**

```bash
git add tests/plugin.rs
git commit -m "test(plugin): memory cap kills unbounded allocator, shell survives

Task 6 of docs/superpowers/plans/2026-07-07-plugin-runtime-limits.md."
```

---

### Task 7: Manager harness — continuous tick thread

**Files:**
- Create: `crates/yosh-plugin-manager/src/tick.rs`
- Modify: `crates/yosh-plugin-manager/src/lib.rs` (add `mod tick;` — check existing `mod` list)
- Modify: `crates/yosh-plugin-manager/src/runner.rs` (replace one-shot watchdog)
- Modify: `crates/yosh-plugin-manager/src/metadata_extract.rs` (replace one-shot watchdog)
- Modify: `crates/yosh-plugin-manager/tests/runner.rs` (`case_5` budget)

**Interfaces:**
- Produces: `tick::TickThread::spawn(engine: wasmtime::Engine) -> TickThread` (stops + joins on `Drop`), `tick::TICK_MS: u64 = 50`.
- Consumes: `precompile::make_engine()` (epoch interruption already on).

- [ ] **Step 1: Tighten `case_5` first (the failing test)**

In `crates/yosh-plugin-manager/tests/runner.rs::case_5_timeout_on_slow_plugin_pre_prompt`, replace the 15 s assertion + comment with:

```rust
    // Continuous 50ms tick thread (production parity): a 200ms deadline
    // trips within ~250ms plus scheduling noise. 5s is a generous CI
    // margin while still catching a regression to one-shot-watchdog
    // latency (3-8s) or an outright hang.
    assert!(
        elapsed < Duration::from_secs(5),
        "timeout interrupt did not fire promptly: {:?}",
        elapsed
    );
```

- [ ] **Step 2: Run to verify it fails (or flakes) under the one-shot watchdog**

Run: `cargo test -p yosh-plugin-manager --test runner case_5`
Expected: FAIL (elapsed typically 3–8 s on macOS). If it passes by luck, proceed anyway — the implementation makes it deterministic.

- [ ] **Step 3: Create `tick.rs`**

```rust
//! Continuous epoch-tick thread for the run/test harness. Mirrors the
//! production host's `TickThread` (`src/plugin/mod.rs`): bump the
//! engine epoch every `TICK_MS` so per-store tick deadlines trip within
//! one tick window, instead of the old one-shot watchdog whose single
//! bump competed with a busy guest for CPU (3-8s observed latency).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

pub const TICK_MS: u64 = 50;

pub struct TickThread {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl TickThread {
    pub fn spawn(engine: wasmtime::Engine) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_inner = stop.clone();
        let handle = std::thread::Builder::new()
            .name("yosh-plugin-manager-epoch-tick".to_string())
            .spawn(move || {
                while !stop_inner.load(Ordering::Acquire) {
                    std::thread::sleep(std::time::Duration::from_millis(TICK_MS));
                    engine.increment_epoch();
                }
            })
            .expect("spawn yosh-plugin-manager-epoch-tick thread");
        TickThread { stop, handle: Some(handle) }
    }
}

impl Drop for TickThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_thread_stops_and_joins_on_drop() {
        let engine = crate::precompile::make_engine().expect("engine");
        let t = TickThread::spawn(engine);
        std::thread::sleep(std::time::Duration::from_millis(120));
        drop(t); // must not hang
    }
}
```

Register in `lib.rs`: `pub(crate) mod tick;` (or `pub mod` if runner is a separate visibility domain — match how `runner` itself is declared).

- [ ] **Step 4: Rewire `runner::load_plugin`**

Replace the `store.set_epoch_deadline(1)` + one-shot watchdog block with:

```rust
    let mut store = Store::new(&engine, TestCtx::new(state));
    // Deadline in ticks; the continuous tick thread bumps the epoch
    // every TICK_MS, so worst-case overshoot is one tick window.
    let ticks = (timeout.as_millis() as u64).div_ceil(crate::tick::TICK_MS).max(1);
    store.set_epoch_deadline(ticks);
    let tick = crate::tick::TickThread::spawn(engine.clone());
```

and add the field to `LoadedPlugin` so the thread lives for the whole invocation:

```rust
pub struct LoadedPlugin {
    pub world: PluginWorld,
    pub store: Store<TestCtx>,
    pub engine: wasmtime::Engine,
    /// Keeps the epoch ticking until the invocation completes; stops
    /// and joins on drop.
    _tick: crate::tick::TickThread,
}
```

Set `_tick: tick` in the `Ok(LoadedPlugin { ... })`. Fix struct literals in any other construction site the compiler reports.

- [ ] **Step 5: Rewire `metadata_extract.rs`**

Replace its detached 5 s one-shot watchdog + `set_epoch_deadline(1)` with:

```rust
    // 100 ticks at 50ms ≈ 5s — same generous metadata budget as before,
    // now enforced by the continuous tick thread.
    store.set_epoch_deadline(100);
    let _tick = crate::tick::TickThread::spawn(engine.clone());
```

(`_tick` binding must stay alive across the `call_metadata` — bind it before the call in the same scope; do not name it `_`.) Update the module doc comment describing the old watchdog.

- [ ] **Step 6: Run the manager suite**

Run in background: `cargo test -p yosh-plugin-manager`
Expected: all PASS, including `case_5` now under 5 s (typically ~0.3 s) and `tick_thread_stops_and_joins_on_drop`.

- [ ] **Step 7: Commit**

```bash
git add crates/yosh-plugin-manager/src/tick.rs crates/yosh-plugin-manager/src/lib.rs crates/yosh-plugin-manager/src/runner.rs crates/yosh-plugin-manager/src/metadata_extract.rs crates/yosh-plugin-manager/tests/runner.rs
git commit -m "fix(plugin-manager): continuous tick thread replaces one-shot watchdogs

Task 7 of docs/superpowers/plans/2026-07-07-plugin-runtime-limits.md.
CPU-bound guests now trap within ~one tick of the deadline (was 3-8s);
case_5 budget restored from 15s to 5s (spec §10 parity)."
```

---

### Task 8: Manifest → lockfile passthrough of the limit fields

**Files:**
- Modify: `crates/yosh-plugin-manager/src/config.rs` (`RawPluginEntry`, `PluginDecl`, `load_config` mapping)
- Modify: `crates/yosh-plugin-manager/src/lockfile.rs` (`LockEntry` + round-trip test)
- Modify: `crates/yosh-plugin-manager/src/sync.rs` (both `LockEntry` construction sites)

**Interfaces:**
- Consumes: existing `PluginDecl` → `LockEntry` flow in `sync.rs` (GitHub branch ~line 251, local branch ~line 311).
- Produces: the four fields round-trip `plugins.toml` → `plugins.lock`, where the host's `PluginEntry` (Task 2) reads them.

Background: today NO user-tuning fields pass from manifest to lock (`allowed_commands`/`files_root` have the same gap — tracked separately). These four new fields must pass through or users could never set them.

- [ ] **Step 1: Write failing tests**

In `crates/yosh-plugin-manager/src/lockfile.rs` tests, extend `sample_entry()` (add the four fields as `None`) and add:

```rust
    #[test]
    fn limit_fields_round_trip() {
        let mut e = sample_entry();
        e.max_memory_mb = Some(64);
        e.hook_timeout_ms = Some(1000);
        e.command_timeout_ms = Some(30_000);
        e.pre_prompt_timeout_ms = Some(250);
        let lf = LockFile { plugin: vec![e.clone()] };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugins.lock");
        save_lockfile(&path, &lf).unwrap();
        let loaded = load_lockfile(&path).unwrap();
        assert_eq!(loaded.plugin[0], e);
    }
```

In `crates/yosh-plugin-manager/src/config.rs` tests (follow the existing test style there — check how `load_config` is tested; if no test module exists, add one with a tempfile-based test):

```rust
    #[test]
    fn limit_fields_parse_into_decl() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(
            &mut f,
            br#"
[[plugin]]
name = "limited"
source = "local:/tmp/x.wasm"
max_memory_mb = 64
hook_timeout_ms = 1000
command_timeout_ms = 30000
pre_prompt_timeout_ms = 250
"#,
        )
        .unwrap();
        let decls = load_config(f.path()).unwrap();
        assert_eq!(decls[0].max_memory_mb, Some(64));
        assert_eq!(decls[0].hook_timeout_ms, Some(1000));
        assert_eq!(decls[0].command_timeout_ms, Some(30_000));
        assert_eq!(decls[0].pre_prompt_timeout_ms, Some(250));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p yosh-plugin-manager limit_fields`
Expected: FAIL to compile (fields don't exist).

- [ ] **Step 3: Add the fields**

1. `RawPluginEntry` and `PluginDecl` each gain:

```rust
    pub max_memory_mb: Option<u64>,
    pub hook_timeout_ms: Option<u64>,
    pub command_timeout_ms: Option<u64>,
    pub pre_prompt_timeout_ms: Option<u64>,
```

(`RawPluginEntry` fields are private — match its existing field visibility.) Map them in `load_config`'s Raw→Decl conversion.

2. `LockEntry` gains the same four fields with `#[serde(default, skip_serializing_if = "Option::is_none")]`.

3. Both `Ok(LockEntry { ... })` sites in `sync.rs` copy them from the decl:

```rust
                max_memory_mb: decl.max_memory_mb,
                hook_timeout_ms: decl.hook_timeout_ms,
                command_timeout_ms: decl.command_timeout_ms,
                pre_prompt_timeout_ms: decl.pre_prompt_timeout_ms,
```

4. Fix any other `LockEntry`/`PluginDecl` struct literals the compiler reports (tests, `install.rs`, `update.rs`) by adding `..` defaults is not possible for non-Default structs — add the four `None` fields explicitly.

- [ ] **Step 4: Run the manager suite**

Run in background: `cargo test -p yosh-plugin-manager`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/yosh-plugin-manager/src/config.rs crates/yosh-plugin-manager/src/lockfile.rs crates/yosh-plugin-manager/src/sync.rs
git commit -m "feat(plugin-manager): pass limit fields through manifest -> lockfile

Task 8 of docs/superpowers/plans/2026-07-07-plugin-runtime-limits.md."
```

---

### Task 9: Docs, TODO cleanup, spec note, full verification

**Files:**
- Modify: `docs/yosh/plugin.md` (config table + Resource Limits section)
- Modify: `TODO.md` (delete resolved items)
- Modify: `docs/superpowers/specs/2026-07-07-plugin-runtime-limits-design.md` (attribution note)

- [ ] **Step 1: Document the fields**

In `docs/yosh/plugin.md` `#### Fields` table, add:

```markdown
| `max_memory_mb` | No | Linear-memory cap in MiB (default 256, max 4096) |
| `hook_timeout_ms` | No | Budget for `pre_exec`/`post_exec`/`on_cd` hooks in ms; `0` = unlimited (default 5000) |
| `command_timeout_ms` | No | Budget for plugin commands in ms; `0` = unlimited (default) |
| `pre_prompt_timeout_ms` | No | Per-plugin `pre_prompt` budget in ms, 1–60000 (default 500; overrides `YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS`) |
```

After the "Confining `files` Access" section add:

```markdown
#### Resource Limits

Every plugin runs under a linear-memory cap (`max_memory_mb`, default
256 MiB) and per-call time budgets. A plugin that exceeds a budget or
the memory cap is interrupted, disabled for the rest of the session,
and reported on stderr with the entry point and limit that tripped.
Hooks (`pre_exec`, `post_exec`, `on_cd`) default to a 5-second budget;
`pre_prompt` defaults to 500 ms; custom commands are unlimited by
default because users invoke them interactively — set
`command_timeout_ms` to bound them. Out-of-range values are clamped
with a warning at load time.
```

- [ ] **Step 2: TODO.md cleanup**

Delete (whole bullets):
- The runner watchdog one-shot item (~L446, starts "`runner::load_plugin` watchdog uses a one-shot detached thread").
- The runtime-limits deferral (~L463, starts "Plugin runtime limits (fuel / memory caps / pre-prompt timeout)").

- [ ] **Step 3: Spec amendment**

In `docs/superpowers/specs/2026-07-07-plugin-runtime-limits-design.md` §4.1, replace the "best-effort attribution … no structured 'limiter denied' code" sentence with a note that the implementation uses a custom `ResourceLimiter` whose `denied` flag gives deterministic attribution (no trap-message sniffing), and the hint prints as `(memory limit N MiB exceeded)`.

- [ ] **Step 4: Full verification (background — suite takes minutes)**

Run in background:
```bash
cargo test --features test-helpers 2>&1 | tail -30
./e2e/run_tests.sh 2>&1 | tail -10
```
Expected: all green (known env-specific exceptions: LC_NUMERIC e2e flake, wasm32-wasip2 plugin-manager test on machines without the target — per project memory).

- [ ] **Step 5: Commit**

```bash
git add docs/yosh/plugin.md TODO.md docs/superpowers/specs/2026-07-07-plugin-runtime-limits-design.md
git commit -m "docs(plugin): document resource limits; drop resolved TODO items

Task 9 of docs/superpowers/plans/2026-07-07-plugin-runtime-limits.md.
Closes the v0.2.0 runtime-limits deferral and the one-shot watchdog
follow-up."
```
