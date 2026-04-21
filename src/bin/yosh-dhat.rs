//! yosh-dhat — dhat-instrumented binary that runs a yosh script in-process
//! with a custom global allocator for heap profiling.
//!
//! Build and run:
//!   cargo build --profile profiling --features dhat-heap --bin yosh-dhat
//!   ./target/profiling/yosh-dhat benches/data/script_heavy.sh
//!
//! Output: `dhat-heap.json` in CWD — open with https://nnethercote.github.io/dh_view/dh_view.html

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
    // Drop profiler explicitly before process::exit, which bypasses destructors.
    // Without this, dhat-heap.json would never be written.
    #[cfg(feature = "dhat-heap")]
    drop(_profiler);
    process::exit(status);
}
