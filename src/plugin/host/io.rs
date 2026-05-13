//! `yosh:plugin/io` host import — write to host stdout/stderr.
//! Granted via CAP_IO.

use std::io::Write;

use super::super::generated::yosh::plugin::types::{ErrorCode, IoStream};
use super::HostContext;

pub fn host_io_write(ctx: &HostContext, target: IoStream, data: &[u8]) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    let result = match target {
        IoStream::Stdout => std::io::stdout().write_all(data),
        IoStream::Stderr => std::io::stderr().write_all(data),
    };
    result.map_err(|_| ErrorCode::IoFailed)
}

pub fn deny_io_write(_ctx: &HostContext, _target: IoStream, _data: &[u8]) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

#[cfg(test)]
mod tests {
    //! Spot test for the metadata-contract via io_write.

    use super::super::test_helpers::null_env_ctx;
    use super::*;

    #[test]
    fn io_write_denied_when_env_null() {
        let ctx = null_env_ctx();
        let result = host_io_write(&ctx, IoStream::Stdout, b"hi");
        assert_eq!(result, Err(ErrorCode::Denied));
    }
}
