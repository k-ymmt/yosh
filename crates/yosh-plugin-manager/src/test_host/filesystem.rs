//! In-memory `yosh:plugin/filesystem` host imports backed by
//! `TestState.cwd`. Gated on CAP_FILESYSTEM. The cwd is virtual —
//! changing it does not call `std::env::set_current_dir`.

use std::path::PathBuf;

use super::TestState;
use crate::generated::yosh::plugin::types::ErrorCode;
use yosh_plugin_api::CAP_FILESYSTEM;

pub fn host_cwd(state: &TestState) -> Result<String, ErrorCode> {
    if state.caps & CAP_FILESYSTEM == 0 {
        return Err(ErrorCode::Denied);
    }
    Ok(state.cwd.to_string_lossy().into_owned())
}

pub fn host_set_cwd(state: &mut TestState, path: &str) -> Result<(), ErrorCode> {
    if state.caps & CAP_FILESYSTEM == 0 {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    state.cwd = PathBuf::from(path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cwd_denied_without_cap() {
        let s = TestState::default();
        assert_eq!(host_cwd(&s), Err(ErrorCode::Denied));
    }

    #[test]
    fn cwd_returns_state_cwd() {
        let mut s = TestState::with_caps(CAP_FILESYSTEM);
        s.cwd = PathBuf::from("/tmp");
        assert_eq!(host_cwd(&s), Ok("/tmp".to_string()));
    }

    #[test]
    fn set_cwd_updates_state() {
        let mut s = TestState::with_caps(CAP_FILESYSTEM);
        host_set_cwd(&mut s, "/home").unwrap();
        assert_eq!(s.cwd, PathBuf::from("/home"));
    }

    #[test]
    fn set_cwd_rejects_empty() {
        let mut s = TestState::with_caps(CAP_FILESYSTEM);
        assert_eq!(host_set_cwd(&mut s, ""), Err(ErrorCode::InvalidArgument));
    }

    #[test]
    fn set_cwd_denied_without_cap() {
        let mut s = TestState::default();
        assert_eq!(host_set_cwd(&mut s, "/home"), Err(ErrorCode::Denied));
        assert_eq!(s.cwd, PathBuf::new());
    }
}
