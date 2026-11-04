//! Guest-call wrappers: `EnvGuard` + `with_env` bind the shell
//! environment for the duration of one guest call, `with_deadline`
//! bounds it by an epoch deadline, and the `log_*` helpers phrase the
//! user-visible failure messages.

use wasmtime::Store;

use crate::env::ShellEnv;

use super::LoadedPlugin;
use super::deadline::STORE_BASELINE_DEADLINE_TICKS;
use super::generated::PluginWorld;
use super::host::HostContext;

// ── EnvGuard + with_env ────────────────────────────────────────────────

/// RAII guard that binds a raw `*mut ShellEnv` into the `Store`'s
/// `HostContext` and resets it to null on drop. Drop runs on every exit
/// path: normal return, `Err`, host-side panic, trap unwind.
pub(super) struct EnvGuard<'a> {
    store: &'a mut Store<HostContext>,
}

impl<'a> EnvGuard<'a> {
    pub(super) fn bind(store: &'a mut Store<HostContext>, env: &mut ShellEnv) -> Self {
        store.data_mut().env = env as *mut _;
        EnvGuard { store }
    }

    pub(super) fn store(&mut self) -> &mut Store<HostContext> {
        self.store
    }
}

impl Drop for EnvGuard<'_> {
    fn drop(&mut self) {
        // Always restores env to null, even during unwinding. Drop itself
        // cannot panic because pointer assignment is infallible.
        self.store.data_mut().env = std::ptr::null_mut();
    }
}

/// Failure mode of a `with_env` dispatch. The caller decides how to
/// phrase the user-visible message — pre_prompt timeouts in particular
/// want hook-specific wording.
pub(super) enum WithEnvError {
    /// Plugin instance was already invalidated by an earlier failure.
    /// `with_env` has already printed a "skipped" line.
    Skipped,
    /// Guest trap. `is_interrupt` is true for `Trap::Interrupt` (epoch
    /// deadline exceeded). The plugin has been marked invalidated.
    Trapped {
        is_interrupt: bool,
        /// The store's memory limiter denied a grow during this call —
        /// the trap is (almost certainly) the guest allocator aborting
        /// on the failed allocation.
        memory_denied: bool,
        trap: wasmtime::Trap,
    },
    /// Non-trap host-side error (e.g. type mismatch). The plugin is NOT
    /// invalidated; the failure is one-off.
    Other(wasmtime::Error),
}

/// Canonical dispatch wrapper for any guest-bound call that needs host
/// API access. Sets up `EnvGuard`, runs the callback, and converts
/// `wasmtime::Error` into a `WithEnvError`. Direct callers never observe
/// `wasmtime::Error` themselves; they pattern-match on `WithEnvError` and
/// pick their own message phrasing.
pub(super) fn with_env<R>(
    plugin: &mut LoadedPlugin,
    env: &mut ShellEnv,
    f: impl FnOnce(&PluginWorld, &mut Store<HostContext>) -> Result<R, wasmtime::Error>,
) -> Result<R, WithEnvError> {
    if plugin.invalidated {
        eprintln!(
            "yosh: plugin '{}': skipped (instance invalidated by earlier trap)",
            plugin.name
        );
        return Err(WithEnvError::Skipped);
    }

    // Clear any stale `denied` flag left over from a prior grow that the
    // guest survived without trapping (e.g. `try_reserve`, or a
    // non-Rust guest that handles allocation failure gracefully), or
    // from a denial during `on_load` (which runs via a raw `EnvGuard`,
    // not `with_env`, so this is the first chance to clear it). Without
    // this, a later unrelated trap would read a stale `true` here and
    // mis-attribute itself as a memory-limit trap.
    plugin.store.data_mut().mem_limiter.denied = false;

    let bindings = &plugin.bindings;
    let result = {
        let mut guard = EnvGuard::bind(&mut plugin.store, env);
        f(bindings, guard.store())
    };

    match result {
        Ok(r) => Ok(r),
        Err(e) => {
            if let Some(trap) = e.downcast_ref::<wasmtime::Trap>() {
                let trap = *trap;
                let is_interrupt = matches!(trap, wasmtime::Trap::Interrupt);
                let memory_denied =
                    std::mem::replace(&mut plugin.store.data_mut().mem_limiter.denied, false);
                plugin.invalidated = true;
                Err(WithEnvError::Trapped {
                    is_interrupt,
                    memory_denied,
                    trap,
                })
            } else {
                Err(WithEnvError::Other(e))
            }
        }
    }
}

/// Run a guest call under an optional epoch deadline. `None` leaves the
/// store at the baseline (effectively unlimited). The baseline is
/// restored after the call unless the plugin trapped (it is then
/// invalidated and its deadline is moot).
pub(super) fn with_deadline<R>(
    plugin: &mut LoadedPlugin,
    env: &mut ShellEnv,
    deadline_ticks: Option<u64>,
    f: impl FnOnce(&PluginWorld, &mut Store<HostContext>) -> Result<R, wasmtime::Error>,
) -> Result<R, WithEnvError> {
    if let Some(ticks) = deadline_ticks {
        plugin.store.set_epoch_deadline(ticks);
    }
    let result = with_env(plugin, env, f);
    if deadline_ticks.is_some() && !matches!(&result, Err(WithEnvError::Trapped { .. })) {
        plugin
            .store
            .set_epoch_deadline(STORE_BASELINE_DEADLINE_TICKS);
    }
    result
}

/// Failure logger for deadline-bounded entry points: names the entry
/// point and its budget on an epoch interrupt; defers to
/// `log_with_env_failure` for everything else.
pub(super) fn log_entry_failure(
    plugin_name: &str,
    entry: &str,
    timeout_ms: u64,
    max_memory_mb: u64,
    err: &WithEnvError,
) {
    match err {
        WithEnvError::Trapped {
            is_interrupt: true, ..
        } => {
            eprintln!(
                "yosh: plugin '{}': {} exceeded {}ms timeout — disabling for the rest of this session",
                plugin_name, entry, timeout_ms
            );
        }
        other => log_with_env_failure(plugin_name, other, max_memory_mb),
    }
}

/// Generic "with_env failure" logger for hooks that don't need
/// hook-specific phrasing. Reproduces the pre-refactor messages exactly.
pub(super) fn log_with_env_failure(plugin_name: &str, err: &WithEnvError, max_memory_mb: u64) {
    match err {
        WithEnvError::Skipped => {}
        // Must precede the generic `Trapped` arm below: both patterns
        // match `Trapped { .. }`, and Rust picks the first arm whose
        // pattern matches, so this more-specific `memory_denied: true`
        // arm has to come first or it would be unreachable.
        WithEnvError::Trapped {
            memory_denied: true,
            trap,
            ..
        } => {
            eprintln!(
                "yosh: plugin '{}': trapped: {} (memory limit {} MiB exceeded) — disabling for the rest of this session",
                plugin_name, trap, max_memory_mb
            );
        }
        WithEnvError::Trapped { trap, .. } => {
            eprintln!(
                "yosh: plugin '{}': trapped: {} — disabling for the rest of this session",
                plugin_name, trap
            );
        }
        WithEnvError::Other(e) => {
            eprintln!("yosh: plugin '{}': call failed: {}", plugin_name, e);
        }
    }
}
