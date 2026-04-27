//! plugin_bench — micro-benchmark baselines for the wasmtime plugin runtime.
//!
//! Measures one full `PluginManager::exec_command` round-trip for a
//! command that exercises one host import (`variables::get`). This sets
//! a regression baseline for the wasm boundary-crossing cost discussed
//! in spec §5 / re-review answer about boundary cost.
//!
//! Pre-requisite: `test_plugin.wasm` must already be built before
//! running this bench. Build it with:
//!
//!     cargo component build -p test_plugin --target wasm32-wasip2 --release
//!
//! The bench panics with a clear message if the wasm is missing, rather
//! than invoking cargo from inside the bench (which would dominate the
//! measurement on cold runs).

use std::path::{Path, PathBuf};

use criterion::{Criterion, black_box, criterion_group, criterion_main};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).into()
}

fn test_plugin_wasm() -> PathBuf {
    let p = workspace_root().join("target/wasm32-wasip2/release/test_plugin.wasm");
    assert!(
        p.exists(),
        "test_plugin.wasm not found at {}; build it first with \
         `cargo component build -p test_plugin --target wasm32-wasip2 --release`",
        p.display()
    );
    p
}

/// Set up a manager with test_plugin loaded and full capabilities granted.
/// Returns the manager and a long-lived ShellEnv. Both must outlive the
/// bench iter loop.
fn make_loaded_manager() -> (yosh::plugin::PluginManager, yosh::env::ShellEnv) {
    let mut env = yosh::env::ShellEnv::new("yosh", Vec::new());
    env.vars
        .set("BENCH_VAR", "bench_value")
        .expect("set BENCH_VAR");
    let mut mgr = yosh::plugin::PluginManager::new();
    yosh::plugin::test_helpers::load_plugin_with_caps(
        &mut mgr,
        &test_plugin_wasm(),
        &mut env,
        yosh_plugin_api::CAP_ALL,
    )
    .expect("load test_plugin");
    (mgr, env)
}

fn bench_exec_no_host_call(c: &mut Criterion) {
    // `test_cmd` writes to stdout and returns 0. One host import call
    // (io.write) per iteration, so this measures roughly: 1 boundary
    // crossing in (exec) + 1 boundary crossing out (write) + the
    // command body overhead.
    let (mut mgr, mut env) = make_loaded_manager();
    let args: Vec<String> = vec!["bench".into()];
    c.bench_function("plugin_exec_test_cmd", |b| {
        b.iter(|| {
            let r = mgr.exec_command(&mut env, "test_cmd", black_box(&args));
            black_box(r);
        });
    });
}

fn bench_exec_with_var_get(c: &mut Criterion) {
    // `echo_var BENCH_VAR` calls variables.get + io.write inside the
    // guest. Measures: 1 exec boundary + 1 variables.get boundary +
    // 1 io.write boundary + the command body overhead.
    let (mut mgr, mut env) = make_loaded_manager();
    let args: Vec<String> = vec!["BENCH_VAR".into()];
    c.bench_function("plugin_exec_echo_var", |b| {
        b.iter(|| {
            let r = mgr.exec_command(&mut env, "echo_var", black_box(&args));
            black_box(r);
        });
    });
}

fn bench_hook_pre_exec(c: &mut Criterion) {
    // `call_pre_exec` dispatches to test_plugin's hook_pre_exec.
    // Measures one hook boundary crossing under the implemented-hooks
    // dispatch filter.
    let (mut mgr, mut env) = make_loaded_manager();
    c.bench_function("plugin_hook_pre_exec", |b| {
        b.iter(|| {
            mgr.call_pre_exec(&mut env, black_box("noop"));
        });
    });
}

criterion_group!(plugin_benches,
    bench_exec_no_host_call,
    bench_exec_with_var_get,
    bench_hook_pre_exec,
);
criterion_main!(plugin_benches);
