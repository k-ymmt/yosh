//! In-memory and sandboxed `yosh:plugin/files` host imports.
//! - Virtual mode (`state.sandbox_root.is_none()`): all 8 functions
//!   read/mutate `state.files` (a `HashMap<PathBuf, Vec<u8>>`).
//! - Sandbox mode: paths are canonicalised against `state.sandbox_root`;
//!   any escape returns `Denied`. Real-FS calls happen via `std::fs`.

use std::path::{Path, PathBuf};

use super::TestState;
use crate::generated::yosh::plugin::files::{DirEntry, FileStat};
use crate::generated::yosh::plugin::types::ErrorCode;
use yosh_plugin_api::{CAP_FILES_READ, CAP_FILES_WRITE};

fn require_read(state: &mut TestState, path: &str) -> Result<(), ErrorCode> {
    if state.caps & CAP_FILES_READ == 0 {
        Err(super::deny(state, "files:read", path))
    } else {
        Ok(())
    }
}

fn require_write(state: &mut TestState, path: &str) -> Result<(), ErrorCode> {
    if state.caps & CAP_FILES_WRITE == 0 {
        Err(super::deny(state, "files:write", path))
    } else {
        Ok(())
    }
}

/// In sandbox mode, return the canonicalised real path or a recorded
/// `Denied` if it escapes `root`. Virtual mode returns the path as-is.
/// Mirrors `src/plugin/host/files.rs::resolve`: a dangling in-root
/// symlink is denied (writing through it would materialize the outside
/// target), while any number of missing plain-name trailing components
/// inside the root is allowed (recursive create).
fn resolve(state: &mut TestState, path: &str) -> Result<PathBuf, ErrorCode> {
    let Some(root) = state.sandbox_root.clone() else {
        return Ok(PathBuf::from(path));
    };
    let candidate = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        root.join(path)
    };
    // Canonicalise lazily: if the file doesn't exist yet
    // (write/create), canonicalise the deepest existing ancestor and
    // re-join the missing suffix.
    let canon = match std::fs::canonicalize(&candidate) {
        Ok(p) => p,
        Err(_) => {
            let mut existing = candidate.as_path();
            let mut missing: Vec<std::ffi::OsString> = Vec::new();
            while std::fs::symlink_metadata(existing).is_err() {
                let Some(name) = existing.file_name() else {
                    return Err(super::deny(state, "files:sandbox-escape", path));
                };
                missing.push(name.to_os_string());
                let Some(parent) = existing.parent() else {
                    return Err(super::deny(state, "files:sandbox-escape", path));
                };
                existing = parent;
            }
            // Exists but didn't canonicalise → dangling symlink.
            let Ok(base) = std::fs::canonicalize(existing) else {
                return Err(super::deny(state, "files:sandbox-escape", path));
            };
            if missing.iter().any(|c| c == ".." || c == ".") {
                return Err(super::deny(state, "files:sandbox-escape", path));
            }
            let mut joined = base;
            for name in missing.iter().rev() {
                joined.push(name);
            }
            joined
        }
    };
    if canon.starts_with(&root) {
        Ok(canon)
    } else {
        Err(super::deny(state, "files:sandbox-escape", path))
    }
}

pub fn host_read_file(state: &mut TestState, path: &str) -> Result<Vec<u8>, ErrorCode> {
    require_read(state, path)?;
    let resolved = resolve(state, path)?;
    match &state.sandbox_root {
        None => state
            .files
            .get(&resolved)
            .cloned()
            .ok_or(ErrorCode::NotFound),
        Some(_) => std::fs::read(&resolved).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ErrorCode::NotFound,
            _ => ErrorCode::IoFailed,
        }),
    }
}

pub fn host_write_file(state: &mut TestState, path: &str, data: &[u8]) -> Result<(), ErrorCode> {
    require_write(state, path)?;
    let resolved = resolve(state, path)?;
    match &state.sandbox_root {
        None => {
            state.files.insert(resolved.clone(), data.to_vec());
        }
        Some(_) => {
            std::fs::write(&resolved, data).map_err(|_| ErrorCode::IoFailed)?;
        }
    }
    state.write_log.push((resolved, data.to_vec()));
    Ok(())
}

