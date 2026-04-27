//! yosh-plugin-sdk — Rust SDK for authoring yosh plugins.
//!
//! Plugins implement the [`Plugin`] trait and invoke [`export!`] to wire
//! the trait into the WIT-generated guest bindings.

#![allow(clippy::missing_safety_doc)]

pub mod style;
mod export;

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
pub use self::exports::yosh::plugin::plugin as plugin_iface;
pub use self::exports::yosh::plugin::hooks as hooks_iface;
pub use self::yosh::plugin::types::{ErrorCode, HookName, IoStream, PluginInfo};
pub use self::yosh::plugin::filesystem as host_filesystem;
pub use self::yosh::plugin::io as host_io;
pub use self::yosh::plugin::variables as host_variables;

// ── Plugin author-facing types ───────────────────────────────────────

pub use yosh_plugin_api::{Capability, capabilities_to_bitflags};

/// The trait every plugin implements.
pub trait Plugin: Send + Default + 'static {
    fn commands(&self) -> &[&'static str];

    fn required_capabilities(&self) -> &[Capability] { &[] }

    /// Hooks this plugin actually overrides. Rust cannot reflectively
    /// detect default-method overrides, so plugins enumerate explicitly.
    fn implemented_hooks(&self) -> &[HookName] { &[] }

    fn on_load(&mut self) -> Result<(), String> { Ok(()) }

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
