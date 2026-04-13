# Plugin Sandbox Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add capability-based sandboxing to the kish plugin system so each plugin only gets access to the HostAPI functions it needs, with denied calls logged to stderr.

**Architecture:** API table substitution — each plugin receives a per-plugin `HostApi` struct where denied functions are replaced with deny stubs at load time. Capabilities are declared by plugins in `PluginDecl` metadata and optionally restricted by user config in `plugins.toml`. Hooks are also capability-gated.

**Tech Stack:** Rust, C FFI (`kish-plugin-api`), `kish-plugin-sdk` macros, `serde`/`toml` for config parsing.

---

## File Structure

| Action | Path | Responsibility |
|--------|------|----------------|
| Modify | `crates/kish-plugin-api/src/lib.rs` | Add `required_capabilities` to `PluginDecl`, add `CAP_*` constants, bump API version |
| Modify | `crates/kish-plugin-sdk/src/lib.rs` | Add `Capability` enum, add `required_capabilities()` to `Plugin` trait, update `export!` macro |
| Modify | `src/plugin/config.rs` | Add `capabilities` field to `PluginEntry`, add string-to-bitflag parsing |
| Modify | `src/plugin/mod.rs` | Add `capabilities`/`host_api` to `LoadedPlugin`, add `HostContext.plugin_name`, add `build_host_api()`, deny functions, capability negotiation in `load_plugin()`, hook filtering |
| Modify | `tests/plugins/test_plugin/src/lib.rs` | Add `required_capabilities()` implementation |
| Modify | `tests/plugin.rs` | Add sandbox integration tests |

---

### Task 1: Add capability constants to kish-plugin-api

**Files:**
- Modify: `crates/kish-plugin-api/src/lib.rs`

- [ ] **Step 1: Write the capability constants and update PluginDecl**

In `crates/kish-plugin-api/src/lib.rs`, replace the entire file with:

