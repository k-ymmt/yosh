use criterion::{Criterion, black_box, criterion_group, criterion_main};
use kish::env::ShellEnv;
use kish::expand::{expand_word, expand_words};
use kish::parser::ast::{ParamExpr, Word, WordPart};

fn bench_expand(c: &mut Criterion) {
    c.bench_function("expand_param_default", |b| {
        b.iter(|| {
            let mut env = ShellEnv::new("kish", vec![]);
            env.vars.set("FOO", "hello").unwrap();
            let word = Word {
                parts: vec![WordPart::Parameter(ParamExpr::Default {
                    name: "BAR".to_string(),
                    word: Some(Word::literal("default_value")),
                    null_check: true,
                })],
            };
            for _ in 0..1000 {
                let _ = expand_word(black_box(&mut env), black_box(&word)).unwrap();
            }
        })
    });

    c.bench_function("expand_field_split", |b| {
        b.iter(|| {
            let mut env = ShellEnv::new("kish", vec![]);
            env.vars.set("IFS", ":").unwrap();
            env.vars.set("PATH", "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/go/bin:/home/user/.cargo/bin").unwrap();
            let word = Word {
                parts: vec![WordPart::Parameter(ParamExpr::Simple("PATH".to_string()))],
            };
            for _ in 0..1000 {
                let _ = expand_word(black_box(&mut env), black_box(&word)).unwrap();
            }
        })
    });

    c.bench_function("expand_literal_words", |b| {
        b.iter(|| {
            let mut env = ShellEnv::new("kish", vec![]);
            let words: Vec<Word> = (0..100)
                .map(|i| Word::literal(&format!("arg{}", i)))
                .collect();
            let _ = expand_words(black_box(&mut env), black_box(&words)).unwrap();
        })
    });
}

criterion_group!(benches, bench_expand);
criterion_main!(benches);