pub fn host_append_file(state: &mut TestState, path: &str, data: &[u8]) -> Result<(), ErrorCode> {
    require_write(state, path)?;
    let resolved = resolve(state, path)?;
    match &state.sandbox_root {
        None => {
            state
                .files
                .entry(resolved.clone())
                .or_default()
                .extend_from_slice(data);
        }
        Some(_) => {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&resolved)
                .map_err(|_| ErrorCode::IoFailed)?;
            f.write_all(data).map_err(|_| ErrorCode::IoFailed)?;
        }
    }
    state.write_log.push((resolved, data.to_vec()));
    Ok(())
}

pub fn host_create_dir(
    state: &mut TestState,
    path: &str,
    recursive: bool,
) -> Result<(), ErrorCode> {
    require_write(state, path)?;
    let resolved = resolve(state, path)?;
    match &state.sandbox_root {
        None => {
            // Virtual mode: directories don't exist as entries; we
            // treat create-dir as a no-op success in virtual mode.
            // Real authors should use sandbox mode for filesystem
            // structure testing.
            let _ = resolved;
            Ok(())
        }
        Some(_) => {
            let r = if recursive {
                std::fs::create_dir_all(&resolved)
            } else {
                std::fs::create_dir(&resolved)
            };
            r.map_err(|_| ErrorCode::IoFailed)
        }
    }
}

pub fn host_remove_file(state: &mut TestState, path: &str) -> Result<(), ErrorCode> {
    require_write(state, path)?;
    let resolved = resolve(state, path)?;
    match &state.sandbox_root {
        None => state
            .files
            .remove(&resolved)
            .map(|_| ())
            .ok_or(ErrorCode::NotFound),
        Some(_) => std::fs::remove_file(&resolved).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ErrorCode::NotFound,
            _ => ErrorCode::IoFailed,
        }),
    }
}

pub fn host_remove_dir(
    state: &mut TestState,
    path: &str,
    recursive: bool,
) -> Result<(), ErrorCode> {
    require_write(state, path)?;
    let resolved = resolve(state, path)?;
    match &state.sandbox_root {
        None => {
            // Virtual mode: directories don't exist as entries; remove-dir
            // is a no-op success. Mirrors `host_create_dir`'s virtual-mode
            // behaviour — use sandbox mode for filesystem-structure tests.
            let _ = (resolved, recursive);
            Ok(())
        }
        Some(_) => {
            let r = if recursive {
                std::fs::remove_dir_all(&resolved)
            } else {
                std::fs::remove_dir(&resolved)
            };
            r.map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => ErrorCode::NotFound,
                _ => ErrorCode::IoFailed,
            })
        }
    }
}

pub fn host_read_dir(state: &mut TestState, path: &str) -> Result<Vec<DirEntry>, ErrorCode> {
    require_read(state, path)?;
    let resolved = resolve(state, path)?;
    match &state.sandbox_root {
        None => {
            // Virtual mode: enumerate keys with `resolved` as a prefix.
            let mut out = Vec::new();
            for k in state.files.keys() {
                if k.parent() == Some(&resolved) {
                    out.push(DirEntry {
                        name: k
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        is_file: true,
                        is_dir: false,
                        is_symlink: false,
                    });
                }
            }
            Ok(out)
        }
        Some(_) => {
            let rd = std::fs::read_dir(&resolved).map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => ErrorCode::NotFound,
                _ => ErrorCode::IoFailed,
            })?;
            let mut out = Vec::new();
            for entry in rd.flatten() {
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
    }
}

