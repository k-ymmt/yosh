# yosh-plugin-sdk Public API Doc Comments — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add rustdoc to every public function and the `Plugin` trait in `crates/yosh-plugin-sdk/src/lib.rs`, surfacing three documented hazards (`exists` ambiguity, `read_to_string` UTF-8 collapse, `files:write` sandbox surface).

**Architecture:** Pure documentation patch. Touches only `crates/yosh-plugin-sdk/src/lib.rs`. Each task adds docs to one logical group of functions, verifies `cargo build` produces no warnings and `cargo doc` builds, then commits. No code-level changes; the existing `exec()` doc (lines 164–180 pre-edit) is the format reference.

**Tech Stack:** Rust rustdoc, cargo, cargo doc.

**Spec:** `docs/superpowers/specs/2026-05-03-plugin-sdk-doc-comments-design.md`

---

## File Structure

Single file modified: `crates/yosh-plugin-sdk/src/lib.rs` (184 lines pre-edit, ~+200 lines post-edit).

No new files, no Cargo.toml changes, no other crates touched.

---

## Task 1: Module-level docs + `Plugin` trait

**Files:**
- Modify: `crates/yosh-plugin-sdk/src/lib.rs:1-4` (module-level rustdoc)
- Modify: `crates/yosh-plugin-sdk/src/lib.rs:43-69` (`Plugin` trait)

- [ ] **Step 1: Read current lib.rs**

Run: `cat crates/yosh-plugin-sdk/src/lib.rs | head -70`
Confirm the module-level doc is at lines 1–4 and the `Plugin` trait is at lines 43–69.

- [ ] **Step 2: Replace the module-level doc block**

Replace lines 1–4:

```rust
//! yosh-plugin-sdk — Rust SDK for authoring yosh plugins.
//!
//! Plugins implement the [`Plugin`] trait and invoke [`export!`] to wire
//! the trait into the WIT-generated guest bindings.
```

With:

```rust
//! yosh-plugin-sdk — Rust SDK for authoring yosh plugins.
//!
//! Plugins implement the [`Plugin`] trait and invoke [`export!`] to wire
//! the trait into the WIT-generated guest bindings.
//!
//! # Capabilities
//!
//! Every host import (variables, filesystem, io, files, commands) is
//! gated by a capability declared in [`Plugin::required_capabilities`].
//! Without the matching capability, the corresponding helper returns
//! [`ErrorCode::Denied`].
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
```

- [ ] **Step 3: Replace the `Plugin` trait block**

Replace lines 43–69:

```rust
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
```

With:

```rust
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

    /// Fires before each external command runs.
    ///
    /// `command` is the command line as the user typed it.
    fn hook_pre_exec(&mut self, _command: &str) {}

    /// Fires after each external command exits.
    ///
    /// `exit_code` is the child's exit status.
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
```

- [ ] **Step 4: Verify build and doc generation**

Run: `cargo build -p yosh-plugin-sdk 2>&1 | tail -5`
Expected: clean build, no warnings.

Run: `cargo doc -p yosh-plugin-sdk --no-deps 2>&1 | tail -5`
Expected: doc generation succeeds without warnings.

If `cargo doc` flags broken intra-doc links (e.g., `[`cwd`]`/`[`get_var`]`), they are forward references to functions added in later tasks. Acceptable: `cargo doc` emits warnings but exits 0. Re-verify after Task 4.

- [ ] **Step 5: Commit**

```bash
git add crates/yosh-plugin-sdk/src/lib.rs
git commit -m "$(cat <<'EOF'
docs(plugin-sdk): document Plugin trait and module-level surface

Adds rustdoc to the crate-level docs (capability gating, metadata
restriction, files:write sandbox warning) and to every Plugin trait
method.

Spec: docs/superpowers/specs/2026-05-03-plugin-sdk-doc-comments-design.md
EOF
)"
```

---

## Task 2: `variables` + `filesystem` + `io` groups

**Files:**
- Modify: `crates/yosh-plugin-sdk/src/lib.rs` (variables block ~line 73, filesystem block ~line 85, io block ~line 93)

These three groups share a uniform shape (no hazards, just capability + errors) so are batched together.

- [ ] **Step 1: Replace the `variables` block**

Find:

```rust
pub fn get_var(name: &str) -> Result<Option<String>, ErrorCode> {
    host_variables::get(name)
}

pub fn set_var(name: &str, value: &str) -> Result<(), ErrorCode> {
    host_variables::set(name, value)
}

pub fn export_var(name: &str, value: &str) -> Result<(), ErrorCode> {
    host_variables::export_env(name, value)
}
```

Replace with:

```rust
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
/// Equivalent to `name=value` at the shell prompt. The variable is
/// not exported to spawned-child environments; use [`export_var`]
/// for that.
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
```

