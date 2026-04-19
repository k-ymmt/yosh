use std::ffi::{c_char, c_void};

/// API version for compatibility checks between yosh and plugins.
pub const YOSH_PLUGIN_API_VERSION: u32 = 2;

// ── Capability bitflags ───────────────────────────────────────────────

pub const CAP_VARIABLES_READ: u32 = 0x01;
pub const CAP_VARIABLES_WRITE: u32 = 0x02;
pub const CAP_FILESYSTEM: u32 = 0x04;
pub const CAP_IO: u32 = 0x08;
pub const CAP_HOOK_PRE_EXEC: u32 = 0x10;
pub const CAP_HOOK_POST_EXEC: u32 = 0x20;
pub const CAP_HOOK_ON_CD: u32 = 0x40;
pub const CAP_HOOK_PRE_PROMPT: u32 = 0x80;

/// All capability bits OR'd together.
pub const CAP_ALL: u32 = CAP_VARIABLES_READ
    | CAP_VARIABLES_WRITE
    | CAP_FILESYSTEM
    | CAP_IO
    | CAP_HOOK_PRE_EXEC
    | CAP_HOOK_POST_EXEC
    | CAP_HOOK_ON_CD
    | CAP_HOOK_PRE_PROMPT;

/// Plugin metadata returned by yosh_plugin_decl().
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

/// API callbacks yosh provides to plugins.
///
/// `ctx` is an opaque pointer to yosh internals. Plugins pass it back to each
/// callback but must not dereference or store it beyond the current call.
#[repr(C)]
pub struct HostApi {
    pub ctx: *mut c_void,

    // Variable operations
    pub get_var: unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char) -> *const c_char,
    pub set_var:
        unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char, value: *const c_char) -> i32,
    pub export_var:
        unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char, value: *const c_char) -> i32,

    // Environment
    pub get_cwd: unsafe extern "C" fn(ctx: *mut c_void) -> *const c_char,
    pub set_cwd: unsafe extern "C" fn(ctx: *mut c_void, path: *const c_char) -> i32,

    // Output
    pub write_stdout:
        unsafe extern "C" fn(ctx: *mut c_void, data: *const c_char, len: usize) -> i32,
    pub write_stderr:
        unsafe extern "C" fn(ctx: *mut c_void, data: *const c_char, len: usize) -> i32,
}
