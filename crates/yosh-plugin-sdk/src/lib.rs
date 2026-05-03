//! yosh-plugin-sdk — Rust SDK for authoring yosh plugins.
//!
//! Plugins implement the [`Plugin`] trait and invoke [`export!`] to wire
//! the trait into the WIT-generated guest bindings.
//!
//! # Capabilities
//!
//! Every host import (variables, filesystem, io, files, commands) is
//! gated by a capability declared in [`Plugin::required_capabilities`].
//! Without the matching capability, the underlying host call returns
//! [`ErrorCode::Denied`]. (Some helpers like [`exists`] swallow this
//! into a default value — see each function's docs.)
//!
//! # `metadata` is special
//!
//! The host calls [`Plugin::commands`], [`Plugin::required_capabilities`],
//! and [`Plugin::implemented_hooks`] without an active `ShellEnv`
//! binding (to read static plugin metadata). Any host import (e.g.
//! [`cwd`], [`get_var`]) called from inside those methods returns
//! [`ErrorCode::Denied`] regardless of granted capabilities. Treat
//! these methods as pure functions of `&self`.
//!
//! # `files:write` sandbox
//!
//! The `files:write` capability has no per-path allowlist. Once
//! granted, a plugin can write to or delete any path the host process
//! can reach. The capability grant is the entire access boundary;
//! treat it accordingly when configuring plugin permissions.

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

/// The trait every yosh plugin implements.
///
/// Plugins are instantiated once per load via `Default::default()` and
/// kept alive in the host's wasm store for the lifetime of the load.
/// `Send + 'static` is required so the host can move the instance
/// across awaits in interactive use.
///
/// `commands`, `required_capabilities`, and `implemented_hooks` are
/// invoked as part of static metadata synthesis without an active
/// `ShellEnv`; they MUST NOT call any host import. See the
/// crate-level `metadata is special` section.
pub trait Plugin: Send + Default + 'static {
    /// The subcommands this plugin handles.
    ///
    /// When the user types `name args...` at the shell prompt, if
    /// `name` matches one of these strings the host dispatches to
    /// [`Self::exec`].
    fn commands(&self) -> &[&'static str];

    /// The capabilities this plugin needs to function.
    ///
    /// Each capability listed here must be granted by host
    /// configuration. Missing capabilities cause the plugin to fail
    /// to load. Default: no capabilities.
    fn required_capabilities(&self) -> &[Capability] {
        &[]
    }

    /// Hooks this plugin actually overrides.
    ///
    /// Rust cannot reflectively detect default-method overrides, so
    /// plugins enumerate explicitly. Listing a hook here registers it
    /// with the host; not listing it means the host skips dispatching
    /// even if you have overridden the corresponding `hook_*` method.
    fn implemented_hooks(&self) -> &[HookName] {
        &[]
    }

    /// Called once after the plugin is loaded and capability checks
    /// pass.
    ///
    /// Use for one-time setup. Returning `Err(_)` aborts the load and
    /// surfaces the message to the user.
    fn on_load(&mut self) -> Result<(), String> {
        Ok(())
    }

    /// Entry point for every dispatched subcommand.
    ///
    /// `command` is one of the strings returned by [`Self::commands`].
    /// `args` is the remaining argv. Return value is the shell exit
    /// code (0 = success).
    fn exec(&mut self, command: &str, args: &[String]) -> i32;

    /// Fires before each command runs (functions, builtins, and
    /// external commands all dispatch through the same hook point).
    ///
    /// `command` is the expanded command line (program name plus
    /// space-joined args) as the host saw it.
    fn hook_pre_exec(&mut self, _command: &str) {}

    /// Fires after each command finishes (functions, builtins, and
    /// external commands).
    ///
    /// `exit_code` is the command's exit status.
    fn hook_post_exec(&mut self, _command: &str, _exit_code: i32) {}

    /// Fires after a successful `cd`. Both arguments are absolute paths.
    fn hook_on_cd(&mut self, _old_dir: &str, _new_dir: &str) {}

    /// Fires before each interactive prompt is rendered.
    fn hook_pre_prompt(&mut self) {}

    /// Best-effort cleanup before the plugin is dropped.
    ///
    /// Not guaranteed to run if the host process crashes. Persistent
    /// state should be flushed eagerly, not deferred to here.
    fn on_unload(&mut self) {}
}

