# Plugin System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a plugin system to kish that supports custom builtin commands and shell hooks via dynamically loaded libraries.

**Architecture:** C ABI at the plugin boundary for Rust compiler version independence. A safe Rust SDK crate hides all `unsafe` from plugin authors. kish loads plugins at startup from a TOML config file and integrates them into command dispatch and hook points.

**Tech Stack:** libloading (dlopen), toml + serde (config parsing), kish-plugin-api (FFI types), kish-plugin-sdk (Plugin trait + export! macro)

---

## File Structure

### New files

```
crates/
  kish-plugin-api/
    Cargo.toml                  # FFI type crate, no dependencies
    src/lib.rs                  # PluginDecl, HostApi, KISH_PLUGIN_API_VERSION
  kish-plugin-sdk/
    Cargo.toml                  # Depends on kish-plugin-api
    src/lib.rs                  # Plugin trait, PluginApi, export! macro
src/
  plugin/
    mod.rs                      # PluginManager, HostContext, host callbacks, loading
    config.rs                   # PluginConfig TOML parsing, tilde expansion
tests/
  plugins/
    test_plugin/
      Cargo.toml                # cdylib crate, depends on kish-plugin-sdk
      src/lib.rs                # TestPlugin for integration tests
  plugin.rs                     # Integration tests: load, exec, hooks, errors
```

### Modified files

```
Cargo.toml                      # Add libloading, toml, serde, kish-plugin-api
src/main.rs                     # Add mod plugin, call load_plugins in run_string
src/lib.rs                      # Add pub mod plugin
src/exec/mod.rs                 # Add plugins: PluginManager field to Executor
src/exec/simple.rs              # Plugin command dispatch + pre_exec/post_exec/on_cd hooks
src/interactive/mod.rs           # Call load_plugins in Repl::new
```

---

### Task 1: Create kish-plugin-api crate

**Files:**
- Create: `crates/kish-plugin-api/Cargo.toml`
- Create: `crates/kish-plugin-api/src/lib.rs`

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "kish-plugin-api"
version = "0.1.0"
edition = "2024"

[dependencies]
```

- [ ] **Step 2: Create src/lib.rs with FFI types**

```rust
use std::ffi::{c_char, c_void};

/// API version for compatibility checks between kish and plugins.
pub const KISH_PLUGIN_API_VERSION: u32 = 1;

/// Plugin metadata returned by kish_plugin_decl().
#[repr(C)]
pub struct PluginDecl {
    pub api_version: u32,
    pub name: *const c_char,
    pub version: *const c_char,
}

/// API callbacks kish provides to plugins.
///
/// `ctx` is an opaque pointer to kish internals. Plugins pass it back to each
/// callback but must not dereference or store it beyond the current call.
#[repr(C)]
pub struct HostApi {
    pub ctx: *mut c_void,

    // Variable operations
    pub get_var: unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char) -> *const c_char,
    pub set_var: unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char, value: *const c_char) -> i32,
    pub export_var: unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char, value: *const c_char) -> i32,

    // Environment
    pub get_cwd: unsafe extern "C" fn(ctx: *mut c_void) -> *const c_char,
    pub set_cwd: unsafe extern "C" fn(ctx: *mut c_void, path: *const c_char) -> i32,

    // Output
    pub write_stdout: unsafe extern "C" fn(ctx: *mut c_void, data: *const c_char, len: usize) -> i32,
    pub write_stderr: unsafe extern "C" fn(ctx: *mut c_void, data: *const c_char, len: usize) -> i32,
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build --manifest-path crates/kish-plugin-api/Cargo.toml`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add crates/kish-plugin-api/
git commit -m "feat(plugin): add kish-plugin-api crate with FFI types"
```

---

### Task 2: Create kish-plugin-sdk crate

**Files:**
- Create: `crates/kish-plugin-sdk/Cargo.toml`
- Create: `crates/kish-plugin-sdk/src/lib.rs`

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "kish-plugin-sdk"
version = "0.1.0"
edition = "2024"

[dependencies]
kish-plugin-api = { path = "../kish-plugin-api" }

[lib]
crate-type = ["rlib"]
```

- [ ] **Step 2: Create src/lib.rs with Plugin trait, PluginApi, and export! macro**

```rust
pub use kish_plugin_api as ffi;

use std::ffi::{CStr, CString, c_char, c_void};

/// Trait plugin authors implement. Requires `Default` for the export! macro.
pub trait Plugin: Send + Default {
    /// Command names this plugin provides.
    fn commands(&self) -> &[&str];

    /// Called when the plugin is loaded. Return Err to abort loading.
    fn on_load(&mut self, _api: &PluginApi) -> Result<(), String> {
        Ok(())
    }

    /// Execute a command. Returns exit status.
    fn exec(&mut self, api: &PluginApi, command: &str, args: &[&str]) -> i32;

    /// Hook: called before each command execution.
    fn hook_pre_exec(&mut self, _api: &PluginApi, _cmd: &str) {}

    /// Hook: called after each command execution.
    fn hook_post_exec(&mut self, _api: &PluginApi, _cmd: &str, _exit_code: i32) {}

    /// Hook: called when the working directory changes.
    fn hook_on_cd(&mut self, _api: &PluginApi, _old_dir: &str, _new_dir: &str) {}

    /// Called when the plugin is about to be unloaded.
    fn on_unload(&mut self) {}
}

