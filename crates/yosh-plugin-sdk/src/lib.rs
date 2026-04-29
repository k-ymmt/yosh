//! yosh-plugin-sdk — Rust SDK for authoring yosh plugins.
//!
//! Plugins implement the [`Plugin`] trait and invoke [`export!`] to wire
//! the trait into the WIT-generated guest bindings.

#![allow(clippy::missing_safety_doc)]

mod export;
pub mod style;

pub use yosh_plugin_api as ffi;

#[doc(hidden)]
pub use wit_bindgen;

// Generate the wit-bindgen guest bindings for the yosh:plugin/plugin-world.
// export_macro_name avoids a collision with our own user-facing `export!` macro.
wit_bindgen::generate!({
    world: "plugin-world",
    path: "../yosh-plugin-api/wit",
    pub_export_macro: true,
    export_macro_name: "export_wit_bindings",
    generate_all,
});

// Re-export under stable names so the `export!` macro can refer to them
// predictably, and so plugin authors get one obvious import path.
pub use self::exports::yosh::plugin::hooks as hooks_iface;
pub use self::exports::yosh::plugin::plugin as plugin_iface;
pub use self::yosh::plugin::commands as host_commands;
pub use self::yosh::plugin::commands::ExecOutput;
pub use self::yosh::plugin::files as host_files;
pub use self::yosh::plugin::files::{DirEntry, FileStat};
pub use self::yosh::plugin::filesystem as host_filesystem;
pub use self::yosh::plugin::io as host_io;
pub use self::yosh::plugin::types::{ErrorCode, HookName, IoStream, PluginInfo};
pub use self::yosh::plugin::variables as host_variables;

// ── Plugin author-facing types ───────────────────────────────────────

pub use yosh_plugin_api::{Capability, capabilities_to_bitflags};

/// The trait every plugin implements.
pub trait Plugin: Send + Default + 'static {
    fn commands(&self) -> &[&'static str];

    fn required_capabilities(&self) -> &[Capability] {
        &[]
    }

    /// Hooks this plugin actually overrides. Rust cannot reflectively
    /// detect default-method overrides, so plugins enumerate explicitly.
    fn implemented_hooks(&self) -> &[HookName] {
        &[]
    }

    fn on_load(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn exec(&mut self, command: &str, args: &[String]) -> i32;

    fn hook_pre_exec(&mut self, _command: &str) {}
    fn hook_post_exec(&mut self, _command: &str, _exit_code: i32) {}
    fn hook_on_cd(&mut self, _old_dir: &str, _new_dir: &str) {}
    fn hook_pre_prompt(&mut self) {}

    fn on_unload(&mut self) {}
}

// ── Host API helpers (typed wrappers over WIT-generated bindings) ────

pub fn get_var(name: &str) -> Result<Option<String>, ErrorCode> {
    host_variables::get(name)
}

pub fn set_var(name: &str, value: &str) -> Result<(), ErrorCode> {
    host_variables::set(name, value)
}

pub fn export_var(name: &str, value: &str) -> Result<(), ErrorCode> {
    host_variables::export_env(name, value)
}

pub fn cwd() -> Result<String, ErrorCode> {
    host_filesystem::cwd()
}

pub fn set_cwd(path: &str) -> Result<(), ErrorCode> {
    host_filesystem::set_cwd(path)
}

pub fn print(s: &str) -> Result<(), ErrorCode> {
    host_io::write(IoStream::Stdout, s.as_bytes())
}

pub fn eprint(s: &str) -> Result<(), ErrorCode> {
    host_io::write(IoStream::Stderr, s.as_bytes())
}

pub fn write_bytes(stream: IoStream, data: &[u8]) -> Result<(), ErrorCode> {
    host_io::write(stream, data)
}

// ── files:read helpers ───────────────────────────────────────────────

pub fn read_file(path: &str) -> Result<Vec<u8>, ErrorCode> {
    host_files::read_file(path)
}

pub fn read_to_string(path: &str) -> Result<String, ErrorCode> {
    let bytes = host_files::read_file(path)?;
    String::from_utf8(bytes).map_err(|_| ErrorCode::InvalidArgument)
}

pub fn read_dir(path: &str) -> Result<Vec<DirEntry>, ErrorCode> {
    host_files::read_dir(path)
}

pub fn metadata(path: &str) -> Result<FileStat, ErrorCode> {
    host_files::metadata(path)
}

pub fn exists(path: &str) -> bool {
    host_files::metadata(path).is_ok()
}

// ── files:write helpers ──────────────────────────────────────────────

pub fn write_file(path: &str, data: &[u8]) -> Result<(), ErrorCode> {
    host_files::write_file(path, data)
}

pub fn write_string(path: &str, s: &str) -> Result<(), ErrorCode> {
    host_files::write_file(path, s.as_bytes())
}

pub fn append_file(path: &str, data: &[u8]) -> Result<(), ErrorCode> {
    host_files::append_file(path, data)
}

pub fn create_dir(path: &str) -> Result<(), ErrorCode> {
    host_files::create_dir(path, false)
}

pub fn create_dir_all(path: &str) -> Result<(), ErrorCode> {
    host_files::create_dir(path, true)
}

pub fn remove_file(path: &str) -> Result<(), ErrorCode> {
    host_files::remove_file(path)
}

pub fn remove_dir(path: &str) -> Result<(), ErrorCode> {
    host_files::remove_dir(path, false)
}

pub fn remove_dir_all(path: &str) -> Result<(), ErrorCode> {
    host_files::remove_dir(path, true)
}

// ── commands:exec helpers ────────────────────────────────────────────

/// Run an external command. Subject to the host's `commands:exec`
/// capability and `allowed_commands` allowlist, plus a 1000ms timeout.
///
/// The child inherits the shell's current working directory and full
/// environment. Stdin is `/dev/null`.
///
/// Returns the captured stdout/stderr and exit code on a normal
/// process exit.
///
/// # Errors
///
/// - `Err(ErrorCode::Denied)` — the `commands:exec` capability isn't granted.
/// - `Err(ErrorCode::PatternNotAllowed)` — the argv is not matched by any
///   entry in the plugin's `allowed_commands` allowlist.
/// - `Err(ErrorCode::Timeout)` — the 1000ms host-enforced cap was hit.
/// - `Err(ErrorCode::NotFound)` — `program` was not found on PATH.
/// - `Err(ErrorCode::InvalidArgument)` — `program` is an empty string.
pub fn exec(program: &str, args: &[&str]) -> Result<ExecOutput, ErrorCode> {
    let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    host_commands::exec(program, &args_owned)
}
