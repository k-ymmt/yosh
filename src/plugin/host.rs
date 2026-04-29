//! HostContext + WasiView impl + the real / deny implementations of the
//! `yosh:plugin/*` host imports.
//!
//! See `docs/superpowers/specs/2026-04-27-wasm-plugin-runtime-design.md`
//! §5 "Execution Model" — `HostContext`, `with_env`, and metadata contract
//! enforcement (host imports return `Err(Denied)` whenever the live
//! `ShellEnv` pointer is null, which is exactly the state during the
//! single `metadata()` call at startup).

use std::io::Write;

use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

use crate::env::ShellEnv;

use super::generated::yosh::plugin::types::{ErrorCode, IoStream};
use super::generated::yosh::plugin::commands::ExecOutput;
use super::pattern::CommandPattern;

/// Per-plugin store data. Carries:
///
/// * A raw `*mut ShellEnv` updated by `with_env` immediately before each
///   guest-bound call and reset to `null` on every exit path (see the
///   `EnvGuard` RAII guard in `mod.rs`). NULL during the single
///   `metadata()` call — enforces the metadata contract by short-circuiting
///   every host import to `Err(Denied)`.
/// * Plugin name and granted capability bitfield, used by deny-stubs and
///   diagnostics.
/// * `WasiCtx` + `ResourceTable` for the `wasi:clocks` / `wasi:random`
///   linker subset; the `WasiView` impl below exposes them per-store.
pub struct HostContext {
    /// Raw pointer to the live `ShellEnv`. Confined to a single `unsafe`
    /// helper (`env_mut`); this is the only `unsafe` site in the new host
    /// binding layer (vs. eight `unsafe extern "C" fn` callbacks in the
    /// dlopen version).
    pub(super) env: *mut ShellEnv,
    pub(super) plugin_name: String,
    pub(super) capabilities: u32,

    pub(super) wasi: WasiCtx,
    pub(super) resource_table: ResourceTable,
    pub(super) allowed_commands: Vec<CommandPattern>,
}

// SAFETY: `*mut ShellEnv` is `!Send` by default, but the pointer is only
// ever dereferenced via `with_env` which holds an `&mut ShellEnv` for the
// duration of the call. The shell is single-threaded for plugin dispatch
// (matches dlopen), and the pointer is null when no call is in progress.
unsafe impl Send for HostContext {}
// SAFETY: same rationale; we never share a `&HostContext` across threads
// in practice (per-store, single-threaded shell).
unsafe impl Sync for HostContext {}

impl HostContext {
    pub fn new_for_plugin(plugin_name: impl Into<String>, capabilities: u32) -> Self {
        // wasmtime-wasi 27 builder: defaults are sufficient (clocks use the
        // host clock, random is seeded; stdout/stderr are eaten — plugins
        // do their own host-side I/O via the `yosh:plugin/io` interface).
        let wasi = WasiCtxBuilder::new().build();
        HostContext {
            env: std::ptr::null_mut(),
            plugin_name: plugin_name.into(),
            capabilities,
            wasi,
            resource_table: ResourceTable::new(),
            allowed_commands: Vec::new(),
        }
    }

    /// Borrow the live `ShellEnv` if currently bound. Returns `None` when
    /// `env` is null (during `metadata()` calls or between `with_env`
    /// invocations).
    ///
    /// SAFETY: callers must hold exclusive access to the `Store<HostContext>`,
    /// which is implied by `&mut self` here. The pointer's lifetime is
    /// managed by `EnvGuard` in `mod.rs` and is guaranteed to be valid
    /// for the duration of any `with_env` callback.
    pub(super) fn env_mut(&mut self) -> Option<&mut ShellEnv> {
        if self.env.is_null() {
            None
        } else {
            // SAFETY: `EnvGuard::bind` set this pointer from a live
            // `&mut ShellEnv`; it is reset to null on guard drop. The
            // shell is single-threaded for plugin dispatch.
            Some(unsafe { &mut *self.env })
        }
    }
}

impl WasiView for HostContext {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.resource_table
    }
}

// ── yosh:plugin/variables host imports ──────────────────────────────────

/// `variables.get` — granted: read from `ShellEnv::vars`. Denied: log nothing,
/// return `Err(Denied)` (the WIT error value is the canonical signal).
pub(super) fn host_variables_get(
    ctx: &mut HostContext,
    name: String,
) -> Result<Option<String>, ErrorCode> {
    let Some(env) = ctx.env_mut() else {
        // metadata-contract enforcement OR null between calls.
        return Err(ErrorCode::Denied);
    };
    Ok(env.vars.get(&name).map(|s| s.to_string()))
}

