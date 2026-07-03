//! `yosh:plugin/files` host imports — read/write/inspect the
//! filesystem. Granted via CAP_FILES_READ / CAP_FILES_WRITE. None of
//! these functions read or write `ShellEnv`, so the metadata guard
//! delegates to `ensure_bound`.
//!
//! Capability model: by default (`files_root` unset in `plugins.toml`)
//! the `files` capability is a **full-filesystem grant** — every path
//! the shell user can reach is reachable by the plugin. Setting
//! `files_root = "<dir>"` on the plugin entry confines all eight
//! functions to that directory: paths are canonicalized (resolving
//! symlinks) and must stay inside the root, otherwise `Denied` is
//! returned. Relative paths resolve against the root, not the process
//! cwd. This mirrors the sandbox-mode semantics of
//! `yosh-plugin-manager`'s test host.
//!
//! Error mapping table (see spec
//! docs/superpowers/specs/2026-04-29-plugin-files-rw-capability-design.md
//! §4):
//! - empty path                 → InvalidArgument
//! - escape from `files_root`   → Denied
//! - std::io::ErrorKind::NotFound (read side) → NotFound
//! - other I/O errors           → IoFailed

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::super::generated::yosh::plugin::files::{DirEntry, FileStat};
use super::super::generated::yosh::plugin::types::ErrorCode;
use super::HostContext;

/// Resolve `path` against the plugin's `files_root`, if configured.
/// Unconfined plugins get the path back verbatim (full-FS grant, see
/// module docs). Confined plugins get the canonicalized real path, or
/// `Denied` if it escapes the root. Canonicalization resolves symlinks,
/// so a link inside the root pointing outside is also denied. For paths
/// that do not exist yet (write/create), the parent is canonicalized
/// and the final component re-joined.
fn resolve(ctx: &HostContext, path: &str) -> Result<PathBuf, ErrorCode> {
    let Some(root) = &ctx.files_root else {
        return Ok(PathBuf::from(path));
    };
    let candidate = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        root.join(path)
    };
    let canon = match std::fs::canonicalize(&candidate) {
        Ok(p) => p,
        Err(_) => {
            let parent = candidate.parent().ok_or(ErrorCode::Denied)?;
            let parent_canon = std::fs::canonicalize(parent).map_err(|_| ErrorCode::Denied)?;
            let file_name = candidate.file_name().ok_or(ErrorCode::Denied)?;
            parent_canon.join(file_name)
        }
    };
    if canon.starts_with(root) {
        Ok(canon)
    } else {
        Err(ErrorCode::Denied)
    }
}

pub fn host_files_read_file(ctx: &HostContext, path: &str) -> Result<Vec<u8>, ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let path = resolve(ctx, path)?;
    match std::fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ErrorCode::NotFound),
        Err(_) => Err(ErrorCode::IoFailed),
    }
}

pub fn host_files_read_dir(ctx: &HostContext, path: &str) -> Result<Vec<DirEntry>, ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let path = resolve(ctx, path)?;
    let iter = match std::fs::read_dir(path) {
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

pub fn host_files_metadata(ctx: &HostContext, path: &str) -> Result<FileStat, ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let path = resolve(ctx, path)?;
    let md = match std::fs::metadata(path) {
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

pub fn host_files_write_file(ctx: &HostContext, path: &str, data: &[u8]) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let path = resolve(ctx, path)?;
    std::fs::write(path, data).map_err(|_| ErrorCode::IoFailed)
}

pub fn host_files_append_file(ctx: &HostContext, path: &str, data: &[u8]) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let path = resolve(ctx, path)?;
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|_| ErrorCode::IoFailed)?;
    f.write_all(data).map_err(|_| ErrorCode::IoFailed)
}

pub fn host_files_create_dir(
    ctx: &HostContext,
    path: &str,
    recursive: bool,
) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let path = resolve(ctx, path)?;
    let result = if recursive {
        std::fs::create_dir_all(path)
    } else {
        std::fs::create_dir(path)
    };
    result.map_err(|_| ErrorCode::IoFailed)
}

