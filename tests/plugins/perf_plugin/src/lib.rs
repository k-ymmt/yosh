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

    fn exec(&mut self, command: &str, _args: &[String]) -> i32 {
        match command {
            "noop_cmd" => 0,
            "noop_var" => {
                let _ = get_var("PERF_VAR");
                0
            }
            "burst_var" => {
                for _ in 0..10 {
                    let _ = get_var("PERF_VAR");
                }
                0
            }
            _ => 127,
        }
    }
}

export!(PerfPlugin);
