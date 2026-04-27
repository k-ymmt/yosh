//! Capability declarations and string parsing shared between the host,
//! the SDK, and the plugin manager. The C ABI types from the dlopen era
//! are removed; the public WIT contract lives at `wit/yosh-plugin.wit`.

/// Capability bitflag constants. Used by the host's linker construction
/// (`src/plugin/linker.rs`) to decide which host imports get the real
/// implementation vs a deny-stub. Also used by the manager to parse
/// `plugins.toml` `capabilities = [...]` allowlists.
pub const CAP_VARIABLES_READ:  u32 = 0x01;
pub const CAP_VARIABLES_WRITE: u32 = 0x02;
pub const CAP_FILESYSTEM:      u32 = 0x04;
pub const CAP_IO:              u32 = 0x08;
pub const CAP_HOOK_PRE_EXEC:   u32 = 0x10;
pub const CAP_HOOK_POST_EXEC:  u32 = 0x20;
pub const CAP_HOOK_ON_CD:      u32 = 0x40;
pub const CAP_HOOK_PRE_PROMPT: u32 = 0x80;

pub const CAP_ALL: u32 = CAP_VARIABLES_READ
    | CAP_VARIABLES_WRITE
    | CAP_FILESYSTEM
    | CAP_IO
    | CAP_HOOK_PRE_EXEC
    | CAP_HOOK_POST_EXEC
    | CAP_HOOK_ON_CD
    | CAP_HOOK_PRE_PROMPT;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    VariablesRead,
    VariablesWrite,
    Filesystem,
    Io,
    HookPreExec,
    HookPostExec,
    HookOnCd,
    HookPrePrompt,
}

impl Capability {
    pub fn to_bitflag(self) -> u32 {
        match self {
            Capability::VariablesRead  => CAP_VARIABLES_READ,
            Capability::VariablesWrite => CAP_VARIABLES_WRITE,
            Capability::Filesystem     => CAP_FILESYSTEM,
            Capability::Io             => CAP_IO,
            Capability::HookPreExec    => CAP_HOOK_PRE_EXEC,
            Capability::HookPostExec   => CAP_HOOK_POST_EXEC,
            Capability::HookOnCd       => CAP_HOOK_ON_CD,
            Capability::HookPrePrompt  => CAP_HOOK_PRE_PROMPT,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Capability::VariablesRead  => "variables:read",
            Capability::VariablesWrite => "variables:write",
            Capability::Filesystem     => "filesystem",
            Capability::Io             => "io",
            Capability::HookPreExec    => "hooks:pre_exec",
            Capability::HookPostExec   => "hooks:post_exec",
            Capability::HookOnCd       => "hooks:on_cd",
            Capability::HookPrePrompt  => "hooks:pre_prompt",
        }
    }
}

/// Parse a single capability string. Returns `None` for unknown strings;
/// callers decide whether to log a warning or fail.
pub fn parse_capability(s: &str) -> Option<Capability> {
    Some(match s {
        "variables:read"   => Capability::VariablesRead,
        "variables:write"  => Capability::VariablesWrite,
        "filesystem"       => Capability::Filesystem,
        "io"               => Capability::Io,
        "hooks:pre_exec"   => Capability::HookPreExec,
        "hooks:post_exec"  => Capability::HookPostExec,
        "hooks:on_cd"      => Capability::HookOnCd,
        "hooks:pre_prompt" => Capability::HookPrePrompt,
        _ => return None,
    })
}

/// Combine a slice of capabilities into a bitfield.
pub fn capabilities_to_bitflags(caps: &[Capability]) -> u32 {
    caps.iter().fold(0u32, |acc, c| acc | c.to_bitflag())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_known_strings() {
        assert_eq!(parse_capability("io"), Some(Capability::Io));
        assert_eq!(
            parse_capability("hooks:pre_prompt"),
            Some(Capability::HookPrePrompt)
        );
    }

    #[test]
    fn parse_unknown_returns_none() {
        assert_eq!(parse_capability("variables:execute"), None);
        assert_eq!(parse_capability(""), None);
    }

    #[test]
    fn capability_round_trip() {
        for cap in [
            Capability::VariablesRead,
            Capability::VariablesWrite,
            Capability::Filesystem,
            Capability::Io,
            Capability::HookPreExec,
            Capability::HookPostExec,
            Capability::HookOnCd,
            Capability::HookPrePrompt,
        ] {
            assert_eq!(parse_capability(cap.as_str()), Some(cap));
        }
    }

    #[test]
    fn cap_all_covers_every_variant() {
        let bits = capabilities_to_bitflags(&[
            Capability::VariablesRead,
            Capability::VariablesWrite,
            Capability::Filesystem,
            Capability::Io,
            Capability::HookPreExec,
            Capability::HookPostExec,
            Capability::HookOnCd,
            Capability::HookPrePrompt,
        ]);
        assert_eq!(bits, CAP_ALL);
    }
}
