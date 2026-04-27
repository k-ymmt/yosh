//! Asset filename resolution for the plugin manager.
//!
//! With the v0.2.0 component model migration, plugin artefacts are
//! platform-independent `.wasm` files — no per-OS / per-arch suffix.
//! The default template is `{name}.wasm` and the legacy `{os}` /
//! `{arch}` / `{ext}` tokens are rejected with a migration message
//! (the plugin author must republish a single `.wasm` asset).
//!
//! See `docs/superpowers/specs/2026-04-27-wasm-plugin-runtime-design.md`
//! §7 for the rationale.

/// Default asset filename template. v0.2.0+: a single platform-independent
/// `.wasm` file per plugin.
pub const DEFAULT_TEMPLATE: &str = "{name}.wasm";

/// Convert plugin name to a form suitable for library names: hyphens
/// become underscores. Kept for backwards-compatibility with existing
/// templates; new `.wasm` artefacts typically use the plugin name as-is.
pub fn normalize_name(name: &str) -> String {
    name.replace('-', "_")
}

/// Tokens that were valid before v0.2.0 but are now rejected. Kept as a
/// constant so the validator and the error message stay in sync.
const FORBIDDEN_TOKENS: &[&str] = &["{os}", "{arch}", "{ext}"];

/// Reject templates that reference platform-specific tokens. Plugins are
/// distributed as single platform-independent `.wasm` files in v0.2.0+.
pub fn check_asset_template(template: &str) -> Result<(), String> {
    for forbidden in FORBIDDEN_TOKENS {
        if template.contains(forbidden) {
            return Err(format!(
                "asset template token '{}' is no longer supported in v0.2.0; \
                 plugins are distributed as single platform-independent .wasm files. \
                 Update the plugin's release to ship `<name>.wasm` and remove \
                 the `asset = \"...\"` line (or set it to `\"{{name}}.wasm\"`).",
                forbidden
            ));
        }
    }
    Ok(())
}

/// Resolve an asset template by replacing `{name}`. Other historical
/// tokens (`{os}`, `{arch}`, `{ext}`) are no longer supported and the
/// caller should have rejected them via `check_asset_template`; for
/// safety we still treat them as plain text here.
pub fn resolve_template(template: &str, plugin_name: &str) -> String {
    template.replace("{name}", &normalize_name(plugin_name))
}

/// Get the resolved asset filename for a plugin, using custom or default
/// template. The custom template, if provided, must already have passed
/// `check_asset_template`.
pub fn asset_filename(plugin_name: &str, custom_template: Option<&str>) -> String {
    let template = custom_template.unwrap_or(DEFAULT_TEMPLATE);
    resolve_template(template, plugin_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_name_replaces_hyphens() {
        assert_eq!(normalize_name("git-status"), "git_status");
    }

    #[test]
    fn normalize_name_no_hyphens() {
        assert_eq!(normalize_name("simple"), "simple");
    }

    #[test]
    fn resolve_default_template() {
        let result = resolve_template(DEFAULT_TEMPLATE, "git-status");
        assert_eq!(result, "git_status.wasm");
    }

    #[test]
    fn resolve_custom_template() {
        let result = resolve_template("yosh_{name}.wasm", "auto-env");
        assert_eq!(result, "yosh_auto_env.wasm");
    }

    #[test]
    fn asset_filename_uses_default() {
        let result = asset_filename("my-plugin", None);
        assert_eq!(result, "my_plugin.wasm");
    }

    #[test]
    fn asset_filename_uses_custom() {
        let result = asset_filename("my-plugin", Some("custom_{name}.wasm"));
        assert_eq!(result, "custom_my_plugin.wasm");
    }

    #[test]
    fn check_template_accepts_default() {
        assert!(check_asset_template(DEFAULT_TEMPLATE).is_ok());
    }

    #[test]
    fn check_template_accepts_custom_wasm() {
        assert!(check_asset_template("plugin-{name}.wasm").is_ok());
    }

    #[test]
    fn check_template_rejects_os_token() {
        let err = check_asset_template("lib{name}-{os}-{arch}.{ext}").unwrap_err();
        assert!(err.contains("{os}"));
        assert!(err.contains("v0.2.0"));
    }

    #[test]
    fn check_template_rejects_arch_token() {
        let err = check_asset_template("plugin-{arch}.wasm").unwrap_err();
        assert!(err.contains("{arch}"));
    }

    #[test]
    fn check_template_rejects_ext_token() {
        let err = check_asset_template("plugin.{ext}").unwrap_err();
        assert!(err.contains("{ext}"));
    }
}
