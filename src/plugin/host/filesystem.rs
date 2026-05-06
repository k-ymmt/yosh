//! `yosh:plugin/filesystem` host imports — cwd / set-cwd. Granted
//! via CAP_FILESYSTEM. These do not read or write `ShellEnv` directly;
//! they call `std::env::current_dir` / `set_current_dir` after the
//! metadata-contract guard.

use super::super::generated::yosh::plugin::types::ErrorCode;
use super::HostContext;

pub fn host_filesystem_cwd(ctx: &mut HostContext) -> Result<String, ErrorCode> {
    ctx.ensure_bound()?;
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|_| ErrorCode::IoFailed)
}

pub fn deny_filesystem_cwd(_ctx: &mut HostContext) -> Result<String, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn host_filesystem_set_cwd(
    ctx: &mut HostContext,
    path: String,
) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    std::env::set_current_dir(&path).map_err(|_| ErrorCode::IoFailed)
}

pub fn deny_filesystem_set_cwd(
    _ctx: &mut HostContext,
    _path: String,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::null_env_ctx;
    use super::*;

    #[test]
    fn filesystem_cwd_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        assert_eq!(host_filesystem_cwd(&mut ctx), Err(ErrorCode::Denied));
    }
}
