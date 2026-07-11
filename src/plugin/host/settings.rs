//! `yosh:plugin/settings` host import — read the plugin's own
//! settings.toml. Capability-free: the linker always registers the
//! real implementation (there is no deny variant). The path is fixed
//! at load time (`~/.config/yosh/plugins/<name>/settings.toml`, see
//! `config::settings_path_for`), so a plugin can structurally reach
//! only its own settings file.
//!
//! Error mapping:
//! - `settings_path == None` (no HOME / unsafe name) → `Ok(None)`
//! - file does not exist                             → `Ok(None)`
//! - any other I/O error (incl. invalid UTF-8)       → `IoFailed`

use super::super::generated::yosh::plugin::types::ErrorCode;
use super::HostContext;

pub fn host_settings_read(ctx: &HostContext) -> Result<Option<String>, ErrorCode> {
    ctx.ensure_bound()?;
    let Some(path) = &ctx.settings_path else {
        return Ok(None);
    };
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(ErrorCode::IoFailed),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::{bound_env_ctx, ctx_with_settings_path, null_env_ctx};
    use super::*;
    use crate::env::ShellEnv;
    use tempfile::tempdir;

    /// Metadata contract: no env binding → Denied, like every other
    /// host import.
    #[test]
    fn settings_read_denied_when_env_null() {
        let ctx = null_env_ctx();
        assert_eq!(host_settings_read(&ctx), Err(ErrorCode::Denied));
    }

    /// No resolved path (HOME unset / unsafe plugin name) behaves as
    /// "no settings file", not as an error.
    #[test]
    fn settings_read_none_when_path_unresolved() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = bound_env_ctx(&mut env);
        assert_eq!(host_settings_read(&ctx), Ok(None));
    }

    #[test]
    fn settings_read_none_when_file_missing() {
        let dir = tempdir().unwrap();
        // Parent dir of the path doesn't exist either — still NotFound.
        let path = dir.path().join("plugins/demo/settings.toml");
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_settings_path(&mut env, &path);
        assert_eq!(host_settings_read(&ctx), Ok(None));
    }

    #[test]
    fn settings_read_returns_file_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "greeting = \"hello\"\n").unwrap();
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_settings_path(&mut env, &path);
        assert_eq!(
            host_settings_read(&ctx),
            Ok(Some("greeting = \"hello\"\n".to_string()))
        );
    }

    /// TOML must be UTF-8; junk bytes surface as IoFailed, not a panic
    /// and not silently-lossy text.
    #[test]
    fn settings_read_invalid_utf8_is_io_failed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_settings_path(&mut env, &path);
        assert_eq!(host_settings_read(&ctx), Err(ErrorCode::IoFailed));
    }
}
