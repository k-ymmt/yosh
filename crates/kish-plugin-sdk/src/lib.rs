pub use kish_plugin_api as ffi;

use std::ffi::{CStr, CString, c_char};

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
