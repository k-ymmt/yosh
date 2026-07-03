//! `yosh:plugin/filesystem` host imports — cwd / set-cwd. Granted
//! via CAP_FILESYSTEM.
//!
//! Capability semantics: `set-cwd` is a *process-global* chdir by
//! design — the capability exists so plugins (e.g. directory-jump
//! hooks) can move the shell itself, exactly like the `cd` builtin.
//! Granting `filesystem` therefore hands the plugin control over
//! relative-path resolution for the whole shell. To keep shell state
//! consistent with that global effect, `set-cwd` mirrors the cd
//! builtin's PWD/OLDPWD bookkeeping on the bound `ShellEnv`.

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

pub fn host_filesystem_set_cwd(ctx: &HostContext, path: &str) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    std::env::set_current_dir(path).map_err(|_| ErrorCode::IoFailed)?;
    // Physical semantics, like `cd -P`: report the resolved directory.
    let new_pwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|_| ErrorCode::IoFailed)?;
    ctx.bound_env_with(|env| {
        if let Some(old) = env.vars.get("PWD").map(|s| s.to_string()) {
            let _ = env.vars.set("OLDPWD", old);
        }
        let _ = env.vars.set("PWD", new_pwd);
    })
}

pub fn deny_filesystem_set_cwd(_ctx: &HostContext, _path: &str) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::{bound_env_ctx, null_env_ctx};
    use super::*;
    use crate::env::ShellEnv;

    #[test]
    fn filesystem_cwd_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        assert_eq!(host_filesystem_cwd(&mut ctx), Err(ErrorCode::Denied));
    }

    #[test]
    fn set_cwd_updates_pwd_and_oldpwd() {
        // set-cwd is a process-global chdir by design (CAP_FILESYSTEM);
        // it must keep the shell's PWD/OLDPWD bookkeeping consistent,
        // exactly like the cd builtin.
        let orig = std::env::current_dir().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let canon = std::fs::canonicalize(tmp.path()).unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars
            .set("PWD", orig.to_string_lossy().into_owned())
            .unwrap();
        let ctx = bound_env_ctx(&mut env);
        let result = host_filesystem_set_cwd(&ctx, &canon.to_string_lossy());

        // Restore the process cwd before asserting so a failure does not
        // leave the lib-test process in a soon-to-be-deleted tempdir.
        std::env::set_current_dir(&orig).unwrap();

        assert_eq!(result, Ok(()));
        assert_eq!(env.vars.get("PWD"), Some(canon.to_string_lossy().as_ref()));
        assert_eq!(
            env.vars.get("OLDPWD"),
            Some(orig.to_string_lossy().as_ref())
        );
    }
}
