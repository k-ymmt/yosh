# Plugin Sandbox Design

## Overview

Add a capability-based sandbox mechanism to the kish plugin system. Plugins declare their required capabilities, and the host grants or restricts them based on user configuration. This is the first shell to implement plugin-level capability control — existing shells (zsh, bash, fish) have no sandboxing for plugins.

## Design Decisions

- **Scope**: HostAPI-level capability control only (no OS-level sandboxing). The architecture supports future layering of OS restrictions using the same capability declarations.
- **Approach**: API table substitution — each plugin receives a `HostApi` with only the permitted function pointers. Denied functions are replaced with deny stubs that log and return errors. Zero runtime overhead for permitted calls; no check-forgetting bugs possible.
- **Denial behavior**: Return error value + log to stderr with plugin name and missing capability.
- **Declaration model**: Plugin self-declares required capabilities in metadata; user config can restrict further. Effective capabilities = intersection of declared and configured.
- **Hook control**: Hooks (`pre_exec`, `post_exec`, `on_cd`) are capabilities. Hooks without the corresponding capability are not fired.

## Capability Groups

| Capability | Bit | Controls |
|---|---|---|
| `variables:read` | `0x01` | `get_var` |
| `variables:write` | `0x02` | `set_var`, `export_var` |
| `filesystem` | `0x04` | `cwd`, `set_cwd` |
| `io` | `0x08` | `print`, `eprint` |
| `hooks:pre_exec` | `0x10` | `pre_exec` hook firing |
| `hooks:post_exec` | `0x20` | `post_exec` hook firing |
| `hooks:on_cd` | `0x40` | `on_cd` hook firing |

## Plugin-Side Declaration

### FFI Layer (kish-plugin-api)

Add `required_capabilities` field to `PluginDecl`:

```rust
#[repr(C)]
pub struct PluginDecl {
    pub api_version: u32,
    pub name: *const c_char,
    pub version: *const c_char,
    pub required_capabilities: u32,  // bitflags
}
```

Bump `KISH_PLUGIN_API_VERSION` from 1 to 2.

Define capability constants:

```rust
pub const CAP_VARIABLES_READ:  u32 = 0x01;
pub const CAP_VARIABLES_WRITE: u32 = 0x02;
pub const CAP_FILESYSTEM:      u32 = 0x04;
pub const CAP_IO:              u32 = 0x08;
pub const CAP_HOOK_PRE_EXEC:   u32 = 0x10;
pub const CAP_HOOK_POST_EXEC:  u32 = 0x20;
pub const CAP_HOOK_ON_CD:      u32 = 0x40;
```

### SDK Layer (kish-plugin-sdk)

Add `Capability` enum and `required_capabilities()` to the `Plugin` trait:

```rust
pub enum Capability {
    VariablesRead,
    VariablesWrite,
    Filesystem,
    Io,
    HookPreExec,
    HookPostExec,
    HookOnCd,
}

pub trait Plugin: Send + Default {
    fn required_capabilities(&self) -> &[Capability] {
        &[]  // default: request nothing
    }
    // ... existing methods unchanged
}
```

The `export!` macro converts `required_capabilities()` to a `u32` bitflag and embeds it in `PluginDecl`.

## User Configuration

### plugins.toml Format

```toml
[[plugin]]
name = "hello"
path = "~/.kish/plugins/libhello.dylib"
enabled = true
# capabilities omitted: grant all requested capabilities (trust mode)

[[plugin]]
name = "analytics"
path = "~/.kish/plugins/libanalytics.dylib"
enabled = true
capabilities = ["variables:read", "io", "hooks:pre_exec", "hooks:post_exec"]
# explicit: only these are granted

[[plugin]]
name = "untrusted"
path = "~/.kish/plugins/libuntrusted.dylib"
enabled = true
capabilities = ["io"]
# minimal: output only
```

### Resolution Rules

