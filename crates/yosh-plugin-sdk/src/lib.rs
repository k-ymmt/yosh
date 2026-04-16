pub mod style;

pub use yosh_plugin_api as ffi;

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
    HookPrePrompt,
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
            Capability::HookPrePrompt => ffi::CAP_HOOK_PRE_PROMPT,
        }
    }
}

/// Convert a slice of capabilities to a combined bitflag.
pub fn capabilities_to_bitflags(caps: &[Capability]) -> u32 {
    caps.iter().fold(0u32, |acc, c| acc | c.to_bitflag())
}

use std::ffi::{CStr, CString, c_char};

/// Trait plugin authors implement. Requires `Default` for the export! macro.
pub trait Plugin: Send + Default {
    /// Command names this plugin provides.
    fn commands(&self) -> &[&str];

    /// Capabilities this plugin requires. The host may restrict these further
    /// via user configuration.
    fn required_capabilities(&self) -> &[Capability] {
        &[]
    }

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

    /// Hook: called before the interactive prompt is displayed.
    fn hook_pre_prompt(&mut self, _api: &PluginApi) {}

    /// Called when the plugin is about to be unloaded.
    fn on_unload(&mut self) {}
}

/// Safe wrapper around the host API callbacks.
pub struct PluginApi {
    api: *const ffi::HostApi,
}

/// A Send + Sync wrapper for *const c_char pointers.
/// SAFETY: These pointers are only used to point to static string data.
pub struct CCharPtr(pub *const c_char);
unsafe impl Send for CCharPtr {}
unsafe impl Sync for CCharPtr {}

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
/// Usage: `yosh_plugin_sdk::export!(MyPlugin);`
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
        static COMMAND_PTRS: OnceLock<Vec<CCharPtr>> = OnceLock::new();

        // Helper to wrap raw pointers in CCharPtr for static storage
        use $crate::CCharPtr;

        #[allow(unsafe_attr_outside_unsafe)]
        #[unsafe(no_mangle)]
        pub extern "C" fn yosh_plugin_decl() -> *const $crate::ffi::PluginDecl {
            PLUGIN_DECL_STATIC.get_or_init(|| {
                let name = PLUGIN_NAME_CSTR.get_or_init(|| {
                    CString::new(env!("CARGO_PKG_NAME")).unwrap()
                });
                let version = PLUGIN_VERSION_CSTR.get_or_init(|| {
                    CString::new(env!("CARGO_PKG_VERSION")).unwrap()
                });
                $crate::ffi::PluginDecl {
                    api_version: $crate::ffi::YOSH_PLUGIN_API_VERSION,
                    name: name.as_ptr(),
                    version: version.as_ptr(),
                    required_capabilities: {
                        let plugin = <$plugin_type as Default>::default();
                        $crate::capabilities_to_bitflags(
                            $crate::Plugin::required_capabilities(&plugin),
                        )
                    },
                }
            })
        }

        #[allow(unsafe_attr_outside_unsafe)]
        #[unsafe(no_mangle)]
        pub extern "C" fn yosh_plugin_init(api: *const $crate::ffi::HostApi) -> i32 {
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

        #[allow(unsafe_attr_outside_unsafe)]
        #[unsafe(no_mangle)]
        pub extern "C" fn yosh_plugin_commands(count: *mut u32) -> *const *const c_char {
            let cstrs = COMMAND_CSTRS.get_or_init(|| {
                let plugin = PLUGIN_INSTANCE.lock().unwrap();
                let p = plugin.as_ref().expect("yosh_plugin_commands called before init");
                $crate::Plugin::commands(p)
                    .iter()
                    .map(|s| CString::new(*s).unwrap())
                    .collect()
            });
            let ptrs = COMMAND_PTRS.get_or_init(|| {
                cstrs.iter().map(|s| CCharPtr(s.as_ptr())).collect()
            });
            unsafe { *count = ptrs.len() as u32; }
            // Cast the Vec of CCharPtr to Vec of raw pointers
            ptrs.as_ptr() as *const *const c_char
        }

        #[allow(unsafe_attr_outside_unsafe)]
        #[unsafe(no_mangle)]
        pub extern "C" fn yosh_plugin_exec(
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

        #[allow(unsafe_attr_outside_unsafe)]
        #[unsafe(no_mangle)]
        pub extern "C" fn yosh_plugin_hook_pre_exec(
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

        #[allow(unsafe_attr_outside_unsafe)]
        #[unsafe(no_mangle)]
        pub extern "C" fn yosh_plugin_hook_post_exec(
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

        #[allow(unsafe_attr_outside_unsafe)]
        #[unsafe(no_mangle)]
        pub extern "C" fn yosh_plugin_hook_on_cd(
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

        #[allow(unsafe_attr_outside_unsafe)]
        #[unsafe(no_mangle)]
        pub extern "C" fn yosh_plugin_hook_pre_prompt(
            api: *const $crate::ffi::HostApi,
        ) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let plugin_api = unsafe { $crate::PluginApi::from_raw(api) };
                let mut plugin = PLUGIN_INSTANCE.lock().unwrap();
                if let Some(p) = plugin.as_mut() {
                    $crate::Plugin::hook_pre_prompt(p, &plugin_api);
                }
            }));
        }

        #[allow(unsafe_attr_outside_unsafe)]
        #[unsafe(no_mangle)]
        pub extern "C" fn yosh_plugin_destroy() {
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
