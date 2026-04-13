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

    /// Execute a plugin command. Returns Some(exit_status) if a plugin handled
    /// the command, or None if no plugin provides this command.
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

unsafe extern "C" fn host_set_cwd(_ctx: *mut c_void, path: *const c_char) -> i32 {
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