/// Safe wrapper around the host API callbacks.
pub struct PluginApi {
    api: *const ffi::HostApi,
}

impl PluginApi {
    /// # Safety
    /// `api` must point to a valid `HostApi` that outlives this `PluginApi`.
    pub unsafe fn from_raw(api: *const ffi::HostApi) -> Self {
        PluginApi { api }
    }

    pub fn get_var(&self, name: &str) -> Option<String> {
        let c_name = CString::new(name).ok()?;
        unsafe {
            let api = &*self.api;
            let result = (api.get_var)(api.ctx, c_name.as_ptr());
            if result.is_null() {
                None
            } else {
                Some(CStr::from_ptr(result).to_string_lossy().into_owned())
            }
        }
    }

    pub fn set_var(&self, name: &str, value: &str) -> Result<(), String> {
        let c_name = CString::new(name).map_err(|e| e.to_string())?;
        let c_value = CString::new(value).map_err(|e| e.to_string())?;
        unsafe {
            let api = &*self.api;
            let rc = (api.set_var)(api.ctx, c_name.as_ptr(), c_value.as_ptr());
            if rc == 0 { Ok(()) } else { Err("set_var failed".into()) }
        }
    }

    pub fn export_var(&self, name: &str, value: &str) -> Result<(), String> {
        let c_name = CString::new(name).map_err(|e| e.to_string())?;
        let c_value = CString::new(value).map_err(|e| e.to_string())?;
        unsafe {
            let api = &*self.api;
            let rc = (api.export_var)(api.ctx, c_name.as_ptr(), c_value.as_ptr());
            if rc == 0 { Ok(()) } else { Err("export_var failed".into()) }
        }
    }

    pub fn cwd(&self) -> String {
        unsafe {
            let api = &*self.api;
            let result = (api.get_cwd)(api.ctx);
            if result.is_null() {
                String::new()
            } else {
                CStr::from_ptr(result).to_string_lossy().into_owned()
            }
        }
    }

    pub fn set_cwd(&self, path: &str) -> Result<(), String> {
        let c_path = CString::new(path).map_err(|e| e.to_string())?;
        unsafe {
            let api = &*self.api;
            let rc = (api.set_cwd)(api.ctx, c_path.as_ptr());
            if rc == 0 { Ok(()) } else { Err("set_cwd failed".into()) }
        }
    }

    pub fn print(&self, msg: &str) {
        unsafe {
            let api = &*self.api;
            (api.write_stdout)(api.ctx, msg.as_ptr() as *const c_char, msg.len());
        }
    }

    pub fn eprint(&self, msg: &str) {
        unsafe {
            let api = &*self.api;
            (api.write_stderr)(api.ctx, msg.as_ptr() as *const c_char, msg.len());
        }
    }
}

