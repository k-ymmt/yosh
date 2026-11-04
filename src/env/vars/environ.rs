//! The exported environment handed to child processes: the exported
//! variables as `(name, value)` pairs and as `execve`-ready `envp`
//! strings, both cached until a write can change them.

use std::ffi::CString;

use super::{Scope, VarStore};

/// Caches derived from the exported variables. Writes that can change
/// what children inherit drop both together (`invalidate`); each is
/// rebuilt on its next use.
#[derive(Debug, Clone, Default)]
pub(super) struct ExportCache {
    environ: Option<Vec<(String, String)>>,
    /// `KEY=VALUE` C strings for `execve`, derived from `environ`
    /// and invalidated together with it. Lets a forked child exec with
    /// a prebuilt `envp` instead of decoding + `setenv`-ing every
    /// exported variable after the fork (which was O(n²) in libc and
    /// allocated per spawn).
    envp: Option<Vec<CString>>,
}

impl ExportCache {
    /// Drop both caches after a write that can change `environ()`.
    pub(super) fn invalidate(&mut self) {
        self.environ = None;
        self.envp = None;
    }
}

impl VarStore {
    /// Exported variables as `KEY=VALUE` C strings ready for `execve`,
    /// with byteenc escapes decoded back to raw bytes. Entries whose
    /// key or value contains NUL are skipped. Cached like [`environ`].
    ///
    /// [`environ`]: Self::environ
    pub fn envp(&mut self) -> &[CString] {
        let cache = &mut self.export_cache;
        if cache.envp.is_none() {
            let pairs = cache
                .environ
                .get_or_insert_with(|| build_environ(&self.scopes));
            let mut v = Vec::with_capacity(pairs.len());
            for (k, val) in pairs.iter() {
                let mut bytes = crate::byteenc::decode_bytes(k).into_owned();
                bytes.push(b'=');
                bytes.extend_from_slice(&crate::byteenc::decode_bytes(val));
                if let Ok(c) = CString::new(bytes) {
                    v.push(c);
                }
            }
            cache.envp = Some(v);
        }
        cache.envp.as_ref().unwrap()
    }

    /// Return only exported variables as (name, value) pairs.
    /// Later scopes shadow earlier ones. Result is cached until next mutation.
    pub fn environ(&mut self) -> &[(String, String)] {
        self.export_cache
            .environ
            .get_or_insert_with(|| build_environ(&self.scopes))
    }
}

