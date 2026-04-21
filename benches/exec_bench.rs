//! exec_bench — in-process micro-benchmarks for W2 pipeline components.
//! Unlike startup_bench (subprocess), these run the shell pipeline through
//! the library API so that parse + expand + exec costs are isolated.

use criterion::{Criterion, black_box, criterion_group, criterion_main};

fn run_script(src: &str) -> i32 {
    let mut executor = yosh::exec::Executor::new("exec_bench", vec![]);
    yosh::env::default_path::ensure_default_path(&mut executor.env);
    let program = yosh::parser::Parser::new(src)
        .parse_program()
        .expect("parse failed");
    executor.exec_program(&program)
}

const LOOP_SCRIPT: &str = r#"
sum=0
for i in $(seq 1 200); do
    sum=$((sum + i))
done
"#;

const FUNCTION_SCRIPT: &str = r#"
f() { : "$1"; }
i=0
while [ "$i" -lt 200 ]; do
    f arg
    i=$((i + 1))
done
"#;

const EXPANSION_SCRIPT: &str = r#"
VAR="hello world"
UNSET=""
for _ in $(seq 1 200); do
    : "${UNSET:-fallback}"
    : "${VAR#hello }"
    : "${VAR%world}"
    : "${#VAR}"
done
"#;

fn bench_exec(c: &mut Criterion) {
    c.bench_function("exec_for_loop_200", |b| {
        b.iter(|| {
            let status = run_script(black_box(LOOP_SCRIPT));
            assert_eq!(status, 0);
        });
    });

    c.bench_function("exec_function_call_200", |b| {
        b.iter(|| {
            let status = run_script(black_box(FUNCTION_SCRIPT));
            assert_eq!(status, 0);
        });
    });

    c.bench_function("exec_param_expansion_200", |b| {
        b.iter(|| {
            let status = run_script(black_box(EXPANSION_SCRIPT));
            assert_eq!(status, 0);
        });
    });
}

criterion_group!(benches, bench_exec);
criterion_main!(benches);
