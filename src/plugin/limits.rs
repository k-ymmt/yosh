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

/// Per-store memory limiter. Denies any linear-memory growth beyond
/// `max_memory_bytes` and records the denial so `with_env` can
/// attribute the guest's subsequent trap (a failed `memory.grow`
/// surfaces as an allocator abort, which carries no structured cause).
pub(in crate::plugin) struct MemoryLimiter {
    max_memory_bytes: usize,
    pub(in crate::plugin) denied: bool,
}

impl MemoryLimiter {
    pub(in crate::plugin) fn new(max_memory_bytes: usize) -> Self {
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
}
