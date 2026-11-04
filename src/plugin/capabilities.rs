//! Capability negotiation at plugin load: parse the capabilities a
//! plugin requests and intersect them with the `plugins.toml` allowlist.

use yosh_plugin_api::{Capability, parse_capability};

use super::generated::yosh::plugin::types::PluginInfo;

/// Negotiate the granted capability bitfield. Parses the strings from
/// `plugin-info`, intersects with the config allowlist (`None` grants
/// every requested capability), and logs denied bits.
pub(super) fn negotiate_capabilities(
    plugin_info: &PluginInfo,
    config_capabilities: Option<u32>,
) -> u32 {
    let requested_capabilities = parse_required_capabilities(plugin_info, &plugin_info.name);
    match config_capabilities {
        None => requested_capabilities,
        Some(allow) => {
            let effective = requested_capabilities & allow;
            let denied = requested_capabilities & !effective;
            if denied != 0 {
                log_denied_capabilities(&plugin_info.name, denied);
            }
            effective
        }
    }
}

/// Parse `plugin-info.required-capabilities` into a bitfield. Unknown
/// strings produce a single warning line each but do NOT block the plugin
/// (matches the §6 "unknown capabilities are warnings, not errors" rule).
fn parse_required_capabilities(plugin_info: &PluginInfo, plugin_name: &str) -> u32 {
    let mut bits: u32 = 0;
    for s in &plugin_info.required_capabilities {
        match parse_capability(s) {
            Some(cap) => bits |= cap.to_bitflag(),
            None => {
                eprintln!(
                    "yosh: plugin '{}': unknown capability string '{}' (ignored)",
                    plugin_name, s
                );
            }
        }
    }
    bits
}

/// Names of the capabilities present in the `denied` bitfield. Must
/// enumerate every `Capability` variant so no denial goes unreported.
fn denied_capability_names(denied: u32) -> Vec<&'static str> {
    let caps = [
        Capability::VariablesRead,
        Capability::VariablesWrite,
        Capability::Filesystem,
        Capability::Io,
        Capability::HookPreExec,
        Capability::HookPostExec,
        Capability::HookOnCd,
        Capability::HookPrePrompt,
        Capability::FilesRead,
        Capability::FilesWrite,
        Capability::CommandsExec,
    ];
    caps.into_iter()
        .filter(|cap| denied & cap.to_bitflag() != 0)
        .map(|cap| cap.as_str())
        .collect()
}

/// Log requested-but-not-granted capabilities in the same shape as the
/// dlopen-era `log_denied_capabilities` — preserves user-visible behaviour.
fn log_denied_capabilities(plugin_name: &str, denied: u32) {
    for name in denied_capability_names(denied) {
        eprintln!(
            "yosh: plugin '{}': capability '{}' requested but not granted",
            plugin_name, name
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: `commands:exec` was missing from the denial
    /// enumeration, so a denied exec capability was the only one that
    /// produced no "requested but not granted" warning. Every
    /// capability bit must map to a name here.
    #[test]
    fn denied_capability_names_covers_every_capability() {
        use yosh_plugin_api::*;
        let all = [
            (CAP_VARIABLES_READ, "variables:read"),
            (CAP_VARIABLES_WRITE, "variables:write"),
            (CAP_FILESYSTEM, "filesystem"),
            (CAP_IO, "io"),
            (CAP_HOOK_PRE_EXEC, "hooks:pre_exec"),
            (CAP_HOOK_POST_EXEC, "hooks:post_exec"),
            (CAP_HOOK_ON_CD, "hooks:on_cd"),
            (CAP_HOOK_PRE_PROMPT, "hooks:pre_prompt"),
            (CAP_FILES_READ, "files:read"),
            (CAP_FILES_WRITE, "files:write"),
            (CAP_COMMANDS_EXEC, "commands:exec"),
        ];
        for (bit, name) in all {
            assert_eq!(
                denied_capability_names(bit),
                vec![name],
                "capability {:#x} must be reported as '{}'",
                bit,
                name
            );
        }
    }
}
