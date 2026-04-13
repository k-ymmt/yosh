# kish Plugin System Design

## Overview

A plugin system for kish that allows extending the shell with custom builtin commands and shell hooks via dynamically loaded libraries (`.dylib`/`.so`). Plugins are written in Rust using a safe SDK, and loaded at startup based on a configuration file.

## Requirements

| Item | Decision |
|---|---|
| Scope | Custom builtin commands + shell hooks |
| Plugin format | Dynamic library (`.dylib`/`.so`), loaded at runtime via `dlopen` |
| ShellEnv access | Restricted write access through a dedicated API |
| Plugin management | Configuration file based (`~/.config/kish/plugins.toml`) |
| Hooks | `pre_exec`, `post_exec`, `on_cd` |
| Distribution | Pre-built binaries (future: install from GitHub) |
| API stability | Unstable initially, future migration to SemVer |

## Architecture: C ABI + Safe Rust SDK

The plugin system uses C ABI at the boundary for Rust compiler version independence, with a safe Rust SDK crate that hides all `unsafe` from plugin authors.

### Crate Structure

```
kish/                          # Shell binary (existing)
  └── Cargo.toml               # Adds libloading dependency

kish-plugin-api/               # C ABI type definitions (shared contract)
  └── src/lib.rs               # FFI types, constants, API version

kish-plugin-sdk/               # Safe Rust SDK for plugin authors
  └── src/lib.rs               # Plugin trait, export! macro, safe wrappers
```

- `kish-plugin-api` is the shared contract between kish and plugins. It defines C ABI structs and callback types.
- `kish-plugin-sdk` depends on `kish-plugin-api` and provides a `Plugin` trait and `export!` macro so plugin authors never write `unsafe`.
- `kish` depends on `kish-plugin-api` and `libloading` to load and manage plugins.

These are independent crates (not a Cargo workspace with kish).

## Plugin API (C ABI Layer)

Defined in `kish-plugin-api`:

```rust
/// API version for compatibility checks
pub const KISH_PLUGIN_API_VERSION: u32 = 1;

/// Plugin metadata returned to kish
#[repr(C)]
pub struct PluginDecl {
    pub api_version: u32,
    pub name: *const c_char,
    pub version: *const c_char,
}

/// API callbacks kish provides to plugins
#[repr(C)]
pub struct HostApi {
    pub ctx: *mut c_void,

    // Variable operations
    pub get_var: unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char) -> *const c_char,
    pub set_var: unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char, value: *const c_char) -> i32,
    pub export_var: unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char, value: *const c_char) -> i32,

    // Environment info
    pub get_cwd: unsafe extern "C" fn(ctx: *mut c_void) -> *const c_char,
    pub set_cwd: unsafe extern "C" fn(ctx: *mut c_void, path: *const c_char) -> i32,

    // Output
    pub write_stdout: unsafe extern "C" fn(ctx: *mut c_void, data: *const c_char, len: usize) -> i32,
    pub write_stderr: unsafe extern "C" fn(ctx: *mut c_void, data: *const c_char, len: usize) -> i32,
}
```

### Exported Functions

Plugins export these C ABI functions (kish resolves them via `dlsym`):

| Function | Required | Purpose |
|---|---|---|
| `kish_plugin_decl() -> *const PluginDecl` | Yes | Return plugin metadata and API version |
| `kish_plugin_init(api: *const HostApi) -> i32` | Yes | Initialize plugin, 0 = success |
| `kish_plugin_exec(api: *const HostApi, name: *const c_char, argc: i32, argv: *const *const c_char) -> i32` | Yes | Execute a plugin command |
| `kish_plugin_hook_pre_exec(api: *const HostApi, cmd: *const c_char)` | No | Hook: before command execution |
| `kish_plugin_hook_post_exec(api: *const HostApi, cmd: *const c_char, exit_code: i32)` | No | Hook: after command execution |
| `kish_plugin_hook_on_cd(api: *const HostApi, old_dir: *const c_char, new_dir: *const c_char)` | No | Hook: directory change |
| `kish_plugin_destroy()` | No | Cleanup on unload |

