//! yosh-dhat — dhat-instrumented binary that runs a yosh script in-process
//! with a custom global allocator for heap profiling.
//!
//! Build and run:
//!   cargo build --profile profiling --features dhat-heap --bin yosh-dhat
//!   ./target/profiling/yosh-dhat benches/data/script_heavy.sh
//!
//! Output: `dhat-heap.json` in CWD — open with https://nnethercote.github.io/dh_view/dh_view.html
//!
//! Divergence from `src/main.rs::run_string`: this binary uses a single
//! whole-program `parse_program` + `exec_program` pass instead of the
//! per-command parse loop in main. Alias expansion (which main wires via
//! `Parser::new_with_aliases_at_line`) is intentionally omitted — the W2
//! workload does not use aliases, and keeping the pipeline short keeps
//! the profile focused on parse/expand/exec cost.

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use std::process;

fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: {} <script-path>", args[0]);
        process::exit(2);
    }
    let script_path = &args[1];

    let input = match std::fs::read_to_string(script_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("yosh-dhat: {}: {}", script_path, e);
            process::exit(127);
        }
    };

    yosh::signal::init_signal_handling();
    let mut executor = yosh::exec::Executor::new("yosh-dhat", vec![]);
    yosh::env::default_path::ensure_default_path(&mut executor.env);
    executor.load_plugins();

    let program = match yosh::parser::Parser::new(&input).parse_program() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("yosh-dhat: parse error: {}", e);
            process::exit(2);
        }
    };

    let status = executor.exec_program(&program);

    executor.process_pending_signals();
    executor.execute_exit_trap();

    // Drop profiler explicitly before process::exit — std::process::exit
    // bypasses Rust destructors, so without this the Drop impl that writes
    // `dhat-heap.json` would never run.
    #[cfg(feature = "dhat-heap")]
    drop(_profiler);
    process::exit(status);
}
