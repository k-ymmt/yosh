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

    let status = match args.get(1).map(|s| s.as_str()) {
        Some("--exec-loop") => run_exec_loop(&args[2..]),
        Some("--pre-prompt-loop") => run_pre_prompt_loop(&args[2..]),
        Some(_) => run_script(&args[1]),
        None => {
            eprintln!(
                "usage: {bin} <script-path>\n       {bin} --exec-loop N CMD [ARG ...]\n       {bin} --pre-prompt-loop N",
                bin = args.first().map(String::as_str).unwrap_or("yosh-dhat")
            );
            2
        }
    };

    // Drop profiler explicitly before process::exit — std::process::exit
    // bypasses Rust destructors, so without this the Drop impl that writes
    // `dhat-heap.json` would never run.
    #[cfg(feature = "dhat-heap")]
    drop(_profiler);
    process::exit(status);
}

fn run_script(script_path: &str) -> i32 {
    let input = match std::fs::read_to_string(script_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("yosh-dhat: {}: {}", script_path, e);
            return 127;
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
            return 2;
        }
    };

    let status = executor.exec_program(&program);
    executor.process_pending_signals();
    executor.execute_exit_trap();
    status
}

fn run_exec_loop(args: &[String]) -> i32 {
    let n: u32 = match args.first().and_then(|s| s.parse().ok()) {
        Some(n) if n > 0 => n,
        _ => {
            eprintln!("yosh-dhat: --exec-loop: missing or invalid N (positive integer)");
            return 2;
        }
    };
    let cmd = match args.get(1) {
        Some(c) => c.clone(),
        None => {
            eprintln!("yosh-dhat: --exec-loop: missing CMD");
            return 2;
        }
    };
    let cmd_args: Vec<String> = args.iter().skip(2).cloned().collect();

    yosh::signal::init_signal_handling();
    let mut executor = yosh::exec::Executor::new("yosh-dhat", vec![]);
    yosh::env::default_path::ensure_default_path(&mut executor.env);
    executor.load_plugins();

    let mut last_status = 0;
    for _ in 0..n {
        let r = executor.plugins.exec_command(&mut executor.env, &cmd, &cmd_args);
        last_status = match r {
            yosh::plugin::PluginExec::Handled(code) => code,
            yosh::plugin::PluginExec::NotHandled => 127,
            yosh::plugin::PluginExec::Failed => 1,
        };
    }
    last_status
}

fn run_pre_prompt_loop(_args: &[String]) -> i32 {
    eprintln!("yosh-dhat: --pre-prompt-loop: not yet implemented");
    2
}
