//! `trap_plugin` — minimal plugin whose only command (`trap_now`) deliberately
//! traps the wasm guest via `unreachable!()`. Used by `tests/plugin.rs::t02`
//! to verify that the host's `with_env` wrapper invalidates the plugin
//! instance after a guest trap and that subsequent dispatch attempts return
//! `PluginExec::Failed`.

use yosh_plugin_sdk::{Capability, Plugin, export};

#[derive(Default)]
struct TrapPlugin;

impl Plugin for TrapPlugin {
    fn commands(&self) -> &[&'static str] {
        &["trap_now"]
    }

    fn required_capabilities(&self) -> &[Capability] {
        &[]
    }

    fn exec(&mut self, _command: &str, _args: &[String]) -> i32 {
        #[allow(clippy::diverging_sub_expression)]
        {
            let _: u32 = unreachable!("intentional trap");
        }
    }
}

export!(TrapPlugin);