- [ ] **Step 2: Replace the `filesystem` block**

Find:

```rust
pub fn cwd() -> Result<String, ErrorCode> {
    host_filesystem::cwd()
}

pub fn set_cwd(path: &str) -> Result<(), ErrorCode> {
    host_filesystem::set_cwd(path)
}
```

Replace with:

```rust
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
```

- [ ] **Step 3: Replace the `io` block**

Find:

```rust
pub fn print(s: &str) -> Result<(), ErrorCode> {
    host_io::write(IoStream::Stdout, s.as_bytes())
}

pub fn eprint(s: &str) -> Result<(), ErrorCode> {
    host_io::write(IoStream::Stderr, s.as_bytes())
}

pub fn write_bytes(stream: IoStream, data: &[u8]) -> Result<(), ErrorCode> {
    host_io::write(stream, data)
}
```

Replace with:

```rust
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
/// UTF-8 strings, prefer [`print`] / [`eprint`].
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
```

- [ ] **Step 4: Verify build and doc generation**

Run: `cargo build -p yosh-plugin-sdk 2>&1 | tail -5`
Expected: clean, no warnings.

Run: `cargo doc -p yosh-plugin-sdk --no-deps 2>&1 | tail -5`
Expected: builds. Forward-reference warnings to functions added in later tasks (`[print]`, `[eprint]`, `[write_bytes]`) are acceptable here.

- [ ] **Step 5: Commit**

```bash
git add crates/yosh-plugin-sdk/src/lib.rs
git commit -m "$(cat <<'EOF'
docs(plugin-sdk): document variables/filesystem/io helpers

Adds # Errors sections and capability requirements to get_var/set_var/
export_var, cwd/set_cwd, print/eprint/write_bytes.

Spec: docs/superpowers/specs/2026-05-03-plugin-sdk-doc-comments-design.md
EOF
)"
```

---

## Task 3: `files:read` group (with `exists` and `read_to_string` hazards)

**Files:**
- Modify: `crates/yosh-plugin-sdk/src/lib.rs` (files:read block, ~lines 105-126 pre-edit)

- [ ] **Step 1: Replace the `files:read` block**

Find:

```rust
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
```

Replace with:

```rust
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
```

- [ ] **Step 2: Verify build and doc generation**

Run: `cargo build -p yosh-plugin-sdk 2>&1 | tail -5`
Expected: clean, no warnings.

Run: `cargo doc -p yosh-plugin-sdk --no-deps 2>&1 | tail -5`
Expected: builds. Intra-doc links `[FileStat::is_symlink]`, `[DirEntry::is_symlink]`, `[String::from_utf8]` should resolve.

- [ ] **Step 3: Commit**

```bash
git add crates/yosh-plugin-sdk/src/lib.rs
git commit -m "$(cat <<'EOF'
docs(plugin-sdk): document files:read helpers and surface 2 hazards

Adds # Errors sections to read_file/read_to_string/read_dir/metadata/
exists. Surfaces two non-obvious behaviors:

(1) read_to_string collapses non-UTF-8 content onto InvalidArgument,
    the same code the host uses for empty-path errors — directs
    callers to read_file + manual decode for binary files.

(2) exists() returns false for "missing", "Denied", and any I/O error
    indistinguishably — directs callers to metadata() when they need
    to tell those apart.

Also notes that metadata().is_symlink is effectively always false (host
follows symlinks); read_dir DirEntry is the symlink-detection path.

Spec: docs/superpowers/specs/2026-05-03-plugin-sdk-doc-comments-design.md
EOF
)"
```

---

## Task 4: `files:write` group (with sandbox notes)

**Files:**
- Modify: `crates/yosh-plugin-sdk/src/lib.rs` (files:write block, ~lines 128-160 pre-edit)

The sandbox warning is repeated inline on every helper. The crate-level docs (Task 1) cover the same warning once at the top of the page; per-function repetition is intentional so authors landing on a single fn page see the warning without navigating away.

- [ ] **Step 1: Replace the `files:write` block**

Find:

```rust
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
```

Replace with:

```rust
// ── files:write helpers ──────────────────────────────────────────────

/// Write `data` to `path`, creating or truncating the file.
///
/// Requires the `files:write` capability. The capability has no
/// per-path allowlist; see the crate-level "files:write sandbox" note.
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
/// "files:write sandbox" note.
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
/// the crate-level "files:write sandbox" note.
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
/// reach; see the crate-level "files:write sandbox" note.
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
```

- [ ] **Step 2: Verify build, lints, and doc generation**

Run: `cargo build -p yosh-plugin-sdk 2>&1 | tail -5`
Expected: clean, no warnings.