/// Generate all C ABI exports for a Plugin implementation.
///
/// Usage: `kish_plugin_sdk::export!(MyPlugin);`
///
/// The plugin type must implement `Plugin + Default`.
/// Plugin name and version are taken from Cargo.toml at compile time.
#[macro_export]
macro_rules! export {
    ($plugin_type:ty) => {
        use std::ffi::{CStr, CString, c_char, c_void};
        use std::sync::{Mutex, OnceLock};

        static PLUGIN_INSTANCE: Mutex<Option<$plugin_type>> = Mutex::new(None);
        static PLUGIN_NAME_CSTR: OnceLock<CString> = OnceLock::new();
        static PLUGIN_VERSION_CSTR: OnceLock<CString> = OnceLock::new();
        static PLUGIN_DECL_STATIC: OnceLock<$crate::ffi::PluginDecl> = OnceLock::new();
        static COMMAND_CSTRS: OnceLock<Vec<CString>> = OnceLock::new();
        static COMMAND_PTRS: OnceLock<Vec<*const c_char>> = OnceLock::new();

        // SAFETY: COMMAND_PTRS contains pointers into COMMAND_CSTRS which are
        // 'static and never mutated after init, so the raw pointers are safe
        // to share across threads.
        unsafe impl Sync for CommandPtrsWrapper {}
        struct CommandPtrsWrapper;

        #[no_mangle]
        pub extern "C" fn kish_plugin_decl() -> *const $crate::ffi::PluginDecl {
            PLUGIN_DECL_STATIC.get_or_init(|| {
                let name = PLUGIN_NAME_CSTR.get_or_init(|| {
                    CString::new(env!("CARGO_PKG_NAME")).unwrap()
                });
                let version = PLUGIN_VERSION_CSTR.get_or_init(|| {
                    CString::new(env!("CARGO_PKG_VERSION")).unwrap()
                });
                $crate::ffi::PluginDecl {
                    api_version: $crate::ffi::KISH_PLUGIN_API_VERSION,
                    name: name.as_ptr(),
                    version: version.as_ptr(),
                }
            })
        }

        #[no_mangle]
        pub extern "C" fn kish_plugin_init(api: *const $crate::ffi::HostApi) -> i32 {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let plugin_api = unsafe { $crate::PluginApi::from_raw(api) };
                let mut plugin = <$plugin_type as Default>::default();
                match $crate::Plugin::on_load(&mut plugin, &plugin_api) {
                    Ok(()) => {
                        *PLUGIN_INSTANCE.lock().unwrap() = Some(plugin);
                        0
                    }
                    Err(_) => 1,
                }
            })) {
                Ok(status) => status,
                Err(_) => 1,
            }
        }

        #[no_mangle]
        pub extern "C" fn kish_plugin_commands(count: *mut u32) -> *const *const c_char {
            let cstrs = COMMAND_CSTRS.get_or_init(|| {
                let plugin = PLUGIN_INSTANCE.lock().unwrap();
                let p = plugin.as_ref().expect("kish_plugin_commands called before init");
                $crate::Plugin::commands(p)
                    .iter()
                    .map(|s| CString::new(*s).unwrap())
                    .collect()
            });
            let ptrs = COMMAND_PTRS.get_or_init(|| {
                cstrs.iter().map(|s| s.as_ptr()).collect()
            });
            unsafe { *count = ptrs.len() as u32; }
            ptrs.as_ptr()
        }

        #[no_mangle]
        pub extern "C" fn kish_plugin_exec(
            api: *const $crate::ffi::HostApi,
            name: *const c_char,
            argc: i32,
            argv: *const *const c_char,
        ) -> i32 {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let plugin_api = unsafe { $crate::PluginApi::from_raw(api) };
                let name_str = unsafe { CStr::from_ptr(name) }.to_str().unwrap_or("");
                let args: Vec<&str> = (0..argc)
                    .map(|i| unsafe {
                        CStr::from_ptr(*argv.add(i as usize)).to_str().unwrap_or("")
                    })
                    .collect();
                let mut plugin = PLUGIN_INSTANCE.lock().unwrap();
                let p = plugin.as_mut().expect("plugin not initialized");
                $crate::Plugin::exec(p, &plugin_api, name_str, &args)
            })) {
                Ok(status) => status,
                Err(_) => 1,
            }
        }

        #[no_mangle]
        pub extern "C" fn kish_plugin_hook_pre_exec(
            api: *const $crate::ffi::HostApi,
            cmd: *const c_char,
        ) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let plugin_api = unsafe { $crate::PluginApi::from_raw(api) };
                let cmd_str = unsafe { CStr::from_ptr(cmd) }.to_str().unwrap_or("");
                let mut plugin = PLUGIN_INSTANCE.lock().unwrap();
                if let Some(p) = plugin.as_mut() {
                    $crate::Plugin::hook_pre_exec(p, &plugin_api, cmd_str);
                }
            }));
        }

        #[no_mangle]
        pub extern "C" fn kish_plugin_hook_post_exec(
            api: *const $crate::ffi::HostApi,
            cmd: *const c_char,
            exit_code: i32,
        ) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let plugin_api = unsafe { $crate::PluginApi::from_raw(api) };
                let cmd_str = unsafe { CStr::from_ptr(cmd) }.to_str().unwrap_or("");
                let mut plugin = PLUGIN_INSTANCE.lock().unwrap();
                if let Some(p) = plugin.as_mut() {
                    $crate::Plugin::hook_post_exec(p, &plugin_api, cmd_str, exit_code);
                }
            }));
        }

        #[no_mangle]
        pub extern "C" fn kish_plugin_hook_on_cd(
            api: *const $crate::ffi::HostApi,
            old_dir: *const c_char,
            new_dir: *const c_char,
        ) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let plugin_api = unsafe { $crate::PluginApi::from_raw(api) };
                let old = unsafe { CStr::from_ptr(old_dir) }.to_str().unwrap_or("");
                let new_d = unsafe { CStr::from_ptr(new_dir) }.to_str().unwrap_or("");
                let mut plugin = PLUGIN_INSTANCE.lock().unwrap();
                if let Some(p) = plugin.as_mut() {
                    $crate::Plugin::hook_on_cd(p, &plugin_api, old, new_d);
                }
            }));
        }

        #[no_mangle]
        pub extern "C" fn kish_plugin_destroy() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut plugin = PLUGIN_INSTANCE.lock().unwrap();
                if let Some(p) = plugin.as_mut() {
                    $crate::Plugin::on_unload(p);
                }
                *plugin = None;
            }));
        }
    };
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build --manifest-path crates/kish-plugin-sdk/Cargo.toml`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add crates/kish-plugin-sdk/
git commit -m "feat(plugin): add kish-plugin-sdk crate with Plugin trait and export! macro"
```

---

### Task 3: Create test plugin

**Files:**
- Create: `tests/plugins/test_plugin/Cargo.toml`
- Create: `tests/plugins/test_plugin/src/lib.rs`

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "test_plugin"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
kish-plugin-sdk = { path = "../../../crates/kish-plugin-sdk" }
```

- [ ] **Step 2: Create src/lib.rs**

```rust
use kish_plugin_sdk::{Plugin, PluginApi, export};

#[derive(Default)]
struct TestPlugin;

impl Plugin for TestPlugin {
    fn commands(&self) -> &[&str] {
        &["test-hello", "test-set-var"]
    }

    fn exec(&mut self, api: &PluginApi, command: &str, args: &[&str]) -> i32 {
        match command {
            "test-hello" => {
                api.print("hello from plugin\n");
                let _ = api.set_var("TEST_EXEC_CALLED", "1");
                0
            }
            "test-set-var" => {
                if args.len() >= 2 {
                    let _ = api.set_var(args[0], args[1]);
                    0
                } else {
                    api.eprint("usage: test-set-var NAME VALUE\n");
                    1
                }
            }
            _ => 127,
        }
    }

