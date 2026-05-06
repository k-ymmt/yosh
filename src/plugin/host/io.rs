//! `yosh:plugin/io` host import — write to host stdout/stderr.
//! Granted via CAP_IO.

use std::io::Write;

use super::super::generated::yosh::plugin::types::{ErrorCode, IoStream};
use super::HostContext;

pub fn host_io_write(
    ctx: &mut HostContext,
    target: IoStream,
    data: Vec<u8>,
) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    let result = match target {
        IoStream::Stdout => std::io::stdout().write_all(&data),
        IoStream::Stderr => std::io::stderr().write_all(&data),
    };
    result.map_err(|_| ErrorCode::IoFailed)
}

pub fn deny_io_write(
    _ctx: &mut HostContext,
    _target: IoStream,
    _data: Vec<u8>,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

#[cfg(test)]
mod tests {
    //! Spot test for the metadata-contract via io_write.

    use super::super::test_helpers::null_env_ctx;
    use super::*;

    #[test]
    fn io_write_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_io_write(&mut ctx, IoStream::Stdout, b"hi".to_vec());
        assert_eq!(result, Err(ErrorCode::Denied));
    }
}
