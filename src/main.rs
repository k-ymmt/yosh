mod env;
mod error;
mod expand;
mod lexer;
mod parser;

use std::fs;
use std::io::{self, Read};
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();

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
                run_string(&args[2]);
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
                    Err(e) => {
                        eprintln!("{}", e);
                        process::exit(2);
                    }
                }
            } else {
                run_file(&args[1]);
            }
        }
    }
}

fn run_string(input: &str) {
    match parser::Parser::new(input).parse_program() {
        Ok(ast) => println!("{:#?}", ast),
        Err(e) => {
            eprintln!("{}", e);
            process::exit(2);
        }
    }
}

fn run_file(path: &str) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("kish: {}: {}", path, e);
            process::exit(127);
        }
    };
    run_string(&content);
}