    fn hook_pre_exec(&mut self, api: &PluginApi, cmd: &str) {
        let _ = api.set_var("TEST_PRE_EXEC", cmd);
    }

    fn hook_post_exec(&mut self, api: &PluginApi, cmd: &str, exit_code: i32) {
        let _ = api.set_var("TEST_POST_EXEC", &format!("{cmd}:{exit_code}"));
    }

    fn hook_on_cd(&mut self, api: &PluginApi, old_dir: &str, new_dir: &str) {
        let _ = api.set_var("TEST_ON_CD", &format!("{old_dir}->{new_dir}"));
    }
}

export!(TestPlugin);
```

- [ ] **Step 3: Build and verify .dylib/.so is produced**

Run: `cargo build --manifest-path tests/plugins/test_plugin/Cargo.toml`
Expected: compiles and produces `tests/plugins/test_plugin/target/debug/libtest_plugin.dylib` (macOS) or `.so` (Linux)

- [ ] **Step 4: Commit**

```bash
git add tests/plugins/test_plugin/
git commit -m "feat(plugin): add test plugin for integration tests"
```

---

### Task 4: Add plugin config parsing to kish

**Files:**
- Modify: `Cargo.toml`
- Create: `src/plugin/config.rs`

- [ ] **Step 1: Write failing test for config parsing**

Create `src/plugin/config.rs`:

```rust
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct PluginConfig {
    #[serde(default)]
    pub plugin: Vec<PluginEntry>,
}

#[derive(Debug, Deserialize)]
pub struct PluginEntry {
    pub name: String,
    pub path: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl PluginConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("{}: {}", path.display(), e))?;
        toml::from_str(&content)
            .map_err(|e| format!("{}: {}", path.display(), e))
    }
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_valid_config() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "hello"
path = "/usr/lib/libhello.dylib"
enabled = true

[[plugin]]
name = "disabled"
path = "/usr/lib/libdisabled.dylib"
enabled = false
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        assert_eq!(config.plugin.len(), 2);
        assert_eq!(config.plugin[0].name, "hello");
        assert!(config.plugin[0].enabled);
        assert!(!config.plugin[1].enabled);
    }

    #[test]
    fn parse_empty_config() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "").unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        assert!(config.plugin.is_empty());
    }

    #[test]
    fn parse_missing_enabled_defaults_true() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "hello"
