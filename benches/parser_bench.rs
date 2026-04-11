use criterion::{Criterion, black_box, criterion_group, criterion_main};
use kish::parser::Parser;

const SMALL_SCRIPT: &str = r#"
echo hello world
FOO=bar
echo "$FOO"
ls -la /tmp
if [ -f /etc/hosts ]; then echo found; fi
cat file.txt | grep pattern | wc -l
A=1; B=2; echo $((A + B))
cd /tmp && pwd
export PATH="/usr/bin:$PATH"
for i in 1 2 3; do echo "$i"; done
"#;

const LARGE_SCRIPT: &str = include_str!("data/large_script.sh");

fn parse_all(input: &str) {
    let mut parser = Parser::new(input);
    let _ = parser.parse_program();
}

fn bench_parser(c: &mut Criterion) {
    c.bench_function("parse_small", |b| {
        b.iter(|| parse_all(black_box(SMALL_SCRIPT)))
    });
    c.bench_function("parse_large", |b| {
        b.iter(|| parse_all(black_box(LARGE_SCRIPT)))
    });
}

criterion_group!(benches, bench_parser);
criterion_main!(benches);
