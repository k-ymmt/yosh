//! Capability-aware Linker construction for the wasmtime Component Model
//! plugin runtime.
//!
//! See `docs/superpowers/specs/2026-04-27-wasm-plugin-runtime-design.md` §6.
//!
//! Two import sources:
//!
//! 1. **WASI Preview 2 (sync)** — registered via
//!    `wasmtime_wasi::add_to_linker_sync`. Cargo-component-built plugins
//!    pull in the full WASI surface transitively through the
//!    Preview 1 adapter, so selectively linking only `clocks` + `random`
//!    leaves later imports unsatisfied. Privacy/isolation is enforced by
//!    `HostContext::new_for_plugin` constructing a `WasiCtx` with no
//!    preopens, no stdio, no environment, no args — the plugin can name
//!    `wasi:cli/environment` etc. but every probe returns empty.
//!
//! 2. **`yosh:plugin/{variables,filesystem,files,io,commands}`** — registered with
//!    either the real implementation from `host.rs` or a deny-stub returning
//!    `Err(Denied)` based on the granted-capability bitfield.

use wasmtime::Engine;
use wasmtime::component::Linker;

use yosh_plugin_api::{
    CAP_COMMANDS_EXEC, CAP_FILES_READ, CAP_FILES_WRITE, CAP_FILESYSTEM, CAP_IO, CAP_VARIABLES_READ,
    CAP_VARIABLES_WRITE,
};

use super::host::{
    HostContext, deny_commands_exec, deny_files_append_file, deny_files_create_dir,
    deny_files_metadata, deny_files_read_dir, deny_files_read_file, deny_files_remove_dir,
    deny_files_remove_file, deny_files_write_file, deny_filesystem_cwd, deny_filesystem_set_cwd,
    deny_io_write, deny_variables_export_env, deny_variables_get, deny_variables_set,
    host_commands_exec, host_files_append_file, host_files_create_dir, host_files_metadata,
    host_files_read_dir, host_files_read_file, host_files_remove_dir, host_files_remove_file,
    host_files_write_file, host_filesystem_cwd, host_filesystem_set_cwd, host_io_write,
    host_settings_read, host_variables_export_env, host_variables_get, host_variables_set,
};

#[inline]
fn has(allowed: u32, cap: u32) -> bool {
    allowed & cap != 0
}