path = "/usr/lib/libhello.dylib"
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        assert!(config.plugin[0].enabled);
    }

    #[test]
    fn missing_config_file_returns_error() {
        let result = PluginConfig::load(Path::new("/nonexistent/plugins.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn expand_tilde_with_home() {
        std::env::set_var("HOME", "/Users/testuser");
        let result = expand_tilde("~/.kish/plugins/lib.dylib");
        assert_eq!(result, PathBuf::from("/Users/testuser/.kish/plugins/lib.dylib"));
    }

    #[test]
    fn expand_tilde_no_tilde() {
        let result = expand_tilde("/absolute/path/lib.dylib");
        assert_eq!(result, PathBuf::from("/absolute/path/lib.dylib"));
    }
}
```

- [ ] **Step 2: Add dependencies to Cargo.toml**

Add to `[dependencies]` in `Cargo.toml`:

```toml
libloading = "0.8"
toml = "0.8"
serde = { version = "1", features = ["derive"] }
kish-plugin-api = { path = "crates/kish-plugin-api" }
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test plugin::config`
Expected: all config tests pass

- [ ] **Step 4: Commit**

```bash
git add src/plugin/config.rs Cargo.toml Cargo.lock
git commit -m "feat(plugin): add plugin config parsing with TOML support"
```

---

### Task 5: Add PluginManager with host callbacks and loading

**Files:**
- Create: `src/plugin/mod.rs`
- Modify: `src/main.rs` (add `mod plugin`)
- Modify: `src/lib.rs` (add `pub mod plugin`)

- [ ] **Step 1: Create src/plugin/mod.rs**

```rust
pub mod config;

use std::ffi::{CStr, CString, c_char, c_void};
use std::io::Write;
use std::path::Path;

use kish_plugin_api::{HostApi, PluginDecl, KISH_PLUGIN_API_VERSION};

use crate::env::ShellEnv;

use self::config::{PluginConfig, expand_tilde};

/// A loaded plugin and its metadata.
struct LoadedPlugin {
    name: String,
    #[allow(dead_code)]
    library: libloading::Library,
    commands: Vec<String>,
    has_pre_exec: bool,
    has_post_exec: bool,
    has_on_cd: bool,
}

/// Manages loaded plugins and dispatches commands/hooks.
pub struct PluginManager {
    plugins: Vec<LoadedPlugin>,
}

impl PluginManager {
    pub fn new() -> Self {
        PluginManager { plugins: Vec::new() }
    }

    /// Load plugins listed in the config file. Errors are printed to stderr
    /// and the failing plugin is skipped.
    pub fn load_from_config(&mut self, config_path: &Path, env: &mut ShellEnv) {
        let config = match PluginConfig::load(config_path) {
            Ok(c) => c,
            Err(_) => return,
        };
        for entry in &config.plugin {
            if !entry.enabled {
                continue;
            }
            let path = expand_tilde(&entry.path);
            if let Err(e) = self.load_plugin(&path, env) {
                eprintln!("kish: plugin: {}", e);
            }
        }
    }

    /// Load a single plugin from a dynamic library path.
    pub fn load_plugin(&mut self, path: &Path, env: &mut ShellEnv) -> Result<(), String> {
        // 1. Load library
        let library = unsafe { libloading::Library::new(path) }
            .map_err(|e| format!("{}: {}", path.display(), e))?;

        // 2. Get and validate declaration
        let name = unsafe {
            let decl_fn: libloading::Symbol<extern "C" fn() -> *const PluginDecl> = library
                .get(b"kish_plugin_decl")
                .map_err(|_| {
                    format!("{}: not a valid kish plugin", path.display())
                })?;
            let decl = &*decl_fn();

            if decl.api_version != KISH_PLUGIN_API_VERSION {
                return Err(format!(
                    "{}: API version mismatch (expected {}, got {})",
                    path.display(),
                    KISH_PLUGIN_API_VERSION,
                    decl.api_version
                ));
            }

            CStr::from_ptr(decl.name).to_string_lossy().into_owned()
        };

        // 3. Initialize plugin
        {
            let mut ctx = HostContext::new(env);
            let api = ctx.build_api();

            let init_fn: libloading::Symbol<unsafe extern "C" fn(*const HostApi) -> i32> =
                unsafe {
                    library.get(b"kish_plugin_init").map_err(|_| {
                        format!("{}: missing kish_plugin_init", path.display())
                    })?
                };

            let status = unsafe { init_fn(&api) };
            if status != 0 {
                return Err(format!("{}: initialization failed", name));
            }
        }

        // 4. Get commands
        let commands: Vec<String> = unsafe {
            let cmd_fn: Result<
                libloading::Symbol<unsafe extern "C" fn(*mut u32) -> *const *const c_char>,
                _,
            > = library.get(b"kish_plugin_commands");

            match cmd_fn {
                Ok(cmd_fn) => {
                    let mut count: u32 = 0;
                    let ptr = cmd_fn(&mut count);
                    (0..count)
                        .map(|i| {
                            CStr::from_ptr(*ptr.add(i as usize))
                                .to_string_lossy()
                                .into_owned()
                        })
                        .collect()
                }
                Err(_) => Vec::new(),
            }
        };

        // 5. Check for optional hook functions
        let has_pre_exec =
            unsafe { library.get::<*const ()>(b"kish_plugin_hook_pre_exec").is_ok() };
        let has_post_exec =
            unsafe { library.get::<*const ()>(b"kish_plugin_hook_post_exec").is_ok() };
        let has_on_cd =
            unsafe { library.get::<*const ()>(b"kish_plugin_hook_on_cd").is_ok() };

        self.plugins.push(LoadedPlugin {
            name,
            library,
            commands,
            has_pre_exec,
            has_post_exec,
            has_on_cd,
        });

        Ok(())
    }

    /// Look up which plugin handles the given command name.
    /// Returns the exit status if a plugin handled it, or None.
    pub fn exec_command(
        &self,
        env: &mut ShellEnv,
        name: &str,
        args: &[String],
    ) -> Option<i32> {
        let plugin = self.plugins.iter().find(|p| p.commands.iter().any(|c| c == name))?;

        let mut ctx = HostContext::new(env);
        let api = ctx.build_api();

        let c_name = CString::new(name).ok()?;
        let c_args: Vec<CString> = args
            .iter()
            .filter_map(|a| CString::new(a.as_str()).ok())
            .collect();
        let c_arg_ptrs: Vec<*const c_char> =
            c_args.iter().map(|s| s.as_ptr()).collect();

        let status = unsafe {
            let exec_fn: libloading::Symbol<
                unsafe extern "C" fn(*const HostApi, *const c_char, i32, *const *const c_char) -> i32,
            > = plugin.library.get(b"kish_plugin_exec").ok()?;
            exec_fn(
                &api,
                c_name.as_ptr(),
                c_arg_ptrs.len() as i32,
                c_arg_ptrs.as_ptr(),
            )
        };

        Some(status)
    }

    /// Call pre_exec hook on all plugins that have it.
    pub fn call_pre_exec(&self, env: &mut ShellEnv, cmd: &str) {
        let c_cmd = match CString::new(cmd) {
            Ok(c) => c,
            Err(_) => return,
        };
        for plugin in &self.plugins {
            if !plugin.has_pre_exec {
                continue;
            }
            let mut ctx = HostContext::new(env);
            let api = ctx.build_api();
            unsafe {
                if let Ok(hook_fn) = plugin.library.get::<
                    unsafe extern "C" fn(*const HostApi, *const c_char),
                >(b"kish_plugin_hook_pre_exec")
                {
                    hook_fn(&api, c_cmd.as_ptr());
                }
            }
        }
    }

    /// Call post_exec hook on all plugins that have it.
    pub fn call_post_exec(&self, env: &mut ShellEnv, cmd: &str, exit_code: i32) {
        let c_cmd = match CString::new(cmd) {
            Ok(c) => c,
            Err(_) => return,
        };
        for plugin in &self.plugins {
            if !plugin.has_post_exec {
                continue;
            }
            let mut ctx = HostContext::new(env);
            let api = ctx.build_api();
            unsafe {
                if let Ok(hook_fn) = plugin.library.get::<
                    unsafe extern "C" fn(*const HostApi, *const c_char, i32),
                >(b"kish_plugin_hook_post_exec")
                {
                    hook_fn(&api, c_cmd.as_ptr(), exit_code);
                }
            }
        }
    }

    /// Call on_cd hook on all plugins that have it.
    pub fn call_on_cd(&self, env: &mut ShellEnv, old_dir: &str, new_dir: &str) {
        let c_old = match CString::new(old_dir) {
            Ok(c) => c,
            Err(_) => return,
        };
        let c_new = match CString::new(new_dir) {
            Ok(c) => c,
            Err(_) => return,
        };
        for plugin in &self.plugins {
            if !plugin.has_on_cd {
                continue;
            }
            let mut ctx = HostContext::new(env);
            let api = ctx.build_api();
            unsafe {
                if let Ok(hook_fn) = plugin.library.get::<
                    unsafe extern "C" fn(*const HostApi, *const c_char, *const c_char),
                >(b"kish_plugin_hook_on_cd")
                {
                    hook_fn(&api, c_old.as_ptr(), c_new.as_ptr());
                }
            }
        }
    }

    /// Call destroy on all plugins and drop them.
    pub fn unload_all(&mut self) {
        for plugin in &self.plugins {
            unsafe {
                if let Ok(destroy_fn) =
                    plugin.library.get::<unsafe extern "C" fn()>(b"kish_plugin_destroy")
                {
                    destroy_fn();
                }
            }
        }
        self.plugins.clear();
    }

    /// Check if any plugin provides the given command.
    pub fn has_command(&self, name: &str) -> bool {
        self.plugins.iter().any(|p| p.commands.iter().any(|c| c == name))
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PluginManager {
    fn drop(&mut self) {
        self.unload_all();
    }
}

// ── Host context and callbacks ─────────────────────────────────────────

/// Context passed to plugin callbacks via the opaque `ctx` pointer.
struct HostContext<'a> {
    env: &'a mut ShellEnv,
    /// Buffer for returning C strings from get_var/get_cwd.
    /// Valid until the next callback invocation.
    return_buf: CString,
}

impl<'a> HostContext<'a> {
    fn new(env: &'a mut ShellEnv) -> Self {
        HostContext {
            env,
            return_buf: CString::default(),
        }
    }

    fn build_api(&mut self) -> HostApi {
        HostApi {
            ctx: self as *mut HostContext as *mut c_void,
            get_var: host_get_var,
            set_var: host_set_var,
            export_var: host_export_var,
            get_cwd: host_get_cwd,
            set_cwd: host_set_cwd,
            write_stdout: host_write_stdout,
            write_stderr: host_write_stderr,
        }
    }
}

unsafe extern "C" fn host_get_var(ctx: *mut c_void, name: *const c_char) -> *const c_char {
    let host = &mut *(ctx as *mut HostContext);
    let name = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null(),
    };
    match host.env.vars.get(name) {
        Some(val) => {
            host.return_buf = CString::new(val).unwrap_or_default();
            host.return_buf.as_ptr()
        }
        None => std::ptr::null(),
    }
}

unsafe extern "C" fn host_set_var(
    ctx: *mut c_void,
    name: *const c_char,
    value: *const c_char,
) -> i32 {
    let host = &mut *(ctx as *mut HostContext);
    let name = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => return 1,
    };
    let value = match CStr::from_ptr(value).to_str() {
        Ok(s) => s,
        Err(_) => return 1,
    };
    match host.env.vars.set(name, value) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

unsafe extern "C" fn host_export_var(
    ctx: *mut c_void,
    name: *const c_char,
    value: *const c_char,
) -> i32 {
    let host = &mut *(ctx as *mut HostContext);
    let name = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => return 1,
    };
    let value = match CStr::from_ptr(value).to_str() {
        Ok(s) => s,
        Err(_) => return 1,
    };
    match host.env.vars.set(name, value) {
        Ok(()) => {
            host.env.vars.export(name);
            0
        }
        Err(_) => 1,
    }
}

