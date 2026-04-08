use crate::env::{FlowControl, ShellEnv};
use crate::exec::Executor;

pub fn exec_special_builtin(name: &str, args: &[String], executor: &mut Executor) -> i32 {
    match name {
        ":" => 0,
        "exit" => builtin_exit(args, &executor.env),
        "export" => builtin_export(args, &mut executor.env),
        "unset" => builtin_unset(args, &mut executor.env),
        "readonly" => builtin_readonly(args, &mut executor.env),
        "return" => builtin_return(args, &mut executor.env),
        "break" => builtin_break(args, &mut executor.env),
        "continue" => builtin_continue(args, &mut executor.env),
        "set" => builtin_set(args, &mut executor.env),
        "eval" => builtin_eval(args, executor),
        "exec" => builtin_exec(args, &mut executor.env),
        "trap" => builtin_trap(args, &mut executor.env),
        "." => builtin_source(args, executor),
        "shift" => builtin_shift(args, &mut executor.env),
        "times" => builtin_times(),
        _ => {
            eprintln!("kish: {}: not a special builtin", name);
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Existing implementations (moved from mod.rs)
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
    env.flow_control = Some(FlowControl::Return(code));
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
    env.flow_control = Some(FlowControl::Break(n));
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
    env.flow_control = Some(FlowControl::Continue(n));
    0
}

// ---------------------------------------------------------------------------
// Stub implementations for new builtins (to be implemented in later tasks)
// ---------------------------------------------------------------------------

fn builtin_set(_args: &[String], _env: &mut ShellEnv) -> i32 {
    0
}

fn builtin_eval(_args: &[String], _executor: &mut Executor) -> i32 {
    0
}

fn builtin_exec(_args: &[String], _env: &mut ShellEnv) -> i32 {
    0
}

fn builtin_trap(_args: &[String], _env: &mut ShellEnv) -> i32 {
    0
}

fn builtin_source(_args: &[String], _executor: &mut Executor) -> i32 {
    0
}

fn builtin_shift(_args: &[String], _env: &mut ShellEnv) -> i32 {
    0
}

fn builtin_times() -> i32 {
    0
}
