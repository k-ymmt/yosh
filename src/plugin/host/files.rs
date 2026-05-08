//! `yosh:plugin/files` host imports — read/write/inspect filesystem
//! within the plugin sandbox. Granted via CAP_FILES_READ /
//! CAP_FILES_WRITE. None of these functions read or write `ShellEnv`,
//! so the metadata guard delegates to `ensure_bound`.
//!
//! Error mapping table (see spec
//! docs/superpowers/specs/2026-04-29-plugin-files-rw-capability-design.md
//! §4):
//! - empty path                 → InvalidArgument
//! - std::io::ErrorKind::NotFound (read side) → NotFound
//! - other I/O errors           → IoFailed

use std::time::UNIX_EPOCH;

use super::super::generated::yosh::plugin::files::{DirEntry, FileStat};
use super::super::generated::yosh::plugin::types::ErrorCode;
use super::HostContext;

pub fn host_files_read_file(ctx: &HostContext, path: &str) -> Result<Vec<u8>, ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    match std::fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ErrorCode::NotFound),
        Err(_) => Err(ErrorCode::IoFailed),
    }
}

pub fn host_files_read_dir(
    ctx: &HostContext,
    path: &str,
) -> Result<Vec<DirEntry>, ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
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

pub fn host_files_write_file(
    ctx: &HostContext,
    path: &str,
    data: Vec<u8>,
) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    std::fs::write(path, &data).map_err(|_| ErrorCode::IoFailed)
}

pub fn host_files_append_file(
    ctx: &HostContext,
    path: &str,
    data: Vec<u8>,
) -> Result<(), ErrorCode> {
    ctx.ensure_bound()?;
    if path.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|_| ErrorCode::IoFailed)?;
    f.write_all(&data).map_err(|_| ErrorCode::IoFailed)
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

pub fn deny_files_read_dir(
    _ctx: &HostContext,
    _path: &str,
) -> Result<Vec<DirEntry>, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_metadata(_ctx: &HostContext, _path: &str) -> Result<FileStat, ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_write_file(
    _ctx: &HostContext,
    _path: &str,
    _data: Vec<u8>,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::Denied)
}

pub fn deny_files_append_file(
    _ctx: &HostContext,
    _path: &str,
    _data: Vec<u8>,
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

    use super::super::test_helpers::{bound_env_ctx, null_env_ctx};
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

        host_files_write_file(&ctx, &p, b"hello".to_vec()).unwrap();
        host_files_append_file(&ctx, &p, b" world".to_vec()).unwrap();

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
