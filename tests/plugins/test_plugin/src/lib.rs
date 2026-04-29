use std::sync::Mutex;
use yosh_plugin_sdk::{
    Capability, ErrorCode, HookName, Plugin, exec, export, get_var, print, read_file, set_var,
    write_string,
};

static EVENT_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn record(event: impl Into<String>) {
    EVENT_LOG
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(event.into());
}

#[derive(Default)]
struct TestPlugin;

impl Plugin for TestPlugin {
    fn commands(&self) -> &[&'static str] {
        &[
            "test_cmd",
            "echo_var",
            "trap_now",
            "dump_events",
            "set_post_exec_marker",
            "read-file",
            "write-file",
            "run-echo",
        ]
    }

    fn required_capabilities(&self) -> &[Capability] {
        &[
            Capability::VariablesRead,
            Capability::VariablesWrite,
            Capability::Io,
            Capability::HookPreExec,
            Capability::HookOnCd,
            Capability::FilesRead,
            Capability::FilesWrite,
            Capability::CommandsExec,
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
                    Ok(Some(v)) => {
                        let _ = print(&format!("{}\n", v));
                        0
                    }
                    Ok(None) => {
                        let _ = print("(unset)\n");
                        0
                    }
                    Err(_) => 2,
                },
                None => 1,
            },
            "trap_now" => {
                #[allow(clippy::diverging_sub_expression)]
                {
                    let _: u32 = unreachable!("intentional trap");
                }
            }
            "dump_events" => {
                // Dump the event log into the host-visible variable
                // YOSH_TEST_EVENT_LOG so integration tests can inspect
                // which hook callbacks ran without scraping stdout.
                let log = EVENT_LOG
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .join(",");
                let _ = set_var("YOSH_TEST_EVENT_LOG", &log);
                0
            }
            "set_post_exec_marker" => {
                // Used by the hook-suppression test to seed a known
                // baseline value before exercising call_post_exec.
                let _ = set_var("YOSH_TEST_POST_EXEC_FIRED", "0");
                0
            }
            "read-file" => {
                let Some(path) = args.first() else { return 1 };
                match read_file(path) {
                    Ok(bytes) => {
                        if bytes == b"YOSH_TEST_CONTENT\n" {
                            0
                        } else {
                            5 // contents mismatch
                        }
                    }
                    Err(ErrorCode::Denied) => 13,
                    Err(ErrorCode::NotFound) => 4,
                    Err(_) => 1,
                }
            }
            "write-file" => {
                let Some(path) = args.first() else { return 1 };
                match write_string(path, "YOSH_TEST_CONTENT\n") {
                    Ok(()) => 0,
                    Err(ErrorCode::Denied) => 13,
                    Err(_) => 1,
                }
            }
            "run-echo" => {
                // Args are passed through as the command's argv tail. The host's
                // allowlist checks the full argv = ["echo", args...], so the
                // integration test sets `allowed_commands` accordingly.
                let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                match exec("echo", &args_refs) {
                    Ok(out) => {
                        // Print stdout verbatim so the test can assert on it.
                        let _ = print(&String::from_utf8_lossy(&out.stdout));
                        out.exit_code
                    }
                    Err(ErrorCode::Denied)            => 100,
                    Err(ErrorCode::PatternNotAllowed) => 101,
                    Err(ErrorCode::Timeout)           => 102,
                    Err(ErrorCode::NotFound)          => 103,
                    Err(_)                            => 1,
                }
            }
            _ => 127,
        }
    }

    fn hook_pre_exec(&mut self, command: &str) {
        record(format!("pre_exec:{}", command));
    }

    /// Note: NOT listed in `implemented_hooks`. The host should never invoke
    /// this method because the dispatch filter checks `implements_hook` first.
    /// If it ever does, this writes a sentinel the test will detect.
    fn hook_post_exec(&mut self, command: &str, exit_code: i32) {
        record(format!("post_exec:{}:{}", command, exit_code));
        let _ = set_var("YOSH_TEST_POST_EXEC_FIRED", "1");
    }

    fn hook_on_cd(&mut self, old_dir: &str, new_dir: &str) {
        record(format!("on_cd:{}:{}", old_dir, new_dir));
    }

    fn on_unload(&mut self) {
        record("on_unload");
    }
}

export!(TestPlugin);
