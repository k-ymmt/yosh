use std::process;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(|s| s.as_str()) {
        Some("sync") => { eprintln!("sync: not yet implemented"); 2 }
        Some("update") => { eprintln!("update: not yet implemented"); 2 }
        Some("list") => { eprintln!("list: not yet implemented"); 2 }
        Some("verify") => { eprintln!("verify: not yet implemented"); 2 }
        Some(cmd) => { eprintln!("kish-plugin: unknown command '{}'", cmd); 2 }
        None => { eprintln!("usage: kish-plugin <sync|update|list|verify>"); 2 }
    };
    process::exit(code);
}