pub fn host_files_remove_file(ctx: &HostContext, path: &str) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let path = resolve(ctx, path)?;
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ErrorCode::NotFound),
        Err(_) => Err(ErrorCode::IoFailed),
    }
}

pub fn host_files_remove_dir(
    ctx: &HostContext,
    path: &str,
    recursive: bool,
) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let path = resolve(ctx, path)?;
    let result = if recursive {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_dir(path)
    };
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ErrorCode::NotFound),
        Err(_) => Err(ErrorCode::IoFailed),
    }
}

pub fn deny_files_read_file(_ctx: &HostContext, _path: &str) -> Result<Vec<u8>, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_read_dir(_ctx: &HostContext, _path: &str) -> Result<Vec<DirEntry>, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_metadata(_ctx: &HostContext, _path: &str) -> Result<FileStat, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_write_file(
    _ctx: &HostContext,
    _path: &str,
    _data: &[u8],
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_append_file(
    _ctx: &HostContext,
    _path: &str,
    _data: &[u8],
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_create_dir(
    _ctx: &HostContext,
    _path: &str,
    _recursive: bool,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_remove_file(_ctx: &HostContext, _path: &str) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_remove_dir(
    _ctx: &HostContext,
    _path: &str,
    _recursive: bool,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

#[cfg(test)]
mod tests {
    //! Spot test for the metadata-contract through this capability,
    //! plus the nine §8 host happy-path / error-mapping tests
    //! prescribed by the 2026-04-29 plugin-files-rw-capability spec.

    use super::super::test_helpers::{bound_env_ctx, ctx_with_files_root, null_env_ctx};
    use super::*;
    use crate::env::ShellEnv;
    use tempfile::tempdir;

    #[test]
    fn files_read_file_denied_when_env_null() {
        let ctx = null_env_ctx();
        let result = host_files_read_file(&ctx, "/tmp/anything");
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    // ── Spec §8 happy-path / error-mapping tests ───────────────────────

    #[test]
    fn host_files_read_file_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("data.txt");
        let payload = b"hello world".to_vec();
        std::fs::write(&path, &payload).unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = bound_env_ctx(&mut env);
        let result = host_files_read_file(&ctx, &path.to_string_lossy());
        assert_eq!(result, Ok(payload));
    }

    #[test]
    fn host_files_read_dir_returns_entries() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = bound_env_ctx(&mut env);
        let entries = host_files_read_dir(&ctx, &dir.path().to_string_lossy()).unwrap();

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
        let ctx = bound_env_ctx(&mut env);

        let f = host_files_metadata(&ctx, &file_path.to_string_lossy()).unwrap();
        assert!(f.is_file);
        assert!(!f.is_dir);
        assert_eq!(f.size, 3);

        let d = host_files_metadata(&ctx, &dir.path().to_string_lossy()).unwrap();
        assert!(!d.is_file);
        assert!(d.is_dir);
    }

    #[test]
    fn host_files_read_file_returns_not_found_for_missing_path() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("does-not-exist.txt");

        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = bound_env_ctx(&mut env);
        let result = host_files_read_file(&ctx, &missing.to_string_lossy());
        assert_eq!(result, Err(ErrorCode::NotFound));
    }

    #[test]
    fn host_files_read_file_invalid_argument_on_empty_path() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = bound_env_ctx(&mut env);
        let result = host_files_read_file(&ctx, "");
        assert_eq!(result, Err(ErrorCode::InvalidArgument));
    }

    #[test]
    fn host_files_remove_dir_io_failed_on_nonempty_without_recursive() {
        let dir = tempdir().unwrap();
        let inner = dir.path().join("d");
        std::fs::create_dir(&inner).unwrap();
        std::fs::write(inner.join("f"), b"x").unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = bound_env_ctx(&mut env);
        let result = host_files_remove_dir(&ctx, &inner.to_string_lossy(), false);
        assert_eq!(result, Err(ErrorCode::IoFailed));
        assert!(inner.exists());
    }

    #[test]
    fn host_files_append_file_appends() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("log");

        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = bound_env_ctx(&mut env);
        let p = path.to_string_lossy();

        host_files_write_file(&ctx, &p, b"hello").unwrap();
        host_files_append_file(&ctx, &p, b" world").unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes, b"hello world");
    }

    #[test]
    fn host_files_create_dir_all_creates_intermediate_dirs() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("a/b/c");

        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = bound_env_ctx(&mut env);
        host_files_create_dir(&ctx, &nested.to_string_lossy(), true).unwrap();

        assert!(nested.is_dir());
        assert!(dir.path().join("a").is_dir());
        assert!(dir.path().join("a/b").is_dir());
    }

    // ── files_root confinement tests ───────────────────────────────────

    /// Canonicalized tempdir root (macOS /tmp is a symlink to /private/tmp).
    fn canon_root(dir: &tempfile::TempDir) -> std::path::PathBuf {
        std::fs::canonicalize(dir.path()).unwrap()
    }

    #[test]
    fn files_root_denies_read_outside_root() {
        let root_dir = tempdir().unwrap();
        let outside_dir = tempdir().unwrap();
        let secret = outside_dir.path().join("secret.txt");
        std::fs::write(&secret, b"secret").unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_files_root(&mut env, &canon_root(&root_dir));
        let result = host_files_read_file(&ctx, &secret.to_string_lossy());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn files_root_denies_dotdot_escape() {
        // The confinement root is nested one level down so the escape
        // target lands in a directory this test owns.
        let outer = tempdir().unwrap();
        let root = outer.path().join("inner");
        std::fs::create_dir(&root).unwrap();
        let root = std::fs::canonicalize(&root).unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_files_root(&mut env, &root);
        let escape = format!("{}/../escape.txt", root.display());
        assert_eq!(
            host_files_write_file(&ctx, &escape, b"x"),
            Err(ErrorCode::Denied)
        );
        assert!(!outer.path().join("escape.txt").exists());
    }

    #[test]
    fn files_root_denies_symlink_escape() {
        let root_dir = tempdir().unwrap();
        let outside_dir = tempdir().unwrap();
        let root = canon_root(&root_dir);
        let secret = outside_dir.path().join("secret.txt");
        std::fs::write(&secret, b"secret").unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&secret, &link).unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_files_root(&mut env, &root);
        let result = host_files_read_file(&ctx, &link.to_string_lossy());
        assert_eq!(result, Err(ErrorCode::Denied));
    }

    #[test]
    fn files_root_allows_inside_root_and_relative_paths() {
        let root_dir = tempdir().unwrap();
        let root = canon_root(&root_dir);

        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_files_root(&mut env, &root);
        // Relative path resolves against the root, not the process cwd.
        host_files_write_file(&ctx, "rel.txt", b"data").unwrap();
        assert_eq!(std::fs::read(root.join("rel.txt")).unwrap(), b"data");
        // Absolute path inside the root is allowed.
        let abs = root.join("rel.txt");
        assert_eq!(
            host_files_read_file(&ctx, &abs.to_string_lossy()),
            Ok(b"data".to_vec())
        );
    }

    #[test]
    fn files_root_denies_remove_dir_outside_root() {
        let root_dir = tempdir().unwrap();
        let outside_dir = tempdir().unwrap();
        let victim = outside_dir.path().join("victim");
        std::fs::create_dir(&victim).unwrap();
        std::fs::write(victim.join("f"), b"x").unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = ctx_with_files_root(&mut env, &canon_root(&root_dir));
        let result = host_files_remove_dir(&ctx, &victim.to_string_lossy(), true);
        assert_eq!(result, Err(ErrorCode::Denied));
        assert!(victim.exists());
    }

    #[test]
    fn host_files_remove_dir_recursive_removes_subtree() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("tree");
        std::fs::create_dir_all(root.join("inner")).unwrap();
        std::fs::write(root.join("f"), b"x").unwrap();
        std::fs::write(root.join("inner/g"), b"y").unwrap();

        let mut env = ShellEnv::new("yosh", vec![]);
        let ctx = bound_env_ctx(&mut env);
        host_files_remove_dir(&ctx, &root.to_string_lossy(), true).unwrap();

        assert!(!root.exists());
    }
}