- Hook functions are optional: kish checks for their presence via `dlsym` and skips if absent.
- `kish_plugin_exec` receives a `name` argument to distinguish which command is being invoked (one plugin can provide multiple commands).
- String ownership stays with the caller; pointers are valid only during the call.
- `get_var` and `get_cwd` return pointers to internally managed buffers that are valid only until the next `HostApi` call. The SDK copies them into owned `String` values immediately.

## Plugin SDK (Safe Rust Wrapper)

Defined in `kish-plugin-sdk`:

```rust
/// Trait plugin authors implement
pub trait Plugin: Send {
    /// Command names this plugin provides
    fn commands(&self) -> &[&str];

    /// Called on plugin load (optional)
    fn on_load(&mut self, _api: &PluginApi) -> Result<(), String> { Ok(()) }

    /// Execute a command
    fn exec(&mut self, api: &PluginApi, command: &str, args: &[&str]) -> i32;

    /// Hook: before command execution (optional)
    fn hook_pre_exec(&mut self, _api: &PluginApi, _cmd: &str) {}

    /// Hook: after command execution (optional)
    fn hook_post_exec(&mut self, _api: &PluginApi, _cmd: &str, _exit_code: i32) {}

    /// Hook: directory change (optional)
    fn hook_on_cd(&mut self, _api: &PluginApi, _old_dir: &str, _new_dir: &str) {}

    /// Called on plugin unload (optional)
    fn on_unload(&mut self) {}
}

/// Safe wrapper around HostApi
pub struct PluginApi { /* wraps HostApi internally */ }

impl PluginApi {
    pub fn get_var(&self, name: &str) -> Option<String>;
    pub fn set_var(&self, name: &str, value: &str) -> Result<(), String>;
    pub fn export_var(&self, name: &str, value: &str) -> Result<(), String>;
    pub fn cwd(&self) -> String;
    pub fn set_cwd(&self, path: &str) -> Result<(), String>;
    pub fn print(&self, msg: &str);
    pub fn eprint(&self, msg: &str);
}

/// Macro to generate C ABI exports from a Plugin implementation
/// Usage: kish_plugin::export!(MyPlugin);
#[macro_export]
macro_rules! export {
    ($plugin_type:ty) => {
        // Generates: kish_plugin_decl, kish_plugin_init, kish_plugin_exec,
        // kish_plugin_hook_pre_exec, kish_plugin_hook_post_exec,
        // kish_plugin_hook_on_cd, kish_plugin_destroy
    };
}
```

### Plugin Author Example

```rust
use kish_plugin_sdk::{Plugin, PluginApi, export};

struct GitStatusPlugin;

impl Plugin for GitStatusPlugin {
    fn commands(&self) -> &[&str] { &["git-status"] }

    fn exec(&mut self, api: &PluginApi, _command: &str, args: &[&str]) -> i32 {
        api.print("on branch main\n");
        0
    }

    fn hook_on_cd(&mut self, api: &PluginApi, _old: &str, new_dir: &str) {
        // React to directory changes
    }
}

export!(GitStatusPlugin);
```

Plugin authors implement `Plugin`, call `export!`, and never write `unsafe`.

## Plugin Manager (kish Side)

New module: `src/plugin/mod.rs`

```rust
pub struct PluginManager {
    plugins: Vec<LoadedPlugin>,
}

struct LoadedPlugin {
    name: String,
    library: libloading::Library,
    commands: Vec<String>,
    has_pre_exec: bool,
    has_post_exec: bool,
    has_on_cd: bool,
}
```

### Key Operations

- `load_from_config(path)` — Parse TOML config, load each enabled plugin
- `load_plugin(path)` — `dlopen`, version check, init, register commands
- `find_command(name)` — Look up which plugin handles a command name
- `call_pre_exec(api, cmd)` — Invoke `pre_exec` on all plugins that have it
- `call_post_exec(api, cmd, exit_code)` — Invoke `post_exec` on all plugins
- `call_on_cd(api, old_dir, new_dir)` — Invoke `on_cd` on all plugins
- `unload_all()` — Call `destroy` on each plugin, drop libraries

### Command Dispatch Integration

Added as step 3 in `exec_simple_command()` (`src/exec/simple.rs`):