pub(super) fn deny_variables_get(
    _ctx: &mut HostContext,
    _name: String,
) -> Result<Option<String>, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn host_variables_set(
    ctx: &mut HostContext,
    name: String,
    value: String,
) -> Result<(), ErrorCode> {
    let Some(env) = ctx.env_mut() else {
        return Err(ErrorCode::Denied);
    };
    env.vars
        .set(&name, &value)
        .map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn deny_variables_set(
    _ctx: &mut HostContext,
    _name: String,
    _value: String,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

/// `variables.export-env` — name in WIT is `export-env` (because `export`
/// is a reserved WIT keyword); the wit-bindgen-generated Rust function
/// is `export_env`.
pub(super) fn host_variables_export_env(
    ctx: &mut HostContext,
    name: String,
    value: String,
) -> Result<(), ErrorCode> {
    let Some(env) = ctx.env_mut() else {
        return Err(ErrorCode::Denied);
    };
    env.vars
        .set(&name, &value)
        .map_err(|_| ErrorCode::IoFailed)?;
    env.vars.export(&name);
    Ok(())
}

pub(super) fn deny_variables_export_env(
    _ctx: &mut HostContext,
    _name: String,
    _value: String,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

// ── yosh:plugin/filesystem host imports ─────────────────────────────────

pub(super) fn host_filesystem_cwd(ctx: &mut HostContext) -> Result<String, ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn deny_filesystem_cwd(_ctx: &mut HostContext) -> Result<String, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn host_filesystem_set_cwd(
    ctx: &mut HostContext,
    path: String,
) -> Result<(), ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    std::env::set_current_dir(&path).map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn deny_filesystem_set_cwd(
    _ctx: &mut HostContext,
    _path: String,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

// ── yosh:plugin/io host imports ─────────────────────────────────────────

pub(super) fn host_io_write(
    ctx: &mut HostContext,
    target: IoStream,
    data: Vec<u8>,
) -> Result<(), ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    let result = match target {
        IoStream::Stdout => std::io::stdout().write_all(&data),
        IoStream::Stderr => std::io::stderr().write_all(&data),
    };
    result.map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn deny_io_write(
    _ctx: &mut HostContext,
    _target: IoStream,
    _data: Vec<u8>,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

// ── yosh:plugin/files host imports ───────────────────────────────────

use super::generated::yosh::plugin::files::{DirEntry, FileStat};
use std::time::UNIX_EPOCH;

pub(super) fn host_files_read_file(
    ctx: &mut HostContext,
    path: String,
) -> Result<Vec<u8>, ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    match std::fs::read(&path) {
        Ok(bytes) => Ok(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ErrorCode::NotFound),
        Err(_) => Err(ErrorCode::IoFailed),
    }
}

pub(super) fn host_files_read_dir(
    ctx: &mut HostContext,
    path: String,
) -> Result<Vec<DirEntry>, ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let iter = match std::fs::read_dir(&path) {
        Ok(i) => i,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(ErrorCode::NotFound),
        Err(_) => return Err(ErrorCode::IoFailed),
    };
    let mut out = Vec::new();
    for entry in iter {
        let entry = entry.map_err(|_| ErrorCode::IoFailed)?;
        let ft = entry.file_type().map_err(|_| ErrorCode::IoFailed)?;
        out.push(DirEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_file: ft.is_file(),
            is_dir: ft.is_dir(),
            is_symlink: ft.is_symlink(),
        });
    }
    Ok(out)
}

pub(super) fn host_files_metadata(
    ctx: &mut HostContext,
    path: String,
) -> Result<FileStat, ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let md = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(ErrorCode::NotFound),
        Err(_) => return Err(ErrorCode::IoFailed),
    };
    let mtime_secs = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(-1);
    Ok(FileStat {
        is_file: md.is_file(),
        is_dir: md.is_dir(),
        is_symlink: md.file_type().is_symlink(),
        size: md.len(),
        mtime_secs,
    })
}

pub(super) fn host_files_write_file(
    ctx: &mut HostContext,
    path: String,
    data: Vec<u8>,
) -> Result<(), ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    std::fs::write(&path, &data).map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn host_files_append_file(
    ctx: &mut HostContext,
    path: String,
    data: Vec<u8>,
) -> Result<(), ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|_| ErrorCode::IoFailed)?;
    f.write_all(&data).map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn host_files_create_dir(
    ctx: &mut HostContext,
    path: String,
    recursive: bool,
) -> Result<(), ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let result = if recursive {
        std::fs::create_dir_all(&path)
    } else {
        std::fs::create_dir(&path)
    };
    result.map_err(|_| ErrorCode::IoFailed)
}

