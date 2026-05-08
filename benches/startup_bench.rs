//! startup_bench — measures the wall-clock cost of a one-shot yosh invocation.
//!
//! Because startup cost involves the full OS process lifecycle (fork/exec,
//! libc init, dynamic linker, our own init), we invoke yosh as an external
//! subprocess per iteration. This is slow but accurate.

use std::process::{Command, Stdio};

use criterion::{Criterion, black_box, criterion_group, criterion_main};

#[path = "plugin_bench_helpers.rs"]
mod plugin_bench_helpers;

fn yosh_binary() -> String {
    // Tests and benches that need the compiled binary can use the
    // CARGO_BIN_EXE_<name> env var that Cargo sets for bench targets.
    // When that is unavailable (e.g., running the binary under samply
    // later), fall back to the profiling profile path.
    option_env!("CARGO_BIN_EXE_yosh")
        .map(String::from)
        .unwrap_or_else(|| "./target/profiling/yosh".to_string())
}

fn bench_startup_echo(c: &mut Criterion) {
    let yosh = yosh_binary();
    c.bench_function("startup_echo_hi", |b| {
        b.iter(|| {
            let status = Command::new(black_box(&yosh))
                .args(["-c", "echo hi"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("failed to spawn yosh");
            assert!(status.success(), "yosh -c 'echo hi' failed");
        });
    });
}

fn bench_startup_with_n_plugins(c: &mut Criterion, n: usize, name: &str) {
    let yosh = yosh_binary();
    let home = plugin_bench_helpers::stage_home_with_plugin(n);
    let home_path = home.path().to_owned();

    c.bench_function(name, |b| {
        b.iter(|| {
            let status = Command::new(black_box(&yosh))
                .args(["-c", "echo hi"])
                .env("HOME", &home_path)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("failed to spawn yosh");
            assert!(status.success(), "yosh -c 'echo hi' failed");
        });
    });
    drop(home); // explicit; tmpdir cleans up here
}

fn bench_startup_one_plugin(c: &mut Criterion) {
    bench_startup_with_n_plugins(c, 1, "startup_one_plugin");
}

fn bench_startup_three_plugins(c: &mut Criterion) {
    bench_startup_with_n_plugins(c, 3, "startup_three_plugins");
}

criterion_group!(
    benches,
    bench_startup_echo,
    bench_startup_one_plugin,
    bench_startup_three_plugins,
);
criterion_main!(benches);
