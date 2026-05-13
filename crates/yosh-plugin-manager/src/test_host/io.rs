//! In-memory `yosh:plugin/io` host import backed by
//! `TestState.stdout` / `TestState.stderr`. Gated on CAP_IO.

use super::TestState;
use crate::generated::yosh::plugin::types::{ErrorCode, IoStream};
use yosh_plugin_api::CAP_IO;

pub fn host_write(state: &mut TestState, target: IoStream, data: &[u8]) -> Result<(), ErrorCode> {
    if state.caps & CAP_IO == 0 {
        return Err(ErrorCode::Denied);
    }
    match target {
        IoStream::Stdout => state.stdout.extend_from_slice(data),
        IoStream::Stderr => state.stderr.extend_from_slice(data),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_denied_without_cap() {
        let mut s = TestState::default();
        assert_eq!(
            host_write(&mut s, IoStream::Stdout, b"x"),
            Err(ErrorCode::Denied)
        );
        assert!(s.stdout.is_empty());
    }

    #[test]
    fn write_appends_to_stdout() {
        let mut s = TestState::with_caps(CAP_IO);
        host_write(&mut s, IoStream::Stdout, b"hello ").unwrap();
        host_write(&mut s, IoStream::Stdout, b"world").unwrap();
        assert_eq!(s.stdout, b"hello world");
    }

    #[test]
    fn write_targets_stderr_separately() {
        let mut s = TestState::with_caps(CAP_IO);
        host_write(&mut s, IoStream::Stderr, b"err").unwrap();
        assert!(s.stdout.is_empty());
        assert_eq!(s.stderr, b"err");
    }
}