unsafe extern "C" fn host_get_cwd(ctx: *mut c_void) -> *const c_char {
    let host = &mut *(ctx as *mut HostContext);
    match std::env::current_dir() {
        Ok(cwd) => {
            host.return_buf = CString::new(cwd.to_string_lossy().as_ref()).unwrap_or_default();
            host.return_buf.as_ptr()
        }
        Err(_) => std::ptr::null(),
    }
}

unsafe extern "C" fn host_set_cwd(ctx: *mut c_void, path: *const c_char) -> i32 {
    let _host = &mut *(ctx as *mut HostContext);
    let path = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return 1,
    };
    match std::env::set_current_dir(path) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

unsafe extern "C" fn host_write_stdout(
    _ctx: *mut c_void,
    data: *const c_char,
    len: usize,
) -> i32 {
    let slice = std::slice::from_raw_parts(data as *const u8, len);
    match std::io::stdout().write_all(slice) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

unsafe extern "C" fn host_write_stderr(
    _ctx: *mut c_void,
    data: *const c_char,
    len: usize,
) -> i32 {
    let slice = std::slice::from_raw_parts(data as *const u8, len);
    match std::io::stderr().write_all(slice) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}
```

- [ ] **Step 2: Add mod plugin to src/main.rs and src/lib.rs**

In `src/main.rs`, add after the existing mod declarations:

```rust
mod plugin;
```

In `src/lib.rs`, add:

```rust
pub mod plugin;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles with no errors (PluginManager is defined but not yet used)

- [ ] **Step 4: Commit**

```bash
git add src/plugin/ src/main.rs src/lib.rs
git commit -m "feat(plugin): add PluginManager with host callbacks and dynamic loading"
```

---

### Task 6: Integrate into Executor (command dispatch + hooks)

**Files:**
- Modify: `src/exec/mod.rs`
- Modify: `src/exec/simple.rs`
- Modify: `src/interactive/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add PluginManager field to Executor**

In `src/exec/mod.rs`, add import and modify the struct:

```rust
use crate::plugin::PluginManager;
```

Change `Executor`:

```rust
pub struct Executor {
    pub env: ShellEnv,
    pub plugins: PluginManager,
    errexit_suppressed_depth: usize,
}
```

Update `Executor::new`:

```rust
pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
    Executor {
        env: ShellEnv::new(shell_name, args),
        plugins: PluginManager::new(),
        errexit_suppressed_depth: 0,
    }
}
```

Update `Executor::from_env`:

```rust
pub fn from_env(env: ShellEnv) -> Self {
    Executor {
        env,
        plugins: PluginManager::new(),
        errexit_suppressed_depth: 0,
    }
}
```

Add a helper method to Executor:

```rust
/// Load plugins from the default config path (~/.config/kish/plugins.toml).
pub fn load_plugins(&mut self) {
    let config_path = dirs_config_path();
    self.plugins.load_from_config(&config_path, &mut self.env);
}
```

Add the helper function (outside the impl):

```rust
fn dirs_config_path() -> std::path::PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        std::path::PathBuf::from(home).join(".config/kish/plugins.toml")
    } else {
        std::path::PathBuf::from("/nonexistent")
    }
}
```

- [ ] **Step 2: Add plugin command dispatch + hooks to exec_simple_command**

In `src/exec/simple.rs`, modify `exec_simple_command`. After the line that builds `command_name` and `args` (around line 71), add the pre_exec hook call. Then add plugin command dispatch in the `BuiltinKind::NotBuiltin` arm, before the external command. Add post_exec after command execution and on_cd after successful cd.

The full method becomes (showing only the changed sections):

After line 72 (`let args: Vec<String> = expanded_iter.collect();`), before the function check, insert:

```rust
        // Pre-exec hook
        let cmd_str_for_hooks = std::iter::once(command_name.as_str())
            .chain(args.iter().map(|s| s.as_str()))
            .collect::<Vec<_>>()
            .join(" ");
        self.plugins.call_pre_exec(&mut self.env, &cmd_str_for_hooks);
