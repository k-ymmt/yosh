//! CLI for the `yosh-completions` binary, reachable as `yosh completions`
//! via the git-style subcommand delegation in `main.rs`. Inspects and
//! exports the completion specs embedded in the shell at compile time.

use crate::interactive::spec_completion::EMBEDDED_SPECS;
use std::path::PathBuf;

pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("-h") | Some("--help") => {
            print_help();
            0
        }
        Some("list") => {
            for (name, _) in EMBEDDED_SPECS {
                println!("{name}");
            }
            0
        }
        Some("export") => export(&args[1..]),
        Some(other) => {
            eprintln!("yosh-completions: unknown command `{other}`");
            eprintln!("Run 'yosh completions --help' for usage.");
            2
        }
    }
}

fn print_help() {
    println!("yosh-completions - manage yosh completion specs");
    println!();
    println!("Usage:  yosh completions <command> [args...]");
    println!();
    println!("Commands:");
    println!("  list                      List embedded completion specs");
    println!("  export [--force] <cmd>..  Copy embedded specs to ~/.config/yosh/completions/");
    println!("                            for customization (won't overwrite without --force)");
}

fn export(args: &[String]) -> i32 {
    let mut force = false;
    let mut names: Vec<&str> = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--force" => force = true,
            other if other.starts_with('-') => {
                eprintln!("yosh-completions: export: unknown option `{other}`");
                return 2;
            }
            other => names.push(other),
        }
    }
    if names.is_empty() {
        eprintln!("yosh-completions: export requires at least one command name");
        return 2;
    }
    let Some(home) = std::env::var_os("HOME") else {
        eprintln!("yosh-completions: HOME is not set");
        return 1;
    };
    let dir = PathBuf::from(home).join(".config/yosh/completions");

    let mut status = 0;
    for name in names {
        let Some((_, text)) = EMBEDDED_SPECS.iter().find(|(n, _)| *n == name) else {
            eprintln!(
                "yosh-completions: no embedded spec for `{name}` (see 'yosh completions list')"
            );
            status = 1;
            continue;
        };
        let path = dir.join(format!("{name}.toml"));
        if path.exists() && !force {
            eprintln!(
                "yosh-completions: {} already exists (use --force to overwrite)",
                path.display()
            );
            status = 1;
            continue;
        }
        let written = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, text));
        match written {
            Ok(()) => println!("{}", path.display()),
            Err(err) => {
                eprintln!("yosh-completions: {}: {err}", path.display());
                status = 1;
            }
        }
    }
    status
}
