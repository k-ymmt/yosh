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