/// Build the exported-environ snapshot in a single pass: walk scopes
/// top-down (current scope first) and track already-seen names in a
/// `HashSet<&str>` so each name is resolved to its shadowing (topmost)
/// scope without allocating an intermediate `HashMap<String, &Variable>`
/// covering every variable (exported or not). Only exported entries
/// are cloned into the result.
fn build_environ(scopes: &[Scope]) -> Vec<(String, String)> {
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut result = Vec::new();
    for scope in scopes.iter().rev() {
        for (name, var) in &scope.vars {
            if !seen.insert(name.as_str()) {
                continue;
            }
            if var.exported
                && let Some(value) = &var.value
            {
                result.push((name.clone(), value.clone()));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envp_tracks_environ_and_decodes_bytes() {
        let mut vs = VarStore::new();
        vs.set("A", "1").unwrap();
        vs.export("A");
        let raw = format!("x{}y", crate::byteenc::escape_char(0xff));
        vs.set("B", raw).unwrap();
        vs.export("B");
        vs.set("C", "unexported").unwrap();
        let mut got: Vec<Vec<u8>> = vs.envp().iter().map(|c| c.as_bytes().to_vec()).collect();
        got.sort();
        assert_eq!(got, vec![b"A=1".to_vec(), b"B=x\xffy".to_vec()]);
        // Cached until the next exported-var mutation.
        assert!(vs.export_cache.envp.is_some());
        vs.set("C", "still unexported").unwrap();
        assert!(vs.export_cache.envp.is_some());
        vs.set("A", "2").unwrap();
        assert!(vs.export_cache.envp.is_none());
        assert!(vs.envp().iter().any(|c| c.as_bytes() == b"A=2"));
        // NUL-containing values are skipped rather than truncated.
        vs.set("N", "a\0b").unwrap();
        vs.export("N");
        assert!(!vs.envp().iter().any(|c| c.as_bytes().starts_with(b"N=")));
    }

    #[test]
    fn test_environ_excludes_unexported() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        store.set("BAZ", "qux").unwrap();
        store.export("FOO");
        let env = store.environ();
        assert_eq!(env.len(), 1);
        assert_eq!(env[0], ("FOO".to_string(), "bar".to_string()));
    }

    // ── environ_cache invalidation gating (TODO PERF item 2) ────────────

    #[test]
    fn set_on_non_exported_var_does_not_invalidate_cache() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        let _ = store.environ(); // populate cache
        assert!(store.export_cache.environ.is_some());

        store.set("FOO", "baz").unwrap();
        assert!(
            store.export_cache.environ.is_some(),
            "writing a non-exported var must not clear the environ cache"
        );
        assert_eq!(store.get("FOO"), Some("baz"));
    }

    #[test]
    fn set_on_exported_var_invalidates_cache() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        store.export("FOO");
        let _ = store.environ(); // populate cache
        assert!(store.export_cache.environ.is_some());

        store.set("FOO", "baz").unwrap();
        assert!(
            store.export_cache.environ.is_none(),
            "writing an exported var must clear the environ cache"
        );
        assert_eq!(store.environ(), &[("FOO".to_string(), "baz".to_string())]);
    }

    #[test]
    fn set_creating_new_non_exported_var_does_not_invalidate_cache() {
        let mut store = VarStore::new();
        store.set("EXPORTED", "1").unwrap();
        store.export("EXPORTED");
        let _ = store.environ();
        assert!(store.export_cache.environ.is_some());

        // Creating a brand-new, non-exported variable must not disturb the
        // cached exported-environ snapshot.
        store.set("NEWVAR", "value").unwrap();
        assert!(store.export_cache.environ.is_some());
        assert_eq!(store.get("NEWVAR"), Some("value"));
    }

    #[test]
    fn set_with_options_allexport_invalidates_cache_for_new_var() {
        let mut store = VarStore::new();
        store.set("EXPORTED", "1").unwrap();
        store.export("EXPORTED");
        let _ = store.environ();
        assert!(store.export_cache.environ.is_some());

        // Under `set -a` (allexport), a newly created variable becomes
        // exported, so the cache must be invalidated.
        store.set_with_options("NEWVAR", "value", true).unwrap();
        assert!(
            store.export_cache.environ.is_none(),
            "allexport-created var must invalidate the cache"
        );
        let env = store.environ();
        assert!(env.contains(&("NEWVAR".to_string(), "value".to_string())));
    }

    #[test]
    fn set_with_options_non_allexport_on_non_exported_var_does_not_invalidate_cache() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        let _ = store.environ();
        assert!(store.export_cache.environ.is_some());

        store.set_with_options("FOO", "baz", false).unwrap();
        assert!(
            store.export_cache.environ.is_some(),
            "non-allexport write to a non-exported var must not invalidate the cache"
        );
        assert_eq!(store.get("FOO"), Some("baz"));
    }

    #[test]
    fn unset_non_exported_var_does_not_invalidate_cache() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        let _ = store.environ();
        assert!(store.export_cache.environ.is_some());

        store.unset("FOO").unwrap();
        assert!(
            store.export_cache.environ.is_some(),
            "unsetting a non-exported var need not invalidate the cache"
        );
        assert_eq!(store.get("FOO"), None);
    }

    #[test]
    fn unset_exported_var_invalidates_cache() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        store.export("FOO");
        let _ = store.environ();
        assert!(store.export_cache.environ.is_some());

        store.unset("FOO").unwrap();
        assert!(
            store.export_cache.environ.is_none(),
            "unsetting an exported var must invalidate the cache"
        );
        assert!(store.environ().is_empty());
    }

    #[test]
    fn export_invalidates_cache() {
        let mut store = VarStore::new();
        store.set("FOO", "bar").unwrap();
        let _ = store.environ();
        assert!(store.export_cache.environ.is_some());

        store.export("FOO");
        assert!(store.export_cache.environ.is_none());
        assert_eq!(store.environ(), &[("FOO".to_string(), "bar".to_string())]);
    }

    // ── build_environ scope shadowing (TODO PERF item 4) ─────────────────

    #[test]
    fn build_environ_later_scope_shadows_earlier() {
        let mut store = VarStore::new();
        store.set("FOO", "global").unwrap();
        store.export("FOO");

        store.push_scope(vec![]);
        // New scope shadows FOO with its own exported value.
        store.set_with_options("FOO", "local", true).unwrap();
        let env = store.environ();
        assert_eq!(env, &[("FOO".to_string(), "local".to_string())]);
        store.pop_scope();

        // After popping, the global FOO value (mutated in place by
        // `set_with_options`, since FOO already existed there... but here
        // FOO only exists in global at this point since the function-scope
        // write updates whichever scope currently holds the var. Since FOO
        // pre-existed in global before push_scope, the write landed in the
        // global scope too (POSIX: existing var updates affect caller).
        assert_eq!(store.get("FOO"), Some("local"));
    }

    #[test]
    fn build_environ_excludes_unexported_across_scopes() {
        let mut store = VarStore::new();
        store.set("GLOBAL_EXPORTED", "g").unwrap();
        store.export("GLOBAL_EXPORTED");
        store.set("GLOBAL_PLAIN", "p").unwrap();

        store.push_scope(vec![]);
        store.set("NEW_IN_SCOPE", "new").unwrap();
        // NEW_IN_SCOPE is new (didn't exist in any scope), so it lands in
        // the global scope per existing `set` semantics — not exported.

        let env = store.environ();
        assert_eq!(env.len(), 1);
        assert_eq!(env[0], ("GLOBAL_EXPORTED".to_string(), "g".to_string()));
        store.pop_scope();
    }
}