Run: `cargo doc -p yosh-plugin-sdk --no-deps 2>&1 | tail -10`
Expected: clean. All forward intra-doc references introduced in earlier tasks should now resolve.

Run: `cargo clippy -p yosh-plugin-sdk --all-targets -- -D warnings 2>&1 | tail -10`
Expected: no warnings.

- [ ] **Step 3: Commit**

```bash
git add crates/yosh-plugin-sdk/src/lib.rs
git commit -m "$(cat <<'EOF'
docs(plugin-sdk): document files:write helpers and surface sandbox

Adds # Errors sections to write_file/write_string/append_file/
create_dir/create_dir_all/remove_file/remove_dir/remove_dir_all.

Each helper carries an inline sandbox note: files:write has no
per-path allowlist beyond the capability grant itself, so the grant
is the entire access boundary. Also notes the read/write asymmetry
on parent-dir-missing (collapses to IoFailed on write side, distinct
NotFound on read side).

Spec: docs/superpowers/specs/2026-05-03-plugin-sdk-doc-comments-design.md
EOF
)"
```

---

## Task 5: Final verification across the workspace

**Files:** none modified; verification only.

- [ ] **Step 1: Run the full SDK build with warnings as errors**

Run: `cargo build -p yosh-plugin-sdk 2>&1 | tail -10`
Expected: `Finished ... profile [...] target(s) in Xs` and no warning lines.

- [ ] **Step 2: Run rustdoc with broken-link detection**

Run: `RUSTDOCFLAGS="-D rustdoc::broken-intra-doc-links" cargo doc -p yosh-plugin-sdk --no-deps 2>&1 | tail -20`
Expected: exits 0. Any broken intra-doc link surfaces as an error and must be fixed before commit.

If a broken link is reported (e.g., `[`Plugin`]` cannot resolve from a free function in the crate root), prefer the fully-qualified form `[`crate::Plugin`]`.

- [ ] **Step 3: Run `cargo fmt` check on the touched file**

Run: `rustfmt --edition 2024 --check crates/yosh-plugin-sdk/src/lib.rs`
Expected: exits 0.

(Per `TODO.md`, `cargo fmt --check -- <path>` mis-detects edition; use direct `rustfmt --edition 2024` instead.)

- [ ] **Step 4: Run the full workspace test suite (background, expected ~6-7 minutes)**

Run: `cargo test 2>&1 | tail -30`
Expected: all tests pass. No new failures vs. main.

This is a docs-only change so test results should be identical to pre-change. The check confirms no accidental code edits crept into the file.

- [ ] **Step 5: Spot-check rendered docs**

Run: `cargo doc -p yosh-plugin-sdk --no-deps --open` (if a browser is available; otherwise inspect `target/doc/yosh_plugin_sdk/index.html` directly).

Visually verify:
- Crate-level page shows the three sections: "Capabilities", "`metadata` is special", "`files:write` sandbox".
- `Plugin` trait page shows method-level docs.
- `exists()` page shows the **Hazard** note prominently.
- `read_to_string()` page shows the binary-file warning.
- Each `files:write` helper page shows a sandbox reference.

- [ ] **Step 6: No commit needed**

This task is verification only. If steps 1–5 all pass, the work is complete.

---

## Self-Review (run by the plan author after writing, before handoff)

**Spec coverage:**
- [x] Module-level capability and metadata-restriction docs → Task 1
- [x] `Plugin` trait method docs → Task 1
- [x] `variables` group helpers → Task 2
- [x] `filesystem` group helpers → Task 2
- [x] `io` group helpers → Task 2
- [x] `files:read` group helpers → Task 3
- [x] `exists` hazard (a) → Task 3
- [x] `read_to_string` hazard (b) → Task 3
- [x] `metadata().is_symlink` symlink note → Task 3
- [x] `files:write` group helpers → Task 4
- [x] `files:write` sandbox note (c) → Task 1 (crate-level) + Task 4 (per-fn references)
- [x] Read/write asymmetry on parent-dir-missing → Task 4
- [x] Verification: cargo build / cargo doc / cargo clippy / cargo test → Task 5

**Placeholder scan:**
- No "TBD" / "TODO" / "implement later" inside any task step.
- All code blocks contain the literal text to write.
- All commands have expected output.

**Type consistency:**
- Function signatures in plan match `crates/yosh-plugin-sdk/src/lib.rs` exactly (no renaming, no signature changes).
- Capability strings used: `variables:read`, `variables:write`, `filesystem`, `io`, `files:read`, `files:write` — verified against `crates/yosh-plugin-api/src/lib.rs:67-77`.
- `ErrorCode` variants used: `Denied`, `IoFailed`, `InvalidArgument`, `NotFound` — verified against `crates/yosh-plugin-api/wit/yosh-plugin.wit:4-12`.
