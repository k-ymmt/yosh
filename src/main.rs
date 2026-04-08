mod builtin;
mod env;
mod error;
mod exec;
mod expand;
mod lexer;
mod parser;

use std::env as std_env;
use std::fs;
use std::io::{self, Read};
use std::process;

use exec::Executor;

fn main() {
    let args: Vec<String> = std_env::args().collect();
    let shell_name = args.first().map_or("kish".to_string(), |a| a.clone());

    match args.len() {
        1 => {
            eprintln!("kish: interactive mode not yet implemented");
            process::exit(1);
        }
        _ => {
            if args[1] == "-c" {
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
                let status = run_string(&args[2], sn, positional);
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
            } else {
                let positional: Vec<String> = args[2..].to_vec();
                let status = run_file(&args[1], shell_name, positional);
                process::exit(status);
            }
        }
    }
}

fn run_string(input: &str, shell_name: String, positional: Vec<String>) -> i32 {
    let mut executor = Executor::new(shell_name, positional);
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
                if executor.env.flow_control.is_some() {
                    break;
                }
                remaining = &remaining[consumed..];
            }
            Err(e) => {
                eprintln!("{}", e);
                execute_exit_trap(&mut executor);
                return 2;
            }
        }
    }

    execute_exit_trap(&mut executor);
    status
}

fn execute_exit_trap(executor: &mut Executor) {
    if let Some(env::TrapAction::Command(cmd)) = executor.env.traps.exit_trap.take()
        && let Ok(program) = parser::Parser::new(&cmd).parse_program()
    {
        executor.exec_program(&program);
    }
}

fn run_file(path: &str, shell_name: String, positional: Vec<String>) -> i32 {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => { eprintln!("kish: {}: {}", path, e); return 127; }
    };
    run_string(&content, shell_name, positional)
}
