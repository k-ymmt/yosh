//! Temporary single-file home for host_* and deny_* functions.
//!
//! PR-A scaffolding step (see
//! `docs/superpowers/specs/2026-05-06-sp1-plugin-host-redesign-design.md`).
//! PR-B splits this file into per-capability submodules and deletes it.

#[cfg(test)]
mod tests {
    //! Unit tests for the metadata contract: every host import must
    //! short-circuit to `Err(Denied)` when `HostContext.env` is null. This
    //! is the canonical enforcement point for the §5 metadata-cannot-reach-
    //! host-APIs invariant. The pointer is null during the single
    //! `metadata()` call at startup and between `with_env` invocations, so
    //! returning `Denied` from these functions blocks any plugin that tries
    //! to call them outside of a properly-bound dispatch.
    //!
    //! Replaces what would have been `tests/plugin.rs::t04_metadata_cannot_
    //! reach_host_apis` — a contrived plugin whose `metadata` calls `cwd()`
    //! is harder to author than this direct call. Same invariant, simpler
    //! test.
    use super::super::super::generated::yosh::plugin::types::ErrorCode;
    use super::super::test_helpers::{bound_env_ctx, null_env_ctx};
    use crate::env::ShellEnv;

    #[test]
    fn ensure_bound_returns_denied_when_env_null() {
        let ctx = null_env_ctx();
        assert_eq!(ctx.ensure_bound(), Err(ErrorCode::Denied));
    }

    #[test]
    fn ensure_bound_returns_ok_when_env_bound() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = bound_env_ctx(&mut env);
        assert_eq!(ctx.ensure_bound(), Ok(()));
    }

    #[test]
    fn bound_env_returns_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = ctx.bound_env();
        assert!(matches!(result, Err(ErrorCode::Denied)));
    }

    #[test]
    fn bound_env_returns_env_when_bound() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        let result = ctx.bound_env();
        assert!(result.is_ok());
    }
}
