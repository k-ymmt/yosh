mod builtin;
mod byteenc;
mod env;
mod error;
mod exec;
mod expand;
mod interactive;
mod lexer;
mod parser;
mod plugin;
mod signal;
#[cfg(test)]
mod test_sync;

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
    if let Some(val) = std::env::var_os("CLICOLOR_FORCE")
        && val != "0"
    {
        return true;
    }
    if let Some(val) = std::env::var_os("CLICOLOR")
        && val == "0"
    {
        return false;
    }
    nix::unistd::isatty(std::io::stdout()).unwrap_or(false)
}

fn print_help() {
    let color = should_colorize();

    let header = "yosh - A POSIX-compliant shell";
    if color {
        println!("{}", header.bold());
    } else {
        println!("{}", header);
    }
    println!();

    if color {
        println!(
            "{}  yosh [options] [file [argument...]]",
            "Usage:".yellow().bold()
        );
    } else {
        println!("Usage:  yosh [options] [file [argument...]]");
    }
    println!();

    struct HelpSection {
        heading: &'static str,
        items: &'static [(&'static str, &'static str)],
    }

    const SECTIONS: &[HelpSection] = &[
        HelpSection {
            heading: "Options",
            items: &[
                ("-c <command>", "Read commands from command_string"),
                ("-i", "Force the shell to be interactive"),
                (
                    "-s [arg...]",
                    "Read commands from stdin; args become $1, $2, ...",
                ),
                (
                    "-abCefhmnuvx",
                    "Set shell option flags (prefix + to unset; see 'set')",
                ),
                (
                    "-o <option>",
                    "Set a shell option by name (+o <option> unsets)",
                ),
                ("--parse <code>", "Parse and dump AST (debug)"),
                ("-h, --help", "Show this help message"),
                ("--version", "Show version information"),
            ],
        },
        HelpSection {
            heading: "Subcommands",
            items: &[
                ("plugin", "Manage shell plugins (see 'yosh plugin --help')"),
                (
                    "completions",
                    "Manage completion specs (see 'yosh completions --help')",
                ),
            ],
        },
    ];

    for section in SECTIONS {
        if color {
            println!("{}", format!("{}:", section.heading).yellow().bold());
        } else {
            println!("{}:", section.heading);
        }
        for &(flag, desc) in section.items {
            if color {
                println!("  {}  {}", flag.green(), desc);
            } else {
                println!("  {:16}{}", flag, desc);
            }
        }
        println!();
    }
}

fn print_version() {
    println!(
        "yosh {} ({} {})",
        env!("CARGO_PKG_VERSION"),
        env!("YOSH_GIT_HASH"),
        env!("YOSH_BUILD_DATE")
    );
}

