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
//! The one exception is `settings` ([`read_settings`]): reading the
//! plugin's own `settings.toml` requires no capability.
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
//
// Path is `wit/` inside this crate — the canonical source lives in
// `yosh-plugin-api/wit/` and `build.rs` verifies the bundled copy matches
// when built inside the workspace. The bundled copy is required because
// `cargo install yosh-plugin-sdk` extracts the crate standalone, with no
// `../yosh-plugin-api/` directory available.
wit_bindgen::generate!({
    world: "plugin-world",
    path: "wit",
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
pub use self::yosh::plugin::settings as host_settings;
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
    /// The host intersects this list with the `capabilities`
    /// allowlist in the user's `plugins.toml` (no allowlist grants
    /// everything requested). The plugin still loads when some
    /// capabilities are denied: the host prints a
    /// "requested but not granted" warning per denied capability and
    /// the corresponding host imports return `ErrorCode::Denied` at
    /// call time. Default: no capabilities.
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

/// Read the entire file at `path` into a byte vector.
///
/// Requires the `files:read` capability.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `files:read` not granted.
/// - [`ErrorCode::InvalidArgument`] — `path` is empty.
/// - [`ErrorCode::NotFound`] — the file does not exist.
/// - [`ErrorCode::IoFailed`] — any other I/O error (permission
///   denied, is-a-directory, etc.).
pub fn read_file(path: &str) -> Result<Vec<u8>, ErrorCode> {
    host_files::read_file(path)
}

/// Read the entire file at `path` and decode as UTF-8.
///
/// Requires the `files:read` capability.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `files:read` not granted.
/// - [`ErrorCode::InvalidArgument`] — **two distinct conditions
///   collapse to this code:** (1) `path` is empty, OR (2) the file
///   contents are not valid UTF-8. Callers cannot distinguish "binary
///   content" from "bad path" through the error code alone. **For
///   files that may contain non-UTF-8 bytes, prefer [`read_file`] and
///   decode explicitly with [`String::from_utf8`].**
/// - [`ErrorCode::NotFound`] — the file does not exist.
/// - [`ErrorCode::IoFailed`] — any other I/O error.
pub fn read_to_string(path: &str) -> Result<String, ErrorCode> {
    let bytes = host_files::read_file(path)?;
    String::from_utf8(bytes).map_err(|_| ErrorCode::InvalidArgument)
}

/// List the entries of a directory.
///
/// Each [`DirEntry`] reports `is_file` / `is_dir` / `is_symlink`
/// based on the entry's own type without following symlinks (unlike
/// [`metadata`], which does follow them — see that function's docs).
///
/// Requires the `files:read` capability.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `files:read` not granted.
/// - [`ErrorCode::InvalidArgument`] — `path` is empty.
/// - [`ErrorCode::NotFound`] — the directory does not exist.
/// - [`ErrorCode::IoFailed`] — any other I/O error.
pub fn read_dir(path: &str) -> Result<Vec<DirEntry>, ErrorCode> {
    host_files::read_dir(path)
}

/// Return file metadata for `path`.
///
/// **Symlink behavior:** the host follows symlinks before reading
/// metadata, so [`FileStat::is_symlink`] is effectively always
/// `false` even when `path` itself is a symlink. To detect symlinks
/// today, call [`read_dir`] on the parent directory and inspect the
/// matching [`DirEntry::is_symlink`].
///
/// Requires the `files:read` capability.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `files:read` not granted.
/// - [`ErrorCode::InvalidArgument`] — `path` is empty.
/// - [`ErrorCode::NotFound`] — the path does not exist.
/// - [`ErrorCode::IoFailed`] — any other I/O error.
pub fn metadata(path: &str) -> Result<FileStat, ErrorCode> {
    host_files::metadata(path)
}

/// Test whether `path` exists.
///
/// **Hazard:** returns `false` if the path is missing **or** the
/// `files:read` capability is not granted **or** any I/O error
/// occurred. Callers cannot distinguish these cases through the
/// boolean alone. If you need to tell `Denied` apart from "really
/// not there" (e.g., for clearer error messages), call [`metadata`]
/// directly and inspect the [`Err`] variant.
///
/// Requires the `files:read` capability (silently treated as
/// "missing" on denial).
pub fn exists(path: &str) -> bool {
    host_files::metadata(path).is_ok()
}

// ── files:write helpers ──────────────────────────────────────────────

/// Write `data` to `path`, creating or truncating the file.
///
/// Requires the `files:write` capability. The capability has no
/// per-path allowlist; see the crate-level
/// [`files:write` sandbox](crate#fileswrite-sandbox) note.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `files:write` not granted.
/// - [`ErrorCode::InvalidArgument`] — `path` is empty.
/// - [`ErrorCode::IoFailed`] — any I/O error, **including parent
///   directory missing**. Unlike the read side, the write helpers do
///   not map a missing parent to [`ErrorCode::NotFound`].
pub fn write_file(path: &str, data: &[u8]) -> Result<(), ErrorCode> {
    host_files::write_file(path, data)
}

/// Write a UTF-8 string to `path`, creating or truncating the file.
///
/// Convenience wrapper over [`write_file`].
///
/// Requires the `files:write` capability. See [`write_file`] for the
/// sandbox note and full error list.
///
/// # Errors
///
/// Same as [`write_file`].
pub fn write_string(path: &str, s: &str) -> Result<(), ErrorCode> {
    host_files::write_file(path, s.as_bytes())
}

/// Append `data` to `path`, creating the file if it does not exist.
///
/// Requires the `files:write` capability. See the crate-level
/// [`files:write` sandbox](crate#fileswrite-sandbox) note.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `files:write` not granted.
/// - [`ErrorCode::InvalidArgument`] — `path` is empty.
/// - [`ErrorCode::IoFailed`] — any I/O error (parent dir missing
///   collapses here, as with [`write_file`]).
pub fn append_file(path: &str, data: &[u8]) -> Result<(), ErrorCode> {
    host_files::append_file(path, data)
}

/// Read this plugin's own settings file
/// (`~/.config/yosh/plugins/<plugin-name>/settings.toml`).
///
/// This is the one capability-free host interface: every plugin may
/// read its own settings without declaring anything in
/// [`Plugin::required_capabilities`]. Returns `Ok(None)` when no
/// settings file exists. The raw TOML text is returned; parsing is
/// the plugin's choice (e.g. the `toml` crate, which compiles to
/// `wasm32-wasip2`).
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — called from `metadata()` (no active
///   shell binding; see the crate-level "metadata is special" note).
/// - [`ErrorCode::IoFailed`] — the file exists but could not be read
///   (permissions, invalid UTF-8).
pub fn read_settings() -> Result<Option<String>, ErrorCode> {
    host_settings::read()
}

/// Create a directory at `path`. Fails if any ancestor is missing.
///
/// For `mkdir -p` semantics, use [`create_dir_all`] instead.
///
/// Requires the `files:write` capability.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `files:write` not granted.
/// - [`ErrorCode::InvalidArgument`] — `path` is empty.
/// - [`ErrorCode::IoFailed`] — any I/O error, including parent
///   directory missing or `path` already existing.
pub fn create_dir(path: &str) -> Result<(), ErrorCode> {
    host_files::create_dir(path, false)
}

/// Create a directory at `path`, including all missing ancestors
/// (`mkdir -p` semantics).
///
/// Requires the `files:write` capability.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `files:write` not granted.
/// - [`ErrorCode::InvalidArgument`] — `path` is empty.
/// - [`ErrorCode::IoFailed`] — any I/O error.
pub fn create_dir_all(path: &str) -> Result<(), ErrorCode> {
    host_files::create_dir(path, true)
}

/// Delete the file at `path`.
///
/// Requires the `files:write` capability. The capability grants
/// destructive access to any path the host process can reach; see
/// the crate-level [`files:write` sandbox](crate#fileswrite-sandbox)
/// note.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `files:write` not granted.
/// - [`ErrorCode::InvalidArgument`] — `path` is empty.
/// - [`ErrorCode::NotFound`] — the file does not exist.
/// - [`ErrorCode::IoFailed`] — any other I/O error (permission denied,
///   is-a-directory, etc.).
pub fn remove_file(path: &str) -> Result<(), ErrorCode> {
    host_files::remove_file(path)
}

/// Remove an empty directory at `path`.
///
/// For recursive removal, use [`remove_dir_all`].
///
/// Requires the `files:write` capability.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `files:write` not granted.
/// - [`ErrorCode::InvalidArgument`] — `path` is empty.
/// - [`ErrorCode::NotFound`] — the directory does not exist.
/// - [`ErrorCode::IoFailed`] — any other I/O error (directory not
///   empty, permission denied, etc.).
pub fn remove_dir(path: &str) -> Result<(), ErrorCode> {
    host_files::remove_dir(path, false)
}

/// Recursively remove a directory and all its contents.
///
/// Requires the `files:write` capability. The capability grants
/// recursive destructive access to any path the host process can
/// reach; see the crate-level
/// [`files:write` sandbox](crate#fileswrite-sandbox) note.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — `files:write` not granted.
/// - [`ErrorCode::InvalidArgument`] — `path` is empty.
/// - [`ErrorCode::NotFound`] — the directory does not exist.
/// - [`ErrorCode::IoFailed`] — any other I/O error.
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
/// process exit. A child terminated by a signal (other than the
/// timeout kill, which is [`ErrorCode::Timeout`]) reports the
/// sentinel exit code `-1`, not `128+N`.
///
/// # Errors
///
/// - [`ErrorCode::Denied`] — the `commands:exec` capability isn't granted.
/// - [`ErrorCode::PatternNotAllowed`] — the argv is not matched by any
///   entry in the plugin's `allowed_commands` allowlist.
/// - [`ErrorCode::Timeout`] — the 1000ms host-enforced cap was hit.
/// - [`ErrorCode::NotFound`] — the OS reported ENOENT for the spawn:
///   PATH lookup failed, an explicit path does not exist, or the
///   script's shebang interpreter is missing.
/// - [`ErrorCode::InvalidArgument`] — `program` is an empty string.
/// - [`ErrorCode::IoFailed`] — the spawn or wait failed for any other
///   reason (e.g. an allowlisted path exists but is not executable).
pub fn exec(program: &str, args: &[&str]) -> Result<ExecOutput, ErrorCode> {
    let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    host_commands::exec(program, &args_owned)
}

/// Run an external command and return its stdout as a `String` plus the
/// exit code. Convenience wrapper over [`exec`], mirroring the
/// [`read_to_string`] / [`read_file`] pairing.
///
/// Stdout is decoded with [`String::from_utf8_lossy`], so invalid UTF-8
/// bytes become U+FFFD instead of failing. Stderr is discarded; use
/// [`exec`] to inspect it or the raw bytes. The exit code carries
/// [`exec`]'s signal-termination sentinel (`-1`) semantics.
///
/// Requires the `commands:exec` capability.
///
/// # Errors
///
/// Same as [`exec`].
pub fn exec_to_string(program: &str, args: &[&str]) -> Result<(String, i32), ErrorCode> {
    let out = exec(program, args)?;
    Ok((
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.exit_code,
    ))
}
