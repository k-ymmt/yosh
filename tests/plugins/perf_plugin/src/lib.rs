//! perf_plugin — minimal-overhead fixture for plugin performance benches.
//!
//! Used by `benches/plugin_bench.rs` and `benches/startup_bench.rs`. Has no
//! stdout side-effects (unlike `test_plugin`'s `print()` calls), so Criterion
//! measurements are not polluted.

use yosh_plugin_sdk::{
    Capability, HookName, IoStream, Plugin, append_file, export, get_var, read_file, remove_file,
    set_var, write_bytes, write_file,
};

#[derive(Default)]
struct PerfPlugin;

impl Plugin for PerfPlugin {
    fn commands(&self) -> &[&'static str] {
        &[
            "noop_cmd",
            "noop_var",
            "burst_var",
            "noop_var_set",
            "noop_files_read",
            "noop_files_remove",
            "noop_io_write",
            "noop_files_write_file",
            "noop_files_append_file",
            "noop_commands_exec",
        ]
    }

    fn required_capabilities(&self) -> &[Capability] {
        &[
            Capability::VariablesRead,
            Capability::VariablesWrite,
            Capability::FilesRead,
            Capability::FilesWrite,
            Capability::Io,
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
            "noop_var_set" => {
                let _ = set_var("PERF_VAR", "v");
                0
            }
            "noop_files_read" => {
                let _ = read_file("/dev/null");
                0
            }
            "noop_files_remove" => {
                let _ = remove_file("/tmp/yosh-perf-rollout-nonexistent");
                0
            }
            "noop_io_write" => {
                let _ = write_bytes(IoStream::Stderr, b"x");
                0
            }
            "noop_files_write_file" => {
                let _ = write_file("/dev/null", b"x");
                0
            }
            "noop_files_append_file" => {
                let _ = append_file("/dev/null", b"x");
                0
            }
            "noop_commands_exec" => {
                // Deny path measurement: perf_plugin does not declare
                // Capability::CommandsExec, so the linker wires the deny closure
                // for commands::exec. The call still crosses the boundary (lift
                // happens), but the host body short-circuits to Err(Denied)
                // without spawning a subprocess. We discard the result.
                let _ = yosh_plugin_sdk::exec("/bin/echo", &["a", "b"]);
                0
            }
            _ => 127,
        }
    }

    fn hook_pre_prompt(&mut self) {
        // Empty body — measures dispatch overhead, not user work.
    }

    fn hook_pre_exec(&mut self, _command: &str) {
        // Empty body — measures dispatch overhead.
    }

    fn hook_post_exec(&mut self, _command: &str, _exit_code: i32) {
        // Empty body — measures dispatch overhead.
    }
}

export!(PerfPlugin);
