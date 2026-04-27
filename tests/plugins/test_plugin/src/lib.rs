use std::sync::Mutex;
use yosh_plugin_sdk::{Capability, HookName, Plugin, export, get_var, print};

static EVENT_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn record(event: impl Into<String>) {
    EVENT_LOG.lock().unwrap_or_else(|e| e.into_inner()).push(event.into());
}

#[derive(Default)]
struct TestPlugin;

impl Plugin for TestPlugin {
    fn commands(&self) -> &[&'static str] {
        &["test_cmd", "echo_var", "trap_now"]
    }

    fn required_capabilities(&self) -> &[Capability] {
        &[
            Capability::VariablesRead,
            Capability::VariablesWrite,
            Capability::Io,
            Capability::HookPreExec,
            Capability::HookOnCd,
        ]
    }

    fn implemented_hooks(&self) -> &[HookName] {
        &[HookName::PreExec, HookName::OnCd]
    }

    fn on_load(&mut self) -> Result<(), String> {
        record("on_load");
        Ok(())
    }

    fn exec(&mut self, command: &str, args: &[String]) -> i32 {
        match command {
            "test_cmd" => {
                let _ = print(&format!("test_cmd args={:?}\n", args));
                0
            }
            "echo_var" => match args.first() {
                Some(name) => match get_var(name) {
                    Ok(Some(v)) => { let _ = print(&format!("{}\n", v)); 0 }
                    Ok(None)    => { let _ = print("(unset)\n"); 0 }
                    Err(_)      => 2,
                },
                None => 1,
            },
            "trap_now" => {
                #[allow(clippy::diverging_sub_expression)]
                {
                    let _: u32 = unreachable!("intentional trap");
                }
            }
            _ => 127,
        }
    }

    fn hook_pre_exec(&mut self, command: &str) {
        record(format!("pre_exec:{}", command));
    }

    fn hook_on_cd(&mut self, old_dir: &str, new_dir: &str) {
        record(format!("on_cd:{}:{}", old_dir, new_dir));
    }

    fn on_unload(&mut self) {
        record("on_unload");
    }
}

export!(TestPlugin);
