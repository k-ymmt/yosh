pub mod special;

use crate::env::ShellEnv;

/// Classification of a command name as a POSIX builtin kind.
#[derive(Debug, PartialEq)]
pub enum BuiltinKind {
    /// POSIX special builtin: prefix assignments persist in current env,
    /// errors in assignments are fatal.
    Special,
    /// Regular builtin: prefix assignments are temporary.
    Regular,
    /// Not a builtin: execute as external command.
    NotBuiltin,
}

/// Classify a command name into its builtin kind.
pub fn classify_builtin(name: &str) -> BuiltinKind {
    match name {
        "break" | ":" | "continue" | "." | "eval" | "exec" | "exit" | "export"
        | "readonly" | "return" | "set" | "shift" | "times" | "trap" | "unset" => {
            BuiltinKind::Special
        }
        "cd" | "echo" | "true" | "false" | "alias" | "unalias" | "kill" | "wait" => BuiltinKind::Regular,
        _ => BuiltinKind::NotBuiltin,
    }
}

/// Execute a regular builtin command, returning its exit status.
pub fn exec_regular_builtin(name: &str, args: &[String], env: &mut ShellEnv) -> i32 {
    match name {
        "cd" => builtin_cd(args, env),
        "true" => 0,
        "false" => 1,
        "echo" => builtin_echo(args),
        "alias" => builtin_alias(args, env),
        "unalias" => builtin_unalias(args, env),
        "kill" => builtin_kill(args),
        "wait" => {
            // Handled in Executor::exec_simple_command — should not reach here
            eprintln!("kish: wait: internal error");
            1
        }
        _ => {
            eprintln!("kish: {}: not a regular builtin", name);
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Individual regular builtin implementations
// ---------------------------------------------------------------------------

fn builtin_cd(args: &[String], env: &mut ShellEnv) -> i32 {
    let target = if args.is_empty() {
        match env.vars.get("HOME") {
            Some(h) => h.to_string(),
            None => {
                eprintln!("kish: cd: HOME not set");
                return 1;
            }
        }
    } else if args[0] == "-" {
        match env.vars.get("OLDPWD") {
            Some(old) => {
                let old = old.to_string();
                println!("{}", old);
                old
            }
            None => {
                eprintln!("kish: cd: OLDPWD not set");
                return 1;
            }
        }
    } else {
        args[0].clone()
    };

    // Save current directory as OLDPWD before changing
    if let Ok(old_pwd) = std::env::current_dir() {
        let _ = env.vars.set("OLDPWD", old_pwd.to_string_lossy().to_string());
    }

    match std::env::set_current_dir(&target) {
        Ok(_) => {
            // Update $PWD
            match std::env::current_dir() {
                Ok(cwd) => {
                    let cwd_str = cwd.to_string_lossy().into_owned();
                    let _ = env.vars.set("PWD", cwd_str);
                }
                Err(e) => {
                    eprintln!("kish: cd: could not determine new directory: {}", e);
                }
            }
            0
        }
        Err(e) => {
            eprintln!("kish: cd: {}: {}", target, e);
            1
        }
    }
}

fn builtin_echo(args: &[String]) -> i32 {
    if args.first().map(|a| a.as_str()) == Some("-n") {
        print!("{}", args[1..].join(" "));
    } else {
        println!("{}", args.join(" "));
    }
    0
}

fn builtin_alias(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        for (name, value) in env.aliases.sorted_iter() {
            println!("alias {}='{}'", name, value);
        }
        return 0;
    }
    let mut status = 0;
    for arg in args {
        if let Some(pos) = arg.find('=') {
            let name = &arg[..pos];
            let value = &arg[pos + 1..];
            env.aliases.set(name, value);
        } else {
            match env.aliases.get(arg) {
                Some(value) => println!("alias {}='{}'", arg, value),
                None => {
                    eprintln!("kish: alias: {}: not found", arg);
                    status = 1;
                }
            }
        }
    }
    status
}

fn builtin_unalias(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        eprintln!("kish: unalias: usage: unalias name [name ...]");
        return 2;
    }
    let mut status = 0;
    for arg in args {
        if arg == "-a" {
            env.aliases.clear();
        } else if !env.aliases.remove(arg) {
            eprintln!("kish: unalias: {}: not found", arg);
            status = 1;
        }
    }
    status
}

fn builtin_kill(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("kish: kill: usage: kill [-s sigspec | -signum] pid...");
        return 2;
    }

    if args[0] == "-l" {
        return kill_list(&args[1..]);
    }

    let (sig_num, pid_args) = match parse_kill_signal(args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("kish: kill: {}", msg);
            return 2;
        }
    };

    let mut status = 0;
    for pid_str in pid_args {
        let pid: i32 = match pid_str.parse() {
            Ok(n) => n,
            Err(_) => {
                eprintln!("kish: kill: {}: invalid pid", pid_str);
                status = 1;
                continue;
            }
        };
        if let Err(e) = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::try_from(sig_num).ok(),
        ) {
            eprintln!("kish: kill: ({}) - {}", pid_str, e);
            status = 1;
        }
    }
    status
}

