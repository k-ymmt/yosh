use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use kish::env::ShellEnv;
use kish::plugin::PluginManager;

/// Serialize all plugin tests to avoid interference from the test plugin's
/// internal static Mutex (each load_plugin call resets the static, so parallel
/// tests that load the same .dylib can corrupt each other's state).
static TEST_LOCK: Mutex<()> = Mutex::new(());

fn build_test_plugin() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/plugins/test_plugin/Cargo.toml");
    let status = Command::new("cargo")
        .args(["build", "--manifest-path", manifest.to_str().unwrap()])
        .status()
        .expect("failed to run cargo build for test plugin");
    assert!(status.success(), "test plugin build failed");

    let target_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/plugins/test_plugin/target/debug");
    if cfg!(target_os = "macos") {
        target_dir.join("libtest_plugin.dylib")
    } else {
        target_dir.join("libtest_plugin.so")
    }
}

#[test]
fn load_plugin_successfully() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    manager.load_plugin(&dylib, &mut env).unwrap();
    assert!(manager.has_command("test-hello"));
    assert!(manager.has_command("test-set-var"));
    assert!(!manager.has_command("nonexistent"));
}

#[test]
fn exec_plugin_command() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    manager.load_plugin(&dylib, &mut env).unwrap();

    let status = manager.exec_command(&mut env, "test-hello", &[]);
    assert_eq!(status, Some(0));
    assert_eq!(env.vars.get("TEST_EXEC_CALLED"), Some("1"));
}

#[test]
fn exec_plugin_command_with_args() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    manager.load_plugin(&dylib, &mut env).unwrap();

    let status = manager.exec_command(
        &mut env,
        "test-set-var",
        &["MY_VAR".to_string(), "my_value".to_string()],
    );
    assert_eq!(status, Some(0));
    assert_eq!(env.vars.get("MY_VAR"), Some("my_value"));
}

#[test]
fn exec_unknown_command_returns_none() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    manager.load_plugin(&dylib, &mut env).unwrap();

    let status = manager.exec_command(&mut env, "nonexistent", &[]);
    assert_eq!(status, None);
}

#[test]
fn hook_pre_exec() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    manager.load_plugin(&dylib, &mut env).unwrap();

    manager.call_pre_exec(&mut env, "echo hello");
    assert_eq!(env.vars.get("TEST_PRE_EXEC"), Some("echo hello"));
}

#[test]
fn hook_post_exec() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    manager.load_plugin(&dylib, &mut env).unwrap();

    manager.call_post_exec(&mut env, "ls -la", 0);
    assert_eq!(env.vars.get("TEST_POST_EXEC"), Some("ls -la:0"));
}

#[test]
fn hook_on_cd() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    manager.load_plugin(&dylib, &mut env).unwrap();

    manager.call_on_cd(&mut env, "/old/dir", "/new/dir");
    assert_eq!(env.vars.get("TEST_ON_CD"), Some("/old/dir->/new/dir"));
}

#[test]
fn load_nonexistent_plugin_fails() {
    let _guard = TEST_LOCK.lock().unwrap();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    let result = manager.load_plugin(Path::new("/nonexistent/libfoo.dylib"), &mut env);
    assert!(result.is_err());
}

#[test]
fn readonly_var_rejected_by_plugin() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dylib = build_test_plugin();
    let mut manager = PluginManager::new();
    let mut env = ShellEnv::new("kish", vec![]);
    manager.load_plugin(&dylib, &mut env).unwrap();

    // Set a readonly variable
    let _ = env.vars.set("RO_VAR", "immutable");
    env.vars.set_readonly("RO_VAR");

    // Plugin tries to overwrite — set_var returns error code 1, but the plugin
    // ignores the result of set_var. The variable should be unchanged.
    let status = manager.exec_command(
        &mut env,
        "test-set-var",
        &["RO_VAR".to_string(), "changed".to_string()],
    );
    assert_eq!(env.vars.get("RO_VAR"), Some("immutable"));
    assert!(status.is_some());
}
