use criterion::{Criterion, black_box, criterion_group, criterion_main};
use yosh::lexer::Lexer;

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

fn lex_all(input: &str) {
    let mut lexer = Lexer::new(input);
    loop {
        match lexer.next_token() {
            Ok(tok) => {
                if tok.token == yosh::lexer::token::Token::Eof {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

fn bench_lexer(c: &mut Criterion) {
    c.bench_function("lex_small", |b| {
        b.iter(|| lex_all(black_box(SMALL_SCRIPT)))
    });
    c.bench_function("lex_large", |b| {
        b.iter(|| lex_all(black_box(LARGE_SCRIPT)))
    });
}

criterion_group!(benches, bench_lexer);
criterion_main!(benches);