fn main() {
    // args_os + byteenc: non-UTF-8 argv values (script paths, -c operands,
    // positional parameters) are preserved losslessly instead of panicking
    // or being dropped by `args()`.
    let args: Vec<String> = {
        use std::os::unix::ffi::OsStrExt;
        std_env::args_os()
            .map(|a| byteenc::encode_bytes(a.as_bytes()).into_owned())
            .collect()
    };
    let shell_name = args.first().map_or("yosh".to_string(), |a| a.clone());

    // Long options and subcommand-style dispatch keep their historical
    // args[1]-position behavior.
    if args.len() > 1 {
        // Bare `-h` doubles as help only when it is the sole argument;
        // with anything after it, it is the POSIX locate-utilities
        // no-op handled by the option loop (`yosh -h script` must run
        // the script).
        if args[1] == "--help" || (args[1] == "-h" && args.len() == 2) {
            print_help();
            process::exit(0);
        } else if args[1] == "--version" {
            print_version();
            process::exit(0);
        } else if args[1] == "--parse" {
            if args.len() < 3 {
                eprintln!("yosh: --parse requires an argument");
                process::exit(2);
            }
            let input = if args[2] == "-" {
                let mut raw = Vec::new();
                io::stdin().read_to_end(&mut raw).unwrap();
                byteenc::encode_bytes(&raw).into_owned()
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
            process::exit(0);
        }
    }

    // Leading option parsing (POSIX XCU sh SYNOPSIS): the set-option
    // letters (-a -b -C -e -f -h -m -n -u -v -x, singly or clustered,
    // `+` prefix to unset), `-o <option>` / `+o <option>` long names,
    // `-c` (command string), `-s` (read stdin; operands become the
    // positional parameters), and `-i` / `+i` (force interactive
    // on/off). `--` — or a lone `-`, its obsolescent synonym — ends
    // option parsing. Note `-h` as args[1] is claimed by the help
    // dispatch above; in clusters or later positions it is the POSIX
    // locate-utilities no-op.
    let mut idx = 1;
    // Tri-state interactive override: None = unspecified (tty
    // auto-detection), Some(true) = -i forces interactive, Some(false)
    // = +i forces non-interactive even on a terminal (bash agrees:
    // `bash +i` at a tty reads stdin as a script, no REPL).
    let mut force_interactive: Option<bool> = None;
    let mut cmd_mode = false;
    let mut read_stdin = false;
    let mut invocation_ops: Vec<env::InvocationOp> = Vec::new();
    // Parse-time validation target so a bad `-o <name>` fails here with
    // a usage error instead of surfacing mid-startup.
    let mut probe = env::ShellOptions::default();
    while idx < args.len() {
        let arg = &args[idx];
        if arg == "--" || arg == "-" {
            idx += 1;
            break;
        }
        let (on, cluster) = match arg.as_bytes().first() {
            // A lone `+` is an empty cluster: consumed as a no-op
            // (bash agrees); a lone `-` was handled above.
            Some(b'-') => (true, &arg[1..]),
            Some(b'+') => (false, &arg[1..]),
            _ => break,
        };
        // `--long` words other than the exact `--` terminator are not
        // supported here (the args[1] dispatch above handles the yosh
        // extensions); report the whole word, not its first character.
        if on && cluster.starts_with('-') {
            invocation_usage_error(&format!("{}: invalid option", arg));
        }
        idx += 1;
        let sign = if on { '-' } else { '+' };
        for (pos, c) in cluster.char_indices() {
            match c {
                'c' if on => cmd_mode = true,
                's' if on => read_stdin = true,
                'i' => force_interactive = Some(on),
                'o' => {
                    // Option name: rest of the cluster (`-opipefail`) or
                    // the next argument (`-o pipefail`).
                    let attached = &cluster[pos + 1..];
                    let name = if !attached.is_empty() {
                        attached.to_string()
                    } else if idx < args.len() {
                        idx += 1;
                        args[idx - 1].clone()
                    } else {
                        invocation_usage_error(&format!("{}o: option requires an argument", sign));
                    };
                    if let Err(e) = probe.set_by_name(&name, on) {
                        invocation_usage_error(&e);
                    }
                    invocation_ops.push(env::InvocationOp::Long(name, on));
                    break;
                }
                // ShellOptions::set_by_char is the single authority on
                // which set-option letters exist; the arms above cover
                // exactly the invocation-only options (-c, -s, -i, -o).
                _ => match probe.set_by_char(c, on) {
                    Ok(()) => invocation_ops.push(env::InvocationOp::Short(c, on)),
                    Err(_) => invocation_usage_error(&format!("{}{}: invalid option", sign, c)),
                },
            }
        }
    }

    if cmd_mode {
        if idx >= args.len() {
            invocation_usage_error("-c requires an argument");
        }
        // POSIX: sh -c cmd [name [arg...]]
        // After the script, the next arg is $0 (shell_name), remaining are $1, $2, ...
        // Support `--` as an optional separator before positional args.
        let command = args[idx].clone();
        let mut rest_start = idx + 1;
        if rest_start < args.len() && args[rest_start] == "--" {
            rest_start += 1;
        }
        let sn = if rest_start < args.len() {
            args[rest_start].clone()
        } else {
            shell_name
        };
        let positional: Vec<String> = if rest_start + 1 < args.len() {
            args[rest_start + 1..].to_vec()
        } else {
            vec![]
        };
        let status = run_string(
            &command,
            sn,
            positional,
            // -s alongside -c still shows `s` in $- (bash agrees),
            // though commands come from the string.
            (true, read_stdin),
            force_interactive == Some(true),
            &invocation_ops,
        );
        process::exit(status);
    }

    if read_stdin {
        // POSIX `sh -s [arg...]`: commands come from standard input and
        // every remaining operand is a positional parameter; $0 stays
        // the shell name.
        let positional: Vec<String> = args[idx..].to_vec();
        run_stdin(
            shell_name,
            positional,
            force_interactive,
            true,
            &invocation_ops,
        );
    }

    if idx < args.len() {
        // `yosh <sub> ...` delegation only applies to the historical
        // no-option form; with options consumed, the operand is a
        // script path per POSIX §2.1.
        if idx == 1
            && let Some(status) = try_subcommand(&args[1..])
        {
            process::exit(status);
        }
        // POSIX §2.1: with a script file operand, $0 is the script
        // path and the remaining operands are $1, $2, ...
        let positional: Vec<String> = args[idx + 1..].to_vec();
        let status = run_file(
            &args[idx],
            args[idx].clone(),
            positional,
            force_interactive == Some(true),
            &invocation_ops,
        );
        process::exit(status);
    }

    run_stdin(
        shell_name,
        vec![],
        force_interactive,
        false,
        &invocation_ops,
    );
}

/// Invalid invocation: print the error plus a usage line and exit 2.
fn invocation_usage_error(msg: &str) -> ! {
    // Decode byteenc escapes so raw argv bytes round-trip to stderr.
    byteenc::write_stderr_decoded_line(&format!("yosh: {}", msg));
    eprintln!(
        "Usage: yosh [-abCefhimnuvx] [-o option]... [+abCefhimnuvx] [+o option]... \
         [-c command_string | -s | file] [argument...]"
    );
    process::exit(2);
}

/// Read commands from standard input: the interactive REPL on a
/// terminal, otherwise the whole stream as a script (used for the
/// no-operand invocation and for `-s`).
fn run_stdin(
    shell_name: String,
    positional: Vec<String>,
    force_interactive: Option<bool>,
    explicit_s: bool,
    invocation_ops: &[env::InvocationOp],
) -> ! {
    let stdin_tty = nix::unistd::isatty(std::io::stdin()).unwrap_or(false);
    // Interactive iff stdin is a terminal, unless -i forces it on or
    // +i forces it off. The REPL (line editor, prompts) additionally
    // needs a real terminal, so `-i` with a non-tty stdin falls
    // through to the script path with interactive semantics instead.
    if force_interactive.unwrap_or(stdin_tty) && stdin_tty {
        let mut repl = interactive::Repl::new(shell_name, positional, explicit_s, invocation_ops);
        process::exit(repl.run());
    } else {
        // stdin is a pipe, or +i forced the REPL off — read the whole
        // stream as a script (bytes, so non-UTF-8 input is preserved
        // via the byteenc escape encoding). With -i the shell still
        // reads the whole stream but runs with interactive semantics
        // ($- reports i, untrapped TERM/QUIT/INT ignored, shell errors
        // do not exit); the line editor and prompts need a terminal
        // and are not engaged.
        let mut raw = Vec::new();
        io::stdin().read_to_end(&mut raw).unwrap_or_else(|e| {
            eprintln!("yosh: {}", e);
            process::exit(1);
        });
        let input = byteenc::encode_bytes(&raw).into_owned();
        let status = run_string(
            &input,
            shell_name,
            positional,
            (false, explicit_s),
            force_interactive == Some(true),
            invocation_ops,
        );
        process::exit(status);
    }
}

/// Try to delegate `yosh <sub> [args...]` to `yosh-<sub>` binary in PATH.
/// Returns Some(exit_status) if a matching binary was found and executed.
fn try_subcommand(args: &[String]) -> Option<i32> {
    let sub = args.first()?;
    // Skip anything that looks like a flag or a file path.
    if sub.starts_with('-') || sub.contains('/') || sub.contains('.') {
        return None;
    }
    let bin_name = format!("yosh-{}", sub);
    let found = std_env::var_os("PATH")
        .and_then(|paths| std_env::split_paths(&paths).find(|dir| dir.join(&bin_name).is_file()));
    let bin_path = found?.join(&bin_name);
    let status = process::Command::new(bin_path)
        .args(&args[1..])
        .status()
        .unwrap_or_else(|e| {
            eprintln!("yosh: {}: {}", bin_name, e);
            process::exit(126);
        });
    Some(status.code().unwrap_or(1))
}

fn run_string(
    input: &str,
    shell_name: String,
    positional: Vec<String>,
    // `$-` invocation-source letters: `c` (command string), `s`
    // (explicit -s read-stdin); both may be set (`yosh -sc cmd`).
    (cmd_string, stdin_reads): (bool, bool),
    interactive: bool,
    invocation_ops: &[env::InvocationOp],
) -> i32 {
    signal::init_signal_handling();
    let mut executor = Executor::new(shell_name, positional);
    env::default_path::ensure_default_path(&mut executor.env);
    // Invocation-time set options (-e, -x, -o name, ...), validated at
    // parse time in main; applied before any command runs.
    executor
        .env
        .mode
        .options
        .apply_invocation_ops(invocation_ops);
    // Invocation -m: enable job control only when the shell owns its
    // controlling terminal; otherwise drop `m` (and its `$-` letter).
    // The gate and rationale live in signal::try_enable_monitor_mode,
    // shared with the runtime `set -m` builtin transition.
    if executor.env.mode.options.monitor && !signal::try_enable_monitor_mode() {
        executor.env.mode.options.monitor = false;
    }
    executor.load_plugins();
    executor.env.mode.options.cmd_string = cmd_string;
    executor.env.mode.options.stdin_reads = stdin_reads;
    if interactive {
        // POSIX sh -i: the shell is interactive regardless of stdin —
        // $- reports `i`, untrapped TERM/QUIT/INT are ignored, and
        // shell errors return control instead of exiting. Monitor mode
        // and the line editor stay off on this non-terminal path.
        executor.env.mode.is_interactive = true;
        signal::set_interactive_shell(true);
    }

    // Parse and execute one complete command at a time so that aliases
    // defined by earlier commands are available for later ones.
    //
    // `current_line` tracks the 1-based line number at the start of `remaining`
    // so that the parser receives the correct initial line counter even after
    // leading whitespace/newlines are stripped between commands.
    let mut remaining = input;
    let mut current_line: usize = 1;
    let mut status = 0;

    loop {
        // Skip leading whitespace and newlines, counting the newlines so the
        // parser can start with the correct source-file line number.
        let before = remaining;
        let trimmed = remaining.trim_start_matches([' ', '\t', '\n']);
        if trimmed.is_empty() {
            break;
        }
        // Count newlines in the skipped prefix to advance current_line.
        let skipped = &before[..before.len() - trimmed.len()];
        current_line += skipped.chars().filter(|&c| c == '\n').count();
        remaining = trimmed;

        let mut p = parser::Parser::new_with_aliases_at_line(
            remaining,
            &executor.env.aliases,
            current_line,
        );
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
                // Count newlines in the consumed bytes to keep current_line in sync.
                let consumed_text = &remaining[..consumed];
                current_line += consumed_text.chars().filter(|&c| c == '\n').count();
                drop(p);
                // set -v: echo input to stderr as it is read (POSIX §2.15 set).
                executor.verbose_print(consumed_text.trim_end_matches('\n'));
                status = executor.exec_complete_command(&cmd);
                // Check for flow control (exit handled by std::process::exit in builtin)
                if executor.env.exec.flow_control.is_some() {
                    break;
                }
                // POSIX §2.8.1 shell errors request exit via exit_requested.
                if let Some(code) = executor.exit_requested {
                    status = code;
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

fn run_file(
    path: &str,
    shell_name: String,
    positional: Vec<String>,
    interactive: bool,
    invocation_ops: &[env::InvocationOp],
) -> i32 {
    use std::os::unix::ffi::OsStrExt;
    // `path` is byteenc-encoded (it came from args_os); decode it back to
    // raw bytes for the OS call, and read the script as bytes so non-UTF-8
    // source is preserved.
    let os_path = byteenc::decode_bytes(path);
    let content = match fs::read(std::ffi::OsStr::from_bytes(&os_path)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("yosh: {}: {}", path, e);
            return 127;
        }
    };
    let content = byteenc::encode_bytes(&content).into_owned();
    run_string(
        &content,
        shell_name,
        positional,
        (false, false),
        interactive,
        invocation_ops,
    )
}
