//! plugin_bench — micro-benchmark baselines for the wasmtime plugin runtime.
//!
//! Measures one full `PluginManager::exec_command` and hook call round-trips
//! using perf_plugin to avoid stdout pollution from `test_plugin`'s print() calls.
//!
//! Pre-requisite: `perf_plugin.wasm` must already be built before
//! running this bench. Build it with:
//!
//!     cargo component build -p perf_plugin --target wasm32-wasip2 --release
//!
//! The bench panics with a clear message if the wasm is missing, rather
//! than invoking cargo from inside the bench (which would dominate the
//! measurement on cold runs).

mod plugin_bench_helpers;

use criterion::{Criterion, black_box, criterion_group, criterion_main};

/// Set up a manager with perf_plugin loaded and full capabilities granted.
/// Returns the manager and a long-lived ShellEnv. Both must outlive the
/// bench iter loop.
fn make_loaded_manager() -> (yosh::plugin::PluginManager, yosh::env::ShellEnv) {
    let mut env = yosh::env::ShellEnv::new("yosh", Vec::new());
    env.vars
        .set("PERF_VAR", "perf_value")
        .expect("set PERF_VAR");
    let mut mgr = yosh::plugin::PluginManager::new();
    yosh::plugin::test_helpers::load_plugin_with_caps(
        &mut mgr,
        &plugin_bench_helpers::perf_plugin_wasm(),
        &mut env,
        yosh_plugin_api::CAP_ALL,
        &[],
    )
    .expect("load perf_plugin");
    (mgr, env)
}

/// Set up a manager with n independent perf_plugin instances loaded
/// and full capabilities granted. Returns the manager and a long-lived
/// ShellEnv. Both must outlive the bench iter loop.
fn make_manager_with_n_plugins(n: usize) -> (yosh::plugin::PluginManager, yosh::env::ShellEnv) {
    let mut env = yosh::env::ShellEnv::new("yosh", Vec::new());
    let mut mgr = yosh::plugin::PluginManager::new();
    let wasm = plugin_bench_helpers::perf_plugin_wasm();
    for _ in 0..n {
        yosh::plugin::test_helpers::load_plugin_with_caps(
            &mut mgr,
            &wasm,
            &mut env,
            yosh_plugin_api::CAP_ALL,
            &[],
        )
        .expect("load perf_plugin");
    }
    (mgr, env)
}

fn bench_exec_noop_cmd(c: &mut Criterion) {
    // `noop_cmd` performs minimal work and returns 0 with no output.
    // Measures: exec boundary + command body overhead (baseline).
    let (mut mgr, mut env) = make_loaded_manager();
    let args: Vec<String> = vec![];
    c.bench_function("plugin_exec_noop_cmd", |b| {
        b.iter(|| {
            let r = mgr.exec_command(&mut env, "noop_cmd", black_box(&args));
            black_box(r);
        });
    });
}

fn bench_exec_noop_var(c: &mut Criterion) {
    // `noop_var` calls variables.get(PERF_VAR) inside the guest.
    // Measures: exec boundary + variables.get boundary + command body overhead.
    let (mut mgr, mut env) = make_loaded_manager();
    let args: Vec<String> = vec![];
    c.bench_function("plugin_exec_noop_var", |b| {
        b.iter(|| {
            let r = mgr.exec_command(&mut env, "noop_var", black_box(&args));
            black_box(r);
        });
    });
}

fn bench_hook_pre_exec(c: &mut Criterion) {
    // `call_pre_exec` dispatches to perf_plugin's hook_pre_exec.
    // Measures one hook boundary crossing under the implemented-hooks
    // dispatch filter.
    let (mut mgr, mut env) = make_loaded_manager();
    c.bench_function("plugin_hook_pre_exec", |b| {
        b.iter(|| {
            mgr.call_pre_exec(&mut env, black_box("noop"));
        });
    });
}

fn bench_pre_prompt_zero_plugins(c: &mut Criterion) {
    // Baseline: `call_pre_prompt` with no plugins loaded.
    // Measures: dispatch mechanism overhead with zero hooks.
    let (mut mgr, mut env) = make_manager_with_n_plugins(0);
    c.bench_function("plugin_pre_prompt_zero_plugins", |b| {
        b.iter(|| {
            mgr.call_pre_prompt(black_box(&mut env));
        });
    });
}

fn bench_pre_prompt_one_noop(c: &mut Criterion) {
    // One perf_plugin loaded with empty pre_prompt hook.
    // Measures: dispatch + one noop hook boundary crossing.
    let (mut mgr, mut env) = make_manager_with_n_plugins(1);
    c.bench_function("plugin_pre_prompt_one_noop", |b| {
        b.iter(|| {
            mgr.call_pre_prompt(black_box(&mut env));
        });
    });
}

fn bench_pre_prompt_three_noop(c: &mut Criterion) {
    // Three independent perf_plugin instances with empty pre_prompt hooks.
    // Measures: dispatch + three noop hook boundary crossings.
    let (mut mgr, mut env) = make_manager_with_n_plugins(3);
    c.bench_function("plugin_pre_prompt_three_noop", |b| {
        b.iter(|| {
            mgr.call_pre_prompt(black_box(&mut env));
        });
    });
}

criterion_group!(
    plugin_benches,
    bench_exec_noop_cmd,
    bench_exec_noop_var,
    bench_hook_pre_exec,
    bench_pre_prompt_zero_plugins,
    bench_pre_prompt_one_noop,
    bench_pre_prompt_three_noop,
);
criterion_main!(plugin_benches);
