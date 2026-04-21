//! startup_bench — measures the wall-clock cost of a one-shot yosh invocation.
//!
//! Because startup cost involves the full OS process lifecycle (fork/exec,
//! libc init, dynamic linker, our own init), we invoke yosh as an external
//! subprocess per iteration. This is slow but accurate.

use std::process::{Command, Stdio};

use criterion::{Criterion, black_box, criterion_group, criterion_main};

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

criterion_group!(benches, bench_startup_echo);
criterion_main!(benches);
