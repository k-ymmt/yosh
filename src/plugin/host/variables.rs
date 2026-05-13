//! `yosh:plugin/variables` host imports — read/write/export shell
//! variables. Granted via CAP_VARIABLES. The deny stubs short-circuit
//! to `Err(Denied)` regardless of env state and are wired in by the
//! linker when the capability bit is clear.

use super::super::generated::yosh::plugin::types::ErrorCode;
use super::HostContext;

pub fn host_variables_get(ctx: &HostContext, name: &str) -> Result<Option<String>, ErrorCode> {
    let env = ctx.bound_env_ref()?;
    Ok(env.vars.get(name).map(|s| s.to_string()))
}

pub fn deny_variables_get(_ctx: &HostContext, _name: &str) -> Result<Option<String>, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn host_variables_set(ctx: &HostContext, name: &str, value: &str) -> Result<(), ErrorCode> {
    ctx.bound_env_with(|env| env.vars.set(name, value).map_err(|_| ErrorCode::IoFailed))?
}

pub fn deny_variables_set(_ctx: &HostContext, _name: &str, _value: &str) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

/// `variables.export-env` — name in WIT is `export-env` (because
/// `export` is a reserved WIT keyword); the wit-bindgen-generated
/// Rust function is `export_env`.
pub fn host_variables_export_env(
    ctx: &HostContext,
    name: &str,
    value: &str,
) -> Result<(), ErrorCode> {
    ctx.bound_env_with(|env| {
        env.vars.set(name, value).map_err(|_| ErrorCode::IoFailed)?;
        env.vars.export(name);
        Ok(())
    })?
}

pub fn deny_variables_export_env(
    _ctx: &HostContext,
    _name: &str,
    _value: &str,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

#[cfg(test)]
mod tests {
    //! One spot test confirms the metadata-contract is reachable through
    //! the variables capability. The full structural guarantee is
    //! verified by the `ensure_bound` tests in `host/mod.rs`. Behavioral
    //! tests for set/get/export-env round-trip belong in this module too
    //! — add them as the need arises (the existing legacy tests covered
    //! only the deny path).

    use super::super::test_helpers::null_env_ctx;
    use super::*;

    #[test]
    fn variables_get_denied_when_env_null() {
        let ctx = null_env_ctx();
        let result = host_variables_get(&ctx, "PATH");
        assert_eq!(result, Err(ErrorCode::Denied));
    }
}
