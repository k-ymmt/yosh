//! In-memory `yosh:plugin/variables` host imports backed by
//! `TestState.vars` / `TestState.exported`. Gated on
//! CAP_VARIABLES_READ and CAP_VARIABLES_WRITE.

use super::TestState;
use crate::generated::yosh::plugin::types::ErrorCode;
use yosh_plugin_api::{CAP_VARIABLES_READ, CAP_VARIABLES_WRITE};

pub fn host_get(state: &TestState, name: &str) -> Result<Option<String>, ErrorCode> {
    if state.caps & CAP_VARIABLES_READ == 0 {
        return Err(ErrorCode::Denied);
    }
    Ok(state.vars.get(name).cloned())
}

pub fn host_set(state: &mut TestState, name: &str, value: &str) -> Result<(), ErrorCode> {
    if state.caps & CAP_VARIABLES_WRITE == 0 {
        return Err(ErrorCode::Denied);
    }
    state.vars.insert(name.to_string(), value.to_string());
    state.set_log.push((name.to_string(), value.to_string()));
    Ok(())
}

pub fn host_export_env(state: &mut TestState, name: &str, value: &str) -> Result<(), ErrorCode> {
    if state.caps & CAP_VARIABLES_WRITE == 0 {
        return Err(ErrorCode::Denied);
    }
    state.vars.insert(name.to_string(), value.to_string());
    state.exported.insert(name.to_string());
    state.export_log.push((name.to_string(), value.to_string()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(caps: u32) -> TestState {
        let mut s = TestState::default();
        s.caps = caps;
        s
    }

    #[test]
    fn get_denied_without_cap() {
        let s = state_with(0);
        assert_eq!(host_get(&s, "FOO"), Err(ErrorCode::Denied));
    }

    #[test]
    fn get_returns_none_for_unset() {
        let s = state_with(CAP_VARIABLES_READ);
        assert_eq!(host_get(&s, "FOO"), Ok(None));
    }

    #[test]
    fn get_returns_value_when_set() {
        let mut s = state_with(CAP_VARIABLES_READ);
        s.vars.insert("FOO".into(), "bar".into());
        assert_eq!(host_get(&s, "FOO"), Ok(Some("bar".into())));
    }

    #[test]
    fn set_denied_without_cap() {
        let mut s = state_with(CAP_VARIABLES_READ);
        assert_eq!(host_set(&mut s, "FOO", "bar"), Err(ErrorCode::Denied));
        assert!(s.set_log.is_empty());
    }

    #[test]
    fn set_records_log() {
        let mut s = state_with(CAP_VARIABLES_WRITE);
        host_set(&mut s, "FOO", "bar").unwrap();
        assert_eq!(s.vars.get("FOO").map(|s| s.as_str()), Some("bar"));
        assert_eq!(s.set_log, vec![("FOO".into(), "bar".into())]);
    }

    #[test]
    fn export_env_records_export_set() {
        let mut s = state_with(CAP_VARIABLES_WRITE);
        host_export_env(&mut s, "PATH", "/bin").unwrap();
        assert!(s.exported.contains("PATH"));
        assert_eq!(s.export_log, vec![("PATH".into(), "/bin".into())]);
    }
}