pub fn host_metadata(state: &mut TestState, path: &str) -> Result<FileStat, ErrorCode> {
    require_read(state, path)?;
    let resolved = resolve(state, path)?;
    match &state.sandbox_root {
        None => {
            let bytes = state.files.get(&resolved).ok_or(ErrorCode::NotFound)?;
            Ok(FileStat {
                is_file: true,
                is_dir: false,
                is_symlink: false,
                size: bytes.len() as u64,
                mtime_secs: 0,
            })
        }
        Some(_) => {
            let md = std::fs::metadata(&resolved).map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => ErrorCode::NotFound,
                _ => ErrorCode::IoFailed,
            })?;
            Ok(FileStat {
                is_file: md.is_file(),
                is_dir: md.is_dir(),
                is_symlink: false,
                size: md.len(),
                mtime_secs: 0,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_rw() -> TestState {
        TestState::with_caps(CAP_FILES_READ | CAP_FILES_WRITE)
    }

    #[test]
    fn read_denied_without_cap() {
        let mut s = TestState::default();
        assert_eq!(host_read_file(&mut s, "/a"), Err(ErrorCode::Denied));
        assert_eq!(s.denied_log, vec!["files:read: /a".to_string()]);
    }

    #[test]
    fn virtual_write_then_read_roundtrips() {
        let mut s = state_rw();
        host_write_file(&mut s, "/a", b"hello").unwrap();
        assert_eq!(host_read_file(&mut s, "/a"), Ok(b"hello".to_vec()));
        assert_eq!(s.write_log.len(), 1);
    }

    #[test]
    fn virtual_read_missing_returns_not_found() {
        let mut s = state_rw();
        assert_eq!(host_read_file(&mut s, "/missing"), Err(ErrorCode::NotFound));
    }

    #[test]
    fn virtual_append_concatenates() {
        let mut s = state_rw();
        host_write_file(&mut s, "/a", b"hello ").unwrap();
        host_append_file(&mut s, "/a", b"world").unwrap();
        assert_eq!(host_read_file(&mut s, "/a"), Ok(b"hello world".to_vec()));
    }

    #[test]
    fn virtual_remove_deletes_entry() {
        let mut s = state_rw();
        host_write_file(&mut s, "/a", b"x").unwrap();
        host_remove_file(&mut s, "/a").unwrap();
        assert_eq!(host_read_file(&mut s, "/a"), Err(ErrorCode::NotFound));
    }

    #[test]
    fn virtual_metadata_reports_size() {
        let mut s = state_rw();
        host_write_file(&mut s, "/a", b"abc").unwrap();
        let md = host_metadata(&mut s, "/a").unwrap();
        assert!(md.is_file);
        assert_eq!(md.size, 3);
    }

    #[test]
    fn sandbox_escape_returns_denied() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let mut s = state_rw();
        s.sandbox_root = Some(root.clone());
        let outside = format!("{}/../etc/passwd", root.display());
        let err = host_read_file(&mut s, &outside);
        assert!(matches!(
            err,
            Err(ErrorCode::Denied) | Err(ErrorCode::NotFound)
        ));
    }

    /// Regression: a dangling in-root symlink pointing outside must not
    /// be writable through (mirrors src/plugin/host/files.rs).
    #[test]
    fn sandbox_denies_dangling_symlink_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let target = outside.path().join("victim.txt"); // does not exist
        let link = root.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let mut s = state_rw();
        s.sandbox_root = Some(root.clone());
        assert_eq!(
            host_write_file(&mut s, &link.to_string_lossy(), b"pwned"),
            Err(ErrorCode::Denied)
        );
        assert!(!target.exists());
    }

    /// Regression: ≥2 missing trailing components inside the root were
    /// denied (and mislabeled as files:sandbox-escape).
    #[test]
    fn sandbox_allows_multi_level_missing_create_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let mut s = state_rw();
        s.sandbox_root = Some(root.clone());
        host_create_dir(&mut s, "a/b/c", true)
            .expect("recursive create-dir of in-root path must be allowed");
        assert!(root.join("a/b/c").is_dir());
        assert!(
            s.denied_log.is_empty(),
            "no sandbox-escape denial for an in-root path: {:?}",
            s.denied_log
        );
    }

    #[test]
    fn sandbox_write_lands_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let mut s = state_rw();
        s.sandbox_root = Some(root.clone());
        host_write_file(&mut s, "out.txt", b"data").unwrap();
        let on_disk = std::fs::read(root.join("out.txt")).unwrap();
        assert_eq!(on_disk, b"data");
    }
}
