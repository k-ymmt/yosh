//! `hog_plugin` — allocates linear memory without bound in `pre_exec`.
//! Used by `tests/plugin.rs` to verify the per-plugin memory cap: the
//! store's `MemoryLimiter` denies the grow, the guest allocator aborts,
//! and the host invalidates the plugin with a memory-cap hint. Makes no
//! host calls (same rationale as `slow_plugin`).

use yosh_plugin_sdk::{Capability, HookName, Plugin, export};

#[derive(Default)]
struct HogPlugin;

impl Plugin for HogPlugin {
    fn commands(&self) -> &[&'static str] {
        &[]
    }

    fn required_capabilities(&self) -> &[Capability] {
        &[Capability::HookPreExec]
    }

    fn implemented_hooks(&self) -> &[HookName] {
        &[HookName::PreExec]
    }

    fn exec(&mut self, _command: &str, _args: &[String]) -> i32 {
        0
    }

    fn hook_pre_exec(&mut self, _command: &str) {
        // 1 MiB chunks force repeated memory.grow until the limiter
        // denies one; the failed allocation aborts the guest.
        let mut sink: Vec<Vec<u8>> = Vec::new();
        loop {
            let mut chunk = vec![0u8; 1 << 20];
            core::hint::black_box(chunk.as_mut_ptr());
            sink.push(chunk);
        }
    }
}

export!(HogPlugin);