```

Replace the `BuiltinKind::NotBuiltin` arm (currently lines 218-233) with:

```rust
            BuiltinKind::NotBuiltin => {
                // Check plugin commands before external
                if let Some(status) = self.plugins.exec_command(&mut self.env, &command_name, &args) {
                    let status = status;
                    self.plugins.call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
                    self.env.exec.last_exit_status = status;
                    return status;
                }

                let env_vars = match self.build_env_vars(&cmd.assignments) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        self.env.exec.last_exit_status = 1;
                        return 1;
                    }
                };
                let status = self.exec_external_with_redirects(
                    &command_name, &args, &env_vars, &cmd.redirects,
                );
                self.plugins.call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
                self.env.exec.last_exit_status = status;
                status
            }
```

For the function call path (around line 91, after `let status = self.exec_function_call(&func_def, &args);`), add post_exec before return:

```rust
            let status = self.exec_function_call(&func_def, &args);
            redirect_state.restore();
            self.restore_assignments(saved);
            self.plugins.call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
            self.env.exec.last_exit_status = status;
            return status;
```

For the Special builtin path (end of the `BuiltinKind::Special` arm, before returning status), add:

```rust
                let status = exec_special_builtin(&command_name, &args, self);
                redirect_state.restore();
                self.plugins.call_post_exec(&mut self.env, &cmd_str_for_hooks, status);
                self.env.exec.last_exit_status = status;
                status
```

For the Regular builtin path, add post_exec and on_cd:

```rust
            BuiltinKind::Regular => {
                let saved = match self.apply_temp_assignments(&cmd.assignments) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("{}", e);
                        self.env.exec.last_exit_status = 1;
                        return 1;
                    }
                };
                let mut redirect_state = RedirectState::new();
                if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                    eprintln!("kish: {}", e);
                    self.restore_assignments(saved);
                    self.env.exec.last_exit_status = 1;
                    return 1;
                }

                // Save old PWD for on_cd hook detection
                let old_pwd = if command_name == "cd" {
                    self.env.vars.get("PWD").map(|s| s.to_string())
                } else {
                    None
                };

                let status = exec_regular_builtin(&command_name, &args, &mut self.env);
                redirect_state.restore();
                self.restore_assignments(saved);
                self.plugins.call_post_exec(&mut self.env, &cmd_str_for_hooks, status);

                // on_cd hook: fire if cd succeeded
                if command_name == "cd" && status == 0 {
                    let old = old_pwd.unwrap_or_default();
                    let new = self.env.vars.get("PWD").unwrap_or("").to_string();
                    self.plugins.call_on_cd(&mut self.env, &old, &new);
                }

                self.env.exec.last_exit_status = status;
                status
            }
