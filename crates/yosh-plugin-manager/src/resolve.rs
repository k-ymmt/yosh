pub const DEFAULT_TEMPLATE: &str = "lib{name}-{os}-{arch}.{ext}";

pub fn current_os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

pub fn current_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "unknown"
    }
}

pub fn lib_ext() -> &'static str {
    if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

/// Convert plugin name to a form suitable for library names: hyphens become underscores.
pub fn normalize_name(name: &str) -> String {
    name.replace('-', "_")
}

/// Resolve an asset template by replacing `{name}`, `{os}`, `{arch}`, `{ext}`.
pub fn resolve_template(template: &str, plugin_name: &str) -> String {
    template
        .replace("{name}", &normalize_name(plugin_name))
        .replace("{os}", current_os())
        .replace("{arch}", current_arch())
        .replace("{ext}", lib_ext())
}

/// Get the resolved asset filename for a plugin, using custom or default template.
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
        let expected = format!("libgit_status-{}-{}.{}", current_os(), current_arch(), lib_ext());
        assert_eq!(result, expected);
    }

    #[test]
    fn resolve_custom_template() {
        let result = resolve_template("yosh_{name}-{os}-{arch}.{ext}", "auto-env");
        let expected = format!("yosh_auto_env-{}-{}.{}", current_os(), current_arch(), lib_ext());
        assert_eq!(result, expected);
    }

    #[test]
    fn asset_filename_uses_default() {
        let result = asset_filename("my-plugin", None);
        assert!(result.starts_with("libmy_plugin-"));
    }

    #[test]
    fn asset_filename_uses_custom() {
        let result = asset_filename("my-plugin", Some("custom_{name}.{ext}"));
        assert!(result.starts_with("custom_my_plugin."));
    }

    #[test]
    fn current_os_is_known() {
        assert!(["macos", "linux"].contains(&current_os()));
    }

    #[test]
    fn current_arch_is_known() {
        assert!(["x86_64", "aarch64"].contains(&current_arch()));
    }
}
