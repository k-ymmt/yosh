use kish_plugin_sdk::{Capability, Plugin, PluginApi, export};

#[derive(Default)]
struct TestPlugin;

impl Plugin for TestPlugin {
    fn commands(&self) -> &[&str] {
        &["test-hello", "test-set-var"]
    }

    fn required_capabilities(&self) -> &[Capability] {
        &[
            Capability::VariablesRead,
            Capability::VariablesWrite,
            Capability::Filesystem,
            Capability::Io,
            Capability::HookPreExec,
            Capability::HookPostExec,
            Capability::HookOnCd,
            Capability::HookPrePrompt,
        ]
    }

    fn exec(&mut self, api: &PluginApi, command: &str, args: &[&str]) -> i32 {
        match command {
            "test-hello" => {
                api.print("hello from plugin\n");
                let _ = api.set_var("TEST_EXEC_CALLED", "1");
                0
            }
            "test-set-var" => {
                if args.len() >= 2 {
                    let _ = api.set_var(args[0], args[1]);
                    0
                } else {
                    api.eprint("usage: test-set-var NAME VALUE\n");
                    1
                }
            }
            _ => 127,
        }
    }

    fn hook_pre_exec(&mut self, api: &PluginApi, cmd: &str) {
        let _ = api.set_var("TEST_PRE_EXEC", cmd);
    }

    fn hook_post_exec(&mut self, api: &PluginApi, cmd: &str, exit_code: i32) {
        let _ = api.set_var("TEST_POST_EXEC", &format!("{cmd}:{exit_code}"));
    }

    fn hook_on_cd(&mut self, api: &PluginApi, old_dir: &str, new_dir: &str) {
        let _ = api.set_var("TEST_ON_CD", &format!("{old_dir}->{new_dir}"));
    }

    fn hook_pre_prompt(&mut self, api: &PluginApi) {
        let _ = api.set_var("TEST_PRE_PROMPT", "1");
    }
}

export!(TestPlugin);
