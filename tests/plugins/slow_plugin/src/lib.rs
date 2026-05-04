//! `slow_plugin` — minimal plugin that busy-loops in `pre_prompt` and
//! returns immediately from `pre_exec`. Used by `tests/plugin.rs` to
//! verify (a) the epoch-deadline timeout path interrupts the busy loop
//! and invalidates the plugin, and (b) the per-call deadline-restore
//! after `pre_prompt` keeps the plugin's other hooks usable when the
//! pre_prompt itself returned in time (regression for the post-call
//! reset bug fixed in commit 154e96e).
//!
//! The plugin makes **zero host calls** by design. The test goal is to
//! verify that wasmtime's epoch-interrupt path itself terminates the
//! busy loop — *not* that the host-call deny short-circuit terminates
//! it. Adding any host call here (even a benign `print()`) would let
//! the host-side capability check fire first and mask whether the
//! pure-wasm interrupt mechanism actually works. Keep this plugin host-
//! call free.

use yosh_plugin_sdk::{Capability, HookName, Plugin, export};

#[derive(Default)]
struct SlowPlugin;

impl Plugin for SlowPlugin {
    fn commands(&self) -> &[&'static str] {
        &[]
    }

    fn required_capabilities(&self) -> &[Capability] {
        &[Capability::HookPrePrompt, Capability::HookPreExec]
    }

    fn implemented_hooks(&self) -> &[HookName] {
        &[HookName::PrePrompt, HookName::PreExec]
    }

    fn exec(&mut self, _command: &str, _args: &[String]) -> i32 {
        // SlowPlugin doesn't register any commands, so this is unreachable
        // in practice. Implemented to satisfy the required trait method.
        0
    }

    fn hook_pre_prompt(&mut self) {
        // Busy loop. core::hint::black_box defeats trivial dead-code
        // elimination; the actual termination comes from the host's
        // increment_epoch -> Trap::Interrupt path.
        loop {
            core::hint::black_box(0u64);
        }
    }

    fn hook_pre_exec(&mut self, _command: &str) {
        // No-op. Used to prove the per-call pre_prompt deadline is
        // restored to the baseline so this hook still runs without
        // tripping the epoch deadline.
    }
}

export!(SlowPlugin);
