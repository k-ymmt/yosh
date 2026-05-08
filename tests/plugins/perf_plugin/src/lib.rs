//! perf_plugin — minimal-overhead fixture for plugin performance benches.
//!
//! Used by `benches/plugin_bench.rs` and `benches/startup_bench.rs`. Has no
//! stdout side-effects (unlike `test_plugin`'s `print()` calls), so Criterion
//! measurements are not polluted.

use yosh_plugin_sdk::{Capability, HookName, Plugin, export, get_var};

#[derive(Default)]
struct PerfPlugin;

impl Plugin for PerfPlugin {
    fn commands(&self) -> &[&'static str] {
        &["noop_cmd", "noop_var", "burst_var"]
    }

    fn required_capabilities(&self) -> &[Capability] {
        &[
            Capability::VariablesRead,
            Capability::HookPrePrompt,
            Capability::HookPreExec,
            Capability::HookPostExec,
        ]
    }

    fn implemented_hooks(&self) -> &[HookName] {
        &[HookName::PrePrompt, HookName::PreExec, HookName::PostExec]
    }

    fn exec(&mut self, _command: &str, _args: &[String]) -> i32 {
        // Filled in by Task 2.
        127
    }
}

export!(PerfPlugin);
