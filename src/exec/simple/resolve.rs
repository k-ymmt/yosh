/// Outcome of resolving a command name to an executable file before fork.
#[derive(Debug)]
pub(super) enum ResolvedExec {
    Executable(std::path::PathBuf),
    NotExecutable,
    NotFound,
}

/// Resolve `cmd` to an absolute executable path, mirroring the search
/// order `execvp` performs internally: if `cmd` contains `/`, it is used
/// directly (no PATH search, no cache); otherwise each `$PATH` entry is
/// searched in order, consulting/populating `utility_hash` for the
/// found-and-executable case (same cache `hash`/`command -v` rely on).
///
/// `effective_path`, when `Some`, is a prefix-assignment PATH override
/// (e.g. `PATH=/custom cmd`) that takes precedence over the shell's own
/// `$PATH` for this single resolution — matching the search a setenv'd
/// child's `execvp` would have performed pre-branch. Because the override
/// is specific to this one invocation, it must NOT be read from or
/// written to `utility_hash`, which caches lookups keyed only on the
/// command name against the shell's own `$PATH`; reusing or polluting
/// that cache under a differing PATH would return stale/wrong results
/// for both this call and subsequent no-override lookups.
pub(super) fn resolve_exec_path(
    cmd: &str,
    vars: &crate::env::vars::VarStore,
    effective_path: Option<&str>,
    utility_hash: &mut std::collections::HashMap<String, crate::env::HashEntry>,
) -> ResolvedExec {
    use crate::exec::command::is_executable_file;

    if cmd.contains('/') {
        // Decode byteenc-escaped bytes so a non-UTF-8 path resolves to the
        // real on-disk name.
        use std::os::unix::ffi::OsStrExt;
        let cmd_bytes = crate::byteenc::decode_bytes(cmd);
        let path = std::path::Path::new(std::ffi::OsStr::from_bytes(&cmd_bytes));
        return if is_executable_file(path) {
            ResolvedExec::Executable(path.to_path_buf())
        } else if path.is_file() || path.is_dir() {
            // Non-executable file, or a directory: execvp would have
            // failed with EACCES/EISDIR → "permission denied", 126.
            ResolvedExec::NotExecutable
        } else {
            ResolvedExec::NotFound
        };
    }

    if let Some(override_path) = effective_path {
        // Bypass utility_hash entirely: it is keyed on cmd name only and
        // assumes the shell's own $PATH, which does not hold here.
        return match crate::exec::command::lookup_in_path_uncached(cmd, override_path) {
            crate::exec::command::PathLookup::Executable(p) => ResolvedExec::Executable(p),
            crate::exec::command::PathLookup::NotExecutable(_) => ResolvedExec::NotExecutable,
            crate::exec::command::PathLookup::NotFound => ResolvedExec::NotFound,
        };
    }

    let path_var = vars.get("PATH").unwrap_or("").to_string();
    match crate::exec::command::lookup_in_path(cmd, &path_var, utility_hash) {
        crate::exec::command::PathLookup::Executable(p) => ResolvedExec::Executable(p),
        crate::exec::command::PathLookup::NotExecutable(_) => ResolvedExec::NotExecutable,
        crate::exec::command::PathLookup::NotFound => ResolvedExec::NotFound,
    }
}