```rust
use std::ffi::{c_char, c_void};

/// API version for compatibility checks between kish and plugins.
pub const KISH_PLUGIN_API_VERSION: u32 = 2;

// ── Capability bitflags ───────────────────────────────────────────────

pub const CAP_VARIABLES_READ: u32 = 0x01;
pub const CAP_VARIABLES_WRITE: u32 = 0x02;
pub const CAP_FILESYSTEM: u32 = 0x04;
pub const CAP_IO: u32 = 0x08;
pub const CAP_HOOK_PRE_EXEC: u32 = 0x10;
pub const CAP_HOOK_POST_EXEC: u32 = 0x20;
pub const CAP_HOOK_ON_CD: u32 = 0x40;

/// All capability bits OR'd together.
pub const CAP_ALL: u32 = CAP_VARIABLES_READ
    | CAP_VARIABLES_WRITE
    | CAP_FILESYSTEM
    | CAP_IO
    | CAP_HOOK_PRE_EXEC
    | CAP_HOOK_POST_EXEC
    | CAP_HOOK_ON_CD;

/// Plugin metadata returned by kish_plugin_decl().
#[repr(C)]
pub struct PluginDecl {
    pub api_version: u32,
    pub name: *const c_char,
    pub version: *const c_char,
    pub required_capabilities: u32,
}

// SAFETY: PluginDecl contains raw pointers to static string data only.
// These are initialized once and never modified, making the struct safe to share.
unsafe impl Send for PluginDecl {}
unsafe impl Sync for PluginDecl {}

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

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p kish-plugin-api`
Expected: compiles with no errors (kish-plugin-sdk and the host will fail due to PluginDecl change — that's expected and fixed in Task 2/3)

- [ ] **Step 3: Commit**

```bash
git add crates/kish-plugin-api/src/lib.rs
git commit -m "feat(plugin-api): add capability constants and required_capabilities to PluginDecl"
```

---

### Task 2: Add Capability enum and update Plugin trait in kish-plugin-sdk

**Files:**
- Modify: `crates/kish-plugin-sdk/src/lib.rs`

- [ ] **Step 1: Add Capability enum and update Plugin trait**

In `crates/kish-plugin-sdk/src/lib.rs`, add the `Capability` enum after the `pub use` line and before the `Plugin` trait. Then add `required_capabilities()` to the trait:

After line 1 (`pub use kish_plugin_api as ffi;`), before line 3 (`use std::ffi::{CStr, CString, c_char};`), insert:

```rust

/// Capabilities a plugin can request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    VariablesRead,
    VariablesWrite,
    Filesystem,
    Io,
    HookPreExec,
    HookPostExec,
    HookOnCd,
}

impl Capability {
    /// Convert to the corresponding FFI bitflag.
    pub fn to_bitflag(self) -> u32 {
        match self {
            Capability::VariablesRead => ffi::CAP_VARIABLES_READ,
            Capability::VariablesWrite => ffi::CAP_VARIABLES_WRITE,
            Capability::Filesystem => ffi::CAP_FILESYSTEM,
            Capability::Io => ffi::CAP_IO,
            Capability::HookPreExec => ffi::CAP_HOOK_PRE_EXEC,
            Capability::HookPostExec => ffi::CAP_HOOK_POST_EXEC,
            Capability::HookOnCd => ffi::CAP_HOOK_ON_CD,
        }
    }
}

/// Convert a slice of capabilities to a combined bitflag.
pub fn capabilities_to_bitflags(caps: &[Capability]) -> u32 {
    caps.iter().fold(0u32, |acc, c| acc | c.to_bitflag())
}
```

Add `required_capabilities()` to the `Plugin` trait, after `fn commands(&self) -> &[&str];`:

```rust
    /// Capabilities this plugin requires. The host may restrict these further
    /// via user configuration.
    fn required_capabilities(&self) -> &[Capability] {
        &[]
    }
```

- [ ] **Step 2: Update the export! macro to embed capabilities in PluginDecl**

In the `export!` macro, update the `kish_plugin_decl()` function. Replace the `PluginDecl` construction (lines 150-154):

```rust
                $crate::ffi::PluginDecl {
                    api_version: $crate::ffi::KISH_PLUGIN_API_VERSION,
                    name: name.as_ptr(),
                    version: version.as_ptr(),
                }
```

with:

```rust
                $crate::ffi::PluginDecl {
                    api_version: $crate::ffi::KISH_PLUGIN_API_VERSION,
                    name: name.as_ptr(),
                    version: version.as_ptr(),
                    required_capabilities: {
                        let plugin = <$plugin_type as Default>::default();
                        $crate::capabilities_to_bitflags(
                            $crate::Plugin::required_capabilities(&plugin),
                        )
                    },
                }
```

- [ ] **Step 3: Verify kish-plugin-sdk compiles**

Run: `cargo check -p kish-plugin-sdk`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add crates/kish-plugin-sdk/src/lib.rs
git commit -m "feat(plugin-sdk): add Capability enum, required_capabilities() to Plugin trait, update export! macro"
```

---

### Task 3: Update test plugin with required_capabilities

**Files:**
- Modify: `tests/plugins/test_plugin/src/lib.rs`

- [ ] **Step 1: Add required_capabilities to TestPlugin**

In `tests/plugins/test_plugin/src/lib.rs`, add the import of `Capability` on line 1:

```rust
use kish_plugin_sdk::{Capability, Plugin, PluginApi, export};
```

Add `required_capabilities()` to the `Plugin` impl, after `fn commands`:

```rust
    fn required_capabilities(&self) -> &[Capability] {
        &[
            Capability::VariablesRead,
            Capability::VariablesWrite,
            Capability::Filesystem,
            Capability::Io,
            Capability::HookPreExec,
            Capability::HookPostExec,
            Capability::HookOnCd,
        ]
    }
```

- [ ] **Step 2: Verify test plugin builds**

Run: `cargo build --manifest-path tests/plugins/test_plugin/Cargo.toml`
Expected: builds successfully

- [ ] **Step 3: Verify existing tests still pass**

Run: `cargo test --test plugin`
Expected: all 10 tests pass (the host-side `load_plugin` doesn't read `required_capabilities` yet, but the struct layout changed — this verifies ABI compatibility)

- [ ] **Step 4: Commit**

```bash
git add tests/plugins/test_plugin/src/lib.rs
git commit -m "feat(test-plugin): declare required_capabilities for all capabilities"
```

---

### Task 4: Add capabilities field to plugin config

**Files:**
- Modify: `src/plugin/config.rs`

- [ ] **Step 1: Write failing test for capabilities parsing**

In `src/plugin/config.rs`, add these tests to the existing `mod tests` block (before the closing `}`):

```rust
    #[test]
    fn parse_capabilities_field() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "restricted"