```
exec_simple_command(SimpleCommand)
├─ expand_words(words)
├─ 1. Check if function defined → exec_function_call
├─ 2. Classify builtin
│  ├─ Special builtin → exec_special_builtin
│  └─ Regular builtin → exec_regular_builtin
├─ 3. Check plugin commands → plugin_manager.exec_command()   ← NEW
└─ 4. External command → fork + execvp
```

### Hook Integration Points

| Hook | Call site | Timing |
|---|---|---|
| `pre_exec` | `exec_simple_command()` | After word expansion, before command execution |
| `post_exec` | `exec_simple_command()` | After command execution, exit code determined |
| `on_cd` | `builtin_cd()` | After successful directory change |

### ShellEnv Integration

```rust
pub struct ShellEnv {
    // ... existing fields
    pub plugins: PluginManager,    // NEW
}
```

## Configuration File

**Path:** `~/.config/kish/plugins.toml`

```toml
[[plugin]]
name = "git-status"
path = "~/.kish/plugins/libkish_git_status.dylib"
enabled = true

[[plugin]]
name = "my-tool"
path = "/usr/local/lib/kish/libkish_my_tool.so"
enabled = false
```

- TOML format, parsed with the `toml` crate
- `enabled` field allows disabling without removing the entry
- `path` supports tilde expansion
- Missing config file = no plugins loaded (not an error)
- Invalid paths or load errors produce stderr warnings and skip to the next plugin

**Load timing:** All plugins load once at shell startup (`ShellEnv::new()`). Dynamic load/unload at runtime is out of initial scope.

## Error Handling and Security

### Error Handling

| Scenario | Behavior |
|---|---|
| `.dylib` not found | stderr warning, skip, continue loading others |
| `kish_plugin_decl` missing | stderr error ("not a valid kish plugin"), skip |
| API version mismatch | stderr error (show expected vs actual), skip |
| `kish_plugin_init` returns non-zero | stderr error ("initialization failed"), skip |
| Panic during command execution | `std::panic::catch_unwind`, return exit code 1, stderr warning |
| Panic during hook execution | `catch_unwind`, stderr warning, continue |

### Security Considerations

- Plugins run with the same privileges as the kish process. No sandboxing in initial scope.
- Plugins access `ShellEnv` only through `HostApi` callbacks, not directly.
- Only plugins listed in the config file are loaded (no directory scanning).
- `readonly` variable protection is enforced: `HostApi` callbacks go through `VarStore` which rejects writes to readonly variables.
- `HostApi` callbacks use a `HostContext` with a borrow of `ShellEnv`; the SDK ensures plugins cannot retain the `ctx` pointer beyond the call lifetime.

## Testing Strategy

### Test Plugin

A test plugin crate at `tests/plugins/test_plugin/`:

```rust
struct TestPlugin;

impl Plugin for TestPlugin {
    fn commands(&self) -> &[&str] { &["test-hello"] }

    fn exec(&mut self, api: &PluginApi, _cmd: &str, _args: &[&str]) -> i32 {
        api.print("hello from plugin\n");
        0
    }

    fn hook_post_exec(&mut self, api: &PluginApi, cmd: &str, exit_code: i32) {
        let _ = api.set_var("KISH_LAST_PLUGIN_HOOK", &format!("post_exec:{cmd}:{exit_code}"));
    }
}

export!(TestPlugin);
```

### Test Levels

| Level | Target | Method |
|---|---|---|
| Unit tests | `PluginManager` load/search logic | Test config parsing, command lookup logic without actual `.dylib` |
| Integration tests | Actual plugin load and execution | Build test plugin, load via `PluginManager::load_plugin()`, execute commands, verify results |
| Hook tests | `pre_exec`, `post_exec`, `on_cd` | Test plugin sets variables as side effects; verify variables after hook invocation |
| Error tests | Invalid plugins, version mismatch | Provide empty `.dylib`, version-mismatched plugins; verify error handling |
| E2E tests | Plugin commands from shell scripts | Add plugin test cases in `e2e/` |

## Out of Scope (Initial Release)

- `pre_prompt` hook
- Runtime plugin load/unload
- Plugin installation from GitHub
- Plugin sandboxing
- Non-Rust plugin authoring (C/C++)
- SemVer-based API version management
- Cargo workspace integration with kish

These are natural future extensions enabled by the C ABI architecture.