/// Compute the effective `$PATH` for resolving an external command,
/// honoring a command's prefix assignments (e.g. `PATH=/custom cmd`).
///
/// POSIX §2.9.1: prefix assignments are applied to the environment of the
/// command being executed and therefore affect that command's own PATH
/// search, without altering the shell's own `PATH` variable. Returns
/// `Some` only when `env_overrides` actually contains a `PATH` entry
/// (last-wins, matching setenv/child-environment semantics); `None`
/// means "use the shell's own $PATH", letting the caller take the
/// normal (cached) resolution path.
pub(super) fn effective_path_override(env_overrides: &[(String, String)]) -> Option<&str> {
    env_overrides
        .iter()
        .rev()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_exec_path_direct_slash_bypasses_path_search() {
        // A cmd containing '/' must resolve directly, ignoring $PATH.
        let mut store = crate::env::vars::VarStore::new();
        store.set("PATH", "").unwrap();
        let mut cache = std::collections::HashMap::new();
        match resolve_exec_path("/bin/sh", &store, None, &mut cache) {
            ResolvedExec::Executable(p) => assert_eq!(p, std::path::Path::new("/bin/sh")),
            _ => panic!("expected /bin/sh to resolve directly"),
        }
        // Direct-path resolution must not populate the PATH cache.
        assert!(cache.is_empty());
    }

    #[test]
    fn resolve_exec_path_not_found_for_missing_slash_path() {
        let mut store = crate::env::vars::VarStore::new();
        store.set("PATH", "").unwrap();
        let mut cache = std::collections::HashMap::new();
        match resolve_exec_path("/no/such/binary_xyz", &store, None, &mut cache) {
            ResolvedExec::NotFound => {}
            _ => panic!("expected NotFound"),
        }
    }

    #[test]
    fn resolve_exec_path_uses_path_search_and_populates_cache() {
        let mut store = crate::env::vars::VarStore::new();
        let path_var = std::env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".to_string());
        store.set("PATH", path_var).unwrap();
        let mut cache = std::collections::HashMap::new();
        match resolve_exec_path("sh", &store, None, &mut cache) {
            ResolvedExec::Executable(p) => assert!(p.ends_with("sh")),
            _ => panic!("expected 'sh' to resolve via PATH"),
        }
        assert!(cache.contains_key("sh"));
    }

    #[test]
    fn resolve_exec_path_honors_effective_path_override() {
        // A prefix assignment (PATH=dir cmd) must be searched instead of
        // the shell's own $PATH, matching execvp semantics in a setenv'd
        // child (the pre-branch behavior this test guards against a
        // regression of).
        let tmp = tempfile::tempdir().unwrap();
        let tool_path = tmp.path().join("mytool_override_test");
        std::fs::write(&tool_path, "#!/bin/sh\necho hi\n").unwrap();
        std::fs::set_permissions(
            &tool_path,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();

        let mut store = crate::env::vars::VarStore::new();
        // Shell's own PATH deliberately does NOT contain the tool.
        store.set("PATH", "/nonexistent_dir_xyz").unwrap();
        let mut cache = std::collections::HashMap::new();
        let override_path = tmp.path().to_str().unwrap();
        match resolve_exec_path(
            "mytool_override_test",
            &store,
            Some(override_path),
            &mut cache,
        ) {
            ResolvedExec::Executable(p) => assert_eq!(p, tool_path),
            other => panic!("expected override PATH to resolve tool, got {other:?}"),
        }
    }

    #[test]
    fn resolve_exec_path_effective_override_does_not_use_or_populate_shell_path_cache() {
        // A cache hit keyed on the shell's own PATH must not be reused
        // when a prefix PATH override is present, and resolving under an
        // override must not pollute the cache used for normal (no
        // override) lookups.
        let path_var = std::env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".to_string());
        let mut store = crate::env::vars::VarStore::new();
        store.set("PATH", &path_var).unwrap();

        let mut cache = std::collections::HashMap::new();
        // Pre-populate the cache with a bogus entry for "sh" as if a
        // normal (non-override) lookup had happened first.
        cache.insert(
            "sh".to_string(),
            crate::env::HashEntry::new(std::path::PathBuf::from(
                "/nonexistent/fake_sh_override_test",
            )),
        );

        // With an override PATH that does contain a real "sh", resolution
        // must not be short-circuited by the stale shell-PATH cache entry.
        match resolve_exec_path("sh", &store, Some(path_var.as_str()), &mut cache) {
            ResolvedExec::Executable(p) => assert!(p.ends_with("sh")),
            other => panic!("expected override lookup to find real sh, got {other:?}"),
        }
        // The stale cache entry must be left untouched (not overwritten by
        // the override-path resolution), preserving normal-path cache
        // semantics for subsequent non-override lookups.
        assert_eq!(
            cache.get("sh").map(|e| e.path.as_path()),
            Some(std::path::Path::new("/nonexistent/fake_sh_override_test"))
        );
    }
}