fn parse_kill_signal(args: &[String]) -> Result<(i32, &[String]), String> {
    if args[0] == "-s" {
        if args.len() < 3 {
            return Err("option requires an argument -- s".to_string());
        }
        let sig = crate::signal::signal_name_to_number(&args[1])?;
        Ok((sig, &args[2..]))
    } else if args[0] == "--" {
        Ok((libc::SIGTERM, &args[1..]))
    } else if args[0].starts_with('-') && args[0].len() > 1 {
        let spec = &args[0][1..];
        if let Ok(num) = spec.parse::<i32>() {
            Ok((num, &args[1..]))
        } else {
            let sig = crate::signal::signal_name_to_number(spec)?;
            Ok((sig, &args[1..]))
        }
    } else {
        Ok((libc::SIGTERM, args))
    }
}

fn kill_list(args: &[String]) -> i32 {
    if args.is_empty() {
        let names: Vec<&str> = crate::signal::SIGNAL_TABLE
            .iter()
            .map(|&(_, name)| name)
            .collect();
        println!("{}", names.join(" "));
        return 0;
    }
    for arg in args {
        if let Ok(num) = arg.parse::<i32>() {
            let sig = if num > 128 { num - 128 } else { num };
            match crate::signal::signal_number_to_name(sig) {
                Some(name) => println!("{}", name),
                None => {
                    eprintln!("kish: kill: {}: invalid signal number", arg);
                    return 1;
                }
            }
        } else {
            match crate::signal::signal_name_to_number(arg) {
                Ok(num) => println!("{}", num),
                Err(e) => {
                    eprintln!("kish: kill: {}", e);
                    return 1;
                }
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;

    fn make_env() -> ShellEnv {
        ShellEnv::new("kish", vec![])
    }

    #[test]
    fn test_classify_builtin() {
        assert!(matches!(classify_builtin(":"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("break"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("continue"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("return"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("exit"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("export"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("readonly"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("unset"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("set"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("eval"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("exec"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("trap"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("."), BuiltinKind::Special));
        assert!(matches!(classify_builtin("shift"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("times"), BuiltinKind::Special));
        assert!(matches!(classify_builtin("cd"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("echo"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("true"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("false"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("alias"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("unalias"), BuiltinKind::Regular));
        assert!(matches!(classify_builtin("ls"), BuiltinKind::NotBuiltin));
    }

    #[test]
    fn test_true_false() {
        let mut env = make_env();
        assert_eq!(exec_regular_builtin("true", &[], &mut env), 0);
        assert_eq!(exec_regular_builtin("false", &[], &mut env), 1);
    }

    #[test]
    fn test_alias_unalias() {
        let mut env = make_env();
        let args = vec!["ll=ls -l".to_string()];
        assert_eq!(exec_regular_builtin("alias", &args, &mut env), 0);
        assert_eq!(env.aliases.get("ll"), Some("ls -l"));
        let args = vec!["ll".to_string()];
        assert_eq!(exec_regular_builtin("unalias", &args, &mut env), 0);
        assert_eq!(env.aliases.get("ll"), None);
    }

    #[test]
    fn test_unalias_all() {
        let mut env = make_env();
        env.aliases.set("ll", "ls -l");
        env.aliases.set("la", "ls -a");
        let args = vec!["-a".to_string()];
        assert_eq!(exec_regular_builtin("unalias", &args, &mut env), 0);
        assert!(env.aliases.is_empty());
    }

    #[test]
    fn test_echo_dash_n() {
        // -n flag should suppress trailing newline.
        // We can't easily capture stdout in unit tests, so verify
        // the function returns 0 (behavior tested via E2E).
        let args = vec!["-n".to_string(), "hello".to_string()];
        assert_eq!(builtin_echo(&args), 0);
    }
}