path = "/usr/lib/librestricted.dylib"
capabilities = ["variables:read", "io", "hooks:pre_exec"]
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        let entry = &config.plugin[0];
        assert_eq!(
            entry.capabilities,
            Some(vec![
                "variables:read".to_string(),
                "io".to_string(),
                "hooks:pre_exec".to_string(),
            ])
        );
    }

    #[test]
    fn parse_missing_capabilities_is_none() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[[plugin]]
name = "trusted"
path = "/usr/lib/libtrusted.dylib"
"#
        )
        .unwrap();
        let config = PluginConfig::load(f.path()).unwrap();
        assert!(config.plugin[0].capabilities.is_none());
    }

    #[test]
    fn parse_capability_string_to_bitflags() {
        use kish_plugin_api::*;
        assert_eq!(
            capability_from_str("variables:read"),
            Some(CAP_VARIABLES_READ)
        );
        assert_eq!(
            capability_from_str("variables:write"),
            Some(CAP_VARIABLES_WRITE)
        );
        assert_eq!(capability_from_str("filesystem"), Some(CAP_FILESYSTEM));
        assert_eq!(capability_from_str("io"), Some(CAP_IO));
        assert_eq!(
            capability_from_str("hooks:pre_exec"),
            Some(CAP_HOOK_PRE_EXEC)
        );
        assert_eq!(
            capability_from_str("hooks:post_exec"),
            Some(CAP_HOOK_POST_EXEC)
        );
        assert_eq!(capability_from_str("hooks:on_cd"), Some(CAP_HOOK_ON_CD));
        assert_eq!(capability_from_str("unknown"), None);
    }

    #[test]
    fn parse_capabilities_to_bitflags() {
        use kish_plugin_api::*;
        let strs = vec![
            "variables:read".to_string(),
            "io".to_string(),
            "hooks:on_cd".to_string(),
        ];
        assert_eq!(
            capabilities_from_strs(&strs),
            CAP_VARIABLES_READ | CAP_IO | CAP_HOOK_ON_CD
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib plugin::config`
Expected: FAIL — `capabilities` field doesn't exist on `PluginEntry`, `capability_from_str` and `capabilities_from_strs` not defined

- [ ] **Step 3: Add capabilities field and parsing functions**

In `src/plugin/config.rs`, add the `capabilities` field to `PluginEntry`:

```rust
#[derive(Debug, Deserialize)]
pub struct PluginEntry {
    pub name: String,
    pub path: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub capabilities: Option<Vec<String>>,
}
```

After the `expand_tilde` function (before `#[cfg(test)]`), add:

```rust
/// Parse a single capability string to its bitflag value.
pub fn capability_from_str(s: &str) -> Option<u32> {
    match s {
        "variables:read" => Some(kish_plugin_api::CAP_VARIABLES_READ),
        "variables:write" => Some(kish_plugin_api::CAP_VARIABLES_WRITE),
        "filesystem" => Some(kish_plugin_api::CAP_FILESYSTEM),
        "io" => Some(kish_plugin_api::CAP_IO),
        "hooks:pre_exec" => Some(kish_plugin_api::CAP_HOOK_PRE_EXEC),
        "hooks:post_exec" => Some(kish_plugin_api::CAP_HOOK_POST_EXEC),
        "hooks:on_cd" => Some(kish_plugin_api::CAP_HOOK_ON_CD),
        _ => None,
    }
}

/// Parse a list of capability strings into a combined bitflag.
/// Unknown strings are ignored.
pub fn capabilities_from_strs(strs: &[String]) -> u32 {
    strs.iter()
        .filter_map(|s| capability_from_str(s))
        .fold(0u32, |acc, f| acc | f)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib plugin::config`
Expected: all config tests pass (including existing ones)

- [ ] **Step 5: Commit**

```bash
git add src/plugin/config.rs
git commit -m "feat(plugin-config): add capabilities field and string-to-bitflag parsing"
```

---

### Task 5: Add deny functions and build_host_api to PluginManager

**Files:**
- Modify: `src/plugin/mod.rs`

- [ ] **Step 1: Add plugin_name to HostContext and update HostContext::new**

In `src/plugin/mod.rs`, update the `HostContext` struct (line 284) and its `new` method:

```rust
struct HostContext<'a> {
    env: &'a mut ShellEnv,
    plugin_name: String,
    /// Buffer for returning C strings from get_var/get_cwd.
    /// Valid until the next callback invocation.
    return_buf: CString,
}

impl<'a> HostContext<'a> {
    fn new(env: &'a mut ShellEnv, plugin_name: &str) -> Self {
        HostContext {
            env,
            plugin_name: plugin_name.to_string(),
            return_buf: CString::default(),
        }
    }
```

- [ ] **Step 2: Add deny functions**

After the existing `host_write_stderr` function (end of file), add all deny function implementations:

```rust
// ── Deny functions for sandboxed capabilities ─────────────────────────

unsafe extern "C" fn deny_get_var(ctx: *mut c_void, _name: *const c_char) -> *const c_char {
    unsafe {
        let host = &*(ctx as *mut HostContext);
        eprintln!(
            "kish: plugin '{}': get_var denied (missing 'variables:read' capability)",
            host.plugin_name
        );
    }
    std::ptr::null()
}

unsafe extern "C" fn deny_set_var(
    ctx: *mut c_void,
    _name: *const c_char,
    _value: *const c_char,
) -> i32 {
    unsafe {
        let host = &*(ctx as *mut HostContext);
        eprintln!(
            "kish: plugin '{}': set_var denied (missing 'variables:write' capability)",
            host.plugin_name
        );
    }
    -1
}

unsafe extern "C" fn deny_export_var(
    ctx: *mut c_void,
    _name: *const c_char,
    _value: *const c_char,
) -> i32 {
    unsafe {
        let host = &*(ctx as *mut HostContext);
        eprintln!(
            "kish: plugin '{}': export_var denied (missing 'variables:write' capability)",
            host.plugin_name
        );
    }
    -1
}

unsafe extern "C" fn deny_get_cwd(ctx: *mut c_void) -> *const c_char {
    unsafe {
        let host = &*(ctx as *mut HostContext);
        eprintln!(
            "kish: plugin '{}': get_cwd denied (missing 'filesystem' capability)",
            host.plugin_name
        );
    }
    std::ptr::null()
}

unsafe extern "C" fn deny_set_cwd(ctx: *mut c_void, _path: *const c_char) -> i32 {
    unsafe {
        let host = &*(ctx as *mut HostContext);
        eprintln!(
            "kish: plugin '{}': set_cwd denied (missing 'filesystem' capability)",
            host.plugin_name
        );
    }
    -1
}

unsafe extern "C" fn deny_write_stdout(
    ctx: *mut c_void,
    _data: *const c_char,
    _len: usize,
) -> i32 {
    unsafe {
        let host = &*(ctx as *mut HostContext);
        eprintln!(
            "kish: plugin '{}': write_stdout denied (missing 'io' capability)",
            host.plugin_name
        );
    }
    -1
}

unsafe extern "C" fn deny_write_stderr(
    ctx: *mut c_void,
    _data: *const c_char,
    _len: usize,
) -> i32 {
    // Cannot log to stderr since write_stderr itself is denied.
    // Use stdout as fallback for the denial message.
    unsafe {
        let host = &*(ctx as *mut HostContext);
        println!(
            "kish: plugin '{}': write_stderr denied (missing 'io' capability)",
            host.plugin_name
        );
    }
    -1
}
```

- [ ] **Step 3: Add build_host_api function**

After the deny functions, add:

```rust
/// Build a HostApi table for a plugin based on its effective capabilities.
/// Denied functions are replaced with stubs that log and return errors.
fn build_host_api(capabilities: u32) -> HostApi {
    use kish_plugin_api::*;

    let has = |cap: u32| capabilities & cap != 0;

    HostApi {
        ctx: std::ptr::null_mut(),
        get_var: if has(CAP_VARIABLES_READ) { host_get_var } else { deny_get_var },
        set_var: if has(CAP_VARIABLES_WRITE) { host_set_var } else { deny_set_var },
        export_var: if has(CAP_VARIABLES_WRITE) { host_export_var } else { deny_export_var },
        get_cwd: if has(CAP_FILESYSTEM) { host_get_cwd } else { deny_get_cwd },
        set_cwd: if has(CAP_FILESYSTEM) { host_set_cwd } else { deny_set_cwd },
        write_stdout: if has(CAP_IO) { host_write_stdout } else { deny_write_stdout },
        write_stderr: if has(CAP_IO) { host_write_stderr } else { deny_write_stderr },
    }
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check --lib`
Expected: may have warnings about `HostContext::new` parameter change — those are addressed in Step 5 of this task.

- [ ] **Step 5: Update all HostContext::new call sites to pass plugin_name**

There are 5 call sites in `mod.rs` where `HostContext::new(env)` is called. Update each to pass the plugin name.

In `load_plugin` (around line 81):
```rust
            let mut ctx = HostContext::new(env, &name);
```
Note: `name` is already available from step 2 of `load_plugin` — but it's extracted inside the `unsafe` block. Move the `HostContext::new` call to after the name is extracted, or pass a placeholder. Actually, looking at the code flow: `name` is extracted in step 2 (line 76) and `HostContext::new` is called in step 3 (line 81). Since `name` is already in scope, just change:
```rust
        // before:
        let mut ctx = HostContext::new(env);
        // after:
        let mut ctx = HostContext::new(env, &name);
```

In `exec_command` (around line 150):
```rust
        let mut ctx = HostContext::new(env, &plugin.name);
```

In `call_pre_exec` (around line 186):
```rust
            let mut ctx = HostContext::new(env, &plugin.name);
```

In `call_post_exec` (around line 209):
```rust
            let mut ctx = HostContext::new(env, &plugin.name);
```

In `call_on_cd` (around line 236):
```rust
            let mut ctx = HostContext::new(env, &plugin.name);
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo check --lib`
Expected: compiles with no errors

- [ ] **Step 7: Commit**

```bash
git add src/plugin/mod.rs
git commit -m "feat(plugin): add deny functions, build_host_api, and plugin_name to HostContext"
```

---

### Task 6: Wire capability negotiation into load_plugin and hook calls

**Files:**
- Modify: `src/plugin/mod.rs`

- [ ] **Step 1: Add capabilities field to LoadedPlugin**

Update the `LoadedPlugin` struct to include the effective capabilities:

```rust
struct LoadedPlugin {
    name: String,
    #[allow(dead_code)]
    library: libloading::Library,
    commands: Vec<String>,
    capabilities: u32,
    has_pre_exec: bool,
    has_post_exec: bool,
    has_on_cd: bool,
}
```

- [ ] **Step 2: Update load_plugin to negotiate capabilities**

Add a new `pub fn load_plugin_with_capabilities` method and update `load_plugin` to delegate to it. In `load_plugin`, after reading `decl` (step 2, line 65-76), read `required_capabilities`:

Replace the current `load_plugin` method with two methods:

```rust
    /// Load a single plugin from a dynamic library path.
    /// Grants all requested capabilities.
    pub fn load_plugin(&mut self, path: &Path, env: &mut ShellEnv) -> Result<(), String> {
        self.load_plugin_with_capabilities(path, env, None)
    }

    /// Load a single plugin with optional capability restrictions.
    /// `config_capabilities`: None = grant all requested, Some(flags) = intersect with requested.
    pub fn load_plugin_with_capabilities(
        &mut self,
        path: &Path,
        env: &mut ShellEnv,
        config_capabilities: Option<u32>,
    ) -> Result<(), String> {
        // 1. Load library
        let library = unsafe { libloading::Library::new(path) }
            .map_err(|e| format!("{}: {}", path.display(), e))?;

        // 2. Get and validate declaration
        let (name, requested_capabilities) = unsafe {
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

            let name = CStr::from_ptr(decl.name).to_string_lossy().into_owned();
            (name, decl.required_capabilities)
        };

        // 3. Negotiate capabilities
        let effective_capabilities = match config_capabilities {
            None => requested_capabilities,
            Some(config_caps) => {
                let effective = requested_capabilities & config_caps;
                let denied = requested_capabilities & !effective;
                if denied != 0 {
                    Self::log_denied_capabilities(&name, denied);
                }
                effective
            }
        };

        // 4. Initialize plugin with sandboxed API
        {
            let mut ctx = HostContext::new(env, &name);
            let mut api = build_host_api(effective_capabilities);
            api.ctx = &mut ctx as *mut HostContext as *mut c_void;

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

        // 5. Get commands
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

        // 6. Check for optional hook functions
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
            capabilities: effective_capabilities,
            has_pre_exec,
            has_post_exec,
            has_on_cd,
        });

        Ok(())
    }

    /// Log which capabilities were requested but not granted.
    fn log_denied_capabilities(plugin_name: &str, denied: u32) {
        use kish_plugin_api::*;
        let caps = [
            (CAP_VARIABLES_READ, "variables:read"),
            (CAP_VARIABLES_WRITE, "variables:write"),
            (CAP_FILESYSTEM, "filesystem"),
            (CAP_IO, "io"),
            (CAP_HOOK_PRE_EXEC, "hooks:pre_exec"),
            (CAP_HOOK_POST_EXEC, "hooks:post_exec"),
            (CAP_HOOK_ON_CD, "hooks:on_cd"),
        ];
        for (flag, name) in caps {
            if denied & flag != 0 {
                eprintln!(
                    "kish: plugin '{}': capability '{}' requested but not granted",
                    plugin_name, name
                );
            }
        }
    }
```

- [ ] **Step 3: Update exec_command to use build_host_api**

Replace the `exec_command` method:

```rust
    pub fn exec_command(
        &self,
        env: &mut ShellEnv,
        name: &str,
        args: &[String],
    ) -> Option<i32> {
        let plugin = self.plugins.iter().find(|p| p.commands.iter().any(|c| c == name))?;

        let mut ctx = HostContext::new(env, &plugin.name);
        let mut api = build_host_api(plugin.capabilities);
        api.ctx = &mut ctx as *mut HostContext as *mut c_void;

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
```

- [ ] **Step 4: Update hook methods with capability checks**

Replace the three hook methods:

```rust
    pub fn call_pre_exec(&self, env: &mut ShellEnv, cmd: &str) {
        let c_cmd = match CString::new(cmd) {
            Ok(c) => c,
            Err(_) => return,
        };
        for plugin in &self.plugins {
            if !plugin.has_pre_exec {
                continue;
            }
            if plugin.capabilities & kish_plugin_api::CAP_HOOK_PRE_EXEC == 0 {
                continue;
            }
            let mut ctx = HostContext::new(env, &plugin.name);
            let mut api = build_host_api(plugin.capabilities);
            api.ctx = &mut ctx as *mut HostContext as *mut c_void;
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

    pub fn call_post_exec(&self, env: &mut ShellEnv, cmd: &str, exit_code: i32) {
        let c_cmd = match CString::new(cmd) {
            Ok(c) => c,
            Err(_) => return,
        };
        for plugin in &self.plugins {
            if !plugin.has_post_exec {
                continue;
            }
            if plugin.capabilities & kish_plugin_api::CAP_HOOK_POST_EXEC == 0 {
                continue;
            }
            let mut ctx = HostContext::new(env, &plugin.name);
            let mut api = build_host_api(plugin.capabilities);
            api.ctx = &mut ctx as *mut HostContext as *mut c_void;
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
            if plugin.capabilities & kish_plugin_api::CAP_HOOK_ON_CD == 0 {
                continue;
            }
            let mut ctx = HostContext::new(env, &plugin.name);
            let mut api = build_host_api(plugin.capabilities);
            api.ctx = &mut ctx as *mut HostContext as *mut c_void;
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
```

- [ ] **Step 5: Update load_from_config to pass capabilities**

Replace the `load_from_config` method:

```rust
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
            let config_caps = entry
                .capabilities
                .as_ref()
                .map(|strs| config::capabilities_from_strs(strs));
            if let Err(e) = self.load_plugin_with_capabilities(&path, env, config_caps) {
                eprintln!("kish: plugin: {}", e);
            }
        }
    }
```

- [ ] **Step 6: Remove the old build_api method from HostContext**

Delete the `build_api` method from `impl<'a> HostContext<'a>` since we now use the standalone `build_host_api` function:

```rust
impl<'a> HostContext<'a> {
    fn new(env: &'a mut ShellEnv, plugin_name: &str) -> Self {
        HostContext {
            env,
            plugin_name: plugin_name.to_string(),
            return_buf: CString::default(),
        }
    }
}
```

- [ ] **Step 7: Verify it compiles**

Run: `cargo check --lib`
Expected: compiles with no errors

- [ ] **Step 8: Verify existing tests still pass**

Run: `cargo test --test plugin`
Expected: all 10 existing tests pass (test plugin requests all capabilities, `load_plugin` grants all by default)

- [ ] **Step 9: Commit**

```bash
git add src/plugin/mod.rs
git commit -m "feat(plugin): wire capability negotiation into load_plugin and hook calls"
```

---

### Task 7: Add sandbox integration tests

**Files:**
- Modify: `tests/plugin.rs`

- [ ] **Step 1: Write test for deny_set_var when variables:write is not granted**

Add to `tests/plugin.rs`:

```rust
#[test]
fn sandbox_deny_set_var_without_capability() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);

    // Load with only variables:read + io (no variables:write)
    let caps = kish_plugin_api::CAP_VARIABLES_READ | kish_plugin_api::CAP_IO;
    manager
        .load_plugin_with_capabilities(&dylib, &mut env, Some(caps))
        .unwrap();

    // test-set-var calls set_var — should be denied
    let status = manager.exec_command(
        &mut env,
        "test-set-var",
        &["MY_VAR".to_string(), "my_value".to_string()],
    );
    assert!(status.is_some());
    // Variable should NOT be set because set_var was denied
    assert_eq!(env.vars.get("MY_VAR"), None);
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test --test plugin sandbox_deny_set_var_without_capability`
Expected: PASS

- [ ] **Step 3: Write test for hook capability filtering**

Add to `tests/plugin.rs`:

```rust
#[test]
fn sandbox_hook_not_fired_without_capability() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);

    // Load with variables:write + io but NO hook capabilities
    let caps = kish_plugin_api::CAP_VARIABLES_READ
        | kish_plugin_api::CAP_VARIABLES_WRITE
        | kish_plugin_api::CAP_IO;
    manager
        .load_plugin_with_capabilities(&dylib, &mut env, Some(caps))
        .unwrap();

    // Hooks should not fire
    manager.call_pre_exec(&mut env, "echo hello");
    assert_eq!(env.vars.get("TEST_PRE_EXEC"), None);

    manager.call_post_exec(&mut env, "ls -la", 0);
    assert_eq!(env.vars.get("TEST_POST_EXEC"), None);

    manager.call_on_cd(&mut env, "/old", "/new");
    assert_eq!(env.vars.get("TEST_ON_CD"), None);
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test plugin sandbox_hook_not_fired_without_capability`
Expected: PASS

- [ ] **Step 5: Write test for selective hook capability**

Add to `tests/plugin.rs`:

```rust
#[test]
fn sandbox_selective_hook_capability() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);

    // Grant variables:write (for hooks to set_var) + only pre_exec hook
    let caps = kish_plugin_api::CAP_VARIABLES_READ
        | kish_plugin_api::CAP_VARIABLES_WRITE
        | kish_plugin_api::CAP_IO
        | kish_plugin_api::CAP_HOOK_PRE_EXEC;
    manager
        .load_plugin_with_capabilities(&dylib, &mut env, Some(caps))
        .unwrap();

    // pre_exec should fire
    manager.call_pre_exec(&mut env, "echo hello");
    assert_eq!(env.vars.get("TEST_PRE_EXEC"), Some("echo hello"));

    // post_exec should NOT fire
    manager.call_post_exec(&mut env, "ls", 0);
    assert_eq!(env.vars.get("TEST_POST_EXEC"), None);

    // on_cd should NOT fire
    manager.call_on_cd(&mut env, "/old", "/new");
    assert_eq!(env.vars.get("TEST_ON_CD"), None);
}
```

- [ ] **Step 6: Write test for full capabilities (backward compatibility)**

Add to `tests/plugin.rs`:

```rust
#[test]
fn sandbox_full_capabilities_works_normally() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);

    // Load with all capabilities (None = trust mode)
    manager.load_plugin(&dylib, &mut env).unwrap();

    // Everything should work as before
    let status = manager.exec_command(&mut env, "test-hello", &[]);
    assert_eq!(status, Some(0));
    assert_eq!(env.vars.get("TEST_EXEC_CALLED"), Some("1"));

    manager.call_pre_exec(&mut env, "echo");
    assert_eq!(env.vars.get("TEST_PRE_EXEC"), Some("echo"));

    manager.call_post_exec(&mut env, "ls", 42);
    assert_eq!(env.vars.get("TEST_POST_EXEC"), Some("ls:42"));

    manager.call_on_cd(&mut env, "/a", "/b");
    assert_eq!(env.vars.get("TEST_ON_CD"), Some("/a->/b"));
}
```

- [ ] **Step 7: Write test for config-based restriction (intersection semantics)**

Add to `tests/plugin.rs`:

```rust
#[test]
fn sandbox_config_restricts_capabilities() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);

    // Plugin requests all capabilities, config only grants io
    let caps = kish_plugin_api::CAP_IO;
    manager
        .load_plugin_with_capabilities(&dylib, &mut env, Some(caps))
        .unwrap();

    // test-hello calls print (io) and set_var (variables:write)
    // print should work, set_var should be denied
    let status = manager.exec_command(&mut env, "test-hello", &[]);
    assert_eq!(status, Some(0));
    // set_var("TEST_EXEC_CALLED", "1") was denied
    assert_eq!(env.vars.get("TEST_EXEC_CALLED"), None);
}
```

- [ ] **Step 8: Run all plugin tests**

Run: `cargo test --test plugin`
Expected: all tests pass (original 10 + new 5 = 15)

- [ ] **Step 9: Commit**

```bash
git add tests/plugin.rs
git commit -m "test(plugin): add sandbox integration tests for capability enforcement"
```

---

### Task 8: Final verification and cleanup

**Files:**
- None new — verification only

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 2: Run config unit tests specifically**

Run: `cargo test --lib plugin::config`
Expected: all config tests pass

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: no warnings

- [ ] **Step 4: Commit any clippy fixes if needed**

```bash
git add -u
git commit -m "fix: address clippy warnings from plugin sandbox changes"
```
