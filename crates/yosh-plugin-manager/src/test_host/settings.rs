//! In-memory `yosh:plugin/settings` host import backed by
//! `TestState.settings`. Capability-free, mirroring the production
//! host: every plugin may read its own settings.toml, so the test
//! harness serves whatever the scenario injected (default: none).

use super::TestState;
use crate::generated::yosh::plugin::types::ErrorCode;

pub fn host_read(state: &TestState) -> Result<Option<String>, ErrorCode> {
    Ok(state.settings.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No capability gate: caps = 0 still reads (none by default).
    #[test]
    fn read_defaults_to_none_without_any_cap() {
        let s = TestState::default();
        assert_eq!(host_read(&s), Ok(None));
        assert!(s.denied_log.is_empty());
    }

    #[test]
    fn read_returns_injected_settings() {
        let s = TestState {
            settings: Some("key = 1\n".to_string()),
            ..TestState::default()
        };
        assert_eq!(host_read(&s), Ok(Some("key = 1\n".to_string())));
    }
}
