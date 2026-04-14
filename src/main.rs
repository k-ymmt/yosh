mod builtin;
mod env;
mod error;
mod exec;
mod expand;
mod lexer;
mod parser;
mod signal;
mod interactive;
mod plugin;

use std::env as std_env;
use std::fs;
use std::io::{self, Read};
use std::process;

use exec::Executor;
use owo_colors::OwoColorize;

fn should_colorize() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if let Some(val) = std::env::var_os("CLICOLOR_FORCE") {
        if val != "0" {
            return true;
        }
    }
    nix::unistd::isatty(std::io::stdout()).unwrap_or(false)
}

fn print_help() {
    let color = should_colorize();

    let header = "kish - A POSIX-compliant shell";
    if color {
        println!("{}", header.bold());
    } else {
        println!("{}", header);
    }
    println!();

    if color {
        println!("{}  kish [options] [file [argument...]]", "Usage:".yellow().bold());
    } else {
        println!("Usage:  kish [options] [file [argument...]]");
    }
    println!();

    if color {
        println!("{}", "Options:".yellow().bold());
        println!("  {}    Read commands from command_string", "-c <command>".green());
        println!("  {}  Parse and dump AST (debug)", "--parse <code>".green());
        println!("  {}     Show this help message", "-h, --help".green());
        println!("  {}       Show version information", "--version".green());
    } else {
        println!("Options:");
        println!("  -c <command>    Read commands from command_string");
        println!("  --parse <code>  Parse and dump AST (debug)");
        println!("  -h, --help      Show this help message");
        println!("  --version       Show version information");
    }
    println!();

    if color {
        println!("{}", "Subcommands:".yellow().bold());
        println!("  {}          Manage shell plugins (see '{}')",
            "plugin".green(), "kish plugin --help".green());
    } else {
        println!("Subcommands:");
        println!("  plugin          Manage shell plugins (see 'kish plugin --help')");
    }
}

fn print_version() {
    println!("kish {} ({} {})",
        env!("CARGO_PKG_VERSION"),
        env!("KISH_GIT_HASH"),
        env!("KISH_BUILD_DATE"));
}

fn main() {
    let args: Vec<String> = std_env::args().collect();
    let shell_name = args.first().map_or("kish".to_string(), |a| a.clone());

    match args.len() {
        1 => {
            if nix::unistd::isatty(std::io::stdin()).unwrap_or(false) {
                let mut repl = interactive::Repl::new(shell_name);
                process::exit(repl.run());
            } else {
                // stdin is a pipe — read as script
                let mut input = String::new();
                io::stdin().read_to_string(&mut input).unwrap_or_else(|e| {
                    eprintln!("kish: {}", e);
                    process::exit(1);
                });
                let status = run_string(&input, shell_name, vec![], false);
                process::exit(status);
            }
        }
        _ => {
            if args[1] == "--help" || args[1] == "-h" {
                print_help();
                process::exit(0);
            } else if args[1] == "--version" {
                print_version();
                process::exit(0);
            } else if args[1] == "-c" {
                if args.len() < 3 {
                    eprintln!("kish: -c requires an argument");
                    process::exit(2);
                }
                // POSIX: sh -c cmd [name [arg...]]
                // After the script, the next arg is $0 (shell_name), remaining are $1, $2, ...
                // Support `--` as an optional separator before positional args.
                let rest_start = if args.len() > 3 && args[3] == "--" { 4 } else { 3 };
                let sn = if rest_start < args.len() { args[rest_start].clone() } else { shell_name };
                let positional: Vec<String> = if rest_start + 1 < args.len() { args[rest_start + 1..].to_vec() } else { vec![] };
                let status = run_string(&args[2], sn, positional, true);
                process::exit(status);
            } else if args[1] == "--parse" {
                if args.len() < 3 {
                    eprintln!("kish: --parse requires an argument");
                    process::exit(2);
                }
                let input = if args[2] == "-" {
                    let mut buf = String::new();
                    io::stdin().read_to_string(&mut buf).unwrap();
                    buf
                } else {
                    args[2].clone()
                };
                match parser::Parser::new(&input).parse_program() {
                    Ok(ast) => println!("{:#?}", ast),
                    Err(e) => { eprintln!("{}", e); process::exit(2); }
                }
            } else if let Some(status) = try_subcommand(&args[1..]) {
                process::exit(status);
            } else {
                let positional: Vec<String> = args[2..].to_vec();
                let status = run_file(&args[1], shell_name, positional);
                process::exit(status);
            }
        }
    }
}

/// Try to delegate `kish <sub> [args...]` to `kish-<sub>` binary in PATH.
/// Returns Some(exit_status) if a matching binary was found and executed.
fn try_subcommand(args: &[String]) -> Option<i32> {
    let sub = args.first()?;
    // Skip anything that looks like a flag or a file path.
    if sub.starts_with('-') || sub.contains('/') || sub.contains('.') {
        return None;
    }
    let bin_name = format!("kish-{}", sub);
    let found = std_env::var_os("PATH").and_then(|paths| {
        std_env::split_paths(&paths).find(|dir| dir.join(&bin_name).is_file())
    });
    let bin_path = found?.join(&bin_name);
    let status = process::Command::new(bin_path)
        .args(&args[1..])
        .status()
        .unwrap_or_else(|e| {
            eprintln!("kish: {}: {}", bin_name, e);
            process::exit(126);
        });
    Some(status.code().unwrap_or(1))
}

fn run_string(input: &str, shell_name: String, positional: Vec<String>, cmd_string: bool) -> i32 {
    signal::init_signal_handling();
    let mut executor = Executor::new(shell_name, positional);
    executor.load_plugins();
    executor.env.mode.options.cmd_string = cmd_string;
    executor.verbose_print(input);

    // Parse and execute one complete command at a time so that aliases
    // defined by earlier commands are available for later ones.
    let mut remaining = input;
    let mut status = 0;

    loop {
        // Skip leading whitespace and newlines
        let trimmed = remaining.trim_start_matches([' ', '\t', '\n']);
        if trimmed.is_empty() {
            break;
        }
        remaining = trimmed;

        let mut p = parser::Parser::new_with_aliases(remaining, &executor.env.aliases);
        if p.is_at_end() {
            break;
        }
        match p.parse_complete_command() {
            Ok(cmd) => {
                let consumed = p.consumed_bytes();
                // Advance remaining past the consumed bytes
                if consumed == 0 {
                    // Nothing consumed — avoid infinite loop.
                    // This can happen if parse_complete_command succeeds but
                    // the look-ahead didn't advance. Break out.
                    break;
                }
                drop(p);
                status = executor.exec_complete_command(&cmd);
                // Check for flow control (exit handled by std::process::exit in builtin)
                if executor.env.exec.flow_control.is_some() {
                    break;
                }
                executor.check_errexit(status);
                remaining = &remaining[consumed..];
            }
            Err(e) => {
                eprintln!("{}", e);
                executor.process_pending_signals();
                executor.execute_exit_trap();
                return 2;
            }
        }
    }

    executor.process_pending_signals();
    executor.execute_exit_trap();
    status
}

fn run_file(path: &str, shell_name: String, positional: Vec<String>) -> i32 {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => { eprintln!("kish: {}: {}", path, e); return 127; }
    };
    run_string(&content, shell_name, positional, false)
}