```

Similarly update the `wait`, `fg`/`bg`/`jobs` paths to add post_exec hook calls.

- [ ] **Step 3: Load plugins in main.rs**

In `src/main.rs`, in `run_string` function, after `let mut executor = Executor::new(shell_name, positional);` (line 79), add:

```rust
    executor.load_plugins();
```

- [ ] **Step 4: Load plugins in Repl::new**

In `src/interactive/mod.rs`, in `Repl::new`, after the history loading (line 52 `executor.env.history.load(...)`), add:

```rust
        executor.load_plugins();
```

- [ ] **Step 5: Verify it compiles and existing tests pass**

Run: `cargo build && cargo test`
Expected: compiles and all existing tests pass (no plugins loaded during tests = no behavior change)

- [ ] **Step 6: Commit**

```bash
git add src/exec/mod.rs src/exec/simple.rs src/main.rs src/interactive/mod.rs
git commit -m "feat(plugin): integrate PluginManager into Executor with command dispatch and hooks"
```

---

### Task 7: Integration tests

**Files:**
- Create: `tests/plugin.rs`

- [ ] **Step 1: Create tests/plugin.rs with build helper and basic load test**

```rust
use std::path::{Path, PathBuf};
use std::process::Command;

use kish::env::ShellEnv;
use kish::plugin::PluginManager;

fn build_test_plugin() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/plugins/test_plugin/Cargo.toml");
    let status = Command::new("cargo")
        .args(["build", "--manifest-path", manifest.to_str().unwrap()])
        .status()
        .expect("failed to run cargo build for test plugin");
    assert!(status.success(), "test plugin build failed");

    let target_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/plugins/test_plugin/target/debug");
    if cfg!(target_os = "macos") {
        target_dir.join("libtest_plugin.dylib")
    } else {
        target_dir.join("libtest_plugin.so")
    }
}

#[test]
fn load_plugin_successfully() {
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    manager.load_plugin(&dylib, &mut env).unwrap();
    assert!(manager.has_command("test-hello"));
    assert!(manager.has_command("test-set-var"));
    assert!(!manager.has_command("nonexistent"));
}

#[test]
fn exec_plugin_command() {
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    manager.load_plugin(&dylib, &mut env).unwrap();

    let status = manager.exec_command(&mut env, "test-hello", &[]);
    assert_eq!(status, Some(0));
    assert_eq!(env.vars.get("TEST_EXEC_CALLED"), Some("1"));
}

#[test]
fn exec_plugin_command_with_args() {
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    manager.load_plugin(&dylib, &mut env).unwrap();

    let status = manager.exec_command(
        &mut env,
        "test-set-var",
        &["MY_VAR".to_string(), "my_value".to_string()],
    );
    assert_eq!(status, Some(0));
    assert_eq!(env.vars.get("MY_VAR"), Some("my_value"));
}

#[test]
fn exec_unknown_command_returns_none() {
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    manager.load_plugin(&dylib, &mut env).unwrap();

    let status = manager.exec_command(&mut env, "nonexistent", &[]);
    assert_eq!(status, None);
}

#[test]
fn hook_pre_exec() {
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    manager.load_plugin(&dylib, &mut env).unwrap();

    manager.call_pre_exec(&mut env, "echo hello");
    assert_eq!(env.vars.get("TEST_PRE_EXEC"), Some("echo hello"));
}

#[test]
fn hook_post_exec() {
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    manager.load_plugin(&dylib, &mut env).unwrap();

    manager.call_post_exec(&mut env, "ls -la", 0);
    assert_eq!(env.vars.get("TEST_POST_EXEC"), Some("ls -la:0"));
}

#[test]
fn hook_on_cd() {
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    manager.load_plugin(&dylib, &mut env).unwrap();

    manager.call_on_cd(&mut env, "/old/dir", "/new/dir");
    assert_eq!(env.vars.get("TEST_ON_CD"), Some("/old/dir->/new/dir"));
}

#[test]
fn load_nonexistent_plugin_fails() {
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    let result = manager.load_plugin(Path::new("/nonexistent/libfoo.dylib"), &mut env);
    assert!(result.is_err());
}

#[test]
fn readonly_var_rejected_by_plugin() {
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    manager.load_plugin(&dylib, &mut env).unwrap();

    // Set a readonly variable
    let _ = env.vars.set("RO_VAR", "immutable");
    env.vars.set_readonly("RO_VAR");

    // Plugin tries to overwrite — should fail (returns non-zero exit)
    let status = manager.exec_command(
        &mut env,
        "test-set-var",
        &["RO_VAR".to_string(), "changed".to_string()],
    );
    // The set_var callback returns 1 for readonly, but the plugin returns 0
    // because it ignores the Result. The variable should be unchanged.
    assert_eq!(env.vars.get("RO_VAR"), Some("immutable"));
    assert!(status.is_some());
}
```

- [ ] **Step 2: Run integration tests**

Run: `cargo test --test plugin`
Expected: all tests pass

- [ ] **Step 3: Run the full test suite**

Run: `cargo test`
Expected: all tests pass (existing + new)

- [ ] **Step 4: Commit**

```bash
git add tests/plugin.rs
git commit -m "test(plugin): add integration tests for plugin loading, execution, and hooks"
```
