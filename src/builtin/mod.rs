use crate::env::ShellEnv;

/// Returns true if `name` is a builtin command.
pub fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "exit" | "cd" | "export" | "unset" | "readonly" | "true" | "false" | ":" | "echo"
            | "return" | "break" | "continue"
    )
}

/// Execute a builtin command, returning its exit status.
pub fn exec_builtin(name: &str, args: &[String], env: &mut ShellEnv) -> i32 {
    match name {
        "exit" => builtin_exit(args, env),
        "cd" => builtin_cd(args, env),
        "export" => builtin_export(args, env),
        "unset" => builtin_unset(args, env),
        "readonly" => builtin_readonly(args, env),
        "true" | ":" => 0,
        "false" => 1,
        "echo" => builtin_echo(args),
        "return" => builtin_return(args, env),
        "break" => builtin_break(args, env),
        "continue" => builtin_continue(args, env),
        _ => {
            eprintln!("kish: {}: not a builtin", name);
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Individual builtin implementations
// ---------------------------------------------------------------------------

fn builtin_exit(args: &[String], env: &ShellEnv) -> i32 {
    let code = if args.is_empty() {
        env.last_exit_status
    } else {
        match args[0].parse::<i32>() {
            Ok(n) => n & 0xFF,
            Err(_) => {
                eprintln!("kish: exit: {}: numeric argument required", args[0]);
                2
            }
        }
    };
    std::process::exit(code);
}

fn builtin_cd(args: &[String], env: &mut ShellEnv) -> i32 {
    let target = if args.is_empty() {
        match env.vars.get("HOME") {
            Some(h) => h.to_string(),
            None => {
                eprintln!("kish: cd: HOME not set");
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

fn builtin_export(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        // Print all exported variables in the form: export NAME=VALUE
        let mut exported: Vec<(String, String)> = env.vars.to_environ();
        exported.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in exported {
            println!("export {}={}", name, value);
        }
        return 0;
    }

    let mut status = 0;
    for arg in args {
        if let Some(pos) = arg.find('=') {
            let name = &arg[..pos];
            let value = &arg[pos + 1..];
            if let Err(e) = env.vars.set(name, value) {
                eprintln!("kish: export: {}", e);
                status = 1;
                continue;
            }
            env.vars.export(name);
        } else {
            // Just mark as exported (or create empty exported var)
            env.vars.export(arg);
        }
    }
    status
}

fn builtin_unset(args: &[String], env: &mut ShellEnv) -> i32 {
    let mut status = 0;
    for name in args {
        if let Err(e) = env.vars.unset(name) {
            eprintln!("kish: unset: {}", e);
            status = 1;
        }
    }
    status
}

fn builtin_readonly(args: &[String], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        // Print all readonly variables
        // Collect names first to avoid borrowing issues
        let readonly_vars: Vec<(String, String)> = env
            .vars
            .vars_iter()
            .filter(|(_, v)| v.readonly)
            .map(|(k, v)| (k.clone(), v.value.clone()))
            .collect();
        let mut sorted = readonly_vars;
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in sorted {
            println!("readonly {}={}", name, value);
        }
        return 0;
    }

    let mut status = 0;
    for arg in args {
        if let Some(pos) = arg.find('=') {
            let name = &arg[..pos];
            let value = &arg[pos + 1..];
            if let Err(e) = env.vars.set(name, value) {
                eprintln!("kish: readonly: {}", e);
                status = 1;
                continue;
            }
            env.vars.set_readonly(name);
        } else {
            env.vars.set_readonly(arg);
        }
    }
    status
}

fn builtin_echo(args: &[String]) -> i32 {
    println!("{}", args.join(" "));
    0
}

fn builtin_return(args: &[String], env: &mut ShellEnv) -> i32 {
    let code = if args.is_empty() {
        env.last_exit_status & 0xFF
    } else {
        match args[0].parse::<i32>() {
            Ok(n) => n & 0xFF,
            Err(_) => {
                eprintln!("kish: return: {}: numeric argument required", args[0]);
                2
            }
        }
    };
    env.flow_control = Some(crate::env::FlowControl::Return(code));
    code
}

fn builtin_break(args: &[String], env: &mut ShellEnv) -> i32 {
    let n = if args.is_empty() {
        1
    } else {
        match args[0].parse::<usize>() {
            Ok(0) => {
                eprintln!("kish: break: loop count must be > 0");
                return 1;
            }
            Ok(n) => n,
            Err(_) => {
                eprintln!("kish: break: {}: numeric argument required", args[0]);
                return 1;
            }
        }
    };
    env.flow_control = Some(crate::env::FlowControl::Break(n));
    0
}

fn builtin_continue(args: &[String], env: &mut ShellEnv) -> i32 {
    let n = if args.is_empty() {
        1
    } else {
        match args[0].parse::<usize>() {
            Ok(0) => {
                eprintln!("kish: continue: loop count must be > 0");
                return 1;
            }
            Ok(n) => n,
            Err(_) => {
                eprintln!("kish: continue: {}: numeric argument required", args[0]);
                return 1;
            }
        }
    };
    env.flow_control = Some(crate::env::FlowControl::Continue(n));
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
    fn test_is_builtin() {
        assert!(is_builtin("exit"));
        assert!(is_builtin("cd"));
        assert!(is_builtin("export"));
        assert!(is_builtin("unset"));
        assert!(is_builtin("readonly"));
        assert!(is_builtin("true"));
        assert!(is_builtin("false"));
        assert!(is_builtin(":"));
        assert!(is_builtin("echo"));
        assert!(is_builtin("return"));
        assert!(!is_builtin("ls"));
        assert!(!is_builtin("grep"));
        assert!(!is_builtin(""));
    }

    #[test]
    fn test_true_false_colon() {
        let mut env = make_env();
        assert_eq!(exec_builtin("true", &[], &mut env), 0);
        assert_eq!(exec_builtin(":", &[], &mut env), 0);
        assert_eq!(exec_builtin("false", &[], &mut env), 1);
    }

    #[test]
    fn test_export_and_unset() {
        let mut env = make_env();

        // export with name=value should set and export the variable
        let args = vec!["MY_VAR=hello".to_string()];
        let status = exec_builtin("export", &args, &mut env);
        assert_eq!(status, 0);
        assert_eq!(env.vars.get("MY_VAR"), Some("hello"));
        assert!(env.vars.get_var("MY_VAR").unwrap().exported);

        // unset should remove the variable
        let args = vec!["MY_VAR".to_string()];
        let status = exec_builtin("unset", &args, &mut env);
        assert_eq!(status, 0);
        assert_eq!(env.vars.get("MY_VAR"), None);
    }

    #[test]
    fn test_cd_to_tmp() {
        let mut env = make_env();
        let args = vec!["/tmp".to_string()];
        let status = exec_builtin("cd", &args, &mut env);
        assert_eq!(status, 0);
        // $PWD should be updated (may resolve symlinks)
        let pwd = env.vars.get("PWD").unwrap_or("");
        assert!(!pwd.is_empty(), "PWD should be set after cd");
    }

    #[test]
    fn test_cd_nonexistent() {
        let mut env = make_env();
        let args = vec!["/this/path/does/not/exist/xyz123".to_string()];
        let status = exec_builtin("cd", &args, &mut env);
        assert_ne!(status, 0);
    }
}
