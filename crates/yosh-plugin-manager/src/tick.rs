//! Continuous epoch-tick thread for the run/test harness. Mirrors the
//! production host's `TickThread` (`src/plugin/mod.rs`): bump the
//! engine epoch every `TICK_MS` so per-store tick deadlines trip within
//! one tick window, instead of the old one-shot watchdog whose single
//! bump competed with a busy guest for CPU (3-8s observed latency).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

pub const TICK_MS: u64 = 50;

pub struct TickThread {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl TickThread {
    pub fn spawn(engine: wasmtime::Engine) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_inner = stop.clone();
        let handle = std::thread::Builder::new()
            .name("yosh-plugin-manager-epoch-tick".to_string())
            .spawn(move || {
                while !stop_inner.load(Ordering::Acquire) {
                    std::thread::sleep(std::time::Duration::from_millis(TICK_MS));
                    engine.increment_epoch();
                }
            })
            .expect("spawn yosh-plugin-manager-epoch-tick thread");
        TickThread {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for TickThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_thread_stops_and_joins_on_drop() {
        let engine = crate::precompile::make_engine().expect("engine");
        let t = TickThread::spawn(engine);
        std::thread::sleep(std::time::Duration::from_millis(120));
        drop(t); // must not hang
    }
}
