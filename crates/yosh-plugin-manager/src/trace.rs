//! Dependency-free trace channel for the run/test harness, enabled by
//! setting `YOSH_PLUGIN_TRACE` to anything but empty or `0`. This
//! supersedes the 2026-05-12 spec §6 `RUST_LOG` (log-crate) promise —
//! see docs/superpowers/specs/2026-07-09-plugin-dx-sweep-design.md
//! §3.7 for the zero-dependency rationale.

use std::sync::OnceLock;

static ENABLED: OnceLock<bool> = OnceLock::new();

/// Whether tracing is on. Reads `YOSH_PLUGIN_TRACE` once per process.
pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| parse_enabled(std::env::var("YOSH_PLUGIN_TRACE").ok().as_deref()))
}

fn parse_enabled(v: Option<&str>) -> bool {
    matches!(v, Some(x) if !x.is_empty() && x != "0")
}

/// `eprintln!` with a `yosh-plugin[trace]:` prefix, compiled to a
/// cheap branch when tracing is off (arguments are only evaluated
/// inside the branch).
macro_rules! trace {
    ($($t:tt)*) => {
        if $crate::trace::enabled() {
            eprintln!("yosh-plugin[trace]: {}", format_args!($($t)*));
        }
    };
}
pub(crate) use trace;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_enabled_truth_table() {
        assert!(!parse_enabled(None));
        assert!(!parse_enabled(Some("")));
        assert!(!parse_enabled(Some("0")));
        assert!(parse_enabled(Some("1")));
        assert!(parse_enabled(Some("true")));
    }
}