pub(super) fn host_files_remove_file(
    ctx: &mut HostContext,
    path: String,
) -> Result<(), ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ErrorCode::NotFound),
        Err(_) => Err(ErrorCode::IoFailed),
    }
}

pub(super) fn host_files_remove_dir(
    ctx: &mut HostContext,
    path: String,
    recursive: bool,
) -> Result<(), ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let result = if recursive {
        std::fs::remove_dir_all(&path)
    } else {
        std::fs::remove_dir(&path)
    };
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ErrorCode::NotFound),
        Err(_) => Err(ErrorCode::IoFailed),
    }
}

pub(super) fn deny_files_read_file(
    _ctx: &mut HostContext,
    _path: String,
) -> Result<Vec<u8>, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_read_dir(
    _ctx: &mut HostContext,
    _path: String,
) -> Result<Vec<DirEntry>, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_metadata(
    _ctx: &mut HostContext,
    _path: String,
) -> Result<FileStat, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_write_file(
    _ctx: &mut HostContext,
    _path: String,
    _data: Vec<u8>,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_append_file(
    _ctx: &mut HostContext,
    _path: String,
    _data: Vec<u8>,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_create_dir(
    _ctx: &mut HostContext,
    _path: String,
    _recursive: bool,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_remove_file(
    _ctx: &mut HostContext,
    _path: String,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub(super) fn deny_files_remove_dir(
    _ctx: &mut HostContext,
    _path: String,
    _recursive: bool,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

// ── yosh:plugin/commands host imports ───────────────────────────────

pub(super) fn host_commands_exec(
    ctx: &mut HostContext,
    program: String,
    args: Vec<String>,
) -> Result<ExecOutput, ErrorCode> {
    if ctx.env_mut().is_none() {
        return Err(ErrorCode::Denied);
    }
    if program.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }

    // argv = [program, args...]; pattern matcher consumes the literal
    // strings (no PATH resolution, no basename normalization — see
    // spec §5).
    let mut argv = Vec::with_capacity(1 + args.len());
    argv.push(program.clone());
    argv.extend(args.iter().cloned());

    if !ctx.allowed_commands.iter().any(|p| p.matches(&argv)) {
        return Err(ErrorCode::PatternNotAllowed);
    }

    spawn_with_timeout(&program, &args, std::time::Duration::from_millis(1000))
}

pub(super) fn deny_commands_exec(
    _ctx: &mut HostContext,
    _program: String,
    _args: Vec<String>,
) -> Result<ExecOutput, ErrorCode> {
    Err(ErrorCode::Denied)
}

fn spawn_with_timeout(
    program: &str,
    args: &[String],
    timeout: std::time::Duration,
) -> Result<ExecOutput, ErrorCode> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Instant;

    let mut child = match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(ErrorCode::NotFound),
        Err(_) => return Err(ErrorCode::IoFailed),
    };

    // Drain stdout and stderr concurrently so a buffer-full child does
    // not deadlock waiting on us. Each thread reads to EOF, which only
    // happens after the child exits or its pipe is closed.
    let mut stdout_pipe = child.stdout.take().expect("piped stdout");
    let mut stderr_pipe = child.stderr.take().expect("piped stderr");
    let (out_tx, out_rx) = mpsc::channel::<std::io::Result<Vec<u8>>>();
    let (err_tx, err_rx) = mpsc::channel::<std::io::Result<Vec<u8>>>();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let r = stdout_pipe.read_to_end(&mut buf).map(|_| buf);
        let _ = out_tx.send(r);
    });
    thread::spawn(move || {
        let mut buf = Vec::new();
        let r = stderr_pipe.read_to_end(&mut buf).map(|_| buf);
        let _ = err_tx.send(r);
    });

    let deadline = Instant::now() + timeout;
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(_) => return Err(ErrorCode::IoFailed),
        }
        if Instant::now() >= deadline {
            // Timeout: SIGTERM, 100ms grace, then SIGKILL.
            let pid = nix::unistd::Pid::from_raw(child.id() as i32);
            let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM);
            let grace = Instant::now() + std::time::Duration::from_millis(100);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    _ => {}
                }
                if Instant::now() >= grace {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                thread::sleep(std::time::Duration::from_millis(10));
            }
            // Drain pipes: the child is dead (SIGKILL + wait), so the
            // pipe fds are closed and the reader threads will EOF and
            // terminate. Blocking recv() is safe here — it cannot hang.
            let _ = out_rx.recv();
            let _ = err_rx.recv();
            return Err(ErrorCode::Timeout);
        }
        thread::sleep(std::time::Duration::from_millis(10));
    };

    // The child has exited (try_wait returned Some(_)), so the pipe fds
    // are closed and the reader threads are guaranteed to terminate.
    // Blocking recv() is safe — it cannot hang.
    let stdout = out_rx.recv().ok().and_then(|r| r.ok()).unwrap_or_default();
    let stderr = err_rx.recv().ok().and_then(|r| r.ok()).unwrap_or_default();

    Ok(ExecOutput {
        exit_code: exit_status.code().unwrap_or(-1),
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    //! Unit tests for the metadata contract: every host import must
    //! short-circuit to `Err(Denied)` when `HostContext.env` is null. This
    //! is the canonical enforcement point for the §5 metadata-cannot-reach-
    //! host-APIs invariant. The pointer is null during the single
    //! `metadata()` call at startup and between `with_env` invocations, so
    //! returning `Denied` from these functions blocks any plugin that tries
    //! to call them outside of a properly-bound dispatch.
    //!
    //! Replaces what would have been `tests/plugin.rs::t04_metadata_cannot_
    //! reach_host_apis` — a contrived plugin whose `metadata` calls `cwd()`
    //! is harder to author than this direct call. Same invariant, simpler
    //! test.
    use super::*;
    use tempfile::tempdir;
    use yosh_plugin_api::CAP_ALL;

    fn null_env_ctx() -> HostContext {
        // Capabilities are deliberately CAP_ALL — the deny short-circuit
        // we are testing fires regardless of granted capabilities, because
        // it is enforced inside the *real* implementations. The deny stubs
        // would also return `Denied` but for a different reason.
        HostContext::new_for_plugin("<test>", CAP_ALL)
    }

    /// Counterpart to `null_env_ctx` for happy-path tests: binds a real
    /// `ShellEnv` so `env_mut()` returns `Some(_)` and the real impls
    /// proceed past the metadata-contract guard. Real impls only branch
    /// on `is_none()` — they never read through the pointer — so the
    /// concrete shell state is irrelevant.
    fn bound_env_ctx(env: &mut ShellEnv) -> HostContext {
        let mut ctx = HostContext::new_for_plugin("<test>", CAP_ALL);
        ctx.env = env as *mut ShellEnv;
        ctx
    }

    #[test]
    fn metadata_contract_real_cwd_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_filesystem_cwd(&mut ctx);
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_set_cwd_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_filesystem_set_cwd(&mut ctx, "/tmp".into());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_io_write_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_io_write(&mut ctx, IoStream::Stdout, b"hi".to_vec());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_variables_get_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_variables_get(&mut ctx, "PATH".into());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_variables_set_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_variables_set(&mut ctx, "FOO".into(), "bar".into());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_variables_export_env_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_variables_export_env(&mut ctx, "FOO".into(), "bar".into());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_files_read_file_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_files_read_file(&mut ctx, "/tmp/anything".into());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_files_read_dir_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_files_read_dir(&mut ctx, "/tmp".into());
        // `Vec<DirEntry>` doesn't impl `PartialEq` (bindgen-generated), so
        // match on the error variant directly.
        assert!(matches!(result, Err(ErrorCode::Denied)));
    }

    #[test]
    fn metadata_contract_real_files_metadata_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_files_metadata(&mut ctx, "/tmp".into());
        // `FileStat` doesn't impl `PartialEq` (bindgen-generated), so match
        // on the error variant directly.
        assert!(matches!(result, Err(ErrorCode::Denied)));
    }

    #[test]
    fn metadata_contract_real_files_write_file_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_files_write_file(&mut ctx, "/tmp/x".into(), b"hi".to_vec());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_files_append_file_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_files_append_file(&mut ctx, "/tmp/x".into(), b"hi".to_vec());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_files_create_dir_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_files_create_dir(&mut ctx, "/tmp/newdir".into(), true);
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_files_remove_file_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_files_remove_file(&mut ctx, "/tmp/x".into());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn metadata_contract_real_files_remove_dir_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_files_remove_dir(&mut ctx, "/tmp/newdir".into(), true);
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    // ── Spec §8 host happy-path / error-mapping tests ───────────────────
    // Backfill of the 9 tests prescribed by
    // docs/superpowers/specs/2026-04-29-plugin-files-rw-capability-design.md
    // §8 that the original implementation plan omitted.

    #[test]
    fn host_files_read_file_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("data.txt");
        let payload = b"hello world".to_vec();
        std::fs::write(&path, &payload).unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        let result = host_files_read_file(&mut ctx, path.to_string_lossy().into_owned());
        assert_eq!(result, Ok(payload));
    }

    #[test]
    fn host_files_read_dir_returns_entries() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        let entries =
            host_files_read_dir(&mut ctx, dir.path().to_string_lossy().into_owned()).unwrap();

        assert_eq!(entries.len(), 2);
        let a = entries.iter().find(|e| e.name == "a.txt").expect("a.txt");
        assert!(a.is_file);
        assert!(!a.is_dir);
        let sub = entries.iter().find(|e| e.name == "sub").expect("sub");
        assert!(!sub.is_file);
        assert!(sub.is_dir);
    }

    #[test]
    fn host_files_metadata_distinguishes_file_and_dir() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("f");
        std::fs::write(&file_path, b"abc").unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);

        let f = host_files_metadata(&mut ctx, file_path.to_string_lossy().into_owned()).unwrap();
        assert!(f.is_file);
        assert!(!f.is_dir);
        assert_eq!(f.size, 3);

        let d = host_files_metadata(&mut ctx, dir.path().to_string_lossy().into_owned()).unwrap();
        assert!(!d.is_file);
        assert!(d.is_dir);
    }

    #[test]
    fn host_files_read_file_returns_not_found_for_missing_path() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("does-not-exist.txt");

        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        let result = host_files_read_file(&mut ctx, missing.to_string_lossy().into_owned());
        assert_eq!(result, Err(ErrorCode::NotFound));
    }

    #[test]
    fn host_files_read_file_invalid_argument_on_empty_path() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        let result = host_files_read_file(&mut ctx, String::new());
        assert_eq!(result, Err(ErrorCode::InvalidArgument));
    }

    #[test]
    fn host_files_remove_dir_io_failed_on_nonempty_without_recursive() {
        let dir = tempdir().unwrap();
        let inner = dir.path().join("d");
        std::fs::create_dir(&inner).unwrap();
        std::fs::write(inner.join("f"), b"x").unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        let result = host_files_remove_dir(&mut ctx, inner.to_string_lossy().into_owned(), false);
        assert_eq!(result, Err(ErrorCode::IoFailed));
        assert!(inner.exists());
    }

    #[test]
    fn host_files_append_file_appends() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("log");

        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        let p = path.to_string_lossy().into_owned();

        host_files_write_file(&mut ctx, p.clone(), b"hello".to_vec()).unwrap();
        host_files_append_file(&mut ctx, p, b" world".to_vec()).unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes, b"hello world");
    }

    #[test]
    fn host_files_create_dir_all_creates_intermediate_dirs() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("a/b/c");

        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        host_files_create_dir(&mut ctx, nested.to_string_lossy().into_owned(), true).unwrap();

        assert!(nested.is_dir());
        assert!(dir.path().join("a").is_dir());
        assert!(dir.path().join("a/b").is_dir());
    }

    #[test]
    fn host_files_remove_dir_recursive_removes_subtree() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("tree");
        std::fs::create_dir_all(root.join("inner")).unwrap();
        std::fs::write(root.join("f"), b"x").unwrap();
        std::fs::write(root.join("inner/g"), b"y").unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        host_files_remove_dir(&mut ctx, root.to_string_lossy().into_owned(), true).unwrap();

        assert!(!root.exists());
    }

    // ── commands:exec host tests (spec §10) ─────────────────────────────

    fn ctx_with_allowed(env: &mut ShellEnv, patterns: &[&str]) -> HostContext {
        let mut ctx = bound_env_ctx(env);
        ctx.allowed_commands = patterns
            .iter()
            .map(|s| super::super::pattern::CommandPattern::parse(s).expect("valid pattern"))
            .collect();
        ctx
    }

    #[test]
    fn host_commands_exec_metadata_contract_denied_when_env_null() {
        let mut ctx = null_env_ctx();
        let result = host_commands_exec(&mut ctx, "/bin/echo".into(), vec!["hi".into()]);
        assert!(matches!(result, Err(ErrorCode::Denied)));
    }

    #[test]
    fn host_commands_exec_invalid_argument_on_empty_program() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = bound_env_ctx(&mut env);
        let result = host_commands_exec(&mut ctx, String::new(), vec![]);
        assert!(matches!(result, Err(ErrorCode::InvalidArgument)));
    }

    #[test]
    fn host_commands_exec_pattern_not_allowed_when_no_match() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = ctx_with_allowed(&mut env, &["ls:*"]);
        let result = host_commands_exec(&mut ctx, "echo".into(), vec!["hi".into()]);
        assert!(matches!(result, Err(ErrorCode::PatternNotAllowed)));
    }

    #[test]
    fn host_commands_exec_runs_when_pattern_matches() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = ctx_with_allowed(&mut env, &["/bin/echo:*"]);
        let result = host_commands_exec(
            &mut ctx,
            "/bin/echo".into(),
            vec!["hello".into()],
        )
        .expect("echo should succeed");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, b"hello\n");
        assert!(result.stderr.is_empty());
    }

    #[test]
    fn host_commands_exec_captures_stderr_separately() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = ctx_with_allowed(&mut env, &["/bin/sh:*"]);
        let result = host_commands_exec(
            &mut ctx,
            "/bin/sh".into(),
            vec!["-c".into(), "echo out; echo err 1>&2".into()],
        )
        .expect("sh should succeed");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, b"out\n");
        assert_eq!(result.stderr, b"err\n");
    }

    #[test]
    fn host_commands_exec_propagates_nonzero_exit() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = ctx_with_allowed(&mut env, &["/bin/sh:*"]);
        let result = host_commands_exec(
            &mut ctx,
            "/bin/sh".into(),
            vec!["-c".into(), "exit 42".into()],
        )
        .expect("sh should run to exit");
        assert_eq!(result.exit_code, 42);
    }

    #[test]
    fn host_commands_exec_returns_not_found_for_missing_binary() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = ctx_with_allowed(&mut env, &["/no/such/binary-xyz:*"]);
        let result = host_commands_exec(
            &mut ctx,
            "/no/such/binary-xyz".into(),
            vec![],
        );
        assert!(matches!(result, Err(ErrorCode::NotFound)));
    }

    #[test]
    fn host_commands_exec_timeout_after_1000ms() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = ctx_with_allowed(&mut env, &["/bin/sleep:*"]);
        let start = std::time::Instant::now();
        let result = host_commands_exec(
            &mut ctx,
            "/bin/sleep".into(),
            vec!["5".into()],
        );
        let elapsed = start.elapsed();
        assert!(matches!(result, Err(ErrorCode::Timeout)));
        // Hard cap is 1000ms + 100ms grace + a generous slack for thread
        // scheduling. Anything past 2 seconds means the timeout enforcement
        // is broken, not just slow.
        assert!(
            elapsed < std::time::Duration::from_millis(2000),
            "timeout took {:?}, expected <2000ms",
            elapsed
        );
    }

    #[test]
    fn host_commands_exec_kills_child_on_timeout() {
        // Spec §10: after a timeout-triggered call returns, the child must
        // be reaped (no zombie). spawn_with_timeout calls `child.wait()`
        // after SIGKILL, so a successful return implies the child PID has
        // been reaped. The test verifies (a) the function returns within
        // a bounded window — meaning child.wait() did NOT block forever
        // waiting for a still-running child — and (b) the elapsed time
        // covers SIGTERM + 100ms grace + SIGKILL + reaping. If any step
        // were broken, this assertion would fail with either a hang or
        // a too-fast / too-slow elapsed time.
        let mut env = ShellEnv::new("yosh", vec![]);
        let mut ctx = ctx_with_allowed(&mut env, &["/bin/sleep:*"]);
        let start = std::time::Instant::now();
        let result = host_commands_exec(
            &mut ctx,
            "/bin/sleep".into(),
            vec!["5".into()],
        );
        let elapsed = start.elapsed();
        assert!(matches!(result, Err(ErrorCode::Timeout)));
        // Lower bound: SIGTERM only fires after the 1000ms deadline.
        assert!(
            elapsed >= std::time::Duration::from_millis(900),
            "elapsed {:?} too small — timeout fired before deadline",
            elapsed
        );
        // Upper bound: deadline + grace + reasonable scheduling slack.
        // If child.wait() blocked indefinitely waiting on an unkilled
        // child, this would hang past 2000ms.
        assert!(
            elapsed < std::time::Duration::from_millis(2000),
            "elapsed {:?} too large — child may not have been reaped",
            elapsed
        );
    }
}
