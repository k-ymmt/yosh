//! `slow_plugin` — minimal plugin whose `pre_prompt` and `on_cd` hooks
//! busy-loop and whose `spin` command busy-loops, while `pre_exec`
//! returns immediately. Used by `tests/plugin.rs` and the manager's
//! `tests/runner.rs` to verify the epoch-deadline timeout paths
//! (per-entry-point budgets) and the post-call deadline restore.
//!
//! The plugin makes **zero host calls** by design. The test goal is to
//! verify that wasmtime's epoch-interrupt path itself terminates the
//! busy loop — *not* that the host-call deny short-circuit terminates
//! it. Keep this plugin host-call free.

use yosh_plugin_sdk::{Capability, HookName, Plugin, export};

#[derive(Default)]
struct SlowPlugin;

fn busy_loop() -> ! {
    // core::hint::black_box defeats trivial dead-code elimination; the
    // actual termination comes from increment_epoch -> Trap::Interrupt.
    loop {
        core::hint::black_box(0u64);
    }
}

impl Plugin for SlowPlugin {
    fn commands(&self) -> &[&'static str] {
        &["spin"]
    }

    fn required_capabilities(&self) -> &[Capability] {
        &[
            Capability::HookPrePrompt,
            Capability::HookPreExec,
            Capability::HookOnCd,
        ]
    }

    fn implemented_hooks(&self) -> &[HookName] {
        &[HookName::PrePrompt, HookName::PreExec, HookName::OnCd]
    }

    fn exec(&mut self, command: &str, _args: &[String]) -> i32 {
        if command == "spin" {
            busy_loop();
        }
        0
    }

    fn hook_pre_prompt(&mut self) {
        busy_loop();
    }

    fn hook_pre_exec(&mut self, _command: &str) {
        // No-op. Proves the per-call deadline is restored to baseline
        // so a later hook on the same store still runs.
    }

    fn hook_on_cd(&mut self, _old: &str, _new: &str) {
        busy_loop();
    }
}

export!(SlowPlugin);