1. `capabilities` field omitted → all `required_capabilities` from plugin are granted.
2. `capabilities` field present → effective = intersection(plugin declared, config granted).
3. Config cannot grant capabilities the plugin did not declare (no capability escalation).
4. On load, if plugin's declared capabilities are restricted by config, log warning:
   ```
   kish: plugin 'analytics': capability 'variables:write' requested but not granted
   ```

## API Table Substitution

### HostApi Construction

Per-plugin `HostApi` is built at load time based on effective capabilities:

```rust
fn build_host_api(capabilities: u32) -> HostApi {
    HostApi {
        ctx: std::ptr::null_mut(),
        get_var:      if cap(capabilities, CAP_VARIABLES_READ)  { host_get_var }      else { deny_get_var },
        set_var:      if cap(capabilities, CAP_VARIABLES_WRITE) { host_set_var }      else { deny_set_var },
        export_var:   if cap(capabilities, CAP_VARIABLES_WRITE) { host_export_var }   else { deny_export_var },
        get_cwd:      if cap(capabilities, CAP_FILESYSTEM)      { host_get_cwd }      else { deny_get_cwd },
        set_cwd:      if cap(capabilities, CAP_FILESYSTEM)      { host_set_cwd }      else { deny_set_cwd },
        write_stdout: if cap(capabilities, CAP_IO)              { host_write_stdout } else { deny_write_stdout },
        write_stderr: if cap(capabilities, CAP_IO)              { host_write_stderr } else { deny_write_stderr },
    }
}
```

### Deny Functions

Each deny function logs to stderr and returns an error:

```rust
unsafe extern "C" fn deny_set_var(
    ctx: *mut c_void, name: *const c_char, _value: *const c_char
) -> i32 {
    let ctx = &mut *(ctx as *mut HostContext);
    eprintln!("kish: plugin '{}': set_var denied (missing 'variables:write' capability)",
              ctx.plugin_name);
    -1
}
```

### Hook Filtering

`PluginManager::call_pre_exec()` etc. check the capability flag before calling the hook. If the plugin lacks the hook capability, the call is skipped entirely (no deny function needed since the hook is never invoked):

```rust
pub fn call_pre_exec(&self, env: &mut ShellEnv, cmd: &str) {
    for plugin in &self.plugins {
        if plugin.has_pre_exec && cap(plugin.capabilities, CAP_HOOK_PRE_EXEC) {
            // invoke hook
        }
    }
}
```

## Changes to Existing Components

### kish-plugin-api
- Add `required_capabilities: u32` to `PluginDecl`
- Add `CAP_*` constants
- Bump `KISH_PLUGIN_API_VERSION` to 2

### kish-plugin-sdk
- Add `Capability` enum
- Add `required_capabilities()` to `Plugin` trait with default empty impl
- Update `export!` macro to embed capabilities in `PluginDecl`

### src/plugin/mod.rs (PluginManager)
- Add `capabilities: u32` and `host_api: HostApi` to `LoadedPlugin`
- Add `plugin_name: String` to `HostContext`
- Add `build_host_api()` function
- Add deny function implementations (one per API function)
- Update `load_plugin()` to negotiate capabilities and build per-plugin `HostApi`
- Update hook call methods to check capability flags

### src/plugin/config.rs
- Add `capabilities: Option<Vec<String>>` to `PluginEntry`
- Add string-to-bitflag parsing function

### tests/plugins/test_plugin
- Add `required_capabilities()` implementation

### No changes to src/exec/simple.rs
- Sandbox logic is fully contained within `PluginManager`

## Testing Strategy

- Unit tests for capability bitflag operations and string parsing
- Unit tests for `build_host_api()` with various capability combinations
- Integration tests: plugin with full capabilities executes normally
- Integration tests: plugin with restricted capabilities gets deny responses
- Integration tests: hook capability filtering (hook not fired when capability missing)
- Integration tests: config-based restriction overrides plugin declaration
- Integration tests: load-time warning messages for restricted capabilities