/// Construct a linker with the limited WASI surface plus the
/// capability-gated `yosh:plugin/*` host imports.
pub fn build_linker(engine: &Engine, allowed: u32) -> Result<Linker<HostContext>, wasmtime::Error> {
    let mut linker = Linker::<HostContext>::new(engine);

    // ── WASI Preview 2 (sync) ──────────────────────────────────────────
    //
    // Register the full WASI Preview 2 sync linker. Reality check vs the
    // §6 spec: any plugin built via `cargo component build --target
    // wasm32-wasip2` imports the full WASI surface transitively through
    // the Preview 1 adapter, regardless of whether the plugin's Rust
    // source ever uses stdio/cli/filesystem. Selectively linking only
    // `wasi:clocks` + `wasi:random` (the original Task 4 plan) leaves the
    // adapter's other imports unsatisfied, so plugins fail to instantiate.
    //
    // Privacy/isolation is provided by `HostContext::new_for_plugin`
    // building an empty `WasiCtx` (no preopens, no stdin/stdout/stderr,
    // no environment, no args). The plugin can name `wasi:cli/environment`
    // but `get_environment()` returns an empty list. Same story for
    // `wasi:filesystem/preopens` (empty) and `wasi:sockets/*` (no
    // network handle). The capability-gated `yosh:plugin/*` imports
    // remain the privileged surface.
    wasmtime_wasi::add_to_linker_sync(&mut linker)?;

    // ── yosh:plugin/variables ───────────────────────────────────────────
    //
    // Function names follow the WIT (kebab-case in the `func_wrap` path
    // string). The interface path uses the package's full qualified form
    // including the `@0.2.1` version (matching the `package` declaration
    // in the WIT and the bindgen-generated import expectations).
    let mut vars = linker.instance("yosh:plugin/variables@0.2.1")?;
    if has(allowed, CAP_VARIABLES_READ) {
        vars.func_wrap("get", |store, (name,): (wasmtime::component::WasmStr,)| {
            let name_str = name.to_str(&store)?;
            Ok((host_variables_get(store.data(), &name_str),))
        })?;
    } else {
        vars.func_wrap("get", |store, (name,): (wasmtime::component::WasmStr,)| {
            let name_str = name.to_str(&store)?;
            Ok((deny_variables_get(store.data(), &name_str),))
        })?;
    }
    if has(allowed, CAP_VARIABLES_WRITE) {
        vars.func_wrap(
            "set",
            |store, (name, value): (wasmtime::component::WasmStr, wasmtime::component::WasmStr)| {
                let name_str = name.to_str(&store)?;
                let value_str = value.to_str(&store)?;
                Ok((host_variables_set(store.data(), &name_str, &value_str),))
            },
        )?;
        vars.func_wrap(
            "export-env",
            |store, (name, value): (wasmtime::component::WasmStr, wasmtime::component::WasmStr)| {
                let name_str = name.to_str(&store)?;
                let value_str = value.to_str(&store)?;
                Ok((host_variables_export_env(
                    store.data(),
                    &name_str,
                    &value_str,
                ),))
            },
        )?;
    } else {
        vars.func_wrap(
            "set",
            |store, (name, value): (wasmtime::component::WasmStr, wasmtime::component::WasmStr)| {
                let name_str = name.to_str(&store)?;
                let value_str = value.to_str(&store)?;
                Ok((deny_variables_set(store.data(), &name_str, &value_str),))
            },
        )?;
        vars.func_wrap(
            "export-env",
            |store, (name, value): (wasmtime::component::WasmStr, wasmtime::component::WasmStr)| {
                let name_str = name.to_str(&store)?;
                let value_str = value.to_str(&store)?;
                Ok((deny_variables_export_env(
                    store.data(),
                    &name_str,
                    &value_str,
                ),))
            },
        )?;
    }

    // ── yosh:plugin/filesystem ──────────────────────────────────────────
    let mut fs = linker.instance("yosh:plugin/filesystem@0.2.1")?;
    if has(allowed, CAP_FILESYSTEM) {
        fs.func_wrap("cwd", |mut store, (): ()| {
            Ok((host_filesystem_cwd(store.data_mut()),))
        })?;
        fs.func_wrap(
            "set-cwd",
            |store, (path,): (wasmtime::component::WasmStr,)| {
                let path_str = path.to_str(&store)?;
                Ok((host_filesystem_set_cwd(store.data(), &path_str),))
            },
        )?;
    } else {
        fs.func_wrap("cwd", |mut store, (): ()| {
            Ok((deny_filesystem_cwd(store.data_mut()),))
        })?;
        fs.func_wrap(
            "set-cwd",
            |store, (path,): (wasmtime::component::WasmStr,)| {
                let path_str = path.to_str(&store)?;
                Ok((deny_filesystem_set_cwd(store.data(), &path_str),))
            },
        )?;
    }

    // ── yosh:plugin/io ──────────────────────────────────────────────────
    use super::generated::yosh::plugin::types::IoStream;
    let mut io = linker.instance("yosh:plugin/io@0.2.1")?;
    if has(allowed, CAP_IO) {
        io.func_wrap(
            "write",
            |store, (target, data): (IoStream, wasmtime::component::WasmList<u8>)| {
                let bytes = data.as_le_slice(&store);
                Ok((host_io_write(store.data(), target, bytes),))
            },
        )?;
    } else {
        io.func_wrap(
            "write",
            |store, (target, data): (IoStream, wasmtime::component::WasmList<u8>)| {
                let bytes = data.as_le_slice(&store);
                Ok((deny_io_write(store.data(), target, bytes),))
            },
        )?;
    }

    // ── yosh:plugin/files ───────────────────────────────────────────────
    let mut files = linker.instance("yosh:plugin/files@0.2.1")?;

    // Read group — gated by CAP_FILES_READ
    if has(allowed, CAP_FILES_READ) {
        files.func_wrap(
            "read-file",
            |store, (path,): (wasmtime::component::WasmStr,)| {
                let path_str = path.to_str(&store)?;
                Ok((host_files_read_file(store.data(), &path_str),))
            },
        )?;
        files.func_wrap(
            "read-dir",
            |store, (path,): (wasmtime::component::WasmStr,)| {
                let path_str = path.to_str(&store)?;
                Ok((host_files_read_dir(store.data(), &path_str),))
            },
        )?;
        files.func_wrap(
            "metadata",
            |store, (path,): (wasmtime::component::WasmStr,)| {
                let path_str = path.to_str(&store)?;
                Ok((host_files_metadata(store.data(), &path_str),))
            },
        )?;
    } else {
        files.func_wrap(
            "read-file",
            |store, (path,): (wasmtime::component::WasmStr,)| {
                let path_str = path.to_str(&store)?;
                Ok((deny_files_read_file(store.data(), &path_str),))
            },
        )?;
        files.func_wrap(
            "read-dir",
            |store, (path,): (wasmtime::component::WasmStr,)| {
                let path_str = path.to_str(&store)?;
                Ok((deny_files_read_dir(store.data(), &path_str),))
            },
        )?;
        files.func_wrap(
            "metadata",
            |store, (path,): (wasmtime::component::WasmStr,)| {
                let path_str = path.to_str(&store)?;
                Ok((deny_files_metadata(store.data(), &path_str),))
            },
        )?;
    }

    // Write group — gated by CAP_FILES_WRITE
    if has(allowed, CAP_FILES_WRITE) {
        files.func_wrap(
            "write-file",
            |store,
             (path, data): (
                wasmtime::component::WasmStr,
                wasmtime::component::WasmList<u8>,
            )| {
                let path_str = path.to_str(&store)?;
                let bytes = data.as_le_slice(&store);
                Ok((host_files_write_file(store.data(), &path_str, bytes),))
            },
        )?;
        files.func_wrap(
            "append-file",
            |store,
             (path, data): (
                wasmtime::component::WasmStr,
                wasmtime::component::WasmList<u8>,
            )| {
                let path_str = path.to_str(&store)?;
                let bytes = data.as_le_slice(&store);
                Ok((host_files_append_file(store.data(), &path_str, bytes),))
            },
        )?;
        files.func_wrap(
            "create-dir",
            |store, (path, recursive): (wasmtime::component::WasmStr, bool)| {
                let path_str = path.to_str(&store)?;
                Ok((host_files_create_dir(store.data(), &path_str, recursive),))
            },
        )?;
        files.func_wrap(
            "remove-file",
            |store, (path,): (wasmtime::component::WasmStr,)| {
                let path_str = path.to_str(&store)?;
                Ok((host_files_remove_file(store.data(), &path_str),))
            },
        )?;
        files.func_wrap(
            "remove-dir",
            |store, (path, recursive): (wasmtime::component::WasmStr, bool)| {
                let path_str = path.to_str(&store)?;
                Ok((host_files_remove_dir(store.data(), &path_str, recursive),))
            },
        )?;
    } else {
        files.func_wrap(
            "write-file",
            |store,
             (path, data): (
                wasmtime::component::WasmStr,
                wasmtime::component::WasmList<u8>,
            )| {
                let path_str = path.to_str(&store)?;
                let bytes = data.as_le_slice(&store);
                Ok((deny_files_write_file(store.data(), &path_str, bytes),))
            },
        )?;
        files.func_wrap(
            "append-file",
            |store,
             (path, data): (
                wasmtime::component::WasmStr,
                wasmtime::component::WasmList<u8>,
            )| {
                let path_str = path.to_str(&store)?;
                let bytes = data.as_le_slice(&store);
                Ok((deny_files_append_file(store.data(), &path_str, bytes),))
            },
        )?;
        files.func_wrap(
            "create-dir",
            |store, (path, recursive): (wasmtime::component::WasmStr, bool)| {
                let path_str = path.to_str(&store)?;
                Ok((deny_files_create_dir(store.data(), &path_str, recursive),))
            },
        )?;
        files.func_wrap(
            "remove-file",
            |store, (path,): (wasmtime::component::WasmStr,)| {
                let path_str = path.to_str(&store)?;
                Ok((deny_files_remove_file(store.data(), &path_str),))
            },
        )?;
        files.func_wrap(
            "remove-dir",
            |store, (path, recursive): (wasmtime::component::WasmStr, bool)| {
                let path_str = path.to_str(&store)?;
                Ok((deny_files_remove_dir(store.data(), &path_str, recursive),))
            },
        )?;
    }

    // ── yosh:plugin/settings ───────────────────────────────────────────
    //
    // Capability-free: every plugin may read its own settings.toml.
    // The path is fixed per-plugin in `HostContext::settings_path`
    // (resolved at load time from the plugin name), so no broader
    // filesystem access is exposed and no deny stub exists.
    let mut settings = linker.instance("yosh:plugin/settings@0.2.1")?;
    settings.func_wrap("read", |store, (): ()| {
        Ok((host_settings_read(store.data()),))
    })?;

    // ── yosh:plugin/commands ───────────────────────────────────────────
    let mut commands = linker.instance("yosh:plugin/commands@0.2.1")?;
    if has(allowed, CAP_COMMANDS_EXEC) {
        commands.func_wrap(
            "exec",
            |mut store,
             (program, args): (
                wasmtime::component::WasmStr,
                wasmtime::component::WasmList<wasmtime::component::WasmStr>,
            )| {
                // Collect WasmStr items first (they are owned fat-pointers into
                // linear memory with no lifetime param), then drop the mutable
                // borrow of `store` held by the iterator before calling to_str.
                let arg_wstrs: Vec<wasmtime::component::WasmStr> =
                    args.iter(&mut store).collect::<wasmtime::Result<_>>()?;
                // Now store is no longer borrowed mutably; to_str only needs &store.
                let program_str = program.to_str(&store)?;
                let args_strs: Vec<std::borrow::Cow<'_, str>> = arg_wstrs
                    .iter()
                    .map(|w| w.to_str(&store))
                    .collect::<wasmtime::Result<_>>()?;
                Ok((host_commands_exec(store.data(), &program_str, &args_strs),))
            },
        )?;
    } else {
        commands.func_wrap(
            "exec",
            |_store,
             (_program, _args): (
                wasmtime::component::WasmStr,
                wasmtime::component::WasmList<wasmtime::component::WasmStr>,
            )| { Ok((deny_commands_exec(),)) },
        )?;
    }

    Ok(linker)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-only smoke test that locks down the wasmtime-wasi 27.x
    /// `WasiView` / `add_to_linker_get_host` signatures. Failure here on a
    /// future wasmtime upgrade signals that the linker construction needs
    /// re-validation against the new API.
    #[test]
    fn linker_construction_smoke() {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        let engine = Engine::new(&config).expect("engine");
        // Build with no capabilities — exercises the deny path of every
        // host import.
        let _linker = build_linker(&engine, 0).expect("linker construction");
        // Build with all capabilities — exercises the granted path.
        let _linker = build_linker(&engine, yosh_plugin_api::CAP_ALL).expect("linker w/ caps");
    }
}