// ── Host API helpers (typed wrappers over WIT-generated bindings) ────

/// Look up a shell variable by name.
///
/// Requires the `variables:read` capability.
///
/// Returns `Ok(None)` if the variable is unset, `Ok(Some(""))` if it
/// is set to the empty string. The outer `Result` carries denial; the
/// inner `Option` distinguishes unset from set-to-empty.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `variables:read` not granted, or called
///   from `metadata`-time methods on [`Plugin`].
pub fn get_var(name: &str) -> Result<Option<String>, ErrorCode> {
    host_variables::get(name)
}

/// Set a shell variable.
///
/// Equivalent to `name=value` at the shell prompt: new variables are
/// created unexported, while updates to an already-exported variable
/// preserve its export flag. To both set the value and mark a
/// variable for export to spawned-child environments, use
/// [`export_var`].
///
/// Requires the `variables:write` capability.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `variables:write` not granted.
/// - [`ErrorCode::IoFailed`] — the host's variable store rejected the
///   assignment (e.g., readonly variable).
pub fn set_var(name: &str, value: &str) -> Result<(), ErrorCode> {
    host_variables::set(name, value)
}

/// Set a shell variable and mark it for export to spawned-child
/// environments (equivalent to `export name=value` in the shell).
///
/// Requires the `variables:write` capability.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `variables:write` not granted.
/// - [`ErrorCode::IoFailed`] — the host's variable store rejected the
///   assignment.
pub fn export_var(name: &str, value: &str) -> Result<(), ErrorCode> {
    host_variables::export_env(name, value)
}

/// Return the shell's current working directory as an absolute path.
///
/// Requires the `filesystem` capability — a single coarse capability
/// covering both read and write of the cwd; not split into
/// `filesystem:read` / `filesystem:write`.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `filesystem` not granted.
/// - [`ErrorCode::IoFailed`] — the host could not read the cwd
///   (e.g., the cwd was unlinked while the shell was running).
pub fn cwd() -> Result<String, ErrorCode> {
    host_filesystem::cwd()
}

/// Change the shell's current working directory.
///
/// Affects every subsequent host call (cwd reads, relative-path file
/// ops, child-process spawn cwd) — not just the calling plugin.
///
/// Requires the `filesystem` capability.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `filesystem` not granted.
/// - [`ErrorCode::IoFailed`] — the path does not exist or is not a
///   directory, or the host process lacks permission to enter it.
pub fn set_cwd(path: &str) -> Result<(), ErrorCode> {
    host_filesystem::set_cwd(path)
}

/// Write a UTF-8 string to the shell's stdout.
///
/// Convenience wrapper over [`write_bytes`] for the common case.
/// Does not append a newline; pass `"foo\n"` if you want one.
///
/// Requires the `io` capability (single capability; no read/write split).
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `io` not granted.
/// - [`ErrorCode::IoFailed`] — the underlying write failed (e.g.,
///   broken pipe).
pub fn print(s: &str) -> Result<(), ErrorCode> {
    host_io::write(IoStream::Stdout, s.as_bytes())
}

/// Write a UTF-8 string to the shell's stderr.
///
/// Convenience wrapper over [`write_bytes`]. Does not append a newline.
///
/// Requires the `io` capability.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `io` not granted.
/// - [`ErrorCode::IoFailed`] — the underlying write failed.
pub fn eprint(s: &str) -> Result<(), ErrorCode> {
    host_io::write(IoStream::Stderr, s.as_bytes())
}

/// Write raw bytes to stdout or stderr.
///
/// Use this when output may contain non-UTF-8 (binary) data. For
/// UTF-8 strings, prefer [`fn@print`] / [`fn@eprint`].
///
/// Requires the `io` capability.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `io` not granted.
/// - [`ErrorCode::IoFailed`] — the underlying write failed.
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
