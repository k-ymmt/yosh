//! Epoch-deadline constants and the pre-prompt timeout setting.
//!
//! Every plugin `Store` runs under wasmtime epoch interruption: the
//! engine's tick thread (`super::engine`) bumps the epoch every
//! [`TICK_MS`], and each store traps once its deadline (in ticks) is
//! reached.

// ── Pre-prompt timeout constants ───────────────────────────────────────

/// Default pre-prompt hook timeout in milliseconds. A plugin's
/// `pre_prompt` hook is interrupted with `Trap::Interrupt` if it has not
/// returned within this budget, then permanently invalidated for the
/// session.
const DEFAULT_PRE_PROMPT_TIMEOUT_MS: u64 = 500;

/// Hard upper bound on the configurable pre-prompt timeout. Values above
/// this clamp back to the default with a stderr warning.
const MAX_PRE_PROMPT_TIMEOUT_MS: u64 = 60_000;

/// Tick interval for the `PluginManager` epoch-bumping thread. Worst-case
/// overshoot of any deadline is one tick window.
pub(super) const TICK_MS: u64 = 50;

/// Environment variable that overrides `DEFAULT_PRE_PROMPT_TIMEOUT_MS`.
const PRE_PROMPT_TIMEOUT_ENV: &str = "YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS";

/// Effectively-never epoch deadline. Set as the persistent baseline on
/// every `LoadedPlugin.store` so non-`pre_prompt` hooks never trap on
/// the engine epoch. `call_pre_prompt` overrides this with a tight bound
/// per invocation and restores it after the call.
pub(super) const STORE_BASELINE_DEADLINE_TICKS: u64 = u64::MAX / 2;

/// Epoch deadline applied to the one-shot scratch store used during
/// `metadata()` extraction at plugin load. With `epoch_interruption(true)`
/// every store needs an explicit deadline (default 0 traps immediately);
/// 100 ticks (~5 s at `TICK_MS = 50`) is a generous bound that never
/// trips for well-behaved plugins while still catching infinite loops.
pub(super) const METADATA_SCRATCH_DEADLINE_TICKS: u64 = 100;

/// Pure parser for the `YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS` env var.
///
/// * `Ok(n)` — caller should use `n` directly. `None` → default,
///   `Some(valid integer in [1, 60000])` → that integer.
/// * `Err(raw)` — input was present but invalid (non-numeric, 0, or
///   greater than 60000). The raw input is returned so the caller can
///   phrase a warning that quotes what the user typed. The caller
///   decides whether to fall back to the default.
fn parse_pre_prompt_timeout(input: Option<&str>) -> Result<u64, String> {
    let Some(s) = input else {
        return Ok(DEFAULT_PRE_PROMPT_TIMEOUT_MS);
    };
    match s.parse::<u64>() {
        Ok(n) if (1..=MAX_PRE_PROMPT_TIMEOUT_MS).contains(&n) => Ok(n),
        _ => Err(s.to_string()),
    }
}

/// Resolve the pre-prompt timeout from `YOSH_PLUGIN_PRE_PROMPT_TIMEOUT_MS`.
/// Invalid values warn and fall back to the default. The pure parser
/// stays unit-testable; the warning lives here because it is I/O.
pub(super) fn pre_prompt_timeout_from_env() -> u64 {
    let raw = std::env::var(PRE_PROMPT_TIMEOUT_ENV).ok();
    match parse_pre_prompt_timeout(raw.as_deref()) {
        Ok(n) => n,
        Err(invalid) => {
            let display: &str = if invalid.is_empty() {
                "<empty>"
            } else {
                &invalid
            };
            eprintln!(
                "yosh: plugin: {}={} invalid (must be 1..={} ms); using default {}ms",
                PRE_PROMPT_TIMEOUT_ENV,
                display,
                MAX_PRE_PROMPT_TIMEOUT_MS,
                DEFAULT_PRE_PROMPT_TIMEOUT_MS
            );
            DEFAULT_PRE_PROMPT_TIMEOUT_MS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pre_prompt_timeout_unset_returns_ok_default() {
        assert_eq!(
            parse_pre_prompt_timeout(None),
            Ok(DEFAULT_PRE_PROMPT_TIMEOUT_MS)
        );
    }

    #[test]
    fn parse_pre_prompt_timeout_valid_in_range() {
        assert_eq!(parse_pre_prompt_timeout(Some("250")), Ok(250));
        assert_eq!(parse_pre_prompt_timeout(Some("1")), Ok(1));
        assert_eq!(parse_pre_prompt_timeout(Some("60000")), Ok(60_000));
    }

    #[test]
    fn parse_pre_prompt_timeout_zero_returns_invalid() {
        assert_eq!(parse_pre_prompt_timeout(Some("0")), Err("0".to_string()));
    }

    #[test]
    fn parse_pre_prompt_timeout_above_max_returns_invalid() {
        assert_eq!(
            parse_pre_prompt_timeout(Some("60001")),
            Err("60001".to_string())
        );
        assert_eq!(
            parse_pre_prompt_timeout(Some("999999")),
            Err("999999".to_string())
        );
    }

    #[test]
    fn parse_pre_prompt_timeout_non_numeric_returns_invalid() {
        assert_eq!(
            parse_pre_prompt_timeout(Some("abc")),
            Err("abc".to_string())
        );
        assert_eq!(parse_pre_prompt_timeout(Some("")), Err("".to_string()));
        assert_eq!(parse_pre_prompt_timeout(Some("-1")), Err("-1".to_string()));
    }
}
