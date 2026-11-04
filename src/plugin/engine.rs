//! Lazily-built wasmtime `Engine` and its epoch tick thread.

use wasmtime::Engine;

use super::PluginManager;
use super::deadline::TICK_MS;

/// Background thread that increments the wasmtime epoch counter at a
/// fixed interval (`TICK_MS`). One per `PluginManager`, spawned lazily
/// with the engine. The worker waits on a `Condvar` with a `TICK_MS`
/// timeout instead of sleeping, so `Drop` can wake it immediately —
/// shutdown join returns in microseconds rather than up to `TICK_MS`.
pub(super) struct TickThread {
    /// Shared shutdown state: `Mutex<bool>` stop flag + the `Condvar`
    /// the worker waits on. `Drop` sets the flag and notifies.
    state: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl TickThread {
    fn spawn(engine: wasmtime::Engine) -> Self {
        use std::sync::{Arc, Condvar, Mutex};
        use std::thread;
        use std::time::Duration;

        let state = Arc::new((Mutex::new(false), Condvar::new()));
        let state_inner = Arc::clone(&state);
        let handle = thread::Builder::new()
            .name("yosh-plugin-epoch-tick".to_string())
            .spawn(move || {
                let (lock, cvar) = &*state_inner;
                let mut stopped = lock.lock().expect("epoch-tick mutex poisoned");
                while !*stopped {
                    let (guard, timeout) = cvar
                        .wait_timeout(stopped, Duration::from_millis(TICK_MS))
                        .expect("epoch-tick mutex poisoned");
                    stopped = guard;
                    // Tick only on a real timeout — a notify means
                    // shutdown (or a spurious wake, where skipping one
                    // tick is harmless since the cadence is best-effort).
                    if !*stopped && timeout.timed_out() {
                        // Cheap and safe to call concurrently with guest
                        // execution; wasmtime is designed for this exact
                        // pattern.
                        engine.increment_epoch();
                    }
                }
            })
            .expect("spawn yosh-plugin-epoch-tick thread");
        TickThread {
            state,
            handle: Some(handle),
        }
    }
}

impl Drop for TickThread {
    fn drop(&mut self) {
        {
            let (lock, cvar) = &*self.state;
            *lock.lock().expect("epoch-tick mutex poisoned") = true;
            cvar.notify_all();
        }
        if let Some(h) = self.handle.take() {
            // The worker is woken by the notify above, so this join
            // returns promptly instead of waiting out a sleep.
            let _ = h.join();
        }
    }
}

impl PluginManager {
    /// Build the wasmtime `Engine` with yosh's canonical config. Split
    /// out of `ensure_engine` so tests can construct one directly.
    fn build_engine() -> Engine {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(false);
        config.consume_fuel(false);
        config.epoch_interruption(true);
        // Best-effort: enable system cache. If unavailable we just proceed
        // without it. cwasm precompile is the durable cache; this is the
        // lower-level wasmtime cranelift cache.
        let _ = config.cache_config_load_default();

        Engine::new(&config).expect("wasmtime Engine::new")
    }

    /// Initialize the `Engine` and its epoch tick thread on first use
    /// (called at the top of `load_one`). Idempotent.
    pub(super) fn ensure_engine(&mut self) {
        if self.engine.is_none() {
            // wasmtime installs its own trap handlers (SEGV/BUS/ILL/FPE):
            // forked children must reset every signal again.
            crate::signal::assume_all_dispositions_changed();
            let engine = Self::build_engine();
            self.tick_thread = Some(TickThread::spawn(engine.clone()));
            self.engine = Some(engine);
        }
    }

    /// Engine accessor for code paths that only run during or after a
    /// plugin load (`load_one` calls `ensure_engine` first; dispatch
    /// paths operate on plugins that could only exist post-load).
    pub(super) fn engine(&self) -> &Engine {
        self.engine
            .as_ref()
            .expect("engine initialized by ensure_engine before use")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_without_plugins_has_no_engine_or_tick_thread() {
        // The no-plugins fast path: `new()` must not build a wasmtime
        // Engine or spawn the epoch tick thread. Eagerly doing so added
        // ~50ms (tick-thread Drop join) to every clean shell exit and
        // command-substitution child.
        let mgr = PluginManager::new();
        assert!(mgr.engine.is_none(), "engine must be lazy");
        assert!(mgr.tick_thread.is_none(), "tick thread must be lazy");
    }

    #[test]
    fn ensure_engine_spawns_engine_and_tick_thread_once() {
        let mut mgr = PluginManager::new();
        mgr.ensure_engine();
        assert!(mgr.engine.is_some());
        assert!(mgr.tick_thread.is_some());
        // Idempotent: a second call must not replace either.
        let state_before = std::sync::Arc::as_ptr(&mgr.tick_thread.as_ref().unwrap().state);
        mgr.ensure_engine();
        let state_after = std::sync::Arc::as_ptr(&mgr.tick_thread.as_ref().unwrap().state);
        assert_eq!(
            state_before, state_after,
            "ensure_engine must be idempotent"
        );
    }

    #[test]
    fn tick_thread_stops_promptly_on_drop() {
        use std::time::Instant;

        let tick = TickThread::spawn(PluginManager::build_engine());
        // Capture the shared state to observe post-drop.
        let state = std::sync::Arc::clone(&tick.state);
        assert!(
            !*state.0.lock().unwrap(),
            "stop flag must be false while the thread is alive"
        );

        let t0 = Instant::now();
        drop(tick);
        let elapsed = t0.elapsed();

        assert!(
            *state.0.lock().unwrap(),
            "stop flag must be true after TickThread drops"
        );
        // The Condvar notify wakes the worker immediately: the join must
        // return well within one tick window, not after sleeping it out.
        assert!(
            elapsed.as_millis() < TICK_MS as u128,
            "Drop took {:?} (>= {}ms tick); Condvar wake-up is not working",
            elapsed,
            TICK_MS
        );
    }
}
